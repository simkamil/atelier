// SPDX-License-Identifier: AGPL-3.0-or-later
//! Frozen vectors and refusals of the messages of R7b-2 (`Search`,
//! `Statistics`, `SearchRects`, `Replace`, `ReplaceAll`, `PropertiesRead`,
//! `PropertiesApply` and their replies). Vectors:
//! `tests/rich-editor-find-vectors.txt`, verified here and by
//! `tests/rich-editor-find-vectors.mjs` (`ATELIER_REGEN_VECTORS=1` rewrites them).

use super::*;
use std::fmt::Write as _;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/rich-editor-find-vectors.txt"
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

fn flags_byte(f: SearchFlags) -> u8 {
    u8::from(f.match_case)
        | u8::from(f.whole_word) << 1
        | u8::from(f.ignore_accents) << 2
        | u8::from(f.match_nbsp) << 3
}

fn counts(c: &StatCounts) -> String {
    format!(
        "{},{},{},{},{},{}",
        c.words, c.characters, c.characters_with_spaces, c.paragraphs, c.lines, c.pages
    )
}

fn describe_state(s: &State) -> String {
    format!(
        "revision={} anchor={} focus={} doc_len={} modified={}",
        s.revision,
        s.anchor,
        s.focus,
        s.doc_len,
        u8::from(s.modified)
    )
}

fn join_or_none(parts: Vec<String>) -> String {
    if parts.is_empty() {
        "none".into()
    } else {
        parts.join(";")
    }
}

fn describe_request(r: &Request) -> String {
    let head = format!("doc={} req={}", r.doc_id, r.request_id);
    let tail = match &r.command {
        Command::Search {
            revision,
            flags,
            query,
        } => format!(
            "Search revision={revision} flags={} query={}",
            flags_byte(*flags),
            hex(query.as_bytes())
        ),
        Command::Statistics => "Statistics".into(),
        Command::SearchRects {
            revision,
            first_page,
            page_count,
            ranges,
        } => format!(
            "SearchRects revision={revision} first={first_page} pages={page_count} ranges={}",
            join_or_none(ranges.iter().map(|(a, b)| format!("{a}-{b}")).collect())
        ),
        Command::Replace {
            revision,
            flags,
            start,
            end,
            query,
            replacement,
        } => format!(
            "Replace revision={revision} flags={} range={start}-{end} query={} replacement={}",
            flags_byte(*flags),
            hex(query.as_bytes()),
            hex(replacement.as_bytes())
        ),
        Command::ReplaceAll {
            revision,
            flags,
            query,
            replacement,
        } => format!(
            "ReplaceAll revision={revision} flags={} query={} replacement={}",
            flags_byte(*flags),
            hex(query.as_bytes()),
            hex(replacement.as_bytes())
        ),
        Command::PropertiesRead => "PropertiesRead".into(),
        Command::PropertiesApply { changes } => format!(
            "PropertiesApply changes={}",
            join_or_none(
                changes
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| c.as_ref().map(|c| format!("{i}:{}", hex(c.as_bytes()))))
                    .collect()
            )
        ),
        other => panic!("not an R7b-2 command: {other:?}"),
    };
    format!("{head} {tail}")
}

fn date(d: &PropertyDateField) -> String {
    match d {
        None => "none".into(),
        Some((valid, text)) => format!("{}:{}", u8::from(*valid), hex(text.as_bytes())),
    }
}

type PropertyDateField = Option<(bool, String)>;

fn describe_reply(r: &Reply) -> String {
    let head = format!("doc={} req={}", r.doc_id, r.request_id);
    let tail = match &r.response {
        Response::SearchResult {
            revision,
            total,
            truncated,
            total_capped,
            items,
        } => format!(
            "SearchResult revision={revision} total={total} flags={} items={}",
            u8::from(*truncated) | u8::from(*total_capped) << 1,
            join_or_none(
                items
                    .iter()
                    .map(|i| format!(
                        "{}-{}/{}/{}-{}/{}",
                        i.start,
                        i.end,
                        hex(i.excerpt.text.as_bytes()),
                        i.excerpt.start,
                        i.excerpt.end,
                        u8::from(i.excerpt.truncated_before)
                            | u8::from(i.excerpt.truncated_after) << 1
                    ))
                    .collect()
            )
        ),
        Response::Statistics {
            revision,
            anchor,
            focus,
            layout_complete,
            document,
            selection,
        } => format!(
            "Statistics revision={revision} anchor={anchor} focus={focus} complete={} document={} selection={}",
            u8::from(*layout_complete),
            counts(document),
            counts(selection)
        ),
        Response::SearchRects { revision, rects } => format!(
            "SearchRects revision={revision} rects={}",
            join_or_none(
                rects
                    .iter()
                    .map(|r| format!(
                        "{}:{}:{}:{}:{}:{}",
                        r.item, r.page, r.x, r.y, r.width, r.height
                    ))
                    .collect()
            )
        ),
        Response::Replaced { count, state } => {
            format!("Replaced count={count} {}", describe_state(state))
        }
        Response::Properties { revision, snapshot } => format!(
            "Properties revision={revision} editable={} limits={} dates={} strings={} total_time={} statistics={} notices={}",
            snapshot
                .editable
                .iter()
                .map(|t| hex(t.as_bytes()))
                .collect::<Vec<_>>()
                .join(","),
            snapshot
                .limits
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
            snapshot
                .dates
                .iter()
                .map(date)
                .collect::<Vec<_>>()
                .join(","),
            snapshot
                .strings
                .iter()
                .map(|s| s
                    .as_ref()
                    .map_or("none".into(), |s| format!("1:{}", hex(s.as_bytes()))))
                .collect::<Vec<_>>()
                .join(","),
            snapshot.total_time.map_or("none".into(), |n| n.to_string()),
            snapshot
                .statistics
                .iter()
                .map(|n| n.map_or("none".into(), |n| n.to_string()))
                .collect::<Vec<_>>()
                .join(","),
            snapshot.notices
        ),
        Response::PropertiesApplied(o) => format!(
            "PropertiesApplied status={} changed={} field={} max={} len={} {}",
            o.status as u8,
            o.changed,
            o.field,
            o.max,
            o.len,
            describe_state(&o.state)
        ),
        Response::Error(code) => format!("Error code={}", code.name()),
        other => panic!("not an R7b-2 reply: {other:?}"),
    };
    format!("{head} {tail}")
}

fn small_state(revision: u64, modified: bool) -> State {
    State {
        revision,
        anchor: 3,
        focus: 5,
        focus_upstream: false,
        doc_len: 20,
        modified,
        can_undo: modified,
        can_redo: false,
        layout_complete: true,
        page_count: 1,
        caret: None,
        invalidated: Vec::new(),
        atlas: AtlasDelta {
            epoch: 1,
            first: 0,
            entries: Vec::new(),
        },
        pages: Vec::new(),
        window: None,
        read_only: false,
        has_revisions: false,
    }
}

fn item(start: u64, end: u64, text: &str, from: u32, to: u32, flags: (bool, bool)) -> SearchItem {
    SearchItem {
        start,
        end,
        excerpt: SearchExcerpt {
            text: text.into(),
            start: from,
            end: to,
            truncated_before: flags.0,
            truncated_after: flags.1,
        },
    }
}

fn snapshot() -> PropertiesSnapshot {
    let mut s = PropertiesSnapshot::default();
    s.editable[0] = "Titre \u{e9}t\u{e9}".into();
    s.editable[2] = "Alice".into();
    s.limits = [255, 255, 255, 255, 255, 255, 1024, 8192, 2048, 255, 35];
    s.dates[0] = Some((true, "2024-05-06T07:08:09Z".into()));
    s.dates[1] = Some((false, "not a date".into()));
    s.strings[0] = Some("Bob".into());
    s.strings[2] = Some("Atelier".into());
    s.total_time = Some(12);
    s.statistics = [Some(2), Some(120), None, Some(700), None, Some(9)];
    s.notices = 1;
    s
}

fn flags(a: bool, b: bool, c: bool, d: bool) -> SearchFlags {
    SearchFlags {
        match_case: a,
        whole_word: b,
        ignore_accents: c,
        match_nbsp: d,
    }
}

fn requests() -> Vec<(&'static str, Request)> {
    let mut changes: [Option<String>; PROPERTY_COUNT] = Default::default();
    changes[0] = Some("Titre".into());
    changes[6] = Some(String::new());
    changes[10] = Some("fr-FR".into());
    vec![
        (
            "search",
            req(
                1,
                Command::Search {
                    revision: 4,
                    flags: flags(true, false, true, false),
                    query: "caf\u{e9}".into(),
                },
            ),
        ),
        (
            "search-empty-query",
            req(
                2,
                Command::Search {
                    revision: 0,
                    flags: SearchFlags::default(),
                    query: String::new(),
                },
            ),
        ),
        ("statistics", req(3, Command::Statistics)),
        (
            "search-rects",
            req(
                4,
                Command::SearchRects {
                    revision: 4,
                    first_page: 2,
                    page_count: 3,
                    ranges: vec![(10, 14), (20, 24)],
                },
            ),
        ),
        (
            "replace",
            req(
                5,
                Command::Replace {
                    revision: 9,
                    flags: flags(false, true, false, true),
                    start: 10,
                    end: 14,
                    query: "chat".into(),
                    replacement: "chien\n".into(),
                },
            ),
        ),
        (
            "replace-all-delete",
            req(
                6,
                Command::ReplaceAll {
                    revision: 9,
                    flags: SearchFlags::default(),
                    query: "x".into(),
                    replacement: String::new(),
                },
            ),
        ),
        ("properties-read", req(7, Command::PropertiesRead)),
        (
            "properties-apply",
            req(8, Command::PropertiesApply { changes }),
        ),
    ]
}

fn replies() -> Vec<(&'static str, Reply)> {
    vec![
        (
            "search-result",
            reply(
                1,
                Response::SearchResult {
                    revision: 4,
                    total: 12_000_u32.min(10_000),
                    truncated: true,
                    total_capped: true,
                    items: vec![
                        item(10, 14, "un chat noir", 3, 7, (false, true)),
                        item(20, 24, "chat", 0, 4, (false, false)),
                    ],
                },
            ),
        ),
        (
            "search-result-empty",
            reply(
                2,
                Response::SearchResult {
                    revision: 0,
                    total: 0,
                    truncated: false,
                    total_capped: false,
                    items: Vec::new(),
                },
            ),
        ),
        (
            "statistics",
            reply(
                3,
                Response::Statistics {
                    revision: 4,
                    anchor: 3,
                    focus: 9,
                    layout_complete: true,
                    document: StatCounts {
                        words: 12,
                        characters: 60,
                        characters_with_spaces: 71,
                        paragraphs: 3,
                        lines: 5,
                        pages: 1,
                    },
                    selection: StatCounts {
                        words: 1,
                        characters: 5,
                        characters_with_spaces: 6,
                        paragraphs: 1,
                        lines: 1,
                        pages: 1,
                    },
                },
            ),
        ),
        (
            "search-rects",
            reply(
                4,
                Response::SearchRects {
                    revision: 4,
                    rects: vec![
                        HighlightRect {
                            item: 0,
                            page: 2,
                            x: 72.0,
                            y: 100.5,
                            width: 30.25,
                            height: 14.0,
                        },
                        HighlightRect {
                            item: 0,
                            page: 3,
                            x: 72.0,
                            y: 72.0,
                            width: 12.0,
                            height: 14.0,
                        },
                        HighlightRect {
                            item: 1,
                            page: 2,
                            x: 100.0,
                            y: 130.0,
                            width: 24.5,
                            height: 14.0,
                        },
                    ],
                },
            ),
        ),
        (
            "search-rects-empty",
            reply(
                5,
                Response::SearchRects {
                    revision: 4,
                    rects: Vec::new(),
                },
            ),
        ),
        (
            "replaced",
            reply(
                6,
                Response::Replaced {
                    count: 3,
                    state: small_state(5, true),
                },
            ),
        ),
        (
            "properties",
            reply(
                7,
                Response::Properties {
                    revision: 4,
                    snapshot: snapshot(),
                },
            ),
        ),
        (
            "properties-applied",
            reply(
                8,
                Response::PropertiesApplied(PropertiesOutcome {
                    status: PropertiesStatus::Applied,
                    changed: 2,
                    field: 0,
                    max: 0,
                    len: 0,
                    state: small_state(0, true),
                }),
            ),
        ),
        (
            "properties-too-long",
            reply(
                9,
                Response::PropertiesApplied(PropertiesOutcome {
                    status: PropertiesStatus::TooLong,
                    changed: 0,
                    field: 7,
                    max: 8192,
                    len: 9000,
                    state: small_state(0, false),
                }),
            ),
        ),
        (
            "empty-query-error",
            reply(10, Response::Error(ErrorCode::EmptyQuery)),
        ),
        (
            "stale-match-error",
            reply(11, Response::Error(ErrorCode::StaleMatch)),
        ),
        (
            "query-too-large-error",
            reply(12, Response::Error(ErrorCode::QueryTooLarge)),
        ),
        (
            "properties-failed-error",
            reply(13, Response::Error(ErrorCode::PropertiesFailed)),
        ),
        (
            "too-many-matches-error",
            reply(14, Response::Error(ErrorCode::TooManyMatches)),
        ),
    ]
}

fn frame(op: u16, body: &[u8]) -> Vec<u8> {
    header(op, 7, 1, body).unwrap()
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

/// A `Search` body: revision, flags byte, reserved bytes, query.
fn search_body(flags: u8, reserved: [u8; 3], query: &[u8]) -> Vec<u8> {
    cat(&[
        &u64le(1),
        &[flags],
        &reserved,
        &u32le(query.len() as u32),
        query,
    ])
}

/// A `Replace` (range given) or `ReplaceAll` body.
fn replace_body(range: Option<(u64, u64)>, query: &[u8], replacement: &[u8]) -> Vec<u8> {
    let mut b = cat(&[&u64le(1), &[0, 0, 0, 0]]);
    if let Some((s, e)) = range {
        b.extend(u64le(s));
        b.extend(u64le(e));
    }
    b.extend(u32le(query.len() as u32));
    b.extend_from_slice(query);
    b.extend(u32le(replacement.len() as u32));
    b.extend_from_slice(replacement);
    b
}

fn rects_body(first: u32, pages: u32, ranges: &[(u64, u64)]) -> Vec<u8> {
    let mut b = cat(&[
        &u64le(1),
        &u32le(first),
        &u32le(pages),
        &u32le(ranges.len() as u32),
    ]);
    for (s, e) in ranges {
        b.extend(u64le(*s));
        b.extend(u64le(*e));
    }
    b
}

fn apply_body(entries: &[(u8, &[u8])]) -> Vec<u8> {
    let mut b = Vec::new();
    for i in 0..PROPERTY_COUNT {
        let (flag, text) = entries.get(i).copied().unwrap_or((0, b""));
        b.push(flag);
        b.extend(u32le(text.len() as u32));
        b.extend_from_slice(text);
    }
    b
}

fn bad_requests() -> Vec<(&'static str, Vec<u8>, ErrorCode)> {
    use ErrorCode::*;
    let big_field = vec![b'a'; MAX_PROPERTY_BYTES + 1];
    let big_query = vec![b'a'; MAX_QUERY_BYTES + 1];
    let mut trailing = search_body(0, [0; 3], b"a");
    trailing.push(0);
    let many: Vec<(u64, u64)> = (0..=MAX_HIGHLIGHT_RANGES as u64)
        .map(|i| (i, i + 1))
        .collect();
    vec![
        (
            "search-flags",
            frame(opcode::SEARCH, &search_body(0x10, [0; 3], b"a")),
            InvalidBody,
        ),
        (
            "search-reserved",
            frame(opcode::SEARCH, &search_body(0, [0, 1, 0], b"a")),
            InvalidBody,
        ),
        (
            "search-query-too-large",
            frame(opcode::SEARCH, &search_body(0, [0; 3], &big_query)),
            QueryTooLarge,
        ),
        (
            "search-invalid-utf8",
            frame(opcode::SEARCH, &search_body(0, [0; 3], &[0xff, 0xfe])),
            InvalidUtf8,
        ),
        (
            "search-trailing",
            frame(opcode::SEARCH, &trailing),
            InvalidBody,
        ),
        (
            "search-short",
            frame(opcode::SEARCH, &[1, 0, 0]),
            InvalidBody,
        ),
        (
            "statistics-body",
            frame(opcode::STATISTICS, &[0]),
            InvalidBody,
        ),
        (
            "rects-no-pages",
            frame(opcode::SEARCH_RECTS, &rects_body(0, 0, &[(0, 1)])),
            InvalidBody,
        ),
        (
            "rects-too-many-pages",
            frame(opcode::SEARCH_RECTS, &rects_body(0, 17, &[(0, 1)])),
            TooManyPages,
        ),
        (
            "rects-first-page",
            frame(opcode::SEARCH_RECTS, &rects_body(MAX_PAGES, 1, &[(0, 1)])),
            PageRange,
        ),
        (
            "rects-no-ranges",
            frame(opcode::SEARCH_RECTS, &rects_body(0, 1, &[])),
            InvalidBody,
        ),
        (
            "rects-too-many-ranges",
            frame(opcode::SEARCH_RECTS, &rects_body(0, 1, &many)),
            InvalidBody,
        ),
        (
            "rects-reversed-range",
            frame(opcode::SEARCH_RECTS, &rects_body(0, 1, &[(5, 2)])),
            InvalidRange,
        ),
        (
            "replace-empty-query",
            frame(opcode::REPLACE, &replace_body(Some((0, 1)), b"", b"x")),
            EmptyQuery,
        ),
        (
            "replace-all-empty-query",
            frame(opcode::REPLACE_ALL, &replace_body(None, b"", b"x")),
            EmptyQuery,
        ),
        (
            "replace-object-query",
            frame(
                opcode::REPLACE_ALL,
                &replace_body(None, "a\u{fffc}".as_bytes(), b"x"),
            ),
            InvalidText,
        ),
        (
            "replace-cr",
            frame(opcode::REPLACE_ALL, &replace_body(None, b"a", b"x\r")),
            InvalidText,
        ),
        (
            "replace-reversed-range",
            frame(opcode::REPLACE, &replace_body(Some((4, 2)), b"a", b"x")),
            InvalidRange,
        ),
        (
            "replace-query-too-large",
            frame(opcode::REPLACE_ALL, &replace_body(None, &big_query, b"x")),
            QueryTooLarge,
        ),
        (
            "replace-text-too-large",
            frame(
                opcode::REPLACE_ALL,
                &replace_body(None, b"a", &vec![b'a'; MAX_TEXT_BYTES + 1]),
            ),
            FrameTooLarge,
        ),
        (
            "properties-read-body",
            frame(opcode::PROPERTIES_READ, &[0]),
            InvalidBody,
        ),
        (
            "properties-apply-flag",
            frame(opcode::PROPERTIES_APPLY, &apply_body(&[(2, b"")])),
            InvalidBody,
        ),
        (
            "properties-apply-absent-with-text",
            frame(opcode::PROPERTIES_APPLY, &apply_body(&[(0, b"a")])),
            InvalidBody,
        ),
        (
            "properties-apply-too-long",
            frame(opcode::PROPERTIES_APPLY, &apply_body(&[(1, &big_field)])),
            FrameTooLarge,
        ),
        (
            "properties-apply-utf8",
            frame(opcode::PROPERTIES_APPLY, &apply_body(&[(1, &[0xc3])])),
            InvalidUtf8,
        ),
        (
            "properties-apply-short",
            frame(opcode::PROPERTIES_APPLY, &[0, 0, 0, 0, 0]),
            InvalidBody,
        ),
    ]
}

fn result_body(total: u32, flags: u8, items: &[(u64, u64, &[u8], u32, u32, u8)]) -> Vec<u8> {
    let mut b = cat(&[
        &u64le(1),
        &u32le(total),
        &[flags, 0, 0, 0],
        &u32le(items.len() as u32),
    ]);
    for (s, e, text, from, to, f) in items {
        b.extend(u64le(*s));
        b.extend(u64le(*e));
        b.extend(u32le(text.len() as u32));
        b.extend(u32le(*from));
        b.extend(u32le(*to));
        b.extend_from_slice(&[*f, 0, 0, 0]);
        b.extend_from_slice(text);
    }
    b
}

fn rect_reply(rects: &[(u32, u32, f64)]) -> Vec<u8> {
    let mut b = cat(&[&u64le(1), &u32le(rects.len() as u32)]);
    for (item, page, x) in rects {
        b.extend(u32le(*item));
        b.extend(u32le(*page));
        for v in [*x, 1.0, 2.0, 3.0] {
            b.extend(v.to_bits().to_le_bytes());
        }
    }
    b
}

fn bad_replies() -> Vec<(&'static str, Vec<u8>, ErrorCode)> {
    use ErrorCode::*;
    let stats_head = |flag: u8| cat(&[&u64le(1), &u64le(0), &u64le(0), &[flag, 0, 0, 0]]);
    let mut stats_flag = stats_head(2);
    stats_flag.extend(vec![0u8; 48]);
    let mut stats_pages = stats_head(1);
    stats_pages.extend(vec![0u8; 20]);
    stats_pages.extend(u32le(MAX_PAGES + 1));
    stats_pages.extend(vec![0u8; 24]);
    let applied = |status: u8, changed: u16, field: u8, max: u32, len: u32| {
        let mut b = vec![status, 0];
        b.extend(changed.to_le_bytes());
        b.extend([field, 0, 0, 0]);
        b.extend(u32le(max));
        b.extend(u32le(len));
        let mut state = Vec::new();
        encode_state(&small_state(0, false), &mut state).unwrap();
        b.extend(state);
        b
    };
    vec![
        (
            "result-flags",
            frame(opcode::R_SEARCH, &result_body(1, 4, &[])),
            InvalidBody,
        ),
        (
            "result-count-over-total",
            frame(
                opcode::R_SEARCH,
                &result_body(0, 0, &[(0, 1, b"a", 0, 1, 0)]),
            ),
            InvalidBody,
        ),
        (
            "result-empty-range",
            frame(
                opcode::R_SEARCH,
                &result_body(1, 0, &[(2, 2, b"a", 0, 1, 0)]),
            ),
            InvalidBody,
        ),
        (
            "result-unordered",
            frame(
                opcode::R_SEARCH,
                &result_body(2, 0, &[(5, 8, b"abc", 0, 3, 0), (6, 9, b"abc", 0, 3, 0)]),
            ),
            InvalidBody,
        ),
        (
            "result-excerpt-outside",
            frame(
                opcode::R_SEARCH,
                &result_body(1, 0, &[(0, 1, b"a", 0, 2, 0)]),
            ),
            InvalidBody,
        ),
        (
            "result-excerpt-splits-scalar",
            frame(
                opcode::R_SEARCH,
                &result_body(1, 0, &[(0, 1, "\u{e9}".as_bytes(), 0, 1, 0)]),
            ),
            InvalidBody,
        ),
        (
            "result-excerpt-flags",
            frame(
                opcode::R_SEARCH,
                &result_body(1, 0, &[(0, 1, b"a", 0, 1, 4)]),
            ),
            InvalidBody,
        ),
        (
            "result-excerpt-utf8",
            frame(
                opcode::R_SEARCH,
                &result_body(1, 0, &[(0, 1, &[0xff], 0, 1, 0)]),
            ),
            InvalidUtf8,
        ),
        (
            "statistics-flag",
            frame(opcode::R_STATISTICS, &stats_flag),
            InvalidBody,
        ),
        (
            "statistics-pages",
            frame(opcode::R_STATISTICS, &stats_pages),
            TooManyPages,
        ),
        (
            "rects-unordered",
            frame(
                opcode::R_SEARCH_RECTS,
                &rect_reply(&[(1, 0, 1.0), (0, 0, 1.0)]),
            ),
            InvalidBody,
        ),
        (
            "rects-page",
            frame(opcode::R_SEARCH_RECTS, &rect_reply(&[(0, MAX_PAGES, 1.0)])),
            InvalidBody,
        ),
        (
            "applied-status",
            frame(opcode::R_PROPERTIES_APPLIED, &applied(2, 0, 0, 0, 0)),
            InvalidBody,
        ),
        (
            "applied-field-with-applied",
            frame(opcode::R_PROPERTIES_APPLIED, &applied(0, 1, 3, 0, 0)),
            InvalidBody,
        ),
        (
            "applied-too-long-short-value",
            frame(opcode::R_PROPERTIES_APPLIED, &applied(1, 0, 3, 255, 255)),
            InvalidBody,
        ),
        (
            "applied-too-long-field",
            frame(opcode::R_PROPERTIES_APPLIED, &applied(1, 0, 11, 255, 300)),
            InvalidBody,
        ),
        (
            "applied-changed-count",
            frame(opcode::R_PROPERTIES_APPLIED, &applied(0, 12, 0, 0, 0)),
            InvalidBody,
        ),
    ]
}

fn render_vectors() -> String {
    let mut out = String::from(
        "# Frozen ATED v2 vectors of R7b-2 (Search, Statistics, SearchRects, Replace, ReplaceAll, PropertiesRead, PropertiesApply and their replies). Columns: kind, name, frame hex, expectation.\n\
         # Generated by the Rust tests of rich_protocol/find_tests.rs (ATELIER_REGEN_VECTORS=1), verified by Rust and tests/rich-editor-find-vectors.mjs.\n",
    );
    for (name, r) in requests() {
        let _ = writeln!(
            out,
            "request\t{name}\t{}\t{}",
            hex(&encode_request(&r).unwrap()),
            describe_request(&r)
        );
    }
    for (name, r) in replies() {
        let _ = writeln!(
            out,
            "reply\t{name}\t{}\t{}",
            hex(&encode_reply(&r).unwrap()),
            describe_reply(&r)
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
    let search = |query: String| {
        encode_request(&req(
            1,
            Command::Search {
                revision: 0,
                flags: SearchFlags::default(),
                query,
            },
        ))
    };
    assert!(search("a".repeat(MAX_QUERY_BYTES)).is_ok());
    assert_eq!(
        search("a".repeat(MAX_QUERY_BYTES + 1)),
        Err(ErrorCode::QueryTooLarge)
    );
    let too_many = Response::SearchResult {
        revision: 0,
        total: 2000,
        truncated: true,
        total_capped: false,
        items: (0..=MAX_SEARCH_ITEMS as u64)
            .map(|i| item(i * 2, i * 2 + 1, "a", 0, 1, (false, false)))
            .collect(),
    };
    assert_eq!(
        encode_reply(&reply(1, too_many)),
        Err(ErrorCode::InvalidBody)
    );
    let long_excerpt = Response::SearchResult {
        revision: 0,
        total: 1,
        truncated: false,
        total_capped: false,
        items: vec![item(
            0,
            1,
            &"a".repeat(MAX_EXCERPT_BYTES + 1),
            0,
            1,
            (false, false),
        )],
    };
    assert_eq!(
        encode_reply(&reply(1, long_excerpt)),
        Err(ErrorCode::InvalidBody)
    );
    let full = Response::SearchRects {
        revision: 0,
        rects: (0..MAX_HIGHLIGHT_RECTS as u32)
            .map(|i| HighlightRect {
                item: i,
                page: 0,
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            })
            .collect(),
    };
    let frame = encode_reply(&reply(1, full)).unwrap();
    let Response::SearchRects { rects, .. } = decode_reply(&frame).unwrap().response else {
        panic!("expected rectangles");
    };
    assert_eq!(rects.len(), MAX_HIGHLIGHT_RECTS);
}
