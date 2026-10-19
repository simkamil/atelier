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
use crate::recent::{self, RecentList};
use crate::webview::{SharedBufferError, WebView};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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

/// Inactivité au-delà de laquelle une écriture en morceaux abandonnée libère son ticket.
const STAGING_IDLE_LIMIT: std::time::Duration = std::time::Duration::from_secs(60);

struct Staging {
    ticket: u64,
    next_seq: u32,
    last_chunk: std::time::Instant,
    bytes: Vec<u8>,
}

/// Travail bloquant (lecture de fichier, balayage des polices, écriture atomique) exécuté sur un fil
/// de travail ; il ne touche pas à l'état du pont, seulement au courtier (partagé) et à ses propres
/// données. Le résultat revient sur la STA par `BridgeState::finish`.
pub(crate) type Job = Box<dyn FnOnce() -> JobResult + Send + 'static>;

pub(crate) struct JobResult {
    outcome: Outcome,
    fonts: Option<Arc<FontCatalog>>,
    /// Fichier écrit avec succès, à noter dans les documents récents (US-320).
    remember: Option<PathBuf>,
    /// Ticket de lecture dont la lecture a réussi : (numéro, chemin), en attente de `recentNote`.
    readable: Option<(u64, PathBuf)>,
}

impl JobResult {
    fn outcome(outcome: Outcome) -> Self {
        Self {
            outcome,
            fonts: None,
            remember: None,
            readable: None,
        }
    }
}

/// Suite d'une requête : réponse immédiate, ou travail à exécuter hors du fil UI.
pub(crate) enum Step {
    Done(Outcome),
    Job(Job),
}

/// État de pont propre à la fenêtre : tickets remis au JS, écriture en cours, catalogue de polices.
pub(crate) struct BridgeState {
    read_tickets: Vec<u64>,
    write_tickets: Vec<u64>,
    staging: Option<Staging>,
    fonts: Option<Arc<FontCatalog>>,
    font_dirs: Vec<PathBuf>,
    /// US-320 : destinations des tickets d'écriture remis (pour noter le fichier une fois écrit).
    write_paths: Vec<(u64, PathBuf)>,
    /// US-320 : chemins des tickets de lecture remis ; lus avec succès, ils attendent la confirmation du front.
    read_paths: Vec<(u64, PathBuf)>,
    readable: Vec<(u64, PathBuf)>,
    /// US-320 : documents récents et fichier où ils sont conservés (`None` : en mémoire seulement).
    recent: RecentList,
    recent_file: Option<PathBuf>,
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
            write_paths: Vec::new(),
            read_paths: Vec::new(),
            readable: Vec::new(),
            recent: RecentList::default(),
            recent_file: None,
        }
    }

    /// Relit les documents récents de `file` et les y réécrit à chaque changement.
    pub(crate) fn with_recent_file(mut self, file: PathBuf) -> Self {
        self.recent = RecentList::load(&file);
        self.recent_file = Some(file);
        self
    }

    fn persist_recent(&self) {
        // Les récents sont un confort : un disque en échec ne doit pas gêner l'ouverture ou l'enregistrement.
        if let Some(file) = &self.recent_file {
            let _ = self.recent.save(file);
        }
    }

    /// Note l'ouverture ou l'enregistrement d'un fichier choisi par le natif.
    pub(crate) fn remember(&mut self, path: &Path) {
        if self.recent.touch(path, recent::now_secs()) {
            self.persist_recent();
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

    /// Applique le résultat d'un travail terminé (sur la STA) et renvoie la réponse à poster.
    pub(crate) fn finish(&mut self, result: JobResult) -> Outcome {
        if let Some(catalog) = result.fonts {
            self.fonts = Some(catalog);
        }
        if let Some(path) = result.remember {
            self.remember(&path);
        }
        if let Some(read) = result.readable {
            self.readable.push(read);
            if self.readable.len() > MAX_OPEN_TICKETS {
                self.readable.remove(0);
            }
        }
        result.outcome
    }

    /// Libère le ticket d'une écriture en morceaux restée sans nouveau morceau depuis `idle`.
    fn expire_idle_write(
        &mut self,
        broker: &FileBroker,
        now: std::time::Instant,
        idle: std::time::Duration,
    ) {
        let expired = self
            .staging
            .as_ref()
            .filter(|staging| now.saturating_duration_since(staging.last_chunk) >= idle)
            .map(|staging| staging.ticket);
        if let Some(ticket) = expired {
            self.forget_write(ticket, broker);
        }
    }

    /// Retire un ticket d'écriture (et l'écriture en cours) sans le révoquer : le travail qui
    /// l'utilise encore le révoque lui-même.
    fn detach_write(&mut self, ticket: u64) {
        self.write_tickets.retain(|held| *held != ticket);
        self.write_paths.retain(|(held, _)| *held != ticket);
        if self
            .staging
            .as_ref()
            .is_some_and(|staging| staging.ticket == ticket)
        {
            self.staging = None;
        }
    }

    fn forget_read(&mut self, ticket: u64, broker: &FileBroker) {
        self.read_tickets.retain(|held| *held != ticket);
        self.read_paths.retain(|(held, _)| *held != ticket);
        broker.revoke_read(ReadTicket::from_id(ticket));
    }

    fn forget_write(&mut self, ticket: u64, broker: &FileBroker) {
        self.write_tickets.retain(|held| *held != ticket);
        self.write_paths.retain(|(held, _)| *held != ticket);
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

/// Démarre une requête du pont étendu. Les requêtes historiques (open, openDocx, save, dirty,
/// launch) ne passent pas ici. Les dialogues et ShellExecute restent sur la STA ; les lectures,
/// l'écriture finale et le balayage des polices deviennent un `Job` pour un fil de travail.
pub(crate) fn begin_bridge_request(
    request: Request,
    state: &mut BridgeState,
    broker: &Arc<FileBroker>,
    host: &mut dyn BridgeHost,
) -> Step {
    state.expire_idle_write(broker, std::time::Instant::now(), STAGING_IDLE_LIMIT);
    match request {
        Request::PickOpen { id, spec } => Step::Done(pick_open(&id, &spec, state, broker, host)),
        Request::PickSave { id, spec } => Step::Done(pick_save(&id, &spec, state, broker, host)),
        Request::ReadTicket { id, ticket } => read_ticket(id, ticket, state, broker),
        Request::ReleaseTicket { id, ticket, write } => {
            let known = if write {
                state.write_tickets.contains(&ticket)
            } else {
                state.read_tickets.contains(&ticket)
            };
            if !known {
                return Step::Done(error_reply(&id, "files.error.unknown_ticket"));
            }
            if write {
                state.forget_write(ticket, broker);
            } else {
                state.forget_read(ticket, broker);
            }
            Step::Done(ok_reply(&id, ""))
        }
        Request::WriteTicket {
            id,
            ticket,
            seq,
            last,
            bytes,
        } => write_ticket(id, ticket, seq, last, bytes, state, broker),
        Request::ListFonts { id } => {
            let previous = state.fonts.clone();
            let dirs = state.font_dirs.clone();
            Step::Job(Box::new(move || {
                let catalog = Arc::new(FontCatalog::scan(&dirs, previous.as_deref()));
                let listing = catalog.list_tsv().into_bytes();
                let size = listing.len();
                JobResult {
                    outcome: bytes_outcome(&id, "b64;", &shared_payload(size), "fb;", listing),
                    fonts: Some(catalog),
                    remember: None,
                    readable: None,
                }
            }))
        }
        Request::ReadFont { id, font } => match state.fonts.clone() {
            None => Step::Done(error_reply(&id, "fonts.error.unknown_id")),
            Some(catalog) => Step::Job(Box::new(move || {
                JobResult::outcome(match catalog.read(&font) {
                    Ok(bytes) => {
                        let payload = shared_payload(bytes.len());
                        bytes_outcome(&id, "b64;", &payload, "fb;", bytes)
                    }
                    Err(key) => error_reply(&id, key),
                })
            })),
        },
        Request::OpenUrl { id, url } => Step::Done(match host.open_url(&url) {
            Ok(()) => ok_reply(&id, ""),
            Err(key) => error_reply(&id, key),
        }),
        Request::RecentList { id } => recent_list(id, state),
        Request::RecentOpen { id, recent } => Step::Done(recent_open(&id, recent, state, broker)),
        Request::RecentPin { id, recent, pinned } => {
            Step::Done(if state.recent.set_pinned(recent, pinned) {
                state.persist_recent();
                ok_reply(&id, "")
            } else {
                error_reply(&id, "files.error.unknown_recent")
            })
        }
        Request::RecentNote { id, ticket } => Step::Done(
            match state.readable.iter().position(|(held, _)| *held == ticket) {
                Some(index) => {
                    let (_, path) = state.readable.remove(index);
                    state.remember(&path);
                    ok_reply(&id, "")
                }
                None => error_reply(&id, "files.error.unknown_ticket"),
            },
        ),
        Request::RecentRemove { id, recent } => Step::Done(if state.recent.remove(recent) {
            state.persist_recent();
            ok_reply(&id, "")
        } else {
            error_reply(&id, "files.error.unknown_recent")
        }),
        Request::Open { id }
        | Request::OpenDocx { id }
        | Request::Launch { id }
        | Request::Dirty { id, .. }
        | Request::Save { id, .. } => Step::Done(error_reply(&id, INVALID_REQUEST)),
    }
}

/// Version synchrone (tests) : exécute le travail dans le fil appelant et applique son résultat.
#[cfg(test)]
pub(crate) fn process_bridge_request(
    request: Request,
    state: &mut BridgeState,
    broker: &Arc<FileBroker>,
    host: &mut dyn BridgeHost,
) -> Outcome {
    match begin_bridge_request(request, state, broker, host) {
        Step::Done(outcome) => outcome,
        Step::Job(job) => {
            let result = job();
            state.finish(result)
        }
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
    issue_open_ticket(id, &path, format, state, broker)
}

/// Ticket de lecture d'un fichier choisi par le natif (dialogue ou récent) : taille bornée, métadonnées
/// obligatoires. Le fichier n'entre dans les récents qu'une fois lu et accepté (`recentNote`).
fn issue_open_ticket(
    id: &str,
    path: &Path,
    format: &str,
    state: &mut BridgeState,
    broker: &FileBroker,
) -> Outcome {
    let ticket = match broker.grant_read(path, TICKET_READ_LIMIT) {
        Ok(ticket) => ticket,
        Err(error) => return error_reply(id, error.message_key),
    };
    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(_) => {
            broker.revoke_read(ticket);
            return error_reply(id, "files.error.read_failed");
        }
    };
    if size > TICKET_READ_LIMIT {
        broker.revoke_read(ticket);
        return error_reply(id, "files.error.too_large");
    }
    state.read_tickets.push(ticket.id());
    state.read_paths.push((ticket.id(), path.to_path_buf()));
    ok_reply(
        id,
        &format!("r{};{format};{size};{}", ticket.id(), file_name_b64(path)),
    )
}

/// Format (identifiant JS) d'un chemin d'après son extension, parmi les formats connus du natif.
fn format_of_extension(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    FORMATS
        .iter()
        .find(|(_, known)| *known == extension)
        .map(|(format, _)| *format)
}

/// Liste des récents : `id\tépinglé\tdate\tprésent\tchemin` par ligne, en Base64 en ligne. La
/// présence sur le disque est testée hors du fil UI (un partage réseau lent ne fige pas la fenêtre).
fn recent_list(id: String, state: &BridgeState) -> Step {
    let entries = state.recent.ordered();
    Step::Job(Box::new(move || {
        let mut text = String::new();
        for entry in entries {
            let present = std::fs::metadata(&entry.path).is_ok_and(|meta| meta.is_file());
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                entry.id,
                u8::from(entry.pinned),
                entry.used,
                u8::from(present),
                entry.path.display()
            ));
        }
        JobResult::outcome(ok_reply(
            &id,
            &format!("b64;{}", encode_base64(text.as_bytes())),
        ))
    }))
}

/// Ouvre un récent comme `pick_open` après le choix de l'utilisateur : ticket de lecture opaque.
/// Fichier absent : `files.error.not_found`, l'entrée reste dans la liste (l'utilisateur la retire).
fn recent_open(id: &str, recent: u64, state: &mut BridgeState, broker: &FileBroker) -> Outcome {
    if state.read_tickets.len() >= MAX_OPEN_TICKETS {
        return error_reply(id, "files.error.ticket_exhausted");
    }
    let Some(path) = state.recent.path_of(recent) else {
        return error_reply(id, "files.error.unknown_recent");
    };
    let Some(format) = format_of_extension(&path) else {
        return error_reply(id, "files.error.unsupported_format");
    };
    issue_open_ticket(id, &path, format, state, broker)
}

fn pick_save(
    id: &str,
    spec: &DialogSpec,
    state: &mut BridgeState,
    broker: &FileBroker,
    host: &mut dyn BridgeHost,
) -> Outcome {
    // Une nouvelle demande d'enregistrement abandonne l'écriture restée en cours : son ticket est
    // révoqué pour qu'elle ne bloque pas les autres.
    if let Some(abandoned) = state.staging.as_ref().map(|staging| staging.ticket) {
        state.forget_write(abandoned, broker);
    }
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
            state.write_paths.push((ticket.id(), path.clone()));
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

fn read_ticket(id: String, ticket: u64, state: &mut BridgeState, broker: &Arc<FileBroker>) -> Step {
    if !state.read_tickets.contains(&ticket) {
        return Step::Done(error_reply(&id, "files.error.unknown_ticket"));
    }
    // Un ticket de lecture sert une fois : retiré tout de suite, révoqué par le travail quel que
    // soit le résultat. La lecture (jusqu'à 256 Mio) ne bloque pas le fil UI.
    state.read_tickets.retain(|held| *held != ticket);
    let path = state
        .read_paths
        .iter()
        .position(|(held, _)| *held == ticket)
        .map(|index| state.read_paths.remove(index).1);
    let broker = Arc::clone(broker);
    Step::Job(Box::new(move || {
        let number = ticket;
        let ticket = ReadTicket::from_id(ticket);
        let result = broker.read(ticket);
        broker.revoke_read(ticket);
        let readable = result
            .is_ok()
            .then_some(path)
            .flatten()
            .map(|path| (number, path));
        let mut done = JobResult::outcome(match result {
            Ok(bytes) => {
                let payload = shared_payload(bytes.len());
                bytes_outcome(&id, "b64;", &payload, "fb;", bytes)
            }
            Err(error) => error_reply(&id, error.message_key),
        });
        done.readable = readable;
        done
    }))
}

fn write_ticket(
    id: String,
    ticket: u64,
    seq: u32,
    last: bool,
    bytes: Vec<u8>,
    state: &mut BridgeState,
    broker: &Arc<FileBroker>,
) -> Step {
    if !state.write_tickets.contains(&ticket) {
        return Step::Done(error_reply(&id, "files.error.unknown_ticket"));
    }
    let mut staging = match state.staging.take() {
        Some(staging) if staging.ticket != ticket => {
            state.staging = Some(staging);
            return Step::Done(error_reply(&id, "files.error.queue_full"));
        }
        Some(staging) => staging,
        None => Staging {
            ticket,
            next_seq: 0,
            last_chunk: std::time::Instant::now(),
            bytes: Vec::new(),
        },
    };
    if seq != staging.next_seq || bytes.len() > MAX_CHUNK_BYTES {
        state.forget_write(ticket, broker);
        return Step::Done(error_reply(&id, "files.error.write_sequence"));
    }
    if staging.bytes.len() + bytes.len() > MAX_WRITE_BYTES {
        state.forget_write(ticket, broker);
        return Step::Done(error_reply(&id, "files.error.too_large"));
    }
    staging.bytes.extend_from_slice(&bytes);
    staging.next_seq += 1;
    staging.last_chunk = std::time::Instant::now();
    if !last {
        state.staging = Some(staging);
        return Step::Done(ok_reply(&id, ""));
    }
    // Écriture atomique par le courtier, hors du fil UI ; le ticket est consommé quel que soit le
    // résultat.
    let destination = state
        .write_paths
        .iter()
        .find(|(held, _)| *held == ticket)
        .map(|(_, path)| path.clone());
    state.detach_write(ticket);
    let broker = Arc::clone(broker);
    Step::Job(Box::new(move || {
        let ticket = WriteTicket::from_id(ticket);
        let result = broker.write(ticket, &staging.bytes);
        broker.revoke_write(ticket);
        let written = result.is_ok();
        let mut done = JobResult::outcome(match result {
            Ok(()) => ok_reply(&id, ""),
            Err(error) => error_reply(&id, error.message_key),
        });
        if written {
            done.remember = destination;
        }
        done
    }))
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
    fn idle_chunked_write_releases_its_ticket_after_the_idle_limit() {
        let dir = work_dir("idle");
        let file = dir.join("out.txt");
        let broker = Arc::new(FileBroker::new());
        let mut state = BridgeState::new(Vec::new());
        let mut host = FakeHost::new(None);
        let handle = state.issue_write_ticket(&broker, &file).unwrap();
        let (_, ticket) = parse_ticket(&handle).unwrap();
        let chunk = process_bridge_request(
            Request::WriteTicket {
                id: "1".into(),
                ticket,
                seq: 0,
                last: false,
                bytes: b"abc".to_vec(),
            },
            &mut state,
            &broker,
            &mut host,
        );
        assert_eq!(text_of(&chunk), "us120-file|1|1|ok|");
        assert!(state.staging.is_some());
        let started = std::time::Instant::now();
        let almost = started + STAGING_IDLE_LIMIT - std::time::Duration::from_secs(5);
        state.expire_idle_write(&broker, almost, STAGING_IDLE_LIMIT);
        assert_eq!(state.outstanding(), (0, 1), "still within the idle limit");
        let late = started + STAGING_IDLE_LIMIT + std::time::Duration::from_secs(5);
        state.expire_idle_write(&broker, late, STAGING_IDLE_LIMIT);
        assert_eq!(state.outstanding(), (0, 0));
        assert!(state.staging.is_none());
        let after = process_bridge_request(
            Request::WriteTicket {
                id: "2".into(),
                ticket,
                seq: 1,
                last: true,
                bytes: b"d".to_vec(),
            },
            &mut state,
            &broker,
            &mut host,
        );
        assert_eq!(
            text_of(&after),
            "us120-file|1|2|error|files.error.unknown_ticket"
        );
        assert!(!file.exists(), "nothing was written");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_then_read_ticket_round_trips_and_ticket_is_single_use() {
        let dir = work_dir("open");
        let file = dir.join("Lettre é.docx");
        std::fs::write(&file, b"docx bytes").unwrap();
        let broker = Arc::new(FileBroker::new());
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
        let broker = Arc::new(FileBroker::new());
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
        let broker = Arc::new(FileBroker::new());
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
        let broker = Arc::new(FileBroker::new());
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
        broker: &Arc<FileBroker>,
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
        broker: &Arc<FileBroker>,
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
        let broker = Arc::new(FileBroker::new());
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
        let broker = Arc::new(FileBroker::new());
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
    fn an_abandoned_write_is_released_by_the_next_save_request() {
        let dir = work_dir("abandoned");
        let broker = Arc::new(FileBroker::new());
        let mut state = BridgeState::new(Vec::new());
        let first = pick_save_ticket(&dir, "a.txt", &mut state, &broker);
        assert_eq!(
            write(first, 0, false, b"partial", &mut state, &broker),
            "us120-file|1|5|ok|"
        );
        let second = pick_save_ticket(&dir, "b.txt", &mut state, &broker);
        assert_eq!(
            state.outstanding(),
            (0, 1),
            "the abandoned ticket was dropped"
        );
        assert_eq!(
            write(first, 1, true, b"x", &mut state, &broker),
            "us120-file|1|5|error|files.error.unknown_ticket"
        );
        assert_eq!(
            write(second, 0, true, b"fine", &mut state, &broker),
            "us120-file|1|5|ok|"
        );
        assert_eq!(std::fs::read(dir.join("b.txt")).unwrap(), b"fine");
        assert!(!dir.join("a.txt").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn blocking_work_is_a_job_that_runs_off_the_calling_thread() {
        let dir = work_dir("job");
        let file = dir.join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let broker = Arc::new(FileBroker::new());
        let mut state = BridgeState::new(vec![dir.clone()]);
        let ticket = state
            .issue_read_ticket(&broker, &file, TICKET_READ_LIMIT)
            .unwrap();
        let mut host = FakeHost::new(None);
        let caller = std::thread::current().id();
        let steps = [
            begin_bridge_request(
                Request::ReadTicket {
                    id: "1".into(),
                    ticket: parse_ticket(&ticket).unwrap().1,
                },
                &mut state,
                &broker,
                &mut host,
            ),
            begin_bridge_request(
                Request::ListFonts { id: "2".into() },
                &mut state,
                &broker,
                &mut host,
            ),
        ];
        for step in steps {
            let Step::Job(job) = step else {
                panic!("read and font scan must be deferred to a worker");
            };
            let worker = std::thread::spawn(move || (std::thread::current().id(), job()));
            let (thread, result) = worker.join().unwrap();
            assert_ne!(thread, caller);
            let outcome = state.finish(result);
            assert!(outcome.text().contains("|ok|b64;"), "{}", outcome.text());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn save_with_an_unlisted_extension_gets_no_ticket_and_release_revokes() {
        let dir = work_dir("release");
        let broker = Arc::new(FileBroker::new());
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
        let broker = Arc::new(FileBroker::new());
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
        let broker = Arc::new(FileBroker::new());
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
        let broker = Arc::new(FileBroker::new());
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

    fn recent_state(dir: &Path) -> BridgeState {
        BridgeState::new(Vec::new()).with_recent_file(dir.join("recent.ini"))
    }

    fn recent_rows(state: &mut BridgeState, broker: &Arc<FileBroker>) -> Vec<Vec<String>> {
        let mut host = FakeHost::new(None);
        let reply = text_of(&process_bridge_request(
            Request::RecentList { id: "9".into() },
            state,
            broker,
            &mut host,
        ));
        let payload = reply.strip_prefix("us120-file|1|9|ok|b64;").expect(&reply);
        String::from_utf8(crate::file_bridge::decode_base64(payload).unwrap())
            .unwrap()
            .lines()
            .map(|line| line.split('\t').map(str::to_owned).collect())
            .collect()
    }

    fn request_text(state: &mut BridgeState, broker: &Arc<FileBroker>, request: Request) -> String {
        let mut host = FakeHost::new(None);
        text_of(&process_bridge_request(request, state, broker, &mut host))
    }

    /// US-320 : ouvrir via le dialogue, enregistrer et rouvrir un récent alimentent `recent.ini`, qui survit au
    /// redémarrage (nouvel état relu depuis le fichier) ; épinglage et retrait sont persistés ; un fichier
    /// disparu est signalé (`présent` = 0, `not_found` à l'ouverture) sans quitter la liste.
    #[test]
    fn recent_documents_persist_across_restarts_and_report_missing_files() {
        let dir = work_dir("recent");
        let first = dir.join("Premier é.docx");
        let second = dir.join("second.txt");
        std::fs::write(&first, b"un").unwrap();
        let broker = Arc::new(FileBroker::new());
        let mut state = recent_state(&dir);
        assert!(recent_rows(&mut state, &broker).is_empty());

        // Ouverture par le dialogue : l'entrée apparaît, avec le chemin complet pour l'affichage.
        let mut host = FakeHost::new(Some(first.clone()));
        let opened = text_of(&process_bridge_request(
            Request::PickOpen {
                id: "1".into(),
                spec: open_spec(),
            },
            &mut state,
            &broker,
            &mut host,
        ));
        assert!(opened.contains("|ok|r"), "{opened}");
        // Choisi mais pas encore lu ni accepté : pas dans la liste.
        assert!(recent_rows(&mut state, &broker).is_empty());
        let open_ticket = opened
            .strip_prefix("us120-file|1|1|ok|")
            .and_then(|payload| parse_ticket(payload.split(';').next()?))
            .unwrap()
            .1;
        let read = request_text(
            &mut state,
            &broker,
            Request::ReadTicket {
                id: "7".into(),
                ticket: open_ticket,
            },
        );
        assert!(read.contains("|ok|b64;"), "{read}");
        assert!(
            recent_rows(&mut state, &broker).is_empty(),
            "lu mais pas accepté"
        );
        let noted = request_text(
            &mut state,
            &broker,
            Request::RecentNote {
                id: "8".into(),
                ticket: open_ticket,
            },
        );
        assert_eq!(noted, "us120-file|1|8|ok|");
        assert_eq!(recent_rows(&mut state, &broker).len(), 1);
        let again = request_text(
            &mut state,
            &broker,
            Request::RecentNote {
                id: "9".into(),
                ticket: open_ticket,
            },
        );
        assert_eq!(again, "us120-file|1|9|error|files.error.unknown_ticket");
        // Enregistrement : le fichier n'entre dans la liste qu'une fois écrit.
        let ticket = pick_save_ticket(&dir, "second.txt", &mut state, &broker);
        assert_eq!(
            recent_rows(&mut state, &broker).len(),
            1,
            "pas avant l'écriture"
        );
        let wrote = request_text(
            &mut state,
            &broker,
            Request::WriteTicket {
                id: "2".into(),
                ticket,
                seq: 0,
                last: true,
                bytes: b"deux".to_vec(),
            },
        );
        assert_eq!(wrote, "us120-file|1|2|ok|");
        let rows = recent_rows(&mut state, &broker);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0][4],
            second.display().to_string(),
            "le plus récent d'abord"
        );
        assert_eq!((rows[0][1].as_str(), rows[0][3].as_str()), ("0", "1"));
        assert_eq!(rows[1][4], first.display().to_string());
        assert!(rows[0][2].parse::<u64>().unwrap() > 1_700_000_000);

        // Redémarrage : un nouvel état relit recent.ini.
        let first_id: u64 = rows[1][0].parse().unwrap();
        let mut restarted = recent_state(&dir);
        let again = recent_rows(&mut restarted, &broker);
        assert_eq!(again.len(), 2);
        assert_eq!(again[1][4], first.display().to_string());

        // Épinglage : l'entrée épinglée passe en tête et le reste après redémarrage.
        let pinned = request_text(
            &mut state,
            &broker,
            Request::RecentPin {
                id: "3".into(),
                recent: first_id,
                pinned: true,
            },
        );
        assert_eq!(pinned, "us120-file|1|3|ok|");
        let mut restarted = recent_state(&dir);
        let after = recent_rows(&mut restarted, &broker);
        // Les numéros sont ceux de la session : on désigne l'entrée par le numéro relu après redémarrage.
        let restarted_first: u64 = after[0][0].parse().unwrap();
        assert_eq!(
            (after[0][1].as_str(), after[0][4].as_str()),
            ("1", first.display().to_string().as_str())
        );

        // Fichier introuvable : signalé dans la liste et à l'ouverture, l'entrée reste.
        std::fs::remove_file(&first).unwrap();
        let missing_rows = recent_rows(&mut restarted, &broker);
        assert_eq!(missing_rows[0][3], "0");
        let missing = request_text(
            &mut restarted,
            &broker,
            Request::RecentOpen {
                id: "4".into(),
                recent: restarted_first,
            },
        );
        assert_eq!(missing, "us120-file|1|4|error|files.error.not_found");
        assert_eq!(recent_rows(&mut restarted, &broker).len(), 2);
        assert_eq!(
            restarted.outstanding(),
            (0, 0),
            "aucun ticket pour un fichier absent"
        );

        // Retrait : l'entrée disparaît (le fichier n'est jamais touché) ; un numéro inconnu est refusé.
        let removed = request_text(
            &mut restarted,
            &broker,
            Request::RecentRemove {
                id: "5".into(),
                recent: restarted_first,
            },
        );
        assert_eq!(removed, "us120-file|1|5|ok|");
        assert!(second.exists());
        let unknown = request_text(
            &mut restarted,
            &broker,
            Request::RecentRemove {
                id: "6".into(),
                recent: restarted_first,
            },
        );
        assert_eq!(unknown, "us120-file|1|6|error|files.error.unknown_recent");
        let last = recent_rows(&mut recent_state(&dir), &broker);
        assert_eq!(last.len(), 1);
        assert_eq!(last[0][4], second.display().to_string());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn opening_a_recent_hands_out_the_same_ticket_as_the_dialog_and_never_a_path() {
        let dir = work_dir("recent-open");
        let file = dir.join("Lettre.docx");
        std::fs::write(&file, b"docx bytes").unwrap();
        let broker = Arc::new(FileBroker::new());
        let mut state = recent_state(&dir);
        state.remember(&file);
        let id: u64 = recent_rows(&mut state, &broker)[0][0].parse().unwrap();
        let text = request_text(
            &mut state,
            &broker,
            Request::RecentOpen {
                id: "1".into(),
                recent: id,
            },
        );
        let payload = text.strip_prefix("us120-file|1|1|ok|").expect(&text);
        let fields: Vec<&str> = payload.split(';').collect();
        assert_eq!((fields[1], fields[2]), ("docx", "10"));
        assert_eq!(
            crate::file_bridge::decode_base64(fields[3]).unwrap(),
            "Lettre.docx".as_bytes()
        );
        assert!(!text.contains(dir.to_string_lossy().as_ref()));
        let (_, ticket) = parse_ticket(fields[0]).unwrap();
        let read = request_text(
            &mut state,
            &broker,
            Request::ReadTicket {
                id: "2".into(),
                ticket,
            },
        );
        assert_eq!(
            read,
            format!("us120-file|1|2|ok|b64;{}", encode_base64(b"docx bytes"))
        );
        let unknown = request_text(
            &mut state,
            &broker,
            Request::RecentOpen {
                id: "3".into(),
                recent: id + 100,
            },
        );
        assert_eq!(unknown, "us120-file|1|3|error|files.error.unknown_recent");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Un fichier devenu illisible entre le choix et la lecture n'entre jamais dans les récents, et un ticket
    /// jamais lu ou jamais confirmé ne peut pas être noté.
    #[test]
    fn an_unreadable_file_never_reaches_the_recent_list() {
        let dir = work_dir("recent-unreadable");
        let file = dir.join("fragile.txt");
        std::fs::write(&file, b"x").unwrap();
        let broker = Arc::new(FileBroker::new());
        let mut state = recent_state(&dir);
        let mut host = FakeHost::new(Some(file.clone()));
        let opened = text_of(&process_bridge_request(
            Request::PickOpen {
                id: "1".into(),
                spec: open_spec(),
            },
            &mut state,
            &broker,
            &mut host,
        ));
        let ticket = parse_ticket(
            opened
                .strip_prefix("us120-file|1|1|ok|")
                .unwrap()
                .split(';')
                .next()
                .unwrap(),
        )
        .unwrap()
        .1;
        let early = request_text(
            &mut state,
            &broker,
            Request::RecentNote {
                id: "2".into(),
                ticket,
            },
        );
        assert_eq!(early, "us120-file|1|2|error|files.error.unknown_ticket");
        std::fs::remove_file(&file).unwrap();
        let read = request_text(
            &mut state,
            &broker,
            Request::ReadTicket {
                id: "3".into(),
                ticket,
            },
        );
        assert!(read.contains("|error|"), "{read}");
        let late = request_text(
            &mut state,
            &broker,
            Request::RecentNote {
                id: "4".into(),
                ticket,
            },
        );
        assert_eq!(late, "us120-file|1|4|error|files.error.unknown_ticket");
        assert!(recent_rows(&mut state, &broker).is_empty());
        assert!(!dir.join("recent.ini").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_write_and_a_cancelled_dialog_leave_the_recent_list_untouched() {
        let dir = work_dir("recent-failed");
        let broker = Arc::new(FileBroker::new());
        let mut state = recent_state(&dir);
        let ticket = pick_save_ticket(&dir, "perdu.txt", &mut state, &broker);
        let bad = request_text(
            &mut state,
            &broker,
            Request::WriteTicket {
                id: "2".into(),
                ticket,
                seq: 5,
                last: true,
                bytes: b"x".to_vec(),
            },
        );
        assert!(bad.contains("|error|"), "{bad}");
        let mut host = FakeHost::new(None);
        let cancelled = text_of(&process_bridge_request(
            Request::PickOpen {
                id: "3".into(),
                spec: open_spec(),
            },
            &mut state,
            &broker,
            &mut host,
        ));
        assert_eq!(cancelled, "us120-file|1|3|cancelled|");
        assert!(recent_rows(&mut state, &broker).is_empty());
        assert!(!dir.join("recent.ini").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recent_requests_are_parsed_strictly_at_ingress() {
        let mut queue = queue();
        let accepted = |queue: &mut Queue, id: u32, operation: &str, tail: &str| {
            queue
                .accept(ORIGIN, &format!("us120-file|1|{id}|{operation}|{tail}"))
                .is_ok()
        };
        assert!(accepted(&mut queue, 1, "recentList", ""));
        assert!(accepted(&mut queue, 2, "recentOpen", "12"));
        assert!(accepted(&mut queue, 3, "recentPin", "12;1"));
        assert!(accepted(&mut queue, 4, "recentRemove", "12"));
        assert_eq!(queue.pop(), Some(Request::RecentList { id: "1".into() }));
        assert_eq!(
            queue.pop(),
            Some(Request::RecentOpen {
                id: "2".into(),
                recent: 12
            })
        );
        assert_eq!(
            queue.pop(),
            Some(Request::RecentPin {
                id: "3".into(),
                recent: 12,
                pinned: true
            })
        );
        assert_eq!(
            queue.pop(),
            Some(Request::RecentRemove {
                id: "4".into(),
                recent: 12
            })
        );
        assert!(accepted(&mut queue, 5, "recentNote", "r12"));
        assert_eq!(
            queue.pop(),
            Some(Request::RecentNote {
                id: "5".into(),
                ticket: 12
            })
        );
        for (index, (operation, tail)) in [
            ("recentNote", "w12"),
            ("recentNote", "12"),
            ("recentList", "x"),
            ("recentOpen", ""),
            ("recentOpen", "0"),
            ("recentOpen", "C:\\a.docx"),
            ("recentOpen", "12345678901"),
            ("recentPin", "12"),
            ("recentPin", "12;2"),
            ("recentPin", ";1"),
            ("recentRemove", "-1"),
        ]
        .into_iter()
        .enumerate()
        {
            let id = 10 + index as u32;
            assert!(
                !accepted(&mut queue, id, operation, tail),
                "{operation}|{tail}"
            );
            assert_eq!(
                queue.pop_rejection().as_deref(),
                Some(format!("us120-file|1|{id}|error|files.error.invalid_request").as_str())
            );
        }
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
