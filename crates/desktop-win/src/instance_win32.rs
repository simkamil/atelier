// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-115 : FFI de l'instance unique (mutex nommé, `WM_COPYDATA`, premier plan) et sonde de diagnostic.
//! Toute la logique de décodage et de validation est dans `instance.rs` (pure et testée).

use crate::instance::{self, PROTOCOL_ID};
use std::ffi::{OsStr, c_void};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

type Handle = *mut c_void;

const WM_COPYDATA: u32 = 0x004a;
const WM_TIMER: u32 = 0x0113;
const SMTO_BLOCK: u32 = 0x1;
const SMTO_ABORTIFHUNG: u32 = 0x2;
const SW_RESTORE: i32 = 9;
const ERROR_ALREADY_EXISTS: u32 = 183;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
/// Nouvelles tentatives quand la fenêtre de la première instance n'existe pas encore.
const FORWARD_ATTEMPTS: u32 = 40;
const FORWARD_DELAY_MS: u32 = 125;
const SEND_TIMEOUT_MS: u32 = 3000;
const PROBE_TIMEOUT_MS: usize = 20_000;

/// Clé d'instance de la fenêtre principale ; absente pour les auto-vérifications (classe par processus).
static WINDOW_KEY: OnceLock<String> = OnceLock::new();

pub(crate) fn set_window_key(key: &str) {
    let _ = WINDOW_KEY.set(key.to_owned());
}

pub(crate) fn window_class_name() -> Option<String> {
    WINDOW_KEY.get().map(|key| instance::window_class_name(key))
}

#[repr(C)]
struct CopyDataStruct {
    dw_data: usize,
    cb_data: u32,
    lp_data: *mut c_void,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct Msg {
    hwnd: Handle,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    point: Point,
    private: u32,
}

#[repr(C)]
struct WndClassW {
    style: u32,
    proc: Option<unsafe extern "system" fn(Handle, u32, usize, isize) -> isize>,
    class_extra: i32,
    window_extra: i32,
    instance: Handle,
    icon: Handle,
    cursor: Handle,
    background: Handle,
    menu_name: *const u16,
    class_name: *const u16,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn FindWindowW(class_name: *const u16, window_name: *const u16) -> Handle;
    fn SendMessageTimeoutW(
        hwnd: Handle,
        message: u32,
        w: usize,
        l: isize,
        flags: u32,
        timeout: u32,
        result: *mut usize,
    ) -> isize;
    fn GetWindowThreadProcessId(hwnd: Handle, process: *mut u32) -> u32;
    fn AllowSetForegroundWindow(process: u32) -> i32;
    fn SetForegroundWindow(hwnd: Handle) -> i32;
    fn IsIconic(hwnd: Handle) -> i32;
    fn ShowWindow(hwnd: Handle, command: i32) -> i32;
    fn RegisterClassW(class: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        param: *mut c_void,
    ) -> Handle;
    fn DefWindowProcW(hwnd: Handle, message: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(msg: *mut Msg, hwnd: Handle, min: u32, max: u32) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn PostQuitMessage(code: i32);
    fn SetTimer(hwnd: Handle, id: usize, elapsed: u32, callback: *mut c_void) -> usize;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(attributes: *mut c_void, initial_owner: i32, name: *const u16) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetLastError() -> u32;
    fn GetCurrentProcessId() -> u32;
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn Sleep(milliseconds: u32);
    fn OpenProcess(access: u32, inherit: i32, process: u32) -> Handle;
    fn QueryFullProcessImageNameW(
        process: Handle,
        flags: u32,
        name: *mut u16,
        size: *mut u32,
    ) -> i32;
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

fn last_error(operation: &str) -> io::Error {
    // SAFETY: lit uniquement le code d'erreur du thread appelant, juste après l'échec signalé.
    let code = unsafe { GetLastError() };
    io::Error::other(format!("{operation} failed with Win32 error {code}"))
}

/// Possède le mutex nommé tant que le processus vit ; la fermeture libère le nom.
pub struct InstanceGuard(Handle);

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: le handle a été renvoyé par CreateMutexW et n'est fermé qu'ici.
        unsafe { CloseHandle(self.0) };
    }
}

pub enum Acquire {
    /// Cette instance est la première : elle garde le mutex.
    Primary(InstanceGuard),
    /// Une autre instance existait et a accepté les chemins ; celle-ci doit se terminer.
    Forwarded,
    /// Une instance existe mais n'a pas répondu à temps ou a refusé : ne pas partager le dossier de données.
    Unreachable,
}

/// Schéma standard : `CreateMutexW` puis `GetLastError() == ERROR_ALREADY_EXISTS`, sans course.
pub fn acquire(root: &Path, paths: &[PathBuf]) -> io::Result<Acquire> {
    let key = instance::instance_key(root);
    let name = wide(&instance::mutex_name(&key));
    // SAFETY: `name` est un tableau UTF-16 terminé par NUL vivant pendant l'appel ; attributs nuls autorisés.
    let handle = unsafe { CreateMutexW(null_mut(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(last_error("CreateMutexW"));
    }
    // SAFETY: lit l'erreur du thread juste après CreateMutexW (183 si le mutex existait déjà).
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if !existed {
        return Ok(Acquire::Primary(InstanceGuard(handle)));
    }
    // SAFETY: handle valide, non utilisé ensuite.
    unsafe { CloseHandle(handle) };
    let payload = instance::encode_paths(paths)
        .map_err(|error| io::Error::other(format!("invalid launch paths: {error:?}")))?;
    Ok(if forward(&key, &payload) {
        Acquire::Forwarded
    } else {
        Acquire::Unreachable
    })
}

/// Envoie la charge à la fenêtre principale par `SendMessageTimeoutW(WM_COPYDATA)` (la mémoire de
/// l'émetteur n'est valide que pendant un envoi synchrone). Réessaie brièvement tant que la fenêtre n'existe
/// pas ou ne répond pas ; un refus explicite du récepteur n'est jamais réessayé.
fn forward(key: &str, payload: &[u8]) -> bool {
    let class = wide(&instance::window_class_name(key));
    // SAFETY: pointer-free query.
    let own_pid = unsafe { GetCurrentProcessId() };
    for attempt in 0..FORWARD_ATTEMPTS {
        if attempt > 0 {
            // SAFETY: simple attente du thread appelant.
            unsafe { Sleep(FORWARD_DELAY_MS) };
        }
        // SAFETY: `class` est terminé par NUL ; un titre nul accepte tout titre.
        let hwnd = unsafe { FindWindowW(class.as_ptr(), null()) };
        if hwnd.is_null() {
            continue;
        }
        let mut target = 0u32;
        // SAFETY: hwnd renvoyé par FindWindowW ; `target` est inscriptible.
        unsafe { GetWindowThreadProcessId(hwnd, &mut target) };
        if target != 0 {
            // SAFETY: autorise le processus cible à prendre le premier plan (appelé côté émetteur).
            unsafe { AllowSetForegroundWindow(target) };
        }
        let data = CopyDataStruct {
            dw_data: PROTOCOL_ID,
            cb_data: payload.len() as u32,
            lp_data: if payload.is_empty() {
                null_mut()
            } else {
                payload.as_ptr() as *mut c_void
            },
        };
        let mut result = 0usize;
        // SAFETY: `data` et `payload` vivent pendant tout l'envoi synchrone ; le récepteur ne modifie pas la charge.
        let sent = unsafe {
            SendMessageTimeoutW(
                hwnd,
                WM_COPYDATA,
                own_pid as usize,
                &data as *const CopyDataStruct as isize,
                SMTO_BLOCK | SMTO_ABORTIFHUNG,
                SEND_TIMEOUT_MS,
                &mut result,
            )
        };
        if sent != 0 {
            return result == 1;
        }
    }
    false
}

/// Le processus `pid` exécute-t-il le même exécutable que celui-ci ?
fn sender_runs_this_executable(pid: u32) -> bool {
    let Ok(own) = std::env::current_exe() else {
        return false;
    };
    // SAFETY: ouverture à droits minimaux ; le handle est fermé plus bas.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return false;
    }
    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    // SAFETY: `buffer` a `size` unités inscriptibles ; `process` est valide.
    let ok = unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size) };
    // SAFETY: handle valide obtenu ci-dessus.
    unsafe { CloseHandle(process) };
    if ok == 0 || size as usize > buffer.len() {
        return false;
    }
    let other = String::from_utf16_lossy(&buffer[..size as usize]);
    normalized(&other) == normalized(&own.to_string_lossy())
}

fn normalized(path: &str) -> String {
    path.replace('/', "\\")
        .trim_start_matches("\\\\?\\")
        .to_lowercase()
}

/// Valide et décode un `WM_COPYDATA` reçu par une fenêtre d'instance. `None` : à refuser (renvoyer 0).
///
/// # Safety
/// `l` doit être la valeur `lParam` d'un `WM_COPYDATA` en cours de traitement (pointeur nul ou
/// `COPYDATASTRUCT` valide pendant l'appel).
pub(crate) unsafe fn decode_copydata(w: usize, l: isize) -> Option<Vec<PathBuf>> {
    if l == 0 {
        return None;
    }
    // SAFETY: contrat de WM_COPYDATA ; la structure n'est lue que pendant ce message.
    let data = unsafe { &*(l as *const CopyDataStruct) };
    let size = data.cb_data as usize;
    if size > instance::MAX_PAYLOAD_BYTES {
        return None;
    }
    let bytes = if data.lp_data.is_null() {
        if size != 0 {
            return None;
        }
        None
    } else {
        // SAFETY: Windows a copié `cb_data` octets lisibles à `lp_data` pour la durée du message ;
        // la taille est bornée ci-dessus.
        Some(unsafe { std::slice::from_raw_parts(data.lp_data as *const u8, size) })
    };
    let trusted = u32::try_from(w).is_ok_and(sender_runs_this_executable);
    instance::process_copydata(data.dw_data, bytes, trusted).ok()
}

/// Met la fenêtre au premier plan : restauration si réduite, puis `SetForegroundWindow`
/// (autorisé par `AllowSetForegroundWindow` côté émetteur).
pub(crate) fn bring_to_front(hwnd: Handle) {
    // SAFETY: hwnd est la fenêtre vivante du thread appelant.
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
}

static PROBE_OUTPUT: OnceLock<PathBuf> = OnceLock::new();

unsafe extern "system" fn probe_proc(hwnd: Handle, message: u32, w: usize, l: isize) -> isize {
    let outcome = std::panic::catch_unwind(|| match message {
        WM_COPYDATA => {
            // SAFETY: lParam provient du WM_COPYDATA en cours.
            match unsafe { decode_copydata(w, l) } {
                Some(paths) => {
                    if let Some(out) = PROBE_OUTPUT.get() {
                        let text: String = paths
                            .iter()
                            .map(|path| format!("{}\n", path.display()))
                            .collect();
                        let _ = std::fs::write(out, text);
                    }
                    // SAFETY: arrête la boucle de messages de ce thread.
                    unsafe { PostQuitMessage(0) };
                    Some(1)
                }
                None => Some(0),
            }
        }
        WM_TIMER => {
            // SAFETY: idem ; le délai d'attente a expiré sans réception.
            unsafe { PostQuitMessage(3) };
            Some(0)
        }
        _ => None,
    });
    match outcome {
        Ok(Some(value)) => value,
        // SAFETY: traitement par défaut avec les arguments reçus de l'OS.
        _ => unsafe { DefWindowProcW(hwnd, message, w, l) },
    }
}

/// Diagnostic `--instance-probe-primary` : prend le mutex, crée une fenêtre cachée de la même classe
/// que la fenêtre principale, attend un `WM_COPYDATA` valide, l'écrit dans `output`, puis se termine.
pub fn probe_primary(root: &Path, output: &Path) -> io::Result<String> {
    let guard = match acquire(root, &[])? {
        Acquire::Primary(guard) => guard,
        _ => return Err(io::Error::other("an instance already owns this data root")),
    };
    let _ = PROBE_OUTPUT.set(output.to_path_buf());
    let key = instance::instance_key(root);
    let class_name = wide(&instance::window_class_name(&key));
    // SAFETY: handle du module courant.
    let module = unsafe { GetModuleHandleW(null()) };
    let class = WndClassW {
        style: 0,
        proc: Some(probe_proc),
        class_extra: 0,
        window_extra: 0,
        instance: module,
        icon: null_mut(),
        cursor: null_mut(),
        background: null_mut(),
        menu_name: null(),
        class_name: class_name.as_ptr(),
    };
    // SAFETY: la classe et son nom restent vivants pendant l'enregistrement.
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(last_error("RegisterClassW"));
    }
    let title = wide("Atelier probe");
    // SAFETY: fenêtre cachée (non montrée) créée sur ce thread ; chaînes vivantes pendant l'appel.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            module,
            null_mut(),
        )
    };
    if hwnd.is_null() {
        return Err(last_error("CreateWindowExW"));
    }
    // SAFETY: hwnd est vivante ; le délai évite une sonde orpheline.
    unsafe { SetTimer(hwnd, 1, PROBE_TIMEOUT_MS as u32, null_mut()) };
    println!("primary-ready");
    let mut msg = Msg {
        hwnd: null_mut(),
        message: 0,
        w_param: 0,
        l_param: 0,
        time: 0,
        point: Point { x: 0, y: 0 },
        private: 0,
    };
    let code = loop {
        // SAFETY: `msg` est inscriptible ; un HWND nul lit la file du thread.
        let result = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
        if result == -1 {
            return Err(last_error("GetMessageW"));
        }
        if result == 0 {
            break msg.w_param as i32;
        }
        // SAFETY: `msg` vient de GetMessageW.
        unsafe { DispatchMessageW(&msg) };
    };
    drop(guard);
    if code == 0 {
        Ok("primary-received".into())
    } else {
        Err(io::Error::other(
            "probe timed out without a valid WM_COPYDATA",
        ))
    }
}

/// Diagnostic `--instance-probe-secondary` : se comporte comme une seconde instance.
pub fn probe_secondary(root: &Path, paths: &[PathBuf]) -> io::Result<String> {
    Ok(match acquire(root, paths)? {
        Acquire::Primary(_) => "became-primary".into(),
        Acquire::Forwarded => "forwarded".into(),
        Acquire::Unreachable => "unreachable".into(),
    })
}
