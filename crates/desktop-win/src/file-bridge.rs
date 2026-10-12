// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded native file-message protocol shared by the WebView callback and STA.

use std::collections::VecDeque;
use std::path::PathBuf;

pub(crate) const MAX_FILE_BYTES: usize = 131_075;
pub(crate) const MAX_TEXT_BYTES: usize = 131_072;
pub(crate) const MAX_DOCX_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
// Base64 expansion plus the longest accepted request id and response envelope.
pub(crate) const MAX_DOCX_REPLY_BYTES: usize = MAX_DOCX_PREVIEW_BYTES.div_ceil(3) * 4 + 64;
const MAX_QUEUE: usize = 4;
const MAX_REPLIES: usize = 4;
const ORIGIN: &str = "https://app.atelier/";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Open { id: String },
    OpenDocx { id: String },
    Save { id: String, bytes: Vec<u8> },
    Dirty { id: String, dirty: bool },
}

impl Request {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Open { id }
            | Self::OpenDocx { id }
            | Self::Save { id, .. }
            | Self::Dirty { id, .. } => id,
        }
    }
}

#[derive(Default)]
pub(crate) struct Queue {
    pending: VecDeque<(u128, Request)>,
    latest_dirty: Option<(u128, Request)>,
    rejects: VecDeque<String>,
    last_id: u128,
    queued_bytes: usize,
    session_dirty: bool,
    page_epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reject {
    Origin,
    Malformed,
    Replay,
    Full,
    TooLarge,
}

impl Queue {
    /// Parse, validate, and copy only. Call from WebMessageReceived; all I/O
    /// and dialogs belong to the window's STA handler.
    pub(crate) fn accept(&mut self, source: &str, message: &str) -> Result<(), Reject> {
        if source != ORIGIN {
            return Err(Reject::Origin);
        }
        if self.page_epoch == 0 {
            return Err(Reject::Origin);
        }
        if message.len() > 175_000 || !message.is_ascii() {
            return Err(Reject::TooLarge);
        }
        let mut parts = message.split('|');
        if parts.next() != Some("us120-file") || parts.next() != Some("1") {
            return Err(Reject::Malformed);
        }
        let id = parts.next().ok_or(Reject::Malformed)?;
        if id.is_empty() || id.len() > 20 || !id.bytes().all(|c| c.is_ascii_digit()) {
            return Err(Reject::Malformed);
        }
        let numeric_id = id.parse::<u128>().map_err(|_| Reject::Malformed)?;
        if numeric_id == 0 {
            return Err(Reject::Malformed);
        }
        if numeric_id <= self.last_id {
            return Err(Reject::Replay);
        }
        let operation = parts.next().ok_or(Reject::Malformed)?;
        let tail = parts.next().ok_or(Reject::Malformed)?;
        if parts.next().is_some() {
            return Err(Reject::Malformed);
        }
        let request = match operation {
            "open" if tail.is_empty() => Request::Open { id: id.to_owned() },
            "openDocx" if tail.is_empty() => Request::OpenDocx { id: id.to_owned() },
            "dirty" if tail == "0" || tail == "1" => Request::Dirty {
                id: id.to_owned(),
                dirty: tail == "1",
            },
            "save" => {
                let bytes = decode_base64(tail)?;
                if bytes.len() > MAX_FILE_BYTES {
                    return Err(Reject::TooLarge);
                }
                match text_error(&bytes) {
                    Some("files.error.invalid_text") => return Err(Reject::Malformed),
                    Some(_) => return Err(Reject::TooLarge),
                    None => {}
                }
                Request::Save {
                    id: id.to_owned(),
                    bytes,
                }
            }
            _ => return Err(Reject::Malformed),
        };
        self.last_id = numeric_id;
        if matches!(&request, Request::Dirty { .. }) {
            self.latest_dirty = Some((numeric_id, request));
            return Ok(());
        }
        let size = match &request {
            Request::Save { bytes, .. } => bytes.len(),
            _ => 0,
        };
        if self.pending.len() == MAX_QUEUE
            || self.queued_bytes.saturating_add(size) > MAX_QUEUE * MAX_FILE_BYTES
        {
            if self.rejects.len() < MAX_REPLIES {
                self.rejects
                    .push_back(format!("us120-file|1|{id}|error|files.error.queue_full"));
            }
            return Err(Reject::Full);
        }
        self.queued_bytes += size;
        self.pending.push_back((numeric_id, request));
        Ok(())
    }

    pub(crate) fn pop(&mut self) -> Option<Request> {
        let take_dirty = match (self.pending.front(), self.latest_dirty.as_ref()) {
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (Some((pending_id, _)), Some((dirty_id, _))) => dirty_id < pending_id,
            (None, None) => return None,
        };
        let request = if take_dirty {
            self.latest_dirty.take()?.1
        } else {
            self.pending.pop_front()?.1
        };
        if let Request::Save { bytes, .. } = &request {
            self.queued_bytes -= bytes.len();
        }
        if let Request::Dirty { dirty, .. } = &request {
            self.session_dirty = *dirty;
        }
        Some(request)
    }

    pub(crate) fn dirty_hint(&self) -> Option<bool> {
        match &self.latest_dirty {
            Some((_, Request::Dirty { dirty, .. })) => Some(*dirty),
            _ => None,
        }
    }

    pub(crate) fn pop_rejection(&mut self) -> Option<String> {
        self.rejects.pop_front()
    }

    pub(crate) fn has_rejections(&self) -> bool {
        !self.rejects.is_empty()
    }

    /// Accept exactly one top-level document for the lifetime of this bridge.
    /// Refusing every later reload avoids carrying dirty state or correlating
    /// a delayed response with a fresh page whose request IDs start at one.
    pub(crate) fn begin_page_navigation(&mut self, file_work_active: bool) -> bool {
        if self.page_epoch != 0 || file_work_active || self.is_dirty() || self.has_pending() {
            return false;
        }
        self.page_epoch = self.page_epoch.wrapping_add(1).max(1);
        true
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty_hint().unwrap_or(self.session_dirty)
    }

    fn has_pending(&self) -> bool {
        !self.pending.is_empty() || self.latest_dirty.is_some() || !self.rejects.is_empty()
    }
}

pub(crate) fn decode_base64(value: &str) -> Result<Vec<u8>, Reject> {
    if value.len() % 4 != 0 || value.len() > 174_768 || !value.is_ascii() {
        return Err(Reject::Malformed);
    }
    let mut out = Vec::with_capacity(value.len() / 4 * 3);
    let bytes = value.as_bytes();
    for (index, chunk) in bytes.chunks_exact(4).enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let a = b64(chunk[0]).ok_or(Reject::Malformed)? as u32;
        let b = b64(chunk[1]).ok_or(Reject::Malformed)? as u32;
        let c = if chunk[2] == b'=' {
            0
        } else {
            b64(chunk[2]).ok_or(Reject::Malformed)? as u32
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            b64(chunk[3]).ok_or(Reject::Malformed)? as u32
        };
        if chunk[2] == b'=' && (chunk[3] != b'=' || !last || b & 0x0f != 0) {
            return Err(Reject::Malformed);
        }
        if chunk[3] == b'=' && (!last || (chunk[2] != b'=' && c & 0x03 != 0)) {
            return Err(Reject::Malformed);
        }
        let n = (a << 18) | (b << 12) | (c << 6) | d;
        out.push((n >> 16) as u8);
        if chunk[2] != b'=' {
            out.push((n >> 8) as u8);
        }
        if chunk[3] != b'=' {
            out.push(n as u8);
        }
    }
    if encode_base64(&out) != value {
        return Err(Reject::Malformed);
    }
    Ok(out)
}

pub(crate) fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (a << 16) | (b << 8) | c;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Execute the broker half of a request after trusted STA code has selected a
/// path. A `None` selection is the native dialog's cancellation result.
pub(crate) fn process_selection(
    request: Request,
    selected: Option<PathBuf>,
    broker: &crate::files::FileBroker,
) -> String {
    let id = request.id().to_owned();
    let Some(path) = selected else {
        return format!("us120-file|1|{id}|cancelled|");
    };
    let result = match request {
        Request::Open { .. } => match broker.grant_read(&path, MAX_FILE_BYTES as u64) {
            Err(error) => Err(error.message_key),
            Ok(ticket) => {
                let result = broker.read(ticket);
                broker.revoke_read(ticket);
                match result {
                    Ok(bytes) => match text_error(&bytes) {
                        Some(key) => Err(key),
                        None => Ok(Some(bytes)),
                    },
                    Err(error) => Err(error.message_key),
                }
            }
        },
        Request::OpenDocx { .. } => match broker.grant_read(&path, MAX_DOCX_PREVIEW_BYTES as u64) {
            Err(error) => Err(error.message_key),
            Ok(ticket) => {
                let result = broker.read(ticket);
                broker.revoke_read(ticket);
                match result {
                    Ok(bytes) if bytes.len() <= MAX_DOCX_PREVIEW_BYTES => Ok(Some(bytes)),
                    Ok(_) => Err("files.error.too_large"),
                    Err(error) => Err(error.message_key),
                }
            }
        },
        Request::Save { bytes, .. } => {
            if let Some(key) = text_error(&bytes) {
                return format!("us120-file|1|{id}|error|{key}");
            }
            match broker.grant_write(&path) {
                Err(error) => Err(error.message_key),
                Ok(ticket) => {
                    let result = broker.write(ticket, &bytes);
                    broker.revoke_write(ticket);
                    match result {
                        Ok(()) => Ok(None),
                        Err(error) => Err(error.message_key),
                    }
                }
            }
        }
        Request::Dirty { .. } => unreachable!("dirty messages do not select a file"),
    };
    let reply = match result {
        Ok(Some(bytes)) => format!("us120-file|1|{id}|ok|{}", encode_base64(&bytes)),
        Ok(None) => format!("us120-file|1|{id}|ok|"),
        Err(key) => format!("us120-file|1|{id}|error|{key}"),
    };
    if reply.len() <= MAX_DOCX_REPLY_BYTES {
        reply
    } else {
        format!("us120-file|1|{id}|error|files.error.too_large")
    }
}

fn text_length(bytes: &[u8]) -> Option<usize> {
    let text = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    std::str::from_utf8(text).ok().map(str::len)
}

fn text_error(bytes: &[u8]) -> Option<&'static str> {
    match text_length(bytes) {
        None => Some("files.error.invalid_text"),
        Some(length) if length > MAX_TEXT_BYTES => Some("files.error.too_large"),
        Some(_) => None,
    }
}

fn b64(value: u8) -> Option<u8> {
    match value {
        b'A'..=b'Z' => Some(value - b'A'),
        b'a'..=b'z' => Some(value - b'a' + 26),
        b'0'..=b'9' => Some(value - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_base64_round_trips_and_rejects_noncanonical_forms() {
        for value in [b"".as_slice(), b"a", b"ab", b"abc", b"\xef\xbb\xbftext"] {
            assert_eq!(decode_base64(&encode_base64(value)).unwrap(), value);
        }
        assert_eq!(decode_base64("Zh=="), Err(Reject::Malformed));
        assert_eq!(decode_base64("Zg==="), Err(Reject::Malformed));
    }

    #[test]
    fn decoded_size_limit_is_enforced_at_exact_boundary() {
        let mut queue = Queue::default();
        assert!(queue.begin_page_navigation(false));
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend(vec![b'x'; MAX_TEXT_BYTES]);
        let at_limit = format!("us120-file|1|1|save|{}", encode_base64(&bytes));
        assert!(queue.accept(ORIGIN, &at_limit).is_ok());
        assert!(
            matches!(queue.pop(), Some(Request::Save { bytes: saved, .. }) if saved.len() == MAX_FILE_BYTES)
        );

        let mut queue = Queue::default();
        assert!(queue.begin_page_navigation(false));
        let too_large = vec![b'x'; MAX_TEXT_BYTES + 1];
        let over_limit = format!("us120-file|1|1|save|{}", encode_base64(&too_large));
        assert_eq!(queue.accept(ORIGIN, &over_limit), Err(Reject::TooLarge));
        assert!(queue.pop().is_none());

        let mut no_bom = Queue::default();
        assert!(no_bom.begin_page_navigation(false));
        assert!(
            no_bom
                .accept(
                    ORIGIN,
                    &format!(
                        "us120-file|1|1|save|{}",
                        encode_base64(&vec![b'x'; MAX_TEXT_BYTES])
                    )
                )
                .is_ok()
        );
    }

    #[test]
    fn open_docx_has_a_distinct_bounded_operation() {
        let mut queue = Queue::default();
        assert!(queue.begin_page_navigation(false));
        queue
            .accept(ORIGIN, "us120-file|1|1|openDocx|")
            .expect("openDocx carries no file bytes from JavaScript");
        assert_eq!(queue.pop(), Some(Request::OpenDocx { id: "1".into() }));
        assert_eq!(MAX_FILE_BYTES, 131_075, "TXT request limit remains frozen");
        assert_eq!(MAX_TEXT_BYTES, 131_072, "TXT content limit remains frozen");
        assert_eq!(MAX_DOCX_PREVIEW_BYTES, 8 * 1024 * 1024);
    }

    #[test]
    fn page_epoch_blocks_reload_after_initial_document() {
        let mut queue = Queue::default();
        assert_eq!(
            queue.accept(ORIGIN, "us120-file|1|1|open|"),
            Err(Reject::Origin)
        );
        assert!(queue.begin_page_navigation(false));
        assert!(!queue.begin_page_navigation(false));
        assert_eq!(queue.page_epoch, 1);
        assert!(!queue.begin_page_navigation(false));
        assert_eq!(queue.page_epoch, 1);
        let mut busy = Queue::default();
        assert!(!busy.begin_page_navigation(true));
        assert_eq!(busy.page_epoch, 0);

        let mut dirty = Queue::default();
        assert!(dirty.begin_page_navigation(false));
        dirty.accept(ORIGIN, "us120-file|1|1|dirty|1").unwrap();
        assert!(!dirty.begin_page_navigation(false));
        assert!(dirty.is_dirty());
    }

    #[test]
    fn ingress_enforces_origin_format_replay_and_capacity() {
        let mut queue = Queue::default();
        assert!(queue.begin_page_navigation(false));
        assert_eq!(
            queue.accept("https://other/", "us120-file|1|1|open|"),
            Err(Reject::Origin)
        );
        assert!(queue.accept(ORIGIN, "us120-file|1|1|open|").is_ok());
        assert_eq!(
            queue.accept(ORIGIN, "us120-file|1|1|open|"),
            Err(Reject::Replay)
        );
        assert_eq!(
            queue.accept(ORIGIN, "us120-file|1|2|open|extra"),
            Err(Reject::Malformed)
        );
        assert_eq!(
            queue.accept(
                ORIGIN,
                &format!("us120-file|1|2|save|{}", encode_base64(&[0xff]))
            ),
            Err(Reject::Malformed)
        );
        for id in 2..=4 {
            queue
                .accept(ORIGIN, &format!("us120-file|1|{id}|open|"))
                .unwrap();
        }
        assert_eq!(
            queue.accept(ORIGIN, "us120-file|1|5|open|"),
            Err(Reject::Full)
        );
        assert_eq!(
            queue.pop_rejection().as_deref(),
            Some("us120-file|1|5|error|files.error.queue_full")
        );
        queue.accept(ORIGIN, "us120-file|1|6|dirty|1").unwrap();
        queue.accept(ORIGIN, "us120-file|1|7|dirty|0").unwrap();
        assert_eq!(queue.dirty_hint(), Some(false));
        assert!(matches!(queue.pop(), Some(Request::Open { id }) if id == "1"));
        assert!(matches!(queue.pop(), Some(Request::Open { id }) if id == "2"));
        assert!(matches!(queue.pop(), Some(Request::Open { id }) if id == "3"));
        assert!(matches!(queue.pop(), Some(Request::Open { id }) if id == "4"));
        assert_eq!(
            queue.pop(),
            Some(Request::Dirty {
                id: "7".into(),
                dirty: false
            })
        );
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn selected_paths_cover_success_cancellation_errors_and_revocation() {
        use crate::files::FileBroker;
        use std::fs;
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let dir = std::env::temp_dir().join(format!(
            "atelier-file-bridge-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("note.txt");
        let broker = FileBroker::new();
        let cancelled = process_selection(Request::Open { id: "1".into() }, None, &broker);
        assert_eq!(cancelled, "us120-file|1|1|cancelled|");
        let docx_cancelled = process_selection(Request::OpenDocx { id: "6".into() }, None, &broker);
        assert_eq!(docx_cancelled, "us120-file|1|6|cancelled|");
        assert_eq!(
            process_selection(
                Request::Save {
                    id: "2".into(),
                    bytes: b"hello\r\n".to_vec()
                },
                Some(path.clone()),
                &broker
            ),
            "us120-file|1|2|ok|"
        );
        assert_eq!(fs::read(&path).unwrap(), b"hello\r\n");
        let opened = process_selection(
            Request::Open { id: "3".into() },
            Some(path.clone()),
            &broker,
        );
        assert_eq!(
            opened,
            format!("us120-file|1|3|ok|{}", encode_base64(b"hello\r\n"))
        );
        let missing = process_selection(
            Request::Open { id: "4".into() },
            Some(dir.join("missing.txt")),
            &broker,
        );
        assert!(missing.starts_with("us120-file|1|4|error|files.error."));
        let oversized_text = dir.join("oversized.txt");
        fs::write(&oversized_text, vec![b'x'; MAX_TEXT_BYTES + 1]).unwrap();
        assert_eq!(
            process_selection(
                Request::Open { id: "5".into() },
                Some(oversized_text),
                &broker
            ),
            "us120-file|1|5|error|files.error.too_large"
        );

        let docx_path = dir.join("preview.docx");
        let docx_bytes = vec![0x5a; MAX_FILE_BYTES + 17];
        fs::write(&docx_path, &docx_bytes).unwrap();
        let docx_reply = process_selection(
            Request::OpenDocx { id: "7".into() },
            Some(docx_path),
            &broker,
        );
        assert_eq!(
            docx_reply,
            format!("us120-file|1|7|ok|{}", encode_base64(&docx_bytes))
        );

        let oversized_docx_path = dir.join("oversized.docx");
        fs::write(&oversized_docx_path, vec![0x5a; MAX_DOCX_PREVIEW_BYTES + 1]).unwrap();
        assert_eq!(
            process_selection(
                Request::OpenDocx { id: "8".into() },
                Some(oversized_docx_path),
                &broker,
            ),
            "us120-file|1|8|error|files.error.too_large"
        );
        let ticket = broker.grant_read(&path, MAX_FILE_BYTES as u64).unwrap();
        assert!(broker.revoke_read(ticket));
        assert_eq!(
            broker.read(ticket).unwrap_err().message_key,
            "files.error.unknown_ticket"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
