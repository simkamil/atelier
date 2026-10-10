// SPDX-License-Identifier: AGPL-3.0-or-later
//! Agent-only driver for independently generated Python/FontTools expectations.
use atelier_font::{FontFace, VariationGlyph};
use atelier_model::journal::*;
use atelier_model::selection::*;
use atelier_model::{
    CodecError, Decoder, Encoder, IndexedTree, Operation, Transaction, TransactionalState,
};
use std::io::{self, BufRead};

#[derive(Clone)]
struct State {
    value: u64,
    resource: Vec<u8>,
}
impl TransactionalState for State {
    type Checkpoint = Self;
    fn checkpoint(&self) -> Self {
        self.clone()
    }
    fn restore(&mut self, checkpoint: Self) {
        *self = checkpoint;
    }
}
struct Codec;
impl SnapshotCodec<State> for Codec {
    type Error = &'static str;
    fn encode(&self, state: &State) -> Result<Vec<u8>, Self::Error> {
        let mut bytes = state.value.to_le_bytes().to_vec();
        bytes.extend_from_slice(&state.resource);
        Ok(bytes)
    }
    fn restore(&self, bytes: &[u8]) -> Result<State, Self::Error> {
        if bytes.len() < 8 {
            return Err("short state");
        }
        Ok(State {
            value: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            resource: bytes[8..].to_vec(),
        })
    }
}
struct Set(u64);
impl Operation<State> for Set {
    type Error = &'static str;
    fn apply(&self, state: &mut State) -> Result<(), Self::Error> {
        state.value = self.0;
        if self.0 == 999 {
            state.resource.clear();
            return Err("deliberate failure after mutation");
        }
        Ok(())
    }
    fn invert(&self, state: &State) -> Result<Self, Self::Error> {
        Ok(Self(state.value))
    }
    fn encode(&self, encoder: &mut Encoder) -> Result<(), CodecError> {
        encoder.write_u64(self.0)
    }
    fn decode(decoder: &mut Decoder<'_>) -> Result<Self, CodecError> {
        Ok(Self(decoder.read_u64()?))
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 32 => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
fn font(path: &str, index: usize) {
    let bytes = std::fs::read(path).unwrap();
    match FontFace::parse_collection(&bytes, index) {
        Err(error) => println!("{{\"error\":{:?}}}", format!("{error:?}")),
        Ok(face) => {
            let m = face.metrics();
            let cps = [0, 32, 64, 65, 66, 67, 68, 65535, 128512, 1114111];
            let glyphs: Vec<String> = cps
                .into_iter()
                .map(|cp| {
                    format!(
                        "\"{}\":{}",
                        cp,
                        face.glyph_index(char::from_u32(cp).unwrap())
                    )
                })
                .collect();
            let metrics: Vec<String> = (0..m.number_of_glyphs)
                .map(|g| {
                    let v = face.horizontal_metric(g).unwrap();
                    format!("[{},{}]", v.advance_width, v.left_side_bearing)
                })
                .collect();
            let variations: Vec<String> = ['A', 'B', 'C']
                .into_iter()
                .map(|c| {
                    let (kind, gid) = match face.variation_glyph(c, '\u{fe0f}') {
                        VariationGlyph::Default => ("default", face.glyph_index(c)),
                        VariationGlyph::NonDefault(g) => ("non-default", g),
                        VariationGlyph::Unsupported => ("unsupported", 0),
                    };
                    format!("\"{}:65039\":[\"{}\",{}]", c as u32, kind, gid)
                })
                .collect();
            let names: Vec<_> = face
                .names()
                .iter()
                .filter(|n| {
                    n.platform_id == 0 || n.platform_id == 3 && matches!(n.encoding_id, 1 | 10)
                })
                .map(|n| {
                    format!(
                        "[{},{},{},{},{}]",
                        n.platform_id,
                        n.encoding_id,
                        n.language_id,
                        n.name_id,
                        json_string(&n.value)
                    )
                })
                .collect();
            println!(
                "{{\"glyphs\":{{{}}},\"metrics\":[{}],\"units_per_em\":{},\"glyph_count\":{},\"hhea\":[{},{},{}],\"variations\":{{{}}},\"unicode_names\":[{}]}}",
                glyphs.join(","),
                metrics.join(","),
                m.units_per_em,
                m.number_of_glyphs,
                m.ascender,
                m.descender,
                m.line_gap,
                variations.join(","),
                names.join(",")
            );
        }
    }
}
fn selection() {
    let mut tree = IndexedTree::new();
    let ids: Vec<_> = (0..3).map(|i| tree.insert(i, i).unwrap()).collect();
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let a: Vec<usize> = line
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect();
        let affinity = |n| {
            if n == 0 {
                Affinity::Before
            } else {
                Affinity::After
            }
        };
        let pos = |n, o, f| TextPosition::new(ids[n], o, affinity(f));
        let event = match a[0] {
            0 => PositionMap::Insert {
                node: ids[0],
                at: a[1],
                byte_len: a[2],
            },
            1 => PositionMap::Delete {
                node: ids[0],
                start: a[1],
                end: a[2],
            },
            2 => PositionMap::Split {
                source: ids[0],
                right: ids[1],
                at: a[1],
            },
            3 => PositionMap::Merge {
                left: ids[0],
                right: ids[1],
                left_len: a[1],
            },
            _ => PositionMap::Remove {
                node: ids[0],
                fallback: pos(2, a[1], a[2]),
            },
        };
        let mut sel = Selection::new(
            vec![
                SelectionRange::new(pos(a[3], a[4], a[5]), pos(a[6], a[7], a[8])),
                SelectionRange::caret(ids[2], 7),
            ],
            1,
        )
        .unwrap();
        let original = sel.clone();
        if sel.transform(event).is_err() {
            assert_eq!(sel, original);
            println!("error");
            continue;
        }
        assert_eq!(sel.primary_index(), 1);
        assert_eq!(sel.ranges()[1], original.ranges()[1]);
        let range = sel.ranges()[0];
        let fmt = |p: TextPosition| {
            format!(
                "{} {} {}",
                ids.iter().position(|id| *id == p.node).unwrap(),
                p.offset,
                usize::from(p.affinity == Affinity::After)
            )
        };
        println!("{} {}", fmt(range.anchor), fmt(range.head));
    }
}
fn replay_files(snapshot: &str, log: &str) {
    let s = std::fs::read(snapshot).unwrap();
    let j = std::fs::read(log).unwrap();
    match replay::<State, Set, _>(&Codec, &s, &j, JournalLimits::default()) {
        Err(e) => println!("{{\"error\":{:?}}}", format!("{e:?}")),
        Ok(r) => println!(
            "{{\"value\":{},\"resource\":\"{}\",\"sequence\":{},\"applied\":{},\"stopped\":{:?}}}",
            r.state.value,
            r.state
                .resource
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            r.last_sequence,
            r.applied_entries,
            format!("{:?}", r.stopped)
        ),
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a[1].as_str() {
        "font" => font(&a[2], a[3].parse().unwrap()),
        "selection" => selection(),
        "emit" => {
            let state = State {
                value: 0,
                resource: b"embedded\0resource".to_vec(),
            };
            let doc = RecoveryId([1; 16]);
            let generation = RecoveryId([2; 16]);
            std::fs::write(
                &a[2],
                snapshot_state(&Codec, &state, doc, generation, 7, JournalLimits::default())
                    .unwrap(),
            )
            .unwrap();
            let mut journal = Journal::new(doc, generation, 7, JournalLimits::default()).unwrap();
            for value in [1, 2, 3] {
                journal
                    .append::<State, _>(&Transaction::new(vec![Set(value)]).unwrap())
                    .unwrap();
            }
            std::fs::write(&a[3], journal.as_bytes()).unwrap();
            println!("{{}}");
        }
        "replay" => replay_files(&a[2], &a[3]),
        "font-fuzz" => {
            let bytes = std::fs::read(&a[2]).unwrap();
            let mut count = 0;
            let exercise = |data: &[u8]| {
                for index in 0..2 {
                    if let Ok(f) = FontFace::parse_collection(data, index) {
                        for cp in ['A', 'B', 'C', '\u{ffff}', '\u{1f600}'] {
                            let _ = f.glyph_index(cp);
                            let _ = f.variation_glyph(cp, '\u{fe0f}');
                        }
                        for g in 0..f.metrics().number_of_glyphs.min(32) {
                            let _ = f.horizontal_metric(g);
                        }
                    }
                }
            };
            for end in 0..bytes.len() {
                exercise(&bytes[..end]);
                count += 1;
            }
            for i in 0..bytes.len() {
                for value in [0, 255] {
                    let mut data = bytes.clone();
                    data[i] = value;
                    exercise(&data);
                    count += 1;
                }
            }
            println!("{{\"font_mutations\":{count}}}");
        }
        _ => panic!("unknown command"),
    }
}
