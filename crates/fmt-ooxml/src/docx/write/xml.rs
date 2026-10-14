// SPDX-License-Identifier: AGPL-3.0-or-later
//! Minimal deterministic XML output used by the DOCX writer.

/// Appends `text` escaped for element content.
pub(super) fn escape_text(out: &mut Vec<u8>, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.extend_from_slice(b"&amp;"),
            '<' => out.extend_from_slice(b"&lt;"),
            '>' => out.extend_from_slice(b"&gt;"),
            '\r' => out.extend_from_slice(b"&#13;"),
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
}

/// Appends `text` escaped for a double-quoted attribute value.
pub(super) fn escape_attr(out: &mut Vec<u8>, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.extend_from_slice(b"&amp;"),
            '<' => out.extend_from_slice(b"&lt;"),
            '>' => out.extend_from_slice(b"&gt;"),
            '"' => out.extend_from_slice(b"&quot;"),
            '\t' => out.extend_from_slice(b"&#9;"),
            '\n' => out.extend_from_slice(b"&#10;"),
            '\r' => out.extend_from_slice(b"&#13;"),
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
}

/// True for the scalar values XML 1.0 allows in a document.
pub(super) fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}')
}

/// A streaming XML writer with a pending start tag, so that an element without
/// content is written as `<a/>`. Output can be muted for the parts of a
/// skeleton that must not be written (`shadow` elements) while the walk goes on.
#[derive(Default)]
pub(super) struct Out {
    buf: Vec<u8>,
    pending: bool,
    muted: u32,
    stack: Vec<String>,
}

impl Out {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Position of the output: length, pending start tag, open elements.
    pub fn snapshot(&self) -> (usize, bool, usize) {
        (self.buf.len(), self.pending, self.stack.len())
    }

    /// Drops everything written since a [`snapshot`](Self::snapshot).
    pub fn restore(&mut self, len: usize, pending: bool, stack: usize) {
        self.buf.truncate(len);
        self.pending = pending;
        self.stack.truncate(stack);
    }

    pub fn is_muted(&self) -> bool {
        self.muted > 0
    }

    pub fn mute(&mut self) {
        self.muted += 1;
    }

    pub fn unmute(&mut self) {
        self.muted = self.muted.saturating_sub(1);
    }

    fn close_pending(&mut self) {
        if self.pending {
            self.buf.push(b'>');
            self.pending = false;
        }
    }

    pub fn declaration(&mut self) {
        self.buf.extend_from_slice(
            b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
        );
    }

    /// Opens an element: `attrs` are written in the given order.
    pub fn start<'a>(&mut self, name: &str, attrs: impl IntoIterator<Item = (&'a str, &'a str)>) {
        self.stack.push(name.to_owned());
        if self.muted > 0 {
            return;
        }
        self.close_pending();
        self.buf.push(b'<');
        self.buf.extend_from_slice(name.as_bytes());
        for (key, value) in attrs {
            self.buf.push(b' ');
            self.buf.extend_from_slice(key.as_bytes());
            self.buf.extend_from_slice(b"=\"");
            escape_attr(&mut self.buf, value);
            self.buf.push(b'"');
        }
        self.pending = true;
    }

    pub fn end(&mut self) {
        let Some(name) = self.stack.pop() else {
            return;
        };
        if self.muted > 0 {
            return;
        }
        if self.pending {
            self.buf.extend_from_slice(b"/>");
            self.pending = false;
        } else {
            self.buf.extend_from_slice(b"</");
            self.buf.extend_from_slice(name.as_bytes());
            self.buf.push(b'>');
        }
    }

    /// An element with attributes and no content.
    pub fn empty<'a>(&mut self, name: &str, attrs: impl IntoIterator<Item = (&'a str, &'a str)>) {
        self.start(name, attrs);
        self.end();
    }

    pub fn text(&mut self, text: &str) {
        if self.muted > 0 || text.is_empty() {
            return;
        }
        self.close_pending();
        escape_text(&mut self.buf, text);
    }

    /// Already serialized XML (a capture), written as is.
    pub fn raw(&mut self, bytes: &[u8]) {
        if self.muted > 0 || bytes.is_empty() {
            return;
        }
        self.close_pending();
        self.buf.extend_from_slice(bytes);
    }
}
