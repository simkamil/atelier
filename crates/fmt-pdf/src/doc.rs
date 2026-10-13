// SPDX-License-Identifier: AGPL-3.0-or-later
//! Document assembly: page objects, page tree, fonts, outlines, named
//! destinations, information dictionary, cross-reference table and trailer.

use crate::font::{self, FontEntry, Mode, Prepared};
use crate::out::{Buf, Out, text_string, uri_string};
use crate::{
    Destination, OutlineItem, PdfDate, PdfError, PdfNotice, PdfOptions, PdfResourceProvider,
};
use atelier_render::DisplayList;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;

/// Children per node of the page tree.
const FANOUT: usize = 32;
const MAX_TEXT_CHARS: usize = 65_536;

pub(crate) struct Patch {
    /// Offset of the four hex digits of a glyph code in the content stream.
    pub offset: usize,
    pub font: usize,
    pub gid: u16,
}

pub(crate) enum LinkKind {
    Uri(String),
    Named(String),
}

pub(crate) struct Annot {
    /// `[llx, lly, urx, ury]` in PDF page space.
    pub rect: [f64; 4],
    pub kind: LinkKind,
}

pub(crate) struct PageData {
    pub index: usize,
    pub width: f64,
    pub height: f64,
    pub content: Vec<u8>,
    pub patches: Vec<Patch>,
    pub fonts: BTreeSet<usize>,
    pub images: BTreeSet<usize>,
    pub alphas: BTreeSet<u8>,
    pub annots: Vec<Annot>,
}

/// Streaming PDF writer. Add pages one by one, then call [`PdfWriter::finish`].
///
/// A page is written as soon as it is added, except when it uses a CFF font:
/// the CIDs of such a font depend on every glyph of the document, so that
/// page's content waits (uncompressed, bounded by
/// [`PdfLimits::max_pending_bytes`](crate::PdfLimits)) until `finish`.
/// After an error the writer must be dropped.
pub struct PdfWriter<W: Write> {
    pub(crate) out: Out<W>,
    pub(crate) options: PdfOptions,
    pub(crate) fonts: Vec<FontEntry>,
    pub(crate) font_by_key: HashMap<(u64, usize), usize>,
    pub(crate) font_by_id: HashMap<u64, usize>,
    pub(crate) image_objs: Vec<u32>,
    pub(crate) image_by_id: HashMap<u64, usize>,
    pub(crate) notices: Vec<PdfNotice>,
    pub(crate) dests: BTreeMap<String, Destination>,
    gs_objs: BTreeMap<u8, u32>,
    page_ids: Vec<u32>,
    page_sizes: Vec<(f64, f64)>,
    leaves: Vec<u32>,
    pending: Vec<PageData>,
    pending_bytes: usize,
}

fn check_date(d: &PdfDate) -> Result<(), PdfError> {
    let ok = (1..=9999).contains(&d.year)
        && (1..=12).contains(&d.month)
        && (1..=31).contains(&d.day)
        && d.hour < 24
        && d.minute < 60
        && d.second < 61
        && d.utc_offset_minutes.abs() <= 14 * 60;
    if ok {
        Ok(())
    } else {
        Err(PdfError::InvalidOptions("date"))
    }
}

fn date_string(d: &PdfDate) -> String {
    let zone = if d.utc_offset_minutes == 0 {
        "Z".to_owned()
    } else {
        let a = d.utc_offset_minutes.unsigned_abs();
        format!(
            "{}{:02}'{:02}'",
            if d.utc_offset_minutes < 0 { '-' } else { '+' },
            a / 60,
            a % 60
        )
    };
    format!(
        "D:{:04}{:02}{:02}{:02}{:02}{:02}{zone}",
        d.year, d.month, d.day, d.hour, d.minute, d.second
    )
}

fn outline_stats(items: &[OutlineItem], depth: usize, max_depth: usize) -> Result<usize, PdfError> {
    if depth > max_depth {
        return Err(PdfError::LimitExceeded("outline depth"));
    }
    let mut n = 0usize;
    for i in items {
        n = n
            .checked_add(1 + outline_stats(&i.children, depth + 1, max_depth)?)
            .ok_or(PdfError::LimitExceeded("outline items"))?;
    }
    Ok(n)
}

impl<W: Write> PdfWriter<W> {
    /// Starts a document: validates the options and writes the header.
    pub fn new(sink: W, options: PdfOptions) -> Result<Self, PdfError> {
        let l = options.limits;
        let mut dests = BTreeMap::new();
        for (name, d) in &options.named_destinations {
            if name.is_empty() || !name.bytes().all(|b| (b' '..=b'~').contains(&b)) {
                return Err(PdfError::InvalidOptions(
                    "destination name must be printable ASCII",
                ));
            }
            if dests.insert(name.clone(), *d).is_some() {
                return Err(PdfError::InvalidOptions("duplicate destination name"));
            }
            if d.top.is_some_and(|t| !t.is_finite() || t.abs() > 1e9) {
                return Err(PdfError::InvalidOptions("destination position"));
            }
        }
        let info = &options.info;
        for s in [
            &info.title,
            &info.author,
            &info.subject,
            &info.keywords,
            &info.creator,
            &info.producer,
        ]
        .into_iter()
        .flatten()
        {
            if s.chars().count() > MAX_TEXT_CHARS {
                return Err(PdfError::LimitExceeded("metadata text"));
            }
        }
        for d in [&info.creation_date, &info.modification_date]
            .into_iter()
            .flatten()
        {
            check_date(d)?;
        }
        if let Some(lang) = &options.language {
            if lang.is_empty()
                || lang.len() > 35
                || !lang.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(PdfError::InvalidOptions("language tag"));
            }
        }
        let total = outline_stats(&options.outlines, 1, l.max_outline_depth)?;
        if total > l.max_outline_items {
            return Err(PdfError::LimitExceeded("outline items"));
        }
        let mut out = Out::new(sink, l.max_output_bytes, l.max_objects);
        out.write(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n")?;
        Ok(Self {
            out,
            options,
            fonts: Vec::new(),
            font_by_key: HashMap::new(),
            font_by_id: HashMap::new(),
            image_objs: Vec::new(),
            image_by_id: HashMap::new(),
            notices: Vec::new(),
            dests,
            gs_objs: BTreeMap::new(),
            page_ids: Vec::new(),
            page_sizes: Vec::new(),
            leaves: Vec::new(),
            pending: Vec::new(),
            pending_bytes: 0,
        })
    }

    /// Number of pages added so far.
    pub fn page_count(&self) -> usize {
        self.page_ids.len()
    }

    /// Adds the next page.
    pub fn add_page<P: PdfResourceProvider + ?Sized>(
        &mut self,
        list: &DisplayList,
        provider: &P,
    ) -> Result<(), PdfError> {
        let index = self.page_ids.len();
        if index >= self.options.limits.max_pages {
            return Err(PdfError::LimitExceeded("pages"));
        }
        let page = self.build_page(list, provider, index)?;
        if index % FANOUT == 0 {
            let leaf = self.out.alloc()?;
            self.leaves.push(leaf);
        }
        let id = self.out.alloc()?;
        self.page_ids.push(id);
        self.page_sizes.push((page.width, page.height));
        if page.patches.is_empty() {
            self.flush_page(page, &[])
        } else {
            self.pending_bytes += page.content.len();
            if self.pending_bytes > self.options.limits.max_pending_bytes {
                return Err(PdfError::LimitExceeded("pending page content"));
            }
            self.pending.push(page);
            Ok(())
        }
    }

    fn gs_object(&mut self, alpha: u8) -> Result<u32, PdfError> {
        if let Some(&id) = self.gs_objs.get(&alpha) {
            return Ok(id);
        }
        let id = self.out.alloc()?;
        let mut b = Buf::default();
        b.num(crate::out::unit_level(alpha), 4);
        let a = String::from_utf8_lossy(&b.v).into_owned();
        self.out
            .object(id, format!("<</Type/ExtGState/ca {a}/CA {a}>>").as_bytes())?;
        self.gs_objs.insert(alpha, id);
        Ok(id)
    }

    fn flush_page(
        &mut self,
        mut page: PageData,
        prepared: &[Option<Prepared>],
    ) -> Result<(), PdfError> {
        for p in std::mem::take(&mut page.patches) {
            let cid = prepared
                .get(p.font)
                .and_then(Option::as_ref)
                .and_then(|pr| pr.cid_of.get(&p.gid))
                .ok_or(PdfError::Internal("no CID for glyph"))?;
            let hex = format!("{cid:04X}");
            page.content
                .get_mut(p.offset..p.offset + 4)
                .ok_or(PdfError::Internal("bad patch offset"))?
                .copy_from_slice(hex.as_bytes());
        }
        let compress = self.options.compress;
        let content = self.out.alloc()?;
        self.out.stream(content, b"", &page.content, compress)?;
        let mut annot_ids = Vec::new();
        for a in &page.annots {
            let id = self.out.alloc()?;
            let mut b = Buf::default();
            b.raw("<</Type/Annot/Subtype/Link/Border[0 0 0]/Rect[");
            for (i, v) in a.rect.iter().enumerate() {
                if i > 0 {
                    b.byte(b' ');
                }
                b.num(*v, 3);
            }
            b.raw("]");
            match &a.kind {
                LinkKind::Uri(u) => {
                    b.raw("/A<</S/URI/URI");
                    uri_string(&mut b.v, u);
                    b.raw(">>");
                }
                LinkKind::Named(n) => {
                    b.raw("/Dest");
                    text_string(&mut b.v, n);
                }
            }
            b.raw(">>");
            if b.bad {
                return Err(PdfError::CoordinateOutOfRange);
            }
            self.out.object(id, &b.v)?;
            annot_ids.push(id);
        }
        let mut gs = Vec::new();
        for &alpha in &page.alphas {
            gs.push((alpha, self.gs_object(alpha)?));
        }
        let mut b = Buf::default();
        let leaf = *self
            .leaves
            .get(page.index / FANOUT)
            .ok_or(PdfError::Internal("page without parent"))?;
        let id = *self
            .page_ids
            .get(page.index)
            .ok_or(PdfError::Internal("page without id"))?;
        b.raw(&format!("<</Type/Page/Parent {leaf} 0 R/MediaBox[0 0 "));
        b.num(page.width, 3);
        b.byte(b' ');
        b.num(page.height, 3);
        b.raw(&format!(
            "]/Contents {content} 0 R/Resources<</ProcSet[/PDF/Text/ImageB/ImageC]"
        ));
        if !page.fonts.is_empty() {
            b.raw("/Font<<");
            for &f in &page.fonts {
                let obj = self.fonts.get(f).map_or(0, |e| e.obj);
                b.raw(&format!("/F{f} {obj} 0 R"));
            }
            b.raw(">>");
        }
        if !page.images.is_empty() {
            b.raw("/XObject<<");
            for &i in &page.images {
                let obj = self.image_objs.get(i).copied().unwrap_or(0);
                b.raw(&format!("/Im{i} {obj} 0 R"));
            }
            b.raw(">>");
        }
        if !gs.is_empty() {
            b.raw("/ExtGState<<");
            for (a, obj) in &gs {
                b.raw(&format!("/GS{a} {obj} 0 R"));
            }
            b.raw(">>");
        }
        b.raw(">>");
        if !gs.is_empty() {
            b.raw("/Group<</S/Transparency/CS/DeviceRGB>>");
        }
        if !annot_ids.is_empty() {
            b.raw("/Annots[");
            for (i, a) in annot_ids.iter().enumerate() {
                if i > 0 {
                    b.byte(b' ');
                }
                b.raw(&format!("{a} 0 R"));
            }
            b.raw("]");
        }
        b.raw(">>");
        self.out.object(id, &b.v)
    }

    fn dest_array(&self, d: &Destination) -> Result<String, PdfError> {
        let id = *self
            .page_ids
            .get(d.page)
            .ok_or(PdfError::InvalidDestination)?;
        let (_, h) = self.page_sizes[d.page];
        let mut b = Buf::default();
        match d.top {
            Some(t) => {
                b.num(h - t, 3);
            }
            None => b.raw("null"),
        }
        if b.bad {
            return Err(PdfError::CoordinateOutOfRange);
        }
        Ok(format!(
            "[{id} 0 R/XYZ null {} null]",
            String::from_utf8_lossy(&b.v)
        ))
    }

    fn write_outlines(&mut self) -> Result<Option<u32>, PdfError> {
        let items = std::mem::take(&mut self.options.outlines);
        if items.is_empty() {
            return Ok(None);
        }
        let root = self.out.alloc()?;
        let visible = self.write_outline_level(&items, root, root)?;
        let first = visible
            .0
            .first()
            .copied()
            .ok_or(PdfError::Internal("outline"))?;
        let last = visible
            .0
            .last()
            .copied()
            .ok_or(PdfError::Internal("outline"))?;
        self.out.object(
            root,
            format!(
                "<</Type/Outlines/First {first} 0 R/Last {last} 0 R/Count {}>>",
                visible.1
            )
            .as_bytes(),
        )?;
        Ok(Some(root))
    }

    /// Writes siblings; returns their object numbers and the visible descendant count.
    fn write_outline_level(
        &mut self,
        items: &[OutlineItem],
        parent: u32,
        _root: u32,
    ) -> Result<(Vec<u32>, i64), PdfError> {
        let mut ids = Vec::with_capacity(items.len());
        for _ in items {
            ids.push(self.out.alloc()?);
        }
        let mut visible = 0i64;
        for (k, item) in items.iter().enumerate() {
            let (child_ids, child_visible) = if item.children.is_empty() {
                (Vec::new(), 0)
            } else {
                self.write_outline_level(&item.children, ids[k], _root)?
            };
            visible += 1 + if item.open { child_visible } else { 0 };
            let mut b = Vec::new();
            b.extend_from_slice(b"<</Title");
            text_string(&mut b, &item.title);
            b.extend_from_slice(format!("/Parent {parent} 0 R").as_bytes());
            if k > 0 {
                b.extend_from_slice(format!("/Prev {} 0 R", ids[k - 1]).as_bytes());
            }
            if let Some(n) = ids.get(k + 1) {
                b.extend_from_slice(format!("/Next {n} 0 R").as_bytes());
            }
            if let (Some(f), Some(l)) = (child_ids.first(), child_ids.last()) {
                let count = if item.open {
                    child_visible
                } else {
                    -child_visible
                };
                b.extend_from_slice(
                    format!("/First {f} 0 R/Last {l} 0 R/Count {count}").as_bytes(),
                );
            }
            b.extend_from_slice(
                format!("/Dest{}>>", self.dest_array(&item.destination)?).as_bytes(),
            );
            self.out.object(ids[k], &b)?;
        }
        Ok((ids, visible))
    }

    /// Finishes the document and returns the sink and the notices.
    pub fn finish<P: PdfResourceProvider + ?Sized>(
        mut self,
        provider: &P,
    ) -> Result<(W, Vec<PdfNotice>), PdfError> {
        if self.page_ids.is_empty() {
            return Err(PdfError::EmptyDocument);
        }
        let compress = self.options.compress;
        // Fonts: subset now that the union of glyphs is known.
        let mut prepared: Vec<Option<Prepared>> = Vec::with_capacity(self.fonts.len());
        for e in &self.fonts {
            if e.mode == Mode::Paths {
                prepared.push(None);
                continue;
            }
            let f = provider
                .font(e.font_id)
                .ok_or(PdfError::MissingFont(e.font_id))?;
            prepared.push(Some(font::prepare(e, &f)?));
        }
        for page in std::mem::take(&mut self.pending) {
            self.flush_page(page, &prepared)?;
        }
        for (e, p) in self.fonts.iter().zip(&prepared) {
            if let Some(p) = p {
                font::write(&mut self.out, e, p, compress)?;
            }
        }
        // Destinations referenced by name.
        let mut names: Option<u32> = None;
        if !self.dests.is_empty() {
            let mut b = Vec::new();
            b.extend_from_slice(b"<</Names[");
            for (name, d) in &self.dests {
                text_string(&mut b, name);
                b.extend_from_slice(self.dest_array(d)?.as_bytes());
                b.push(b'\n');
            }
            b.extend_from_slice(b"]>>");
            let id = self.out.alloc()?;
            self.out.object(id, &b)?;
            names = Some(id);
        }
        let outlines = self.write_outlines()?;
        // Page tree, balanced.
        let mut levels: Vec<Vec<u32>> = vec![self.leaves.clone()];
        while levels.last().is_some_and(|l| l.len() > 1) {
            let n = levels.last().map_or(0, |l| l.len().div_ceil(FANOUT));
            let mut ids = Vec::with_capacity(n);
            for _ in 0..n {
                ids.push(self.out.alloc()?);
            }
            levels.push(ids);
        }
        let mut counts: Vec<usize> = Vec::new();
        for (depth, nodes) in levels.iter().enumerate() {
            let mut next_counts = Vec::with_capacity(nodes.len());
            for (i, &node) in nodes.iter().enumerate() {
                let kids: Vec<u32> = if depth == 0 {
                    self.page_ids
                        .chunks(FANOUT)
                        .nth(i)
                        .map(<[u32]>::to_vec)
                        .unwrap_or_default()
                } else {
                    levels[depth - 1]
                        .chunks(FANOUT)
                        .nth(i)
                        .map(<[u32]>::to_vec)
                        .unwrap_or_default()
                };
                let count: usize = if depth == 0 {
                    kids.len()
                } else {
                    counts.chunks(FANOUT).nth(i).map_or(0, |c| c.iter().sum())
                };
                let mut b = format!("<</Type/Pages/Count {count}/Kids[");
                for (k, id) in kids.iter().enumerate() {
                    if k > 0 {
                        b.push(' ');
                    }
                    b.push_str(&format!("{id} 0 R"));
                }
                b.push(']');
                if let Some(parent) = levels.get(depth + 1).and_then(|l| l.get(i / FANOUT)) {
                    b.push_str(&format!("/Parent {parent} 0 R"));
                }
                b.push_str(">>");
                self.out.object(node, b.as_bytes())?;
                next_counts.push(count);
            }
            counts = next_counts;
        }
        let root = levels
            .last()
            .and_then(|l| l.first())
            .copied()
            .ok_or(PdfError::Internal("empty page tree"))?;
        // Catalog.
        let catalog = self.out.alloc()?;
        let mut c = format!("<</Type/Catalog/Pages {root} 0 R");
        if let Some(o) = outlines {
            c.push_str(&format!("/Outlines {o} 0 R/PageMode/UseOutlines"));
        }
        if let Some(n) = names {
            c.push_str(&format!("/Names<</Dests {n} 0 R>>"));
        }
        if let Some(l) = &self.options.language {
            c.push_str(&format!("/Lang({l})"));
        }
        c.push_str(">>");
        self.out.object(catalog, c.as_bytes())?;
        // Information dictionary.
        let info_id = self.out.alloc()?;
        let mut b = Vec::new();
        b.extend_from_slice(b"<<");
        let info = &self.options.info;
        for (key, v) in [
            ("Title", &info.title),
            ("Author", &info.author),
            ("Subject", &info.subject),
            ("Keywords", &info.keywords),
            ("Creator", &info.creator),
            ("Producer", &info.producer),
        ] {
            if let Some(s) = v {
                b.extend_from_slice(format!("/{key}").as_bytes());
                text_string(&mut b, s);
            }
        }
        for (key, d) in [
            ("CreationDate", &info.creation_date),
            ("ModDate", &info.modification_date),
        ] {
            if let Some(d) = d {
                b.extend_from_slice(format!("/{key}").as_bytes());
                text_string(&mut b, &date_string(d));
            }
        }
        b.extend_from_slice(b">>");
        self.out.object(info_id, &b)?;
        // Cross-reference table and trailer.
        let digest = self.out.digest();
        let start = self.out.xref()?;
        let hex: String = digest.iter().map(|x| format!("{x:02X}")).collect();
        let size = self.out_object_count() + 1;
        self.out.write(
            format!(
                "trailer\n<</Size {size}/Root {catalog} 0 R/Info {info_id} 0 R/ID[<{hex}><{hex}>]>>\nstartxref\n{start}\n%%EOF\n"
            )
            .as_bytes(),
        )?;
        Ok((self.out.into_sink(), self.notices))
    }

    fn out_object_count(&self) -> usize {
        self.out.object_count()
    }
}
