// SPDX-License-Identifier: AGPL-3.0-or-later
//! Assembly of the rich document: main story text, special characters, fields,
//! character runs and paragraph properties.

use super::assemble::{Emitter, Grouper, Rec, Repairs};
use super::bookmarks::{self, BookmarkDef};
use super::fib::{
    Fib, PAIR_CLX, PAIR_DOP, PAIR_PLCF_BKF, PAIR_PLCF_BKL, PAIR_PLCF_LST, PAIR_PLCFBTE_CHPX,
    PAIR_PLCFBTE_PAPX, PAIR_PLCFFLD_HDR, PAIR_PLCFFLD_MOM, PAIR_PLCFHDD, PAIR_PLCFSED,
    PAIR_PLF_LFO, PAIR_STSHF, PAIR_STTBF_FFN,
};
use super::fields::{self, FieldKind, FldMarks, ImportedField};
use super::fkp::{self, Fkp};
use super::headers::{Hdd, SLOTS};
use super::lists::{ImportedList, ImportedListItem, ListTables};
use super::sections::{
    self, HfState, ImportedHeaderFooter, ImportedSection, SectionBreak, SectionDef,
};
use super::sprm::{BaseToggles, CharDelta, ParaDelta, Stats};
use super::stsh::{self, Converted};
use super::tables::ImportedDocTables;
use super::text::{self, Clx};
use super::{
    DocError, DocFont, DocInfo, DocLimits, DocVersion, DocumentProperties, ImportNotice,
    ImportedBookmark, ImportedBreak, ImportedDoc, ImportedRunStyle, TextRegion, fonts, rd_u16,
    summary,
};
use atelier_cfb::{CfbError, Compound};
use atelier_model::Transaction;
use atelier_text_model::rich::{
    Bookmark, CharRun, Field, HeaderFooter, Link, LinkTarget, ListRef, Node, NodeId, Paragraph,
    RichDocument, RichOp, Subtree,
};
use atelier_unicode::segmentation::grapheme_boundaries;
use std::collections::{BTreeMap, BTreeSet};

const BATCH: usize = 4096;

fn read_stream(cfb: &Compound, name: &'static str) -> Result<Vec<u8>, DocError> {
    cfb.read(name).map_err(|e| match e {
        CfbError::NotFound | CfbError::NotAStream => DocError::MissingStream(name),
        e => DocError::Cfb(e),
    })
}

/// Limit errors stay fatal; other damage in a secondary structure becomes a
/// notice and an empty value.
fn soft<T: Default>(
    r: Result<T, DocError>,
    what: &'static str,
    notices: &mut Vec<ImportNotice>,
) -> Result<T, DocError> {
    match r {
        Ok(v) => Ok(v),
        Err(e @ DocError::Limit(_)) => Err(e),
        Err(_) => {
            notices.push(ImportNotice::DamagedData { what, count: 1 });
            Ok(T::default())
        }
    }
}

fn version_of(fib: &Fib) -> DocVersion {
    match fib.n_fib_new.unwrap_or(fib.n_fib) {
        0x00C1 => DocVersion::Word97,
        0x00D9 => DocVersion::Word2000,
        0x0101 => DocVersion::Word2002,
        0x010C => DocVersion::Word2003,
        0x0112 => DocVersion::Word2007,
        n => DocVersion::Other(n),
    }
}

struct Seg {
    start: usize,
    end: usize,
    delta: CharDelta,
}

fn piece_grpprl<'t>(
    table: &'t [u8],
    clx: &Clx,
    prm: u16,
    simple_prm: &mut usize,
) -> Option<&'t [u8]> {
    if prm == 0 {
        None
    } else if prm & 1 == 1 {
        clx.prcs
            .get((prm >> 1) as usize)
            .and_then(|r| table.get(r.clone()))
    } else {
        *simple_prm += 1;
        None
    }
}

struct PendField {
    start: usize,
    end: usize,
    kind: FieldKind,
    code: String,
    flt: Option<u8>,
    nested: bool,
    link: Option<(LinkTarget, Option<String>)>,
}

struct Open {
    text: String,
    spans: Vec<(usize, usize, usize)>,
    breaks: Vec<(usize, &'static str, Option<&'static str>)>,
    fields: Vec<PendField>,
    /// Closed bookmarks: definition index, start, end (byte offsets), cut.
    bks: Vec<(usize, usize, usize, bool)>,
}

impl Open {
    fn new() -> Self {
        Self {
            text: String::new(),
            spans: Vec::new(),
            breaks: Vec::new(),
            fields: Vec::new(),
            bks: Vec::new(),
        }
    }
    fn push(&mut self, c: char, seg: Option<usize>) {
        let start = self.text.len();
        self.text.push(c);
        if let Some(s) = seg {
            if let Some(last) = self.spans.last_mut() {
                if last.2 == s && last.1 == start {
                    last.1 = self.text.len();
                    return;
                }
            }
            self.spans.push((start, self.text.len(), s));
        }
    }
}

#[derive(Default)]
struct Counters {
    simple_prm: usize,
    bad_para_style: usize,
    bad_char_style: usize,
    bad_huge: usize,
    unterminated: usize,
    bad_list: usize,
    legacy_numbering: usize,
    objects: BTreeMap<&'static str, usize>,
    field_issues: BTreeMap<&'static str, usize>,
    bookmark_issues: BTreeMap<&'static str, usize>,
}

/// A field being read.
struct Frame {
    code: String,
    in_result: bool,
    /// Number of paragraphs closed when the field began.
    para: usize,
    result_start: usize,
    nested: bool,
    flt: Option<u8>,
}

/// Story being read.
struct Story {
    /// Character position of the first character of the story.
    start: usize,
    /// Character position the field marks of the story are relative to.
    base: usize,
    marks: FldMarks,
    main: bool,
    section: usize,
}

struct Builder<'a> {
    cfb: &'a Compound<'a>,
    word: &'a [u8],
    table: &'a [u8],
    clx: &'a Clx,
    papx: &'a [Fkp],
    segs: &'a [Seg],
    sections: &'a [SectionDef],
    conv: &'a Converted,
    fonts: &'a [DocFont],
    limits: &'a DocLimits,
    lfo_count: usize,
    stats: Stats,
    counters: Counters,
    data: Option<Option<Vec<u8>>>,
    document: RichDocument,
    recs: Vec<Rec>,
    story: Story,
    breaks: Vec<ImportedBreak>,
    run_styles: Vec<ImportedRunStyle>,
    list_items: Vec<ImportedListItem>,
    fields: Vec<ImportedField>,
    bookmarks: Vec<ImportedBookmark>,
    bk_defs: Vec<BookmarkDef>,
    bk_by_start: Vec<usize>,
    bk_by_end: Vec<usize>,
    bk_cursor: (usize, usize),
    bk_open: BTreeMap<usize, usize>,
    bk_names: BTreeSet<String>,
}

impl Builder<'_> {
    fn model_err(e: impl ToString) -> DocError {
        DocError::Model(e.to_string())
    }

    fn huge_grpprl(&mut self, offset: u32) -> Option<Vec<u8>> {
        if self.data.is_none() {
            self.data = Some(self.cfb.read("Data").ok());
        }
        let d = self.data.as_ref()?.as_ref()?;
        let o = offset as usize;
        let cb = rd_u16(d, o)? as usize;
        d.get(o.checked_add(2)?..o.checked_add(2)?.checked_add(cb)?)
            .map(<[u8]>::to_vec)
    }

    fn issue(map: &mut BTreeMap<&'static str, usize>, what: &'static str) {
        *map.entry(what).or_insert(0) += 1;
    }

    fn section_of(&self, cp: usize) -> usize {
        if !self.story.main {
            return self.story.section;
        }
        let i = self.sections.partition_point(|d| d.end_cp <= cp);
        i.min(self.sections.len().saturating_sub(1))
    }

    /// Fires the bookmark start and end positions up to `cp`.
    fn fire_bookmarks(&mut self, cp: usize, open: &mut Open) {
        while let Some(&i) = self.bk_by_start.get(self.bk_cursor.0) {
            if self.bk_defs[i].start > cp {
                break;
            }
            self.bk_cursor.0 += 1;
            if self.bk_defs[i].start >= self.story.start {
                self.bk_open.insert(i, open.text.len());
            }
        }
        while let Some(&i) = self.bk_by_end.get(self.bk_cursor.1) {
            if self.bk_defs[i].end > cp {
                break;
            }
            self.bk_cursor.1 += 1;
            if let Some(start) = self.bk_open.remove(&i) {
                open.bks.push((i, start, open.text.len(), false));
            }
        }
    }

    /// Closes the open paragraph whose last character is at `end_cp`.
    fn finish(&mut self, open: Open, end_cp: usize, mark: u16) -> Result<(), DocError> {
        let mut open = open;
        // Paragraph properties: FKP entry of the last character, then the
        // piece's property modifier.
        let (mut istd0, mut delta) = (0u16, ParaDelta::default());
        let piece = self.clx.piece_index(end_cp).map(|i| &self.clx.pieces[i]);
        if let Some(p) = piece {
            let fc = p.fc_of(end_cp);
            if let Some(e) = u32::try_from(fc)
                .ok()
                .and_then(|f| fkp::lookup(self.papx, f))
            {
                istd0 = e.istd;
                let g = &self.word[e.grpprl.clone()];
                delta.apply_all(g, &mut self.stats);
                if let Some(off) = delta.huge {
                    delta = ParaDelta::default();
                    match self.huge_grpprl(off) {
                        Some(h) => delta.apply_all(&h, &mut self.stats),
                        None => self.counters.bad_huge += 1,
                    }
                }
            }
            // Simple modifiers are counted once per piece by the run pass.
            if let Some(g) = piece_grpprl(self.table, self.clx, p.prm, &mut 0) {
                delta.apply_all(g, &mut self.stats);
            }
        }
        if self.stats.sprms > self.limits.max_sprms {
            return Err(DocError::Limit("sprm count"));
        }
        let itap = match delta.itap {
            Some(v) if v > 0 => v as usize,
            _ => usize::from(delta.in_table),
        };
        let row_end = itap > 0 && (delta.row_end || delta.inner_ttp);
        let cell_end = itap > 0 && (mark == 0x07 || delta.inner_cell);
        let tap = (itap > 0 && delta.tap.any).then(|| Box::new(delta.tap.clone()));
        let istd = delta.istd.unwrap_or(istd0) as usize;
        let style_id = match self.conv.paragraph_id(istd) {
            Some(id) => Some(id.to_owned()),
            None => {
                if self.conv.paragraph_id(0).is_some() {
                    self.counters.bad_para_style += 1;
                }
                self.conv.paragraph_id(0).map(str::to_owned)
            }
        };
        let para_istd = if self.conv.paragraph_id(istd).is_some() {
            istd
        } else {
            0
        };
        let mut props = delta.to_props(&mut self.stats);
        props.style = style_id;
        if delta.anld {
            self.counters.legacy_numbering += 1;
        }
        // List reference: paragraph, else style.
        let (style_ilfo, style_ilvl) = self.conv.style_list(para_istd);
        let ilfo = delta.ilfo.or(style_ilfo);
        let ilvl = delta.ilvl.or(style_ilvl);
        let mut list_item = None;
        if let Some(i) = ilfo.filter(|i| *i != 0) {
            if i > 0 && (i as usize) <= self.lfo_count {
                let level = ilvl.unwrap_or(0).min(8);
                props.list = Some(ListRef {
                    list_id: i as u32,
                    level,
                });
                list_item = Some((i as u32, level, delta.ilfo.is_none()));
            } else {
                self.counters.bad_list += 1;
            }
        }

        // Character runs, snapped to grapheme boundaries.
        let text = std::mem::take(&mut open.text);
        let index = (!text.is_ascii()).then(|| grapheme_boundaries(&text));
        let snap_down = |o: usize| match &index {
            None => o,
            Some(ix) => {
                let b = ix.boundaries();
                b[b.partition_point(|x| *x <= o).saturating_sub(1)]
            }
        };
        let snap_up = |o: usize| match &index {
            None => o,
            Some(ix) => {
                let b = ix.boundaries();
                b.get(b.partition_point(|x| *x < o)).copied().unwrap_or(o)
            }
        };
        let mut runs: Vec<CharRun> = Vec::new();
        let mut styled: Vec<(usize, usize, String)> = Vec::new();
        let mut prev_end = 0usize;
        for &(s, e, seg) in &open.spans {
            let (s, e) = (snap_down(s).max(prev_end), snap_up(e).min(text.len()));
            if e <= s {
                continue;
            }
            prev_end = e;
            let d = &self.segs[seg].delta;
            let char_style = d.istd.and_then(|i| {
                let id = self.conv.character_id(i as usize);
                if id.is_none() {
                    self.counters.bad_char_style += 1;
                }
                id.map(|id| (i as usize, id.to_owned()))
            });
            let base = if d.needs_base() {
                self.conv
                    .base_toggles(Some(para_istd), char_style.as_ref().map(|c| c.0))
            } else {
                BaseToggles::default()
            };
            let attrs = d.to_attrs(base, self.fonts, &mut self.stats);
            if let Some((_, id)) = char_style {
                match styled.last_mut() {
                    Some(l) if l.1 == s && l.2 == id => l.1 = e,
                    _ => styled.push((s, e, id)),
                }
            }
            if attrs.is_empty() {
                continue;
            }
            match runs.last_mut() {
                Some(l) if l.end == s && l.attrs == attrs => l.end = e,
                _ => runs.push(CharRun {
                    start: s,
                    end: e,
                    attrs,
                }),
            }
        }
        let id = self.document.alloc_id().map_err(Self::model_err)?;

        // Fields, links and bookmarks.
        let mut model_fields: Vec<Field> = Vec::new();
        let mut model_links: Vec<Link> = Vec::new();
        let mut pending = std::mem::take(&mut open.fields);
        pending.sort_by_key(|f| (f.start, f.end));
        let mut last_field_end = 0usize;
        let mut last_link_end = 0usize;
        for pf in pending {
            let s = snap_down(pf.start);
            let e = snap_up(pf.end).max(s);
            let mut entry = ImportedField {
                paragraph: id,
                kind: pf.kind,
                code: pf.code.clone(),
                field_type: pf.flt,
                field: None,
                link: None,
            };
            if pf.kind == FieldKind::Hyperlink {
                match pf.link {
                    Some((target, tooltip)) if e > s && s >= last_link_end => {
                        let lid = self.document.alloc_id().map_err(Self::model_err)?;
                        model_links.push(Link {
                            id: lid,
                            start: s,
                            end: e,
                            target,
                            tooltip,
                        });
                        last_link_end = e;
                        entry.link = Some(lid);
                    }
                    _ => Self::issue(&mut self.counters.field_issues, "hyperlink not kept"),
                }
            } else if pf.nested {
                Self::issue(&mut self.counters.field_issues, "field containing a field");
            } else if s < last_field_end {
                Self::issue(&mut self.counters.field_issues, "overlapping fields");
            } else {
                let fid = self.document.alloc_id().map_err(Self::model_err)?;
                model_fields.push(Field {
                    id: fid,
                    start: s,
                    end: e,
                    instruction: fields::instruction_of(pf.kind, &pf.code),
                });
                last_field_end = e;
                entry.field = Some(fid);
            }
            self.fields.push(entry);
        }
        // Still open bookmarks end with the paragraph.
        let open_now = std::mem::take(&mut self.bk_open);
        for (i, start) in open_now {
            let cut = self.bk_defs[i].end > end_cp + 1;
            open.bks.push((i, start, text.len(), cut));
        }
        let mut model_bks: Vec<Bookmark> = Vec::new();
        for (i, s, e, cut) in std::mem::take(&mut open.bks) {
            let def = &self.bk_defs[i];
            if !self.bk_names.insert(def.name.clone()) {
                Self::issue(&mut self.counters.bookmark_issues, "duplicate name");
                continue;
            }
            if cut {
                Self::issue(
                    &mut self.counters.bookmark_issues,
                    "range cut at the paragraph end",
                );
            }
            let s = snap_down(s);
            let e = snap_up(e).max(s);
            let bid = self.document.alloc_id().map_err(Self::model_err)?;
            model_bks.push(Bookmark {
                id: bid,
                name: def.name.clone(),
                start: s,
                end: e,
            });
            self.bookmarks.push(ImportedBookmark {
                name: def.name.clone(),
                paragraph: id,
                bookmark: bid,
                start_cp: def.start,
                end_cp: def.end,
                truncated: cut,
            });
        }
        model_bks.sort_by_key(|b| (b.start, b.end, b.id));
        let paragraph = Paragraph {
            text,
            runs,
            props,
            fields: model_fields,
            links: model_links,
            bookmarks: model_bks,
            ..Paragraph::default()
        };
        if let Some((list_id, level, from_style)) = list_item {
            self.list_items.push(ImportedListItem {
                paragraph: id,
                list_id,
                level,
                from_style,
            });
        }
        for (offset, kind, break_type) in open.breaks {
            self.breaks.push(ImportedBreak {
                paragraph: id,
                offset,
                kind: kind.to_owned(),
                break_type: break_type.map(str::to_owned),
            });
        }
        for (start, end, style) in styled {
            self.run_styles.push(ImportedRunStyle {
                paragraph: id,
                start,
                end,
                style_id: Some(style),
            });
        }
        let section = self.section_of(end_cp);
        let has_text = !paragraph.text.is_empty() || !paragraph.bookmarks.is_empty();
        if row_end && has_text {
            // Content in the row-end paragraph: a last cell, then the row end.
            self.recs.push(Rec {
                id,
                para: paragraph,
                itap,
                cell_end: true,
                row_end: false,
                tap: None,
                section,
            });
            let marker = self.document.alloc_id().map_err(Self::model_err)?;
            self.recs.push(Rec {
                id: marker,
                para: Paragraph::default(),
                itap,
                cell_end: false,
                row_end: true,
                tap,
                section,
            });
        } else {
            self.recs.push(Rec {
                id,
                para: paragraph,
                itap,
                cell_end,
                row_end,
                tap: if row_end { tap } else { None },
                section,
            });
        }
        if self.recs.len() > self.limits.max_paragraphs {
            return Err(DocError::Limit("paragraph count"));
        }
        Ok(())
    }
}

/// Inputs of [`build_segments`].
struct SegInput<'a> {
    word: &'a [u8],
    table: &'a [u8],
    clx: &'a Clx,
    chpx: &'a [Fkp],
    n_units: usize,
    limits: &'a DocLimits,
}

fn build_segments(
    input: &SegInput<'_>,
    stats: &mut Stats,
    simple_prm: &mut usize,
) -> Result<Vec<Seg>, DocError> {
    let SegInput {
        word,
        table,
        clx,
        chpx,
        n_units,
        limits,
    } = *input;
    let mut segs: Vec<Seg> = Vec::new();
    for p in &clx.pieces {
        let (a, b) = (p.cp_start, p.cp_end.min(n_units));
        if a >= b {
            break;
        }
        let w = p.width();
        let prm = piece_grpprl(table, clx, p.prm, simple_prm);
        let mut cp = a;
        while cp < b {
            let f = p.fc_of(cp) as u32;
            let (next_cp, entry) = match fkp::lookup(chpx, f) {
                Some(e) => {
                    let n = ((e.fc_end - f) as usize).div_ceil(w).max(1);
                    (b.min(cp + n), Some(e))
                }
                None => match fkp::next_start(chpx, f) {
                    Some(s) => {
                        let n = ((s - f) as usize).div_ceil(w).max(1);
                        (b.min(cp + n), None)
                    }
                    None => (b, None),
                },
            };
            let mut delta = CharDelta::default();
            if let Some(e) = entry {
                delta.apply_all(&word[e.grpprl.clone()], stats);
            }
            if let Some(g) = prm {
                delta.apply_all(g, stats);
            }
            if stats.sprms > limits.max_sprms {
                return Err(DocError::Limit("sprm count"));
            }
            if delta != CharDelta::default() {
                match segs.last_mut() {
                    Some(l) if l.end == cp && l.delta == delta => l.end = next_cp,
                    _ => {
                        if segs.len() >= limits.max_runs {
                            return Err(DocError::Limit("character runs"));
                        }
                        segs.push(Seg {
                            start: cp,
                            end: next_cp,
                            delta,
                        });
                    }
                }
            }
            cp = next_cp;
        }
    }
    Ok(segs)
}

/// Detached copy of the branch rooted at `root`, taken out of `nodes`.
fn take_subtree(nodes: &mut BTreeMap<NodeId, Node>, root: NodeId) -> Subtree {
    let mut out = BTreeMap::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let Some(n) = nodes.remove(&id) {
            stack.extend(n.children());
            out.insert(id, n);
        }
    }
    Subtree { root, nodes: out }
}

fn apply_ops(document: &mut RichDocument, ops: &mut Vec<RichOp>) -> Result<(), DocError> {
    if ops.is_empty() {
        return Ok(());
    }
    let t = Transaction::new(std::mem::take(ops)).map_err(Builder::model_err)?;
    document.apply(&t).map_err(Builder::model_err)?;
    Ok(())
}

pub(super) fn import(cfb: &Compound, limits: &DocLimits) -> Result<ImportedDoc, DocError> {
    let word = read_stream(cfb, "WordDocument")?;
    let fib = Fib::parse(&word)?;
    let table_name = if fib.which_table_is_1() {
        "1Table"
    } else {
        "0Table"
    };
    let table = read_stream(cfb, table_name)?;
    let mut notices = Vec::new();
    let ccp_text = fib.ccp_text as usize;
    if ccp_text > limits.max_text_chars {
        return Err(DocError::Limit("text characters"));
    }

    // Piece table and text.
    let (fc, lcb) = fib.pair(PAIR_CLX);
    let clx = Clx::parse(&table, fc, lcb, limits.max_pieces)?;
    if clx.pieces.first().is_some_and(|p| p.cp_start != 0) {
        return Err(DocError::InvalidStructure(
            "piece table does not start at position 0",
        ));
    }
    // Header stories follow the main text and the footnotes.
    let ccp_ftn = fib.ccp_ftn as usize;
    let ccp_hdd = fib.ccp_hdd as usize;
    let (fc, lcb) = fib.pair(PAIR_PLCFHDD);
    let hdd = if ccp_hdd > 0 && ccp_ftn.saturating_add(ccp_hdd) <= limits.max_text_chars {
        Hdd::parse(&table, fc, lcb, limits)
    } else {
        None
    };
    let hdd_start = ccp_text.saturating_add(ccp_ftn);
    let decode_end = if hdd.is_some() {
        hdd_start.saturating_add(ccp_hdd)
    } else {
        ccp_text
    };
    let mut units: Vec<u16> = Vec::with_capacity(decode_end.min(word.len()));
    for p in &clx.pieces {
        if p.cp_start >= decode_end {
            break;
        }
        let end = p.cp_end.min(decode_end);
        let n = text::decode_piece(&word, p, p.cp_start, end, &mut units);
        if n < end - p.cp_start {
            break;
        }
    }
    if units.len() < ccp_text {
        notices.push(ImportNotice::DamagedData {
            what: "main text shorter than declared",
            count: ccp_text - units.len(),
        });
    }

    // Fonts and styles.
    let (fc, lcb) = fib.pair(PAIR_STTBF_FFN);
    let (font_list, fonts_damaged) = soft(
        fonts::parse(&table, fc, lcb, limits.max_fonts),
        "font table",
        &mut notices,
    )?;
    if fonts_damaged {
        notices.push(ImportNotice::DamagedData {
            what: "font table record",
            count: 1,
        });
    }
    let mut stats = Stats::default();
    let (fc, lcb) = fib.pair(PAIR_STSHF);
    let sheet = soft(
        stsh::parse(&table, fc, lcb, limits.max_styles),
        "style sheet",
        &mut notices,
    )?;
    if sheet.damaged > 0 {
        notices.push(ImportNotice::DamagedData {
            what: "style definition",
            count: sheet.damaged,
        });
    }
    let conv = stsh::convert(&sheet, &table, &font_list, &mut stats, &mut notices, limits)?;

    // Lists.
    let raw_lists = soft(
        ListTables::parse(
            &table,
            fib.pair(PAIR_PLCF_LST),
            fib.pair(PAIR_PLF_LFO),
            limits,
        ),
        "list tables",
        &mut notices,
    )?;
    if raw_lists.damaged > 0 {
        notices.push(ImportNotice::DamagedData {
            what: "list definition",
            count: raw_lists.damaged,
        });
    }
    let lists: Vec<ImportedList> = raw_lists.convert(&font_list, &mut stats);

    // Formatting pages.
    let load = |pair: usize, papx: bool, notices: &mut Vec<ImportNotice>| {
        let (fc, lcb) = fib.pair(pair);
        let bte = soft(
            fkp::read_bte(&table, fc, lcb, limits.max_fkp_pages),
            "bin table",
            notices,
        )?;
        let (entries, bad) = fkp::read_pages(&word, &bte, papx, limits.max_runs);
        if bad > 0 {
            notices.push(ImportNotice::DamagedData {
                what: "formatted disk page",
                count: bad,
            });
        }
        Ok::<_, DocError>(entries)
    };
    let chpx = load(PAIR_PLCFBTE_CHPX, false, &mut notices)?;
    let papx = load(PAIR_PLCFBTE_PAPX, true, &mut notices)?;

    // Sections.
    let (mut sec_defs, sec_damaged) = sections::parse(
        &table,
        &word,
        fib.pair(PAIR_PLCFSED),
        ccp_text,
        limits,
        &mut stats,
    )?;
    if sec_damaged > 0 {
        notices.push(ImportNotice::DamagedData {
            what: "section properties",
            count: sec_damaged,
        });
    }
    if sec_defs.is_empty() {
        sec_defs.push(SectionDef {
            first_cp: 0,
            end_cp: ccp_text,
            sepx: Default::default(),
        });
    }

    // Bookmarks.
    let (bk_defs, bk_damaged) = bookmarks::parse(
        &table,
        fib.pair(super::fib::PAIR_STTBF_BKMK),
        fib.pair(PAIR_PLCF_BKF),
        fib.pair(PAIR_PLCF_BKL),
        limits,
    );
    let mut bk_issues: BTreeMap<&'static str, usize> = BTreeMap::new();
    if bk_damaged > 0 {
        bk_issues.insert("unreadable entry", bk_damaged);
    }
    let bk_defs: Vec<BookmarkDef> = bk_defs
        .into_iter()
        .filter(|d| {
            let ok = d.end >= d.start;
            if !ok {
                *bk_issues.entry("end before start").or_insert(0) += 1;
            }
            ok
        })
        .collect();
    let mut bk_by_start: Vec<usize> = (0..bk_defs.len()).collect();
    bk_by_start.sort_by_key(|&i| (bk_defs[i].start, i));
    let mut bk_by_end: Vec<usize> = (0..bk_defs.len()).collect();
    bk_by_end.sort_by_key(|&i| (bk_defs[i].end, i));

    let facing_pages = {
        let (fc, lcb) = fib.pair(PAIR_DOP);
        lcb > 0 && table.get(fc).is_some_and(|b| b & 1 != 0)
    };

    let mut simple_prm = 0usize;
    let segs = build_segments(
        &SegInput {
            word: &word,
            table: &table,
            clx: &clx,
            chpx: &chpx,
            n_units: units.len(),
            limits,
        },
        &mut stats,
        &mut simple_prm,
    )?;

    let document = RichDocument::new();
    let (fc, lcb) = fib.pair(PAIR_PLCFFLD_MOM);
    let main_marks = FldMarks::parse(&table, fc, lcb, limits);
    let mut b = Builder {
        cfb,
        word: &word,
        table: &table,
        clx: &clx,
        papx: &papx,
        segs: &segs,
        sections: &sec_defs,
        conv: &conv,
        fonts: &font_list,
        limits,
        lfo_count: lists.len(),
        stats,
        counters: Counters {
            simple_prm,
            ..Counters::default()
        },
        data: None,
        document,
        recs: Vec::new(),
        story: Story {
            start: 0,
            base: 0,
            marks: main_marks,
            main: true,
            section: 0,
        },
        breaks: Vec::new(),
        run_styles: Vec::new(),
        list_items: Vec::new(),
        fields: Vec::new(),
        bookmarks: Vec::new(),
        bk_defs,
        bk_by_start,
        bk_by_end,
        bk_cursor: (0, 0),
        bk_open: BTreeMap::new(),
        bk_names: BTreeSet::new(),
    };
    b.counters.bookmark_issues.extend(bk_issues);
    walk(&mut b, &units, 0, ccp_text)?;

    // Body: blocks, then one model section per section of the file.
    let mut tables = ImportedDocTables::default();
    let mut rep = Repairs::default();
    let main_recs = std::mem::take(&mut b.recs);
    let (blocks, _) = Grouper::new(main_recs, limits.max_table_depth).run();
    let first_section = *b
        .document
        .section_ids()
        .first()
        .ok_or(DocError::InvalidStructure("rich document has no section"))?;
    let nsec = sec_defs.len();
    let mut section_ids = vec![first_section];
    let mut ops: Vec<RichOp> = Vec::new();
    for k in 1..nsec {
        let sub = b.document.new_section().map_err(Builder::model_err)?;
        section_ids.push(sub.root);
        ops.push(RichOp::InsertBlock {
            parent: None,
            position: k,
            subtree: sub,
        });
    }
    apply_ops(&mut b.document, &mut ops)?;
    let mut positions = vec![1usize; nsec];
    let mut order: Vec<NodeId>;
    let (mut rows_seen, mut cells_seen);
    let mut header_paragraphs: Vec<(NodeId, Vec<NodeId>)> = Vec::new();
    let mut imported_sections: Vec<ImportedSection> = Vec::new();
    {
        let mut em = Emitter {
            doc: &mut b.document,
            tables: &mut tables,
            rep: &mut rep,
            limits,
            order: Vec::new(),
            rows: 0,
            cells: 0,
        };
        for block in blocks {
            let sec = block.section().min(nsec - 1);
            let mut nodes = BTreeMap::new();
            let ids = em.emit(vec![block], &mut nodes, 0)?;
            for id in ids {
                let subtree = take_subtree(&mut nodes, id);
                ops.push(RichOp::InsertBlock {
                    parent: Some(section_ids[sec]),
                    position: positions[sec],
                    subtree,
                });
                positions[sec] += 1;
            }
            if ops.len() >= BATCH {
                apply_ops(em.doc, &mut ops)?;
            }
        }
        for k in 0..nsec {
            if positions[k] > 1 {
                // Drop the empty paragraph the section is created with.
                ops.push(RichOp::RemoveBlock {
                    parent: Some(section_ids[k]),
                    position: 0,
                });
            }
        }
        apply_ops(em.doc, &mut ops)?;
        order = std::mem::take(&mut em.order);
        rows_seen = em.rows;
        cells_seen = em.cells;
    }

    // Headers and footers.
    let (fc, lcb) = fib.pair(PAIR_PLCFFLD_HDR);
    let hdr_marks = FldMarks::parse(&table, fc, lcb, limits);
    {
        for (k, def) in sec_defs.iter().enumerate() {
            let mut slots = Vec::new();
            if let Some(hdd) = &hdd {
                for (slot, &(kind, variant)) in SLOTS.iter().enumerate() {
                    use atelier_text_model::rich::HfVariant;
                    let applies = match variant {
                        HfVariant::First => def.sepx.title_page,
                        HfVariant::Even => facing_pages,
                        HfVariant::Default => true,
                    };
                    if !applies {
                        continue;
                    }
                    let Some((a, e)) = hdd.story(k, slot) else {
                        continue;
                    };
                    let a = hdd_start.saturating_add(a).min(units.len());
                    let e = hdd_start
                        .saturating_add(e)
                        .min(hdd_start.saturating_add(ccp_hdd))
                        .min(units.len());
                    let state = if e <= a {
                        HfState::Inherited
                    } else {
                        let content_end = if units[e - 1] == 0x0D { e - 1 } else { e };
                        if content_end <= a || (content_end - a == 1 && units[a] == 0x0D) {
                            HfState::Empty
                        } else {
                            b.story = Story {
                                start: a,
                                base: hdd_start,
                                marks: hdr_marks.clone(),
                                main: false,
                                section: k,
                            };
                            walk(&mut b, &units, a, content_end)?;
                            let recs = std::mem::take(&mut b.recs);
                            let (blocks, _) = Grouper::new(recs, limits.max_table_depth).run();
                            let mut nodes = BTreeMap::new();
                            let mut em = Emitter {
                                doc: &mut b.document,
                                tables: &mut tables,
                                rep: &mut rep,
                                limits,
                                order: Vec::new(),
                                rows: rows_seen,
                                cells: cells_seen,
                            };
                            let ids = em.emit(blocks, &mut nodes, 0)?;
                            rows_seen = em.rows;
                            cells_seen = em.cells;
                            let paras = std::mem::take(&mut em.order);
                            let hf = em.doc.alloc_id().map_err(Builder::model_err)?;
                            nodes.insert(
                                hf,
                                Node::HeaderFooter(HeaderFooter {
                                    blocks: super::assemble::block_list(&ids)?,
                                }),
                            );
                            header_paragraphs.push((hf, paras));
                            let ops = vec![RichOp::AttachHeaderFooter {
                                section: section_ids[k],
                                kind,
                                variant,
                                subtree: Subtree { root: hf, nodes },
                            }];
                            let mut ops = ops;
                            apply_ops(em.doc, &mut ops)?;
                            HfState::Present(hf)
                        }
                    };
                    slots.push(ImportedHeaderFooter {
                        kind,
                        variant,
                        state,
                    });
                }
            }
            imported_sections.push(ImportedSection {
                section: section_ids[k],
                index: k,
                first_cp: def.first_cp,
                end_cp: def.end_cp,
                break_type: SectionBreak::from_bkc(def.sepx.bkc),
                page: def.sepx.page.clone(),
                columns: def.sepx.columns.clone(),
                page_numbers: def.sepx.numbering.clone(),
                title_page: def.sepx.title_page,
                headers_footers: slots,
            });
        }
    }
    if order.is_empty() {
        order = b.document.section_ids().first().map_or_else(Vec::new, |_| {
            b.document
                .block_ids(Some(section_ids[0]))
                .unwrap_or_default()
        });
    }
    b.document.validate().map_err(Builder::model_err)?;

    // Notices, in a stable order.
    let mut ignored: Vec<(TextRegion, u32)> = vec![
        (TextRegion::Footnotes, fib.ccp_ftn),
        (TextRegion::Comments, fib.ccp_atn),
        (TextRegion::Endnotes, fib.ccp_edn),
        (TextRegion::Textboxes, fib.ccp_txbx),
        (TextRegion::HeaderTextboxes, fib.ccp_hdr_txbx),
    ];
    if hdd.is_none() {
        ignored.insert(1, (TextRegion::HeadersFooters, fib.ccp_hdd));
    }
    for (region, n) in ignored {
        if n > 0 {
            notices.push(ImportNotice::IgnoredText {
                region,
                characters: n as usize,
            });
        }
    }
    rep.notices(&mut notices);
    let c = &b.counters;
    for (&what, &count) in &c.field_issues {
        notices.push(ImportNotice::FieldDegraded { what, count });
    }
    for (&what, &count) in &c.bookmark_issues {
        notices.push(ImportNotice::BookmarkAdjusted { what, count });
    }
    if c.legacy_numbering > 0 {
        notices.push(ImportNotice::LegacyNumbering {
            paragraphs: c.legacy_numbering,
        });
    }
    for (&kind, &count) in &c.objects {
        notices.push(ImportNotice::ObjectsDropped { kind, count });
    }
    for (&name, &count) in &b.stats.unsupported {
        notices.push(ImportNotice::UnsupportedProperty { name, count });
    }
    for (&name, &count) in &b.stats.approximated {
        notices.push(ImportNotice::ApproximatedProperty { name, count });
    }
    if b.stats.unknown_sprms > 0 {
        notices.push(ImportNotice::UnknownSprms {
            count: b.stats.unknown_sprms,
        });
    }
    if c.simple_prm > 0 {
        notices.push(ImportNotice::SimplePrmIgnored {
            count: c.simple_prm,
        });
    }
    for (what, count) in [
        ("paragraph refers to a missing style", c.bad_para_style),
        (
            "character style reference is not a character style",
            c.bad_char_style,
        ),
        ("huge paragraph properties unreadable", c.bad_huge),
        ("truncated property list", b.stats.truncated_grpprls),
        ("final paragraph mark missing", c.unterminated),
        ("list reference to a missing list", c.bad_list),
    ] {
        if count > 0 {
            notices.push(ImportNotice::DamagedData { what, count });
        }
    }

    // Document properties.
    let properties = match cfb.read("\u{5}SummaryInformation") {
        Ok(bytes) => match summary::parse(&bytes) {
            Ok((p, skipped)) => {
                if skipped > 0 {
                    notices.push(ImportNotice::DamagedData {
                        what: "summary information property",
                        count: skipped,
                    });
                }
                p
            }
            Err(problem) => {
                notices.push(ImportNotice::SummaryInformation { problem });
                DocumentProperties::default()
            }
        },
        Err(CfbError::NotFound) => {
            notices.push(ImportNotice::SummaryInformation {
                problem: "stream absent",
            });
            DocumentProperties::default()
        }
        Err(_) => {
            notices.push(ImportNotice::SummaryInformation {
                problem: "stream unreadable",
            });
            DocumentProperties::default()
        }
    };

    Ok(ImportedDoc {
        document: b.document,
        styles: conv.engine.clone(),
        info: DocInfo {
            n_fib: fib.n_fib,
            n_fib_new: fib.n_fib_new,
            version: version_of(&fib),
            template: fib.is_dot(),
            fast_saved: fib.is_complex() || fib.quick_saves() > 0,
            table_stream: table_name,
            lid: fib.lid,
            main_text_chars: ccp_text,
        },
        properties,
        fonts: font_list.clone(),
        style_records: conv.records.clone(),
        breaks: b.breaks,
        run_styles: b.run_styles,
        notices,
        tables,
        lists,
        list_items: b.list_items,
        sections: imported_sections,
        header_paragraphs,
        fields: b.fields,
        bookmarks: b.bookmarks,
        facing_pages,
        paragraph_order: order,
    })
}

/// Walks the story `start..end`, splitting it into paragraphs.
fn walk(b: &mut Builder<'_>, units: &[u16], start: usize, end: usize) -> Result<(), DocError> {
    let n = end.min(units.len());
    let mut open = Open::new();
    let mut frames: Vec<Frame> = Vec::new();
    let (mut code_depth, mut hidden, mut begun) = (0usize, 0usize, 0usize);
    let mut seg_cursor = b.segs.partition_point(|s| s.end <= start);
    let mut i = start;
    b.bk_open.clear();
    while i < n {
        let u = units[i];
        let cp = i;
        i += 1;
        while seg_cursor < b.segs.len() && b.segs[seg_cursor].end <= cp {
            seg_cursor += 1;
        }
        let seg = b
            .segs
            .get(seg_cursor)
            .filter(|s| s.start <= cp)
            .map(|_| seg_cursor);
        b.fire_bookmarks(cp, &mut open);
        let rel = cp - b.story.base.min(cp);
        let listed = b.story.marks.at(rel);
        let check = |want: u8| b.story.marks.is_empty() || listed.is_some_and(|(c, _)| c == want);
        match u {
            0x0D => {
                b.finish(std::mem::replace(&mut open, Open::new()), cp, 0x0D)?;
                continue;
            }
            0x07 => {
                b.finish(std::mem::replace(&mut open, Open::new()), cp, 0x07)?;
                continue;
            }
            0x13 => {
                if !check(0x13) {
                    Builder::issue(
                        &mut b.counters.field_issues,
                        "field mark missing from Plcffld",
                    );
                    continue;
                }
                begun += 1;
                if begun > b.limits.max_fields {
                    return Err(DocError::Limit("field count"));
                }
                if hidden > 0 || frames.len() >= b.limits.max_field_depth {
                    if hidden == 0 {
                        Builder::issue(&mut b.counters.field_issues, "field nesting too deep");
                    }
                    hidden += 1;
                    continue;
                }
                if let Some(p) = frames.last_mut() {
                    if p.in_result {
                        p.nested = true;
                    }
                }
                frames.push(Frame {
                    code: String::new(),
                    in_result: false,
                    para: b.recs.len(),
                    result_start: 0,
                    nested: false,
                    flt: listed.map(|(_, f)| f),
                });
                code_depth += 1;
                continue;
            }
            0x14 => {
                if hidden > 0 {
                    continue;
                }
                if !check(0x14) {
                    Builder::issue(
                        &mut b.counters.field_issues,
                        "field mark missing from Plcffld",
                    );
                    continue;
                }
                match frames.last_mut() {
                    Some(f) if !f.in_result => {
                        f.in_result = true;
                        f.result_start = open.text.len();
                        code_depth -= 1;
                    }
                    _ => Builder::issue(&mut b.counters.field_issues, "unmatched field mark"),
                }
                continue;
            }
            0x15 => {
                if hidden > 0 {
                    hidden -= 1;
                    continue;
                }
                if !check(0x15) {
                    Builder::issue(
                        &mut b.counters.field_issues,
                        "field mark missing from Plcffld",
                    );
                    continue;
                }
                let Some(f) = frames.pop() else {
                    Builder::issue(&mut b.counters.field_issues, "unmatched field mark");
                    continue;
                };
                if !f.in_result {
                    code_depth -= 1;
                }
                close_field(b, f, &mut open);
                continue;
            }
            _ => {}
        }
        if hidden > 0 {
            continue;
        }
        if code_depth > 0 {
            if let Some(f) = frames.iter_mut().rev().find(|f| !f.in_result) {
                if f.code.len() < 8192 {
                    match u {
                        0x09 => f.code.push(' '),
                        0x20..=0xD7FF | 0xE000..=0xFFFF => {
                            f.code.push(char::from_u32(u32::from(u)).unwrap_or(' '));
                        }
                        _ => {}
                    }
                }
            }
            continue;
        }
        let object = |b: &mut Builder<'_>, kind: &'static str| {
            *b.counters.objects.entry(kind).or_insert(0) += 1;
        };
        match u {
            0x09 => open.push('\t', seg),
            0x0B => {
                open.breaks.push((open.text.len(), "br", None));
                open.push('\n', seg);
            }
            0x0C => {
                let is_section = b.story.main
                    && b.sections
                        .binary_search_by_key(&(cp + 1), |d| d.end_cp)
                        .is_ok();
                if is_section {
                    open.breaks.push((open.text.len(), "section", None));
                    b.finish(std::mem::replace(&mut open, Open::new()), cp, 0x0C)?;
                } else {
                    open.breaks.push((open.text.len(), "br", Some("page")));
                    open.push('\n', seg);
                }
            }
            0x0E => {
                open.breaks.push((open.text.len(), "br", Some("column")));
                open.push('\n', seg);
            }
            0x1E => open.push('\u{2011}', seg),
            0x1F => open.push('\u{00AD}', seg),
            0x01 => object(b, "inlinePicture"),
            0x08 => object(b, "drawnObject"),
            0x02 => object(b, "noteReference"),
            0x05 => object(b, "commentReference"),
            0x00..=0x1F => object(b, "controlCharacter"),
            0xD800..=0xDBFF => {
                match units
                    .get(i)
                    .copied()
                    .filter(|l| (0xDC00..0xE000).contains(l))
                {
                    Some(low) if i < n => {
                        i += 1;
                        let c =
                            0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
                        open.push(char::from_u32(c).unwrap_or('\u{FFFD}'), seg);
                    }
                    _ => open.push('\u{FFFD}', seg),
                }
            }
            0xDC00..=0xDFFF => open.push('\u{FFFD}', seg),
            0x28 if seg.is_some_and(|s| b.segs[s].delta.symbol.is_some()) => {
                // Placeholder of an inserted symbol (sprmCSymbol).
                let c = seg.and_then(|s| b.segs[s].delta.symbol_char());
                open.push(c.unwrap_or('\u{FFFD}'), seg);
            }
            _ => open.push(char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}'), seg),
        }
    }
    if !frames.is_empty() {
        for _ in &frames {
            Builder::issue(&mut b.counters.field_issues, "field not closed");
        }
    }
    if !open.text.is_empty() || !open.breaks.is_empty() || !open.fields.is_empty() {
        if b.story.main {
            b.counters.unterminated += 1;
        }
        b.finish(open, n.saturating_sub(1), 0x0D)?;
    }
    b.bk_open.clear();
    Ok(())
}

/// Turns a finished field into model items of the open paragraph.
fn close_field(b: &mut Builder<'_>, f: Frame, open: &mut Open) {
    let code = f.code.trim().to_owned();
    let kind = fields::kind_of(&code);
    if f.para != b.recs.len() {
        Builder::issue(&mut b.counters.field_issues, "field spans paragraphs");
        return;
    }
    let (start, end) = if f.in_result {
        (f.result_start, open.text.len())
    } else {
        (open.text.len(), open.text.len())
    };
    if !f.in_result
        && !matches!(
            kind,
            FieldKind::Page | FieldKind::NumPages | FieldKind::Date | FieldKind::Time
        )
    {
        Builder::issue(&mut b.counters.field_issues, "field without result");
        return;
    }
    let link = if kind == FieldKind::Hyperlink {
        fields::hyperlink(&code)
    } else {
        None
    };
    open.fields.push(PendField {
        start,
        end,
        kind,
        code,
        flt: f.flt,
        nested: f.nested,
        link,
    });
}
