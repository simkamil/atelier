// SPDX-License-Identifier: AGPL-3.0-or-later
//! Assembly of the rich document: main story text, special characters, fields,
//! character runs and paragraph properties.

use super::fib::{
    Fib, PAIR_CLX, PAIR_PLCFBTE_CHPX, PAIR_PLCFBTE_PAPX, PAIR_PLCFSED, PAIR_STSHF, PAIR_STTBF_FFN,
};
use super::fkp::{self, Fkp};
use super::sprm::{BaseToggles, CharDelta, ParaDelta, Stats};
use super::stsh::{self, Converted};
use super::text::{self, Clx};
use super::{
    DocError, DocFont, DocInfo, DocLimits, DocVersion, DocumentProperties, ImportNotice,
    ImportedBreak, ImportedDoc, ImportedRunStyle, TextRegion, fonts, rd_u16, rd_u32, summary,
};
use atelier_cfb::{CfbError, Compound};
use atelier_model::Transaction;
use atelier_text_model::rich::{CharRun, Node, NodeId, Paragraph, RichDocument, RichOp};
use atelier_unicode::segmentation::grapheme_boundaries;
use std::collections::BTreeMap;

const MAX_FIELD_DEPTH: usize = 64;
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

/// CPs at which sections end, as `end - 1` (the position of the section mark).
fn section_marks(table: &[u8], fc: usize, lcb: usize, max: usize) -> Vec<usize> {
    let Some(data) = fc
        .checked_add(lcb)
        .and_then(|e| table.get(fc..e))
        .filter(|d| d.len() >= 4)
    else {
        return Vec::new();
    };
    let n = ((data.len() - 4) / 16).min(max);
    let mut out: Vec<usize> = (1..=n)
        .filter_map(|i| rd_u32(data, i * 4))
        .filter_map(|cp| (cp as usize).checked_sub(1))
        .collect();
    out.sort_unstable();
    out
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

struct Open {
    text: String,
    spans: Vec<(usize, usize, usize)>,
    breaks: Vec<(usize, &'static str, Option<&'static str>)>,
}

impl Open {
    fn new() -> Self {
        Self {
            text: String::new(),
            spans: Vec::new(),
            breaks: Vec::new(),
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
    cells: usize,
    fields: usize,
    simple_prm: usize,
    bad_para_style: usize,
    bad_char_style: usize,
    bad_huge: usize,
    unterminated: usize,
    objects: BTreeMap<&'static str, usize>,
}

struct Builder<'a> {
    cfb: &'a Compound<'a>,
    word: &'a [u8],
    table: &'a [u8],
    clx: &'a Clx,
    papx: &'a [Fkp],
    segs: &'a [Seg],
    section_marks: &'a [usize],
    conv: &'a Converted,
    fonts: &'a [DocFont],
    limits: &'a DocLimits,
    stats: Stats,
    counters: Counters,
    data: Option<Option<Vec<u8>>>,
    document: RichDocument,
    section: NodeId,
    ops: Vec<RichOp>,
    inserted: usize,
    ids: Vec<NodeId>,
    breaks: Vec<ImportedBreak>,
    run_styles: Vec<ImportedRunStyle>,
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

    fn flush(&mut self) -> Result<(), DocError> {
        if self.ops.is_empty() {
            return Ok(());
        }
        let ops = std::mem::take(&mut self.ops);
        let t = Transaction::new(ops).map_err(Self::model_err)?;
        self.document.apply(&t).map_err(Self::model_err)?;
        Ok(())
    }

    /// Closes the open paragraph whose last character is at `end_cp`.
    fn finish(&mut self, open: Open, end_cp: usize, cell: bool) -> Result<(), DocError> {
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
        if cell {
            if delta.row_end && open.text.is_empty() {
                return Ok(());
            }
            self.counters.cells += 1;
        }
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
        let paragraph = Paragraph {
            text,
            runs,
            props,
            ..Paragraph::default()
        };
        let mut sub = self.document.new_paragraph("").map_err(Self::model_err)?;
        let id = sub.root;
        if let Some(Node::Paragraph(p)) = sub.nodes.get_mut(&id) {
            *p = paragraph;
        }
        self.ops.push(RichOp::InsertBlock {
            parent: Some(self.section),
            position: self.inserted + 1,
            subtree: sub,
        });
        self.inserted += 1;
        self.ids.push(id);
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
        if self.inserted > self.limits.max_paragraphs {
            return Err(DocError::Limit("paragraph count"));
        }
        if self.ops.len() >= BATCH {
            self.flush()?;
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
    let mut units: Vec<u16> = Vec::with_capacity(ccp_text.min(word.len()));
    for p in &clx.pieces {
        if p.cp_start >= ccp_text {
            break;
        }
        let end = p.cp_end.min(ccp_text);
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
    let (fc, lcb) = fib.pair(PAIR_PLCFSED);
    let marks = section_marks(&table, fc, lcb, limits.max_pieces);

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
    let section = *document
        .section_ids()
        .first()
        .ok_or(DocError::InvalidStructure("rich document has no section"))?;
    let mut b = Builder {
        cfb,
        word: &word,
        table: &table,
        clx: &clx,
        papx: &papx,
        segs: &segs,
        section_marks: &marks,
        conv: &conv,
        fonts: &font_list,
        limits,
        stats,
        counters: Counters {
            simple_prm,
            ..Counters::default()
        },
        data: None,
        document,
        section,
        ops: Vec::new(),
        inserted: 0,
        ids: Vec::new(),
        breaks: Vec::new(),
        run_styles: Vec::new(),
    };
    walk(&mut b, &units)?;
    b.flush()?;
    if b.inserted > 0 {
        // Drop the empty paragraph the document is created with.
        let t = Transaction::new(vec![RichOp::RemoveBlock {
            parent: Some(section),
            position: 0,
        }])
        .map_err(Builder::model_err)?;
        b.document.apply(&t).map_err(Builder::model_err)?;
    } else {
        b.ids = b.document.paragraph_ids();
    }
    b.document.validate().map_err(Builder::model_err)?;

    // Notices, in a stable order.
    for (region, n) in [
        (TextRegion::Footnotes, fib.ccp_ftn),
        (TextRegion::HeadersFooters, fib.ccp_hdd),
        (TextRegion::Comments, fib.ccp_atn),
        (TextRegion::Endnotes, fib.ccp_edn),
        (TextRegion::Textboxes, fib.ccp_txbx),
        (TextRegion::HeaderTextboxes, fib.ccp_hdr_txbx),
    ] {
        if n > 0 {
            notices.push(ImportNotice::IgnoredText {
                region,
                characters: n as usize,
            });
        }
    }
    let c = &b.counters;
    if c.cells > 0 {
        notices.push(ImportNotice::TableCellsFlattened { cells: c.cells });
    }
    if c.fields > 0 {
        notices.push(ImportNotice::FieldCodesDropped { fields: c.fields });
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
        paragraph_order: b.ids,
    })
}

/// Walks the main story, splitting it into paragraphs.
fn walk(b: &mut Builder<'_>, units: &[u16]) -> Result<(), DocError> {
    let n = units.len();
    let mut open = Open::new();
    let mut fields: Vec<bool> = Vec::new();
    let mut seg_cursor = 0usize;
    let mut i = 0usize;
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
        match u {
            0x0D => {
                b.finish(std::mem::replace(&mut open, Open::new()), cp, false)?;
                continue;
            }
            0x07 => {
                b.finish(std::mem::replace(&mut open, Open::new()), cp, true)?;
                continue;
            }
            0x13 => {
                b.counters.fields += 1;
                if fields.len() < MAX_FIELD_DEPTH {
                    fields.push(false);
                }
                continue;
            }
            0x14 => {
                if let Some(top) = fields.last_mut() {
                    *top = true;
                }
                continue;
            }
            0x15 => {
                fields.pop();
                continue;
            }
            _ => {}
        }
        if !fields.iter().all(|r| *r) {
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
                if b.section_marks.binary_search(&cp).is_ok() {
                    open.breaks.push((open.text.len(), "section", None));
                    b.finish(std::mem::replace(&mut open, Open::new()), cp, false)?;
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
                    Some(low) => {
                        i += 1;
                        let c =
                            0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
                        open.push(char::from_u32(c).unwrap_or('\u{FFFD}'), seg);
                    }
                    None => open.push('\u{FFFD}', seg),
                }
            }
            0xDC00..=0xDFFF => open.push('\u{FFFD}', seg),
            _ => open.push(char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}'), seg),
        }
    }
    if !open.text.is_empty() || !open.breaks.is_empty() {
        b.counters.unterminated += 1;
        b.finish(open, n - 1, false)?;
    }
    Ok(())
}
