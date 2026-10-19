// SPDX-License-Identifier: AGPL-3.0-or-later
//! Frozen vectors and refusals of the formatting messages (F0): `FormatState`,
//! `ApplyCharFormat`, `ApplyParaFormat`, `ListStyles`, `ApplyStyle`,
//! `PreviewStyle` and the replies `FormatState` and `StyleList`. Vectors:
//! `tests/rich-editor-format-vectors.txt`, verified here and by
//! `tests/rich-editor-format-vectors.mjs` (`ATELIER_REGEN_VECTORS=1` rewrites them).

use super::*;
use std::fmt::Write as _;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/rich-editor-format-vectors.txt"
);

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn req(request_id: u32, command: Command) -> Request {
    Request {
        doc_id: 7,
        request_id,
        command,
    }
}

fn reply(request_id: u32, response: Response) -> Reply {
    Reply {
        doc_id: 7,
        request_id,
        response,
    }
}

fn frame(op: u16, body: &[u8]) -> Vec<u8> {
    header(op, 7, 1, body).unwrap()
}

fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

// ---------------------------------------------------------------- dumps

fn dump_rgb(c: Rgb) -> String {
    format!("{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn dump_guard(g: u64) -> String {
    if g == NO_REVISION_GUARD {
        "none".into()
    } else {
        g.to_string()
    }
}

fn dump_char_value(v: &Option<CharValue>) -> String {
    match v {
        None => "-".into(),
        Some(CharValue::Text(t)) => hex(t.as_bytes()),
        Some(CharValue::Half(v)) => v.to_string(),
        Some(CharValue::Flag(v)) => u8::from(*v).to_string(),
        Some(CharValue::Choice(v)) => v.to_string(),
        Some(CharValue::Color(c)) => dump_rgb(*c),
    }
}

fn dump_border_edit(e: &BorderEdit) -> String {
    match e {
        BorderEdit::Keep => "keep".into(),
        BorderEdit::None => "none".into(),
        BorderEdit::Line(l) => dump_line(l),
    }
}

fn dump_line(l: &BorderLine) -> String {
    format!(
        "{}.{}.{}.{}",
        l.style,
        l.eighths,
        l.space,
        l.color.map_or("auto".into(), dump_rgb)
    )
}

fn dump_para_value(v: &Option<ParaValue>) -> String {
    match v {
        None => "-".into(),
        Some(ParaValue::Choice(v)) => v.to_string(),
        Some(ParaValue::Twips(v)) => v.to_string(),
        Some(ParaValue::Length(v)) => v.to_string(),
        Some(ParaValue::Line { rule, value }) => format!("{rule}/{value}"),
        Some(ParaValue::Color(c)) => dump_rgb(*c),
        Some(ParaValue::Borders(s)) => s.iter().map(dump_border_edit).collect::<Vec<_>>().join(","),
    }
}

fn dump_request(r: &Request) -> String {
    let head = format!("doc={} req={}", r.doc_id, r.request_id);
    let tail = match &r.command {
        Command::FormatState => "FormatState".to_string(),
        Command::ApplyCharFormat(f) => format!(
            "ApplyCharFormat guard={} scope={} edits={}",
            dump_guard(f.base_revision),
            f.scope as u8,
            f.edits
                .iter()
                .map(|e| format!(
                    "{}:{}:{}",
                    e.attr as u8,
                    e.op as u8,
                    dump_char_value(&e.value)
                ))
                .collect::<Vec<_>>()
                .join(";")
        ),
        Command::ApplyParaFormat(f) => format!(
            "ApplyParaFormat guard={} edits={}",
            dump_guard(f.base_revision),
            f.edits
                .iter()
                .map(|e| format!(
                    "{}:{}:{}",
                    e.attr as u8,
                    e.op as u8,
                    dump_para_value(&e.value)
                ))
                .collect::<Vec<_>>()
                .join(";")
        ),
        Command::ListStyles { filter, used_only } => {
            format!(
                "ListStyles filter={} used={}",
                *filter as u8,
                u8::from(*used_only)
            )
        }
        Command::ApplyStyle {
            base_revision,
            target,
            id,
        } => format!(
            "ApplyStyle guard={} target={} id={}",
            dump_guard(*base_revision),
            *target as u8,
            hex(id.as_bytes())
        ),
        Command::PreviewStyle { target, show, id } => format!(
            "PreviewStyle target={} show={} id={}",
            *target as u8,
            u8::from(*show),
            hex(id.as_bytes())
        ),
        Command::ListFonts => "ListFonts".to_string(),
        Command::FormatMarks { first, count } => format!("FormatMarks first={first} count={count}"),
        other => panic!("not a formatting command: {other:?}"),
    };
    format!("{head} {tail}")
}

fn tri<T>(t: &Tri<T>, show: impl FnOnce(&T) -> String) -> String {
    match t {
        Tri::Mixed => "m".into(),
        Tri::Unset => "u".into(),
        Tri::Value(v) => format!("v{}", show(v)),
    }
}

fn dump_style_entry(s: &StyleEntry) -> String {
    format!(
        "{}/{}/{}/{}/{}/{}/{}/{}/{}/{}/{}/{}",
        hex(s.id.as_bytes()),
        hex(s.name.as_bytes()),
        hex(s.based_on.as_bytes()),
        s.target as u8,
        s.flags,
        s.heading_level,
        hex(s.font.as_bytes()),
        s.half_points,
        s.char_flags,
        s.underline,
        s.color.map_or("auto".into(), dump_rgb),
        s.alignment
    )
}

fn dump_reply(r: &Reply) -> String {
    let head = format!("doc={} req={}", r.doc_id, r.request_id);
    let tail = match &r.response {
        Response::FormatState(s) => {
            let c = &s.chars;
            let p = &s.paragraph;
            format!(
                "FormatState revision={} anchor={} focus={} flags={} font={} size={} bold={} italic={} underline={} strike={} valign={} color={} highlight={} cstyle={} | align={} left={} right={} first={} before={} after={} line={} shading={} borders={} pstyle={}",
                s.revision,
                s.anchor,
                s.focus,
                u8::from(s.collapsed)
                    | u8::from(s.read_only) << 1
                    | u8::from(s.partial) << 2
                    | u8::from(s.typing) << 3
                    | u8::from(s.plain) << 4,
                tri(&c.font, |f| format!(
                    "{}/{}/{}",
                    hex(f.requested.as_bytes()),
                    hex(f.used.as_bytes()),
                    u8::from(f.substituted)
                )),
                tri(&c.half_points, u16::to_string),
                tri(&c.bold, |v| u8::from(*v).to_string()),
                tri(&c.italic, |v| u8::from(*v).to_string()),
                tri(&c.underline, u8::to_string),
                tri(&c.strike, |v| u8::from(*v).to_string()),
                tri(&c.vertical_align, u8::to_string),
                tri(&c.color, |c| dump_rgb(*c)),
                tri(&c.highlight, |c| dump_rgb(*c)),
                tri(&c.style, |s| hex(s.as_bytes())),
                tri(&p.alignment, u8::to_string),
                tri(&p.indent_left, i32::to_string),
                tri(&p.indent_right, i32::to_string),
                tri(&p.first_line, i32::to_string),
                tri(&p.space_before, u32::to_string),
                tri(&p.space_after, u32::to_string),
                tri(&p.line_spacing, |(r, v)| format!("{r}/{v}")),
                tri(&p.shading, |c| dump_rgb(*c)),
                p.borders
                    .iter()
                    .map(|b| tri(b, dump_line))
                    .collect::<Vec<_>>()
                    .join(","),
                tri(&p.style, |s| hex(s.as_bytes())),
            )
        }
        Response::StyleList { revision, styles } => format!(
            "StyleList revision={revision} styles={}",
            if styles.is_empty() {
                "none".into()
            } else {
                styles
                    .iter()
                    .map(dump_style_entry)
                    .collect::<Vec<_>>()
                    .join(";")
            }
        ),
        Response::FontList(l) => format!(
            "FontList revision={} theme={}/{} families={}",
            l.revision,
            hex(l.theme_major.as_bytes()),
            hex(l.theme_minor.as_bytes()),
            if l.families.is_empty() {
                "none".into()
            } else {
                l.families
                    .iter()
                    .map(|f| {
                        format!(
                            "{}/{}/{}",
                            hex(f.name.as_bytes()),
                            f.flags,
                            hex(f.substitute.as_bytes())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(";")
            }
        ),
        Response::FormatMarks { revision, marks } => format!(
            "FormatMarks revision={revision} marks={}",
            if marks.is_empty() {
                "none".into()
            } else {
                marks
                    .iter()
                    .map(|m| format!("{}:{}:{}:{}:{}", m.page, m.kind, m.x, m.y, m.width))
                    .collect::<Vec<_>>()
                    .join(";")
            }
        ),
        other => panic!("not a formatting reply: {other:?}"),
    };
    format!("{head} {tail}")
}

// ---------------------------------------------------------------- vectors

fn char_edit(attr: CharAttr, op: EditOp, value: Option<CharValue>) -> CharEdit {
    CharEdit { attr, op, value }
}

fn para_edit(attr: ParaAttr, op: EditOp, value: Option<ParaValue>) -> ParaEdit {
    ParaEdit { attr, op, value }
}

fn requests() -> Vec<(&'static str, Request)> {
    use CharAttr as A;
    use EditOp as O;
    let line = BorderLine {
        style: 0,
        eighths: 4,
        space: 1,
        color: None,
    };
    vec![
        ("format-state", req(1, Command::FormatState)),
        (
            "char-toggle-bold",
            req(
                2,
                Command::ApplyCharFormat(CharFormat {
                    base_revision: NO_REVISION_GUARD,
                    scope: FormatScope::WordOrTyping,
                    edits: vec![char_edit(A::Bold, O::Toggle, None)],
                }),
            ),
        ),
        (
            "char-font-size-color",
            req(
                3,
                Command::ApplyCharFormat(CharFormat {
                    base_revision: 9,
                    scope: FormatScope::WordOrTyping,
                    edits: vec![
                        char_edit(A::Font, O::Set, Some(CharValue::Text("Calibri".into()))),
                        char_edit(A::Size, O::Set, Some(CharValue::Half(24))),
                        char_edit(A::Color, O::Set, Some(CharValue::Color(rgb(255, 0, 0)))),
                        char_edit(A::Italic, O::Set, Some(CharValue::Flag(false))),
                    ],
                }),
            ),
        ),
        (
            "char-toggle-underline-valign",
            req(
                4,
                Command::ApplyCharFormat(CharFormat {
                    base_revision: 0,
                    scope: FormatScope::SelectionOnly,
                    edits: vec![
                        char_edit(A::Underline, O::Toggle, Some(CharValue::Choice(1))),
                        char_edit(A::VerticalAlign, O::Toggle, Some(CharValue::Choice(1))),
                        char_edit(A::Strike, O::Toggle, None),
                    ],
                }),
            ),
        ),
        (
            "char-size-steps-clear",
            req(
                5,
                Command::ApplyCharFormat(CharFormat {
                    base_revision: NO_REVISION_GUARD,
                    scope: FormatScope::WordOrTyping,
                    edits: vec![
                        char_edit(A::Size, O::StepUp, None),
                        char_edit(A::Highlight, O::Clear, None),
                    ],
                }),
            ),
        ),
        (
            "char-size-point-down",
            req(
                6,
                Command::ApplyCharFormat(CharFormat {
                    base_revision: NO_REVISION_GUARD,
                    scope: FormatScope::WordOrTyping,
                    edits: vec![char_edit(A::Size, O::StepDownPoint, None)],
                }),
            ),
        ),
        (
            "para-align-indent-space",
            req(
                7,
                Command::ApplyParaFormat(ParaFormat {
                    base_revision: 3,
                    edits: vec![
                        para_edit(ParaAttr::Alignment, O::Set, Some(ParaValue::Choice(1))),
                        para_edit(ParaAttr::IndentLeft, O::StepUp, None),
                        para_edit(ParaAttr::SpaceBefore, O::Toggle, None),
                        para_edit(ParaAttr::FirstLine, O::Set, Some(ParaValue::Twips(-360))),
                    ],
                }),
            ),
        ),
        (
            "para-line-shading-borders",
            req(
                8,
                Command::ApplyParaFormat(ParaFormat {
                    base_revision: NO_REVISION_GUARD,
                    edits: vec![
                        para_edit(
                            ParaAttr::LineSpacing,
                            O::Set,
                            Some(ParaValue::Line {
                                rule: 0,
                                value: 360,
                            }),
                        ),
                        para_edit(
                            ParaAttr::Shading,
                            O::Set,
                            Some(ParaValue::Color(rgb(255, 255, 0))),
                        ),
                        para_edit(
                            ParaAttr::Borders,
                            O::Set,
                            Some(ParaValue::Borders([
                                BorderEdit::Line(line),
                                BorderEdit::Keep,
                                BorderEdit::Line(BorderLine {
                                    color: Some(rgb(0, 0, 255)),
                                    ..line
                                }),
                                BorderEdit::None,
                                BorderEdit::Keep,
                                BorderEdit::Keep,
                            ])),
                        ),
                        para_edit(ParaAttr::SpaceAfter, O::Clear, None),
                    ],
                }),
            ),
        ),
        (
            "list-styles",
            req(
                9,
                Command::ListStyles {
                    filter: StyleFilter::Paragraph,
                    used_only: true,
                },
            ),
        ),
        (
            "apply-paragraph-style",
            req(
                10,
                Command::ApplyStyle {
                    base_revision: 12,
                    target: StyleTarget::Paragraph,
                    id: "Heading1".into(),
                },
            ),
        ),
        (
            "apply-character-style-clear",
            req(
                11,
                Command::ApplyStyle {
                    base_revision: NO_REVISION_GUARD,
                    target: StyleTarget::Character,
                    id: String::new(),
                },
            ),
        ),
        (
            "preview-show",
            req(
                12,
                Command::PreviewStyle {
                    target: StyleTarget::Paragraph,
                    show: true,
                    id: "Titre".into(),
                },
            ),
        ),
        ("list-fonts", req(14, Command::ListFonts)),
        (
            "format-marks",
            req(15, Command::FormatMarks { first: 2, count: 3 }),
        ),
        (
            "preview-cancel",
            req(
                13,
                Command::PreviewStyle {
                    target: StyleTarget::Paragraph,
                    show: false,
                    id: String::new(),
                },
            ),
        ),
    ]
}

fn mixed_char_state() -> CharState {
    CharState {
        font: Tri::Value(FontState {
            requested: "Calibri".into(),
            used: "Carlito".into(),
            substituted: true,
        }),
        half_points: Tri::Mixed,
        bold: Tri::Value(true),
        italic: Tri::Mixed,
        underline: Tri::Unset,
        strike: Tri::Value(false),
        vertical_align: Tri::Value(1),
        color: Tri::Value(rgb(255, 0, 0)),
        highlight: Tri::Unset,
        style: Tri::Value("Emphasis".into()),
    }
}

fn some_para_state() -> ParaState {
    let line = BorderLine {
        style: 1,
        eighths: 6,
        space: 4,
        color: Some(rgb(1, 2, 3)),
    };
    ParaState {
        alignment: Tri::Value(1),
        indent_left: Tri::Value(720),
        indent_right: Tri::Unset,
        first_line: Tri::Value(-360),
        space_before: Tri::Value(0),
        space_after: Tri::Mixed,
        line_spacing: Tri::Value((0, 240)),
        shading: Tri::Unset,
        borders: [
            Tri::Value(line),
            Tri::Unset,
            Tri::Mixed,
            Tri::Unset,
            Tri::Unset,
            Tri::Unset,
        ],
        style: Tri::Value("Heading1".into()),
    }
}

fn unset_char_state() -> CharState {
    CharState {
        font: Tri::Unset,
        half_points: Tri::Unset,
        bold: Tri::Unset,
        italic: Tri::Unset,
        underline: Tri::Unset,
        strike: Tri::Unset,
        vertical_align: Tri::Unset,
        color: Tri::Unset,
        highlight: Tri::Unset,
        style: Tri::Unset,
    }
}

fn unset_para_state() -> ParaState {
    ParaState {
        alignment: Tri::Unset,
        indent_left: Tri::Unset,
        indent_right: Tri::Unset,
        first_line: Tri::Unset,
        space_before: Tri::Unset,
        space_after: Tri::Unset,
        line_spacing: Tri::Unset,
        shading: Tri::Unset,
        borders: [const { Tri::Unset }; BORDER_SIDES],
        style: Tri::Unset,
    }
}

fn style_entry(id: &str, target: StyleTarget, flags: u8) -> StyleEntry {
    StyleEntry {
        id: id.into(),
        name: if id == "Heading1" {
            "Titre 1".into()
        } else {
            String::new()
        },
        based_on: if id == "Normal" {
            String::new()
        } else {
            "Normal".into()
        },
        target,
        flags,
        heading_level: u8::from(id.starts_with("Heading")),
        font: "Calibri".into(),
        half_points: 22,
        char_flags: 1,
        underline: 0,
        color: Some(rgb(0x2f, 0x54, 0x96)),
        alignment: 0,
    }
}

fn replies() -> Vec<(&'static str, Reply)> {
    vec![
        (
            "format-state-mixed",
            reply(
                1,
                Response::FormatState(Box::new(FormatStateData {
                    revision: 7,
                    anchor: 2,
                    focus: 9,
                    collapsed: false,
                    read_only: false,
                    partial: false,
                    typing: false,
                    plain: false,
                    chars: mixed_char_state(),
                    paragraph: some_para_state(),
                })),
            ),
        ),
        (
            "format-state-caret-read-only",
            reply(
                2,
                Response::FormatState(Box::new(FormatStateData {
                    revision: 0,
                    anchor: 0,
                    focus: 0,
                    collapsed: true,
                    read_only: true,
                    partial: true,
                    typing: true,
                    plain: true,
                    chars: unset_char_state(),
                    paragraph: unset_para_state(),
                })),
            ),
        ),
        (
            "style-list",
            reply(
                3,
                Response::StyleList {
                    revision: 5,
                    styles: vec![
                        style_entry("Normal", StyleTarget::Paragraph, 0b1101),
                        style_entry("Heading1", StyleTarget::Paragraph, 0b0001),
                        StyleEntry {
                            color: None,
                            font: String::new(),
                            half_points: 0,
                            ..style_entry("Strong", StyleTarget::Character, 0b0011)
                        },
                    ],
                },
            ),
        ),
        (
            "font-list",
            reply(
                5,
                Response::FontList(FontListData {
                    revision: 3,
                    theme_major: "Calibri Light".into(),
                    theme_minor: "Calibri".into(),
                    families: vec![
                        FontEntry {
                            name: "Carlito".into(),
                            flags: 0b0111,
                            substitute: String::new(),
                        },
                        FontEntry {
                            name: "Calibri".into(),
                            flags: 0b1111,
                            substitute: "Carlito".into(),
                        },
                    ],
                }),
            ),
        ),
        (
            "font-list-empty",
            reply(
                6,
                Response::FontList(FontListData {
                    revision: 0,
                    theme_major: String::new(),
                    theme_minor: String::new(),
                    families: Vec::new(),
                }),
            ),
        ),
        (
            "format-marks",
            reply(
                7,
                Response::FormatMarks {
                    revision: 4,
                    marks: vec![
                        FormatMark {
                            page: 0,
                            kind: 0,
                            x: 120.5,
                            y: 96.25,
                            width: 4.5,
                        },
                        FormatMark {
                            page: 1,
                            kind: 2,
                            x: 72.0,
                            y: 110.0,
                            width: 36.0,
                        },
                    ],
                },
            ),
        ),
        (
            "format-marks-empty",
            reply(
                8,
                Response::FormatMarks {
                    revision: 0,
                    marks: Vec::new(),
                },
            ),
        ),
        (
            "style-list-empty",
            reply(
                4,
                Response::StyleList {
                    revision: 0,
                    styles: Vec::new(),
                },
            ),
        ),
    ]
}

// ---------------------------------------------------------------- refusals

fn u16le(v: u16) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn u32le(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn u64le(v: u64) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// A `ApplyCharFormat` body: guard, scope, reserved byte, edit count, edits.
fn char_body(scope: u8, reserved: u8, count: u16, edits: &[u8]) -> Vec<u8> {
    cat(&[&u64le(1), &[scope, reserved], &u16le(count), edits])
}

/// An `ApplyParaFormat` body: guard, edit count, reserved, edits.
fn para_body(count: u16, reserved: [u8; 2], edits: &[u8]) -> Vec<u8> {
    cat(&[&u64le(1), &u16le(count), &reserved, edits])
}

fn side(kind: u8, style: u8, eighths: u16, space: u8, mode: u8, c: [u8; 3]) -> Vec<u8> {
    cat(&[&[kind, style], &u16le(eighths), &[space, mode], &c])
}

fn borders_body(sides: [Vec<u8>; 6]) -> Vec<u8> {
    let mut b = vec![9, 1];
    for s in sides {
        b.extend(s);
    }
    b
}

fn good_sides() -> [Vec<u8>; 6] {
    [
        side(2, 0, 4, 1, 0, [0; 3]),
        side(0, 0, 0, 0, 0, [0; 3]),
        side(1, 0, 0, 0, 0, [0; 3]),
        side(0, 0, 0, 0, 0, [0; 3]),
        side(0, 0, 0, 0, 0, [0; 3]),
        side(0, 0, 0, 0, 0, [0; 3]),
    ]
}

fn with_side(index: usize, replacement: Vec<u8>) -> [Vec<u8>; 6] {
    let mut s = good_sides();
    s[index] = replacement;
    s
}

fn style_body(guard: u64, target: u8, reserved: [u8; 3], id: &[u8]) -> Vec<u8> {
    cat(&[
        &u64le(guard),
        &[target],
        &reserved,
        &u16le(id.len() as u16),
        id,
    ])
}

fn preview_body(target: u8, action: u8, reserved: [u8; 2], id: &[u8]) -> Vec<u8> {
    cat(&[&[target, action], &reserved, &u16le(id.len() as u16), id])
}

fn bad_requests() -> Vec<(&'static str, Vec<u8>, ErrorCode)> {
    use ErrorCode::*;
    let chr = |body: Vec<u8>| frame(opcode::APPLY_CHAR_FORMAT, &body);
    let par = |body: Vec<u8>| frame(opcode::APPLY_PARA_FORMAT, &body);
    let long_id = vec![b'a'; MAX_NAME_BYTES + 1];
    let mut trailing = char_body(0, 0, 1, &[3, 2]);
    trailing.push(0);
    let many: Vec<u8> = (0..MAX_FORMAT_EDITS + 1).flat_map(|_| [3u8, 2]).collect();
    vec![
        (
            "format-state-body",
            frame(opcode::FORMAT_STATE, &[0]),
            InvalidBody,
        ),
        ("char-count-zero", chr(char_body(0, 0, 0, &[])), InvalidBody),
        (
            "char-count-too-many",
            chr(char_body(0, 0, 17, &many)),
            InvalidBody,
        ),
        ("char-scope", chr(char_body(2, 0, 1, &[3, 2])), InvalidBody),
        (
            "char-reserved",
            chr(char_body(0, 1, 1, &[3, 2])),
            InvalidBody,
        ),
        ("char-trailing", chr(trailing), InvalidBody),
        (
            "char-unknown-attr-0",
            chr(char_body(0, 0, 1, &[0, 2])),
            InvalidBody,
        ),
        (
            "char-unknown-attr-10",
            chr(char_body(0, 0, 1, &[10, 2])),
            InvalidBody,
        ),
        (
            "char-unknown-op-0",
            chr(char_body(0, 0, 1, &[3, 0])),
            InvalidBody,
        ),
        (
            "char-unknown-op-8",
            chr(char_body(0, 0, 1, &[3, 8])),
            InvalidBody,
        ),
        (
            "char-duplicate-attr",
            chr(char_body(0, 0, 2, &[3, 2, 3, 3])),
            InvalidBody,
        ),
        (
            "char-toggle-font",
            chr(char_body(0, 0, 1, &[1, 3])),
            InvalidBody,
        ),
        (
            "char-step-bold",
            chr(char_body(0, 0, 1, &[3, 4])),
            InvalidBody,
        ),
        (
            "char-toggle-size",
            chr(char_body(0, 0, 1, &[2, 3])),
            InvalidBody,
        ),
        (
            "char-size-too-small",
            chr(char_body(0, 0, 1, &cat(&[&[2, 1], &u16le(1)]))),
            InvalidBody,
        ),
        (
            "char-size-too-large",
            chr(char_body(0, 0, 1, &cat(&[&[2, 1], &u16le(3277)]))),
            InvalidBody,
        ),
        (
            "char-bold-flag",
            chr(char_body(0, 0, 1, &[3, 1, 2])),
            InvalidBody,
        ),
        (
            "char-font-empty",
            chr(char_body(0, 0, 1, &cat(&[&[1, 1], &u16le(0)]))),
            InvalidBody,
        ),
        (
            "char-font-control",
            chr(char_body(0, 0, 1, &cat(&[&[1, 1], &u16le(2), b"a\n"]))),
            InvalidBody,
        ),
        (
            "char-font-utf8",
            chr(char_body(0, 0, 1, &cat(&[&[1, 1], &u16le(1), &[0xff]]))),
            InvalidUtf8,
        ),
        (
            "char-font-too-long",
            chr(char_body(0, 0, 1, &cat(&[&[1, 1], &u16le(256), &long_id]))),
            InvalidBody,
        ),
        (
            "char-underline-7",
            chr(char_body(0, 0, 1, &[5, 1, 7])),
            InvalidBody,
        ),
        (
            "char-toggle-underline-none",
            chr(char_body(0, 0, 1, &[5, 3, 0])),
            InvalidBody,
        ),
        (
            "char-valign-3",
            chr(char_body(0, 0, 1, &[7, 1, 3])),
            InvalidBody,
        ),
        (
            "char-color-short",
            chr(char_body(0, 0, 1, &[8, 1, 255, 0])),
            InvalidBody,
        ),
        (
            "para-count-zero",
            par(para_body(0, [0; 2], &[])),
            InvalidBody,
        ),
        (
            "para-reserved",
            par(para_body(1, [0, 1], &[1, 2])),
            InvalidBody,
        ),
        (
            "para-unknown-attr",
            par(para_body(1, [0; 2], &[10, 2])),
            InvalidBody,
        ),
        (
            "para-toggle-alignment",
            par(para_body(1, [0; 2], &[1, 3])),
            InvalidBody,
        ),
        (
            "para-step-space",
            par(para_body(1, [0; 2], &[5, 4])),
            InvalidBody,
        ),
        (
            "para-step-right-indent",
            par(para_body(1, [0; 2], &[3, 4])),
            InvalidBody,
        ),
        (
            "para-alignment-4",
            par(para_body(1, [0; 2], &[1, 1, 4])),
            InvalidBody,
        ),
        (
            "para-indent-too-large",
            par(para_body(1, [0; 2], &cat(&[&[2, 1], &u32le(31_681)]))),
            InvalidBody,
        ),
        (
            "para-space-negative",
            par(para_body(1, [0; 2], &cat(&[&[5, 1], &u32le(-1i32 as u32)]))),
            InvalidBody,
        ),
        (
            "para-line-auto-14",
            par(para_body(1, [0; 2], &cat(&[&[7, 1, 0], &u32le(14)]))),
            InvalidBody,
        ),
        (
            "para-line-rule-3",
            par(para_body(1, [0; 2], &cat(&[&[7, 1, 3], &u32le(240)]))),
            InvalidBody,
        ),
        (
            "para-line-exact-zero",
            par(para_body(1, [0; 2], &cat(&[&[7, 1, 1], &u32le(0)]))),
            InvalidBody,
        ),
        (
            "para-border-style",
            par(para_body(
                1,
                [0; 2],
                &borders_body(with_side(0, side(2, 4, 4, 1, 0, [0; 3]))),
            )),
            InvalidBody,
        ),
        (
            "para-border-width-1",
            par(para_body(
                1,
                [0; 2],
                &borders_body(with_side(0, side(2, 0, 1, 1, 0, [0; 3]))),
            )),
            InvalidBody,
        ),
        (
            "para-border-width-97",
            par(para_body(
                1,
                [0; 2],
                &borders_body(with_side(0, side(2, 0, 97, 1, 0, [0; 3]))),
            )),
            InvalidBody,
        ),
        (
            "para-border-space-32",
            par(para_body(
                1,
                [0; 2],
                &borders_body(with_side(0, side(2, 0, 4, 32, 0, [0; 3]))),
            )),
            InvalidBody,
        ),
        (
            "para-border-color-mode",
            par(para_body(
                1,
                [0; 2],
                &borders_body(with_side(0, side(2, 0, 4, 1, 2, [0; 3]))),
            )),
            InvalidBody,
        ),
        (
            "para-border-auto-color-bytes",
            par(para_body(
                1,
                [0; 2],
                &borders_body(with_side(0, side(2, 0, 4, 1, 0, [1, 0, 0]))),
            )),
            InvalidBody,
        ),
        (
            "para-border-kind-3",
            par(para_body(
                1,
                [0; 2],
                &borders_body(with_side(0, side(3, 0, 0, 0, 0, [0; 3]))),
            )),
            InvalidBody,
        ),
        (
            "para-border-keep-bytes",
            par(para_body(
                1,
                [0; 2],
                &borders_body(with_side(1, side(0, 1, 0, 0, 0, [0; 3]))),
            )),
            InvalidBody,
        ),
        (
            "para-border-short",
            par(para_body(1, [0; 2], &[9, 1, 2, 0])),
            InvalidBody,
        ),
        (
            "list-styles-filter",
            frame(opcode::LIST_STYLES, &[3, 0, 0, 0]),
            InvalidBody,
        ),
        (
            "list-styles-flag",
            frame(opcode::LIST_STYLES, &[0, 2, 0, 0]),
            InvalidBody,
        ),
        (
            "list-styles-reserved",
            frame(opcode::LIST_STYLES, &[0, 0, 1, 0]),
            InvalidBody,
        ),
        (
            "apply-style-paragraph-empty",
            frame(opcode::APPLY_STYLE, &style_body(1, 0, [0; 3], b"")),
            InvalidBody,
        ),
        (
            "apply-style-target",
            frame(opcode::APPLY_STYLE, &style_body(1, 2, [0; 3], b"a")),
            InvalidBody,
        ),
        (
            "apply-style-reserved",
            frame(opcode::APPLY_STYLE, &style_body(1, 0, [0, 1, 0], b"a")),
            InvalidBody,
        ),
        (
            "apply-style-utf8",
            frame(opcode::APPLY_STYLE, &style_body(1, 0, [0; 3], &[0xff])),
            InvalidUtf8,
        ),
        (
            "apply-style-too-long",
            frame(opcode::APPLY_STYLE, &style_body(1, 0, [0; 3], &long_id)),
            InvalidBody,
        ),
        (
            "preview-show-empty",
            frame(opcode::PREVIEW_STYLE, &preview_body(0, 0, [0; 2], b"")),
            InvalidBody,
        ),
        (
            "preview-cancel-with-id",
            frame(opcode::PREVIEW_STYLE, &preview_body(0, 1, [0; 2], b"a")),
            InvalidBody,
        ),
        (
            "preview-action",
            frame(opcode::PREVIEW_STYLE, &preview_body(0, 2, [0; 2], b"a")),
            InvalidBody,
        ),
        (
            "preview-reserved",
            frame(opcode::PREVIEW_STYLE, &preview_body(0, 0, [1, 0], b"a")),
            InvalidBody,
        ),
    ]
}

/// A `FormatState` body whose first fields are `prefix` (`fields` of them);
/// the 25 fields (ten of characters, fifteen of paragraphs) are completed
/// with `Unset`.
fn state_body(flags: u8, prefix: &[u8], fields: usize) -> Vec<u8> {
    let mut b = cat(&[&u64le(1), &u64le(0), &u64le(0), &[flags, 0, 0, 0], prefix]);
    b.extend(std::iter::repeat_n(2u8, 25 - fields));
    b
}

/// A `FontList` body with one family `a` of the given flags and substitute.
fn font_body(flags: u8, substitute: &[u8]) -> Vec<u8> {
    cat(&[
        &u64le(0),
        &u16le(0),
        &u16le(0),
        &u32le(1),
        &u16le(1),
        b"a",
        &[flags],
        &u16le(substitute.len() as u16),
        substitute,
    ])
}

/// A `FormatMarks` body with one mark.
fn marks_body(page: u32, kind: u8, reserved: [u8; 3], width: f64) -> Vec<u8> {
    cat(&[
        &u64le(0),
        &u32le(1),
        &u32le(page),
        &[kind],
        &reserved,
        &1.0f64.to_bits().to_le_bytes(),
        &2.0f64.to_bits().to_le_bytes(),
        &width.to_bits().to_le_bytes(),
    ])
}

fn bad_replies() -> Vec<(&'static str, Vec<u8>, ErrorCode)> {
    use ErrorCode::*;
    let good_state = |flags: u8| state_body(flags, &[], 0);
    let mut state_extra = good_state(0);
    state_extra.push(0);
    let mut bad_style_flags = cat(&[&u64le(0), &u32le(1)]);
    bad_style_flags.extend(cat(&[
        &u16le(1),
        b"a",
        &u16le(0),
        &u16le(0),
        &[0, 0x20, 0, 0],
        &u16le(0),
        &u16le(0),
        &[0, 0, 0, 0, 0, 0, 0, 0],
    ]));
    vec![
        (
            "state-flags",
            frame(opcode::R_FORMAT_STATE, &good_state(0x20)),
            InvalidBody,
        ),
        (
            "state-reserved",
            frame(
                opcode::R_FORMAT_STATE,
                &cat(&[&u64le(1), &u64le(0), &u64le(0), &[0, 0, 1, 0], &[2]]),
            ),
            InvalidBody,
        ),
        (
            "state-trailing",
            frame(opcode::R_FORMAT_STATE, &state_extra),
            InvalidBody,
        ),
        (
            "state-tri-3",
            frame(opcode::R_FORMAT_STATE, &state_body(0, &[3], 1)),
            InvalidBody,
        ),
        (
            "state-size-too-small",
            frame(opcode::R_FORMAT_STATE, &state_body(0, &[2, 1, 1, 0], 2)),
            InvalidBody,
        ),
        (
            "state-short",
            frame(opcode::R_FORMAT_STATE, &u64le(1)),
            InvalidBody,
        ),
        (
            "style-list-count",
            frame(
                opcode::R_STYLE_LIST,
                &cat(&[&u64le(0), &u32le(MAX_STYLE_LIST as u32 + 1)]),
            ),
            InvalidBody,
        ),
        (
            "style-list-short",
            frame(opcode::R_STYLE_LIST, &cat(&[&u64le(0), &u32le(1)])),
            InvalidBody,
        ),
        (
            "style-list-flags",
            frame(opcode::R_STYLE_LIST, &bad_style_flags),
            InvalidBody,
        ),
        (
            "font-list-flags",
            frame(opcode::R_FONT_LIST, &font_body(0x10, b"")),
            InvalidBody,
        ),
        (
            "font-list-substitute-missing",
            frame(opcode::R_FONT_LIST, &font_body(8, b"")),
            InvalidBody,
        ),
        (
            "font-list-substitute-without-flag",
            frame(opcode::R_FONT_LIST, &font_body(0, b"b")),
            InvalidBody,
        ),
        (
            "font-list-count",
            frame(
                opcode::R_FONT_LIST,
                &cat(&[
                    &u64le(0),
                    &u16le(0),
                    &u16le(0),
                    &u32le(MAX_FONT_LIST as u32 + 1),
                ]),
            ),
            InvalidBody,
        ),
        (
            "marks-kind",
            frame(opcode::R_FORMAT_MARKS, &marks_body(0, 7, [0; 3], 1.0)),
            InvalidBody,
        ),
        (
            "marks-width",
            frame(opcode::R_FORMAT_MARKS, &marks_body(0, 0, [0; 3], -1.0)),
            InvalidBody,
        ),
        (
            "marks-page",
            frame(
                opcode::R_FORMAT_MARKS,
                &marks_body(MAX_PAGES, 0, [0; 3], 1.0),
            ),
            PageRange,
        ),
        (
            "marks-reserved",
            frame(opcode::R_FORMAT_MARKS, &marks_body(0, 0, [0, 1, 0], 1.0)),
            InvalidBody,
        ),
        (
            "marks-count",
            frame(
                opcode::R_FORMAT_MARKS,
                &cat(&[&u64le(0), &u32le(MAX_FORMAT_MARKS as u32 + 1)]),
            ),
            InvalidBody,
        ),
    ]
}

fn render_vectors() -> String {
    let mut out = String::from(
        "# Frozen ATED v2 vectors of the formatting family (F0: FormatState, ApplyCharFormat, ApplyParaFormat, ListStyles, ApplyStyle, PreviewStyle and the replies FormatState and StyleList). Columns: kind, name, frame hex, expectation.\n\
         # Generated by the Rust tests of rich_protocol/format_tests.rs (ATELIER_REGEN_VECTORS=1), verified by Rust and tests/rich-editor-format-vectors.mjs.\n",
    );
    for (name, r) in requests() {
        let _ = writeln!(
            out,
            "request\t{name}\t{}\t{}",
            hex(&encode_request(&r).unwrap()),
            dump_request(&r)
        );
    }
    for (name, r) in replies() {
        let _ = writeln!(
            out,
            "reply\t{name}\t{}\t{}",
            hex(&encode_reply(&r).unwrap()),
            dump_reply(&r)
        );
    }
    for (name, f, code) in bad_requests() {
        let _ = writeln!(out, "bad-request\t{name}\t{}\t{}", hex(&f), code.name());
    }
    for (name, f, code) in bad_replies() {
        let _ = writeln!(out, "bad-reply\t{name}\t{}\t{}", hex(&f), code.name());
    }
    out
}

#[test]
fn frozen_vectors_match_encoder() {
    let rendered = render_vectors();
    if std::env::var_os("ATELIER_REGEN_VECTORS").is_some() {
        std::fs::write(VECTORS, &rendered).unwrap();
    }
    let frozen = std::fs::read_to_string(VECTORS)
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(frozen, rendered, "vectors are frozen");
}

#[test]
fn valid_frames_round_trip() {
    for (name, r) in requests() {
        let bytes = encode_request(&r).unwrap();
        assert_eq!(decode_request(&bytes).unwrap(), r, "{name}");
    }
    for (name, r) in replies() {
        let bytes = encode_reply(&r).unwrap();
        assert_eq!(decode_reply(&bytes).unwrap(), r, "{name}");
    }
}

#[test]
fn invalid_frames_have_stable_codes() {
    for (name, bytes, code) in bad_requests() {
        let error = decode_request(&bytes).unwrap_err();
        assert_eq!(error.code, code, "{name}");
        assert_eq!(error.context, Some((7, 1)), "{name}");
    }
    for (name, bytes, code) in bad_replies() {
        let error = decode_reply(&bytes).unwrap_err();
        assert_eq!(error.code, code, "{name}");
    }
}

#[test]
fn every_truncation_of_a_valid_frame_is_refused_without_panicking() {
    for (name, r) in requests() {
        let bytes = encode_request(&r).unwrap();
        for cut in 0..bytes.len() {
            assert!(decode_request(&bytes[..cut]).is_err(), "{name} cut {cut}");
        }
    }
    for (name, r) in replies() {
        let bytes = encode_reply(&r).unwrap();
        for cut in 0..bytes.len() {
            assert!(decode_reply(&bytes[..cut]).is_err(), "{name} cut {cut}");
        }
    }
}

#[test]
fn encoders_refuse_what_the_decoders_would() {
    let char_format = |edits: Vec<CharEdit>| {
        encode_request(&req(
            1,
            Command::ApplyCharFormat(CharFormat {
                base_revision: 0,
                scope: FormatScope::WordOrTyping,
                edits,
            }),
        ))
    };
    let bold = || char_edit(CharAttr::Bold, EditOp::Toggle, None);
    assert!(char_format(vec![bold()]).is_ok());
    assert_eq!(char_format(vec![]), Err(ErrorCode::InvalidBody));
    assert_eq!(
        char_format(vec![bold(), bold()]),
        Err(ErrorCode::InvalidBody)
    );
    // A value on an operation that takes none, a missing value, a wrong kind.
    assert_eq!(
        char_format(vec![char_edit(
            CharAttr::Bold,
            EditOp::Toggle,
            Some(CharValue::Flag(true))
        )]),
        Err(ErrorCode::InvalidBody)
    );
    assert_eq!(
        char_format(vec![char_edit(CharAttr::Bold, EditOp::Set, None)]),
        Err(ErrorCode::InvalidBody)
    );
    assert_eq!(
        char_format(vec![char_edit(
            CharAttr::Bold,
            EditOp::Set,
            Some(CharValue::Half(3))
        )]),
        Err(ErrorCode::InvalidBody)
    );
    assert_eq!(
        char_format(vec![char_edit(
            CharAttr::Font,
            EditOp::Set,
            Some(CharValue::Text("a".repeat(MAX_NAME_BYTES + 1)))
        )]),
        Err(ErrorCode::InvalidBody)
    );
    let too_many: Vec<StyleEntry> = (0..=MAX_STYLE_LIST)
        .map(|i| style_entry(&format!("S{i}"), StyleTarget::Paragraph, 0))
        .collect();
    assert_eq!(
        encode_reply(&reply(
            1,
            Response::StyleList {
                revision: 0,
                styles: too_many
            }
        )),
        Err(ErrorCode::InvalidBody)
    );
}
