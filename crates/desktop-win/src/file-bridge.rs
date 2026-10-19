// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded native file-message protocol shared by the WebView callback and STA.

use std::collections::VecDeque;
use std::path::PathBuf;

use crate::bridge_ops::{self, DialogSpec, Outcome};

/// Limite des opérations historiques « octets dans le message » (`open`, `save`) : ancien éditeur v1.
/// Plus atteintes par l'interface v2 (dialogue + tickets) ; ne pas s'en servir pour l'ouverture.
pub(crate) const MAX_FILE_BYTES: usize = 131_075;
/// Taille maximale d'un fichier d'ouverture (US-115) : `MAX_OPEN_BYTES` du moteur v2
/// (`MAX_FRAME_BYTES - HEADER_BYTES - 8`, `rich_protocol.rs`), valeur recopiée ici car ce crate
/// n'a aucune dépendance ; un test la compare au source. Côté JS : `RICH_LIMITS.maxOpenBytes`.
pub(crate) const MAX_LAUNCH_BYTES: usize = 32 * 1024 * 1024 - 20 - 8;
pub(crate) const MAX_TEXT_BYTES: usize = 131_072;
pub(crate) const MAX_DOCX_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
// Base64 expansion plus the longest accepted request id and response envelope.
pub(crate) const MAX_DOCX_REPLY_BYTES: usize = MAX_DOCX_PREVIEW_BYTES.div_ceil(3) * 4 + 64;
const MAX_QUEUE: usize = 4;
const MAX_REPLIES: usize = 4;
const ORIGIN: &str = "https://app.atelier/";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Open {
        id: String,
    },
    OpenDocx {
        id: String,
    },
    Save {
        id: String,
        bytes: Vec<u8>,
    },
    /// US-115 : le front réclame le prochain document demandé par la ligne de commande
    /// ou par une seconde instance. Aucun chemin ne vient de JavaScript.
    Launch {
        id: String,
    },
    Dirty {
        id: String,
        dirty: bool,
    },
    /// US-113 : dialogue d'ouverture par formats ; remet un ticket de lecture opaque.
    PickOpen {
        id: String,
        spec: DialogSpec,
    },
    /// US-113 : dialogue d'enregistrement par formats ; remet un ticket d'écriture opaque.
    PickSave {
        id: String,
        spec: DialogSpec,
    },
    /// US-113 : lecture unique d'un ticket `r<n>` remis par `PickOpen`.
    ReadTicket {
        id: String,
        ticket: u64,
    },
    /// US-113 : abandon d'un ticket `r<n>` ou `w<n>`.
    ReleaseTicket {
        id: String,
        ticket: u64,
        write: bool,
    },
    /// US-113 : morceau d'écriture atomique sur un ticket `w<n>` ; le dernier publie le fichier.
    WriteTicket {
        id: String,
        ticket: u64,
        seq: u32,
        last: bool,
        bytes: Vec<u8>,
    },
    ListFonts {
        id: String,
    },
    ReadFont {
        id: String,
        font: String,
    },
    OpenUrl {
        id: String,
        url: String,
    },
    /// US-320 : liste des documents récents (chemin, date, épinglage, présence sur le disque).
    RecentList {
        id: String,
    },
    /// US-320 : ouvre un document récent par son numéro de session ; remet un ticket de lecture.
    RecentOpen {
        id: String,
        recent: u64,
    },
    /// US-320 : épingle ou désépingle un document récent.
    RecentPin {
        id: String,
        recent: u64,
        pinned: bool,
    },
    /// US-320 : le front confirme qu'un document lu par un ticket `r<n>` a été accepté : il entre dans les récents.
    RecentNote {
        id: String,
        ticket: u64,
    },
    /// US-320 : retire un document de la liste (le fichier n'est jamais touché).
    RecentRemove {
        id: String,
        recent: u64,
    },
}

impl Request {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Open { id }
            | Self::OpenDocx { id }
            | Self::Save { id, .. }
            | Self::Launch { id }
            | Self::PickOpen { id, .. }
            | Self::PickSave { id, .. }
            | Self::ReadTicket { id, .. }
            | Self::ReleaseTicket { id, .. }
            | Self::WriteTicket { id, .. }
            | Self::ListFonts { id }
            | Self::ReadFont { id, .. }
            | Self::OpenUrl { id, .. }
            | Self::RecentList { id }
            | Self::RecentOpen { id, .. }
            | Self::RecentPin { id, .. }
            | Self::RecentRemove { id, .. }
            | Self::RecentNote { id, .. }
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
        let request = match parse_extended(operation, tail, id) {
            Some(Ok(request)) => request,
            Some(Err(key)) => {
                // Argument invalide d'une opération US-113 : l'identifiant est consommé et le
                // script reçoit une erreur traduisible plutôt qu'un silence.
                self.last_id = numeric_id;
                if self.rejects.len() < MAX_REPLIES {
                    self.rejects
                        .push_back(format!("us120-file|1|{id}|error|{key}"));
                }
                return Err(Reject::Malformed);
            }
            None => match operation {
                "open" if tail.is_empty() => Request::Open { id: id.to_owned() },
                "openDocx" if tail.is_empty() => Request::OpenDocx { id: id.to_owned() },
                "launch" if tail.is_empty() => Request::Launch { id: id.to_owned() },
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
            },
        };
        self.last_id = numeric_id;
        if matches!(&request, Request::Dirty { .. }) {
            self.latest_dirty = Some((numeric_id, request));
            return Ok(());
        }
        let size = match &request {
            Request::Save { bytes, .. } | Request::WriteTicket { bytes, .. } => bytes.len(),
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
        if let Request::Save { bytes, .. } | Request::WriteTicket { bytes, .. } = &request {
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
        _ => {
            unreachable!("only open, openDocx and save select a file here")
        }
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

/// Résultat de la lecture d'un fichier d'ouverture : (type, octets) ou clé d'erreur.
pub(crate) type LaunchRead = Result<(&'static str, Vec<u8>), &'static str>;

/// US-115 : lit un fichier d'ouverture (`.txt` ou `.docx`). Peut bloquer (partage réseau lent) :
/// à appeler hors du fil UI.
pub(crate) fn read_launch_file(path: &std::path::Path) -> LaunchRead {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    let (kind, limit) = match extension.as_deref() {
        Some("txt") => ("txt", MAX_LAUNCH_BYTES),
        Some("docx") => ("docx", MAX_LAUNCH_BYTES),
        _ => return Err("files.error.unsupported_type"),
    };
    let broker = crate::files::FileBroker::new();
    let ticket = broker
        .grant_read(path, limit as u64)
        .map_err(|error| error.message_key)?;
    let result = broker.read(ticket);
    broker.revoke_read(ticket);
    match result {
        // Aucun contrôle d'encodage : le moteur v2 détecte UTF-8, UTF-16 ou Windows-1252 comme pour le dialogue.
        Ok(bytes) if bytes.len() <= limit => Ok((kind, bytes)),
        Ok(_) => Err("files.error.too_large"),
        Err(error) => Err(error.message_key),
    }
}

/// Identité du fichier d'ouverture pour le front (titre de l'onglet, fichier déjà ouvert) : le chemin canonique
/// (voir `bridge_ops::canonical_identity`) s'il tient en 1 Kio, sinon le nom seul, sinon rien.
fn launch_identity(path: &std::path::Path) -> String {
    bridge_ops::identity_limited(path)
}

/// Réponse du pont pour un fichier d'ouverture lu : `ok|<txt|docx>;<restants>;<identité base64>;<base64>` en
/// ligne, ou `ok|<txt|docx>;<restants>;<identité base64>;@sb` quand les octets partent par SharedBuffer (repli :
/// la forme en ligne). `<restants>` compte les chemins encore en attente : ils ne sont PAS écartés (US-327, un
/// onglet par fichier : le front les réclame un par un). Une erreur est `error|<clé>;<identité base64>` ; les chemins
/// suivants restent en attente dans les deux cas.
pub(crate) fn launch_reply(
    id: &str,
    path: &std::path::Path,
    read: LaunchRead,
    pending: &mut VecDeque<PathBuf>,
) -> Outcome {
    let who = encode_base64(launch_identity(path).as_bytes());
    match read {
        Ok((kind, bytes)) => {
            let remaining = pending.len();
            let prefix = format!("{kind};{remaining};{who};");
            bridge_ops::bytes_outcome(id, &prefix, &format!("{prefix}@sb"), &prefix, bytes)
        }
        Err(key) => Outcome::Text(format!("us120-file|1|{id}|error|{key};{who}")),
    }
}

/// Version synchrone (tests) : sert le prochain chemin en attente ; file vide : `ok|`.
#[cfg(test)]
pub(crate) fn process_launch(id: &str, pending: &mut VecDeque<PathBuf>) -> String {
    let Some(path) = pending.pop_front() else {
        return format!("us120-file|1|{id}|ok|");
    };
    launch_reply(id, &path, read_launch_file(&path), pending)
        .text()
        .to_owned()
}

/// US-113 : analyse des opérations étendues ; `None` si l'opération n'en fait pas partie.
fn parse_extended(operation: &str, tail: &str, id: &str) -> Option<Result<Request, &'static str>> {
    const BAD: &str = "files.error.invalid_request";
    let id = id.to_owned();
    Some(match operation {
        "pickOpen" => {
            bridge_ops::parse_dialog_spec(tail, false).map(|spec| Request::PickOpen { id, spec })
        }
        "pickSave" => {
            bridge_ops::parse_dialog_spec(tail, true).map(|spec| Request::PickSave { id, spec })
        }
        "readTicket" => match bridge_ops::parse_ticket(tail) {
            Some((false, ticket)) => Ok(Request::ReadTicket { id, ticket }),
            _ => Err(BAD),
        },
        "releaseTicket" => match bridge_ops::parse_ticket(tail) {
            Some((write, ticket)) => Ok(Request::ReleaseTicket { id, ticket, write }),
            None => Err(BAD),
        },
        "writeTicket" => parse_write(tail, id),
        "listFonts" if tail.is_empty() => Ok(Request::ListFonts { id }),
        "listFonts" => Err(BAD),
        "readFont" if bridge_ops::valid_font_id(tail) => Ok(Request::ReadFont {
            id,
            font: tail.to_owned(),
        }),
        "readFont" => Err(BAD),
        "recentList" if tail.is_empty() => Ok(Request::RecentList { id }),
        "recentList" => Err(BAD),
        "recentOpen" => parse_recent_id(tail)
            .map(|recent| Request::RecentOpen { id, recent })
            .ok_or(BAD),
        "recentNote" => match bridge_ops::parse_ticket(tail) {
            Some((false, ticket)) => Ok(Request::RecentNote { id, ticket }),
            _ => Err(BAD),
        },
        "recentRemove" => parse_recent_id(tail)
            .map(|recent| Request::RecentRemove { id, recent })
            .ok_or(BAD),
        "recentPin" => {
            let (number, flag) = tail.split_once(';').unwrap_or(("", ""));
            match (parse_recent_id(number), flag) {
                (Some(recent), "0" | "1") => Ok(Request::RecentPin {
                    id,
                    recent,
                    pinned: flag == "1",
                }),
                _ => Err(BAD),
            }
        }
        "openUrl" => match decode_base64(tail) {
            Ok(bytes) if bytes.len() > crate::url_open::MAX_URL_BYTES => Err("url.error.too_long"),
            Ok(bytes) => String::from_utf8(bytes)
                .map(|url| Request::OpenUrl { id, url })
                .map_err(|_| "url.error.invalid"),
            Err(_) => Err("url.error.invalid"),
        },
        _ => return None,
    })
}

/// Numéro de document récent : 1 à 10 chiffres, non nul.
fn parse_recent_id(text: &str) -> Option<u64> {
    if text.is_empty() || text.len() > 10 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok().filter(|number| *number != 0)
}

/// `w<n>;<seq>;<0|1>;<base64>` : un morceau d'écriture.
fn parse_write(tail: &str, id: String) -> Result<Request, &'static str> {
    const BAD: &str = "files.error.invalid_request";
    let mut fields = tail.split(';');
    let (Some(ticket), Some(seq), Some(last), Some(data), None) = (
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
    ) else {
        return Err(BAD);
    };
    let Some((true, ticket)) = bridge_ops::parse_ticket(ticket) else {
        return Err(BAD);
    };
    if seq.is_empty() || seq.len() > 9 || !seq.bytes().all(|b| b.is_ascii_digit()) {
        return Err(BAD);
    }
    let last = match last {
        "0" => false,
        "1" => true,
        _ => return Err(BAD),
    };
    let bytes = decode_base64(data).map_err(|_| BAD)?;
    if bytes.len() > bridge_ops::MAX_CHUNK_BYTES {
        return Err("files.error.too_large");
    }
    Ok(Request::WriteTicket {
        id,
        ticket,
        seq: seq.parse().map_err(|_| BAD)?,
        last,
        bytes,
    })
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
        assert_eq!(
            MAX_FILE_BYTES, 131_075,
            "legacy v1 request limit remains frozen"
        );
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

    #[test]
    fn launch_is_an_id_checked_operation_without_payload() {
        let mut queue = Queue::default();
        assert!(queue.begin_page_navigation(false));
        queue.accept(ORIGIN, "us120-file|1|1|launch|").unwrap();
        assert_eq!(queue.pop(), Some(Request::Launch { id: "1".into() }));
        assert_eq!(
            queue.accept(ORIGIN, "us120-file|1|2|launch|C:%5Ca.txt"),
            Err(Reject::Malformed)
        );
        assert_eq!(
            queue.accept(ORIGIN, "us120-file|1|1|launch|"),
            Err(Reject::Replay)
        );
    }

    fn launch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atelier-us115-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn launch_serves_first_file_and_counts_the_skipped_ones() {
        let dir = launch_dir("serve");
        let a = dir.join("a.TXT");
        let b = dir.join("b.txt");
        std::fs::write(&a, "bonjour").unwrap();
        std::fs::write(&b, "autre").unwrap();
        let mut pending: VecDeque<PathBuf> = [a.clone(), b.clone()].into();
        let reply = process_launch("7", &mut pending);
        assert_eq!(
            reply,
            format!(
                "us120-file|1|7|ok|txt;1;{};{}",
                encode_base64(bridge_ops::canonical_identity(&a).as_bytes()),
                encode_base64(b"bonjour")
            )
        );
        // US-327 : le second fichier n'est pas écarté, il est servi à la réclamation suivante.
        assert_eq!(pending.len(), 1);
        assert_eq!(
            process_launch("8", &mut pending),
            format!(
                "us120-file|1|8|ok|txt;0;{};{}",
                encode_base64(bridge_ops::canonical_identity(&b).as_bytes()),
                encode_base64(b"autre")
            )
        );
        assert_eq!(process_launch("9", &mut pending), "us120-file|1|9|ok|");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn launch_errors_keep_the_following_files_pending() {
        let dir = launch_dir("errors");
        let image = dir.join("x.png");
        let missing = dir.join("absent.txt");
        let good = dir.join("good.docx");
        std::fs::write(&image, "x").unwrap();
        std::fs::write(&good, "PK").unwrap();
        let mut pending: VecDeque<PathBuf> = [image.clone(), missing, good.clone()].into();
        assert_eq!(
            process_launch("1", &mut pending),
            format!(
                "us120-file|1|1|error|files.error.unsupported_type;{}",
                encode_base64(bridge_ops::canonical_identity(&image).as_bytes())
            )
        );
        assert!(process_launch("2", &mut pending).starts_with("us120-file|1|2|error|files.error."));
        assert_eq!(
            process_launch("3", &mut pending),
            format!(
                "us120-file|1|3|ok|docx;0;{};{}",
                encode_base64(bridge_ops::canonical_identity(&good).as_bytes()),
                encode_base64(b"PK")
            )
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn launch_limit_is_the_v2_engine_open_limit() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../core-wasm/src/rich_protocol.rs"),
        )
        .unwrap();
        // Évalue `pub const NAME: usize = a * b - NAME2 - 8;` (produits, + et -, noms de constantes).
        fn constant(source: &str, name: &str) -> usize {
            let marker = format!("pub const {name}: usize = ");
            let start = source.find(&marker).expect(name) + marker.len();
            let end = start + source[start..].find(';').unwrap();
            let mut total = 0i64;
            let mut sign = 1i64;
            let mut term = String::new();
            let flush = |term: &mut String, sign: i64, total: &mut i64| {
                let product: i64 = term
                    .split('*')
                    .map(|factor| match factor.trim().parse::<i64>() {
                        Ok(number) => number,
                        Err(_) => constant(source, factor.trim()) as i64,
                    })
                    .product();
                *total += sign * product;
                term.clear();
            };
            for c in source[start..end].chars() {
                if c == '-' || c == '+' {
                    flush(&mut term, sign, &mut total);
                    sign = if c == '-' { -1 } else { 1 };
                } else {
                    term.push(c);
                }
            }
            flush(&mut term, sign, &mut total);
            usize::try_from(total).unwrap()
        }
        assert_eq!(MAX_LAUNCH_BYTES, constant(&source, "MAX_OPEN_BYTES"));
    }

    #[test]
    fn launch_delivers_a_one_mebibyte_file_whole_and_refuses_one_past_the_limit() {
        let dir = launch_dir("large");
        let big = dir.join("big.txt");
        let content = vec![b'x'; 1024 * 1024];
        std::fs::write(&big, &content).unwrap();
        let mut pending: VecDeque<PathBuf> = [big.clone()].into();
        let path = pending.pop_front().unwrap();
        match launch_reply("1", &path, read_launch_file(&path), &mut pending) {
            Outcome::Shared { reply, bytes, .. } => {
                assert_eq!(
                    reply,
                    format!(
                        "us120-file|1|1|ok|txt;0;{};@sb",
                        encode_base64(bridge_ops::canonical_identity(&big).as_bytes())
                    )
                );
                assert_eq!(bytes, content);
            }
            _ => panic!("a 1 MiB file must travel by SharedBuffer"),
        }
        let at_limit = dir.join("limit.txt");
        std::fs::write(&at_limit, vec![b'y'; MAX_LAUNCH_BYTES]).unwrap();
        assert!(
            matches!(read_launch_file(&at_limit), Ok(("txt", b)) if b.len() == MAX_LAUNCH_BYTES)
        );
        let over = dir.join("over.txt");
        std::fs::write(&over, vec![b'y'; MAX_LAUNCH_BYTES + 1]).unwrap();
        assert_eq!(
            read_launch_file(&over).unwrap_err(),
            "files.error.too_large"
        );
        let over_docx = dir.join("over.docx");
        std::fs::write(&over_docx, vec![b'y'; MAX_LAUNCH_BYTES + 1]).unwrap();
        assert_eq!(
            read_launch_file(&over_docx).unwrap_err(),
            "files.error.too_large"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn launch_identity_is_canonical_and_bounded_to_one_kibibyte() {
        let dir = launch_dir("identity");
        let file = dir.join("Casse.TXT");
        std::fs::write(&file, "x").unwrap();
        // Même fichier, écrit autrement (`.` et `..`, casse du dossier) : une seule identité canonique.
        let detour = dir.join(".").join("sous").join("..").join("Casse.TXT");
        std::fs::create_dir_all(dir.join("sous")).unwrap();
        assert_eq!(
            launch_identity(&file).to_lowercase(),
            launch_identity(&detour).to_lowercase()
        );
        assert!(!launch_identity(&file).starts_with(r"\\?\"));
        // Chemin de plus de 1 Kio (fichier inexistant : le chemin reste tel quel) : le nom seul est envoyé.
        let deep = std::iter::repeat_n("d".repeat(100), 12)
            .fold(dir.clone(), |path, part| path.join(part))
            .join("fichier.txt");
        assert!(deep.to_string_lossy().len() > 1024);
        assert_eq!(launch_identity(&deep), "fichier.txt");
        // Même le nom ne tient pas : rien n'est envoyé, jamais une chaîne tronquée.
        let long_name = dir.join("e".repeat(1100));
        assert_eq!(bridge_ops::identity_limited(&long_name), "");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn launch_replies_carry_the_identity_even_when_it_is_not_valid_base64_material() {
        // Le champ est toujours du Base64 valide, quelle que soit la forme du nom (accents, espaces, `;`).
        let dir = launch_dir("odd-names");
        let odd = dir.join("é ; a.txt");
        std::fs::write(&odd, "x").unwrap();
        let mut pending: VecDeque<PathBuf> = [odd.clone()].into();
        let reply = process_launch("3", &mut pending);
        let fields: Vec<&str> = reply.split('|').collect();
        let payload: Vec<&str> = fields[4].split(';').collect();
        assert_eq!(payload.len(), 4, "type;restants;identité;octets : {reply}");
        assert!(
            payload[2]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_path_forwarded_while_a_file_is_being_opened_is_kept_in_order() {
        // WM_COPYDATA pendant une ouverture : le chemin arrive dans la file pendant que le front ouvre le précédent.
        let dir = launch_dir("during-open");
        let names = ["a.txt", "b.txt", "c.txt"];
        for name in names {
            std::fs::write(dir.join(name), name).unwrap();
        }
        let mut pending: VecDeque<PathBuf> = [dir.join("a.txt"), dir.join("b.txt")].into();
        assert!(process_launch("1", &mut pending).contains("|ok|txt;1;"));
        pending.push_back(dir.join("c.txt"));
        let second = process_launch("2", &mut pending);
        assert!(
            second.contains("|ok|txt;1;"),
            "c.txt attend encore : {second}"
        );
        let third = process_launch("3", &mut pending);
        assert!(third.contains("|ok|txt;0;"), "{third}");
        assert!(third.ends_with(&encode_base64(b"c.txt")));
        assert_eq!(process_launch("4", &mut pending), "us120-file|1|4|ok|");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn launch_delivers_utf16_and_windows_1252_text_unchanged() {
        let dir = launch_dir("encodings");
        let mut utf16 = vec![0xff, 0xfe];
        for unit in "Été à Paris".encode_utf16() {
            utf16.extend(unit.to_le_bytes());
        }
        let ansi = b"caf\xe9".to_vec();
        for (name, bytes) in [("u16.txt", utf16), ("ansi.txt", ansi)] {
            let path = dir.join(name);
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(read_launch_file(&path), Ok(("txt", bytes)));
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
