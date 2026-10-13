// SPDX-License-Identifier: AGPL-3.0-or-later
//! Text search (US-340): options, occurrences with exact ranges, context
//! excerpts and next/previous navigation, over the rich document or plain text.
//!
//! The algorithm lives in one place ([`Searcher`]) and works on any
//! [`ParagraphSource`]; `RichDocument` (ids are `NodeId`), `PlainTextDocument`
//! and [`TextLines`] (ids are [`LineId`]) implement it. See
//! `docs/us-340-search.md` for the exact semantics.

mod matcher;
mod source;

use core::fmt::Debug;
use core::ops::Range;
use matcher::{Analysis, Flags, Matcher};

pub use source::{LineId, TextLines};

/// A cap for callers that have no better value for [`Searcher::find_all`].
pub const DEFAULT_MAX_RESULTS: usize = 10_000;

/// Search options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchOptions {
    /// Compare case-sensitively. Otherwise full Unicode case folding is used.
    pub match_case: bool,
    /// Both ends of an occurrence must be UAX #29 word boundaries.
    pub whole_word: bool,
    /// Compare without combining marks (after decomposition).
    pub ignore_accents: bool,
    /// Distinguish U+00A0 and U+202F from the plain space.
    pub match_nbsp: bool,
    /// Graphemes kept before and after an occurrence in its excerpt; `0`
    /// disables excerpts.
    pub context_graphemes: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            match_case: false,
            whole_word: false,
            ignore_accents: false,
            match_nbsp: false,
            context_graphemes: 30,
        }
    }
}

/// Where a paragraph lives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Zone {
    Body,
    /// A table cell of the body, at any nesting depth.
    Table,
    Header,
    Footer,
}

/// A paragraph handed to the search: identifier, zone, section index, text.
#[derive(Clone, Copy, Debug)]
pub struct SearchParagraph<'a, I> {
    pub id: I,
    pub zone: Zone,
    pub section: usize,
    pub text: &'a str,
}

/// Anything that can list its paragraphs in reading order.
pub trait ParagraphSource {
    type Id: Copy + Eq + Debug;
    fn paragraphs(&self) -> Vec<SearchParagraph<'_, Self::Id>>;
}

/// A caret: paragraph and byte offset in that paragraph's text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Position<I> {
    pub paragraph: I,
    pub offset: usize,
}

/// Text around an occurrence, bounded to its paragraph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Excerpt {
    pub text: String,
    /// The occurrence inside `text`, in bytes.
    pub start: usize,
    pub end: usize,
    /// The paragraph continues before or after the excerpt.
    pub truncated_before: bool,
    pub truncated_after: bool,
}

/// One occurrence: a range of bytes of the original paragraph text, on
/// grapheme boundaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Occurrence<I> {
    pub paragraph: I,
    /// Index of the paragraph in reading order.
    pub paragraph_index: usize,
    pub section: usize,
    pub zone: Zone,
    pub range: Range<usize>,
    pub excerpt: Option<Excerpt>,
}

impl Occurrence<LineId> {
    /// The range in bytes of the whole plain text.
    pub fn global_range(&self) -> Range<usize> {
        self.paragraph.0 + self.range.start..self.paragraph.0 + self.range.end
    }
}

/// The result of a next/previous request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Step<I> {
    pub occurrence: Occurrence<I>,
    /// The search went past the end (or start) of the document and wrapped.
    pub wrapped: bool,
}

/// All occurrences, up to a cap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResults<I> {
    pub items: Vec<Occurrence<I>>,
    /// More occurrences exist than the cap allowed.
    pub truncated: bool,
}

impl<I> SearchResults<I> {
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// Zero-based index of `occurrence` in the list ("3 of 17" is `index + 1`).
    pub fn index_of(&self, occurrence: &Occurrence<I>) -> Option<usize> {
        let key = |o: &Occurrence<I>| (o.paragraph_index, o.range.start);
        self.items.binary_search_by_key(&key(occurrence), key).ok()
    }
}

/// A compiled query over a snapshot of the paragraphs of a document.
pub struct Searcher<'a, I> {
    paragraphs: Vec<SearchParagraph<'a, I>>,
    matcher: Matcher,
    context: usize,
}

struct ParagraphMatches<'a> {
    analysis: Option<Analysis<'a>>,
    ranges: Vec<Range<usize>>,
}

impl<'a, I: Copy + Eq + Debug> Searcher<'a, I> {
    /// The query is normalized to NFC first. An empty query (or one made only
    /// of marks that the options ignore) matches nothing.
    pub fn new<S: ParagraphSource<Id = I> + ?Sized>(
        source: &'a S,
        query: &str,
        options: SearchOptions,
    ) -> Self {
        let flags = Flags {
            match_case: options.match_case,
            whole_word: options.whole_word,
            ignore_accents: options.ignore_accents,
            match_nbsp: options.match_nbsp,
        };
        Self {
            paragraphs: source.paragraphs(),
            matcher: Matcher::new(query, flags),
            context: options.context_graphemes,
        }
    }

    /// Whether the query can match nothing at all.
    pub fn is_empty_query(&self) -> bool {
        self.matcher.is_empty()
    }

    /// Index of a paragraph in reading order.
    pub fn paragraph_index(&self, id: I) -> Option<usize> {
        self.paragraphs.iter().position(|p| p.id == id)
    }

    /// Lazy iterator over all occurrences in reading order.
    pub fn iter(&self) -> Matches<'_, 'a, I> {
        Matches {
            searcher: self,
            next_paragraph: 0,
            current: None,
            with_excerpt: true,
        }
    }

    /// Up to `max` occurrences, with a flag telling whether there were more.
    pub fn find_all(&self, max: usize) -> SearchResults<I> {
        let mut it = self.iter();
        let items: Vec<_> = it.by_ref().take(max).collect();
        let truncated = it.next().is_some();
        SearchResults { items, truncated }
    }

    /// Number of occurrences, stopping after `max`; the flag tells whether the
    /// count was cut. Builds no excerpt.
    pub fn count(&self, max: usize) -> (usize, bool) {
        let mut it = self.iter();
        it.with_excerpt = false;
        let n = it.by_ref().take(max).count();
        (n, it.next().is_some())
    }

    /// First occurrence starting at or after `from` (document order), wrapping
    /// to the start of the document. An unknown paragraph means the start.
    pub fn next_from(&self, from: &Position<I>) -> Option<Step<I>> {
        let n = self.paragraphs.len();
        if n == 0 {
            return None;
        }
        let (p, offset) = match self.paragraph_index(from.paragraph) {
            Some(p) => (p, from.offset),
            None => (0, 0),
        };
        for i in p..n {
            let found = self.paragraph_matches(i);
            let k = found.ranges.partition_point(|r| i == p && r.start < offset);
            if k < found.ranges.len() {
                return Some(self.step(i, &found, k, false));
            }
        }
        for i in 0..=p {
            let found = self.paragraph_matches(i);
            if !found.ranges.is_empty() {
                return Some(self.step(i, &found, 0, true));
            }
        }
        None
    }

    /// Last occurrence starting before `from`, wrapping to the end of the
    /// document. An unknown paragraph means the end.
    pub fn previous_before(&self, from: &Position<I>) -> Option<Step<I>> {
        let n = self.paragraphs.len();
        if n == 0 {
            return None;
        }
        let (p, offset) = match self.paragraph_index(from.paragraph) {
            Some(p) => (p, from.offset),
            None => (n - 1, usize::MAX),
        };
        for i in (0..=p).rev() {
            let found = self.paragraph_matches(i);
            let k = found.ranges.partition_point(|r| i != p || r.start < offset);
            if k > 0 {
                return Some(self.step(i, &found, k - 1, false));
            }
        }
        for i in (p..n).rev() {
            let found = self.paragraph_matches(i);
            if let Some(k) = found.ranges.len().checked_sub(1) {
                return Some(self.step(i, &found, k, true));
            }
        }
        None
    }

    fn paragraph_matches(&self, index: usize) -> ParagraphMatches<'a> {
        let text = self.paragraphs[index].text;
        if self.matcher.is_empty() || text.is_empty() {
            return ParagraphMatches {
                analysis: None,
                ranges: Vec::new(),
            };
        }
        let analysis = self.matcher.analyze(text);
        let ranges = self.matcher.find(text, &analysis);
        ParagraphMatches {
            analysis: Some(analysis),
            ranges,
        }
    }

    fn step(&self, index: usize, found: &ParagraphMatches<'a>, k: usize, wrapped: bool) -> Step<I> {
        Step {
            occurrence: self.occurrence(index, found, k, true),
            wrapped,
        }
    }

    fn occurrence(
        &self,
        index: usize,
        found: &ParagraphMatches<'a>,
        k: usize,
        with_excerpt: bool,
    ) -> Occurrence<I> {
        let p = &self.paragraphs[index];
        let range = found.ranges[k].clone();
        let excerpt = match (&found.analysis, with_excerpt && self.context > 0) {
            (Some(analysis), true) => Some(excerpt(p.text, analysis, &range, self.context)),
            _ => None,
        };
        Occurrence {
            paragraph: p.id,
            paragraph_index: index,
            section: p.section,
            zone: p.zone,
            range,
            excerpt,
        }
    }
}

fn excerpt(text: &str, analysis: &Analysis<'_>, range: &Range<usize>, n: usize) -> Excerpt {
    let b = analysis.boundaries();
    // Both ends of `range` are grapheme boundaries by construction.
    let first = b.partition_point(|&x| x < range.start);
    let last = b.partition_point(|&x| x < range.end);
    let from = first.saturating_sub(n);
    let to = last.saturating_add(n).min(b.len() - 1);
    Excerpt {
        text: text[b[from]..b[to]].to_owned(),
        start: range.start - b[from],
        end: range.end - b[from],
        truncated_before: from > 0,
        truncated_after: to < b.len() - 1,
    }
}

/// Lazy iterator of [`Searcher::iter`]; holds the matches of one paragraph.
pub struct Matches<'s, 'a, I> {
    searcher: &'s Searcher<'a, I>,
    next_paragraph: usize,
    current: Option<(usize, ParagraphMatches<'a>, usize)>,
    with_excerpt: bool,
}

impl<I: Copy + Eq + Debug> Iterator for Matches<'_, '_, I> {
    type Item = Occurrence<I>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((index, found, k)) = &mut self.current {
                if *k < found.ranges.len() {
                    let item = self
                        .searcher
                        .occurrence(*index, found, *k, self.with_excerpt);
                    *k += 1;
                    return Some(item);
                }
                self.current = None;
            }
            if self.next_paragraph >= self.searcher.paragraphs.len() {
                return None;
            }
            let index = self.next_paragraph;
            self.next_paragraph += 1;
            let found = self.searcher.paragraph_matches(index);
            if !found.ranges.is_empty() {
                self.current = Some((index, found, 0));
            }
        }
    }
}
