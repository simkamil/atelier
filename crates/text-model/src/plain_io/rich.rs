// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rich-document side of text files: one paragraph per line on the way
//! in, flattening and the formatting-loss report on the way out.
//!
//! Export rules (what [`rich_to_text`] writes):
//! - every paragraph of every section body is one line, in order; a line break
//!   inside a paragraph's text is also a break, so the line count can exceed
//!   the paragraph count;
//! - a table gives one line per row, the cells separated by one TAB (the rule
//!   of Word's "plain text" export); a cell spanning `n` columns is followed
//!   by `n - 1` more TABs to keep the columns aligned; a slot covered by a
//!   cell spanning down from an earlier row is an empty field, so every row
//!   has the same number of TABs; the text of a merged cell is written once,
//!   in its first row (Word's rule). When cells are merged, the model moves
//!   the covered cells' paragraphs into the merged cell, so it holds several
//!   paragraphs: they are joined by one space (`r1c0 r2c0`), which comes from
//!   the merge, not from the export;
//!   the paragraphs of a cell, and the text of a table nested in it, are
//!   joined by one space and inner breaks become spaces, so a row is always
//!   one line;
//! - fields give their displayed result, links their text;
//! - list numbers and bullets are generated, not stored: they are not written;
//! - anchored objects, opaque blocks, bookmarks, headers and footers are not
//!   written.

use super::{TextIoError, push_with_breaks};
use crate::rich::{Node, NodeId, Paragraph, RichDocument};

/// Most columns of a table taken into account when flattening.
const MAX_COLUMNS: usize = 1 << 12;

/// An item met while walking blocks in document order.
enum Item<'a> {
    Paragraph(&'a Paragraph),
    Table,
    Opaque,
}

/// Walks `roots` and everything below them (tables, rows, cells) in document
/// order, with an explicit stack: no recursion depth to exhaust.
fn walk(doc: &RichDocument, roots: &[NodeId], visit: &mut dyn FnMut(Item<'_>)) {
    let mut stack: Vec<NodeId> = roots.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        let Some(node) = doc.node(id) else { continue };
        match node {
            Node::Paragraph(p) => visit(Item::Paragraph(p)),
            Node::Opaque(_) => visit(Item::Opaque),
            Node::Table(_) => {
                visit(Item::Table);
                stack.extend(node.children().into_iter().rev());
            }
            Node::Row(_) | Node::Cell(_) => stack.extend(node.children().into_iter().rev()),
            Node::Section(_) | Node::HeaderFooter(_) => {}
        }
    }
}

/// Builds a rich document with one default-style paragraph per line of
/// `text` (split on `\n`; a final `\n` gives a last empty paragraph).
///
/// The paragraphs are installed directly, not through undoable operations: the
/// result is a freshly loaded document with an empty history, and building
/// stays O(n log n) instead of cloning the document per transaction.
pub(crate) fn text_to_rich(text: &str, max_paragraphs: usize) -> Result<RichDocument, TextIoError> {
    let count = text.bytes().filter(|&b| b == b'\n').count() + 1;
    if count > max_paragraphs {
        return Err(TextIoError::TooManyLines {
            limit: max_paragraphs,
            actual: count,
        });
    }
    let mut doc = RichDocument::new();
    let section = doc.section_ids()[0];
    let first = doc.block_ids(Some(section)).map_err(TextIoError::Rich)?[0];
    let mut lines = text.split('\n');
    if let Some(Node::Paragraph(p)) = doc.nodes.get_mut(&first) {
        p.text.push_str(lines.next().unwrap_or(""));
    }
    for (index, line) in lines.enumerate() {
        let paragraph = doc.new_paragraph(line).map_err(TextIoError::Rich)?;
        let id = paragraph.root;
        doc.commit_install(paragraph);
        if let Some(Node::Section(s)) = doc.nodes.get_mut(&section) {
            s.body.insert(index + 1, id).map_err(TextIoError::Rich)?;
        }
    }
    Ok(doc)
}

fn cell_text(doc: &RichDocument, cell: NodeId) -> String {
    let mut out = String::new();
    let mut first = true;
    walk(doc, &[cell], &mut |item| {
        if let Item::Paragraph(p) = item {
            if !first {
                out.push(' ');
            }
            first = false;
            push_with_breaks(&mut out, &p.text, " ");
        }
    });
    out
}

/// The text a save as text writes, lines separated by `\n` (see the module
/// documentation for the rules).
pub fn rich_to_text(doc: &RichDocument) -> String {
    let mut out = String::new();
    let mut first_line = true;
    let mut start_line = |out: &mut String| {
        if !first_line {
            out.push('\n');
        }
        first_line = false;
    };
    for section in doc.section_ids() {
        let Some(Node::Section(s)) = doc.node(section) else {
            continue;
        };
        for block in s.body.to_vec() {
            match doc.node(block) {
                Some(Node::Paragraph(p)) => {
                    start_line(&mut out);
                    push_with_breaks(&mut out, &p.text, "\n");
                }
                Some(Node::Table(t)) => {
                    // Grid slots still covered by a cell spanning down.
                    let columns = t.columns.clamp(1, MAX_COLUMNS);
                    let mut covered = vec![0usize; columns];
                    for row in &t.rows {
                        let Some(Node::Row(r)) = doc.node(*row) else {
                            continue;
                        };
                        start_line(&mut out);
                        let mut fields: Vec<String> = Vec::new();
                        let mut column = 0;
                        let skip_covered =
                            |column: &mut usize,
                             fields: &mut Vec<String>,
                             covered: &mut [usize]| {
                                while *column < columns && covered[*column] > 0 {
                                    covered[*column] -= 1;
                                    fields.push(String::new());
                                    *column += 1;
                                }
                            };
                        for cell in &r.cells {
                            skip_covered(&mut column, &mut fields, &mut covered);
                            fields.push(cell_text(doc, *cell));
                            let (col_span, row_span) = match doc.node(*cell) {
                                Some(Node::Cell(c)) => (c.col_span.max(1), c.row_span.max(1)),
                                _ => (1, 1),
                            };
                            for k in 0..col_span.min(columns.saturating_sub(column).max(1)) {
                                if k > 0 {
                                    fields.push(String::new());
                                }
                                if row_span > 1 && column + k < columns {
                                    covered[column + k] = row_span - 1;
                                }
                            }
                            column += col_span.min(columns);
                        }
                        skip_covered(&mut column, &mut fields, &mut covered);
                        out.push_str(&fields.join("\t"));
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// A kind of information a save as text discards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LossCategory {
    CharacterFormatting,
    ParagraphFormatting,
    Styles,
    Tables,
    Objects,
    Fields,
    Links,
    Bookmarks,
    HeadersFooters,
    Lists,
    SectionBreaks,
}

impl LossCategory {
    /// Proposed catalogue key of the line describing the category.
    pub const fn catalog_key(self) -> &'static str {
        match self {
            Self::CharacterFormatting => "text-loss-character-formatting",
            Self::ParagraphFormatting => "text-loss-paragraph-formatting",
            Self::Styles => "text-loss-styles",
            Self::Tables => "text-loss-tables",
            Self::Objects => "text-loss-objects",
            Self::Fields => "text-loss-fields",
            Self::Links => "text-loss-links",
            Self::Bookmarks => "text-loss-bookmarks",
            Self::HeadersFooters => "text-loss-headers-footers",
            Self::Lists => "text-loss-lists",
            Self::SectionBreaks => "text-loss-section-breaks",
        }
    }
}

/// What saving a rich document as text discards, with counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FormattingLoss {
    /// Runs of characters with direct attributes (bold, font, colour...).
    pub character_runs: usize,
    /// Paragraphs with direct properties other than style and list
    /// (alignment, indents, spacing...).
    pub formatted_paragraphs: usize,
    /// Paragraphs that reference a style.
    pub styled_paragraphs: usize,
    /// Tables, nested ones included: flattened to TAB-separated lines.
    pub tables: usize,
    /// Anchored objects (pictures, shapes...) and opaque blocks: dropped.
    pub objects: usize,
    /// Fields: the displayed result is kept, the field itself is lost.
    pub fields: usize,
    /// Links: the text is kept, the target is lost.
    pub links: usize,
    pub bookmarks: usize,
    /// Headers and footers holding something; not exported.
    pub headers_footers: usize,
    /// Paragraphs in a list: numbers and bullets are not written.
    pub list_items: usize,
    /// Section breaks (sections beyond the first).
    pub section_breaks: usize,
}

impl FormattingLoss {
    /// Nothing would be lost: no warning needed.
    pub fn is_empty(&self) -> bool {
        self.categories().is_empty()
    }

    /// The categories with a non-zero count, in a fixed order.
    pub fn categories(&self) -> Vec<(LossCategory, usize)> {
        [
            (LossCategory::CharacterFormatting, self.character_runs),
            (LossCategory::ParagraphFormatting, self.formatted_paragraphs),
            (LossCategory::Styles, self.styled_paragraphs),
            (LossCategory::Tables, self.tables),
            (LossCategory::Objects, self.objects),
            (LossCategory::Fields, self.fields),
            (LossCategory::Links, self.links),
            (LossCategory::Bookmarks, self.bookmarks),
            (LossCategory::HeadersFooters, self.headers_footers),
            (LossCategory::Lists, self.list_items),
            (LossCategory::SectionBreaks, self.section_breaks),
        ]
        .into_iter()
        .filter(|&(_, n)| n > 0)
        .collect()
    }
}

/// Inspects a rich document and reports what a save as text discards.
pub fn formatting_loss(doc: &RichDocument) -> FormattingLoss {
    let mut loss = FormattingLoss::default();
    let sections = doc.section_ids();
    loss.section_breaks = sections.len().saturating_sub(1);
    let mut count = |item: Item<'_>| match item {
        Item::Paragraph(p) => {
            loss.character_runs += p.runs.len();
            let mut direct = p.props.clone();
            direct.style = None;
            direct.list = None;
            loss.formatted_paragraphs += usize::from(!direct.is_empty());
            loss.styled_paragraphs += usize::from(p.props.style.is_some());
            loss.list_items += usize::from(p.props.list.is_some());
            loss.objects += p.anchored.len();
            loss.fields += p.fields.len();
            loss.links += p.links.len();
            loss.bookmarks += p.bookmarks.len();
        }
        Item::Table => loss.tables += 1,
        Item::Opaque => loss.objects += 1,
    };
    let mut parts = Vec::new();
    for section in &sections {
        let Some(Node::Section(s)) = doc.node(*section) else {
            continue;
        };
        walk(doc, &s.body.to_vec(), &mut count);
        parts.extend(s.headers.iter().chain(&s.footers).map(|(_, id)| *id));
    }
    for part in parts {
        let Some(Node::HeaderFooter(hf)) = doc.node(part) else {
            continue;
        };
        let mut holds_something = false;
        walk(doc, &hf.blocks.to_vec(), &mut |item| match item {
            Item::Paragraph(p) => holds_something |= !p.text.is_empty(),
            _ => holds_something = true,
        });
        loss.headers_footers += usize::from(holds_something);
    }
    loss
}
