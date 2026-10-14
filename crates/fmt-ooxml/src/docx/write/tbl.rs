// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tables of the DOCX writer: the skeleton of a source table with the model's
//! rows and cells deciding what exists, and tables built from the model alone.

use super::super::{XmlNode, parse_xml_bytes, tables::dom_bytes};
use super::body::Gen;
use super::props::local_of;
use crate::PartUri;
use atelier_model::NodeId;
use atelier_text_model::rich::{Cell, Node, Row, Table};
use atelier_xml::{Attribute, Limits as XmlLimits, Name};

/// What fills one grid position range of a row.
#[derive(Clone, Copy, Debug)]
pub(super) enum Slot {
    Cell(NodeId, usize, usize),
    /// Continuation of a cell spanning down from a row above.
    Cont(usize, usize),
}

/// Slots of every row of a table: the cells of the model with the vertical
/// merge continuations that the model leaves implicit.
pub(super) fn layout(doc: &atelier_text_model::rich::RichDocument, t: &Table) -> Vec<Vec<Slot>> {
    let columns = t.columns.max(1);
    let mut remaining = vec![0usize; columns];
    let mut origin_span = vec![1usize; columns];
    let mut rows = Vec::with_capacity(t.rows.len());
    for row_id in &t.rows {
        let cells: Vec<NodeId> = match doc.node(*row_id) {
            Some(Node::Row(r)) => r.cells.clone(),
            _ => Vec::new(),
        };
        let mut iter = cells.into_iter();
        let mut slots = Vec::new();
        let mut col = 0;
        while col < columns {
            if remaining[col] > 0 {
                let span = origin_span[col].clamp(1, columns - col);
                slots.push(Slot::Cont(col, span));
                for r in &mut remaining[col..col + span] {
                    *r = r.saturating_sub(1);
                }
                col += span;
            } else if let Some(cid) = iter.next() {
                let (span, row_span) = match doc.node(cid) {
                    Some(Node::Cell(c)) => (c.col_span.max(1), c.row_span.max(1)),
                    _ => (1, 1),
                };
                let span = span.min(columns - col);
                slots.push(Slot::Cell(cid, col, span));
                if row_span > 1 {
                    for r in &mut remaining[col..col + span] {
                        *r = row_span - 1;
                    }
                    origin_span[col] = span;
                }
                col += span;
            } else {
                break;
            }
        }
        // Cells beyond the grid (invalid input): kept at the end.
        for cid in iter {
            slots.push(Slot::Cell(cid, columns, 1));
        }
        rows.push(slots);
    }
    rows
}

const TC_PR_ORDER: &[&str] = &[
    "cnfStyle",
    "tcW",
    "gridSpan",
    "hMerge",
    "vMerge",
    "tcBorders",
    "shd",
    "noWrap",
    "tcMar",
    "textDirection",
    "tcFitText",
    "vAlign",
    "hideMark",
    "headers",
    "cellIns",
    "cellDel",
    "cellMerge",
    "tcPrChange",
];

fn tc_rank(local: &str) -> usize {
    TC_PR_ORDER
        .iter()
        .position(|n| *n == local)
        .unwrap_or(TC_PR_ORDER.len())
}

fn child_of(parent: &XmlNode, local: &str, val: Option<&str>, width: bool) -> XmlNode {
    let name = |l: &str| Name {
        prefix: parent.name.prefix.clone(),
        local: l.to_owned(),
        namespace: parent.name.namespace.clone(),
    };
    XmlNode {
        name: name(local),
        attrs: val
            .map(|v| Attribute {
                name: name(if width { "w" } else { "val" }),
                value: v.to_owned(),
            })
            .into_iter()
            .collect(),
        children: Vec::new(),
        text: String::new(),
        opaque: None,
    }
}

fn parse(bytes: &[u8]) -> Option<XmlNode> {
    let uri = PartUri::parse("word/document.xml").ok()?;
    parse_xml_bytes(bytes, &uri, &XmlLimits::default(), None).ok()
}

/// A `w:tcPr` made consistent with the spans of the cell.
fn patch_tc_pr(bytes: &[u8], cell: &Cell) -> Vec<u8> {
    let Some(mut node) = parse(bytes) else {
        return bytes.to_vec();
    };
    let span = node
        .children
        .iter()
        .find(|c| c.name.local == "gridSpan")
        .and_then(|c| c.attrs.iter().find(|a| a.name.local == "val"))
        .and_then(|a| a.value.parse::<usize>().ok())
        .unwrap_or(1);
    let restart = node.children.iter().any(|c| {
        c.name.local == "vMerge"
            && c.attrs
                .iter()
                .any(|a| a.name.local == "val" && a.value == "restart")
    });
    let mut changed = false;
    if span != cell.col_span.max(1) {
        node.children.retain(|c| c.name.local != "gridSpan");
        if cell.col_span > 1 {
            let new = child_of(&node, "gridSpan", Some(&cell.col_span.to_string()), false);
            insert(&mut node, new);
        }
        changed = true;
    }
    if cell.row_span > 1 && !restart {
        node.children.retain(|c| c.name.local != "vMerge");
        let new = child_of(&node, "vMerge", Some("restart"), false);
        insert(&mut node, new);
        changed = true;
    } else if cell.row_span <= 1 && restart {
        node.children.retain(|c| c.name.local != "vMerge");
        changed = true;
    }
    if changed {
        dom_bytes(&node)
    } else {
        bytes.to_vec()
    }
}

fn insert(parent: &mut XmlNode, child: XmlNode) {
    let r = tc_rank(&child.name.local);
    let at = parent
        .children
        .iter()
        .rposition(|c| tc_rank(&c.name.local) <= r)
        .map_or(0, |p| p + 1);
    parent.children.insert(at, child);
}

/// A `w:tblGrid` with `columns` columns.
fn patch_grid(bytes: &[u8], columns: usize, fallback: u32) -> Vec<u8> {
    let Some(mut node) = parse(bytes) else {
        return bytes.to_vec();
    };
    let have = node.children.iter().filter(|c| c.name.local == "gridCol").count();
    if have == columns {
        return bytes.to_vec();
    }
    if have > columns {
        let mut seen = 0;
        node.children.retain(|c| {
            if c.name.local == "gridCol" {
                seen += 1;
                seen <= columns
            } else {
                true
            }
        });
    } else {
        let width = node
            .children
            .iter()
            .rev()
            .find(|c| c.name.local == "gridCol")
            .and_then(|c| c.attrs.iter().find(|a| a.name.local == "w"))
            .map_or_else(|| fallback.to_string(), |a| a.value.clone());
        for _ in have..columns {
            let new = child_of(&node, "gridCol", Some(&width), true);
            node.children.push(new);
        }
    }
    dom_bytes(&node)
}

impl<'a> Gen<'a> {
    fn opaque_of(&self, owner: NodeId, name: &str) -> Option<Vec<u8>> {
        let list = match self.doc.node(owner)? {
            Node::Table(t) => &t.opaque,
            Node::Row(r) => &r.opaque,
            Node::Cell(c) => &c.opaque,
            _ => return None,
        };
        list.iter().find(|o| o.name == name).map(|o| o.value.clone())
    }

    /// A property container (`w:tblPr`, `w:trPr`, `w:tcPr`...) held by the
    /// `opaque` slot of the node, adjusted to the node's grid.
    pub fn emit_slot(&mut self, owner: NodeId, e: &super::super::preserve::PreservedElement) {
        let local = local_of(&e.name);
        let Some(bytes) = self.opaque_of(owner, local) else {
            return;
        };
        let out = match (self.doc.node(owner), local) {
            (Some(Node::Cell(c)), "tcPr") => patch_tc_pr(&bytes, c),
            (Some(Node::Table(t)), "tblGrid") => {
                patch_grid(&bytes, t.columns, self.text_width / u32::try_from(t.columns.max(1)).unwrap_or(1))
            }
            _ => bytes,
        };
        self.out.raw(&out);
    }

    pub fn emit_table(&mut self, id: NodeId) {
        let Some(Node::Table(t)) = self.doc.node(id) else {
            return;
        };
        if let Some(el) = self.pres_table(id).filter(|_| !self.structure_changed(id)) {
            self.out.start(
                &el.name,
                el.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
            );
            self.table_stack.push((id, None));
            let rows = t.rows.clone();
            self.emit_container(&el.children, &rows, Some(id));
            self.table_stack.pop();
            self.out.end();
        } else {
            self.gen_table(id);
        }
    }

    /// Whether the rows, cells or spans of a table differ from the import: the
    /// source grid (merged cells, folded continuations) no longer fits.
    fn structure_changed(&self, id: NodeId) -> bool {
        let Some(base) = self.base else {
            return true;
        };
        let (Some(Node::Table(now)), Some(Node::Table(then))) = (self.doc.node(id), base.node(id))
        else {
            return true;
        };
        if now.rows != then.rows || now.columns != then.columns {
            return true;
        }
        now.rows.iter().any(|r| match (self.doc.node(*r), base.node(*r)) {
            (Some(Node::Row(a)), Some(Node::Row(b))) => {
                a.cells != b.cells
                    || a.cells.iter().any(|c| match (self.doc.node(*c), base.node(*c)) {
                        (Some(Node::Cell(x)), Some(Node::Cell(y))) => {
                            x.col_span != y.col_span || x.row_span != y.row_span
                        }
                        _ => true,
                    })
            }
            _ => true,
        })
    }

    fn widths(&self, id: NodeId, columns: usize) -> Vec<u32> {
        let each = self.text_width / u32::try_from(columns.max(1)).unwrap_or(1);
        let known = self
            .tables_info
            .and_then(|t| t.tables.get(&id))
            .map(|t| t.grid.clone())
            .unwrap_or_default();
        (0..columns)
            .map(|i| known.get(i).copied().flatten().unwrap_or(each))
            .collect()
    }

    /// A table built from the model alone.
    fn gen_table(&mut self, id: NodeId) {
        let Some(Node::Table(t)) = self.doc.node(id) else {
            return;
        };
        if self.depth > 64 {
            self.failed = Some("nesting depth");
            return;
        }
        self.depth += 1;
        let w = self.p.w.clone();
        let widths = self.widths(id, t.columns);
        let slots = layout(self.doc, t);
        match self.pres_table(id) {
            Some(el) => self.out.start(
                &el.name,
                el.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
            ),
            None => self.out.start(&format!("{w}:tbl"), std::iter::empty()),
        }
        match self.opaque_of(id, "tblPr") {
            Some(b) => self.out.raw(&b),
            None => {
                self.out.start(&format!("{w}:tblPr"), std::iter::empty());
                if let Some(style) = &t.style {
                    self.out
                        .empty(&format!("{w}:tblStyle"), [(&*format!("{w}:val"), style.as_str())]);
                }
                self.out.empty(
                    &format!("{w}:tblW"),
                    [
                        (&*format!("{w}:w"), "0"),
                        (&*format!("{w}:type"), "auto"),
                    ],
                );
                self.out.empty(
                    &format!("{w}:tblLook"),
                    [
                        (&*format!("{w}:val"), "04A0"),
                        (&*format!("{w}:firstRow"), "1"),
                        (&*format!("{w}:lastRow"), "0"),
                        (&*format!("{w}:firstColumn"), "1"),
                        (&*format!("{w}:lastColumn"), "0"),
                        (&*format!("{w}:noHBand"), "0"),
                        (&*format!("{w}:noVBand"), "1"),
                    ],
                );
                self.out.end();
            }
        }
        match self.opaque_of(id, "tblGrid") {
            Some(b) => {
                let fallback = self.text_width / u32::try_from(t.columns.max(1)).unwrap_or(1);
                let patched = patch_grid(&b, t.columns, fallback);
                self.out.raw(&patched);
            }
            None => {
                self.out.start(&format!("{w}:tblGrid"), std::iter::empty());
                for width in &widths {
                    self.out
                        .empty(&format!("{w}:gridCol"), [(&*format!("{w}:w"), width.to_string().as_str())]);
                }
                self.out.end();
            }
        }
        self.table_stack.push((id, Some(slots)));
        for row in t.rows.clone() {
            self.check_limit();
            if self.failed.is_some() {
                break;
            }
            let doc = self.doc;
            if let Some(Node::Row(r)) = doc.node(row) {
                self.gen_row(row, r);
            }
        }
        self.table_stack.pop();
        self.out.end();
        self.depth -= 1;
    }

    pub fn emit_row(&mut self, id: NodeId) {
        let Some(Node::Row(r)) = self.doc.node(id) else {
            return;
        };
        if let Some(el) = self.pres_table(id) {
            self.out.start(
                &el.name,
                el.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
            );
            let cells = r.cells.clone();
            self.emit_container(&el.children, &cells, Some(id));
            self.out.end();
        } else {
            self.gen_row(id, r);
        }
    }

    fn gen_row(&mut self, id: NodeId, r: &Row) {
        let w = self.p.w.clone();
        let Some((table, cached)) = self.table_stack.last().cloned() else {
            return;
        };
        let Some(Node::Table(t)) = self.doc.node(table) else {
            return;
        };
        let slots = match cached {
            Some(all) => all,
            None => {
                let all = layout(self.doc, t);
                if let Some(top) = self.table_stack.last_mut() {
                    top.1 = Some(all.clone());
                }
                all
            }
        };
        let index = t.rows.iter().position(|x| *x == id).unwrap_or(0);
        let widths = self.widths(table, t.columns);
        let row_slots = slots.get(index).cloned().unwrap_or_default();
        match self.pres_table(id) {
            Some(el) => self.out.start(
                &el.name,
                el.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
            ),
            None => self.out.start(&format!("{w}:tr"), std::iter::empty()),
        }
        for name in ["tblPrEx", "trPr"] {
            if let Some(b) = r.opaque.iter().find(|o| o.name == name) {
                self.out.raw(&b.value);
            }
        }
        for slot in row_slots {
            match slot {
                Slot::Cell(cid, col, span) => {
                    let width: u32 = widths.iter().skip(col).take(span).sum();
                    self.gen_cell(cid, Some(width));
                }
                Slot::Cont(col, span) => {
                    let width: u32 = widths.iter().skip(col).take(span).sum();
                    self.gen_continuation(width, span);
                }
            }
        }
        self.out.end();
    }

    fn gen_continuation(&mut self, width: u32, span: usize) {
        let w = self.p.w.clone();
        self.out.start(&format!("{w}:tc"), std::iter::empty());
        self.out.start(&format!("{w}:tcPr"), std::iter::empty());
        self.tc_w(width);
        if span > 1 {
            self.out.empty(
                &format!("{w}:gridSpan"),
                [(&*format!("{w}:val"), span.to_string().as_str())],
            );
        }
        self.out.empty(&format!("{w}:vMerge"), std::iter::empty());
        self.out.end();
        self.out.empty(&format!("{w}:p"), std::iter::empty());
        self.out.end();
    }

    fn tc_w(&mut self, width: u32) {
        let w = self.p.w.clone();
        if width == 0 {
            self.out.empty(
                &format!("{w}:tcW"),
                [(&*format!("{w}:w"), "0"), (&*format!("{w}:type"), "auto")],
            );
        } else {
            self.out.empty(
                &format!("{w}:tcW"),
                [
                    (&*format!("{w}:w"), width.to_string().as_str()),
                    (&*format!("{w}:type"), "dxa"),
                ],
            );
        }
    }

    pub fn emit_cell(&mut self, id: NodeId) {
        let Some(Node::Cell(c)) = self.doc.node(id) else {
            return;
        };
        if let Some(el) = self.pres_table(id) {
            self.out.start(
                &el.name,
                el.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
            );
            let blocks = c.blocks.to_vec();
            self.emit_container(&el.children, &blocks, Some(id));
            // A cell must end with a paragraph.
            if !blocks
                .last()
                .is_some_and(|b| matches!(self.doc.node(*b), Some(Node::Paragraph(_))))
            {
                let w = self.p.w.clone();
                self.out.empty(&format!("{w}:p"), std::iter::empty());
            }
            self.out.end();
        } else {
            self.gen_cell(id, None);
        }
    }

    fn gen_cell(&mut self, id: NodeId, width: Option<u32>) {
        let Some(Node::Cell(c)) = self.doc.node(id) else {
            return;
        };
        let w = self.p.w.clone();
        let skel = self.pres_table(id);
        match skel {
            Some(el) => self.out.start(
                &el.name,
                el.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
            ),
            None => self.out.start(&format!("{w}:tc"), std::iter::empty()),
        }
        match self.opaque_of(id, "tcPr") {
            Some(b) => {
                let patched = patch_tc_pr(&b, c);
                self.out.raw(&patched);
            }
            None => {
                self.out.start(&format!("{w}:tcPr"), std::iter::empty());
                self.tc_w(width.unwrap_or(0));
                if c.col_span > 1 {
                    self.out.empty(
                        &format!("{w}:gridSpan"),
                        [(&*format!("{w}:val"), c.col_span.to_string().as_str())],
                    );
                }
                if c.row_span > 1 {
                    self.out.empty(
                        &format!("{w}:vMerge"),
                        [(&*format!("{w}:val"), "restart")],
                    );
                }
                self.out.end();
            }
        }
        let blocks = c.blocks.to_vec();
        // The content keeps the wrappers and unknown children of the source
        // cell; its `w:tcPr` was written above (no owner: slots are skipped).
        let children: &[super::super::preserve::PreservedChild] = skel.map_or(&[], |e| &e.children);
        self.emit_container(children, &blocks, None);
        // A cell must end with a paragraph.
        let last_is_paragraph = blocks
            .last()
            .is_some_and(|b| matches!(self.doc.node(*b), Some(Node::Paragraph(_))));
        if !last_is_paragraph {
            self.out.empty(&format!("{w}:p"), std::iter::empty());
        }
        self.out.end();
    }

}
