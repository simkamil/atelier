// SPDX-License-Identifier: AGPL-3.0-or-later
//! Display list to page content stream: graphics state, paths, text, images
//! and link rectangles.

use crate::doc::{Annot, LinkKind, PageData, Patch, PdfWriter};
use crate::font::{self, MAX_GLYPH_TEXT, Mode};
use crate::image::{ImageLimits, write_image};
use crate::out::Buf;
use crate::{NoEmbedReason, PdfError, PdfNotice, PdfResourceProvider};
use atelier_geom::Rect;
use atelier_render::{
    Color, Command, DisplayList, DisplayTransform, FillRule, LineCap, LineJoin, PathElement,
    PositionedGlyph, StrokeStyle,
};
use std::collections::BTreeSet;
use std::io::Write;

/// Largest page side in user units (the Acrobat limit, 200 inches).
const MAX_PAGE_SIDE: f64 = 14_400.0;
/// Gap, in em, above which a space is inferred between two glyphs of a run.
const WORD_GAP_EM: f64 = 0.15;
const MAX_NOTICES: usize = 1000;
const IDENT: DisplayTransform = DisplayTransform::identity();

#[derive(Clone, Default)]
struct Gs {
    fill: Option<[u8; 3]>,
    stroke: Option<[u8; 3]>,
    alpha: u8,
    line: Option<(u64, u8, u8, u64)>,
}

struct State {
    b: Buf,
    patches: Vec<Patch>,
    fonts: BTreeSet<usize>,
    images: BTreeSet<usize>,
    alphas: BTreeSet<u8>,
    annots: Vec<Annot>,
    clips: Vec<Rect>,
    tfs: Vec<DisplayTransform>,
    em_clips: Vec<Rect>,
    em_tf: DisplayTransform,
    clip_q: bool,
    tf_q: bool,
    cur: Gs,
    saved: Vec<Gs>,
    max_content: usize,
}

fn then(a: DisplayTransform, n: DisplayTransform) -> DisplayTransform {
    DisplayTransform {
        a: n.a * a.a + n.c * a.b,
        b: n.b * a.a + n.d * a.b,
        c: n.a * a.c + n.c * a.d,
        d: n.b * a.c + n.d * a.d,
        e: n.a * a.e + n.c * a.f + n.e,
        f: n.b * a.e + n.d * a.f + n.f,
    }
}

fn quant(c: f32) -> u8 {
    (c * 255.0).round().clamp(0.0, 255.0) as u8
}

impl State {
    fn q(&mut self) {
        self.b.op("q");
        self.saved.push(self.cur.clone());
    }
    fn big_q(&mut self) {
        self.b.op("Q");
        if let Some(g) = self.saved.pop() {
            self.cur = g;
        }
    }

    /// Brings the PDF graphics state to the current clip stack and transform.
    fn sync(&mut self) {
        let want = self.tfs.last().copied().unwrap_or(IDENT);
        if self.em_clips != self.clips {
            if self.tf_q {
                self.big_q();
                self.tf_q = false;
            }
            if self.clip_q {
                self.big_q();
                self.clip_q = false;
            }
            self.em_tf = IDENT;
            if !self.clips.is_empty() {
                self.q();
                self.clip_q = true;
                for i in 0..self.clips.len() {
                    let r = self.clips[i];
                    self.b.n(r.left());
                    self.b.n(r.top());
                    self.b.n(r.width());
                    self.b.n(r.height());
                    self.b.op("re W n");
                }
            }
            self.em_clips = self.clips.clone();
        }
        if self.em_tf != want {
            if self.tf_q {
                self.big_q();
                self.tf_q = false;
            }
            if want != IDENT {
                self.q();
                self.tf_q = true;
                self.b.n6(want.a);
                self.b.n6(want.b);
                self.b.n6(want.c);
                self.b.n6(want.d);
                self.b.n(want.e);
                self.b.n(want.f);
                self.b.op("cm");
            }
            self.em_tf = want;
        }
    }

    fn gray_or_rgb(&mut self, c: [u8; 3], gray_op: &str, rgb_op: &str) {
        let f = |v: u8| f64::from(v) / 255.0;
        if c[0] == c[1] && c[1] == c[2] {
            self.b.n(f(c[0]));
            self.b.op(gray_op);
        } else {
            for v in c {
                self.b.n(f(v));
            }
            self.b.op(rgb_op);
        }
    }
    fn set_fill(&mut self, c: [u8; 3]) {
        if self.cur.fill != Some(c) {
            self.gray_or_rgb(c, "g", "rg");
            self.cur.fill = Some(c);
        }
    }
    fn set_stroke(&mut self, c: [u8; 3]) {
        if self.cur.stroke != Some(c) {
            self.gray_or_rgb(c, "G", "RG");
            self.cur.stroke = Some(c);
        }
    }
    fn set_alpha(&mut self, a: u8) {
        if self.cur.alpha != a {
            self.b.raw(&format!("/GS{a} gs\n"));
            self.alphas.insert(a);
            self.cur.alpha = a;
        }
    }

    fn path(&mut self, elements: &[PathElement], dx: f64, dy: f64) {
        let mut cur = (0.0, 0.0);
        let mut start = (0.0, 0.0);
        for e in elements {
            match *e {
                PathElement::MoveTo(p) => {
                    cur = (p.x() + dx, p.y() + dy);
                    start = cur;
                    self.b.n(cur.0);
                    self.b.n(cur.1);
                    self.b.op("m");
                }
                PathElement::LineTo(p) => {
                    cur = (p.x() + dx, p.y() + dy);
                    self.b.n(cur.0);
                    self.b.n(cur.1);
                    self.b.op("l");
                }
                PathElement::QuadraticTo { control, to } => {
                    let q = (control.x() + dx, control.y() + dy);
                    let t = (to.x() + dx, to.y() + dy);
                    let c1 = (
                        cur.0 + 2.0 / 3.0 * (q.0 - cur.0),
                        cur.1 + 2.0 / 3.0 * (q.1 - cur.1),
                    );
                    let c2 = (t.0 + 2.0 / 3.0 * (q.0 - t.0), t.1 + 2.0 / 3.0 * (q.1 - t.1));
                    for v in [c1.0, c1.1, c2.0, c2.1, t.0, t.1] {
                        self.b.n(v);
                    }
                    self.b.op("c");
                    cur = t;
                }
                PathElement::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    for p in [control1, control2, to] {
                        self.b.n(p.x() + dx);
                        self.b.n(p.y() + dy);
                    }
                    self.b.op("c");
                    cur = (to.x() + dx, to.y() + dy);
                }
                PathElement::Close => {
                    self.b.op("h");
                    cur = start;
                }
            }
        }
    }

    fn check(&self) -> Result<(), PdfError> {
        if self.b.bad {
            return Err(PdfError::CoordinateOutOfRange);
        }
        if self.b.v.len() > self.max_content {
            return Err(PdfError::LimitExceeded("page content size"));
        }
        Ok(())
    }
}

fn rgb(c: &Color) -> [u8; 3] {
    [quant(c.red), quant(c.green), quant(c.blue)]
}

impl<W: Write> PdfWriter<W> {
    pub(crate) fn notice(&mut self, n: PdfNotice) {
        if self.notices.len() < MAX_NOTICES {
            self.notices.push(n);
        } else if self.notices.len() == MAX_NOTICES {
            self.notices.push(PdfNotice::NoticesTruncated);
        }
    }

    /// Index of the (shared) font entry behind `font_id`, and the em size.
    fn font_for<P: PdfResourceProvider + ?Sized>(
        &mut self,
        provider: &P,
        font_id: u64,
    ) -> Result<(usize, f64), PdfError> {
        let f = provider
            .font(font_id)
            .ok_or(PdfError::MissingFont(font_id))?;
        if !f.size.is_finite() || f.size <= 0.0 || f.size > 1.0e6 {
            return Err(PdfError::CoordinateOutOfRange);
        }
        if let Some(&i) = self.font_by_id.get(&font_id) {
            return Ok((i, f.size));
        }
        let key = (f.program_id, f.face_index);
        let idx = if let Some(&i) = self.font_by_key.get(&key) {
            i
        } else {
            if self.fonts.len() >= self.options.limits.max_fonts {
                return Err(PdfError::LimitExceeded("fonts"));
            }
            let mut created = font::create(font_id, &f)?;
            if let Some((mode, reason)) = created.notice {
                let n = match (mode, reason) {
                    (Mode::Whole, _) => PdfNotice::FontEmbeddedWhole { font_id },
                    (_, r) => PdfNotice::FontDrawnAsPaths {
                        font_id,
                        reason: r.unwrap_or(NoEmbedReason::Restricted),
                    },
                };
                self.notice(n);
            }
            if created.entry.mode != Mode::Paths {
                created.entry.obj = self.out.alloc()?;
            }
            self.fonts.push(created.entry);
            let i = self.fonts.len() - 1;
            self.font_by_key.insert(key, i);
            i
        };
        self.font_by_id.insert(font_id, idx);
        Ok((idx, f.size))
    }

    fn image_for<P: PdfResourceProvider + ?Sized>(
        &mut self,
        provider: &P,
        image_id: u64,
    ) -> Result<usize, PdfError> {
        if let Some(&i) = self.image_by_id.get(&image_id) {
            return Ok(i);
        }
        let img = provider
            .image(image_id)
            .ok_or(PdfError::MissingImage(image_id))?;
        let lim = &self.options.limits;
        let limits = ImageLimits {
            max_bytes: lim.max_image_bytes,
            max_pixels: lim.max_image_pixels,
        };
        let obj = write_image(
            &mut self.out,
            img.mime_type,
            img.bytes,
            limits,
            self.options.compress,
        )?;
        self.image_objs.push(obj);
        let i = self.image_objs.len() - 1;
        self.image_by_id.insert(image_id, i);
        Ok(i)
    }

    /// Records the glyphs and their text for the font's `ToUnicode` map.
    fn note_glyphs<P: PdfResourceProvider + ?Sized>(
        &mut self,
        provider: &P,
        font_id: u64,
        fi: usize,
        glyphs: &[PositionedGlyph],
    ) -> Result<(), PdfError> {
        for g in glyphs {
            let gid = u16::try_from(g.glyph_id)
                .ok()
                .filter(|&x| x < self.fonts[fi].num_glyphs)
                .ok_or(PdfError::UnknownGlyph {
                    font_id,
                    glyph_id: g.glyph_id,
                })?;
            let text = provider.glyph_text(font_id, g.glyph_id);
            let entry = &mut self.fonts[fi];
            entry.gids.insert(gid);
            let mut report = None;
            match text {
                Some(t) if !t.is_empty() => {
                    if t.chars().count() > MAX_GLYPH_TEXT {
                        return Err(PdfError::LimitExceeded("glyph text"));
                    }
                    match entry.text.get(&gid) {
                        None => {
                            entry.text.insert(gid, t.to_owned());
                        }
                        Some(old) if old != t && entry.text_reported.insert(gid) => {
                            report = Some(PdfNotice::ConflictingGlyphText {
                                font_id,
                                glyph_id: g.glyph_id,
                            });
                        }
                        Some(_) => {}
                    }
                }
                _ => {
                    if !entry.text.contains_key(&gid) && entry.text_reported.insert(gid) {
                        report = Some(PdfNotice::MissingGlyphText {
                            font_id,
                            glyph_id: g.glyph_id,
                        });
                    }
                }
            }
            if let Some(n) = report {
                self.notice(n);
            }
        }
        Ok(())
    }

    fn glyph_run<P: PdfResourceProvider + ?Sized>(
        &mut self,
        st: &mut State,
        provider: &P,
        font_id: u64,
        color: &Color,
        glyphs: &[PositionedGlyph],
    ) -> Result<(), PdfError> {
        let (fi, size) = self.font_for(provider, font_id)?;
        let alpha = quant(color.alpha);
        if self.fonts[fi].mode == Mode::Paths {
            for g in glyphs {
                if g.glyph_id >= u32::from(self.fonts[fi].num_glyphs) {
                    return Err(PdfError::UnknownGlyph {
                        font_id,
                        glyph_id: g.glyph_id,
                    });
                }
                let outline = provider.glyph_outline(font_id, g.glyph_id).ok_or(
                    PdfError::MissingOutline {
                        font_id,
                        glyph_id: g.glyph_id,
                    },
                )?;
                if alpha == 0 || outline.elements.is_empty() {
                    continue;
                }
                st.sync();
                st.set_alpha(alpha);
                st.set_fill(rgb(color));
                st.path(&outline.elements, g.position.x(), g.position.y());
                st.b.op("f");
                st.check()?;
            }
            return Ok(());
        }
        self.note_glyphs(provider, font_id, fi, glyphs)?;
        if alpha == 0 || glyphs.is_empty() {
            return Ok(());
        }
        // Effective glyph list, with inferred spaces.
        let entry = &mut self.fonts[fi];
        let space = entry.space_gid;
        let infer = self.options.infer_spaces;
        let mut eff: Vec<(u16, f64, f64)> = Vec::with_capacity(glyphs.len());
        for (i, g) in glyphs.iter().enumerate() {
            let gid = g.glyph_id as u16;
            let (x, y) = (g.position.x(), g.position.y());
            if let (true, Some(sp), Some(&(pg, px, py))) = (infer, space, eff.last()) {
                let is_space = |gid: u16| {
                    gid == sp
                        || entry
                            .text
                            .get(&gid)
                            .is_some_and(|t| t.chars().all(char::is_whitespace))
                };
                let end = px + f64::from(entry.widths[usize::from(pg)]) * size / 1000.0;
                if (y - py).abs() <= 0.001
                    && x - end > WORD_GAP_EM * size
                    && !is_space(pg)
                    && !is_space(gid)
                    && i > 0
                {
                    eff.push((sp, end, py));
                }
            }
            eff.push((gid, x, y));
        }
        if eff.iter().any(|e| Some(e.0) == space) {
            if let Some(sp) = space {
                entry.gids.insert(sp);
                entry.text.entry(sp).or_insert_with(|| " ".to_owned());
            }
        }
        let cff = entry.cff;
        st.sync();
        st.set_alpha(alpha);
        st.set_fill(rgb(color));
        st.fonts.insert(fi);
        st.b.raw("BT\n");
        st.b.raw(&format!("/F{fi} "));
        st.b.n(size);
        st.b.op("Tf");
        let mut i = 0;
        while i < eff.len() {
            let (_, x0, y0) = eff[i];
            st.b.raw("1 0 0 -1 ");
            st.b.n(x0);
            st.b.n(y0);
            st.b.op("Tm");
            st.b.byte(b'[');
            let mut pen = x0;
            let mut first = true;
            while i < eff.len() && (eff[i].2 - y0).abs() <= 0.001 {
                let (gid, x, _) = eff[i];
                if !first {
                    let delta = (x - pen) / size * 1000.0;
                    let r = (delta * 10.0).round() / 10.0;
                    if r != 0.0 {
                        st.b.num(-r, 1);
                        pen += r * size / 1000.0;
                    }
                }
                first = false;
                st.b.byte(b'<');
                if cff {
                    st.patches.push(Patch {
                        offset: st.b.v.len(),
                        font: fi,
                        gid,
                    });
                }
                st.b.raw(&format!("{gid:04X}"));
                st.b.byte(b'>');
                pen += f64::from(self.fonts[fi].widths[usize::from(gid)]) * size / 1000.0;
                i += 1;
            }
            st.b.raw("]TJ\n");
        }
        st.b.op("ET");
        st.check()
    }

    fn stroke_style(st: &mut State, style: &StrokeStyle) {
        let cap = match style.cap {
            LineCap::Butt => 0u8,
            LineCap::Round => 1,
            LineCap::Square => 2,
        };
        let join = match style.join {
            LineJoin::Miter => 0u8,
            LineJoin::Round => 1,
            LineJoin::Bevel => 2,
        };
        let width = style.width.max(0.001);
        let key = (width.to_bits(), cap, join, style.miter_limit.to_bits());
        if st.cur.line != Some(key) {
            st.b.n(width);
            st.b.op("w");
            st.b.raw(&format!("{cap} J\n{join} j\n"));
            st.b.n(style.miter_limit);
            st.b.op("M");
            st.cur.line = Some(key);
        }
    }

    /// Builds the content stream and resources of one page.
    pub(crate) fn build_page<P: PdfResourceProvider + ?Sized>(
        &mut self,
        list: &DisplayList,
        provider: &P,
        index: usize,
    ) -> Result<PageData, PdfError> {
        list.validate().map_err(|_| PdfError::InvalidList)?;
        let bounds = list.page.bounds;
        let (x0, y0, w, h) = (bounds.left(), bounds.top(), bounds.width(), bounds.height());
        if w > MAX_PAGE_SIDE || h > MAX_PAGE_SIDE {
            return Err(PdfError::PageTooLarge);
        }
        let mut st = State {
            b: Buf::default(),
            patches: Vec::new(),
            fonts: BTreeSet::new(),
            images: BTreeSet::new(),
            alphas: BTreeSet::new(),
            annots: Vec::new(),
            clips: Vec::new(),
            tfs: vec![IDENT],
            em_clips: Vec::new(),
            em_tf: IDENT,
            clip_q: false,
            tf_q: false,
            cur: Gs {
                alpha: 255,
                ..Gs::default()
            },
            saved: Vec::new(),
            max_content: self.options.limits.max_content_bytes,
        };
        // Page coordinates have the origin at the top left, PDF at the bottom left.
        st.b.raw("1 0 0 -1 ");
        st.b.n(-x0);
        st.b.n(y0 + h);
        st.b.op("cm");
        for cmd in &list.commands {
            match cmd {
                Command::FillPath { path, color, rule } => {
                    let alpha = quant(color.alpha);
                    if alpha == 0 || path.elements.is_empty() {
                        continue;
                    }
                    st.sync();
                    st.set_alpha(alpha);
                    st.set_fill(rgb(color));
                    st.path(&path.elements, 0.0, 0.0);
                    st.b.op(match rule {
                        FillRule::NonZero => "f",
                        FillRule::EvenOdd => "f*",
                    });
                }
                Command::StrokePath { path, color, style } => {
                    let alpha = quant(color.alpha);
                    if alpha == 0 || path.elements.is_empty() {
                        continue;
                    }
                    st.sync();
                    st.set_alpha(alpha);
                    st.set_stroke(rgb(color));
                    Self::stroke_style(&mut st, style);
                    st.path(&path.elements, 0.0, 0.0);
                    st.b.op("S");
                }
                Command::DrawGlyphRun {
                    font_id,
                    color,
                    glyphs,
                } => self.glyph_run(&mut st, provider, *font_id, color, glyphs)?,
                Command::DrawImage { image_id, bounds } => {
                    let ii = self.image_for(provider, *image_id)?;
                    st.sync();
                    st.set_alpha(255);
                    st.images.insert(ii);
                    st.q();
                    st.b.n(bounds.width());
                    st.b.n(0.0);
                    st.b.n(0.0);
                    st.b.n(-bounds.height());
                    st.b.n(bounds.left());
                    st.b.n(bounds.bottom());
                    st.b.op("cm");
                    st.b.raw(&format!("/Im{ii} Do\n"));
                    st.big_q();
                }
                Command::PushClip { bounds } => st.clips.push(*bounds),
                Command::PopClip => {
                    st.clips.pop();
                }
                Command::PushTransform { transform } => {
                    let cur = st.tfs.last().copied().unwrap_or(IDENT);
                    st.tfs.push(then(cur, *transform));
                }
                Command::PopTransform => {
                    st.tfs.pop();
                }
                Command::Link { bounds, target } => {
                    let rect = [
                        bounds.left() - x0,
                        y0 + h - bounds.bottom(),
                        bounds.right() - x0,
                        y0 + h - bounds.top(),
                    ];
                    let kind = match target.strip_prefix('#') {
                        Some(name) if self.dests.contains_key(name) => {
                            Some(LinkKind::Named(name.to_owned()))
                        }
                        Some(_) => None,
                        None if !target.is_empty() => Some(LinkKind::Uri(target.clone())),
                        None => None,
                    };
                    match kind {
                        Some(kind) => st.annots.push(Annot { rect, kind }),
                        None => self.notice(PdfNotice::UnresolvedLink {
                            page: index,
                            target: target.clone(),
                        }),
                    }
                }
                Command::Structure { .. } => {}
            }
            st.check()?;
        }
        if st.tf_q {
            st.big_q();
        }
        if st.clip_q {
            st.big_q();
        }
        st.check()?;
        if st
            .annots
            .iter()
            .any(|a| a.rect.iter().any(|v| !v.is_finite() || v.abs() > 1e9))
        {
            return Err(PdfError::CoordinateOutOfRange);
        }
        Ok(PageData {
            index,
            width: w,
            height: h,
            content: st.b.v,
            patches: st.patches,
            fonts: st.fonts,
            images: st.images,
            alphas: st.alphas,
            annots: st.annots,
        })
    }
}
