// SPDX-License-Identifier: AGPL-3.0-or-later
//! Paragraph providers: the rich document and plain text (US-340).

use super::{ParagraphSource, Position, SearchParagraph, Zone};
use crate::PlainTextDocument;
use crate::rich::{Node, NodeId, RichDocument};

/// Identifier of a plain-text paragraph (a line): the byte offset, in the
/// whole text, of its first byte.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LineId(pub usize);

/// Plain text seen as paragraphs: lines separated by LF, CRLF or lone CR. The
/// terminators belong to no paragraph; a text ending with a terminator has a
/// last, empty paragraph.
#[derive(Clone, Copy, Debug)]
pub struct TextLines<'a>(pub &'a str);

impl TextLines<'_> {
    /// The position `offset` (a global byte offset) as a line and a local
    /// offset. An offset inside a CRLF pair is moved before the pair.
    pub fn position(&self, offset: usize) -> Position<LineId> {
        let bytes = self.0.as_bytes();
        let mut o = offset.min(bytes.len());
        if o > 0 && bytes[o - 1] == b'\r' && bytes.get(o) == Some(&b'\n') {
            o -= 1;
        }
        let start = bytes[..o]
            .iter()
            .rposition(|&b| b == b'\n' || b == b'\r')
            .map_or(0, |p| p + 1);
        Position {
            paragraph: LineId(start),
            offset: o - start,
        }
    }
}

impl ParagraphSource for TextLines<'_> {
    type Id = LineId;

    fn paragraphs(&self) -> Vec<SearchParagraph<'_, LineId>> {
        lines(self.0)
    }
}

fn lines(text: &str) -> Vec<SearchParagraph<'_, LineId>> {
    {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let (mut start, mut i) = (0, 0);
        let mut push = |start: usize, end: usize| {
            out.push(SearchParagraph {
                id: LineId(start),
                zone: Zone::Body,
                section: 0,
                text: &text[start..end],
            });
        };
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => {
                    push(start, i);
                    i += 1;
                    start = i;
                }
                b'\r' => {
                    push(start, i);
                    i += if bytes.get(i + 1) == Some(&b'\n') {
                        2
                    } else {
                        1
                    };
                    start = i;
                }
                _ => i += 1,
            }
        }
        push(start, bytes.len());
        out
    }
}

impl ParagraphSource for PlainTextDocument {
    type Id = LineId;

    fn paragraphs(&self) -> Vec<SearchParagraph<'_, LineId>> {
        lines(self.text())
    }
}

impl ParagraphSource for RichDocument {
    type Id = NodeId;

    /// Per section: headers, body, footers. Tables (nested ones too) are
    /// walked row by row, cell by cell.
    fn paragraphs(&self) -> Vec<SearchParagraph<'_, NodeId>> {
        let mut out = Vec::new();
        for (section, id) in self.section_ids().into_iter().enumerate() {
            let Some(Node::Section(s)) = self.node(id) else {
                continue;
            };
            for (_, h) in &s.headers {
                walk(self, vec![*h], Zone::Header, section, &mut out);
            }
            walk(self, s.body.to_vec(), Zone::Body, section, &mut out);
            for (_, f) in &s.footers {
                walk(self, vec![*f], Zone::Footer, section, &mut out);
            }
        }
        out
    }
}

fn walk<'a>(
    doc: &'a RichDocument,
    roots: Vec<NodeId>,
    region: Zone,
    section: usize,
    out: &mut Vec<SearchParagraph<'a, NodeId>>,
) {
    // Explicit stack: the nesting depth is not bounded by the model.
    let mut stack: Vec<(NodeId, bool)> = roots.into_iter().rev().map(|r| (r, false)).collect();
    while let Some((id, in_table)) = stack.pop() {
        match doc.node(id) {
            Some(Node::Paragraph(p)) => out.push(SearchParagraph {
                id,
                zone: if in_table && region == Zone::Body {
                    Zone::Table
                } else {
                    region
                },
                section,
                text: &p.text,
            }),
            Some(node @ (Node::Table(_) | Node::Row(_))) => {
                stack.extend(node.children().into_iter().rev().map(|c| (c, true)));
            }
            Some(node @ (Node::Cell(_) | Node::HeaderFooter(_))) => {
                stack.extend(node.children().into_iter().rev().map(|c| (c, in_table)));
            }
            Some(Node::Section(_) | Node::Opaque(_)) | None => {}
        }
    }
}
