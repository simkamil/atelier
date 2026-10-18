// SPDX-License-Identifier: AGPL-3.0-or-later
//! Search, replace, statistics and document properties of the rich editor
//! session (R7b-2). Layout of the messages: `rich_protocol/find.rs`; rules in
//! `docs/us-340-search.md` ("Rich editor protocol").
//!
//! The search runs on the flat paragraphs of the model (items as U+FFFC, so
//! a match never spans an object, a break or a symbol, and every position is a
//! flat byte position). Headers and footers are not in the flat text and are
//! not searched.

use std::borrow::Cow;

use atelier_fmt_ooxml::docx_image::write_new_docx_with_pictures;
use atelier_fmt_ooxml::{
    DocxWriteOptions, EditableProperties, NewDocument, Package, PropertyChanges, PropertyDate,
    PropertyError,
};
use atelier_text_model::rich::flat::{
    EditOutcome, FlatEdit, FlatIndex, LayoutContext, flat_to_text_offset,
};
use atelier_text_model::rich::{RichDocument, default_rich_document};
use atelier_text_model::search::{
    Occurrence, ParagraphSource, SearchOptions, SearchParagraph, Searcher, Zone,
};
use atelier_text_model::statistics::{
    CountingRules, Counts, StatisticsOptions, count_paragraph, document,
};

use super::Session;
use super::io::Source;
use crate::properties::{FIELDS, LIMITS};
use crate::rich_layout::{PageDelta, RichPageCache};
use crate::rich_protocol::{
    DEFAULT_LAYOUT_BUDGET, ErrorCode, HighlightRect, MAX_EXCERPT_BYTES, MAX_HIGHLIGHT_RECTS,
    MAX_REPLACE_MATCHES, MAX_SEARCH_ITEMS, PROPERTY_COUNT, PropertiesOutcome, PropertiesSnapshot,
    PropertiesStatus, Response, SEARCH_COUNT_CAP, SearchExcerpt, SearchFlags, SearchItem,
    StatCounts,
};

/// Graphemes of context kept on each side of an occurrence (as in the plain editor).
const CONTEXT_GRAPHEMES: usize = 30;
/// Paragraphs of layout `Statistics` may run before it answers (a short document
/// is complete then; the layout of a long one goes on in the background, and
/// the reply says whether its line and page counts are final).
const STATISTICS_LAYOUT_BUDGET: u32 = 32;
/// Bytes of paragraph text a `ReplaceAll` may rewrite: each replacement clones
/// its paragraph twice into the undo history.
const MAX_REPLACE_WORK_BYTES: usize = 64 * 1024 * 1024;

/// The flat paragraphs of a document, in order, as a search source.
struct FlatParagraphs<'a> {
    starts: Vec<usize>,
    texts: Vec<Cow<'a, str>>,
}

impl ParagraphSource for FlatParagraphs<'_> {
    type Id = usize;

    fn paragraphs(&self) -> Vec<SearchParagraph<'_, usize>> {
        self.texts
            .iter()
            .enumerate()
            .map(|(id, text)| SearchParagraph {
                id,
                zone: Zone::Body,
                section: 0,
                text,
            })
            .collect()
    }
}

fn flat_paragraphs(doc: &RichDocument) -> Result<FlatParagraphs<'_>, ErrorCode> {
    let count = doc.paragraph_count();
    let mut starts = Vec::new();
    let mut texts = Vec::new();
    starts
        .try_reserve_exact(count)
        .map_err(|_| ErrorCode::Limit)?;
    texts
        .try_reserve_exact(count)
        .map_err(|_| ErrorCode::Limit)?;
    for index in 0..count {
        starts.push(LayoutContext::paragraph_start(doc, index)?);
        texts.push(LayoutContext::paragraph_flat(doc, index)?);
    }
    Ok(FlatParagraphs { starts, texts })
}

fn options(flags: SearchFlags, context_graphemes: usize) -> SearchOptions {
    SearchOptions {
        match_case: flags.match_case,
        whole_word: flags.whole_word,
        ignore_accents: flags.ignore_accents,
        match_nbsp: flags.match_nbsp,
        context_graphemes,
    }
}

/// The excerpt of an occurrence, or its match alone (clipped) when the
/// context would not fit the wire bound.
fn excerpt_of(paragraph: &str, occurrence: &Occurrence<usize>) -> SearchExcerpt {
    if let Some(e) = &occurrence.excerpt {
        if e.text.len() <= MAX_EXCERPT_BYTES && e.start < e.end && e.end <= e.text.len() {
            return SearchExcerpt {
                text: e.text.clone(),
                start: e.start as u32,
                end: e.end as u32,
                truncated_before: e.truncated_before,
                truncated_after: e.truncated_after,
            };
        }
    }
    let matched = paragraph.get(occurrence.range.clone()).unwrap_or("");
    let mut cut = matched.len().min(MAX_EXCERPT_BYTES);
    while !matched.is_char_boundary(cut) {
        cut -= 1;
    }
    SearchExcerpt {
        text: matched[..cut].to_owned(),
        start: 0,
        end: cut as u32,
        truncated_before: occurrence.range.start > 0,
        truncated_after: cut < matched.len() || occurrence.range.end < paragraph.len(),
    }
}

/// Counts of the flat range `lo..hi`: the characters of each paragraph it
/// holds (objects and breaks have none), a paragraph counted as a whole
/// follows the paragraph rule, a part of one counts as a paragraph when it
/// holds text (the rule of `statistics::selection`).
fn selection_counts(
    doc: &RichDocument,
    lo: usize,
    hi: usize,
    rules: &CountingRules,
) -> Result<Counts, ErrorCode> {
    let mut total = Counts::default();
    if lo >= hi {
        return Ok(total);
    }
    let first = doc.locate(lo)?.paragraph;
    let last = doc.locate(hi)?.paragraph;
    for index in first..=last {
        let Some(paragraph) = LayoutContext::paragraph(doc, index) else {
            continue; // an opaque block holds no text
        };
        let start = LayoutContext::paragraph_start(doc, index)?;
        let flat_len = LayoutContext::paragraph_flat(doc, index)?.len();
        let from = lo.saturating_sub(start).min(flat_len);
        let to = hi.saturating_sub(start).min(flat_len);
        if from >= to {
            continue;
        }
        let s = flat_to_text_offset(paragraph, from)?;
        let e = flat_to_text_offset(paragraph, to)?;
        if s >= e {
            continue;
        }
        let mut counts = count_paragraph(&paragraph.text[s..e], rules);
        if !(s == 0 && e == paragraph.text.len()) {
            counts.paragraphs = 1;
        }
        total += counts;
    }
    Ok(total)
}

fn clamp(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Properties of a document that has no DOCX package (a new document, a text
/// file): the values live in a package made from the default profile, and are
/// copied into the package of the file when it is saved as DOCX.
#[derive(Default)]
pub(super) struct Props {
    package: Option<Box<Package>>,
    /// Bumped by every change of a stored value.
    version: u32,
    /// `version` when the document was last saved or opened.
    clean: u32,
    /// `version` when `Save` last serialised the document.
    saved: u32,
}

impl Props {
    /// A property changed since the last save the client confirmed.
    pub(super) fn dirty(&self) -> bool {
        self.version != self.clean
    }

    /// `Save` serialised the document; a text file does not hold the
    /// properties, so only a save in a DOCX format takes them.
    pub(super) fn note_save(&mut self, keeps_properties: bool) {
        if keeps_properties {
            self.saved = self.version;
        }
    }

    /// `MarkSaved`: the file on disk holds what the last `Save` serialised.
    pub(super) fn mark_saved(&mut self) {
        self.clean = self.saved;
    }

    /// Writes the properties into the DOCX just serialised from the model.
    pub(super) fn rewrite(&self, bytes: Vec<u8>) -> Result<Vec<u8>, ErrorCode> {
        let Some(source) = &self.package else {
            return Ok(bytes);
        };
        let changes = PropertyChanges::between(
            &EditableProperties::default(),
            &source.properties_snapshot().editable,
        );
        let mut target = Package::read(&bytes).map_err(|_| ErrorCode::SaveFailed)?;
        target
            .apply_property_changes(&changes)
            .map_err(|_| ErrorCode::SaveFailed)?;
        target.write().map_err(|_| ErrorCode::SaveFailed)
    }

    fn package(&mut self) -> Result<&mut Package, ErrorCode> {
        if self.package.is_none() {
            let defaults = default_rich_document();
            let written = write_new_docx_with_pictures(
                &NewDocument::from_default(&defaults),
                &DocxWriteOptions::default(),
            )
            .map_err(|_| ErrorCode::PropertiesFailed)?;
            let package = Package::read(&written.bytes).map_err(|_| ErrorCode::PropertiesFailed)?;
            self.package = Some(Box::new(package));
        }
        self.package
            .as_deref_mut()
            .ok_or(ErrorCode::PropertiesFailed)
    }
}

impl Session {
    fn check_revision(&self, revision: u64) -> Result<(), ErrorCode> {
        if revision == self.revision() {
            Ok(())
        } else {
            Err(ErrorCode::StaleRevision)
        }
    }

    /// `Search`: every occurrence (at most `SEARCH_COUNT_CAP` counted, the
    /// first `MAX_SEARCH_ITEMS` returned) with its excerpt.
    pub(super) fn search(
        &self,
        revision: u64,
        flags: SearchFlags,
        query: &str,
    ) -> Result<Response, ErrorCode> {
        self.check_revision(revision)?;
        let source = flat_paragraphs(&self.doc)?;
        let searcher = Searcher::new(&source, query, options(flags, CONTEXT_GRAPHEMES));
        let found = searcher.find_all(SEARCH_COUNT_CAP);
        let total = found.items.len();
        let mut items = Vec::with_capacity(total.min(MAX_SEARCH_ITEMS));
        for occurrence in found.items.iter().take(MAX_SEARCH_ITEMS) {
            let base = source.starts[occurrence.paragraph_index];
            items.push(SearchItem {
                start: (base + occurrence.range.start) as u64,
                end: (base + occurrence.range.end) as u64,
                excerpt: excerpt_of(&source.texts[occurrence.paragraph_index], occurrence),
            });
        }
        Ok(Response::SearchResult {
            revision,
            total: clamp(total),
            truncated: found.truncated || total > items.len(),
            total_capped: found.truncated,
            items,
        })
    }

    /// Lays the document out further (within a budget), toward final line and
    /// page counts.
    fn complete_layout(&mut self) {
        let mut spent = 0;
        while !self.cache.layout_complete() && spent < STATISTICS_LAYOUT_BUDGET {
            let more = self.cache.step(&self.doc, 16);
            self.carry(more);
            spent += 16;
        }
    }

    /// `Statistics`: counts of the document (body and table cells) and of the
    /// selection.
    pub(super) fn statistics(&mut self) -> Result<Response, ErrorCode> {
        self.complete_layout();
        let complete = self.cache.layout_complete();
        let lines = self.cache.line_count();
        let pages = self.cache.page_count() as usize;
        let stats_options = StatisticsOptions {
            lines: Some(lines),
            pages: Some(pages),
            ..StatisticsOptions::default()
        };
        // The text counts only change with the document: kept per revision.
        let key = (
            self.doc.revision(),
            self.doc.len(),
            self.doc.paragraph_count(),
        );
        let mut whole = match self.stats_memo {
            Some((k, memo)) if k == key => memo,
            _ => {
                let fresh = document(&self.doc, &StatisticsOptions::default());
                self.stats_memo = Some((key, fresh));
                fresh
            }
        };
        whole.lines = stats_options.lines;
        whole.pages = stats_options.pages;
        let (lo, hi) = self.selection_range();
        let picked = selection_counts(&self.doc, lo, hi, &stats_options.rules)?;
        let (picked_lines, picked_pages) = if lo == hi {
            (0, 0)
        } else {
            self.cache.lines_in_range(lo, hi).unwrap_or((0, 0))
        };
        Ok(Response::Statistics {
            revision: self.revision(),
            anchor: self.anchor as u64,
            focus: self.focus as u64,
            layout_complete: complete,
            document: StatCounts {
                words: clamp(whole.words),
                characters: clamp(whole.characters),
                characters_with_spaces: clamp(whole.characters_with_spaces),
                paragraphs: clamp(whole.paragraphs),
                lines: clamp(whole.lines.unwrap_or(0)),
                pages: clamp(whole.pages.unwrap_or(0)),
            },
            selection: StatCounts {
                words: clamp(picked.words),
                characters: clamp(picked.characters),
                characters_with_spaces: clamp(picked.characters_with_spaces),
                paragraphs: clamp(picked.paragraphs),
                lines: clamp(picked_lines),
                pages: clamp(picked_pages),
            },
        })
    }

    /// `SearchRects`: highlight rectangles of the given ranges on the pages
    /// asked for (the visible ones). Pages or ranges the layout does not
    /// reach yet give nothing.
    pub(super) fn search_rects(
        &mut self,
        revision: u64,
        first_page: u32,
        page_count: u32,
        ranges: &[(u64, u64)],
    ) -> Result<Response, ErrorCode> {
        self.check_revision(revision)?;
        let pages = self.cache.page_count();
        if first_page >= pages {
            return Err(ErrorCode::PageRange);
        }
        let mut flat = Vec::with_capacity(ranges.len());
        for &(start, end) in ranges {
            let (start, end) = (
                crate::rich_protocol::to_position(start)?,
                crate::rich_protocol::to_position(end)?,
            );
            self.doc.check_position(start)?;
            self.doc.check_position(end)?;
            flat.push((start, end));
        }
        let mut rects: Vec<HighlightRect> = Vec::new();
        for page in first_page..first_page.saturating_add(page_count).min(pages) {
            let room = MAX_HIGHLIGHT_RECTS - rects.len();
            if room == 0 {
                break;
            }
            let Ok(found) = self.cache.range_rects(page, &flat, room) else {
                continue;
            };
            rects.extend(found.into_iter().map(|(item, r)| HighlightRect {
                item: item as u32,
                page: r.page,
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
            }));
        }
        rects.sort_by_key(|r| (r.item, r.page));
        Ok(Response::SearchRects {
            revision: self.revision(),
            rects,
        })
    }

    /// What an edit that went through the model changes in the session.
    fn after_replace(
        &mut self,
        outcome: Option<EditOutcome>,
        count: usize,
    ) -> Result<Response, ErrorCode> {
        let mut delta = PageDelta::default();
        if let Some(outcome) = outcome {
            delta = self.cache.apply_edit(&self.doc, &outcome);
            let (anchor, focus) = outcome.selection;
            self.select(self.snap(anchor), self.snap(focus), false, None);
        }
        match self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)? {
            Response::State(state) => Ok(Response::Replaced {
                count: clamp(count),
                state,
            }),
            _ => Err(ErrorCode::Internal),
        }
    }

    /// `Replace`: the occurrence `start..end`, which must be the selection.
    pub(super) fn replace_current(
        &mut self,
        revision: u64,
        flags: SearchFlags,
        range: (u64, u64),
        query: &str,
        replacement: &str,
    ) -> Result<Response, ErrorCode> {
        self.deny_read_only()?;
        self.check_revision(revision)?;
        let (start, end) = (
            crate::rich_protocol::to_position(range.0)?,
            crate::rich_protocol::to_position(range.1)?,
        );
        if self.selection_range() != (start, end) || start == end {
            return Err(ErrorCode::StaleMatch);
        }
        self.doc.check_position(start)?;
        self.doc.check_position(end)?;
        // Only the paragraph that holds the range is searched.
        let at = self.doc.locate(start)?;
        let base = LayoutContext::paragraph_start(&self.doc, at.paragraph)?;
        let text = LayoutContext::paragraph_flat(&self.doc, at.paragraph)?;
        let source = FlatParagraphs {
            starts: vec![base],
            texts: vec![Cow::Borrowed(&*text)],
        };
        let searcher = Searcher::new(&source, query, options(flags, 0));
        let wanted = at.offset..at.offset + (end - start);
        let same = searcher.iter().any(|o| o.range == wanted);
        if !same {
            return Err(ErrorCode::StaleMatch);
        }
        let unchanged = text.get(wanted) == Some(replacement);
        drop(searcher);
        drop(source);
        drop(text);
        if unchanged {
            return self.after_replace(None, 1);
        }
        let outcome = self.doc.replace_range(start, end, replacement)?;
        self.after_replace(Some(outcome), 1)
    }

    /// `ReplaceAll`: every occurrence, in one undo step.
    pub(super) fn replace_all(
        &mut self,
        revision: u64,
        flags: SearchFlags,
        query: &str,
        replacement: &str,
    ) -> Result<Response, ErrorCode> {
        self.deny_read_only()?;
        self.check_revision(revision)?;
        let (count, edits) = {
            let source = flat_paragraphs(&self.doc)?;
            let searcher = Searcher::new(&source, query, options(flags, 0));
            let mut count = 0usize;
            let mut work = 0usize;
            let mut edits: Vec<(usize, usize)> = Vec::new();
            for occurrence in searcher.iter() {
                count += 1;
                if count > MAX_REPLACE_MATCHES {
                    return Err(ErrorCode::TooManyMatches);
                }
                let text = &source.texts[occurrence.paragraph_index];
                work = work.saturating_add(text.len());
                if work > MAX_REPLACE_WORK_BYTES {
                    return Err(ErrorCode::Limit);
                }
                if text.get(occurrence.range.clone()) == Some(replacement) {
                    continue; // already what it should be
                }
                let base = source.starts[occurrence.paragraph_index];
                edits.push((base + occurrence.range.start, base + occurrence.range.end));
            }
            (count, edits)
        };
        if edits.is_empty() {
            return self.after_replace(None, count);
        }
        let ranges: Vec<(usize, usize, &str)> = edits
            .iter()
            .map(|&(start, end)| (start, end, replacement))
            .collect();
        let outcome = self.doc.replace_ranges(&ranges)?;
        self.after_replace(Some(outcome), count)
    }

    /// Live counts for the read-only fields of the properties: the stored
    /// ones (written at the last save) are stale as soon as the text changes.
    fn live_statistics(&mut self) -> [Option<u64>; 6] {
        let whole = document(&self.doc, &StatisticsOptions::default());
        let complete = self.cache.layout_complete();
        let pages = complete.then(|| u64::from(self.cache.page_count()));
        let lines = complete.then(|| self.cache.line_count() as u64);
        [
            pages,
            Some(whole.words as u64),
            Some(whole.characters as u64),
            Some(whole.characters_with_spaces as u64),
            lines,
            Some(whole.paragraphs as u64),
        ]
    }

    fn properties_package(&mut self) -> Result<&mut Package, ErrorCode> {
        match &mut self.origin.source {
            Source::Docx(imported) => Ok(&mut imported.package),
            _ => self.origin.props.package(),
        }
    }

    fn properties_snapshot(&mut self) -> Result<PropertiesSnapshot, ErrorCode> {
        let live = self.live_statistics();
        let package = self.properties_package()?;
        let snapshot = package.properties_snapshot();
        let (e, r) = (&snapshot.editable, &snapshot.read_only);
        let date = |d: &Option<PropertyDate>| {
            d.as_ref().map(|d| match d {
                PropertyDate::Valid(v) => (true, v.to_w3cdtf()),
                PropertyDate::Invalid(text) => (false, text.clone()),
            })
        };
        let mut statistics = [
            r.statistics.pages,
            r.statistics.words,
            r.statistics.characters,
            r.statistics.characters_with_spaces,
            r.statistics.lines,
            r.statistics.paragraphs,
        ];
        // Pages and lines stay as stored until the layout is complete.
        for (slot, value) in statistics.iter_mut().zip(live) {
            if value.is_some() {
                *slot = value;
            }
        }
        let mut limits = [0u32; PROPERTY_COUNT];
        for (slot, limit) in limits.iter_mut().zip(LIMITS) {
            *slot = clamp(limit);
        }
        Ok(PropertiesSnapshot {
            editable: [
                e.title.clone(),
                e.subject.clone(),
                e.author.clone(),
                e.manager.clone(),
                e.company.clone(),
                e.category.clone(),
                e.keywords.clone(),
                e.comments.clone(),
                e.hyperlink_base.clone(),
                e.content_status.clone(),
                e.language.clone(),
            ],
            limits,
            dates: [date(&r.created), date(&r.modified), date(&r.last_printed)],
            strings: [
                r.last_modified_by.clone(),
                r.revision.clone(),
                r.application.clone(),
                r.app_version.clone(),
                r.template.clone(),
            ],
            total_time: r.total_time,
            statistics,
            notices: clamp(snapshot.notices.len()),
        })
    }

    /// `PropertiesRead`.
    pub(super) fn properties_read(&mut self) -> Result<Response, ErrorCode> {
        Ok(Response::Properties {
            revision: self.revision(),
            snapshot: Box::new(self.properties_snapshot()?),
        })
    }

    /// `PropertiesApply`: the values are checked first and nothing is applied
    /// when one is too long.
    pub(super) fn properties_apply(
        &mut self,
        changes: [Option<String>; PROPERTY_COUNT],
    ) -> Result<Response, ErrorCode> {
        self.deny_read_only()?;
        let [
            title,
            subject,
            author,
            manager,
            company,
            category,
            keywords,
            comments,
            hyperlink_base,
            content_status,
            language,
        ] = changes;
        let changes = PropertyChanges {
            title,
            subject,
            author,
            manager,
            company,
            category,
            keywords,
            comments,
            hyperlink_base,
            content_status,
            language,
        };
        let result = self.properties_package()?.apply_property_changes(&changes);
        let (status, changed, field, max, len) = match result {
            Ok(report) => {
                if !report.changed.is_empty() {
                    self.origin.props.version = self.origin.props.version.wrapping_add(1);
                }
                (
                    PropertiesStatus::Applied,
                    report.changed.len() as u16,
                    0,
                    0,
                    0,
                )
            }
            Err(PropertyError::TooLong { field, max, len }) => (
                PropertiesStatus::TooLong,
                0,
                FIELDS.iter().position(|f| *f == field).unwrap_or(0) as u8,
                clamp(max),
                clamp(len),
            ),
        };
        let state = self.state(PageDelta::default(), false)?;
        Ok(Response::PropertiesApplied(PropertiesOutcome {
            status,
            changed,
            field,
            max,
            len,
            state,
        }))
    }
}
