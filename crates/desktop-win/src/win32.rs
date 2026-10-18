// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hand-written Win32 bindings and the single-owner HWND lifecycle.

use super::{
    MonitorArea, Rect, WindowState, encode_state, load_state, minimum_size, restore_state,
};
use crate::bridge_ops::{self, BridgeState, JobResult, Outcome, StaHost, Step};
use crate::file_bridge::{self, Request};
use crate::file_dialog::{DialogOwner, FileFilter, StaFileDialogs};
use crate::files::{FileBroker, FileError};
use crate::instance_win32;
use crate::webview::{self, WebView};
use std::ffi::c_void;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

type Hwnd = *mut c_void;
type Hinstance = *mut c_void;
type Hmonitor = *mut c_void;
type Hkey = *mut c_void;
type Lparam = isize;
type Wparam = usize;
type Lresult = isize;

const WM_SIZE: u32 = 0x0005;
const WM_SETFOCUS: u32 = 0x0007;
const WM_CLOSE: u32 = 0x0010;
const WM_DESTROY: u32 = 0x0002;
const WM_NCCREATE: u32 = 0x0081;
const WM_NCDESTROY: u32 = 0x0082;
const WM_DPICHANGED: u32 = 0x02e0;
const WM_SETTINGCHANGE: u32 = 0x001a;
const WM_THEMECHANGED: u32 = 0x031a;
const WM_TIMER: u32 = 0x0113;
const WM_COPYDATA: u32 = 0x004a;
/// Message privé (plage WM_APP) : traiter hors du WM_COPYDATA envoyé les chemins reçus.
const WM_ATELIER_LAUNCH: u32 = 0x8000 + 0x115;
/// Lecture d'un fichier d'ouverture terminée sur le thread de travail.
const WM_ATELIER_LAUNCH_DONE: u32 = 0x8000 + 0x116;
/// US-113 : un travail du pont (lecture, écriture, polices) a fini sur son fil de travail.
const WM_ATELIER_BRIDGE_DONE: u32 = 0x8000 + 0x117;
const WM_GETMINMAXINFO: u32 = 0x0024;
const GWLP_USERDATA: i32 = -21;
const TIMER_WEBVIEW_INIT: usize = 0x1110;
const TIMER_WEBVIEW_CLOSE: usize = 0x1111;
const TIMER_LAUNCH_READ: usize = 0x1112;
/// Diagnostic du pont seulement : ferme le dialogue commun ouvert pendant `--bridge-self-check`.
const TIMER_DIAG_DIALOG_CLOSE: usize = 0x1113;
/// Reprise de la remise des résultats du pont différée pendant un dialogue modal.
const TIMER_BRIDGE_RETRY: usize = 0x1114;
const BRIDGE_RETRY_MS: u32 = 50;
const MAX_BRIDGE_JOBS: usize = 4;
const TIMER_CLOSE_WAIT: usize = 0x1115;
const CLOSE_WAIT_POLL_MS: u32 = 250;
/// Attente maximale des travaux du pont avant de laisser l'utilisateur décider de quitter.
const CLOSE_WAIT_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Décision de fermeture vis-à-vis des travaux du pont encore en cours.
#[derive(Debug, PartialEq, Eq)]
enum CloseGate {
    Proceed,
    Wait,
    /// Délai dépassé : demander à l'utilisateur plutôt que d'abandonner une écriture en silence.
    AskTimeout,
}

fn close_gate(inflight: usize, waited: Option<std::time::Duration>) -> CloseGate {
    if inflight == 0 {
        CloseGate::Proceed
    } else if waited.is_some_and(|waited| waited >= CLOSE_WAIT_LIMIT) {
        CloseGate::AskTimeout
    } else {
        CloseGate::Wait
    }
}
/// Délai maximal de lecture d'un fichier d'ouverture (partage réseau injoignable).
const LAUNCH_READ_TIMEOUT_MS: u32 = 30_000;
const SIZE_MINIMIZED: usize = 1;
const MONITOR_DEFAULTTONEAREST: u32 = 2;
const SW_SHOWNORMAL: i32 = 1;
const SW_SHOWMAXIMIZED: i32 = 3;
const SW_SHOWMINIMIZED: u32 = 2;
const SW_HIDE: u32 = 0;
const WPF_RESTORETOMAXIMIZED: u32 = 0x2;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const WS_OVERLAPPEDWINDOW: u32 = 0x00cf0000;
const WS_EX_APPWINDOW: u32 = 0x00040000;
const CS_HREDRAW: u32 = 0x0002;
const CS_VREDRAW: u32 = 0x0001;
const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;
const HKEY_CURRENT_USER: Hkey = (-2147483647isize) as Hkey;
const RRF_RT_REG_DWORD: u32 = 0x00000010;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_USE_IMMERSIVE_DARK_MODE_LEGACY: u32 = 19;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NativeRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    w_param: Wparam,
    l_param: Lparam,
    time: u32,
    point: Point,
    private: u32,
}
#[repr(C)]
struct WndClassW {
    style: u32,
    proc: Option<unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lresult>,
    class_extra: i32,
    window_extra: i32,
    instance: Hinstance,
    icon: *mut c_void,
    cursor: *mut c_void,
    background: *mut c_void,
    menu_name: *const u16,
    class_name: *const u16,
}
#[repr(C)]
struct CreateStructW {
    create_params: *mut c_void,
    instance: Hinstance,
    menu: *mut c_void,
    parent: Hwnd,
    height: i32,
    width: i32,
    y: i32,
    x: i32,
    style: i32,
    name: *const u16,
    class: *const u16,
    ex_style: u32,
}
#[repr(C)]
struct WindowPlacement {
    length: u32,
    flags: u32,
    show_cmd: u32,
    min_position: Point,
    max_position: Point,
    normal_position: NativeRect,
}
#[repr(C)]
struct MonitorInfo {
    size: u32,
    monitor: NativeRect,
    work: NativeRect,
    flags: u32,
}
#[repr(C)]
struct MinMaxInfo {
    reserved: Point,
    max_size: Point,
    max_position: Point,
    min_track_size: Point,
    max_track_size: Point,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetProcessDpiAwarenessContext(context: isize) -> i32;
    fn GetThreadDpiAwarenessContext() -> isize;
    fn AreDpiAwarenessContextsEqual(first: isize, second: isize) -> i32;
    fn GetDpiForSystem() -> u32;
    fn GetDpiForWindow(hwnd: Hwnd) -> u32;
    fn GetModuleHandleW(name: *const u16) -> Hinstance;
    fn RegisterClassW(class: *const WndClassW) -> u16;
    fn UnregisterClassW(name: *const u16, instance: Hinstance) -> i32;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: *mut c_void,
        instance: Hinstance,
        param: *mut c_void,
    ) -> Hwnd;
    fn DestroyWindow(hwnd: Hwnd) -> i32;
    fn DefWindowProcW(hwnd: Hwnd, message: u32, w: Wparam, l: Lparam) -> Lresult;
    fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
    fn GetMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> Lresult;
    fn PeekMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32, remove: u32) -> i32;
    fn PostQuitMessage(code: i32);
    fn GetProcessHandleCount(process: *mut c_void, count: *mut u32) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn MsgWaitForMultipleObjectsEx(
        count: u32,
        handles: *const Hwnd,
        milliseconds: u32,
        wake_mask: u32,
        flags: u32,
    ) -> u32;
    fn SetTimer(hwnd: Hwnd, id: usize, milliseconds: u32, callback: *mut c_void) -> usize;
    fn KillTimer(hwnd: Hwnd, id: usize) -> i32;
    fn GetLastActivePopup(hwnd: Hwnd) -> Hwnd;
    fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
    fn PostMessageW(hwnd: Hwnd, message: u32, w: Wparam, l: Lparam) -> i32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn UpdateWindow(hwnd: Hwnd) -> i32;
    fn GetClientRect(hwnd: Hwnd, rect: *mut NativeRect) -> i32;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut NativeRect) -> i32;
    fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    fn GetWindowPlacement(hwnd: Hwnd, placement: *mut WindowPlacement) -> i32;
    fn SetWindowPlacement(hwnd: Hwnd, placement: *const WindowPlacement) -> i32;
    fn MonitorFromRect(rect: *const NativeRect, flags: u32) -> Hmonitor;
    fn GetMonitorInfoW(monitor: Hmonitor, info: *mut MonitorInfo) -> i32;
    fn EnumDisplayMonitors(
        dc: *mut c_void,
        clip: *const NativeRect,
        callback: Option<
            unsafe extern "system" fn(Hmonitor, *mut c_void, *mut NativeRect, isize) -> i32,
        >,
        data: isize,
    ) -> i32;
    fn SendMessageW(hwnd: Hwnd, message: u32, w: Wparam, l: Lparam) -> Lresult;
    fn MessageBoxW(hwnd: Hwnd, text: *const u16, caption: *const u16, kind: u32) -> i32;
    fn GetLastError() -> u32;
    fn GetCurrentProcessId() -> u32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: Hwnd, attribute: u32, value: *const c_void, size: u32) -> i32;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegGetValueW(
        key: Hkey,
        subkey: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut c_void,
        size: *mut u32,
    ) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
}

#[path = "bridge-check.rs"]
mod bridge_check;
pub use bridge_check::bridge_self_check;

struct Context {
    state_file: PathBuf,
    title: Vec<u16>,
    client: (i32, i32),
    dpi: u32,
    persisted: bool,
    callback_error: Option<io::Error>,
    webview: Option<WebView>,
    close_requested: bool,
    minimized: bool,
    file_runtime: Option<FileRuntime>,
}

struct FileRuntime {
    broker: Arc<FileBroker>,
    dialogs: StaFileDialogs,
    dirty: bool,
    /// Chemins demandés (ligne de commande, seconde instance) pas encore réclamés par le front.
    launch: std::collections::VecDeque<PathBuf>,
    /// Lecture en cours hors du fil UI : (génération, id de la requête).
    launch_inflight: Option<(u64, String)>,
    launch_generation: u64,
    launch_slot: Arc<Mutex<Option<(u64, file_bridge::LaunchRead)>>>,
    /// US-113 : tickets remis au JS, écriture en cours et catalogue de polices.
    bridge: BridgeState,
    /// Résultats des travaux du pont terminés sur un fil de travail, en attente de remise.
    bridge_done: Arc<Mutex<std::collections::VecDeque<JobResult>>>,
    bridge_inflight: usize,
    /// Début de l'attente de fin des travaux du pont après un WM_CLOSE différé.
    close_wait_since: Option<std::time::Instant>,
}

impl FileRuntime {
    fn new() -> Result<Self, crate::files::FileError> {
        Ok(Self {
            broker: Arc::new(FileBroker::new()),
            dialogs: StaFileDialogs::initialize_current_thread()?,
            dirty: false,
            launch: std::collections::VecDeque::new(),
            launch_inflight: None,
            launch_generation: 0,
            launch_slot: Arc::new(Mutex::new(None)),
            bridge: BridgeState::system(),
            bridge_done: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            bridge_inflight: 0,
            close_wait_since: None,
        })
    }
}

fn file_reply(id: &str, result: Result<Option<Vec<u8>>, &'static str>) -> String {
    match result {
        Ok(Some(bytes)) => format!(
            "us120-file|1|{id}|ok|{}",
            file_bridge::encode_base64(&bytes)
        ),
        Ok(None) => format!("us120-file|1|{id}|cancelled|"),
        Err(key) => format!("us120-file|1|{id}|error|{key}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileDialogKind {
    Open,
    OpenDocx,
    Save,
}

/// Shared native routing for production dialogs and deterministic path-injected
/// tests. Only trusted dialog code supplies `choose`.
fn route_file_request(
    request: Request,
    broker: &FileBroker,
    mut choose: impl FnMut(FileDialogKind) -> Result<Option<PathBuf>, &'static str>,
) -> String {
    match request {
        Request::Open { id } => match choose(FileDialogKind::Open) {
            Ok(selected) => file_bridge::process_selection(Request::Open { id }, selected, broker),
            Err(key) => file_reply(&id, Err(key)),
        },
        Request::OpenDocx { id } => match choose(FileDialogKind::OpenDocx) {
            Ok(selected) => {
                file_bridge::process_selection(Request::OpenDocx { id }, selected, broker)
            }
            Err(key) => file_reply(&id, Err(key)),
        },
        Request::Save { id, bytes } => match choose(FileDialogKind::Save) {
            Ok(selected) => {
                file_bridge::process_selection(Request::Save { id, bytes }, selected, broker)
            }
            Err(key) => file_reply(&id, Err(key)),
        },
        _ => unreachable!("dirty, launch and bridge messages are handled before dialog routing"),
    }
}

/// Build and pass the exact dialog options used by the production dispatcher.
/// Keeping option selection behind this seam lets tests inspect filters without
/// showing an interactive native dialog.
fn with_file_dialog_options(
    kind: FileDialogKind,
    show: impl FnOnce(&str, &[FileFilter], usize, Option<&str>) -> Result<Option<PathBuf>, FileError>,
) -> Result<Option<PathBuf>, FileError> {
    let (title, label, extensions, suggested_name) = match kind {
        FileDialogKind::Open => ("Ouvrir un fichier texte", "Fichier texte", ["txt"], None),
        FileDialogKind::OpenDocx => ("Ouvrir un document Word", "Document Word", ["docx"], None),
        FileDialogKind::Save => (
            "Enregistrer le fichier texte",
            "Fichier texte",
            ["txt"],
            Some("Sans titre.txt"),
        ),
    };
    let filter = FileFilter::new(label, extensions)?;
    show(title, std::slice::from_ref(&filter), 0, suggested_name)
}

/// US-115 : démarre la lecture du prochain fichier d'ouverture sur un thread de travail (un chemin UNC
/// lent ne doit pas figer l'interface). Renvoie une réponse immédiate si rien n'attend ou si une lecture
/// est déjà en cours ; sinon `None` et la réponse part à `WM_ATELIER_LAUNCH_DONE` ou à l'expiration.
fn begin_launch(hwnd: Hwnd, runtime: &mut FileRuntime, id: &str) -> Option<Outcome> {
    if runtime.launch_inflight.is_some() {
        return Some(Outcome::Text(format!(
            "us120-file|1|{id}|error|files.error.queue_full"
        )));
    }
    let Some(path) = runtime.launch.pop_front() else {
        return Some(Outcome::Text(format!("us120-file|1|{id}|ok|")));
    };
    runtime.launch_generation += 1;
    let generation = runtime.launch_generation;
    let slot = Arc::clone(&runtime.launch_slot);
    let target = hwnd as usize;
    let spawned = std::thread::Builder::new()
        .name("atelier-launch-read".into())
        .spawn(move || {
            let read = file_bridge::read_launch_file(&path);
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some((generation, read));
            // SAFETY: PostMessageW est sûr sur tout thread ; un HWND détruit fait seulement échouer l'appel.
            unsafe { PostMessageW(target as Hwnd, WM_ATELIER_LAUNCH_DONE, 0, 0) };
        });
    if spawned.is_err() {
        return Some(Outcome::Text(format!(
            "us120-file|1|{id}|error|files.error.io"
        )));
    }
    runtime.launch_inflight = Some((generation, id.to_owned()));
    // SAFETY: hwnd est la fenêtre vivante du fil UI ; sans minuterie la lecture reste simplement sans délai.
    unsafe { SetTimer(hwnd, TIMER_LAUNCH_READ, LAUNCH_READ_TIMEOUT_MS, null_mut()) };
    None
}

/// Réponse due à la lecture terminée (`expired = false`) ou expirée ; `None` si rien n'est attendu.
/// Un résultat tardif d'une génération abandonnée est ignoré.
fn finish_launch(runtime: &mut FileRuntime, expired: bool) -> Option<Outcome> {
    let (generation, id) = runtime.launch_inflight.clone()?;
    let ready = runtime
        .launch_slot
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    match ready {
        Some((done, read)) if done == generation => {
            runtime.launch_inflight = None;
            Some(file_bridge::launch_reply(&id, read, &mut runtime.launch))
        }
        _ if expired => {
            runtime.launch_inflight = None;
            Some(Outcome::Text(format!(
                "us120-file|1|{id}|error|files.error.timeout"
            )))
        }
        _ => None,
    }
}

fn process_file_request(
    hwnd: Hwnd,
    runtime: &mut FileRuntime,
    request: Request,
) -> Option<Outcome> {
    let id = request.id().to_owned();
    if let Request::Dirty { dirty, .. } = request {
        runtime.dirty = dirty;
        return Some(Outcome::Text(format!("us120-file|1|{id}|ok|")));
    }
    if let Request::Launch { .. } = request {
        unreachable!("launch requests are started by begin_launch");
    }
    // SAFETY: this handler runs on the UI STA while hwnd is the live owner window.
    let owner = unsafe { DialogOwner::from_raw(hwnd) };
    let broker = &runtime.broker;
    let dialogs = &mut runtime.dialogs;
    if matches!(
        request,
        Request::PickOpen { .. }
            | Request::PickSave { .. }
            | Request::ReadTicket { .. }
            | Request::ReleaseTicket { .. }
            | Request::WriteTicket { .. }
            | Request::ListFonts { .. }
            | Request::ReadFont { .. }
            | Request::OpenUrl { .. }
    ) {
        let mut host = StaHost { dialogs, owner };
        return match bridge_ops::begin_bridge_request(
            request,
            &mut runtime.bridge,
            broker,
            &mut host,
        ) {
            Step::Done(outcome) => Some(outcome),
            Step::Job(job) => spawn_bridge_job(
                hwnd,
                &mut runtime.bridge_inflight,
                &runtime.bridge_done,
                id,
                job,
            ),
        };
    }
    Some(Outcome::Text(route_file_request(request, broker, |kind| {
        with_file_dialog_options(
            kind,
            |title, filters, selected_filter, suggested_name| match kind {
                FileDialogKind::Open | FileDialogKind::OpenDocx => {
                    dialogs.open(owner.as_ref(), title, filters, selected_filter)
                }
                FileDialogKind::Save => dialogs.save(
                    owner.as_ref(),
                    title,
                    filters,
                    selected_filter,
                    suggested_name,
                ),
            },
        )
        .map_err(|error| error.message_key)
    })))
}

/// Lance un travail bloquant du pont sur un fil de travail ; sa réponse est postée à la fenêtre par
/// `WM_ATELIER_BRIDGE_DONE`. Renvoie une réponse immédiate seulement en cas de refus.
fn spawn_bridge_job(
    hwnd: Hwnd,
    inflight: &mut usize,
    done: &Arc<Mutex<std::collections::VecDeque<JobResult>>>,
    id: String,
    job: bridge_ops::Job,
) -> Option<Outcome> {
    if *inflight >= MAX_BRIDGE_JOBS {
        return Some(Outcome::Text(format!(
            "us120-file|1|{id}|error|files.error.queue_full"
        )));
    }
    let slot = Arc::clone(done);
    let target = hwnd as usize;
    let spawned = std::thread::Builder::new()
        .name("atelier-bridge-job".into())
        .spawn(move || {
            let result = job();
            slot.lock()
                .unwrap_or_else(|e| e.into_inner())
                .push_back(result);
            // SAFETY: PostMessageW est sûr sur tout thread ; un HWND détruit fait seulement échouer l'appel.
            unsafe { PostMessageW(target as Hwnd, WM_ATELIER_BRIDGE_DONE, 0, 0) };
        });
    if spawned.is_err() {
        return Some(Outcome::Text(format!(
            "us120-file|1|{id}|error|files.error.io"
        )));
    }
    *inflight += 1;
    None
}

/// Réponses des travaux terminés. Différé (`None`) tant qu'un dialogue ou un traitement de messages
/// détient l'état du pont : la boucle modale d'un dialogue peut distribuer ce message.
fn take_bridge_results(context: *mut Context) -> Option<Vec<Outcome>> {
    // SAFETY: appelé depuis la procédure de fenêtre sur le fil UI avec le contexte vivant.
    unsafe {
        let busy = (*context)
            .webview
            .as_ref()
            .is_some_and(WebView::file_work_active);
        if busy {
            return None;
        }
        let runtime = (*context).file_runtime.as_mut()?;
        let results: Vec<JobResult> = runtime
            .bridge_done
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        runtime.bridge_inflight = runtime.bridge_inflight.saturating_sub(results.len());
        Some(
            results
                .into_iter()
                .map(|result| runtime.bridge.finish(result))
                .collect(),
        )
    }
}

impl Context {
    fn record(&mut self, result: io::Result<()>) {
        if let Err(error) = result {
            self.callback_error.get_or_insert(error);
        }
    }
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn error_from_last(operation: &str) -> io::Error {
    // SAFETY: GetLastError only reads the calling thread's Win32 error slot and accepts no pointers.
    let code = unsafe { GetLastError() };
    io::Error::other(format!("{operation} failed with Win32 error {code}"))
}
fn hresult(operation: &str, code: i32) -> io::Result<()> {
    if code >= 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{operation} failed with HRESULT 0x{:08x}",
            code as u32
        )))
    }
}
fn to_native(r: Rect) -> NativeRect {
    NativeRect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}
fn from_native(r: NativeRect) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

pub fn initialize_dpi() -> io::Result<()> {
    // SAFETY: this process-wide setting is called by main before any HWND exists.
    if unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) } != 0 {
        return Ok(());
    }
    // SAFETY: reads this thread's error slot immediately after SetProcessDpiAwarenessContext failed.
    let error_code = unsafe { GetLastError() };
    // ERROR_ACCESS_DENIED means another component set awareness first; accept only if the process is already PMv2.
    if error_code == 5 {
        // SAFETY: these APIs only query the current thread's DPI context.
        let current = unsafe { GetThreadDpiAwarenessContext() };
        if unsafe {
            AreDpiAwarenessContextsEqual(current, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
        } != 0
        {
            return Ok(());
        }
    }
    Err(io::Error::other(format!(
        "SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2) failed with Win32 error {error_code}"
    )))
}

fn monitors() -> io::Result<Vec<MonitorArea>> {
    unsafe extern "system" fn collect(
        monitor: Hmonitor,
        _: *mut c_void,
        _: *mut NativeRect,
        data: isize,
    ) -> i32 {
        // SAFETY: data points to the Vec owned by monitors() for the duration of EnumDisplayMonitors.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            let output = &mut *(data as *mut Vec<MonitorArea>);
            let mut info = MonitorInfo {
                size: std::mem::size_of::<MonitorInfo>() as u32,
                monitor: NativeRect::default(),
                work: NativeRect::default(),
                flags: 0,
            };
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return false;
            }
            // SAFETY: GetDpiForSystem is a pointer-free query of this process's system DPI context.
            let x_dpi = GetDpiForSystem().clamp(48, 960);
            output.push(MonitorArea {
                monitor: from_native(info.monitor),
                work: from_native(info.work),
                dpi: x_dpi,
            });
            true
        }));
        i32::from(matches!(result, Ok(true)))
    }
    let mut result = Vec::new();
    // SAFETY: callback data is a live Vec pointer and callback does not retain it.
    if unsafe {
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(collect),
            &mut result as *mut _ as isize,
        )
    } == 0
        || result.is_empty()
    {
        return Err(error_from_last("EnumDisplayMonitors"));
    }
    Ok(result)
}

fn monitor_info_for_rect(rect: Rect) -> io::Result<MonitorInfo> {
    let native = to_native(rect);
    // SAFETY: native is valid for the duration of the call; GetMonitorInfo initializes the sized structure.
    let monitor = unsafe { MonitorFromRect(&native, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() {
        return Err(error_from_last("MonitorFromRect"));
    }
    let mut info = MonitorInfo {
        size: std::mem::size_of::<MonitorInfo>() as u32,
        monitor: NativeRect::default(),
        work: NativeRect::default(),
        flags: 0,
    };
    // SAFETY: monitor is a live HMONITOR from MonitorFromRect, info has the required size.
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return Err(error_from_last("GetMonitorInfoW"));
    }
    Ok(info)
}

fn workspace_to_screen(rect: NativeRect, info: MonitorInfo) -> Rect {
    let dx = info.work.left - info.monitor.left;
    let dy = info.work.top - info.monitor.top;
    Rect {
        left: rect.left.saturating_add(dx),
        top: rect.top.saturating_add(dy),
        right: rect.right.saturating_add(dx),
        bottom: rect.bottom.saturating_add(dy),
    }
}
fn screen_to_workspace(rect: Rect, info: MonitorInfo) -> NativeRect {
    let dx = info.work.left - info.monitor.left;
    let dy = info.work.top - info.monitor.top;
    NativeRect {
        left: rect.left.saturating_sub(dx),
        top: rect.top.saturating_sub(dy),
        right: rect.right.saturating_sub(dx),
        bottom: rect.bottom.saturating_sub(dy),
    }
}

fn capture(hwnd: Hwnd, context: &Context) -> io::Result<WindowState> {
    let mut placement = WindowPlacement {
        length: std::mem::size_of::<WindowPlacement>() as u32,
        flags: 0,
        show_cmd: 0,
        min_position: Point::default(),
        max_position: Point::default(),
        normal_position: NativeRect::default(),
    };
    // SAFETY: hwnd is owned by this run and placement advertises its exact SDK-compatible size.
    if unsafe { GetWindowPlacement(hwnd, &mut placement) } == 0 {
        return Err(error_from_last("GetWindowPlacement"));
    }
    let mut current = NativeRect::default();
    // SAFETY: hwnd is owned by this run and current is writable storage for GetWindowRect.
    if unsafe { GetWindowRect(hwnd, &mut current) } == 0 {
        return Err(error_from_last("GetWindowRect"));
    }
    let info = monitor_info_for_rect(from_native(current))?;
    let client = Rect {
        left: placement.normal_position.left,
        top: placement.normal_position.top,
        right: placement.normal_position.right,
        bottom: placement.normal_position.bottom,
    };
    let normal = workspace_to_screen(to_native(client), info);
    Ok(WindowState {
        normal,
        dpi: context.dpi.max(48),
        maximized: placement_is_maximized(placement.show_cmd, placement.flags),
    })
}

fn placement_is_maximized(show_cmd: u32, flags: u32) -> bool {
    show_cmd == SW_SHOWMAXIMIZED as u32
        || (show_cmd == SW_SHOWMINIMIZED && flags & WPF_RESTORETOMAXIMIZED != 0)
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "state path must have a file name",
            )
        })?
        .to_string_lossy();
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    // SAFETY: GetCurrentProcessId is a pointer-free query returning the current process identifier.
    let temp = parent.join(format!(
        ".{name}.{}.{}.tmp",
        unsafe { GetCurrentProcessId() },
        sequence
    ));
    atomic_write_to_temp(path, &temp, bytes)
}

fn atomic_write_to_temp(path: &Path, temp: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    // A collision belongs to another file. Return immediately without cleaning it up.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        let from = wide(temp.as_os_str());
        let to = wide(path.as_os_str());
        // SAFETY: both NUL-terminated UTF-16 paths live through the call; source and destination share a directory.
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(error_from_last("MoveFileExW(atomic state replace)"));
        }
        Ok(())
    })();
    if result.is_err() && temp.exists() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

fn save(hwnd: Hwnd, context: &Context) -> io::Result<()> {
    atomic_write(&context.state_file, &encode_state(capture(hwnd, context)?))
}

fn system_prefers_dark() -> bool {
    let key = wide(std::ffi::OsStr::new(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
    ));
    let name = wide(std::ffi::OsStr::new("AppsUseLightTheme"));
    let mut value = 1u32;
    let mut kind = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: key/name are NUL-terminated and value/kind/size point to initialized writable storage.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            &mut kind,
            &mut value as *mut _ as *mut c_void,
            &mut size,
        )
    };
    status == 0 && kind == 4 && value == 0
}
fn apply_theme(hwnd: Hwnd) -> io::Result<()> {
    let dark: i32 = i32::from(system_prefers_dark());
    // SAFETY: hwnd is the live window handled by this callback/run; DWM reads the initialized BOOL
    // synchronously and does not retain its stack pointer.
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const _ as *const c_void,
            std::mem::size_of_val(&dark) as u32,
        )
    };
    if result >= 0 {
        return Ok(());
    }
    // Windows 10 releases expose the legacy attribute identifier.
    // SAFETY: same live HWND and synchronous BOOL lifetime as the current DWM attribute call above.
    hresult(
        "DwmSetWindowAttribute(DWMWA_USE_IMMERSIVE_DARK_MODE)",
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE_LEGACY,
                &dark as *const _ as *const c_void,
                std::mem::size_of_val(&dark) as u32,
            )
        },
    )
}

fn context_from(hwnd: Hwnd) -> *mut Context {
    // SAFETY: GWLP_USERDATA is installed from WM_NCCREATE and points to the Context owned by run/self_check;
    // that Box remains alive until after DestroyWindow returns, and callbacks execute on that same thread.
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Context }
}

unsafe extern "system" fn window_proc(hwnd: Hwnd, message: u32, w: Wparam, l: Lparam) -> Lresult {
    // SAFETY: Windows invokes this callback with a live HWND; lParam is dereferenced only for
    // WM_NCCREATE, WM_DPICHANGED, and WM_GETMINMAXINFO after their documented non-null checks.
    // catch_unwind contains Rust panics so none unwind across the Win32 callback ABI.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        if message == WM_NCCREATE {
            let create = &*(l as *const CreateStructW);
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.create_params as isize);
        }
        let context = context_from(hwnd);
        if !context.is_null() {
            match message {
                WM_SIZE => {
                    let mut client = NativeRect::default();
                    if GetClientRect(hwnd, &mut client) != 0 {
                        (*context).client = (
                            client.right.saturating_sub(client.left),
                            client.bottom.saturating_sub(client.top),
                        );
                        (*context).minimized = w == SIZE_MINIMIZED;
                        if let Some(webview) = (*context).webview.as_ref() {
                            (*context).record(webview.set_bounds(
                                client.right - client.left,
                                client.bottom - client.top,
                            ));
                            (*context).record(webview.set_visible(!(*context).minimized));
                        }
                    }
                }
                WM_SETFOCUS => {
                    if let Some(webview) = (*context).webview.as_ref() {
                        (*context).record(webview.move_focus_programmatic());
                    }
                }
                WM_DPICHANGED => {
                    (*context).dpi = ((w >> 16) as u32).clamp(48, 960);
                    if l != 0 {
                        // SAFETY: WM_DPICHANGED guarantees l points to a RECT valid during this callback; copy before using.
                        let suggested = *(l as *const NativeRect);
                        if SetWindowPos(
                            hwnd,
                            null_mut(),
                            suggested.left,
                            suggested.top,
                            suggested.right - suggested.left,
                            suggested.bottom - suggested.top,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        ) == 0
                        {
                            (*context).record(Err(error_from_last("SetWindowPos(WM_DPICHANGED)")));
                        } else if let Some(webview) = (*context).webview.as_ref() {
                            let mut client = NativeRect::default();
                            if GetClientRect(hwnd, &mut client) != 0 {
                                (*context).client = (
                                    client.right.saturating_sub(client.left),
                                    client.bottom.saturating_sub(client.top),
                                );
                                (*context).record(webview.set_bounds(
                                    client.right - client.left,
                                    client.bottom - client.top,
                                ));
                            } else {
                                (*context)
                                    .record(Err(error_from_last("GetClientRect(WM_DPICHANGED)")));
                            }
                        }
                    }
                }
                webview::WEBVIEW_MESSAGE => {
                    // Common file dialogs pump this STA. An owned guard prevents
                    // nested wakes from entering the WebView or file runtime.
                    let Some(_file_work) = (*context)
                        .webview
                        .as_ref()
                        .and_then(WebView::begin_file_work)
                    else {
                        return 0;
                    };
                    if let Some(webview) = (*context).webview.as_mut() {
                        match webview.take_completed() {
                            Ok(ready) if ready => {
                                let _ = KillTimer(hwnd, TIMER_WEBVIEW_INIT);
                                let mut client = NativeRect::default();
                                if GetClientRect(hwnd, &mut client) != 0 {
                                    (*context).client = (
                                        client.right.saturating_sub(client.left),
                                        client.bottom.saturating_sub(client.top),
                                    );
                                    (*context).record(webview.set_bounds(
                                        client.right - client.left,
                                        client.bottom - client.top,
                                    ));
                                    (*context).record(webview.set_visible(!(*context).minimized));
                                } else {
                                    (*context).record(Err(error_from_last(
                                        "GetClientRect(WebView2 ready)",
                                    )));
                                }
                            }
                            Ok(_) => {}
                            Err(error) => {
                                (*context).record(Err(error));
                                if let Some(webview) = (*context).webview.as_mut() {
                                    webview.close();
                                    webview.detach_window();
                                }
                                let _ = DestroyWindow(hwnd);
                                return 0;
                            }
                        }
                    }
                    let rejections: Vec<String> = (*context)
                        .webview
                        .as_ref()
                        .map(|view| std::iter::from_fn(|| view.take_file_rejection()).collect())
                        .unwrap_or_default();
                    for reply in rejections {
                        if let Some(view) = (*context).webview.as_ref() {
                            if let Err(error) = view.post_web_message_as_string(&reply) {
                                (*context).record(Err(error));
                            }
                        }
                    }
                    let requests: Vec<Request> = (*context)
                        .webview
                        .as_ref()
                        .map(|view| std::iter::from_fn(|| view.take_file_message()).collect())
                        .unwrap_or_default();
                    for request in requests {
                        let reply = (*context).file_runtime.as_mut().and_then(|runtime| {
                            if let Request::Launch { id } = &request {
                                begin_launch(hwnd, runtime, id)
                            } else {
                                process_file_request(hwnd, runtime, request)
                            }
                        });
                        if let (Some(reply), Some(view)) = (reply, (*context).webview.as_ref()) {
                            if let Err(error) = bridge_ops::deliver(view, reply) {
                                (*context).record(Err(error));
                            }
                        }
                    }
                    if (*context).close_requested
                        && (*context)
                            .webview
                            .as_ref()
                            .is_none_or(|view| view.pending_callbacks() == 0)
                    {
                        let _ = KillTimer(hwnd, TIMER_WEBVIEW_CLOSE);
                        if let Some(view) = (*context).webview.as_ref() {
                            view.detach_window();
                        }
                        if DestroyWindow(hwnd) == 0 {
                            (*context).record(Err(error_from_last(
                                "DestroyWindow(after WebView2 callback drain)",
                            )));
                            PostQuitMessage(1);
                        }
                        return 0;
                    }
                }
                WM_COPYDATA => {
                    // US-115 : chemins d'une seconde instance. Le décodage valide provenance, protocole,
                    // taille et UTF-16 ; une charge invalide est refusée (0) sans effet. Le travail
                    // réel est reporté à WM_ATELIER_LAUNCH car ce message est envoyé par un autre thread.
                    // SAFETY (bloc unsafe englobant) : l est le lParam de ce WM_COPYDATA.
                    return match instance_win32::decode_copydata(w, l) {
                        Some(paths) => {
                            let mut dropped = false;
                            if let Some(runtime) = (*context).file_runtime.as_mut() {
                                for path in paths {
                                    if runtime.launch.len() >= crate::instance::MAX_PATHS {
                                        dropped = true;
                                        break;
                                    }
                                    runtime.launch.push_back(path);
                                }
                            }
                            // SAFETY: hwnd est la fenêtre vivante ; message privé sans pointeur.
                            PostMessageW(hwnd, WM_ATELIER_LAUNCH, 0, 0);
                            // File pleine : les chemins acceptés restent, l'émetteur apprend le refus.
                            if dropped { 0 } else { 1 }
                        }
                        None => 0,
                    };
                }
                WM_ATELIER_BRIDGE_DONE => {
                    match take_bridge_results(context) {
                        Some(outcomes) => {
                            if let Some(view) = (*context).webview.as_ref() {
                                for outcome in outcomes {
                                    if let Err(error) = bridge_ops::deliver(view, outcome) {
                                        (*context).record(Err(error));
                                    }
                                }
                            }
                            // Fermeture différée : le dernier travail est fini, on la relance.
                            if (*context).file_runtime.as_ref().is_some_and(|runtime| {
                                runtime.close_wait_since.is_some() && runtime.bridge_inflight == 0
                            }) {
                                // SAFETY: hwnd est la fenêtre vivante ; message sans pointeur.
                                PostMessageW(hwnd, WM_CLOSE, 0, 0);
                            }
                        }
                        None => {
                            SetTimer(hwnd, TIMER_BRIDGE_RETRY, BRIDGE_RETRY_MS, null_mut());
                        }
                    }
                    return 0;
                }
                WM_ATELIER_LAUNCH_DONE => {
                    let reply = (*context)
                        .file_runtime
                        .as_mut()
                        .and_then(|runtime| finish_launch(runtime, false));
                    if let Some(reply) = reply {
                        let _ = KillTimer(hwnd, TIMER_LAUNCH_READ);
                        if let Some(view) = (*context).webview.as_ref() {
                            let _ = bridge_ops::deliver(view, reply);
                        }
                    }
                    return 0;
                }
                WM_ATELIER_LAUNCH => {
                    instance_win32::bring_to_front(hwnd);
                    // Échec ignoré : le front réclame aussi les chemins à son démarrage.
                    if let Some(view) = (*context).webview.as_ref() {
                        let _ = view.post_web_message_as_string("us115-launch|1");
                    }
                    return 0;
                }
                WM_TIMER => {
                    if w == TIMER_CLOSE_WAIT {
                        let _ = KillTimer(hwnd, TIMER_CLOSE_WAIT);
                        PostMessageW(hwnd, WM_CLOSE, 0, 0);
                        return 0;
                    }
                    if w == TIMER_BRIDGE_RETRY {
                        let _ = KillTimer(hwnd, TIMER_BRIDGE_RETRY);
                        PostMessageW(hwnd, WM_ATELIER_BRIDGE_DONE, 0, 0);
                        return 0;
                    }
                    if w == TIMER_DIAG_DIALOG_CLOSE {
                        // Armé uniquement par `--bridge-self-check` : ferme le dialogue commun
                        // éventuellement ouvert (WM_CLOSE = annulation), sans effet sinon.
                        // SAFETY: hwnd est la fenêtre vivante ; GetLastActivePopup lit seulement la chaîne
                        // de propriété et PostMessageW accepte tout HWND (échec inoffensif).
                        let popup = GetLastActivePopup(hwnd);
                        if !popup.is_null() && popup != hwnd {
                            PostMessageW(popup, WM_CLOSE, 0, 0);
                        }
                        return 0;
                    }
                    if w == TIMER_LAUNCH_READ {
                        let _ = KillTimer(hwnd, TIMER_LAUNCH_READ);
                        let reply = (*context)
                            .file_runtime
                            .as_mut()
                            .and_then(|runtime| finish_launch(runtime, true));
                        if let (Some(reply), Some(view)) = (reply, (*context).webview.as_ref()) {
                            let _ = bridge_ops::deliver(view, reply);
                        }
                        return 0;
                    }
                    if w == TIMER_WEBVIEW_INIT {
                        let _ = KillTimer(hwnd, TIMER_WEBVIEW_INIT);
                        (*context).record(Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "WebView2 initialization callback timed out",
                        )));
                        if let Some(webview) = (*context).webview.as_mut() {
                            webview.close();
                            webview.detach_window();
                        }
                        let _ = DestroyWindow(hwnd);
                        return 0;
                    }
                    if w == TIMER_WEBVIEW_CLOSE {
                        let _ = KillTimer(hwnd, TIMER_WEBVIEW_CLOSE);
                        let pending = (*context)
                            .webview
                            .as_ref()
                            .map_or(0, |view| view.pending_callbacks());
                        if pending != 0 {
                            (*context).record(Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                format!("WebView2 close timed out with {pending} runtime callback(s) still owned by WebView2"),
                            )));
                        }
                        if let Some(webview) = (*context).webview.as_ref() {
                            webview.detach_window();
                        }
                        let _ = DestroyWindow(hwnd);
                        return 0;
                    }
                }
                WM_SETTINGCHANGE | WM_THEMECHANGED => (*context).record(apply_theme(hwnd)),
                WM_GETMINMAXINFO => {
                    if l != 0 {
                        let info = &mut *(l as *mut MinMaxInfo);
                        let mut rect = NativeRect::default();
                        // SAFETY: hwnd is the live window receiving WM_GETMINMAXINFO; rect is writable output storage.
                        if GetWindowRect(hwnd, &mut rect) != 0 {
                            if let Ok(monitor) = monitor_info_for_rect(from_native(rect)) {
                                // SAFETY: hwnd is the live window receiving this message.
                                let dpi = GetDpiForWindow(hwnd).clamp(48, 960);
                                let work = Rect {
                                    left: monitor.work.left,
                                    top: monitor.work.top,
                                    right: monitor.work.right,
                                    bottom: monitor.work.bottom,
                                };
                                let (width, height) = minimum_size(work, dpi);
                                info.min_track_size.x = width;
                                info.min_track_size.y = height;
                            }
                        }
                    }
                }
                WM_CLOSE => {
                    let _ = KillTimer(hwnd, TIMER_CLOSE_WAIT);
                    let gate =
                        (*context)
                            .file_runtime
                            .as_mut()
                            .map_or(CloseGate::Proceed, |runtime| {
                                let waited = runtime.close_wait_since.map(|since| since.elapsed());
                                let gate = close_gate(runtime.bridge_inflight, waited);
                                if gate == CloseGate::Wait && runtime.close_wait_since.is_none() {
                                    runtime.close_wait_since = Some(std::time::Instant::now());
                                }
                                gate
                            });
                    match gate {
                        CloseGate::Wait => {
                            // Un travail du pont (écriture atomique, lecture) tourne : on attend sa fin
                            // (relance par BRIDGE_DONE) ; la minuterie borne l'attente.
                            SetTimer(hwnd, TIMER_CLOSE_WAIT, CLOSE_WAIT_POLL_MS, null_mut());
                            return 0;
                        }
                        CloseGate::AskTimeout => {
                            let prompt: Vec<u16> = "Un enregistrement ou une lecture est toujours en cours. Quitter quand même ? Une écriture interrompue peut être perdue."
                                .encode_utf16().chain(Some(0)).collect();
                            let caption: Vec<u16> =
                                "Atelier".encode_utf16().chain(Some(0)).collect();
                            // SAFETY: hwnd est la fenêtre propriétaire vivante, chaînes terminées par NUL pour cet appel synchrone.
                            let answer = MessageBoxW(
                                hwnd,
                                prompt.as_ptr(),
                                caption.as_ptr(),
                                0x0000_0024 | 0x0000_0100,
                            );
                            if let Some(runtime) = (*context).file_runtime.as_mut() {
                                runtime.close_wait_since = None;
                            }
                            if answer != 6 {
                                return 0;
                            }
                        }
                        CloseGate::Proceed => {
                            if let Some(runtime) = (*context).file_runtime.as_mut() {
                                runtime.close_wait_since = None;
                            }
                        }
                    }
                    if let Some(view) = (*context).webview.as_ref()
                        && view.file_work_active()
                    {
                        view.defer_close_until_file_work_finishes();
                        return 0;
                    }
                    let dirty_hint = (*context)
                        .webview
                        .as_ref()
                        .and_then(WebView::pending_dirty_hint);
                    let dirty = dirty_hint.unwrap_or_else(|| {
                        (*context)
                            .file_runtime
                            .as_ref()
                            .is_some_and(|runtime| runtime.dirty)
                    });
                    if dirty {
                        let prompt: Vec<u16> = "Des modifications ne sont pas enregistrées. Quitter sans enregistrer ?"
                            .encode_utf16().chain(Some(0)).collect();
                        let caption: Vec<u16> = "Atelier".encode_utf16().chain(Some(0)).collect();
                        // SAFETY: hwnd is the live owner window and both strings are NUL-terminated for this synchronous call.
                        let answer = MessageBoxW(
                            hwnd,
                            prompt.as_ptr(),
                            caption.as_ptr(),
                            0x0000_0024 | 0x0000_0100,
                        );
                        if answer != 6 {
                            return 0;
                        } // IDYES proceeds; IDNO cancels closing.
                    }
                    if !(*context).persisted {
                        let path = (*context).state_file.clone();
                        let dpi = (*context).dpi;
                        let snapshot = Context {
                            state_file: path,
                            title: Vec::new(),
                            client: (0, 0),
                            dpi,
                            persisted: false,
                            callback_error: None,
                            webview: None,
                            close_requested: false,
                            minimized: false,
                            file_runtime: None,
                        };
                        (*context).record(save(hwnd, &snapshot));
                        (*context).persisted = true;
                    }
                    if let Some(webview) = (*context).webview.as_mut() {
                        webview.close();
                        if webview.pending_callbacks() != 0 {
                            (*context).close_requested = true;
                            let _ = ShowWindow(hwnd, SW_HIDE as i32);
                            // Keep the STA alive to drain callbacks; the timer bounds normal shutdown.
                            if SetTimer(
                                hwnd,
                                TIMER_WEBVIEW_CLOSE,
                                webview::timeout_ms(),
                                null_mut(),
                            ) == 0
                            {
                                (*context)
                                    .record(Err(error_from_last("SetTimer(WebView2 close drain)")));
                                webview.detach_window();
                                let _ = DestroyWindow(hwnd);
                            }
                            return 0;
                        }
                        webview.detach_window();
                    }
                    // SAFETY: hwnd is the live window receiving WM_CLOSE; Context remains boxed while destroy callbacks run.
                    if DestroyWindow(hwnd) == 0 {
                        (*context).record(Err(error_from_last("DestroyWindow(WM_CLOSE)")));
                        PostQuitMessage(1);
                    }
                    return 0;
                }
                WM_DESTROY => {
                    if let Some(webview) = (*context).webview.as_mut() {
                        webview.close();
                        webview.detach_window();
                    }
                    if !(*context).persisted {
                        let snapshot = Context {
                            state_file: (*context).state_file.clone(),
                            title: Vec::new(),
                            client: (0, 0),
                            dpi: (*context).dpi,
                            persisted: false,
                            callback_error: None,
                            webview: None,
                            close_requested: false,
                            minimized: false,
                            file_runtime: None,
                        };
                        (*context).record(save(hwnd, &snapshot));
                        (*context).persisted = true;
                    }
                    PostQuitMessage(0);
                    return 0;
                }
                WM_NCDESTROY => {
                    let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, message, w, l)
    }));
    match outcome {
        Ok(result) => result,
        Err(_) => {
            // A panic is contained at the FFI boundary. Closing the owning thread is safer than continuing with unknown state.
            // SAFETY: hwnd/message arguments came from the active OS callback and remain valid until it returns.
            unsafe {
                PostQuitMessage(2);
                DefWindowProcW(hwnd, message, w, l)
            }
        }
    }
}

struct RegisteredClass {
    name: Vec<u16>,
    instance: Hinstance,
    owned: bool,
}
impl RegisteredClass {
    fn new() -> io::Result<Self> {
        // Classe propre à la racine de données pour la fenêtre principale (US-115, retrouvée par
        // FindWindowW) ; sinon nom unique par processus pour les auto-vérifications.
        let name = wide(std::ffi::OsStr::new(
            &instance_win32::window_class_name().unwrap_or_else(|| {
                // SAFETY: GetCurrentProcessId is a pointer-free query for a process-unique class name.
                format!("AtelierWindow-{}", unsafe { GetCurrentProcessId() })
            }),
        ));
        // SAFETY: null requests the current process module handle.
        let instance = unsafe { GetModuleHandleW(null()) };
        if instance.is_null() {
            return Err(error_from_last("GetModuleHandleW"));
        }
        let class = WndClassW {
            style: CS_HREDRAW | CS_VREDRAW,
            proc: Some(window_proc),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: null_mut(),
            cursor: null_mut(),
            background: null_mut(),
            menu_name: null(),
            class_name: name.as_ptr(),
        };
        // SAFETY: class and all referenced strings remain alive throughout registration.
        let atom = unsafe { RegisterClassW(&class) };
        if atom != 0 {
            return Ok(Self {
                name,
                instance,
                owned: true,
            });
        }
        // SAFETY: GetLastError reads the immediately preceding RegisterClassW failure.
        if unsafe { GetLastError() } == ERROR_CLASS_ALREADY_EXISTS {
            Ok(Self {
                name,
                instance,
                owned: false,
            })
        } else {
            Err(error_from_last("RegisterClassW"))
        }
    }
}
impl Drop for RegisteredClass {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: class was registered by this value and no window remains when it is dropped.
            unsafe {
                UnregisterClassW(self.name.as_ptr(), self.instance);
            }
        }
    }
}

// The class guard remains owned alongside the HWND until the HWND is destroyed.
struct OwnedWindow {
    hwnd: Hwnd,
    context: *mut Context,
    _class: RegisteredClass,
}
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        // SAFETY: Context is declared before OwnedWindow by every caller, so it remains alive here.
        unsafe {
            if !self.context.is_null() {
                if let Some(webview) = (*self.context).webview.as_mut() {
                    webview.close();
                    webview.detach_window();
                }
            }
        }
        // SAFETY: this owner is dropped on the creating thread while the Context Box is still alive.
        if unsafe { GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) != 0 } {
            // SAFETY: nonzero user data proves the owned HWND is still live; destruction runs on its owner thread.
            unsafe {
                DestroyWindow(self.hwnd);
            }
        }
    }
}

fn create_window(context: &mut Box<Context>, initial: WindowState) -> io::Result<OwnedWindow> {
    let class = RegisteredClass::new()?;
    let width = initial.normal.width().max(1);
    let height = initial.normal.height().max(1);
    let title = context.title.as_ptr();
    let style = WS_OVERLAPPEDWINDOW;
    // SAFETY: class strings and context Box remain valid; window creation is on the owning GUI thread.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            class.name.as_ptr(),
            title,
            style,
            initial.normal.left,
            initial.normal.top,
            width,
            height,
            null_mut(),
            null_mut(),
            class.instance,
            (&mut **context as *mut Context).cast(),
        )
    };
    if hwnd.is_null() {
        return Err(error_from_last("CreateWindowExW"));
    }
    Ok(OwnedWindow {
        hwnd,
        context: (&mut **context) as *mut Context,
        _class: class,
    })
}

fn apply_state(hwnd: Hwnd, state: WindowState, hidden: bool) -> io::Result<()> {
    let info = monitor_info_for_rect(state.normal)?;
    let placement = WindowPlacement {
        length: std::mem::size_of::<WindowPlacement>() as u32,
        flags: 0,
        show_cmd: if hidden {
            SW_HIDE
        } else if state.maximized {
            SW_SHOWMAXIMIZED as u32
        } else {
            SW_SHOWNORMAL as u32
        },
        min_position: Point::default(),
        max_position: Point::default(),
        normal_position: screen_to_workspace(state.normal, info),
    };
    // SAFETY: placement is initialized and its length matches the native WINDOWPLACEMENT size.
    if unsafe { SetWindowPlacement(hwnd, &placement) } == 0 {
        return Err(error_from_last("SetWindowPlacement"));
    }
    Ok(())
}

fn pump(hwnd: Hwnd) -> io::Result<i32> {
    let mut msg = Msg {
        hwnd: null_mut(),
        message: 0,
        w_param: 0,
        l_param: 0,
        time: 0,
        point: Point::default(),
        private: 0,
    };
    loop {
        // SAFETY: msg is writable; null HWND requests the current thread's message queue.
        let result = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
        if result == -1 {
            return Err(error_from_last("GetMessageW"));
        }
        if result == 0 {
            return Ok(msg.w_param as i32);
        }
        // SAFETY: msg was filled by GetMessageW and is valid for these dispatch calls.
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if hwnd.is_null() {
            return Err(io::Error::other("message loop lost its window handle"));
        }
    }
}

fn create_context(
    path: &Path,
    title: &str,
    state: Option<WindowState>,
) -> io::Result<Box<Context>> {
    let screens = monitors()?;
    // SAFETY: pointer-free query of the process's DPI context after initialization.
    let dpi = unsafe { GetDpiForSystem() }.clamp(48, 960);
    let state = restore_state(state, &screens, dpi);
    let mut wide_title: Vec<u16> = title.encode_utf16().collect();
    wide_title.push(0);
    Ok(Box::new(Context {
        state_file: path.to_path_buf(),
        title: wide_title,
        client: (0, 0),
        dpi: state.dpi,
        persisted: false,
        callback_error: None,
        webview: None,
        close_requested: false,
        minimized: false,
        file_runtime: None,
    }))
}

pub fn run(state_file: &Path, title: &str, webview_data: &Path) -> io::Result<()> {
    run_with_launch(state_file, title, webview_data, None, Vec::new())
}

/// Fenêtre principale avec instance unique : `instance_key` nomme la classe retrouvée par les
/// secondes instances ; `launch` sont les chemins d'ouverture de la ligne de commande.
pub fn run_with_launch(
    state_file: &Path,
    title: &str,
    webview_data: &Path,
    instance_key: Option<&str>,
    launch: Vec<PathBuf>,
) -> io::Result<()> {
    if let Some(key) = instance_key {
        instance_win32::set_window_key(key);
    }
    match webview::probe_runtime()? {
        Some(_) => {}
        None => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                webview::diagnostic(),
            ));
        }
    }
    let _apartment = webview::ComApartment::initialize_sta()?;
    let saved = load_state(state_file)?;
    let mut context = create_context(state_file, title, saved)?;
    let mut runtime = FileRuntime::new().map_err(|error| io::Error::other(error.message_key))?;
    runtime
        .launch
        .extend(launch.into_iter().take(crate::instance::MAX_PATHS));
    context.file_runtime = Some(runtime);
    let initial = restore_state(saved, &monitors()?, context.dpi);
    let window = create_window(&mut context, initial)?;
    let hwnd = window.hwnd;
    match WebView::begin(hwnd, webview_data) {
        Ok(control) => context.webview = Some(control),
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("{error}\n\n{}", webview::diagnostic()),
            ));
        }
    }
    if unsafe { SetTimer(hwnd, TIMER_WEBVIEW_INIT, webview::timeout_ms(), null_mut()) } == 0 {
        return Err(error_from_last("SetTimer(WebView2 initialization)"));
    }
    // SAFETY: hwnd is the live, owned window created above; this pointer-free call obtains its monitor DPI.
    context.dpi = unsafe { GetDpiForWindow(hwnd) }.clamp(48, 960);
    let screens = monitors_with_window_dpi(initial.normal, context.dpi)?;
    let state = restore_state(saved, &screens, context.dpi);
    apply_state(hwnd, state, true)?;
    apply_theme(hwnd)?;
    let mut initial_client = NativeRect::default();
    // SAFETY: hwnd is a live owned window and initial_client is writable RECT storage.
    if unsafe { GetClientRect(hwnd, &mut initial_client) } == 0 {
        return Err(error_from_last("GetClientRect(initial WebView2 bounds)"));
    }
    if let Some(webview) = context.webview.as_ref() {
        webview.set_bounds(
            initial_client.right - initial_client.left,
            initial_client.bottom - initial_client.top,
        )?;
        webview.set_visible(false)?;
    }
    // SAFETY: hwnd was created on this thread and is still live.
    unsafe {
        ShowWindow(
            hwnd,
            if state.maximized {
                SW_SHOWMAXIMIZED
            } else {
                SW_SHOWNORMAL
            },
        );
        UpdateWindow(hwnd);
    }
    let result = pump(hwnd);
    if !hwnd.is_null() && context.callback_error.is_none() && !context.persisted {
        context.record(save(hwnd, &context));
        context.persisted = true;
    }
    if let Some(error) = context.callback_error.take() {
        if let Some(control) = context.webview.as_mut() {
            control.close();
            control.detach_window();
        }
        return Err(io::Error::new(
            error.kind(),
            format!("{error}\n\n{}", webview::diagnostic()),
        ));
    }
    if let Some(control) = context.webview.as_mut() {
        control.close();
        control.detach_window();
    }
    drop(window);
    result.map(|_| ())
}

fn monitors_with_window_dpi(rect: Rect, dpi: u32) -> io::Result<Vec<MonitorArea>> {
    let mut screens = monitors()?;
    let cx = (i64::from(rect.left) + i64::from(rect.right)) / 2;
    let cy = (i64::from(rect.top) + i64::from(rect.bottom)) / 2;
    if let Some(screen) = screens.iter_mut().find(|screen| {
        cx >= i64::from(screen.monitor.left)
            && cx < i64::from(screen.monitor.right)
            && cy >= i64::from(screen.monitor.top)
            && cy < i64::from(screen.monitor.bottom)
    }) {
        screen.dpi = dpi.clamp(48, 960);
    }
    Ok(screens)
}

pub fn self_check(state_file: &Path) -> io::Result<String> {
    let state_file = validate_self_check_path(state_file)?;
    if let Some(parent) = state_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let saved = load_state(&state_file)?;
    let mut context = create_context(&state_file, "Atelier window self-check", saved)?;
    let initial = restore_state(saved, &monitors()?, context.dpi);
    let window = create_window(&mut context, initial)?;
    let hwnd = window.hwnd;
    // SAFETY: hwnd is the live hidden window owned by `window` and remains valid until its guard drops.
    context.dpi = unsafe { GetDpiForWindow(hwnd) }.clamp(48, 960);
    let screens = monitors_with_window_dpi(initial.normal, context.dpi)?;
    let state = restore_state(saved, &screens, context.dpi);
    apply_state(hwnd, state, true)?;
    // SAFETY: hwnd is a live owned handle; IsWindowVisible is a pointer-free query.
    if unsafe { IsWindowVisible(hwnd) } != 0 {
        return Err(io::Error::other("self-check HWND became visible"));
    }
    let mut client = NativeRect::default();
    // SAFETY: hwnd is a live hidden window created by this routine.
    if unsafe { GetClientRect(hwnd, &mut client) } == 0 {
        // SAFETY: hwnd remains live and owned; destruction completes callbacks before the context is dropped.
        unsafe {
            DestroyWindow(hwnd);
        }
        return Err(error_from_last("GetClientRect(self-check)"));
    }
    if client.right <= client.left || client.bottom <= client.top {
        // SAFETY: hwnd remains live and owned; destruction completes callbacks before the context is dropped.
        unsafe {
            DestroyWindow(hwnd);
        }
        return Err(io::Error::other(
            "hidden window has an empty client surface",
        ));
    }
    // SAFETY: hwnd is a live owned hidden window; this is a pointer-free DPI query.
    let before = unsafe { GetDpiForWindow(hwnd) };
    let mut rect = NativeRect {
        left: 120,
        top: 110,
        right: 980,
        bottom: 730,
    };
    // SAFETY: SendMessageW synchronously invokes WM_DPICHANGED; the stack RECT remains live for the callback.
    unsafe {
        SendMessageW(
            hwnd,
            WM_DPICHANGED,
            ((144usize) << 16) | 144,
            &mut rect as *mut _ as isize,
        );
    }
    // SAFETY: hwnd is still live and owned; this query retains no handle.
    if unsafe { IsWindowVisible(hwnd) } != 0 {
        return Err(io::Error::other(
            "self-check HWND became visible after DPI change",
        ));
    }
    let mut observed = NativeRect::default();
    // SAFETY: hwnd remains live and observed is writable storage with RECT layout.
    if unsafe { GetWindowRect(hwnd, &mut observed) } == 0 {
        // SAFETY: hwnd remains live and owned; destruction completes callbacks before the context is dropped.
        unsafe {
            DestroyWindow(hwnd);
        }
        return Err(error_from_last("GetWindowRect(self-check)"));
    }
    if (observed.left, observed.top, observed.right, observed.bottom)
        != (rect.left, rect.top, rect.right, rect.bottom)
    {
        // SAFETY: hwnd remains live and owned; destruction completes callbacks before the context is dropped.
        unsafe {
            DestroyWindow(hwnd);
        }
        return Err(io::Error::other("WM_DPICHANGED rectangle was not applied"));
    }
    context.dpi = 144;
    save(hwnd, &context)?;
    let persisted = load_state(&state_file)?
        .ok_or_else(|| io::Error::other("self-check state did not round-trip"))?;
    if persisted.dpi != 144 {
        // SAFETY: hwnd remains live and owned; destruction completes callbacks before the context is dropped.
        unsafe {
            DestroyWindow(hwnd);
        }
        return Err(io::Error::other("self-check DPI did not persist"));
    }
    let restored = restore_state(
        Some(persisted),
        &monitors_with_window_dpi(from_native(observed), context.dpi)?,
        context.dpi,
    );
    apply_state(hwnd, restored, true)?;
    // SAFETY: hwnd is still live and owned; this query retains no handle.
    if unsafe { IsWindowVisible(hwnd) } != 0 {
        return Err(io::Error::other(
            "self-check HWND became visible after restore",
        ));
    }
    // SAFETY: closes the hidden HWND before its owning Box is dropped.
    unsafe {
        DestroyWindow(hwnd);
    }
    context.persisted = true;
    drop(window);
    Ok(format!(
        "hidden HWND OK; client={}x{}; DPI {before}->144; state persisted and restored",
        client.right - client.left,
        client.bottom - client.top
    ))
}

fn process_handle_count() -> io::Result<u32> {
    let mut count = 0u32;
    // SAFETY: GetCurrentProcess returns a pseudo-handle and count is writable output storage.
    if unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) } == 0 {
        return Err(error_from_last("GetProcessHandleCount"));
    }
    Ok(count)
}

fn pump_until_webview_ready(hwnd: Hwnd, context: &mut Context) -> io::Result<()> {
    let mut msg = Msg {
        hwnd: null_mut(),
        message: 0,
        w_param: 0,
        l_param: 0,
        time: 0,
        point: Point::default(),
        private: 0,
    };
    loop {
        // SAFETY: `msg` is writable and null HWND requests messages from this STA queue.
        let status = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
        if status == -1 {
            return Err(error_from_last("GetMessageW(WebView2 self-check)"));
        }
        if status == 0 {
            return Err(io::Error::other(
                "WebView2 self-check window closed before initialization completed",
            ));
        }
        // SAFETY: msg was filled by GetMessageW and is valid for these dispatch calls.
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if let Some(error) = context.callback_error.take() {
            return Err(error);
        }
        if context
            .webview
            .as_mut()
            .is_some_and(|view| view.take_completed().is_ok_and(|ready| ready))
        {
            return Ok(());
        }
        if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 0 {
            return Err(io::Error::other("WebView2 self-check window was destroyed"));
        }
    }
}

fn run_webview_check_cycle(
    state_file: &Path,
    webview_data: &Path,
    probe_embedded_resources: bool,
) -> io::Result<String> {
    let saved = load_state(state_file)?;
    let mut context = create_context(state_file, "Atelier WebView2 self-check", saved)?;
    let initial = restore_state(saved, &monitors()?, context.dpi);
    let window = create_window(&mut context, initial)?;
    let hwnd = window.hwnd;
    // SAFETY: hwnd is a live hidden window; use the actual DPI context of its current monitor.
    context.dpi = unsafe { GetDpiForWindow(hwnd) }.clamp(48, 960);
    context.webview = Some(WebView::begin(hwnd, webview_data)?);
    if unsafe { SetTimer(hwnd, TIMER_WEBVIEW_INIT, webview::timeout_ms(), null_mut()) } == 0 {
        return Err(error_from_last("SetTimer(WebView2 self-check)"));
    }
    pump_until_webview_ready(hwnd, &mut context)?;
    let embedded_resources = if probe_embedded_resources {
        embedded_resource_self_check(hwnd, &mut context)?
    } else {
        String::new()
    };
    let devtools = context
        .webview
        .as_ref()
        .expect("created WebView2")
        .devtools_enabled()?;
    webview::validate_devtools(devtools)?;
    {
        let webview = context.webview.as_ref().expect("created WebView2");
        webview.set_bounds(317, 211)?;
        if webview.bounds()? != (0, 0, 317, 211) {
            return Err(io::Error::other(
                "WebView2 controller did not retain the self-check bounds",
            ));
        }
        webview.set_visible(false)?;
        if webview.is_visible()? {
            return Err(io::Error::other(
                "WebView2 controller remained visible after put_IsVisible(FALSE)",
            ));
        }
        webview.set_visible(true)?;
        if !webview.is_visible()? {
            return Err(io::Error::other(
                "WebView2 controller remained hidden after put_IsVisible(TRUE)",
            ));
        }
    }
    // Exercise the real WM_SIZE and WM_DPICHANGED handlers with the live WebView2 controller.
    // The host remains hidden, so this cannot flash a window during the diagnostic.
    // SAFETY: hwnd is the hidden self-check window and remains owned through this cycle.
    if unsafe {
        SetWindowPos(
            hwnd,
            null_mut(),
            120,
            110,
            860,
            620,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    } == 0
    {
        return Err(error_from_last("SetWindowPos(WebView2 resize self-check)"));
    }
    verify_webview_bounds_match_client(hwnd, &context)?;
    let mut dpi_rect = NativeRect {
        left: 120,
        top: 110,
        right: 980,
        bottom: 730,
    };
    // SAFETY: the RECT remains valid for this synchronous synthetic WM_DPICHANGED call.
    unsafe {
        SendMessageW(
            hwnd,
            WM_DPICHANGED,
            ((144usize) << 16) | 144,
            &mut dpi_rect as *mut _ as isize,
        );
    }
    verify_webview_bounds_match_client(hwnd, &context)?;
    {
        let webview = context.webview.as_mut().expect("created WebView2");
        webview.set_visible(false)?;
        webview.close_for_check()?;
    }
    pump_after_webview_close(hwnd, &mut context)?;
    context
        .webview
        .as_mut()
        .expect("created WebView2")
        .detach_window();
    // SAFETY: this routine owns hwnd and destroys it before context is dropped.
    if unsafe { DestroyWindow(hwnd) } == 0 {
        return Err(error_from_last("DestroyWindow(WebView2 self-check)"));
    }
    let report = embedded_resources;
    drop(window);
    drop(context);
    // WM_DESTROY posts WM_QUIT for this STA; remove it before the next independent cycle.
    let mut quit = Msg {
        hwnd: null_mut(),
        message: 0,
        w_param: 0,
        l_param: 0,
        time: 0,
        point: Point::default(),
        private: 0,
    };
    // SAFETY: quit is writable; PM_REMOVE consumes only WM_QUIT messages left by this cycle.
    while unsafe { PeekMessageW(&mut quit, null_mut(), 0x0012, 0x0012, 0x0001) } != 0 {}
    Ok(report)
}

fn embedded_resource_self_check(hwnd: Hwnd, context: &mut Context) -> io::Result<String> {
    use std::time::{Duration, Instant};

    const START: &str = r#"(()=>{
      if(location.origin!=='https://app.atelier')return false;
      if(window.__atelierUs112Started)return true;
      window.__atelierUs112Started=true;
      window.__atelierUs112Violations=[];
      addEventListener('securitypolicyviolation',e=>window.__atelierUs112Violations.push(e.effectiveDirective+'|'+e.blockedURI));
      window.__atelierUs112Result=null;
      void (async()=>{
        try {
          // Finish startup imports before explicit probes so their callback records
          // remain together in the bounded diagnostic log.
          const ready=await (await import('/app.mjs')).editorReady;
          if(ready.status!=='ready')throw new Error('EDITOR_NOT_READY');
          const paths=[
            ['/', 'text/html'], ['/app.mjs','text/javascript'], ['/app.css','text/css'],
            ['/editor-shell.mjs','text/javascript'],
            ['/ui-components.mjs','text/javascript'],
            ['/ui-components.css','text/css'],
            ['/ribbon.mjs','text/javascript'],
            ['/ribbon.css','text/css'],
            ['/theme.mjs','text/javascript'],
            ['/theme.css','text/css'],
            ['/rich-preview.mjs','text/javascript'],
            ['/text-drag-drop.mjs','text/javascript'],
            ['/french-spacing.mjs','text/javascript'],
            ['/i18n.mjs','text/javascript'], ['/i18n/fr-FR.txt','text/plain'], ['/engine-worker.mjs','text/javascript'],
            ['/engine-client.mjs','text/javascript'], ['/wasm-loader.mjs','text/javascript'],
            ['/editor-protocol.mjs','text/javascript'], ['/editor.mjs','text/javascript'],
            ['/native-host.mjs','text/javascript'], ['/display-list.mjs','text/javascript'],
            ['/navigation-tools.mjs','text/javascript'], ['/search-pane.mjs','text/javascript'], ['/search-pane.css','text/css'],
            ['/statistics-controls.mjs','text/javascript'], ['/statistics-controls.css','text/css'],
            ['/zoom.mjs','text/javascript'], ['/zoom-controls.mjs','text/javascript'], ['/zoom-controls.css','text/css'],
            ['/canvas-renderer.mjs','text/javascript'], ['/text-input.mjs','text/javascript'],
            ['/mouse-selection.mjs','text/javascript'],
            ['/wasm-protocol.mjs','text/javascript'], ['/atelier_core_wasm.wasm','application/wasm'],
            ['/fonts/Carlito-Regular.ttf','font/ttf'], ['/licenses/Carlito-OFL.txt','text/plain']
          ];
          paths.push(['/page-navigation.mjs','text/javascript'],['/replace-controls.mjs','text/javascript'],['/replace-controls.css','text/css'],['/document-status.mjs','text/javascript'],['/document-status.css','text/css']);
          paths.push(['/symbol-dialog.mjs','text/javascript'],['/symbol-dialog.css','text/css'],['/symbol-catalogue.mjs','text/javascript'],['/editor-context-menu.mjs','text/javascript'],['/about-dialog.mjs','text/javascript'],['/about-dialog.css','text/css'],['/about-licenses.mjs','text/javascript'],['/build-info.mjs','text/javascript'],['/image-insert.mjs','text/javascript'],['/properties-dialog.mjs','text/javascript'],['/properties-dialog.css','text/css'],['/properties-protocol.mjs','text/javascript']);
          // Keep required log probes within the bounded request log.
          paths.sort((a,b)=>Number(['/app.css','/engine-worker.mjs','/atelier_core_wasm.wasm'].includes(a[0]))-Number(['/app.css','/engine-worker.mjs','/atelier_core_wasm.wasm'].includes(b[0])));
          const assets={};
          for(const [path,mime] of paths){const r=await fetch(path,{cache:'no-store'});assets[path]={status:r.status,type:r.headers.get('content-type')||'',mime};}
          const root=await fetch('/',{cache:'no-store'});
          const encoded=await fetch('/%61pp.mjs?secret=do-not-log',{cache:'no-store'});
          const getProbe=await fetch('/app.mjs',{cache:'no-store'});
          const getBytes=await getProbe.arrayBuffer();
          const headProbe=await fetch('/app.mjs',{method:'HEAD',cache:'no-store'});
          const headBytes=await headProbe.arrayBuffer();
          const missingProbe=await fetch('/__us112_missing__',{cache:'no-store'});
          const postProbe=await fetch('/app.mjs',{method:'POST',cache:'no-store'});
          const methodProbesOk=headProbe.status===200&&headBytes.byteLength===0
            &&Number(headProbe.headers.get('content-length'))===getBytes.byteLength
            &&getProbe.status===200&&missingProbe.status===404&&postProbe.status===405;
          void fetch('https://us112-fetch.invalid/probe?private=1').catch(()=>{});
          const image=new Image();image.src='https://us112-image.invalid/probe?private=1';
          const frame=document.createElement('iframe');frame.src='https://us112-frame.invalid/probe?private=1';document.body.append(frame);
          const script=document.createElement('script');script.src='https://us112-script.invalid/probe.js?private=1';document.head.append(script);
          const stylesheet=document.createElement('link');stylesheet.rel='stylesheet';stylesheet.href='https://us112-style.invalid/probe.css?private=1';document.head.append(stylesheet);
          try{window.open('https://outside.invalid/us112-window?private=1','_blank')}catch{}
          try{new WebSocket('wss://us112-socket.invalid/probe?private=1')}catch{}
          try{location.assign('https://app.atelier.evil/us112-navigation')}catch{}
          const required=['connect-src|https://us112-fetch.invalid','connect-src|wss://us112-socket.invalid','img-src|https://us112-image.invalid','frame-src|https://us112-frame.invalid','script-src-elem|https://us112-script.invalid','style-src-elem|https://us112-style.invalid'];
          const deadline=Date.now()+12000;
          while(Date.now()<deadline&&(document.documentElement.dataset.engineEcho!=='ok'||!required.every(prefix=>window.__atelierUs112Violations.some(v=>v.startsWith(prefix)))))await new Promise(r=>setTimeout(r,40));
           const editor=ready.editor;
           const buildInfo=(await import('/build-info.mjs')).BUILD_INFO;
           const legal=(await import('/about-licenses.mjs')).ABOUT_LICENSES;
           const catalogue=(await import('/symbol-catalogue.mjs')).SYMBOL_CATALOGUE;
           ready.about.open({returnFocusTo:editor.element});
           const aboutText=ready.about.element.textContent;
           const expectedDate=(await import('/i18n.mjs')).createI18n({locale:'fr-FR'}).formatDate(new Date(buildInfo.buildDate+'T00:00:00Z'),{timeZone:'UTC'});
           const utilitiesOk=/^[0-9]{4}-[0-9]{2}-[0-9]{2}$/.test(buildInfo.buildDate)
             &&aboutText.includes(buildInfo.version)
             &&[...ready.about.element.querySelectorAll('time[datetime]')].some(t=>t.getAttribute('datetime')===buildInfo.buildDate&&t.textContent===expectedDate)
             &&aboutText.includes(legal.agpl)&&aboutText.includes(legal.unicode)
             &&legal.fonts.every(font=>aboutText.includes(font.text))&&catalogue.symbols.length>2000&&!!ready.symbols&&!!ready.contextMenu;
           ready.about.close();
          const input=editor.element;
          input.focus();
          const expected='Bonjour é€\n';
          input.dispatchEvent(new InputEvent('beforeinput',{inputType:'insertText',data:expected,bubbles:true,cancelable:true}));
          const exported=await ready.exportDocument();
          const edited=ready.getState();
          const typedOk=new TextDecoder().decode(exported.bytes)===expected&&edited.dirty&&edited.pages.length===1;
          const canvas=document.querySelector('canvas');
          const pixels=canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height).data;
          let ink=0;for(let i=0;i<pixels.length;i+=4)if(pixels[i]<100&&pixels[i+1]<100&&pixels[i+2]<100&&pixels[i+3]>0)ink++;
          await editor.undo();
          const undoOk=ready.getState().text===''&&!ready.getState().dirty;
          await editor.redo();
          const redoOk=ready.getState().text===expected&&ready.getState().dirty;
          await editor.undo();
          const editorOk=typedOk&&undoOk&&redoOk&&ink>20&&!ready.getState().dirty;
          window.__atelierUs112Result={
            assets,
            assetsOk:Object.entries(assets).length===paths.length&&Object.values(assets).every(x=>x.status===200&&x.type.startsWith(x.mime)),
            utilitiesOk,
            origin:location.origin,
            echo:document.documentElement.dataset.engineEcho||'',
            csp:root.headers.get('content-security-policy')||'',
            encodedStatus:encoded.status,
            methodProbesOk,
            editorOk,typedOk,undoOk,redoOk,ink,
            violations:window.__atelierUs112Violations,
            cspProbesOk:required.every(prefix=>window.__atelierUs112Violations.some(v=>v.startsWith(prefix)))
          };
        }catch(error){window.__atelierUs112Result={error:String(error)};}
      })();
      return true;
    })()"#;
    const POLL: &str =
        r#"({done:!!window.__atelierUs112Result,result:window.__atelierUs112Result})"#;

    let start_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let started = run_script_until(hwnd, context, START, Duration::from_secs(2))?;
        if started.trim() == "true" {
            break;
        }
        if Instant::now() >= start_deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("embedded origin did not become active; ExecuteScript returned {started}"),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    let payload = loop {
        let value = run_script_until(hwnd, context, POLL, Duration::from_secs(3))?;
        if value.contains("\"done\":true") {
            break value;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("embedded page self-check timed out; last result={value}"),
            ));
        }
    };
    if !payload.contains("\"utilitiesOk\":true")
        || !payload.contains("\"assetsOk\":true")
        || !payload.contains("\"origin\":\"https://app.atelier\"")
        || !payload.contains("\"echo\":\"ok\"")
        || !payload.contains("\"encodedStatus\":403")
        || !payload.contains("\"methodProbesOk\":true")
        || !payload.contains("\"editorOk\":true")
        || !payload.contains("connect-src https://app.atelier")
        || !payload.contains("\"cspProbesOk\":true")
    {
        return Err(io::Error::other(format!(
            "embedded resources/CSP/Worker self-check failed: {payload}"
        )));
    }
    let view = context.webview.as_ref().expect("created WebView2");
    let attempts = view.resource_log_snapshot();
    for needle in [
        "/atelier_core_wasm.wasm [served]",
        "/engine-worker.mjs [served]",
        "/app.css [served]",
        "%61pp.mjs [refused]",
        "us112-window [refused]",
        "app.atelier.evil/us112-navigation [refused]",
        "CSP connect-src https://us112-fetch.invalid/probe [blocked]",
        "CSP connect-src wss://us112-socket.invalid/probe [blocked]",
        "CSP img-src https://us112-image.invalid/probe [blocked]",
        "CSP frame-src https://us112-frame.invalid [blocked]",
        "CSP script-src-elem https://us112-script.invalid/probe.js [blocked]",
        "CSP style-src-elem https://us112-style.invalid/probe.css [blocked]",
    ] {
        if !attempts.iter().any(|entry| entry.contains(needle)) {
            return Err(io::Error::other(format!(
                "embedded resource callback did not record {needle}; entries={attempts:?}"
            )));
        }
    }
    // Startup plus explicit probes may rotate the bounded diagnostic log.
    // Every required probe above must still be present; report rotation honestly.
    Ok(format!(
        "embedded resources: assets={}; worker-ping=echo; editor=typed+ink+undo+redo; encoded-path=403; CSP fetch+WebSocket+image+frame=blocked; attempts={}; rotated={}",
        crate::resources::RESOURCES.len(),
        attempts.len(),
        view.dropped_resource_attempts()
    ))
}

fn run_script_until(
    hwnd: Hwnd,
    context: &mut Context,
    script: &str,
    timeout: std::time::Duration,
) -> io::Result<String> {
    use std::time::Instant;
    let view = context.webview.as_ref().expect("created WebView2");
    view.execute_script(script)?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(result) = context
            .webview
            .as_ref()
            .expect("created WebView2")
            .take_script_result()
        {
            return result;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "WebView2 ExecuteScript self-check callback timed out",
            ));
        }
        // Drain STA messages so the COM completion callback can run. The bounded wait keeps
        // the hidden diagnostic responsive while avoiding a busy loop.
        let wait = unsafe { MsgWaitForMultipleObjectsEx(0, null(), 25, 0x04ff, 0x0004) };
        if wait == u32::MAX {
            return Err(error_from_last(
                "MsgWaitForMultipleObjectsEx(ExecuteScript self-check)",
            ));
        }
        if wait == 0 {
            let mut msg = Msg {
                hwnd: null_mut(),
                message: 0,
                w_param: 0,
                l_param: 0,
                time: 0,
                point: Point::default(),
                private: 0,
            };
            while unsafe { PeekMessageW(&mut msg, null_mut(), 0, 0, 0x0001) } != 0 {
                if msg.message == 0x0012 {
                    return Err(io::Error::other(
                        "unexpected WM_QUIT during embedded resource self-check",
                    ));
                }
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 0 {
                    return Err(io::Error::other("hidden self-check window was destroyed"));
                }
            }
        }
        if let Some(error) = context.callback_error.take() {
            return Err(error);
        }
    }
}

fn verify_webview_bounds_match_client(hwnd: Hwnd, context: &Context) -> io::Result<()> {
    let mut client = NativeRect::default();
    // SAFETY: hwnd is a live hidden HWND and client is writable RECT storage.
    if unsafe { GetClientRect(hwnd, &mut client) } == 0 {
        return Err(error_from_last("GetClientRect(WebView2 bounds self-check)"));
    }
    if let Some(error) = &context.callback_error {
        return Err(io::Error::other(error.to_string()));
    }
    let actual = context
        .webview
        .as_ref()
        .expect("created WebView2")
        .bounds()?;
    let expected = (0, 0, client.right - client.left, client.bottom - client.top);
    if actual != expected {
        return Err(io::Error::other(format!(
            "WebView2 bounds did not track host resize/DPI change: expected {expected:?}, got {actual:?}"
        )));
    }
    Ok(())
}

fn pump_after_webview_close(hwnd: Hwnd, context: &mut Context) -> io::Result<()> {
    use std::time::{Duration, Instant};

    const QS_ALLINPUT: u32 = 0x04ff;
    const MWMO_INPUTAVAILABLE: u32 = 0x0004;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const WAIT_FAILED: u32 = u32::MAX;
    const QUIET_PERIOD: Duration = Duration::from_millis(100);
    const MAX_DRAIN: Duration = Duration::from_secs(5);

    let started = Instant::now();
    let mut last_message = started;
    loop {
        if let Some(error) = context.callback_error.take() {
            return Err(error);
        }
        if started.elapsed() >= MAX_DRAIN {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "STA message queue did not become idle after WebView2 controller Close",
            ));
        }
        if last_message.elapsed() >= QUIET_PERIOD {
            return Ok(());
        }
        let remaining = QUIET_PERIOD
            .saturating_sub(last_message.elapsed())
            .min(MAX_DRAIN.saturating_sub(started.elapsed()));
        let wait_ms = remaining.as_millis().clamp(1, u128::from(u32::MAX)) as u32;
        // SAFETY: count 0 means no handles; this waits for messages on the current STA thread.
        let wait = unsafe {
            MsgWaitForMultipleObjectsEx(0, null(), wait_ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        if wait == WAIT_FAILED {
            return Err(error_from_last(
                "MsgWaitForMultipleObjectsEx(WebView2 close drain)",
            ));
        }
        if wait == WAIT_TIMEOUT {
            continue;
        }
        if wait != WAIT_OBJECT_0 {
            return Err(io::Error::other(format!(
                "unexpected STA wait result 0x{wait:08x} during WebView2 close drain"
            )));
        }
        let mut message = Msg {
            hwnd: null_mut(),
            message: 0,
            w_param: 0,
            l_param: 0,
            time: 0,
            point: Point::default(),
            private: 0,
        };
        // SAFETY: message is writable; PM_REMOVE drains the current thread's queued messages.
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, 0x0001) } != 0 {
            if message.message == 0x0012 {
                return Err(io::Error::other(
                    "unexpected WM_QUIT during WebView2 close drain",
                ));
            }
            // SAFETY: message was filled by PeekMessageW and is valid for dispatch.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            last_message = Instant::now();
            if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 0 {
                return Err(io::Error::other(
                    "WebView2 parent window was destroyed while draining close messages",
                ));
            }
        }
    }
}

/// Run one hidden WebView2 cycle that exercises every embedded resource and security probe.
pub fn resources_self_check(state_file: &Path, cache: &Path) -> io::Result<String> {
    if probe_runtime_unavailable()? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Microsoft Edge WebView2 Evergreen runtime is unavailable",
        ));
    }
    let state_file = validate_self_check_path(state_file)?;
    let cache_marker = validate_self_check_path(&cache.join(".self-check-path"))?;
    let cache = cache_marker.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "WebView2 self-check cache needs a directory",
        )
    })?;
    if let Some(parent) = state_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(cache)?;

    let _apartment = webview::ComApartment::initialize_sta()?;
    let callback_baseline = WebView::live_callback_objects();
    let report = run_webview_check_cycle(&state_file, cache, true)?;
    let callbacks_after = WebView::live_callback_objects();
    if callbacks_after != callback_baseline {
        return Err(io::Error::other(format!(
            "WebView2 callback objects leaked during the resource self-check: {callback_baseline}->{callbacks_after}"
        )));
    }
    let _ = std::fs::remove_file(&state_file);
    Ok(format!(
        "{report}; callback objects {callback_baseline}->{callbacks_after}"
    ))
}

pub fn webview_self_check(
    state_file: &Path,
    webview_data: &Path,
    cycles: usize,
) -> io::Result<String> {
    if !(1..=100).contains(&cycles) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WebView2 self-check cycles must be between 1 and 100",
        ));
    }
    if probe_runtime_unavailable()? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Microsoft Edge WebView2 Evergreen runtime is unavailable",
        ));
    }
    // Validate every requested path before creating any state or cache directory.
    let state_file = validate_self_check_path(state_file)?;
    let cache_marker = validate_self_check_path(&webview_data.join(".self-check-path"))?;
    let webview_data = cache_marker
        .parent()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "WebView2 self-check cache needs a directory",
            )
        })?
        .to_path_buf();
    if let Some(parent) = state_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&webview_data)?;

    let _apartment = webview::ComApartment::initialize_sta()?;
    let callback_baseline = WebView::live_callback_objects();
    let _ = run_webview_check_cycle(&state_file, &webview_data, false)?; // Warm runtime and graphics paths before measuring.
    let handle_baseline = process_handle_count()?;
    if WebView::live_callback_objects() != callback_baseline {
        return Err(io::Error::other(
            "WebView2 callback references did not return to their pre-warm-up count",
        ));
    }
    let mut peak = handle_baseline;
    let mut handle_samples = Vec::new();
    for cycle in 1..=cycles {
        run_webview_check_cycle(&state_file, &webview_data, false)?;
        let live = WebView::live_callback_objects();
        if live != callback_baseline {
            return Err(io::Error::other(format!(
                "WebView2 callback objects leaked: baseline {callback_baseline}, now {live}"
            )));
        }
        let handles = process_handle_count()?;
        peak = peak.max(handles);
        if cycle == 1 || cycle == 10 || cycle == 100 || cycle == cycles {
            handle_samples.push((cycle, handles));
        }
    }
    let final_handles = process_handle_count()?;
    if final_handles > handle_baseline {
        return Err(io::Error::other(format!(
            "WebView2 functional OK; cycles={cycles}; callback objects {callback_baseline}->{callback_baseline}; DevTools={}; process handle count grew: baseline {handle_baseline}, samples {handle_samples:?}, final {final_handles}, peak {peak}",
            cfg!(debug_assertions)
        )));
    }
    let _ = std::fs::remove_file(&state_file);
    Ok(format!(
        "WebView2 OK; warm-up=1; cycles={cycles}; callback objects {callback_baseline}->{callback_baseline}; process handles baseline={handle_baseline}, samples={handle_samples:?}, peak={peak}, final={final_handles}; DevTools={}",
        cfg!(debug_assertions),
    ))
}

fn probe_runtime_unavailable() -> io::Result<bool> {
    Ok(webview::probe_runtime()?.is_none())
}

fn validate_self_check_path(path: &Path) -> io::Result<PathBuf> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let target = workspace.join("target").canonicalize()?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    let absolute = strip_verbatim_prefix(&absolute);
    let target = strip_verbatim_prefix(&target);
    if !absolute.is_absolute()
        || absolute
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || !absolute.starts_with(&target)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "self-check state path must resolve beneath the workspace target/ directory",
        ));
    }
    let file_name = absolute.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "self-check state path needs a file name",
        )
    })?;
    let relative = absolute.strip_prefix(&target).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "self-check path escapes target/",
        )
    })?;
    let relative_parent = relative.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "self-check state path needs a parent",
        )
    })?;
    let canonical_target = target.canonicalize()?;
    let mut current = canonical_target.clone();
    let components: Vec<_> = relative_parent.components().collect();
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid self-check parent",
            ));
        };
        let child = current.join(name);
        match std::fs::symlink_metadata(&child) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                for remaining in &components[index..] {
                    if !matches!(remaining, std::path::Component::Normal(_)) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "invalid self-check parent",
                        ));
                    }
                    current.push(remaining.as_os_str());
                }
                break;
            }
            Err(error) => return Err(error),
        }
        let resolved = child.canonicalize()?;
        if !strip_verbatim_prefix(&resolved).starts_with(strip_verbatim_prefix(&canonical_target)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "self-check state parent resolves outside workspace target/",
            ));
        }
        if !std::fs::metadata(&resolved)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "self-check parent is not a directory",
            ));
        }
        current = resolved;
    }
    if !strip_verbatim_prefix(&current).starts_with(strip_verbatim_prefix(&canonical_target)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "self-check path escapes target/",
        ));
    }
    let requested = current.join(file_name);
    match std::fs::symlink_metadata(&requested) {
        Ok(_) => {
            let resolved = requested.canonicalize()?;
            if !strip_verbatim_prefix(&resolved)
                .starts_with(strip_verbatim_prefix(&canonical_target))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "self-check file resolves outside workspace target/",
                ));
            }
            Ok(resolved)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(requested),
        Err(error) => Err(error),
    }
}

fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

pub fn show_error(message: &str) {
    let body: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let caption: Vec<u16> = "Atelier".encode_utf16().chain(Some(0)).collect();
    // SAFETY: both strings are NUL-terminated and remain alive for the synchronous MessageBoxW call.
    unsafe {
        MessageBoxW(null_mut(), body.as_ptr(), caption.as_ptr(), 0x10);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CLOSE_WAIT_LIMIT, CloseGate, FileDialogKind, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED,
        WPF_RESTORETOMAXIMIZED, atomic_write_to_temp, close_gate, placement_is_maximized,
        route_file_request, validate_self_check_path, with_file_dialog_options,
    };
    use crate::file_bridge::{self, Request};
    use crate::file_dialog::FileFilter;
    use crate::files::FileBroker;
    use std::io;
    use std::path::{Path, PathBuf};

    #[test]
    fn close_gate_waits_for_bridge_jobs_then_asks_after_the_limit() {
        use std::time::Duration;
        assert_eq!(close_gate(0, None), CloseGate::Proceed);
        assert_eq!(
            close_gate(0, Some(CLOSE_WAIT_LIMIT * 2)),
            CloseGate::Proceed
        );
        assert_eq!(close_gate(1, None), CloseGate::Wait);
        assert_eq!(close_gate(3, Some(Duration::from_secs(1))), CloseGate::Wait);
        let just_under = CLOSE_WAIT_LIMIT - Duration::from_millis(1);
        assert_eq!(close_gate(1, Some(just_under)), CloseGate::Wait);
        assert_eq!(close_gate(1, Some(CLOSE_WAIT_LIMIT)), CloseGate::AskTimeout);
    }

    #[test]
    fn common_file_route_uses_distinct_dialogs_and_selected_paths() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("atelier-file-route-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&directory).expect("create isolated fixture directory");
        let open_path = directory.join("open.txt");
        let save_path = directory.join("save.txt");
        std::fs::write(&open_path, b"selected open bytes\r\n").expect("open fixture");
        std::fs::write(&save_path, b"old save bytes").expect("save fixture");
        let broker = FileBroker::new();
        let mut selected_kinds = Vec::new();

        let open_reply = route_file_request(Request::Open { id: "1".into() }, &broker, |kind| {
            selected_kinds.push(kind);
            assert_eq!(kind, FileDialogKind::Open);
            Ok(Some(open_path.clone()))
        });
        assert_eq!(
            open_reply,
            format!(
                "us120-file|1|1|ok|{}",
                file_bridge::encode_base64(b"selected open bytes\r\n")
            )
        );

        let save_reply = route_file_request(
            Request::Save {
                id: "2".into(),
                bytes: b"new saved bytes\r\n".to_vec(),
            },
            &broker,
            |kind| {
                selected_kinds.push(kind);
                assert_eq!(kind, FileDialogKind::Save);
                Ok(Some(save_path.clone()))
            },
        );
        assert_eq!(save_reply, "us120-file|1|2|ok|");
        assert_eq!(std::fs::read(&save_path).unwrap(), b"new saved bytes\r\n");
        assert_eq!(selected_kinds, [FileDialogKind::Open, FileDialogKind::Save]);

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn common_file_route_returns_cancel_and_dialog_errors() {
        let broker = FileBroker::new();
        let mut selected = None;
        let cancelled = route_file_request(Request::Open { id: "3".into() }, &broker, |kind| {
            selected = Some(kind);
            Ok(None)
        });
        assert_eq!(selected, Some(FileDialogKind::Open));
        assert_eq!(cancelled, "us120-file|1|3|cancelled|");

        let failed = route_file_request(
            Request::Save {
                id: "4".into(),
                bytes: b"save".to_vec(),
            },
            &broker,
            |kind| {
                assert_eq!(kind, FileDialogKind::Save);
                Err("files.error.dialog_failed")
            },
        );
        assert_eq!(failed, "us120-file|1|4|error|files.error.dialog_failed");

        let missing = std::env::temp_dir().join(format!(
            "atelier-route-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let broker_error = route_file_request(Request::Open { id: "5".into() }, &broker, |_| {
            Ok(Some(missing.clone()))
        });
        assert_eq!(broker_error, "us120-file|1|5|error|files.error.not_found");

        let directory_error = route_file_request(
            Request::Save {
                id: "6".into(),
                bytes: b"save".to_vec(),
            },
            &broker,
            |_| Ok(Some(std::env::temp_dir())),
        );
        assert_eq!(
            directory_error,
            "us120-file|1|6|error|files.error.destination_is_directory"
        );
    }

    #[test]
    fn docx_route_selects_its_own_dialog_and_accepts_files_over_txt_limit() {
        let directory = std::env::temp_dir().join(format!(
            "atelier-docx-route-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).expect("create isolated fixture directory");
        let docx_path = directory.join("preview.docx");
        let bytes = vec![0x5a; file_bridge::MAX_FILE_BYTES + 1];
        std::fs::write(&docx_path, &bytes).expect("write larger than TXT limit");
        let txt_path = directory.join("note.txt");
        std::fs::write(&txt_path, b"text").expect("write TXT fixture");
        let broker = FileBroker::new();
        let mut selected_kind = None;
        let reply = route_file_request(Request::OpenDocx { id: "31".into() }, &broker, |kind| {
            selected_kind = Some(kind);
            with_file_dialog_options(kind, |title, filters, selected_filter, suggested_name| {
                assert_eq!(title, "Ouvrir un document Word");
                assert_eq!(
                    filters,
                    &[FileFilter::new("Document Word", ["docx"]).unwrap()]
                );
                assert_eq!(selected_filter, 0);
                assert_eq!(suggested_name, None);
                Ok(Some(docx_path.clone()))
            })
            .map_err(|error| error.message_key)
        });
        assert_eq!(selected_kind, Some(FileDialogKind::OpenDocx));
        assert_eq!(
            reply,
            format!("us120-file|1|31|ok|{}", file_bridge::encode_base64(&bytes))
        );

        let txt_reply = route_file_request(Request::Open { id: "33".into() }, &broker, |kind| {
            with_file_dialog_options(kind, |title, filters, selected_filter, suggested_name| {
                assert_eq!(title, "Ouvrir un fichier texte");
                assert_eq!(
                    filters,
                    &[FileFilter::new("Fichier texte", ["txt"]).unwrap()]
                );
                assert_eq!(selected_filter, 0);
                assert_eq!(suggested_name, None);
                Ok(Some(txt_path.clone()))
            })
            .map_err(|error| error.message_key)
        });
        assert_eq!(
            txt_reply,
            format!("us120-file|1|33|ok|{}", file_bridge::encode_base64(b"text"))
        );

        let cancelled =
            route_file_request(Request::OpenDocx { id: "32".into() }, &broker, |kind| {
                assert_eq!(kind, FileDialogKind::OpenDocx);
                Ok(None)
            });
        assert_eq!(cancelled, "us120-file|1|32|cancelled|");
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn temporary_name_collision_preserves_the_existing_file() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root");
        let directory = root.join("target");
        let destination = directory.join(format!("us110-atomic-{}.state", std::process::id()));
        let temporary = directory.join(format!("us110-collision-{}.tmp", std::process::id()));
        std::fs::write(&temporary, b"pre-existing owner").expect("create collision fixture");
        let error = atomic_write_to_temp(&destination, &temporary, b"new state")
            .expect_err("create_new must reject an occupied temp name");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(&temporary).expect("collision file retained"),
            b"pre-existing owner"
        );
        let _ = std::fs::remove_file(temporary);
        let _ = std::fs::remove_file(destination);
    }

    #[test]
    fn minimized_maximized_window_restores_its_normal_maximized_state() {
        assert!(placement_is_maximized(SW_SHOWMAXIMIZED as u32, 0));
        assert!(placement_is_maximized(
            SW_SHOWMINIMIZED,
            WPF_RESTORETOMAXIMIZED
        ));
        assert!(!placement_is_maximized(SW_SHOWMINIMIZED, 0));
    }

    #[test]
    fn self_check_accepts_normal_absolute_workspace_target_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root");
        let normal_root = super::strip_verbatim_prefix(&root);
        let path = normal_root.join("target/shell-check/window-state-absolute-test.txt");
        let normalized = validate_self_check_path(&path).expect("normal absolute path accepted");
        assert!(normalized.starts_with(root.join("target").canonicalize().unwrap()));
        let verbatim = PathBuf::from(format!(r"\\?\{}", path.display()));
        let verbatim_normalized =
            validate_self_check_path(&verbatim).expect("verbatim absolute path accepted");
        assert!(verbatim_normalized.starts_with(root.join("target").canonicalize().unwrap()));
        let traversal = normal_root.join("target/../outside-self-check.txt");
        assert_eq!(
            validate_self_check_path(&traversal)
                .expect_err("parent traversal rejected")
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let _ = std::fs::remove_file(normalized);
        let _ = std::fs::remove_file(verbatim_normalized);
    }

    #[test]
    fn self_check_rejects_junction_before_creating_descendants() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root");
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let fixture_id = format!("{}-{nonce}", std::process::id());
        let junction = root
            .join("target")
            .join(format!("us110-junction-{fixture_id}"));
        let outside = root.join(format!("us110-junction-outside-{fixture_id}"));
        std::fs::create_dir(&outside).expect("outside fixture");
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .output()
            .expect("cmd.exe available on Windows");
        assert!(
            output.status.success(),
            "mklink /J failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let descendant = junction.join("missing-descendant");
        let request = descendant.join("state.txt");
        assert_eq!(
            validate_self_check_path(&request)
                .expect_err("junction escape rejected")
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!outside.join("missing-descendant").exists());
        let _ = std::fs::remove_dir(&junction);
        let _ = std::fs::remove_dir(&outside);
    }
}
