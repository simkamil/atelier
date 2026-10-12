// SPDX-License-Identifier: AGPL-3.0-or-later
//! Style sheet (`STSH` = `Stshi` + `LPStd*`, [MS-DOC] 2.9.271, 2.9.272, 2.9.253)
//! and its conversion to the US-201 style engine. Table and numbering styles
//! are recognised and ignored; base-style cycles and invalid references are
//! cut with a notice, so style resolution can never loop.

use super::sprm::{BaseToggles, CharDelta, ParaDelta, Stats};
use super::{DocError, DocFont, DocLimits, DocStyleKind, ImportNotice, StyleRecord, rd_u16};
use atelier_text_model::rich::{
    CharAttrs, DocumentStyleDefaults, ParaProps, Style, StyleCatalog, StyleCharAttrs, StyleEngine,
    StyleKind, Theme,
};
use std::collections::BTreeSet;

/// `istdBase` / `istdNext` value meaning "none".
const ISTD_NONE: u16 = 0x0FFF;
const STDF_BASE_MIN: usize = 10;

/// A parsed `STD` before conversion; grpprls are ranges in the table stream.
#[derive(Clone, Debug)]
pub(crate) struct RawStd {
    pub sti: u16,
    pub sgc: u8,
    pub base: u16,
    pub next: u16,
    pub name: String,
    pub papx: Option<std::ops::Range<usize>>,
    pub chpx: Option<std::ops::Range<usize>>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Stsh {
    pub stds: Vec<Option<RawStd>>,
    /// `rgftcStandardChpStsh`: default ASCII, East Asian and other fonts.
    pub default_ftc: [Option<u16>; 3],
    pub damaged: usize,
}

fn utf16(b: &[u8]) -> String {
    char::decode_utf16(b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])))
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

pub(crate) fn parse(
    table: &[u8],
    fc: usize,
    lcb: usize,
    max_styles: usize,
) -> Result<Stsh, DocError> {
    let mut out = Stsh::default();
    if lcb == 0 {
        return Ok(out);
    }
    let end =
        fc.checked_add(lcb)
            .filter(|e| *e <= table.len())
            .ok_or(DocError::InvalidStructure(
                "style sheet outside the table stream",
            ))?;
    let cb_stshi = rd_u16(table, fc).ok_or(DocError::InvalidStructure("truncated Stshi"))? as usize;
    let stshi = table
        .get(fc + 2..fc + 2 + cb_stshi)
        .filter(|_| fc + 2 + cb_stshi <= end)
        .ok_or(DocError::InvalidStructure("Stshi beyond the style sheet"))?;
    if stshi.len() < 4 {
        return Err(DocError::InvalidStructure("Stshi too small"));
    }
    let cstd = rd_u16(stshi, 0).unwrap_or(0) as usize;
    let cb_base = rd_u16(stshi, 2).unwrap_or(0) as usize;
    if cstd > max_styles {
        return Err(DocError::Limit("style count"));
    }
    for (slot, at) in [12usize, 14, 16].into_iter().enumerate() {
        out.default_ftc[slot] = rd_u16(stshi, at);
    }
    let mut pos = fc + 2 + cb_stshi;
    out.stds.reserve(cstd.min((end - pos) / 2 + 1));
    for _ in 0..cstd {
        let Some(cb) = rd_u16(table, pos).filter(|_| pos + 2 <= end) else {
            out.damaged += 1;
            break;
        };
        let cb = cb as usize;
        let start = pos + 2;
        if cb == 0 {
            out.stds.push(None);
            pos = start;
            continue;
        }
        if start + cb > end {
            out.damaged += 1;
            break;
        }
        match parse_std(table, start, start + cb, cb_base) {
            Some(std) => out.stds.push(Some(std)),
            None => {
                out.damaged += 1;
                out.stds.push(None);
            }
        }
        pos = start + cb;
    }
    Ok(out)
}

fn parse_std(table: &[u8], start: usize, end: usize, cb_base: usize) -> Option<RawStd> {
    if cb_base < STDF_BASE_MIN || start + cb_base + 2 > end {
        return None;
    }
    let w0 = rd_u16(table, start)?;
    let w1 = rd_u16(table, start + 2)?;
    let w2 = rd_u16(table, start + 4)?;
    let sgc = (w1 & 0x000F) as u8;
    let base = w1 >> 4;
    let cupx = (w2 & 0x000F) as usize;
    let next = w2 >> 4;
    let cch = rd_u16(table, start + cb_base)? as usize;
    let name_end = start + cb_base + 2 + cch * 2;
    let name = utf16(
        table
            .get(start + cb_base + 2..name_end)
            .filter(|_| name_end <= end)?,
    );
    // Terminator, then padding up to an even offset inside the STD.
    let mut pos = name_end + 2;
    if (pos - start) % 2 == 1 {
        pos += 1;
    }
    let mut upx = Vec::new();
    for _ in 0..cupx {
        let Some(cb) = rd_u16(table, pos).filter(|_| pos + 2 <= end) else {
            break;
        };
        let (a, b) = (pos + 2, pos + 2 + cb as usize);
        if b > end {
            break;
        }
        upx.push(a..b);
        pos = b + (cb as usize % 2);
    }
    let (papx, chpx) = match sgc {
        // Paragraph: UpxPapx (istd + grpprl), then UpxChpx.
        1 => (
            upx.first()
                .filter(|r| r.len() >= 2)
                .map(|r| r.start + 2..r.end),
            upx.get(1).cloned(),
        ),
        2 => (None, upx.first().cloned()),
        _ => (None, None),
    };
    Some(RawStd {
        sti: w0 & 0x0FFF,
        sgc,
        base,
        next,
        name,
        papx,
        chpx,
    })
}

/// Identifier used by the model for the built-in styles that are shared with
/// the docx importer; `None` for the others (name-derived identifiers).
fn builtin_id(sti: u16) -> Option<String> {
    let fixed = match sti {
        0 => "Normal",
        1..=9 => return Some(format!("Heading{sti}")),
        29 => "FootnoteText",
        31 => "Header",
        32 => "Footer",
        34 => "Caption",
        38 => "FootnoteReference",
        62 => "Title",
        65 => "DefaultParagraphFont",
        66 => "BodyText",
        74 => "Subtitle",
        85 => "Hyperlink",
        86 => "FollowedHyperlink",
        87 => "Strong",
        88 => "Emphasis",
        _ => return None,
    };
    Some(fixed.to_owned())
}

fn name_id(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

/// Result of converting a style sheet.
pub(crate) struct Converted {
    pub engine: StyleEngine,
    pub records: Vec<StyleRecord>,
    /// Model identifier by `istd` for paragraph and character styles.
    pub ids: Vec<Option<String>>,
    kinds: Vec<Option<DocStyleKind>>,
    bases: Vec<Option<usize>>,
    deltas: Vec<CharDelta>,
}

impl Converted {
    pub fn paragraph_id(&self, istd: usize) -> Option<&str> {
        match self.kinds.get(istd) {
            Some(Some(DocStyleKind::Paragraph)) => self.ids[istd].as_deref(),
            _ => None,
        }
    }
    pub fn character_id(&self, istd: usize) -> Option<&str> {
        match self.kinds.get(istd) {
            Some(Some(DocStyleKind::Character)) => self.ids[istd].as_deref(),
            _ => None,
        }
    }

    fn chain(&self, istd: usize) -> Vec<usize> {
        let mut chain = Vec::new();
        let mut cur = Some(istd);
        while let Some(i) = cur {
            if chain.len() > self.bases.len() || i >= self.bases.len() {
                break;
            }
            chain.push(i);
            cur = self.bases[i];
        }
        chain.reverse();
        chain
    }

    fn fold(&self, mut t: BaseToggles, istd: usize) -> BaseToggles {
        for i in self.chain(istd) {
            let d = &self.deltas[i];
            t.bold = d.bold.resolve(t.bold);
            t.italic = d.italic.resolve(t.italic);
            t.strike = d.strike.resolve(t.strike);
        }
        t
    }

    /// Effective toggles inherited by a run: paragraph style chain, then the
    /// character style chain.
    pub fn base_toggles(&self, para: Option<usize>, chr: Option<usize>) -> BaseToggles {
        let mut t = BaseToggles::default();
        if let Some(p) = para {
            t = self.fold(t, p);
        }
        if let Some(c) = chr {
            t = self.fold(t, c);
        }
        t
    }
}

pub(crate) fn convert(
    stsh: &Stsh,
    table: &[u8],
    fonts: &[DocFont],
    stats: &mut Stats,
    notices: &mut Vec<ImportNotice>,
    limits: &DocLimits,
) -> Result<Converted, DocError> {
    let n = stsh.stds.len();
    if n > limits.max_styles {
        return Err(DocError::Limit("style count"));
    }
    let mut kinds: Vec<Option<DocStyleKind>> = vec![None; n];
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut ids: Vec<Option<String>> = vec![None; n];
    // Identifiers of built-in styles are reserved first so that a custom style
    // named like one cannot take its place.
    for (i, s) in stsh.stds.iter().enumerate() {
        let Some(s) = s else { continue };
        kinds[i] = Some(match s.sgc {
            1 => DocStyleKind::Paragraph,
            2 => DocStyleKind::Character,
            3 => DocStyleKind::Table,
            _ => DocStyleKind::Numbering,
        });
        if matches!(
            kinds[i],
            Some(DocStyleKind::Table | DocStyleKind::Numbering)
        ) {
            notices.push(ImportNotice::UnsupportedStyleKind {
                istd: i as u16,
                kind: kinds[i].unwrap_or(DocStyleKind::Table),
            });
            continue;
        }
        if let Some(id) = builtin_id(s.sti) {
            if used.insert(id.clone()) {
                ids[i] = Some(id);
            }
        }
    }
    for (i, s) in stsh.stds.iter().enumerate() {
        let Some(s) = s else { continue };
        if ids[i].is_some()
            || !matches!(
                kinds[i],
                Some(DocStyleKind::Paragraph | DocStyleKind::Character)
            )
        {
            continue;
        }
        let mut id = name_id(&s.name);
        if id.is_empty() {
            id = format!("Style{i}");
        }
        if !used.insert(id.clone()) {
            id = format!("{id}_{i}");
            used.insert(id.clone());
        }
        ids[i] = Some(id);
    }
    // Base styles: must exist, be supported and have the same kind.
    let mut bases: Vec<Option<usize>> = vec![None; n];
    for (i, s) in stsh.stds.iter().enumerate() {
        let Some(s) = s else { continue };
        if ids[i].is_none() || s.base == ISTD_NONE {
            continue;
        }
        let b = s.base as usize;
        if b < n && b != i && ids[b].is_some() && kinds[b] == kinds[i] {
            bases[i] = Some(b);
        } else {
            notices.push(ImportNotice::InvalidStyleReference {
                istd: i as u16,
                reason: "base style missing, unsupported or of another kind",
            });
        }
    }
    // Cycle cut (iterative colouring).
    let mut state = vec![0u8; n];
    for start in 0..n {
        if state[start] != 0 {
            continue;
        }
        let mut path = Vec::new();
        let mut cur = start;
        loop {
            state[cur] = 1;
            path.push(cur);
            match bases[cur] {
                Some(b) if state[b] == 1 => {
                    bases[cur] = None;
                    notices.push(ImportNotice::InvalidStyleReference {
                        istd: cur as u16,
                        reason: "base style cycle",
                    });
                    break;
                }
                Some(b) if state[b] == 0 => cur = b,
                _ => break,
            }
        }
        for p in path {
            state[p] = 2;
        }
    }
    // Deltas, in table order.
    let mut deltas = vec![CharDelta::default(); n];
    let mut para_deltas = vec![ParaDelta::default(); n];
    for (i, s) in stsh.stds.iter().enumerate() {
        let Some(s) = s else { continue };
        if ids[i].is_none() {
            continue;
        }
        if let Some(r) = s.chpx.clone().and_then(|r| table.get(r)) {
            deltas[i].apply_all(r, stats);
            deltas[i].istd = None;
        }
        if let Some(r) = s.papx.clone().and_then(|r| table.get(r)) {
            para_deltas[i].apply_all(r, stats);
        }
    }
    let default_font = stsh.default_ftc[0]
        .and_then(|f| fonts.get(f as usize))
        .map(|f| f.name.clone())
        .filter(|f| !f.is_empty());
    let defaults = DocumentStyleDefaults {
        character: StyleCharAttrs {
            values: CharAttrs {
                font: default_font,
                size_half_points: Some(20),
                ..CharAttrs::default()
            },
            ..StyleCharAttrs::default()
        },
        paragraph: ParaProps::default(),
        table_style: None,
    };
    let mut engine = StyleEngine::new(StyleCatalog::new(defaults), Theme::default(), 1024);
    let mut conv = Converted {
        engine: engine.clone(),
        records: Vec::new(),
        ids,
        kinds,
        bases,
        deltas,
    };
    for (i, s) in stsh.stds.iter().enumerate() {
        let Some(s) = s else { continue };
        let kind = conv.kinds[i].unwrap_or(DocStyleKind::Paragraph);
        conv.records.push(StyleRecord {
            istd: i as u16,
            sti: s.sti,
            name: s.name.clone(),
            kind,
            id: conv.ids[i].clone(),
            based_on: conv.bases[i].map(|b| b as u16),
            next: (s.next != ISTD_NONE
                && (s.next as usize) < n
                && conv.ids[s.next as usize].is_some())
            .then_some(s.next),
        });
        let Some(id) = conv.ids[i].clone() else {
            continue;
        };
        let base_toggles = conv.bases[i].map_or(BaseToggles::default(), |b| {
            conv.fold(BaseToggles::default(), b)
        });
        let values = conv.deltas[i].to_attrs(base_toggles, fonts, stats);
        let mut paragraph = if kind == DocStyleKind::Paragraph {
            para_deltas[i].to_props(stats)
        } else {
            ParaProps::default()
        };
        paragraph.style = None;
        let style = Style {
            id,
            kind: if kind == DocStyleKind::Paragraph {
                StyleKind::Paragraph
            } else {
                StyleKind::Character
            },
            based_on: conv.bases[i].and_then(|b| conv.ids[b].clone()),
            linked_style: None,
            character: StyleCharAttrs {
                values,
                ..StyleCharAttrs::default()
            },
            paragraph,
        };
        engine
            .set_style(style)
            .map_err(|e| DocError::Style(e.to_string()))?;
    }
    conv.engine = engine;
    Ok(conv)
}
