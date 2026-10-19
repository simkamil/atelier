// SPDX-License-Identifier: AGPL-3.0-or-later
//! `FormatState` (F0): the formatting of the selection or the caret, for the
//! ribbon buttons, the font and size boxes and the styles gallery. Written
//! once for US-240 (characters), US-243 (paragraphs) and US-250 (styles);
//! contract in `docs/us-240-format-protocol.md`.
//!
//! Every value is the **resolved** one (document defaults, styles and direct
//! formatting), as Word shows it. A caret shows the character before it (the
//! one after it at the start of a paragraph) with the typing attributes
//! applied; a selection shows the uniform value of every character it covers
//! or "mixed". The paragraph part covers every paragraph the selection
//! touches (not the one a selection ends at the very start of).

use atelier_text_model::rich::flat::{FlatIndex, LayoutContext};
use atelier_text_model::rich::{
    Alignment, CharAttrs, LineRule, ParaBorder, ParaBorderStyle, ParaProps, Underline,
    VerticalAlign,
};

use std::collections::BTreeMap;

use atelier_text_model::rich::ThemeFont;

use atelier_text_model::rich::RichDocument;

use super::Session;
use crate::rich_protocol::{
    BORDER_SIDES, BorderLine, CharState, ErrorCode, FontEntry, FontListData, FontState,
    FormatStateData, MAX_BORDER_EIGHTHS, MAX_BORDER_SPACE, MAX_FONT_LIST, MAX_HALF_POINTS,
    MAX_NAME_BYTES, MAX_STATE_RUNS, MAX_TWIPS, MIN_HALF_POINTS, NO_REVISION_GUARD, ParaState,
    Response, Rgb, Tri,
};

/// Word fonts the engine draws with a metric-compatible family (the aliases of
/// `atelier_font::manager`).
const METRIC_ALIASES: [&str; 5] = [
    "Calibri",
    "Cambria",
    "Arial",
    "Times New Roman",
    "Courier New",
];
/// Families of the built-in catalog (`atelier_font::embedded`), named here and
/// not read from it: referencing the catalog would link every face into the WASM.
const CATALOG_FAMILIES: [&str; 5] = [
    "Carlito",
    "Caladea",
    "Liberation Sans",
    "Liberation Serif",
    "Liberation Mono",
];
/// Paragraphs `ListFonts` reads to find the fonts the document names.
const FONT_SCAN_PARAGRAPHS: usize = 200_000;
const FONT_BOLD: u8 = 1;
const FONT_ITALIC: u8 = 2;
const FONT_USED: u8 = 4;
const FONT_SUBSTITUTED: u8 = 8;

/// What the samples seen so far agree on.
enum Acc<T> {
    Empty,
    One(Option<T>),
    Mixed,
}

impl<T: PartialEq> Acc<T> {
    fn add(&mut self, value: Option<T>) {
        match self {
            Acc::Empty => *self = Acc::One(value),
            Acc::One(current) if *current == value => {}
            Acc::One(_) => *self = Acc::Mixed,
            Acc::Mixed => {}
        }
    }

    fn tri(self) -> Tri<T> {
        match self {
            Acc::Empty | Acc::One(None) => Tri::Unset,
            Acc::One(Some(v)) => Tri::Value(v),
            Acc::Mixed => Tri::Mixed,
        }
    }
}

// Not derived: a derive would require `T: Default`.
#[allow(clippy::derivable_impls)]
impl<T> Default for Acc<T> {
    fn default() -> Self {
        Acc::Empty
    }
}

/// A name as the wire carries it: control characters removed, cut to
/// `MAX_NAME_BYTES` on a UTF-8 boundary; `None` when nothing is left (a DOCX
/// can hold a longer or odd name, and the reply must stay encodable).
pub(super) fn clean_name(name: &str) -> Option<String> {
    let mut cleaned: String = name.chars().filter(|c| !c.is_control()).collect();
    if cleaned.len() > MAX_NAME_BYTES {
        let mut cut = MAX_NAME_BYTES;
        while !cleaned.is_char_boundary(cut) {
            cut -= 1;
        }
        cleaned.truncate(cut);
    }
    (!cleaned.is_empty()).then_some(cleaned)
}

/// The word the formatting of a caret at `position` targets (Word's rule,
/// shared by `FormatState` and the handler of `ApplyCharFormat`): the segment
/// that starts at or contains the position, only if it holds a letter or a
/// digit. Blanks, punctuation, objects and the paragraph mark are no word, and
/// the end of a word is not in it (`word|` and `word |` target the typing
/// attributes, `|word` the word).
pub(super) fn word_under_caret(doc: &RichDocument, position: usize) -> Option<(usize, usize)> {
    let (start, end) = doc.word_at(position).ok()??;
    if position < start || position >= end {
        return None;
    }
    let text = doc.text(start, end).ok()?;
    (text.chars().any(char::is_alphanumeric) && !text.contains(['\n', '\u{FFFC}']))
        .then_some((start, end))
}

#[derive(Default)]
struct CharAcc {
    font: Acc<String>,
    size: Acc<u16>,
    bold: Acc<bool>,
    italic: Acc<bool>,
    underline: Acc<u8>,
    strike: Acc<bool>,
    vertical_align: Acc<u8>,
    color: Acc<Rgb>,
    highlight: Acc<Rgb>,
    style: Acc<String>,
    /// First character shown, to ask which face draws the font.
    sample: Option<char>,
}

fn rgb(c: atelier_text_model::rich::Color) -> Rgb {
    Rgb {
        r: c.r,
        g: c.g,
        b: c.b,
    }
}

impl CharAcc {
    /// One run: `direct` its direct attributes, `resolved` the effective ones.
    fn add(&mut self, direct: &CharAttrs, resolved: &CharAttrs) {
        self.font.add(resolved.font.as_deref().and_then(clean_name));
        self.size.add(
            resolved
                .size_half_points
                .map(|v| v.clamp(u32::from(MIN_HALF_POINTS), u32::from(MAX_HALF_POINTS)) as u16),
        );
        self.bold.add(Some(resolved.bold.unwrap_or(false)));
        self.italic.add(Some(resolved.italic.unwrap_or(false)));
        self.underline.add(Some(match resolved.underline {
            None | Some(Underline::None) => 0,
            Some(Underline::Single) => 1,
            Some(Underline::Double) => 2,
            Some(Underline::Thick) => 3,
            Some(Underline::Dotted) => 4,
            Some(Underline::Dashed) => 5,
            Some(Underline::Wave) => 6,
        }));
        self.strike.add(Some(resolved.strike.unwrap_or(false)));
        self.vertical_align.add(Some(match resolved.vertical_align {
            None | Some(VerticalAlign::Baseline) => 0,
            Some(VerticalAlign::Superscript) => 1,
            Some(VerticalAlign::Subscript) => 2,
        }));
        self.color.add(resolved.color.map(rgb));
        self.highlight.add(resolved.highlight.map(rgb));
        self.style
            .add(direct.char_style.as_deref().and_then(clean_name));
    }
}

#[derive(Default)]
struct ParaAcc {
    alignment: Acc<u8>,
    indent_left: Acc<i32>,
    indent_right: Acc<i32>,
    first_line: Acc<i32>,
    space_before: Acc<u32>,
    space_after: Acc<u32>,
    line: Acc<(u8, u32)>,
    style: Acc<String>,
    shading: Acc<Rgb>,
    borders: [Acc<BorderLine>; BORDER_SIDES],
}

fn twips(v: i32) -> i32 {
    v.clamp(-MAX_TWIPS, MAX_TWIPS)
}

impl ParaAcc {
    fn add(&mut self, resolved: &ParaProps, direct: &ParaProps) {
        self.alignment.add(Some(match resolved.alignment {
            None | Some(Alignment::Left) => 0,
            Some(Alignment::Center) => 1,
            Some(Alignment::Right) => 2,
            Some(Alignment::Justify) => 3,
        }));
        self.indent_left
            .add(Some(twips(resolved.indent_left.unwrap_or(0))));
        self.indent_right
            .add(Some(twips(resolved.indent_right.unwrap_or(0))));
        self.first_line
            .add(Some(twips(resolved.indent_first_line.unwrap_or(0))));
        self.space_before.add(Some(
            resolved.space_before.unwrap_or(0).min(MAX_TWIPS as u32),
        ));
        self.space_after.add(Some(
            resolved.space_after.unwrap_or(0).min(MAX_TWIPS as u32),
        ));
        self.line.add(Some(match resolved.line_spacing {
            None => (0, 240),
            Some(l) => (
                match l.rule {
                    LineRule::Auto => 0,
                    LineRule::Exact => 1,
                    LineRule::AtLeast => 2,
                },
                l.value,
            ),
        }));
        self.style.add(direct.style.as_deref().and_then(clean_name));
        self.shading.add(
            resolved
                .shading
                .as_ref()
                .and_then(|s| s.paint_color())
                .map(|c| Rgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                }),
        );
        for (acc, side) in self.borders.iter_mut().zip(resolved.borders.sides()) {
            acc.add(side.filter(ParaBorder::is_line).map(|b| BorderLine {
                style: match b.style {
                    ParaBorderStyle::Double => 1,
                    ParaBorderStyle::Dotted => 2,
                    ParaBorderStyle::Dashed => 3,
                    _ => 0,
                },
                eighths: b.eighths.clamp(2, MAX_BORDER_EIGHTHS),
                space: b.space.min(MAX_BORDER_SPACE),
                color: b.color.map(|c| Rgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                }),
            }));
        }
    }
}

impl Session {
    /// First check of every changing formatting command: a read-only document
    /// (revisions) is `RICH_READ_ONLY`, a plain-text one `RICH_FORMAT_PLAIN_TEXT`.
    /// The handlers of US-240, US-243 and US-250 call it before anything else.
    pub(super) fn deny_formatting(&self) -> Result<(), ErrorCode> {
        self.deny_read_only()?;
        if self.txt {
            return Err(ErrorCode::FormatPlainText);
        }
        Ok(())
    }

    /// First steps of every changing formatting command, in the order of the
    /// contract: read-only, plain text, **repeat** (`command_id` equal to the
    /// last accepted one: `RICH_DUPLICATE_COMMAND`), then the revision guard.
    pub(super) fn begin_format(
        &self,
        base_revision: u64,
        command_id: u32,
    ) -> Result<(), ErrorCode> {
        self.deny_formatting()?;
        if command_id != 0 && command_id == self.last_command_id {
            return Err(ErrorCode::DuplicateCommand);
        }
        if base_revision != NO_REVISION_GUARD && base_revision != self.revision() {
            return Err(ErrorCode::StaleRevision);
        }
        Ok(())
    }

    /// Records that the command was accepted (applied, or found to change
    /// nothing, typing attributes included): `FormatState.last_command_id`.
    /// A refused command is never recorded.
    #[allow(dead_code)] // called by the handlers of US-240, US-243 and US-250
    pub(super) fn note_command(&mut self, command_id: u32) {
        if command_id != 0 {
            self.last_command_id = command_id;
        }
    }

    /// `FormatState`: the state of the selection, see the module documentation.
    pub(super) fn format_state(&mut self) -> Result<Response, ErrorCode> {
        let (low, high) = self.selection_range();
        let collapsed = low == high;
        let mut chars = CharAcc::default();
        let mut paras = ParaAcc::default();
        let mut runs = 0usize;
        let mut partial = false;
        // A caret in a word describes (and formats) that word; elsewhere the
        // typing attributes, else the character before the caret.
        let word = if collapsed {
            word_under_caret(&self.doc, low)
        } else {
            None
        };
        let typing = collapsed && word.is_none() && self.typing.armed;
        {
            let doc = &self.doc;
            let first = doc.locate(low)?;
            let last = doc.locate(high)?;
            let mut last_index = last.paragraph;
            // A selection that ends at the very start of a paragraph does not touch it.
            if !collapsed && last_index > first.paragraph && last.offset == 0 {
                last_index -= 1;
            }
            let sample = |index: usize, direct: &CharAttrs, chars: &mut CharAcc| match doc
                .resolved_run(index, direct)
            {
                Ok(resolved) => chars.add(direct, &resolved),
                Err(_) => chars.add(direct, direct),
            };
            'paragraphs: for index in first.paragraph..=last_index {
                let Some(p) = LayoutContext::paragraph(doc, index) else {
                    continue;
                };
                // A style the catalog lacks (an import can reference one) falls
                // back to the direct values instead of hiding the paragraph.
                match doc.resolved(index) {
                    Ok(resolved) => paras.add(&resolved.paragraph, &p.props),
                    Err(_) => paras.add(&p.props, &p.props),
                }
                let flat_len = doc.paragraph_flat(index)?.len();
                let (mut start_flat, mut end_flat) = (
                    if index == first.paragraph {
                        first.offset
                    } else {
                        0
                    },
                    if index == last.paragraph {
                        last.offset
                    } else {
                        flat_len
                    },
                );
                if let Some((word_start, word_end)) = word {
                    let paragraph_start = doc.paragraph_start(index)?;
                    start_flat = word_start - paragraph_start;
                    end_flat = word_end - paragraph_start;
                }
                let from = doc.flat_to_text(index, start_flat)?;
                let to = doc.flat_to_text(index, end_flat)?;
                if collapsed && word.is_none() {
                    // The character before the caret, else the one after it.
                    let probe = if from > 0 {
                        p.text[..from].char_indices().next_back().map(|(i, _)| i)
                    } else {
                        None
                    };
                    let mut direct = match probe {
                        Some(i) => p.attrs_at(i),
                        None if !p.text.is_empty() => p.attrs_at(0),
                        None => CharAttrs::default(),
                    };
                    chars.sample = chars.sample.or_else(|| {
                        p.text
                            .get(probe.unwrap_or(0)..)
                            .and_then(|t| t.chars().next())
                    });
                    if typing {
                        direct.clear(&self.typing.clear);
                        direct.overlay(&self.typing.set);
                    }
                    sample(index, &direct, &mut chars);
                    continue;
                }
                let mut at = from;
                let mut pieces: Vec<CharAttrs> = Vec::new();
                for run in p.runs.iter().filter(|r| r.end > from && r.start < to) {
                    if run.start > at {
                        pieces.push(CharAttrs::default());
                    }
                    pieces.push(run.attrs.clone());
                    at = run.end.min(to);
                }
                if at < to {
                    pieces.push(CharAttrs::default());
                }
                // An empty paragraph inside the selection shows its own mark.
                if p.text.is_empty() {
                    pieces.push(CharAttrs::default());
                }
                chars.sample = chars
                    .sample
                    .or_else(|| p.text.get(from..).and_then(|t| t.chars().next()));
                for direct in pieces {
                    runs += 1;
                    if runs > MAX_STATE_RUNS {
                        partial = true;
                        break 'paragraphs;
                    }
                    sample(index, &direct, &mut chars);
                }
            }
        }
        let font = self.font_state(&chars);
        let CharAcc {
            size,
            bold,
            italic,
            underline,
            strike,
            vertical_align,
            color,
            highlight,
            style,
            ..
        } = chars;
        let state = FormatStateData {
            revision: self.revision(),
            anchor: self.anchor as u64,
            focus: self.focus as u64,
            last_command_id: self.last_command_id,
            collapsed,
            read_only: self.origin.read_only,
            partial,
            typing,
            plain: self.txt,
            chars: CharState {
                font,
                half_points: size.tri(),
                bold: bold.tri(),
                italic: italic.tri(),
                underline: underline.tri(),
                strike: strike.tri(),
                vertical_align: vertical_align.tri(),
                color: color.tri(),
                highlight: highlight.tri(),
                style: style.tri(),
            },
            paragraph: ParaState {
                alignment: paras.alignment.tri(),
                indent_left: paras.indent_left.tri(),
                indent_right: paras.indent_right.tri(),
                first_line: paras.first_line.tri(),
                space_before: paras.space_before.tri(),
                space_after: paras.space_after.tri(),
                line_spacing: paras.line.tri(),
                shading: paras.shading.tri(),
                borders: paras.borders.map(Acc::tri),
                style: paras.style.tri(),
            },
        };
        Ok(Response::FormatState(Box::new(state)))
    }

    /// `ListFonts`: the families the engine draws, the Word names it draws
    /// with a metric-compatible family, and the fonts the document names (those
    /// the engine lacks are marked substituted), with the heading and body fonts
    /// of the theme. Availability is probed on the font manager of the layout, so
    /// the list follows the faces the editor really registers.
    pub(super) fn list_fonts(&mut self) -> Result<Response, ErrorCode> {
        // The answer only depends on the document: memoised by revision, so a
        // font box that opens again does not read the document again.
        if let Some((revision, list)) = &self.fonts_memo {
            if *revision == self.revision() {
                return Ok(Response::FontList(list.clone()));
            }
        }
        let mut used_by_document: BTreeMap<String, String> = BTreeMap::new();
        {
            let doc = &self.doc;
            for index in 0..doc.paragraph_count().min(FONT_SCAN_PARAGRAPHS) {
                let Some(p) = LayoutContext::paragraph(doc, index) else {
                    continue;
                };
                let base = doc.resolved(index).ok().and_then(|r| r.character.font);
                let runs = p.runs.iter().filter_map(|r| r.attrs.font.clone());
                for font in base.into_iter().chain(runs) {
                    let Some(font) = clean_name(&font) else {
                        continue;
                    };
                    used_by_document
                        .entry(font.to_ascii_lowercase())
                        .or_insert(font);
                }
            }
        }
        // Candidates: the catalog families, the Word aliases, the document fonts.
        let mut candidates: BTreeMap<String, String> = BTreeMap::new();
        for family in CATALOG_FAMILIES {
            candidates
                .entry(family.to_ascii_lowercase())
                .or_insert_with(|| (*family).to_owned());
        }
        for alias in METRIC_ALIASES {
            candidates
                .entry(alias.to_ascii_lowercase())
                .or_insert_with(|| alias.to_owned());
        }
        for (key, font) in &used_by_document {
            candidates
                .entry(key.clone())
                .or_insert_with(|| font.clone());
        }
        let mut families = Vec::new();
        for (key, name) in candidates {
            if families.len() >= MAX_FONT_LIST {
                break;
            }
            let used = used_by_document.contains_key(&key);
            let probe = |bold, italic| self.cache.used_face(&name, bold, italic, 'a');
            let Some(regular) = probe(false, false) else {
                continue;
            };
            let substituted = !regular.family.eq_ignore_ascii_case(&name);
            let alias = METRIC_ALIASES.iter().any(|a| a.eq_ignore_ascii_case(&name));
            // A catalog family the editor does not register is no choice to offer.
            if substituted && !alias && !used {
                continue;
            }
            let has_bold = probe(true, false)
                .is_some_and(|f| f.bold && f.family.eq_ignore_ascii_case(&regular.family));
            let has_italic = probe(false, true)
                .is_some_and(|f| f.italic && f.family.eq_ignore_ascii_case(&regular.family));
            families.push(FontEntry {
                name,
                flags: (if has_bold { FONT_BOLD } else { 0 })
                    | (if has_italic { FONT_ITALIC } else { 0 })
                    | (if used { FONT_USED } else { 0 })
                    | (if substituted { FONT_SUBSTITUTED } else { 0 }),
                substitute: if substituted {
                    regular.family
                } else {
                    String::new()
                },
            });
        }
        let theme = LayoutContext::theme(&self.doc);
        let theme_font = |slot| theme.fonts.get(&slot).cloned().unwrap_or_default();
        let list = FontListData {
            revision: self.revision(),
            theme_major: clean_name(&theme_font(ThemeFont::Major)).unwrap_or_default(),
            theme_minor: clean_name(&theme_font(ThemeFont::Minor)).unwrap_or_default(),
            families,
        };
        self.fonts_memo = Some((self.revision(), list.clone()));
        Ok(Response::FontList(list))
    }

    /// The font of the selection and the family the engine draws it with.
    fn font_state(&self, chars: &CharAcc) -> Tri<FontState> {
        let requested = match &chars.font {
            Acc::Empty | Acc::One(None) => return Tri::Unset,
            Acc::Mixed => return Tri::Mixed,
            Acc::One(Some(font)) => font.clone(),
        };
        let uniform = |acc: &Acc<bool>| matches!(acc, Acc::One(Some(true)));
        let used = self
            .cache
            .used_face(
                &requested,
                uniform(&chars.bold),
                uniform(&chars.italic),
                chars.sample.unwrap_or('a'),
            )
            .map_or_else(|| requested.clone(), |face| face.family);
        let substituted = !used.eq_ignore_ascii_case(&requested);
        Tri::Value(FontState {
            requested,
            used,
            substituted,
        })
    }
}
