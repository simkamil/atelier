// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-113 : opérations du pont natif ↔ JS au-delà du protocole texte historique (`us120-file`) :
//! dialogues d'ouverture et d'enregistrement par formats, lecture et écriture par tickets opaques,
//! polices système, ouverture d'URL. Tout le travail ici s'exécute sur la STA de la fenêtre ; le
//! callback WebView2 ne fait que valider et copier (`file_bridge::Queue`).
//!
//! Le JS ne fournit jamais de chemin : il reçoit des tickets `r<n>` (lecture) et `w<n>` (écriture)
//! délivrés par le courtier de fichiers après un choix de l'utilisateur dans un dialogue natif.

use crate::file_bridge::{MAX_DOCX_PREVIEW_BYTES, Request, encode_base64};
use crate::file_dialog::{DialogOwner, FileFilter, StaFileDialogs};
use crate::files::{FileBroker, FileError, ReadTicket, WriteTicket};
use crate::fonts::FontCatalog;
use crate::webview::{SharedBufferError, WebView};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Au-delà, une lecture part par SharedBuffer ; en deçà, en Base64 dans le message.
pub(crate) const INLINE_MAX_BYTES: usize = 64 * 1024;
/// Taille maximale lisible via un ticket d'ouverture.
pub(crate) const TICKET_READ_LIMIT: u64 = 256 * 1024 * 1024;
/// Taille maximale d'un fichier écrit par morceaux.
pub(crate) const MAX_WRITE_BYTES: usize = 64 * 1024 * 1024;
/// Taille maximale d'un morceau d'écriture (avant Base64).
pub(crate) const MAX_CHUNK_BYTES: usize = 131_072;
const MAX_OPEN_TICKETS: usize = 8;
const MAX_WRITE_TICKETS: usize = 4;
const MAX_SPEC_BYTES: usize = 4096;
const MAX_FILTERS: usize = 8;
const MAX_TITLE_CHARS: usize = 120;
const MAX_LABEL_CHARS: usize = 80;
const MAX_NAME_CHARS: usize = 120;

/// Formats connus du natif (identifiant JS, extension). Le moteur décide lesquels il lit ou écrit :
/// le JS ne transmet que le sous-ensemble utile.
pub(crate) const FORMATS: [(&str, &str); 6] = [
    ("txt", "txt"),
    ("docx", "docx"),
    ("docm", "docm"),
    ("dotx", "dotx"),
    ("odt", "odt"),
    ("doc", "doc"),
];

const INVALID_REQUEST: &str = "files.error.invalid_request";

/// Résultat d'une opération : message texte, ou octets destinés à un SharedBuffer. Si le runtime
/// ne fournit pas SharedBuffer, `fallback_text` donne le message de repli (Base64 jusqu'à 8 Mio,
/// sinon erreur traduisible), calculé seulement à ce moment-là.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Text(String),
    Shared {
        id: String,
        reply: String,
        bytes: Vec<u8>,
        fallback_prefix: String,
    },
}

impl Outcome {
    /// Message texte du résultat, ou le repli pour un résultat par SharedBuffer (repli, tests).
    pub(crate) fn fallback_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Shared {
                id,
                bytes,
                fallback_prefix,
                ..
            } => {
                if bytes.len() <= MAX_DOCX_PREVIEW_BYTES {
                    format!(
                        "us120-file|1|{id}|ok|{fallback_prefix}{}",
                        encode_base64(bytes)
                    )
                } else {
                    format!("us120-file|1|{id}|error|files.error.shared_buffer_unavailable")
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn text(&self) -> String {
        self.fallback_text()
    }
}

fn error_reply(id: &str, key: &str) -> Outcome {
    Outcome::Text(format!("us120-file|1|{id}|error|{key}"))
}

fn ok_reply(id: &str, payload: &str) -> Outcome {
    Outcome::Text(format!("us120-file|1|{id}|ok|{payload}"))
}

/// Octets à livrer : en ligne si petits, sinon SharedBuffer avec repli Base64 borné.
/// `inline_prefix`/`fallback_prefix` précèdent les données Base64 ; `shared_payload` est la charge
/// utile annoncée avec le tampon.
pub(crate) fn bytes_outcome(
    id: &str,
    inline_prefix: &str,
    shared_payload: &str,
    fallback_prefix: &str,
    bytes: Vec<u8>,
) -> Outcome {
    if bytes.len() <= INLINE_MAX_BYTES {
        return ok_reply(id, &format!("{inline_prefix}{}", encode_base64(&bytes)));
    }
    Outcome::Shared {
        id: id.to_owned(),
        reply: format!("us120-file|1|{id}|ok|{shared_payload}"),
        bytes,
        fallback_prefix: fallback_prefix.to_owned(),
    }
}

fn shared_payload(size: usize) -> String {
    format!("sb;{size}")
}

/// Spécification d'un dialogue : titre, filtres (format, libellé) dans l'ordre d'affichage et, pour
/// l'enregistrement, un nom suggéré. Les libellés viennent du catalogue i18n du JS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DialogSpec {
    pub(crate) title: String,
    pub(crate) filters: Vec<(String, String)>,
    pub(crate) suggested_name: Option<String>,
}

fn plain_text(text: &str, max_chars: usize) -> bool {
    !text.is_empty()
        && text.chars().count() <= max_chars
        && !text.chars().any(char::is_control)
        && text.trim() == text
}

fn valid_file_name(name: &str) -> bool {
    plain_text(name, MAX_NAME_CHARS)
        && !name.ends_with('.')
        && name != ".."
        && !name
            .chars()
            .any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/// Décode la spécification (Base64 d'un texte UTF-8) :
/// ```text
/// <titre>
/// filter:<format>=<libellé>      (1 à 8 lignes, formats connus et distincts)
/// name:<nom suggéré>             (optionnel, enregistrement seulement)
/// ```
pub(crate) fn parse_dialog_spec(encoded: &str, save: bool) -> Result<DialogSpec, &'static str> {
    let raw = crate::file_bridge::decode_base64(encoded).map_err(|_| INVALID_REQUEST)?;
    if raw.len() > MAX_SPEC_BYTES {
        return Err(INVALID_REQUEST);
    }
    let text = String::from_utf8(raw).map_err(|_| INVALID_REQUEST)?;
    let mut lines = text.split('\n');
    let title = lines.next().ok_or(INVALID_REQUEST)?;
    if !plain_text(title, MAX_TITLE_CHARS) {
        return Err(INVALID_REQUEST);
    }
    let mut filters: Vec<(String, String)> = Vec::new();
    let mut suggested_name = None;
    for line in lines {
        if let Some(rest) = line.strip_prefix("filter:") {
            let (format, label) = rest.split_once('=').ok_or(INVALID_REQUEST)?;
            if !FORMATS.iter().any(|(known, _)| *known == format)
                || filters.iter().any(|(existing, _)| existing == format)
                || filters.len() == MAX_FILTERS
                || !plain_text(label, MAX_LABEL_CHARS)
            {
                return Err(INVALID_REQUEST);
            }
            filters.push((format.to_owned(), label.to_owned()));
        } else if let Some(name) = line.strip_prefix("name:") {
            if !save || suggested_name.is_some() || !valid_file_name(name) {
                return Err(INVALID_REQUEST);
            }
            suggested_name = Some(name.to_owned());
        } else {
            return Err(INVALID_REQUEST);
        }
    }
    if filters.is_empty() {
        return Err(INVALID_REQUEST);
    }
    Ok(DialogSpec {
        title: title.to_owned(),
        filters,
        suggested_name,
    })
}

fn extension_of(format: &str) -> &'static str {
    FORMATS
        .iter()
        .find(|(known, _)| *known == format)
        .map_or("", |(_, extension)| extension)
}

/// Filtres natifs et index du filtre sélectionné (celui du nom suggéré, sinon le premier).
pub(crate) fn dialog_filters(spec: &DialogSpec) -> Result<(Vec<FileFilter>, usize), FileError> {
    let filters = spec
        .filters
        .iter()
        .map(|(format, label)| FileFilter::new(label.clone(), [extension_of(format)]))
        .collect::<Result<Vec<_>, _>>()?;
    let selected = spec
        .suggested_name
        .as_deref()
        .and_then(|name| Path::new(name).extension())
        .and_then(|extension| {
            let extension = extension.to_string_lossy().to_ascii_lowercase();
            spec.filters
                .iter()
                .position(|(format, _)| extension_of(format) == extension)
        })
        .unwrap_or(0);
    Ok((filters, selected))
}

/// Format de la spécification correspondant à l'extension du chemin choisi.
pub(crate) fn format_for_path<'a>(spec: &'a DialogSpec, path: &Path) -> Option<&'a str> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    spec.filters
        .iter()
        .map(|(format, _)| format.as_str())
        .find(|format| extension_of(format) == extension)
}

/// `r<n>` ou `w<n>` : (écriture ?, numéro). Au plus 20 chiffres, non nul.
pub(crate) fn parse_ticket(text: &str) -> Option<(bool, u64)> {
    let write = match text.as_bytes().first()? {
        b'r' => false,
        b'w' => true,
        _ => return None,
    };
    let digits = &text[1..];
    if digits.is_empty() || digits.len() > 20 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number: u64 = digits.parse().ok()?;
    (number != 0).then_some((write, number))
}

/// Contrôle d'un identifiant de police : forme seulement, la validité relève du catalogue.
pub(crate) fn valid_font_id(text: &str) -> bool {
    text.len() <= 48
        && text.starts_with('f')
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'f' | b'-'))
}

struct Staging {
    ticket: u64,
    next_seq: u32,
    bytes: Vec<u8>,
}

/// État de pont propre à la fenêtre : tickets remis au JS, écriture en cours, catalogue de polices.
pub(crate) struct BridgeState {
    read_tickets: Vec<u64>,
    write_tickets: Vec<u64>,
    staging: Option<Staging>,
    fonts: Option<FontCatalog>,
    font_dirs: Vec<PathBuf>,
}

/// Services de confiance appelés sur la STA ; injectables pour les tests sans dialogue.
pub(crate) trait BridgeHost {
    /// Dialogue d'ouverture (`save = false`) ou d'enregistrement ; `None` si annulé.
    fn choose(&mut self, save: bool, spec: &DialogSpec) -> Result<Option<PathBuf>, &'static str>;
    fn open_url(&mut self, url: &str) -> Result<(), &'static str>;
}

impl BridgeState {
    pub(crate) fn new(font_dirs: Vec<PathBuf>) -> Self {
        Self {
            read_tickets: Vec::new(),
            write_tickets: Vec::new(),
            staging: None,
            fonts: None,
            font_dirs,
        }
    }

    pub(crate) fn system() -> Self {
        Self::new(crate::fonts::system_font_dirs())
    }

    /// Nombre de tickets (lecture, écriture) encore détenus par le JS.
    #[cfg(test)]
    pub(crate) fn outstanding(&self) -> (usize, usize) {
        (self.read_tickets.len(), self.write_tickets.len())
    }

    /// Diagnostic natif sans dialogue : remet un ticket de lecture pour un fichier choisi par le code
    /// de confiance (jamais par le script), comme `PickOpen` après le choix de l'utilisateur.
    pub(crate) fn issue_read_ticket(
        &mut self,
        broker: &FileBroker,
        path: &Path,
        limit: u64,
    ) -> Result<String, FileError> {
        let ticket = broker.grant_read(path, limit)?;
        self.read_tickets.push(ticket.id());
        Ok(format!("r{}", ticket.id()))
    }

    /// Diagnostic natif sans dialogue : ticket d'écriture pour une destination choisie par le natif.
    pub(crate) fn issue_write_ticket(
        &mut self,
        broker: &FileBroker,
        path: &Path,
    ) -> Result<String, FileError> {
        let ticket = broker.grant_write(path)?;
        self.write_tickets.push(ticket.id());
        Ok(format!("w{}", ticket.id()))
    }

    fn forget_read(&mut self, ticket: u64, broker: &FileBroker) {
        self.read_tickets.retain(|held| *held != ticket);
        broker.revoke_read(ReadTicket::from_id(ticket));
    }

    fn forget_write(&mut self, ticket: u64, broker: &FileBroker) {
        self.write_tickets.retain(|held| *held != ticket);
        if self
            .staging
            .as_ref()
            .is_some_and(|staging| staging.ticket == ticket)
        {
            self.staging = None;
        }
        broker.revoke_write(WriteTicket::from_id(ticket));
    }
}

/// Exécute une requête du pont étendu. Les requêtes historiques (open, openDocx, save, dirty,
/// launch) ne passent pas ici.
pub(crate) fn process_bridge_request(
    request: Request,
    state: &mut BridgeState,
    broker: &FileBroker,
    host: &mut dyn BridgeHost,
) -> Outcome {
    match request {
        Request::PickOpen { id, spec } => pick_open(&id, &spec, state, broker, host),
        Request::PickSave { id, spec } => pick_save(&id, &spec, state, broker, host),
        Request::ReadTicket { id, ticket } => read_ticket(&id, ticket, state, broker),
        Request::ReleaseTicket { id, ticket, write } => {
            let known = if write {
                state.write_tickets.contains(&ticket)
            } else {
                state.read_tickets.contains(&ticket)
            };
            if !known {
                return error_reply(&id, "files.error.unknown_ticket");
            }
            if write {
                state.forget_write(ticket, broker);
            } else {
                state.forget_read(ticket, broker);
            }
            ok_reply(&id, "")
        }
        Request::WriteTicket {
            id,
            ticket,
            seq,
            last,
            bytes,
        } => write_ticket(&id, ticket, seq, last, bytes, state, broker),
        Request::ListFonts { id } => {
            let catalog = crate::fonts::FontCatalog::scan(&state.font_dirs, state.fonts.as_ref());
            let listing = catalog.list_tsv().into_bytes();
            state.fonts = Some(catalog);
            let size = listing.len();
            bytes_outcome(&id, "b64;", &shared_payload(size), "fb;", listing)
        }
        Request::ReadFont { id, font } => match state.fonts.as_ref() {
            None => error_reply(&id, "fonts.error.unknown_id"),
            Some(catalog) => match catalog.read(&font) {
                Ok(bytes) => {
                    let payload = shared_payload(bytes.len());
                    bytes_outcome(&id, "b64;", &payload, "fb;", bytes)
                }
                Err(key) => error_reply(&id, key),
            },
        },
        Request::OpenUrl { id, url } => match host.open_url(&url) {
            Ok(()) => ok_reply(&id, ""),
            Err(key) => error_reply(&id, key),
        },
        Request::Open { id }
        | Request::OpenDocx { id }
        | Request::Launch { id }
        | Request::Dirty { id, .. }
        | Request::Save { id, .. } => error_reply(&id, INVALID_REQUEST),
    }
}

fn pick_open(
    id: &str,
    spec: &DialogSpec,
    state: &mut BridgeState,
    broker: &FileBroker,
    host: &mut dyn BridgeHost,
) -> Outcome {
    if state.read_tickets.len() >= MAX_OPEN_TICKETS {
        return error_reply(id, "files.error.ticket_exhausted");
    }
    let path = match host.choose(false, spec) {
        Err(key) => return error_reply(id, key),
        Ok(None) => return Outcome::Text(format!("us120-file|1|{id}|cancelled|")),
        Ok(Some(path)) => path,
    };
    let Some(format) = format_for_path(spec, &path) else {
        return error_reply(id, "files.error.unsupported_format");
    };
    let ticket = match broker.grant_read(&path, TICKET_READ_LIMIT) {
        Ok(ticket) => ticket,
        Err(error) => return error_reply(id, error.message_key),
    };
    let size = std::fs::metadata(&path).map_or(0, |meta| meta.len());
    if size > TICKET_READ_LIMIT {
        broker.revoke_read(ticket);
        return error_reply(id, "files.error.too_large");
    }
    state.read_tickets.push(ticket.id());
    ok_reply(
        id,
        &format!("r{};{format};{size};{}", ticket.id(), file_name_b64(&path)),
    )
}

fn pick_save(
    id: &str,
    spec: &DialogSpec,
    state: &mut BridgeState,
    broker: &FileBroker,
    host: &mut dyn BridgeHost,
) -> Outcome {
    if state.write_tickets.len() >= MAX_WRITE_TICKETS {
        return error_reply(id, "files.error.ticket_exhausted");
    }
    let path = match host.choose(true, spec) {
        Err(key) => return error_reply(id, key),
        Ok(None) => return Outcome::Text(format!("us120-file|1|{id}|cancelled|")),
        Ok(Some(path)) => path,
    };
    let Some(format) = format_for_path(spec, &path) else {
        return error_reply(id, "files.error.unsupported_format");
    };
    match broker.grant_write(&path) {
        Err(error) => error_reply(id, error.message_key),
        Ok(ticket) => {
            state.write_tickets.push(ticket.id());
            ok_reply(
                id,
                &format!("w{};{format};{}", ticket.id(), file_name_b64(&path)),
            )
        }
    }
}

fn file_name_b64(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    encode_base64(name.as_bytes())
}

fn read_ticket(id: &str, ticket: u64, state: &mut BridgeState, broker: &FileBroker) -> Outcome {
    if !state.read_tickets.contains(&ticket) {
        return error_reply(id, "files.error.unknown_ticket");
    }
    // Un ticket de lecture sert une fois : révoqué quel que soit le résultat.
    let result = broker.read(ReadTicket::from_id(ticket));
    state.forget_read(ticket, broker);
    match result {
        Ok(bytes) => {
            let payload = shared_payload(bytes.len());
            bytes_outcome(id, "b64;", &payload, "fb;", bytes)
        }
        Err(error) => error_reply(id, error.message_key),
    }
}

fn write_ticket(
    id: &str,
    ticket: u64,
    seq: u32,
    last: bool,
    bytes: Vec<u8>,
    state: &mut BridgeState,
    broker: &FileBroker,
) -> Outcome {
    if !state.write_tickets.contains(&ticket) {
        return error_reply(id, "files.error.unknown_ticket");
    }
    let staging = match state.staging.take() {
        Some(staging) if staging.ticket != ticket => {
            state.staging = Some(staging);
            return error_reply(id, "files.error.queue_full");
        }
        Some(staging) => staging,
        None => Staging {
            ticket,
            next_seq: 0,
            bytes: Vec::new(),
        },
    };
    let mut staging = staging;
    if seq != staging.next_seq || bytes.len() > MAX_CHUNK_BYTES {
        state.forget_write(ticket, broker);
        return error_reply(id, "files.error.write_sequence");
    }
    if staging.bytes.len() + bytes.len() > MAX_WRITE_BYTES {
        state.forget_write(ticket, broker);
        return error_reply(id, "files.error.too_large");
    }
    staging.bytes.extend_from_slice(&bytes);
    staging.next_seq += 1;
    if !last {
        state.staging = Some(staging);
        return ok_reply(id, "");
    }
    // Écriture atomique par le courtier ; le ticket est consommé quel que soit le résultat.
    let result = broker.write(WriteTicket::from_id(ticket), &staging.bytes);
    state.forget_write(ticket, broker);
    match result {
        Ok(()) => ok_reply(id, ""),
        Err(error) => error_reply(id, error.message_key),
    }
}

/// Services natifs réels sur la STA : dialogues communs et ShellExecute.
pub(crate) struct StaHost<'a> {
    pub(crate) dialogs: &'a mut StaFileDialogs,
    pub(crate) owner: Option<DialogOwner>,
}

impl BridgeHost for StaHost<'_> {
    fn choose(&mut self, save: bool, spec: &DialogSpec) -> Result<Option<PathBuf>, &'static str> {
        let (filters, selected) = dialog_filters(spec).map_err(|error| error.message_key)?;
        let result = if save {
            self.dialogs.save(
                self.owner.as_ref(),
                &spec.title,
                &filters,
                selected,
                spec.suggested_name.as_deref(),
            )
        } else {
            self.dialogs
                .open(self.owner.as_ref(), &spec.title, &filters, selected)
        };
        result.map_err(|error| error.message_key)
    }

    fn open_url(&mut self, url: &str) -> Result<(), &'static str> {
        crate::url_open::open_in_browser(url)
    }
}

/// Compteurs de diagnostic : tampons postés, replis Base64 faute d'interface, échecs COM.
pub(crate) static SHARED_BUFFERS_POSTED: AtomicUsize = AtomicUsize::new(0);
pub(crate) static SHARED_BUFFER_UNAVAILABLE: AtomicUsize = AtomicUsize::new(0);
pub(crate) static SHARED_BUFFER_FAILED: AtomicUsize = AtomicUsize::new(0);
/// Dernier message d'échec COM (diagnostic seulement ; jamais envoyé au script).
pub(crate) static LAST_SHARED_BUFFER_ERROR: std::sync::Mutex<Option<String>> =
    std::sync::Mutex::new(None);

/// Envoie le résultat au script : message texte, ou SharedBuffer en lecture seule dont
/// `additionalData` porte la réponse corrélée. Si le runtime ne fournit pas SharedBuffer (ou si
/// l'envoi échoue), le message de repli part à la place et le compteur correspondant est incrémenté.
pub(crate) fn deliver(view: &WebView, outcome: Outcome) -> std::io::Result<()> {
    match outcome {
        Outcome::Text(text) => view.post_web_message_as_string(&text),
        Outcome::Shared { .. } => {
            let Outcome::Shared { reply, bytes, .. } = &outcome else {
                unreachable!("matched as shared above")
            };
            let safe = reply
                .bytes()
                .all(|b| b.is_ascii_graphic() && b != b'"' && b != 0x5c);
            if !safe {
                return view.post_web_message_as_string(&outcome.fallback_text());
            }
            let json = format!("{{\"v\":1,\"reply\":\"{reply}\"}}");
            match view.post_shared_buffer(bytes, &json) {
                Ok(()) => {
                    SHARED_BUFFERS_POSTED.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
                Err(SharedBufferError::Unavailable) => {
                    SHARED_BUFFER_UNAVAILABLE.fetch_add(1, Ordering::Relaxed);
                    view.post_web_message_as_string(&outcome.fallback_text())
                }
                Err(SharedBufferError::Failed(error)) => {
                    SHARED_BUFFER_FAILED.fetch_add(1, Ordering::Relaxed);
                    *LAST_SHARED_BUFFER_ERROR
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(error.to_string());
                    // Un échec COM sur un contenu trop gros pour le repli Base64 devient une erreur traduisible.
                    let text = match &outcome {
                        Outcome::Shared { id, bytes, .. }
                            if bytes.len() > MAX_DOCX_PREVIEW_BYTES =>
                        {
                            format!("us120-file|1|{id}|error|files.error.shared_buffer_failed")
                        }
                        _ => outcome.fallback_text(),
                    };
                    view.post_web_message_as_string(&text)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_bridge::Queue;

    const ORIGIN: &str = "https://app.atelier/";

    fn spec_b64(text: &str) -> String {
        encode_base64(text.as_bytes())
    }

    fn open_spec() -> DialogSpec {
        parse_dialog_spec(
            &spec_b64("Ouvrir\nfilter:docx=Document Word\nfilter:txt=Texte brut"),
            false,
        )
        .unwrap()
    }

    #[test]
    fn dialog_spec_accepts_known_formats_and_labels_from_the_js() {
        let spec = parse_dialog_spec(
            &spec_b64(
                "Enregistrer sous\nfilter:odt=Texte OpenDocument\nfilter:docx=Document Word (*.docx)\nname:Rapport é.docx",
            ),
            true,
        )
        .unwrap();
        assert_eq!(spec.title, "Enregistrer sous");
        assert_eq!(spec.filters.len(), 2);
        assert_eq!(spec.suggested_name.as_deref(), Some("Rapport é.docx"));
        let (filters, selected) = dialog_filters(&spec).unwrap();
        assert_eq!(filters.len(), 2);
        assert_eq!(selected, 1, "the suggested name selects the docx filter");
    }

    #[test]
    fn filters_cover_every_known_format_with_single_extensions() {
        let mut text = String::from("Ouvrir");
        for (format, _) in FORMATS {
            text.push_str(&format!("\nfilter:{format}=Format {format}"));
        }
        let spec = parse_dialog_spec(&spec_b64(&text), false).unwrap();
        let (filters, selected) = dialog_filters(&spec).unwrap();
        assert_eq!(filters.len(), 6);
        assert_eq!(selected, 0);
        for ((format, extension), filter) in FORMATS.iter().zip(&filters) {
            assert_eq!(
                *filter,
                FileFilter::new(format!("Format {format}"), [*extension]).unwrap(),
                "label from the JS catalogue, single extension per filter"
            );
        }
        for (format, extension) in FORMATS {
            assert_eq!(extension, format);
            assert_eq!(
                format_for_path(
                    &spec,
                    Path::new(&format!("C:/x/y.{}", extension.to_uppercase()))
                ),
                Some(format)
            );
        }
        assert_eq!(format_for_path(&spec, Path::new("C:/x/y.pdf")), None);
        assert_eq!(format_for_path(&spec, Path::new("C:/x/noextension")), None);
    }

    #[test]
    fn dialog_spec_rejects_unknown_duplicate_oversized_and_unsafe_input() {
        let big = "x".repeat(MAX_SPEC_BYTES);
        let cases = [
            "".to_owned(),
            "Titre".to_owned(),
            "Titre\nfilter:pdf=PDF".to_owned(),
            "Titre\nfilter:txt=A\nfilter:txt=B".to_owned(),
            "Titre\nfilter:txt=".to_owned(),
            "Titre\nfilter:txt".to_owned(),
            "Titre\nfilter:txt=A\nfoo:bar".to_owned(),
            "Titre\nfilter:txt=A\nname:x.txt".to_owned(),
            "Ti\u{0}tre\nfilter:txt=A".to_owned(),
            " Titre\nfilter:txt=A".to_owned(),
            format!("{big}\nfilter:txt=A"),
            format!("Titre\nfilter:txt={}", "é".repeat(MAX_LABEL_CHARS + 1)),
        ];
        for case in cases {
            assert_eq!(
                parse_dialog_spec(&spec_b64(&case), false),
                Err(INVALID_REQUEST),
                "{case:?}"
            );
        }
        for name in [
            "..\\x.txt",
            "a/b.txt",
            "C:x.txt",
            "a*.txt",
            "x.txt.",
            "..",
            "a\tb.txt",
            "",
        ] {
            let text = format!("T\nfilter:txt=A\nname:{name}");
            assert_eq!(
                parse_dialog_spec(&spec_b64(&text), true),
                Err(INVALID_REQUEST),
                "{name:?}"
            );
        }
        assert_eq!(
            parse_dialog_spec("!!!!", false),
            Err(INVALID_REQUEST),
            "not base64"
        );
        let nine: String = (0..9)
            .map(|index| format!("\nfilter:{}=L{index}", FORMATS[index % 6].0))
            .collect();
        assert_eq!(
            parse_dialog_spec(&spec_b64(&format!("T{nine}")), false),
            Err(INVALID_REQUEST)
        );
    }

    #[test]
    fn tickets_and_font_ids_are_strictly_formed() {
        assert_eq!(parse_ticket("r12"), Some((false, 12)));
        assert_eq!(parse_ticket("w7"), Some((true, 7)));
        for bad in [
            "",
            "r",
            "x1",
            "r0",
            "r-1",
            "r1x",
            "R1",
            "r 1",
            "r123456789012345678901",
            "w+1",
        ] {
            assert_eq!(parse_ticket(bad), None, "{bad:?}");
        }
        assert!(valid_font_id("f3-12"));
        for bad in [
            "",
            "3-12",
            "C:\\a",
            "f1-1/..",
            &format!("f{}", "1".repeat(60)),
        ] {
            assert!(!valid_font_id(bad), "{bad:?}");
        }
    }

    fn queue() -> Queue {
        let mut queue = Queue::default();
        assert!(queue.begin_page_navigation(false));
        queue
    }

    #[test]
    fn ingress_parses_the_extended_operations() {
        let mut queue = queue();
        let open = spec_b64("Ouvrir\nfilter:txt=Texte");
        let save = spec_b64("Enregistrer\nfilter:txt=Texte\nname:a.txt");
        let messages = [
            format!("us120-file|1|1|pickOpen|{open}"),
            format!("us120-file|1|2|pickSave|{save}"),
            "us120-file|1|3|readTicket|r5".to_owned(),
            "us120-file|1|4|releaseTicket|w6".to_owned(),
            format!(
                "us120-file|1|5|writeTicket|w6;0;1;{}",
                encode_base64(b"hello")
            ),
            "us120-file|1|6|listFonts|".to_owned(),
            "us120-file|1|7|readFont|f1-2".to_owned(),
            format!(
                "us120-file|1|8|openUrl|{}",
                encode_base64(b"https://example.org/")
            ),
        ];
        for message in &messages[..2] {
            assert_eq!(queue.accept(ORIGIN, message), Ok(()), "{message}");
        }
        assert!(matches!(queue.pop(), Some(Request::PickOpen { .. })));
        assert!(matches!(queue.pop(), Some(Request::PickSave { .. })));
        for message in &messages[2..5] {
            assert_eq!(queue.accept(ORIGIN, message), Ok(()), "{message}");
        }
        assert_eq!(
            queue.pop(),
            Some(Request::ReadTicket {
                id: "3".into(),
                ticket: 5
            })
        );
        assert_eq!(
            queue.pop(),
            Some(Request::ReleaseTicket {
                id: "4".into(),
                ticket: 6,
                write: true
            })
        );
        assert_eq!(
            queue.pop(),
            Some(Request::WriteTicket {
                id: "5".into(),
                ticket: 6,
                seq: 0,
                last: true,
                bytes: b"hello".to_vec()
            })
        );
        for message in &messages[5..] {
            assert_eq!(queue.accept(ORIGIN, message), Ok(()), "{message}");
        }
        assert_eq!(queue.pop(), Some(Request::ListFonts { id: "6".into() }));
        assert_eq!(
            queue.pop(),
            Some(Request::ReadFont {
                id: "7".into(),
                font: "f1-2".into()
            })
        );
        assert_eq!(
            queue.pop(),
            Some(Request::OpenUrl {
                id: "8".into(),
                url: "https://example.org/".into()
            })
        );
    }

    #[test]
    fn invalid_extended_requests_get_an_error_reply_and_consume_their_id() {
        let mut queue = queue();
        let bad = [
            "us120-file|1|1|pickOpen|%%%%",
            "us120-file|1|2|readTicket|w5",
            "us120-file|1|3|readTicket|C:\\secret.txt",
            "us120-file|1|4|writeTicket|r6;0;1;",
            "us120-file|1|5|writeTicket|w6;0;2;",
            "us120-file|1|6|writeTicket|w6;x;1;",
            "us120-file|1|7|readFont|C:\\Windows\\Fonts\\arial.ttf",
            "us120-file|1|8|openUrl|!!!!",
            "us120-file|1|9|listFonts|x",
        ];
        for message in bad {
            assert!(queue.accept(ORIGIN, message).is_err(), "{message}");
        }
        let mut replies = Vec::new();
        while let Some(reply) = queue.pop_rejection() {
            replies.push(reply);
        }
        // La file de refus est bornée : les premiers refus sont conservés.
        assert!(!replies.is_empty());
        assert!(
            replies
                .iter()
                .all(|reply| reply.contains("|error|files.error.invalid_request")),
            "{replies:?}"
        );
        assert!(queue.pop().is_none());
        // Les identifiants consommés ne peuvent pas être rejoués.
        assert!(queue.accept(ORIGIN, "us120-file|1|3|listFonts|").is_err());
        assert!(queue.accept(ORIGIN, "us120-file|1|10|listFonts|").is_ok());
    }

    #[test]
    fn write_chunks_are_bounded_at_ingress() {
        let mut queue = queue();
        let exact = vec![b'a'; MAX_CHUNK_BYTES];
        let message = format!(
            "us120-file|1|1|writeTicket|w1;0;1;{}",
            encode_base64(&exact)
        );
        assert!(message.len() < 175_000);
        assert_eq!(queue.accept(ORIGIN, &message), Ok(()));
        let over = vec![b'a'; MAX_CHUNK_BYTES + 1];
        let message = format!("us120-file|1|2|writeTicket|w1;1;1;{}", encode_base64(&over));
        assert!(queue.accept(ORIGIN, &message).is_err());
    }

    // ---- exécution contre le courtier réel, sans dialogue ----

    struct FakeHost {
        path: Option<PathBuf>,
        urls: Vec<String>,
        url_result: Result<(), &'static str>,
        dialogs: usize,
        error: Option<&'static str>,
    }

    impl FakeHost {
        fn new(path: Option<PathBuf>) -> Self {
            Self {
                path,
                urls: Vec::new(),
                url_result: Ok(()),
                dialogs: 0,
                error: None,
            }
        }
    }

    impl BridgeHost for FakeHost {
        fn choose(
            &mut self,
            _save: bool,
            _spec: &DialogSpec,
        ) -> Result<Option<PathBuf>, &'static str> {
            self.dialogs += 1;
            match self.error {
                Some(key) => Err(key),
                None => Ok(self.path.clone()),
            }
        }
        fn open_url(&mut self, url: &str) -> Result<(), &'static str> {
            self.urls.push(url.to_owned());
            self.url_result
        }
    }

    fn work_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atelier-bridge-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn text_of(outcome: &Outcome) -> String {
        outcome.text()
    }

    #[test]
    fn open_then_read_ticket_round_trips_and_ticket_is_single_use() {
        let dir = work_dir("open");
        let file = dir.join("Lettre é.docx");
        std::fs::write(&file, b"docx bytes").unwrap();
        let broker = FileBroker::new();
        let mut state = BridgeState::new(Vec::new());
        let mut host = FakeHost::new(Some(file.clone()));
        let reply = process_bridge_request(
            Request::PickOpen {
                id: "1".into(),
                spec: open_spec(),
            },
            &mut state,
            &broker,
            &mut host,
        );
        let text = text_of(&reply);
        let payload = text.strip_prefix("us120-file|1|1|ok|").expect(&text);
        let fields: Vec<&str> = payload.split(';').collect();
        assert_eq!(fields.len(), 4, "{payload}");
        assert!(fields[0].starts_with('r'));
        assert_eq!(fields[1], "docx");
        assert_eq!(fields[2], "10");
        assert_eq!(
            crate::file_bridge::decode_base64(fields[3]).unwrap(),
            "Lettre é.docx".as_bytes()
        );
        assert!(
            !text.contains(dir.to_string_lossy().as_ref()),
            "no path leaves the native side"
        );
        let (_, ticket) = parse_ticket(fields[0]).unwrap();
        let read = process_bridge_request(
            Request::ReadTicket {
                id: "2".into(),
                ticket,
            },
            &mut state,
            &broker,
            &mut host,
        );
        assert_eq!(
            read,
            Outcome::Text(format!(
                "us120-file|1|2|ok|b64;{}",
                encode_base64(b"docx bytes")
            ))
        );
        assert_eq!(state.outstanding(), (0, 0));
        let again = process_bridge_request(
            Request::ReadTicket {
                id: "3".into(),
                ticket,
            },
            &mut state,
            &broker,
            &mut host,
        );
        assert_eq!(
            text_of(&again),
            "us120-file|1|3|error|files.error.unknown_ticket"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn large_reads_use_the_shared_buffer_with_an_explicit_fallback() {
        let dir = work_dir("large");
        let file = dir.join("big.txt");
        let bytes = vec![b'z'; INLINE_MAX_BYTES + 1];
        std::fs::write(&file, &bytes).unwrap();
        let broker = FileBroker::new();
        let mut state = BridgeState::new(Vec::new());
        let mut host = FakeHost::new(Some(file));
        let reply = process_bridge_request(
            Request::PickOpen {
                id: "1".into(),
                spec: open_spec(),
            },
            &mut state,
            &broker,
            &mut host,
        );
        let ticket = text_of(&reply)
            .split('|')
            .nth(4)
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let (_, number) = parse_ticket(&ticket).unwrap();
        let outcome = process_bridge_request(
            Request::ReadTicket {
                id: "2".into(),
                ticket: number,
            },
            &mut state,
            &broker,
            &mut host,
        );
        let fallback = outcome.fallback_text();
        let Outcome::Shared {
            reply,
            bytes: shared,
            ..
        } = outcome
        else {
            panic!("a read over the inline threshold must use a shared buffer");
        };
        assert_eq!(reply, format!("us120-file|1|2|ok|sb;{}", bytes.len()));
        assert_eq!(shared, bytes);
        assert!(fallback.starts_with("us120-file|1|2|ok|fb;"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fallback_above_the_base64_limit_is_a_translatable_error() {
        let outcome = bytes_outcome(
            "9",
            "b64;",
            "sb;1",
            "fb;",
            vec![0u8; MAX_DOCX_PREVIEW_BYTES + 1],
        );
        assert!(matches!(outcome, Outcome::Shared { .. }));
        assert_eq!(
            outcome.fallback_text(),
            "us120-file|1|9|error|files.error.shared_buffer_unavailable"
        );
    }

    #[test]
    fn dialog_cancellation_errors_and_wrong_formats_hand_out_no_ticket() {
        let dir = work_dir("cancel");
        let pdf = dir.join("a.pdf");
        std::fs::write(&pdf, b"x").unwrap();
        let broker = FileBroker::new();
        let mut state = BridgeState::new(Vec::new());
        let pick = |host: &mut FakeHost, state: &mut BridgeState| {
            text_of(&process_bridge_request(
                Request::PickOpen {
                    id: "1".into(),
                    spec: open_spec(),
                },
                state,
                &broker,
                host,
            ))
        };
        assert_eq!(
            pick(&mut FakeHost::new(None), &mut state),
            "us120-file|1|1|cancelled|"
        );
        let mut failing = FakeHost::new(None);
        failing.error = Some("files.error.dialog_failed");
        assert_eq!(
            pick(&mut failing, &mut state),
            "us120-file|1|1|error|files.error.dialog_failed"
        );
        assert_eq!(
            pick(&mut FakeHost::new(Some(pdf)), &mut state),
            "us120-file|1|1|error|files.error.unsupported_format"
        );
        assert_eq!(
            pick(
                &mut FakeHost::new(Some(dir.join("missing.txt"))),
                &mut state
            ),
            "us120-file|1|1|error|files.error.not_found"
        );
        assert_eq!(state.outstanding(), (0, 0));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_tickets_are_capped() {
        let dir = work_dir("cap");
        let file = dir.join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let broker = FileBroker::new();
        let mut state = BridgeState::new(Vec::new());
        let mut host = FakeHost::new(Some(file));
        for index in 0..MAX_OPEN_TICKETS {
            let reply = process_bridge_request(
                Request::PickOpen {
                    id: index.to_string(),
                    spec: open_spec(),
                },
                &mut state,
                &broker,
                &mut host,
            );
            assert!(text_of(&reply).contains("|ok|r"), "{}", text_of(&reply));
        }
        let dialogs = host.dialogs;
        let full = process_bridge_request(
            Request::PickOpen {
                id: "99".into(),
                spec: open_spec(),
            },
            &mut state,
            &broker,
            &mut host,
        );
        assert_eq!(
            text_of(&full),
            "us120-file|1|99|error|files.error.ticket_exhausted"
        );
        assert_eq!(
            host.dialogs, dialogs,
            "no dialog is shown when the cap is hit"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn pick_save_ticket(
        dir: &Path,
        name: &str,
        state: &mut BridgeState,
        broker: &FileBroker,
    ) -> u64 {
        let mut host = FakeHost::new(Some(dir.join(name)));
        let spec = parse_dialog_spec(
            &spec_b64("Enregistrer\nfilter:txt=Texte\nfilter:docx=Word\nname:a.txt"),
            true,
        )
        .unwrap();
        let reply = text_of(&process_bridge_request(
            Request::PickSave {
                id: "1".into(),
                spec,
            },
            state,
            broker,
            &mut host,
        ));
        let payload = reply.strip_prefix("us120-file|1|1|ok|").expect(&reply);
        parse_ticket(payload.split(';').next().unwrap()).unwrap().1
    }

    fn write(
        ticket: u64,
        seq: u32,
        last: bool,
        bytes: &[u8],
        state: &mut BridgeState,
        broker: &FileBroker,
    ) -> String {
        let mut host = FakeHost::new(None);
        text_of(&process_bridge_request(
            Request::WriteTicket {
                id: "5".into(),
                ticket,
                seq,
                last,
                bytes: bytes.to_vec(),
            },
            state,
            broker,
            &mut host,
        ))
    }

    #[test]
    fn chunked_write_is_atomic_and_consumes_the_ticket() {
        let dir = work_dir("write");
        let broker = FileBroker::new();
        let mut state = BridgeState::new(Vec::new());
        let ticket = pick_save_ticket(&dir, "out.docx", &mut state, &broker);
        let target = dir.join("out.docx");
        assert_eq!(
            write(ticket, 0, false, b"first ", &mut state, &broker),
            "us120-file|1|5|ok|"
        );
        assert!(
            !target.exists(),
            "nothing is published before the last chunk"
        );
        assert_eq!(
            write(ticket, 1, true, b"second", &mut state, &broker),
            "us120-file|1|5|ok|"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"first second");
        assert_eq!(state.outstanding(), (0, 0));
        assert_eq!(
            write(ticket, 0, true, b"again", &mut state, &broker),
            "us120-file|1|5|error|files.error.unknown_ticket"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"first second");
        // Remplacement d'un fichier existant, y compris vide.
        let ticket = pick_save_ticket(&dir, "out.docx", &mut state, &broker);
        assert_eq!(
            write(ticket, 0, true, b"", &mut state, &broker),
            "us120-file|1|5|ok|"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"");
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(leftovers.len(), 1, "no temporary file remains");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn write_sequence_errors_discard_the_ticket_and_keep_the_destination() {
        let dir = work_dir("sequence");
        let broker = FileBroker::new();
        let mut state = BridgeState::new(Vec::new());
        let target = dir.join("keep.txt");
        std::fs::write(&target, b"original").unwrap();
        let ticket = pick_save_ticket(&dir, "keep.txt", &mut state, &broker);
        assert_eq!(
            write(ticket, 1, true, b"skipped", &mut state, &broker),
            "us120-file|1|5|error|files.error.write_sequence"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(state.outstanding(), (0, 0));
        let ticket = pick_save_ticket(&dir, "keep.txt", &mut state, &broker);
        assert_eq!(
            write(ticket, 0, false, b"a", &mut state, &broker),
            "us120-file|1|5|ok|"
        );
        assert_eq!(
            write(ticket, 0, true, b"b", &mut state, &broker),
            "us120-file|1|5|error|files.error.write_sequence"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        // Un seul fichier en cours d'écriture à la fois.
        let first = pick_save_ticket(&dir, "one.txt", &mut state, &broker);
        let second = pick_save_ticket(&dir, "two.txt", &mut state, &broker);
        assert_eq!(
            write(first, 0, false, b"a", &mut state, &broker),
            "us120-file|1|5|ok|"
        );
        assert_eq!(
            write(second, 0, true, b"b", &mut state, &broker),
            "us120-file|1|5|error|files.error.queue_full"
        );
        assert_eq!(
            write(first, 1, true, b"c", &mut state, &broker),
            "us120-file|1|5|ok|"
        );
        assert_eq!(std::fs::read(dir.join("one.txt")).unwrap(), b"ac");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn save_with_an_unlisted_extension_gets_no_ticket_and_release_revokes() {
        let dir = work_dir("release");
        let broker = FileBroker::new();
        let mut state = BridgeState::new(Vec::new());
        let mut host = FakeHost::new(Some(dir.join("x.exe")));
        let spec = parse_dialog_spec(&spec_b64("S\nfilter:txt=Texte"), true).unwrap();
        let reply = process_bridge_request(
            Request::PickSave {
                id: "1".into(),
                spec,
            },
            &mut state,
            &broker,
            &mut host,
        );
        assert_eq!(
            text_of(&reply),
            "us120-file|1|1|error|files.error.unsupported_format"
        );
        let ticket = pick_save_ticket(&dir, "r.txt", &mut state, &broker);
        let released = process_bridge_request(
            Request::ReleaseTicket {
                id: "6".into(),
                ticket,
                write: true,
            },
            &mut state,
            &broker,
            &mut host,
        );
        assert_eq!(text_of(&released), "us120-file|1|6|ok|");
        assert_eq!(
            write(ticket, 0, true, b"x", &mut state, &broker),
            "us120-file|1|5|error|files.error.unknown_ticket"
        );
        assert!(!dir.join("r.txt").exists());
        assert!(
            !broker.revoke_write(WriteTicket::from_id(ticket)),
            "already revoked"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_forged_ticket_number_never_reaches_the_broker() {
        let dir = work_dir("forged");
        let file = dir.join("secret.txt");
        std::fs::write(&file, b"secret").unwrap();
        let broker = FileBroker::new();
        // Un ticket existant du courtier mais jamais remis au JS par le pont.
        let internal = broker.grant_read(&file, 100).unwrap();
        let mut state = BridgeState::new(Vec::new());
        let mut host = FakeHost::new(None);
        let reply = process_bridge_request(
            Request::ReadTicket {
                id: "1".into(),
                ticket: internal.id(),
            },
            &mut state,
            &broker,
            &mut host,
        );
        assert_eq!(
            text_of(&reply),
            "us120-file|1|1|error|files.error.unknown_ticket"
        );
        assert!(broker.read(internal).is_ok(), "untouched");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn write_test_font(dir: &Path, name: &str, family: &str) {
        let utf16: Vec<u8> = family.encode_utf16().flat_map(u16::to_be_bytes).collect();
        let mut table = vec![0, 0, 0, 1, 0, 18];
        table.extend([0, 3, 0, 1, 0x04, 0x09, 0, 1]);
        table.extend((utf16.len() as u16).to_be_bytes());
        table.extend([0, 0]);
        table.extend(utf16);
        let mut font = vec![0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        font.extend(b"name");
        font.extend(0u32.to_be_bytes());
        font.extend(28u32.to_be_bytes());
        font.extend((table.len() as u32).to_be_bytes());
        font.extend(table);
        std::fs::write(dir.join(name), font).unwrap();
    }

    #[test]
    fn font_list_then_read_by_opaque_id_only() {
        let dir = work_dir("fonts");
        write_test_font(&dir, "a.ttf", "Zeta");
        let broker = FileBroker::new();
        let mut state = BridgeState::new(vec![dir.clone()]);
        let mut host = FakeHost::new(None);
        let early = process_bridge_request(
            Request::ReadFont {
                id: "1".into(),
                font: "f1-0".into(),
            },
            &mut state,
            &broker,
            &mut host,
        );
        assert_eq!(
            text_of(&early),
            "us120-file|1|1|error|fonts.error.unknown_id"
        );
        let listed = text_of(&process_bridge_request(
            Request::ListFonts { id: "2".into() },
            &mut state,
            &broker,
            &mut host,
        ));
        let payload = listed
            .strip_prefix("us120-file|1|2|ok|b64;")
            .expect(&listed);
        let tsv = String::from_utf8(crate::file_bridge::decode_base64(payload).unwrap()).unwrap();
        let line = tsv.lines().next().unwrap();
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(&fields[1..3], ["Zeta", "Regular"]);
        assert!(!tsv.contains(dir.to_string_lossy().as_ref()));
        let read = text_of(&process_bridge_request(
            Request::ReadFont {
                id: "3".into(),
                font: fields[0].to_owned(),
            },
            &mut state,
            &broker,
            &mut host,
        ));
        assert!(read.starts_with("us120-file|1|3|ok|b64;"), "{read}");
        let forged = text_of(&process_bridge_request(
            Request::ReadFont {
                id: "4".into(),
                font: "f1-99".into(),
            },
            &mut state,
            &broker,
            &mut host,
        ));
        assert_eq!(forged, "us120-file|1|4|error|fonts.error.unknown_id");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_url_goes_through_the_host_and_reports_its_error_key() {
        let broker = FileBroker::new();
        let mut state = BridgeState::new(Vec::new());
        let mut host = FakeHost::new(None);
        let ok = process_bridge_request(
            Request::OpenUrl {
                id: "1".into(),
                url: "https://example.org/".into(),
            },
            &mut state,
            &broker,
            &mut host,
        );
        assert_eq!(text_of(&ok), "us120-file|1|1|ok|");
        assert_eq!(host.urls, ["https://example.org/"]);
        host.url_result = Err("url.error.no_handler");
        let failed = process_bridge_request(
            Request::OpenUrl {
                id: "2".into(),
                url: "https://example.org/".into(),
            },
            &mut state,
            &broker,
            &mut host,
        );
        assert_eq!(
            text_of(&failed),
            "us120-file|1|2|error|url.error.no_handler"
        );
    }

    /// Chaque clé d'erreur du natif doit exister dans le catalogue français (US-113, critère 3).
    #[test]
    fn every_native_error_key_exists_in_the_french_catalogue() {
        let catalogue = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/i18n/fr-FR.txt"),
        )
        .unwrap();
        let defined: std::collections::HashSet<&str> = catalogue
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, _)| key)
            .collect();
        let mut missing = Vec::new();
        for file in [
            "bridge_ops.rs",
            "file-bridge.rs",
            "file_dialog.rs",
            "files.rs",
            "fonts.rs",
            "url_open.rs",
            "win32.rs",
            "webview/shared_buffer.rs",
        ] {
            let source = std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file),
            )
            .unwrap();
            let mut rest = source.as_str();
            while let Some(at) = rest.find('"') {
                let after = &rest[at + 1..];
                let Some(end) = after.find('"') else { break };
                let literal = &after[..end];
                rest = &after[end + 1..];
                let looks_like_key = ["files.error.", "fonts.error.", "url.error."]
                    .iter()
                    .any(|prefix| literal.starts_with(prefix))
                    && !literal.ends_with('.')
                    && literal
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || matches!(b, b'.' | b'_'));
                if looks_like_key && !defined.contains(literal) {
                    missing.push(format!("{file}: {literal}"));
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "keys missing from fr-FR.txt: {missing:#?}"
        );
    }
}
