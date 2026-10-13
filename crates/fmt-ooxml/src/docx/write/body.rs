// SPDX-License-Identifier: AGPL-3.0-or-later
//! The body of the DOCX writer: blocks and paragraphs.
//!
//! A paragraph that exists in the source skeleton and whose model equals the
//! model of the import is written back from the skeleton, so its XML is the
//! source's. A modified paragraph is written from the skeleton too, with its
//! text ranges mapped through the edit and the properties that changed rebuilt
//! from the model; when that cannot account for the whole model text, or for a
//! paragraph that has no skeleton, it is written from the model alone.

use super::super::preserve::{Preserved, PreservedChild, PreservedElement};
use super::super::{ppr_rank, rpr_rank};
use super::props::{
    GenEl, Item, Prefixes, ThemeRefs, insert_ranked, local_of, local_of_bytes, merge_ppr,
    merge_rpr, new_ppr, new_rpr,
};
use super::xml::{Out, xml_char};
use super::WriteNotice;
use atelier_model::NodeId as ModelNodeId;
use atelier_text_model::rich::{
    AnchoredObject, CharAttrs, Color, LinkTarget, Node, ParaProps, Paragraph, RichDocument,
};
use std::collections::{BTreeMap, BTreeSet};

type NodeId = ModelNodeId;

const MAX_DEPTH: usize = 128;
const MAX_NOTICES: usize = 512;

/// A saved position of the output, to drop what an attempt wrote.
pub(super) struct Mark {
    len: usize,
    pending: bool,
    stack: usize,
}

pub(super) struct Gen<'a> {
    pub doc: &'a RichDocument,
    /// The model of the import, to tell what the caller changed.
    pub base: Option<&'a RichDocument>,
    pub pres: Option<&'a Preserved>,
    pub p: Prefixes,
    pub default_style: Option<String>,
    pub out: Out,
    pub notices: Vec<WriteNotice>,
    pub dropped_notices: usize,
    /// External hyperlink targets with the relationship id given to them.
    pub links: Vec<(String, String)>,
    pub link_prefix: String,
    pub bookmark_ids: BTreeMap<NodeId, u32>,
    pub fonts: BTreeSet<String>,
    pub lists: BTreeSet<u32>,
    pub max_output: usize,
    pub failed: Option<&'static str>,
    pub depth: usize,
    /// Column widths (twips) given to the tables created by the writer.
    pub text_width: u32,
    /// Section properties of the sections before the last one.
    pub sect_after: BTreeMap<NodeId, Vec<u8>>,
    /// Tables being written, innermost last, with their lazily built layout.
    pub table_stack: Vec<(NodeId, Option<Vec<Vec<super::tbl::Slot>>>)>,
    pub tables_info: Option<&'a super::super::ImportedTables>,
}

/// Edit that turned the text of the import into the current text.
struct Mapper {
    identity: bool,
    p: usize,
    eb: usize,
    ec: usize,
}

impl Mapper {
    fn new(base: &str, cur: &str) -> Self {
        if base == cur {
            return Self {
                identity: true,
                p: 0,
                eb: 0,
                ec: 0,
            };
        }
        let (bb, cb) = (base.as_bytes(), cur.as_bytes());
        let min = bb.len().min(cb.len());
        let mut p = 0;
        while p < min && bb[p] == cb[p] {
            p += 1;
        }
        while p > 0 && !(base.is_char_boundary(p) && cur.is_char_boundary(p)) {
            p -= 1;
        }
        let mut s = 0;
        while s < min - p && bb[bb.len() - 1 - s] == cb[cb.len() - 1 - s] {
            s += 1;
        }
        while s > 0 && !(base.is_char_boundary(bb.len() - s) && cur.is_char_boundary(cb.len() - s))
        {
            s -= 1;
        }
        Self {
            identity: false,
            p,
            eb: bb.len() - s,
            ec: cb.len() - s,
        }
    }

    /// Start of a source range in the current text.
    fn start(&self, a: usize) -> usize {
        if self.identity || a < self.p {
            a
        } else if a == self.p {
            if a == 0 { 0 } else { self.ec }
        } else if a < self.eb {
            self.ec
        } else {
            a - self.eb + self.ec
        }
    }

    /// Offset of the source text a current offset comes from; `None` inside
    /// inserted text.
    fn back(&self, s: usize) -> Option<usize> {
        if self.identity || s <= self.p {
            Some(s)
        } else if s >= self.ec {
            Some(s - self.ec + self.eb)
        } else {
            None
        }
    }

    /// End of a source range in the current text.
    fn end(&self, b: usize) -> usize {
        if self.identity || b < self.p {
            b
        } else if b < self.eb {
            self.ec
        } else {
            b - self.eb + self.ec
        }
    }
}

/// State of the paragraph being written from its skeleton.
struct ParaCx<'p> {
    id: NodeId,
    cur: &'p Paragraph,
    base: &'p Paragraph,
    map: Mapper,
    emitted: String,
    seen: BTreeSet<NodeId>,
    ok: bool,
}

const RUN_LEVEL: &[&str] = &[
    "drawing",
    "pict",
    "object",
    "fldChar",
    "instrText",
    "delInstrText",
    "commentReference",
    "footnoteReference",
    "endnoteReference",
    "annotationRef",
    "footnoteRef",
    "endnoteRef",
    "separator",
    "continuationSeparator",
    "ptab",
    "lastRenderedPageBreak",
    "contentPart",
    "dayShort",
    "monthShort",
    "yearShort",
    "dayLong",
    "monthLong",
    "yearLong",
    "pgNum",
];

fn needs_preserve(text: &str) -> bool {
    text.starts_with(char::is_whitespace)
        || text.ends_with(char::is_whitespace)
        || text.contains("  ")
}

impl<'a> Gen<'a> {
    pub fn note(&mut self, notice: WriteNotice) {
        if self.notices.len() < MAX_NOTICES {
            self.notices.push(notice);
        } else {
            self.dropped_notices += 1;
        }
    }

    pub fn check_limit(&mut self) {
        self.check_size();
    }

    fn check_size(&mut self) {
        if self.failed.is_none() && self.out.len() > self.max_output {
            self.failed = Some("output size");
        }
    }

    pub fn mark(&self) -> Mark {
        let (len, pending, stack) = self.out.snapshot();
        Mark {
            len,
            pending,
            stack,
        }
    }

    pub fn rollback(&mut self, mark: &Mark) {
        self.out.restore(mark.len, mark.pending, mark.stack);
    }

    fn pres_paragraph(&self, id: NodeId) -> Option<&'a PreservedElement> {
        let pres: &'a Preserved = self.pres?;
        pres.paragraphs.get(&id)
    }

    pub fn pres_table(&self, id: NodeId) -> Option<&'a PreservedElement> {
        let pres: &'a Preserved = self.pres?;
        pres.tables.get(&id)
    }

    /// True when the node is absent from the import or differs from it.
    pub fn changed(&self, id: NodeId) -> bool {
        match self.base {
            Some(base) => base.node(id) != self.doc.node(id),
            None => true,
        }
    }

    // ----- skeleton elements ------------------------------------------------

    /// A skeleton element written as it is, without any model content.
    pub fn write_plain(&mut self, e: &PreservedElement) {
        if self.depth > MAX_DEPTH {
            self.failed = Some("nesting depth");
            return;
        }
        self.depth += 1;
        self.out.start(
            &e.name,
            e.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
        );
        for c in &e.children {
            match c {
                PreservedChild::Element(x) => self.write_plain(x),
                PreservedChild::Opaque(b) => self.out.raw(b),
                PreservedChild::Block(_) | PreservedChild::Anchored(_) => {}
            }
        }
        self.out.end();
        self.depth -= 1;
    }

    pub fn write_gen(&mut self, g: &GenEl) {
        if local_of(&g.name) == "numPr" {
            let get = |k: &str| {
                g.attrs
                    .iter()
                    .find(|(n, _)| n == k)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            let (level, id) = (get("ilvl"), get("numId"));
            if let Ok(n) = id.parse::<u32>() {
                self.lists.insert(n);
            }
            let w = self.p.w.clone();
            self.out.start(&g.name, std::iter::empty());
            self.out
                .empty(&format!("{w}:ilvl"), [(&*format!("{w}:val"), level.as_str())]);
            self.out
                .empty(&format!("{w}:numId"), [(&*format!("{w}:val"), id.as_str())]);
            self.out.end();
            return;
        }
        self.out.start(
            &g.name,
            g.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        );
        self.out.end();
    }

    fn write_items(&mut self, items: &[Item<'_>]) {
        for item in items {
            match item {
                Item::Skel(PreservedChild::Element(e)) => self.write_plain(e),
                Item::Skel(PreservedChild::Opaque(b)) => self.out.raw(b),
                Item::Skel(_) => {}
                Item::Gen(g) => self.write_gen(g),
                Item::Raw(b) => self.out.raw(b),
            }
        }
    }

    fn approximated(&mut self, colors: Vec<Color>, paragraph: NodeId) {
        for c in colors {
            self.note(WriteNotice::HighlightApproximated {
                paragraph,
                color: (c.r, c.g, c.b),
            });
        }
    }

    // ----- block containers -------------------------------------------------

    /// Writes the children of a container: the skeleton `children` in their
    /// order, with the model blocks `model` (the current list of the container)
    /// deciding what exists and where new ones go.
    pub fn emit_container(
        &mut self,
        children: &[PreservedChild],
        model: &[NodeId],
        owner: Option<NodeId>,
    ) {
        let in_model: BTreeSet<NodeId> = model.iter().copied().collect();
        let mut skel_ids = Vec::new();
        collect_blocks(children, &mut skel_ids);
        let in_skel: BTreeSet<NodeId> = skel_ids.iter().copied().collect();
        let skel_order: Vec<NodeId> = skel_ids
            .iter()
            .copied()
            .filter(|i| in_model.contains(i))
            .collect();
        let model_order: Vec<NodeId> = model
            .iter()
            .copied()
            .filter(|i| in_skel.contains(i))
            .collect();
        if skel_order != model_order {
            // Blocks were moved: the model order wins, the wrappers are dropped.
            self.note(WriteNotice::BlocksReordered);
            for id in model {
                self.emit_item(*id);
            }
            for c in children {
                if let PreservedChild::Opaque(b) = c {
                    if matches!(local_of_bytes(b), "sectPr") {
                        self.out.raw(b);
                    }
                }
            }
            return;
        }
        // New blocks go after the nearest preceding block that has a skeleton.
        let mut front: Vec<NodeId> = Vec::new();
        let mut after: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        let mut anchor: Option<NodeId> = None;
        for id in model {
            if in_skel.contains(id) {
                anchor = Some(*id);
            } else if let Some(a) = anchor {
                after.entry(a).or_default().push(*id);
            } else {
                front.push(*id);
            }
        }
        let mut state = Walk {
            in_model,
            after,
            front,
            front_done: false,
            emitted: BTreeSet::new(),
            owner,
        };
        self.walk(children, &mut state);
        if !state.front_done {
            let front = std::mem::take(&mut state.front);
            for id in front {
                self.emit_item(id);
                state.emitted.insert(id);
            }
        }
        for id in model {
            if !state.emitted.contains(id) {
                self.emit_item(*id);
                state.emitted.insert(*id);
            }
        }
    }

    fn walk(&mut self, children: &[PreservedChild], st: &mut Walk) {
        if self.depth > MAX_DEPTH {
            self.failed = Some("nesting depth");
            return;
        }
        self.depth += 1;
        let mut skip_shadow_tag = false;
        for (i, c) in children.iter().enumerate() {
            if self.failed.is_some() {
                break;
            }
            match c {
                PreservedChild::Block(id) => {
                    if self.doc.node(*id).is_none() {
                        continue;
                    }
                    if st.in_model.contains(id) {
                        if st.emitted.contains(id) {
                            continue;
                        }
                        if !st.front_done {
                            st.front_done = true;
                            let front = std::mem::take(&mut st.front);
                            for f in front {
                                self.emit_item(f);
                                st.emitted.insert(f);
                            }
                        }
                        self.emit_item(*id);
                        st.emitted.insert(*id);
                        if let Some(list) = st.after.remove(id) {
                            for n in list {
                                self.emit_item(n);
                                st.emitted.insert(n);
                            }
                        }
                    } else {
                        self.emit_item(*id);
                    }
                }
                PreservedChild::Opaque(b) => {
                    let next_shadow = match children.get(i + 1) {
                        Some(PreservedChild::Element(e)) if e.shadow => Some(e),
                        _ => None,
                    };
                    if local_of_bytes(b) == "AlternateContent"
                        && next_shadow.is_some_and(|e| self.shadow_changed_blocks(e))
                    {
                        self.note(WriteNotice::AlternateContentFlattened);
                        skip_shadow_tag = true;
                    } else {
                        self.out.raw(b);
                    }
                }
                PreservedChild::Element(e) if e.slot => {
                    if let Some(owner) = st.owner {
                        self.emit_slot(owner, e);
                    }
                }
                PreservedChild::Element(e) => {
                    if e.shadow {
                        if skip_shadow_tag {
                            skip_shadow_tag = false;
                            self.walk(&e.children, st);
                        } else {
                            self.out.mute();
                            self.walk(&e.children, st);
                            self.out.unmute();
                        }
                    } else {
                        self.out.start(
                            &e.name,
                            e.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
                        );
                        self.walk(&e.children, st);
                        self.out.end();
                    }
                }
                PreservedChild::Anchored(_) => {}
            }
        }
        self.depth -= 1;
    }

    /// True when a block inside a shadow element differs from the import.
    fn shadow_changed_blocks(&self, e: &PreservedElement) -> bool {
        let mut ids = Vec::new();
        collect_blocks(&e.children, &mut ids);
        ids.iter().any(|id| self.changed(*id))
    }

    fn emit_item(&mut self, id: NodeId) {
        self.check_size();
        if self.failed.is_some() {
            return;
        }
        match self.doc.node(id) {
            Some(Node::Paragraph(_) | Node::Table(_) | Node::Opaque(_)) => self.emit_block(id),
            Some(Node::Row(_)) => self.emit_row(id),
            Some(Node::Cell(_)) => self.emit_cell(id),
            _ => {}
        }
    }

    /// A block of a section, header, footer or cell.
    pub fn emit_block(&mut self, id: NodeId) {
        match self.doc.node(id) {
            Some(Node::Paragraph(_)) => {
                let sect = self.sect_after.remove(&id);
                self.emit_paragraph(id, sect.as_deref());
            }
            Some(Node::Table(_)) => {
                self.emit_table(id);
                if let Some(sect) = self.sect_after.remove(&id) {
                    self.emit_sect_paragraph(&sect);
                }
            }
            Some(Node::Opaque(o)) => {
                let data = o.data.clone();
                self.out.raw(&data);
                if let Some(sect) = self.sect_after.remove(&id) {
                    self.emit_sect_paragraph(&sect);
                }
            }
            _ => {}
        }
    }

    pub fn emit_sect_paragraph(&mut self, sect: &[u8]) {
        let w = self.p.w.clone();
        self.out.start(&format!("{w}:p"), std::iter::empty());
        self.out.start(&format!("{w}:pPr"), std::iter::empty());
        self.out.raw(sect);
        self.out.end();
        self.out.end();
    }

    // ----- paragraphs -------------------------------------------------------

    pub fn emit_paragraph(&mut self, id: NodeId, sect: Option<&[u8]>) {
        let Ok(cur) = self.doc.paragraph(id) else {
            return;
        };
        let skel = self.pres_paragraph(id);
        let base = self.base.and_then(|b| b.paragraph(id).ok());
        if let (Some(skel), Some(base)) = (skel, base) {
            if cur.fields.is_empty() && cur.bookmarks.is_empty() && cur.links.is_empty() {
                let mark = self.mark();
                let notes = self.notices.len();
                if self.paragraph_from_skeleton(id, skel, cur, base, sect) {
                    return;
                }
                self.rollback(&mark);
                self.notices.truncate(notes);
            }
            self.note(WriteNotice::ParagraphRebuilt { paragraph: id });
            self.paragraph_flat(id, cur, Some(skel), Some(&base.props), sect);
            return;
        }
        if skel.is_some() {
            self.note(WriteNotice::ParagraphRebuilt { paragraph: id });
        }
        self.paragraph_flat(id, cur, skel, None, sect);
    }

    /// The `w:pPr` skeleton child of a `w:p` skeleton.
    fn ppr_of(skel: &PreservedElement) -> Option<&PreservedElement> {
        skel.children.iter().find_map(|c| match c {
            PreservedChild::Element(e) if local_of(&e.name) == "pPr" => Some(e),
            _ => None,
        })
    }

    fn write_ppr(
        &mut self,
        skel: Option<&PreservedElement>,
        base: Option<&ParaProps>,
        cur: &ParaProps,
        sect: Option<&[u8]>,
    ) {
        let default = self.default_style.clone();
        match skel {
            Some(e) => {
                let items = merge_ppr(&e.children, base, cur, &self.p, default.as_deref(), sect);
                self.out.start(
                    &e.name,
                    e.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
                );
                self.write_items(&items);
                self.out.end();
            }
            None => {
                let gens = new_ppr(cur, &self.p, default.as_deref());
                if gens.is_empty() && sect.is_none() {
                    return;
                }
                let w = self.p.w.clone();
                self.out.start(&format!("{w}:pPr"), std::iter::empty());
                let mut items: Vec<Item<'_>> = gens.into_iter().map(Item::Gen).collect();
                if let Some(s) = sect {
                    insert_ranked(&mut items, Item::Raw(s.to_vec()), ppr_rank);
                }
                self.write_items(&items);
                self.out.end();
            }
        }
    }

    /// Writes a paragraph from its skeleton; false when the result does not
    /// carry exactly the model's text and objects.
    fn paragraph_from_skeleton(
        &mut self,
        id: NodeId,
        skel: &PreservedElement,
        cur: &Paragraph,
        base: &Paragraph,
        sect: Option<&[u8]>,
    ) -> bool {
        let mut cx = ParaCx {
            id,
            cur,
            base,
            map: Mapper::new(&base.text, &cur.text),
            emitted: String::with_capacity(cur.text.len()),
            seen: BTreeSet::new(),
            ok: true,
        };
        self.out.start(
            &skel.name,
            skel.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
        );
        let ppr = Self::ppr_of(skel);
        if ppr.is_none() {
            self.write_ppr(None, Some(&base.props), &cur.props, sect);
        }
        let mut first_ppr = true;
        let mut flatten = false;
        for (i, c) in skel.children.iter().enumerate() {
            match c {
                PreservedChild::Element(e) if local_of(&e.name) == "pPr" && first_ppr => {
                    first_ppr = false;
                    self.write_ppr(Some(e), Some(&base.props), &cur.props, sect);
                }
                _ => {
                    flatten = self.inline_child(&skel.children, i, flatten, &mut cx);
                }
            }
        }
        self.out.end();
        cx.ok && cx.emitted == cur.text && cur.anchored.iter().all(|a| cx.seen.contains(&a.id))
    }

    /// Writes `kids[i]`; the result tells whether the next sibling is an edited
    /// branch that replaces an `mc:AlternateContent` skipped here.
    fn inline_child(
        &mut self,
        kids: &[PreservedChild],
        i: usize,
        flatten: bool,
        cx: &mut ParaCx<'_>,
    ) -> bool {
        if self.depth > MAX_DEPTH {
            self.failed = Some("nesting depth");
            cx.ok = false;
            return false;
        }
        match &kids[i] {
            PreservedChild::Element(e) if local_of(&e.name) == "r" && e.range.is_some() => {
                self.emit_run(e, cx);
            }
            PreservedChild::Element(e) if e.shadow => {
                self.depth += 1;
                if flatten {
                    self.children_inline(&e.children, cx);
                } else {
                    self.out.mute();
                    self.children_inline(&e.children, cx);
                    self.out.unmute();
                }
                self.depth -= 1;
            }
            PreservedChild::Element(e) => {
                self.depth += 1;
                let before = self.mark();
                let text_before = cx.emitted.len();
                let seen_before = cx.seen.len();
                self.out.start(
                    &e.name,
                    e.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
                );
                self.children_inline(&e.children, cx);
                self.out.end();
                // An envelope whose text was all deleted goes with it.
                if wrapper_text_emptied(e, cx)
                    && cx.emitted.len() == text_before
                    && cx.seen.len() == seen_before
                {
                    self.rollback(&before);
                    self.note(WriteNotice::EnvelopeDropped {
                        name: e.name.clone(),
                    });
                }
                self.depth -= 1;
            }
            PreservedChild::Opaque(b) => {
                if local_of_bytes(b) == "AlternateContent" {
                    if let Some(PreservedChild::Element(next)) = kids.get(i + 1) {
                        if next.shadow && self.shadow_changed_text(next, cx) {
                            self.note(WriteNotice::AlternateContentFlattened);
                            return true;
                        }
                    }
                }
                self.out.raw(b);
            }
            PreservedChild::Anchored(id) => {
                if let Some(a) = cx.cur.anchored.iter().find(|a| a.id == *id) {
                    if cx.seen.insert(*id) {
                        self.out.raw(&a.data);
                    }
                }
            }
            PreservedChild::Block(_) => cx.ok = false,
        }
        false
    }

    fn children_inline(&mut self, kids: &[PreservedChild], cx: &mut ParaCx<'_>) {
        let mut flatten = false;
        for i in 0..kids.len() {
            flatten = self.inline_child(kids, i, flatten, cx);
        }
    }

    /// Whether the text of a shadow branch differs from the import.
    fn shadow_changed_text(&self, e: &PreservedElement, cx: &ParaCx<'_>) -> bool {
        let mut lo = usize::MAX;
        let mut hi = 0;
        range_extent(e, &mut lo, &mut hi);
        if lo == usize::MAX {
            return false;
        }
        let (a, b) = (cx.map.start(lo), cx.map.end(hi));
        cx.base.text.get(lo..hi) != cx.cur.text.get(a..b.max(a))
    }

    fn emit_run(&mut self, el: &PreservedElement, cx: &mut ParaCx<'_>) {
        let (a, b) = el.range.unwrap_or((0, 0));
        let a2 = cx.map.start(a);
        let b2 = cx.map.end(b).max(a2);
        if b2 > cx.cur.text.len() {
            cx.ok = false;
            return;
        }
        enum Part<'e> {
            Rpr(&'e PreservedElement),
            /// A leaf with a text range: mapped range, empty in the source.
            Leaf(&'e PreservedElement, usize, usize, bool),
            Anch(NodeId),
            Raw(&'e [u8]),
            Other(&'e PreservedElement),
        }
        let mut parts: Vec<(Part<'_>, usize)> = Vec::new();
        let mut cursor = a2;
        let mut seen_rpr = false;
        for c in &el.children {
            match c {
                PreservedChild::Element(e) if local_of(&e.name) == "rPr" && !seen_rpr => {
                    seen_rpr = true;
                    parts.push((Part::Rpr(e), a2));
                }
                PreservedChild::Element(e) if e.range.is_some() => {
                    let (x, y) = e.range.unwrap_or((0, 0));
                    let x2 = cx.map.start(x).max(a2).min(b2);
                    let y2 = cx.map.end(y).max(x2).min(b2);
                    parts.push((Part::Leaf(e, x2, y2, x == y), x2));
                    cursor = y2;
                }
                PreservedChild::Element(e) => parts.push((Part::Other(e), cursor)),
                PreservedChild::Opaque(bytes) => parts.push((Part::Raw(bytes), cursor)),
                PreservedChild::Anchored(id) => {
                    if cx.cur.anchored.iter().any(|o| o.id == *id) {
                        parts.push((Part::Anch(*id), cursor));
                    }
                }
                PreservedChild::Block(_) => cx.ok = false,
            }
        }
        let has_other = parts
            .iter()
            .any(|(p, _)| matches!(p, Part::Other(_) | Part::Raw(_) | Part::Anch(_)));
        let has_empty_leaf = parts
            .iter()
            .any(|(p, _)| matches!(p, Part::Leaf(_, _, _, true)));
        if b > a && a2 == b2 && !has_other && !has_empty_leaf {
            return;
        }
        // Pieces: the run is cut where the model's attributes change.
        let mut cuts = vec![a2, b2];
        for r in &cx.cur.runs {
            for o in [r.start, r.end] {
                if o > a2 && o < b2 {
                    cuts.push(o);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        let candidates: Vec<(usize, usize)> = if cuts.len() < 2 {
            vec![(a2, b2)]
        } else {
            cuts.windows(2).map(|w| (w[0], w[1])).collect()
        };
        // What the source had where a piece sits; a piece is rebuilt when the
        // model's attributes differ from it. Adjacent pieces that agree merge.
        struct Piece {
            s: usize,
            e: usize,
            regen: bool,
            attrs: CharAttrs,
            from: CharAttrs,
        }
        let run_attrs = cx.base.attrs_at(a);
        let mut pieces: Vec<Piece> = Vec::new();
        for (s, e) in candidates {
            let attrs = cx.cur.attrs_at(s);
            let from = cx
                .map
                .back(s)
                .map_or_else(|| run_attrs.clone(), |o| cx.base.attrs_at(o));
            let regen = b > a && s < e && attrs != from;
            if let Some(last) = pieces.last_mut() {
                if (!last.regen && !regen) || (last.regen && regen && last.attrs == attrs) {
                    last.e = e;
                    continue;
                }
            }
            pieces.push(Piece {
                s,
                e,
                regen,
                attrs,
                from,
            });
        }
        let last_piece = pieces.len() - 1;
        let has_rpr = seen_rpr;
        for (index, piece) in pieces.iter().enumerate() {
            let (s, e) = (piece.s, piece.e);
            let piece_attrs = piece.attrs.clone();
            let base_attrs = piece.from.clone();
            let regen = piece.regen;
            let inside = |pos: usize| (pos >= s && pos < e) || (index == last_piece && pos == e);
            self.out.start(
                &el.name,
                el.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
            );
            if regen && !has_rpr {
                self.write_rpr_new(&piece_attrs, cx.id);
            }
            for (part, pos) in &parts {
                match part {
                    Part::Rpr(rp) => {
                        if regen {
                            let mut approx = Vec::new();
                            let items = merge_rpr(
                                &rp.children,
                                Some(&base_attrs),
                                &piece_attrs,
                                ThemeRefs::default(),
                                &self.p,
                                rpr_rank,
                                &mut approx,
                            );
                            self.approximated(approx, cx.id);
                            if let Some(f) = &piece_attrs.font {
                                self.fonts.insert(f.clone());
                            }
                            self.out.start(
                                &rp.name,
                                rp.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
                            );
                            self.write_items(&items);
                            self.out.end();
                        } else {
                            self.write_plain(rp);
                        }
                    }
                    Part::Leaf(l, x, y, orig_empty) => {
                        if x == y {
                            if *orig_empty && inside(*x) {
                                self.write_plain(l);
                            }
                            continue;
                        }
                        let (cs, ce) = ((*x).max(s), (*y).min(e));
                        if cs >= ce {
                            continue;
                        }
                        let text = cx.cur.text.get(cs..ce).unwrap_or("");
                        if local_of(&l.name) == "t" {
                            let mut attrs: Vec<(&str, &str)> = l
                                .attrs
                                .iter()
                                .map(|a| (a.name.as_str(), a.value.as_str()))
                                .collect();
                            if !cx.map.identity
                                && needs_preserve(text)
                                && !l.attrs.iter().any(|a| a.name == "xml:space")
                            {
                                attrs.push(("xml:space", "preserve"));
                            }
                            self.out.start(&l.name, attrs);
                            self.out.text(text);
                            for k in &l.children {
                                if let PreservedChild::Opaque(bytes) = k {
                                    self.out.raw(bytes);
                                }
                            }
                            self.out.end();
                        } else {
                            self.write_plain(l);
                        }
                        cx.emitted.push_str(text);
                    }
                    Part::Anch(id) => {
                        if inside(*pos) && cx.seen.insert(*id) {
                            if let Some(o) = cx.cur.anchored.iter().find(|o| o.id == *id) {
                                self.out.raw(&o.data);
                            }
                        }
                    }
                    Part::Raw(bytes) => {
                        if inside(*pos) {
                            self.out.raw(bytes);
                        }
                    }
                    Part::Other(x) => {
                        if inside(*pos) {
                            self.write_plain(x);
                        }
                    }
                }
            }
            self.out.end();
        }
    }

    // ----- paragraphs written from the model ---------------------------------

    fn paragraph_flat(
        &mut self,
        id: NodeId,
        cur: &Paragraph,
        skel: Option<&PreservedElement>,
        base_props: Option<&ParaProps>,
        sect: Option<&[u8]>,
    ) {
        let w = self.p.w.clone();
        match skel {
            Some(s) => self.out.start(
                &s.name,
                s.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
            ),
            None => self.out.start(&format!("{w}:p"), std::iter::empty()),
        }
        let ppr = skel.and_then(Self::ppr_of);
        let props_base = if skel.is_some() { base_props } else { None };
        self.write_ppr(ppr, props_base, &cur.props, sect);
        if let Some(s) = skel {
            // Second and later `w:pPr` copies and other opaque children stay.
            let mut first = true;
            for c in &s.children {
                match c {
                    PreservedChild::Element(e) if local_of(&e.name) == "pPr" && first => {
                        first = false;
                    }
                    PreservedChild::Opaque(b) if local_of_bytes(b) == "pPr" => self.out.raw(b),
                    _ => {}
                }
            }
        }
        self.paragraph_content(id, cur);
        self.out.end();
    }

    fn paragraph_content(&mut self, id: NodeId, cur: &Paragraph) {
        let len = cur.text.len();
        let mut cuts: BTreeSet<usize> = BTreeSet::new();
        cuts.insert(0);
        cuts.insert(len);
        for r in &cur.runs {
            cuts.insert(r.start.min(len));
            cuts.insert(r.end.min(len));
        }
        for a in &cur.anchored {
            cuts.insert(a.offset.min(len));
        }
        for f in &cur.fields {
            cuts.insert(f.start.min(len));
            cuts.insert(f.end.min(len));
        }
        for b in &cur.bookmarks {
            cuts.insert(b.start.min(len));
            cuts.insert(b.end.min(len));
        }
        for l in &cur.links {
            cuts.insert(l.start.min(len));
            cuts.insert(l.end.min(len));
        }
        let cuts: Vec<usize> = cuts.into_iter().collect();
        let w = self.p.w.clone();
        let mut open_link: Option<usize> = None;
        for (i, &o) in cuts.iter().enumerate() {
            // Closings first (inner to outer), then openings (outer to inner).
            for _ in cur.fields.iter().filter(|f| f.end.min(len) == o) {
                self.field_char("end");
            }
            for bm in cur.bookmarks.iter().filter(|b| b.end.min(len) == o) {
                let bid = self.bookmark_id(bm.id);
                self.out.empty(
                    &format!("{w}:bookmarkEnd"),
                    [(&*format!("{w}:id"), bid.to_string().as_str())],
                );
            }
            if open_link.is_some() && cur.links.iter().any(|l| l.end.min(len) == o) {
                self.out.end();
                open_link = None;
            }
            for (li, l) in cur.links.iter().enumerate() {
                if l.start.min(len) == o && l.end.min(len) > o && open_link.is_none() {
                    self.open_link(l);
                    open_link = Some(li);
                }
            }
            for bm in cur.bookmarks.iter().filter(|b| b.start.min(len) == o) {
                let bid = self.bookmark_id(bm.id);
                self.out.empty(
                    &format!("{w}:bookmarkStart"),
                    [
                        (&*format!("{w}:id"), bid.to_string().as_str()),
                        (&*format!("{w}:name"), bm.name.as_str()),
                    ],
                );
            }
            for f in cur.fields.iter().filter(|f| f.start.min(len) == o) {
                self.field_begin(&f.instruction);
            }
            for a in cur.anchored.iter().filter(|a| a.offset.min(len) == o) {
                self.anchored_flat(id, a, &cur.attrs_at(o));
            }
            if let Some(&next) = cuts.get(i + 1) {
                if next > o {
                    if let Some(text) = cur.text.get(o..next) {
                        let attrs = cur.attrs_at(o);
                        self.text_run(id, text, &attrs);
                    }
                }
            }
        }
        if open_link.is_some() {
            self.out.end();
        }
    }

    /// The `w:id` of a bookmark: numbered in order of appearance from 100000,
    /// above the identifiers a source document is likely to use.
    fn bookmark_id(&mut self, bm: NodeId) -> u32 {
        let next = 100_000 + u32::try_from(self.bookmark_ids.len()).unwrap_or(0);
        *self.bookmark_ids.entry(bm).or_insert(next)
    }

    fn open_link(&mut self, l: &atelier_text_model::rich::Link) {
        let w = self.p.w.clone();
        let r = self.p.r.clone();
        let mut attrs: Vec<(String, String)> = Vec::new();
        match &l.target {
            LinkTarget::External(url) => {
                let rid = match self.links.iter().find(|(_, t)| t == url) {
                    Some((rid, _)) => rid.clone(),
                    None => {
                        let rid = format!("{}{}", self.link_prefix, self.links.len() + 1);
                        self.links.push((rid.clone(), url.clone()));
                        rid
                    }
                };
                attrs.push((format!("{r}:id"), rid));
            }
            LinkTarget::Bookmark(name) => attrs.push((format!("{w}:anchor"), name.clone())),
        }
        if let Some(t) = &l.tooltip {
            attrs.push((format!("{w}:tooltip"), t.clone()));
        }
        attrs.push((format!("{w}:history"), "1".into()));
        self.out.start(
            &format!("{w}:hyperlink"),
            attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        );
    }

    fn field_char(&mut self, kind: &str) {
        let w = self.p.w.clone();
        self.out.start(&format!("{w}:r"), std::iter::empty());
        self.out.empty(
            &format!("{w}:fldChar"),
            [(&*format!("{w}:fldCharType"), kind)],
        );
        self.out.end();
    }

    fn field_begin(&mut self, instruction: &str) {
        let w = self.p.w.clone();
        self.field_char("begin");
        self.out.start(&format!("{w}:r"), std::iter::empty());
        self.out
            .start(&format!("{w}:instrText"), [("xml:space", "preserve")]);
        let clean: String = instruction.chars().filter(|c| xml_char(*c)).collect();
        self.out.text(&format!(" {} ", clean.trim()));
        self.out.end();
        self.out.end();
        self.field_char("separate");
    }

    fn anchored_flat(&mut self, para: NodeId, a: &AnchoredObject, attrs: &CharAttrs) {
        let local = a.kind.rsplit(':').next().unwrap_or("");
        let in_w = a.kind.starts_with(&format!("{}:", self.p.w)) || !a.kind.contains(':');
        let mut run_level = in_w && RUN_LEVEL.contains(&local);
        if local == "AlternateContent" {
            let d = &a.data;
            let has = |needle: &[u8]| d.windows(needle.len()).any(|w| w == needle);
            run_level = has(b":drawing") || has(b":pict");
            if has(b"<w:r>") || has(b"<w:r ") {
                run_level = false;
            }
        }
        if run_level {
            let w = self.p.w.clone();
            self.out.start(&format!("{w}:r"), std::iter::empty());
            self.write_rpr_new(attrs, para);
            self.out.raw(&a.data);
            self.out.end();
        } else {
            self.out.raw(&a.data);
        }
    }

    fn write_rpr_new(&mut self, attrs: &CharAttrs, para: NodeId) {
        let mut approx = Vec::new();
        let gens = new_rpr(attrs, ThemeRefs::default(), &self.p, &mut approx);
        self.approximated(approx, para);
        if gens.is_empty() {
            return;
        }
        if let Some(f) = &attrs.font {
            self.fonts.insert(f.clone());
        }
        let w = self.p.w.clone();
        self.out.start(&format!("{w}:rPr"), std::iter::empty());
        for g in &gens {
            self.write_gen(g);
        }
        self.out.end();
    }

    fn text_run(&mut self, para: NodeId, text: &str, attrs: &CharAttrs) {
        let w = self.p.w.clone();
        self.out.start(&format!("{w}:r"), std::iter::empty());
        self.write_rpr_new(attrs, para);
        let mut buf = String::new();
        let flush = |this: &mut Self, buf: &mut String| {
            if buf.is_empty() {
                return;
            }
            let attrs: &[(&str, &str)] = if needs_preserve(buf) {
                &[("xml:space", "preserve")]
            } else {
                &[]
            };
            this.out
                .start(&format!("{}:t", this.p.w), attrs.iter().copied());
            this.out.text(buf);
            this.out.end();
            buf.clear();
        };
        for ch in text.chars() {
            let special = match ch {
                '\t' => Some("tab"),
                '\n' => Some("br"),
                '\u{2011}' => Some("noBreakHyphen"),
                '\u{00ad}' => Some("softHyphen"),
                _ => None,
            };
            if let Some(name) = special {
                flush(self, &mut buf);
                self.out.empty(&format!("{w}:{name}"), std::iter::empty());
            } else if xml_char(ch) && ch != '\r' {
                buf.push(ch);
            } else {
                self.note(WriteNotice::UnsupportedCharacter {
                    paragraph: para,
                    character: u32::from(ch),
                });
            }
        }
        flush(self, &mut buf);
        self.out.end();
    }
}

struct Walk {
    in_model: BTreeSet<NodeId>,
    after: BTreeMap<NodeId, Vec<NodeId>>,
    front: Vec<NodeId>,
    front_done: bool,
    emitted: BTreeSet<NodeId>,
    owner: Option<NodeId>,
}

pub(super) fn collect_blocks(children: &[PreservedChild], out: &mut Vec<NodeId>) {
    for c in children {
        match c {
            PreservedChild::Block(id) => out.push(*id),
            PreservedChild::Element(e) => collect_blocks(&e.children, out),
            _ => {}
        }
    }
}

fn range_extent(e: &PreservedElement, lo: &mut usize, hi: &mut usize) {
    if let Some((a, b)) = e.range {
        if e.children.is_empty() || local_of(&e.name) == "r" {
            *lo = (*lo).min(a);
            *hi = (*hi).max(b);
        }
    }
    for c in &e.children {
        if let PreservedChild::Element(x) = c {
            range_extent(x, lo, hi);
        }
    }
}

/// A wrapper that contained text in the import and whose text is now empty.
fn wrapper_text_emptied(e: &PreservedElement, cx: &ParaCx<'_>) -> bool {
    let (mut lo, mut hi) = (usize::MAX, 0);
    range_extent(e, &mut lo, &mut hi);
    if lo == usize::MAX || hi <= lo {
        return false;
    }
    cx.map.start(lo) >= cx.map.end(hi)
}
