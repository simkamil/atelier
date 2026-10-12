// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Prl`/`Sprm` decoding ([MS-DOC] 2.6.1, 2.9.253, 2.9.254) and the mapping of
//! the supported character and paragraph sprms to the rich text model.
//!
//! Operand sizes come from `spra`, so unknown sprms are skipped without
//! losing synchronisation. Properties the model cannot express are kept as
//! opaque attributes (namespace [`OPAQUE_NS`]) and counted in [`Stats`].

use super::DocFont;
use atelier_text_model::rich::{
    Alignment, CharAttrs, Color, LineRule, LineSpacing, OpaqueAttr, ParaProps, Underline,
    VerticalAlign,
};
use std::collections::BTreeMap;

/// Namespace of the opaque attributes written by this importer.
pub const OPAQUE_NS: &str = "urn:atelier:doc";

// Character sprms.
const SPRM_C_FBOLD: u16 = 0x0835;
const SPRM_C_FITALIC: u16 = 0x0836;
const SPRM_C_FSTRIKE: u16 = 0x0837;
const SPRM_C_FOUTLINE: u16 = 0x0838;
const SPRM_C_FSHADOW: u16 = 0x0839;
const SPRM_C_FSMALLCAPS: u16 = 0x083A;
const SPRM_C_FCAPS: u16 = 0x083B;
const SPRM_C_FVANISH: u16 = 0x083C;
const SPRM_C_FIMPRINT: u16 = 0x0854;
const SPRM_C_FEMBOSS: u16 = 0x0858;
const SPRM_C_KUL: u16 = 0x2A3E;
const SPRM_C_HPS: u16 = 0x4A43;
const SPRM_C_ISS: u16 = 0x2A48;
const SPRM_C_ICO: u16 = 0x2A42;
const SPRM_C_CV: u16 = 0x6870;
const SPRM_C_HIGHLIGHT: u16 = 0x2A0C;
const SPRM_C_RGFTC0: u16 = 0x4A4F;
const SPRM_C_RGFTC1: u16 = 0x4A50;
const SPRM_C_RGFTC2: u16 = 0x4A51;
const SPRM_C_FTCDEFAULT: u16 = 0x4A3D;
const SPRM_C_ISTD: u16 = 0x4A30;
const SPRM_C_DXASPACE: u16 = 0x8840;
const SPRM_C_FDSTRIKE: u16 = 0x2A53;
const SPRM_C_PLAIN: u16 = 0x2A33;
const SPRM_C_HPSPOS: u16 = 0x4845;
const SPRM_C_SYMBOL: u16 = 0x6A09;
const SPRM_C_SHD80: u16 = 0x4866;
const SPRM_C_SHD: u16 = 0xCA71;
/// Known sprms ignored without a notice (pictures, symbols, proofing,
/// complex-script variants, revision ids).
const C_SILENT: [u16; 19] = [
    0x0855, 0x6A03, 0x0806, 0x080A, 0x0875, 0x085C, 0x085D, 0x4A5E, 0x485F, 0x4A60, 0x4A61, 0x486D,
    0x486E, 0x6815, 0x6816, 0x6817, 0x486B, 0x4873, 0x4874,
];

// Paragraph sprms.
const SPRM_P_ISTD: u16 = 0x4600;
const SPRM_P_JC80: u16 = 0x2403;
const SPRM_P_JC: u16 = 0x2461;
const SPRM_P_FKEEP: u16 = 0x2405;
const SPRM_P_FKEEPFOLLOW: u16 = 0x2406;
const SPRM_P_FPAGEBREAKBEFORE: u16 = 0x2407;
const SPRM_P_FWIDOWCONTROL: u16 = 0x2431;
const SPRM_P_OUTLVL: u16 = 0x2640;
const SPRM_P_FINTABLE: u16 = 0x2416;
const SPRM_P_FTTP: u16 = 0x2417;
const SPRM_P_DXARIGHT80: u16 = 0x840E;
const SPRM_P_DXALEFT80: u16 = 0x840F;
const SPRM_P_DXALEFT180: u16 = 0x8411;
const SPRM_P_DXARIGHT: u16 = 0x845D;
const SPRM_P_DXALEFT: u16 = 0x845E;
const SPRM_P_DXALEFT1: u16 = 0x8460;
const SPRM_P_DYABEFORE: u16 = 0xA413;
const SPRM_P_DYAAFTER: u16 = 0xA414;
const SPRM_P_DYALINE: u16 = 0x6412;
const SPRM_P_CHGTABSPAPX: u16 = 0xC60D;
const SPRM_P_CHGTABS: u16 = 0xC615;
/// Operand: offset in the `Data` stream of the real grpprl.
pub(crate) const SPRM_P_HUGEPAPX: u16 = 0x6646;

pub(crate) fn sprm_sgc(op: u16) -> u8 {
    ((op >> 10) & 7) as u8
}

/// Size of the operand of `op`, given the bytes following the opcode.
/// `None` when the data is too short to tell or to hold the operand.
fn operand_len(op: u16, rest: &[u8]) -> Option<usize> {
    let len = match op >> 13 {
        0 | 1 => 1,
        2 | 4 | 5 => 2,
        3 => 4,
        7 => 3,
        _ => match op {
            0xD608 => {
                let cb = u16::from_le_bytes([*rest.first()?, *rest.get(1)?]) as usize;
                (cb + 1).max(2)
            }
            SPRM_P_CHGTABS => {
                let cb = *rest.first()? as usize;
                if cb == 255 {
                    let del = *rest.get(1)? as usize;
                    let add = *rest.get(2 + 4 * del)? as usize;
                    3 + 4 * del + 3 * add
                } else {
                    cb + 1
                }
            }
            _ => *rest.first()? as usize + 1,
        },
    };
    (len <= rest.len()).then_some(len)
}

/// One `Prl`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Prl<'a> {
    pub op: u16,
    pub operand: &'a [u8],
}

impl Prl<'_> {
    fn byte(&self) -> Option<u8> {
        self.operand.first().copied()
    }
    fn u16(&self) -> Option<u16> {
        Some(u16::from_le_bytes([
            *self.operand.first()?,
            *self.operand.get(1)?,
        ]))
    }
    fn i16(&self) -> Option<i16> {
        self.u16().map(|v| v as i16)
    }
}

/// Iterator over a grpprl. Stops (and sets `truncated`) on a cut-off sprm.
pub(crate) struct Grpprl<'a> {
    data: &'a [u8],
    pos: usize,
    pub truncated: bool,
}

impl<'a> Grpprl<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            truncated: false,
        }
    }
}

impl<'a> Iterator for Grpprl<'a> {
    type Item = Prl<'a>;
    fn next(&mut self) -> Option<Prl<'a>> {
        if self.pos >= self.data.len() {
            return None;
        }
        let rest = &self.data[self.pos..];
        if rest.len() < 2 {
            // A single trailing zero byte is the padding of an FKP entry.
            self.truncated = rest != [0];
            self.pos = self.data.len();
            return None;
        }
        let op = u16::from_le_bytes([rest[0], rest[1]]);
        let Some(len) = operand_len(op, &rest[2..]) else {
            self.truncated = true;
            self.pos = self.data.len();
            return None;
        };
        let operand = &rest[2..2 + len];
        self.pos += 2 + len;
        Some(Prl { op, operand })
    }
}

/// Counters behind the import notices.
#[derive(Default, Debug)]
pub(crate) struct Stats {
    pub unsupported: BTreeMap<&'static str, usize>,
    pub approximated: BTreeMap<&'static str, usize>,
    pub unknown_sprms: usize,
    pub truncated_grpprls: usize,
    pub sprms: usize,
}

impl Stats {
    fn unsupported(&mut self, name: &'static str) {
        *self.unsupported.entry(name).or_insert(0) += 1;
    }
    fn approximated(&mut self, name: &'static str) {
        *self.approximated.entry(name).or_insert(0) += 1;
    }
}

/// A "toggle" operand: 0 off, 1 on, 0x80 same as style, 0x81 opposite of style.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Toggle {
    #[default]
    Unset,
    Off,
    On,
    Same,
    Opposite,
}

impl Toggle {
    fn from_byte(b: u8) -> Toggle {
        match b {
            0 => Toggle::Off,
            1 => Toggle::On,
            0x80 => Toggle::Same,
            0x81 => Toggle::Opposite,
            _ => Toggle::Unset,
        }
    }
    /// Value after applying this toggle on top of the inherited `base`.
    pub fn resolve(self, base: Option<bool>) -> Option<bool> {
        match self {
            Toggle::Unset | Toggle::Same => base,
            Toggle::Off => Some(false),
            Toggle::On => Some(true),
            Toggle::Opposite => Some(!base.unwrap_or(false)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ColorSpec {
    Auto,
    Rgb(Color),
}

/// Direct character formatting collected from a grpprl.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CharDelta {
    pub bold: Toggle,
    pub italic: Toggle,
    pub strike: Toggle,
    pub dstrike: Toggle,
    pub small_caps: Toggle,
    pub caps: Toggle,
    pub vanish: Toggle,
    pub kul: Option<u8>,
    pub hps: Option<u16>,
    pub iss: Option<u8>,
    pub color: Option<ColorSpec>,
    pub highlight: Option<u8>,
    /// Character shading (`sprmCShd`, `sprmCShd80`), as a fill colour.
    pub shading: Option<ColorSpec>,
    pub ftc: [Option<u16>; 3],
    /// `sprmCSymbol`: font index and character code of an inserted symbol.
    pub symbol: Option<(u16, u16)>,
    pub istd: Option<u16>,
    pub dxa_space: Option<i16>,
    pub hps_pos: Option<i16>,
}

/// Inherited toggle values used to resolve `0x81` ("opposite of style").
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BaseToggles {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub strike: Option<bool>,
}

impl CharDelta {
    /// The character a `sprmCSymbol` run stands for: codes below 0x100 go to
    /// the private use area U+F000..U+F0FF, as Word does for symbol fonts;
    /// other codes are code points.
    pub fn symbol_char(&self) -> Option<char> {
        let (_, code) = self.symbol?;
        char::from_u32(if code < 0x100 {
            0xF000 | u32::from(code)
        } else {
            u32::from(code)
        })
    }

    /// True when a toggle depends on the inherited value (`0x81`).
    pub fn needs_base(&self) -> bool {
        [self.bold, self.italic, self.strike].contains(&Toggle::Opposite)
    }

    pub fn apply_all(&mut self, grpprl: &[u8], stats: &mut Stats) {
        let mut it = Grpprl::new(grpprl);
        for prl in it.by_ref() {
            stats.sprms += 1;
            self.apply(prl, stats);
        }
        if it.truncated {
            stats.truncated_grpprls += 1;
        }
    }

    fn apply(&mut self, prl: Prl<'_>, stats: &mut Stats) {
        let tog = |p: &Prl<'_>| p.byte().map(Toggle::from_byte);
        match prl.op {
            SPRM_C_FBOLD => self.bold = tog(&prl).unwrap_or(self.bold),
            SPRM_C_FITALIC => self.italic = tog(&prl).unwrap_or(self.italic),
            SPRM_C_FSTRIKE => self.strike = tog(&prl).unwrap_or(self.strike),
            SPRM_C_FDSTRIKE => self.dstrike = tog(&prl).unwrap_or(self.dstrike),
            SPRM_C_FSMALLCAPS => self.small_caps = tog(&prl).unwrap_or(self.small_caps),
            SPRM_C_FCAPS => self.caps = tog(&prl).unwrap_or(self.caps),
            SPRM_C_FVANISH => self.vanish = tog(&prl).unwrap_or(self.vanish),
            SPRM_C_FOUTLINE => note_effect(&prl, "outline", stats),
            SPRM_C_FSHADOW => note_effect(&prl, "shadow", stats),
            SPRM_C_FEMBOSS => note_effect(&prl, "emboss", stats),
            SPRM_C_FIMPRINT => note_effect(&prl, "imprint", stats),
            SPRM_C_KUL => self.kul = prl.byte().or(self.kul),
            SPRM_C_HPS => {
                if let Some(v) = prl.u16().filter(|v| (2..=3276).contains(v)) {
                    self.hps = Some(v);
                }
            }
            SPRM_C_ISS => self.iss = prl.byte().or(self.iss),
            SPRM_C_ICO => {
                if let Some(i) = prl.byte() {
                    self.color = Some(ico_color(i).map_or(ColorSpec::Auto, ColorSpec::Rgb));
                }
            }
            SPRM_C_CV => {
                if let [r, g, b, a] = prl.operand[..] {
                    self.color = Some(if a == 0xFF {
                        ColorSpec::Auto
                    } else {
                        ColorSpec::Rgb(Color { r, g, b })
                    });
                }
            }
            SPRM_C_HIGHLIGHT => self.highlight = prl.byte().or(self.highlight),
            SPRM_C_SHD => {
                // Shd: cvFore, cvBack (4 bytes each), ipat (2 bytes).
                if let [f0, f1, f2, fa, b0, b1, b2, ba, p0, p1] = prl.operand[1..] {
                    let solid = u16::from_le_bytes([p0, p1]) == 1;
                    let (r, g, b, a) = if solid {
                        (f0, f1, f2, fa)
                    } else {
                        (b0, b1, b2, ba)
                    };
                    self.shading = Some(if a == 0xFF {
                        ColorSpec::Auto
                    } else {
                        ColorSpec::Rgb(Color { r, g, b })
                    });
                }
            }
            SPRM_C_SHD80 => {
                // Shd80: icoFore (bits 0-4), icoBack (5-9), ipat (10-15).
                if let Some(w) = prl.u16() {
                    let solid = w >> 10 == 1;
                    let ico = if solid { w & 0x1F } else { (w >> 5) & 0x1F };
                    self.shading =
                        Some(ico_color(ico as u8).map_or(ColorSpec::Auto, ColorSpec::Rgb));
                }
            }
            SPRM_C_RGFTC0 => self.ftc[0] = prl.u16().or(self.ftc[0]),
            SPRM_C_RGFTC1 => self.ftc[1] = prl.u16().or(self.ftc[1]),
            SPRM_C_RGFTC2 => self.ftc[2] = prl.u16().or(self.ftc[2]),
            SPRM_C_FTCDEFAULT => {
                if let Some(v) = prl.u16() {
                    self.ftc = [Some(v); 3];
                }
            }
            SPRM_C_SYMBOL => {
                if let [a, b, c, d] = prl.operand[..] {
                    self.symbol = Some((u16::from_le_bytes([a, b]), u16::from_le_bytes([c, d])));
                }
            }
            SPRM_C_ISTD => self.istd = prl.u16().or(self.istd),
            SPRM_C_DXASPACE => self.dxa_space = prl.i16().or(self.dxa_space),
            SPRM_C_HPSPOS => self.hps_pos = prl.i16().or(self.hps_pos),
            SPRM_C_PLAIN => *self = CharDelta::default(),
            op if C_SILENT.contains(&op) => {}
            op if sprm_sgc(op) == 2 => stats.unknown_sprms += 1,
            _ => {}
        }
    }

    /// Maps the collected properties to model attributes. `base` resolves
    /// style-relative toggles; fonts come from `SttbfFfn`.
    pub fn to_attrs(&self, base: BaseToggles, fonts: &[DocFont], stats: &mut Stats) -> CharAttrs {
        let mut a = CharAttrs {
            bold: value_of(self.bold, base.bold),
            ..CharAttrs::default()
        };
        a.italic = value_of(self.italic, base.italic);
        a.strike = value_of(self.strike, base.strike);
        if let Some(k) = self.kul {
            // Values of `Kul`, checked against a LibreOffice file (see
            // `tests/doc-reference/underlines.doc`). Heavy and patterned variants
            // are reduced to the closest model value.
            a.underline = Some(match k {
                0 => Underline::None,
                1 => Underline::Single,
                2 => {
                    stats.approximated("wordsOnlyUnderline");
                    Underline::Single
                }
                3 => Underline::Double,
                4 => Underline::Dotted,
                6 => Underline::Thick,
                7 | 9 | 0x0A | 0x27 => Underline::Dashed,
                0x0B => Underline::Wave,
                0x14 | 0x17 | 0x19 | 0x1A | 0x37 => {
                    stats.approximated("heavyUnderline");
                    if k == 0x14 {
                        Underline::Dotted
                    } else {
                        Underline::Dashed
                    }
                }
                0x1B | 0x2B => {
                    stats.approximated("heavyUnderline");
                    Underline::Wave
                }
                _ => {
                    stats.approximated("unknownUnderline");
                    Underline::Single
                }
            });
        }
        if let Some(i) = self.iss {
            a.vertical_align = match i {
                0 => Some(VerticalAlign::Baseline),
                1 => Some(VerticalAlign::Superscript),
                2 => Some(VerticalAlign::Subscript),
                _ => None,
            };
        }
        a.size_half_points = self.hps.map(u32::from);
        match self.color {
            Some(ColorSpec::Rgb(c)) => a.color = Some(c),
            Some(ColorSpec::Auto) => stats.unsupported("automaticColor"),
            None => {}
        }
        if let Some(h) = self.highlight {
            a.highlight = ico_color(h);
        }
        if a.highlight.is_none() {
            if let Some(ColorSpec::Rgb(c)) = self.shading {
                // The model has no character shading: the fill is the closest
                // available property.
                stats.approximated("characterShadingAsHighlight");
                a.highlight = Some(c);
            }
        }
        // The model keeps a single font: ASCII first, then the "other" slot.
        let ftc = match self.symbol {
            Some((font, _)) => {
                stats.approximated("symbolCharacter");
                Some(font)
            }
            None => self.ftc[0].or(self.ftc[2]).or(self.ftc[1]),
        };
        if let Some(f) = ftc {
            match fonts.get(f as usize) {
                Some(font) if !font.name.is_empty() => a.font = Some(font.name.clone()),
                _ => stats.unsupported("unknownFontIndex"),
            }
        }
        let mut opaque = Vec::new();
        let mut put = |name: &'static str, t: Toggle, label: &'static str| match t {
            Toggle::On | Toggle::Off => {
                stats.unsupported(label);
                opaque.push(OpaqueAttr {
                    namespace: OPAQUE_NS.to_owned(),
                    name: name.to_owned(),
                    value: vec![u8::from(t == Toggle::On)],
                });
            }
            Toggle::Opposite => stats.unsupported(label),
            _ => {}
        };
        put("smallCaps", self.small_caps, "smallCaps");
        put("caps", self.caps, "caps");
        put("hidden", self.vanish, "hiddenText");
        match self.dstrike {
            Toggle::On => {
                stats.approximated("doubleStrike");
                a.strike = Some(true);
                opaque.push(OpaqueAttr {
                    namespace: OPAQUE_NS.to_owned(),
                    name: "doubleStrike".to_owned(),
                    value: vec![1],
                });
            }
            Toggle::Off => opaque.push(OpaqueAttr {
                namespace: OPAQUE_NS.to_owned(),
                name: "doubleStrike".to_owned(),
                value: vec![0],
            }),
            _ => {}
        }
        if let Some(d) = self.dxa_space.filter(|d| *d != 0) {
            stats.unsupported("characterSpacing");
            opaque.push(OpaqueAttr {
                namespace: OPAQUE_NS.to_owned(),
                name: "spacing".to_owned(),
                value: d.to_le_bytes().to_vec(),
            });
        }
        if let Some(p) = self.hps_pos.filter(|p| *p != 0) {
            stats.unsupported("raisedLoweredText");
            opaque.push(OpaqueAttr {
                namespace: OPAQUE_NS.to_owned(),
                name: "position".to_owned(),
                value: p.to_le_bytes().to_vec(),
            });
        }
        opaque.sort_by(|x, y| x.name.cmp(&y.name));
        a.opaque = opaque;
        a
    }
}

fn note_effect(prl: &Prl<'_>, name: &'static str, stats: &mut Stats) {
    if matches!(prl.byte(), Some(1 | 0x81)) {
        stats.unsupported(name);
    }
}

/// `Same` (0x80) leaves the property inherited; `Opposite` flips the base.
fn value_of(t: Toggle, base: Option<bool>) -> Option<bool> {
    match t {
        Toggle::Unset | Toggle::Same => None,
        _ => t.resolve(base),
    }
}

/// `Ico` colour table; index 0 is "automatic".
pub(crate) fn ico_color(i: u8) -> Option<Color> {
    let (r, g, b) = match i {
        1 => (0x00, 0x00, 0x00),
        2 => (0x00, 0x00, 0xFF),
        3 => (0x00, 0xFF, 0xFF),
        4 => (0x00, 0xFF, 0x00),
        5 => (0xFF, 0x00, 0xFF),
        6 => (0xFF, 0x00, 0x00),
        7 => (0xFF, 0xFF, 0x00),
        8 => (0xFF, 0xFF, 0xFF),
        9 => (0x00, 0x00, 0x80),
        10 => (0x00, 0x80, 0x80),
        11 => (0x00, 0x80, 0x00),
        12 => (0x80, 0x00, 0x80),
        13 => (0x80, 0x00, 0x00),
        14 => (0x80, 0x80, 0x00),
        15 => (0x80, 0x80, 0x80),
        16 => (0xC0, 0xC0, 0xC0),
        _ => return None,
    };
    Some(Color { r, g, b })
}

/// Direct paragraph formatting collected from a grpprl.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ParaDelta {
    pub istd: Option<u16>,
    pub jc: Option<u8>,
    pub left: Option<i16>,
    pub right: Option<i16>,
    pub first: Option<i16>,
    pub before: Option<u16>,
    pub after: Option<u16>,
    pub line: Option<(i16, u16)>,
    pub keep_lines: Toggle,
    pub keep_next: Toggle,
    pub page_break_before: Toggle,
    pub widow_control: Toggle,
    pub outline_level: Option<u8>,
    pub tabs: Option<Vec<u8>>,
    pub in_table: bool,
    pub row_end: bool,
    pub huge: Option<u32>,
}

impl ParaDelta {
    pub fn apply_all(&mut self, grpprl: &[u8], stats: &mut Stats) {
        let mut it = Grpprl::new(grpprl);
        for prl in it.by_ref() {
            stats.sprms += 1;
            self.apply(prl, stats);
        }
        if it.truncated {
            stats.truncated_grpprls += 1;
        }
    }

    fn apply(&mut self, prl: Prl<'_>, stats: &mut Stats) {
        let tog = |p: &Prl<'_>, cur: Toggle| p.byte().map_or(cur, Toggle::from_byte);
        match prl.op {
            SPRM_P_ISTD => self.istd = prl.u16().or(self.istd),
            SPRM_P_JC80 | SPRM_P_JC => self.jc = prl.byte().or(self.jc),
            SPRM_P_DXALEFT80 | SPRM_P_DXALEFT => self.left = prl.i16().or(self.left),
            SPRM_P_DXARIGHT80 | SPRM_P_DXARIGHT => self.right = prl.i16().or(self.right),
            SPRM_P_DXALEFT180 | SPRM_P_DXALEFT1 => self.first = prl.i16().or(self.first),
            SPRM_P_DYABEFORE => self.before = prl.u16().or(self.before),
            SPRM_P_DYAAFTER => self.after = prl.u16().or(self.after),
            SPRM_P_DYALINE => {
                if let [a, b, c, d] = prl.operand[..] {
                    self.line = Some((i16::from_le_bytes([a, b]), u16::from_le_bytes([c, d])));
                }
            }
            SPRM_P_FKEEP => self.keep_lines = tog(&prl, self.keep_lines),
            SPRM_P_FKEEPFOLLOW => self.keep_next = tog(&prl, self.keep_next),
            SPRM_P_FPAGEBREAKBEFORE => self.page_break_before = tog(&prl, self.page_break_before),
            SPRM_P_FWIDOWCONTROL => self.widow_control = tog(&prl, self.widow_control),
            SPRM_P_OUTLVL => self.outline_level = prl.byte().or(self.outline_level),
            SPRM_P_CHGTABSPAPX | SPRM_P_CHGTABS => self.tabs = Some(prl.operand.to_vec()),
            SPRM_P_FINTABLE => self.in_table = prl.byte().is_some_and(|b| b != 0),
            SPRM_P_FTTP => self.row_end = prl.byte().is_some_and(|b| b != 0),
            SPRM_P_HUGEPAPX => {
                if let [a, b, c, d] = prl.operand[..] {
                    self.huge = Some(u32::from_le_bytes([a, b, c, d]));
                }
            }
            0x6424..=0x6428 | 0xC64E..=0xC653 => stats.unsupported("paragraphBorders"),
            0x442D | 0xC64D => stats.unsupported("paragraphShading"),
            0x260A | 0x460B => stats.unsupported("listNumbering"),
            op if sprm_sgc(op) == 5 => stats.unsupported("tableFormatting"),
            op if sprm_sgc(op) == 1 => stats.unknown_sprms += 1,
            _ => {}
        }
    }

    /// Maps the collected properties (without the style) to model properties.
    pub fn to_props(&self, stats: &mut Stats) -> ParaProps {
        let alignment = self.jc.and_then(|j| match j {
            0 => Some(Alignment::Left),
            1 => Some(Alignment::Center),
            2 => Some(Alignment::Right),
            3 => Some(Alignment::Justify),
            4..=9 => {
                stats.approximated("distributedAlignment");
                Some(Alignment::Justify)
            }
            _ => None,
        });
        let mut p = ParaProps {
            alignment,
            indent_left: self.left.map(i32::from),
            indent_right: self.right.map(i32::from),
            indent_first_line: self.first.map(i32::from),
            space_before: self.before.map(u32::from),
            space_after: self.after.map(u32::from),
            ..ParaProps::default()
        };
        if let Some((line, mult)) = self.line {
            p.line_spacing = if mult == 1 {
                (line > 0).then_some(LineSpacing {
                    rule: LineRule::Auto,
                    value: line as u32,
                })
            } else if line > 0 {
                Some(LineSpacing {
                    rule: LineRule::AtLeast,
                    value: line as u32,
                })
            } else if line < 0 {
                Some(LineSpacing {
                    rule: LineRule::Exact,
                    value: u32::from(line.unsigned_abs()),
                })
            } else {
                None
            };
        }
        let mut opaque = Vec::new();
        let mut put = |name: &str, label: &'static str, value: Vec<u8>| {
            stats.unsupported(label);
            opaque.push(OpaqueAttr {
                namespace: OPAQUE_NS.to_owned(),
                name: name.to_owned(),
                value,
            });
        };
        for (name, label, t) in [
            ("keepLines", "keepLinesTogether", self.keep_lines),
            ("keepNext", "keepWithNext", self.keep_next),
            ("pageBreakBefore", "pageBreakBefore", self.page_break_before),
            ("widowControl", "widowOrphanControl", self.widow_control),
        ] {
            if matches!(t, Toggle::On | Toggle::Off) {
                put(name, label, vec![u8::from(t == Toggle::On)]);
            }
        }
        if let Some(l) = self.outline_level.filter(|l| *l < 9) {
            put("outlineLevel", "outlineLevel", vec![l]);
        }
        if let Some(t) = &self.tabs {
            put("tabs", "tabStops", t.clone());
        }
        opaque.sort_by(|x, y| x.name.cmp(&y.name));
        p.opaque = opaque;
        p
    }
}
