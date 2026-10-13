// SPDX-License-Identifier: AGPL-3.0-or-later
//! Byte sink with object numbering, offsets, a running hash and the PDF
//! primitives (numbers, strings, names, streams) shared by every module.

use crate::PdfError;
use atelier_compress::{CompressError, Level, compress_zlib};
use std::io::Write;

/// Number buffer that remembers whether a non-finite or absurd value was written.
#[derive(Default)]
pub(crate) struct Buf {
    pub v: Vec<u8>,
    pub bad: bool,
}

/// Largest absolute coordinate accepted in a content stream (PDF readers
/// work in 32-bit fixed or single precision).
const MAX_ABS: f64 = 1.0e9;

impl Buf {
    pub fn raw(&mut self, s: &str) {
        self.v.extend_from_slice(s.as_bytes());
    }
    pub fn byte(&mut self, b: u8) {
        self.v.push(b);
    }
    /// Writes `x` with at most `decimals` fraction digits, trimmed, never in exponent form.
    pub fn num(&mut self, x: f64, decimals: u32) {
        write_num(&mut self.v, &mut self.bad, x, decimals);
    }
    /// Operand followed by a space.
    pub fn n(&mut self, x: f64) {
        self.num(x, 3);
        self.v.push(b' ');
    }
    pub fn n6(&mut self, x: f64) {
        self.num(x, 6);
        self.v.push(b' ');
    }
    pub fn op(&mut self, name: &str) {
        self.v.extend_from_slice(name.as_bytes());
        self.v.push(b'\n');
    }
}

/// An 8-bit level as a number in 0..=1. Intermediate levels are written at
/// a quarter of their 1/255 step above the level, so that a reader that truncates (MuPDF)
/// and one that rounds both come back to the same 8-bit value.
pub(crate) fn unit_level(v: u8) -> f64 {
    match v {
        0 => 0.0,
        255 => 1.0,
        v => (f64::from(v) + 0.25) / 255.0,
    }
}

pub(crate) fn write_num(out: &mut Vec<u8>, bad: &mut bool, x: f64, decimals: u32) {
    if !x.is_finite() || x.abs() > MAX_ABS {
        *bad = true;
        out.push(b'0');
        return;
    }
    let digits = decimals.min(9);
    let scale = 10u64.pow(digits);
    let scaled = (x.abs() * scale as f64).round() as u64;
    if scaled == 0 {
        out.push(b'0');
        return;
    }
    if x < 0.0 {
        out.push(b'-');
    }
    let int = scaled / scale;
    let mut frac = scaled % scale;
    out.extend_from_slice(int.to_string().as_bytes());
    if frac != 0 {
        let mut width = digits as usize;
        while frac % 10 == 0 {
            frac /= 10;
            width -= 1;
        }
        out.push(b'.');
        out.extend_from_slice(format!("{frac:0width$}").as_bytes());
    }
}

/// Literal string for printable ASCII, otherwise UTF-16BE with a byte order mark (hex).
pub(crate) fn text_string(out: &mut Vec<u8>, s: &str) {
    if s.chars().all(|c| (' '..='~').contains(&c)) {
        out.push(b'(');
        for b in s.bytes() {
            if matches!(b, b'(' | b')' | b'\\') {
                out.push(b'\\');
            }
            out.push(b);
        }
        out.push(b')');
    } else {
        out.extend_from_slice(b"<FEFF");
        for u in s.encode_utf16() {
            out.extend_from_slice(format!("{u:04X}").as_bytes());
        }
        out.push(b'>');
    }
}

/// ASCII literal string for URIs: bytes outside `!`..`~` are percent-encoded.
pub(crate) fn uri_string(out: &mut Vec<u8>, s: &str) {
    out.push(b'(');
    for b in s.bytes() {
        if b > b' ' && b < 0x7f {
            if matches!(b, b'(' | b')' | b'\\') {
                out.push(b'\\');
            }
            out.push(b);
        } else {
            out.extend_from_slice(format!("%{b:02X}").as_bytes());
        }
    }
    out.push(b')');
}

pub(crate) fn name(out: &mut Vec<u8>, s: &str) {
    out.push(b'/');
    for b in s.bytes() {
        if b > b' ' && b < 0x7f && !b"#()<>[]{}/%".contains(&b) {
            out.push(b);
        } else {
            out.extend_from_slice(format!("#{b:02X}").as_bytes());
        }
    }
}

/// zlib-compress `data`; `None` when the result would not be smaller.
pub(crate) fn deflate(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 16 {
        return None;
    }
    let mut buf = vec![0u8; data.len() + data.len() / 16 + 64];
    let n = match compress_zlib(data, Level::Normal, &mut buf) {
        Ok(n) => n,
        Err(CompressError::OutputTooSmall { .. }) => return None,
    };
    if n >= data.len() {
        return None;
    }
    buf.truncate(n);
    Some(buf)
}

pub(crate) struct Out<W: Write> {
    sink: W,
    pos: u64,
    h1: u64,
    h2: u64,
    offsets: Vec<u64>,
    max_bytes: u64,
    max_objects: usize,
}

impl<W: Write> Out<W> {
    pub fn new(sink: W, max_bytes: u64, max_objects: usize) -> Self {
        Self {
            sink,
            pos: 0,
            h1: 0xcbf2_9ce4_8422_2325,
            h2: 0x8422_2325_cbf2_9ce4,
            offsets: Vec::new(),
            max_bytes,
            max_objects,
        }
    }
    pub fn into_sink(self) -> W {
        self.sink
    }
    pub fn object_count(&self) -> usize {
        self.offsets.len()
    }
    /// 128-bit digest of everything written so far (for the trailer `/ID`).
    pub fn digest(&self) -> [u8; 16] {
        let mut d = [0u8; 16];
        d[..8].copy_from_slice(&self.h1.to_be_bytes());
        d[8..].copy_from_slice(&self.h2.to_be_bytes());
        d
    }
    pub fn write(&mut self, b: &[u8]) -> Result<(), PdfError> {
        let len = b.len() as u64;
        if self.pos.checked_add(len).is_none_or(|t| t > self.max_bytes) {
            return Err(PdfError::LimitExceeded("output size"));
        }
        self.sink.write_all(b).map_err(|e| PdfError::Io(e.kind()))?;
        for &x in b {
            self.h1 = (self.h1 ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3);
            self.h2 =
                (self.h2 ^ u64::from(x)).wrapping_mul(0x0000_0001_0000_01b3) ^ (self.h2 >> 29);
        }
        self.pos += len;
        Ok(())
    }
    /// Reserves the next object number.
    pub fn alloc(&mut self) -> Result<u32, PdfError> {
        if self.offsets.len() >= self.max_objects {
            return Err(PdfError::LimitExceeded("object count"));
        }
        self.offsets.push(0);
        Ok(self.offsets.len() as u32)
    }
    fn begin(&mut self, id: u32) -> Result<(), PdfError> {
        let pos = self.pos;
        let slot = self
            .offsets
            .get_mut((id as usize).wrapping_sub(1))
            .ok_or(PdfError::Internal("unknown object number"))?;
        if *slot != 0 {
            return Err(PdfError::Internal("object written twice"));
        }
        *slot = pos;
        self.write(format!("{id} 0 obj\n").as_bytes())
    }
    pub fn object(&mut self, id: u32, body: &[u8]) -> Result<(), PdfError> {
        self.begin(id)?;
        self.write(body)?;
        self.write(b"\nendobj\n")
    }
    /// Writes a stream object; `dict` holds the entries without `<<`, `>>` or `/Length`.
    pub fn stream(
        &mut self,
        id: u32,
        dict: &[u8],
        data: &[u8],
        compress: bool,
    ) -> Result<(), PdfError> {
        let packed = if compress { deflate(data) } else { None };
        let body: &[u8] = packed.as_deref().unwrap_or(data);
        let mut head = Vec::with_capacity(dict.len() + 64);
        head.extend_from_slice(b"<<");
        head.extend_from_slice(dict);
        if packed.is_some() {
            head.extend_from_slice(b"/Filter/FlateDecode");
        }
        head.extend_from_slice(format!("/Length {}>>\nstream\n", body.len()).as_bytes());
        self.begin(id)?;
        self.write(&head)?;
        self.write(body)?;
        self.write(b"\nendstream\nendobj\n")
    }
    /// Cross-reference table; every allocated object must have been written.
    pub fn xref(&mut self) -> Result<u64, PdfError> {
        let start = self.pos;
        let n = self.offsets.len() + 1;
        let mut t = Vec::with_capacity(n * 20 + 32);
        t.extend_from_slice(format!("xref\n0 {n}\n").as_bytes());
        t.extend_from_slice(b"0000000000 65535 f \n");
        for &o in &self.offsets {
            if o == 0 {
                return Err(PdfError::Internal("object never written"));
            }
            t.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        self.write(&t)?;
        Ok(start)
    }
}
