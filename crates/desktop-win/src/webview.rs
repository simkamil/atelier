// SPDX-License-Identifier: AGPL-3.0-or-later
//! Minimal WebView2 hosting through the official static loader and hand-written COM ABI.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::io;
use std::marker::PhantomData;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

#[path = "webview-resources.rs"]
mod embedded_resources;
use crate::resources;

pub(crate) const WEBVIEW_MESSAGE: u32 = 0x8000 + 0x111;
pub(crate) const WEBVIEW_RUNTIME_URL: &str =
    "https://developer.microsoft.com/microsoft-edge/webview2/";
const S_OK: i32 = 0;
const E_POINTER: i32 = 0x8000_4003u32 as i32;
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
const WM_CLOSE_DRAIN_TIMEOUT_MS: u32 = 15_000;
pub(super) static LIVE_CALLBACK_OBJECTS: AtomicUsize = AtomicUsize::new(0);

/// Test-only reference ledger. `ACQUIRED`/`RELEASED` count the COM references this module obtains
/// (AddRef or owned out-parameter) and releases; they are per thread (the STA thread of one test) so
/// parallel tests cannot disturb each other. `OUR_ADD_REF`/`OUR_RELEASE` count calls on the objects
/// this module implements and are process-wide because the runtime may call them from any thread;
/// tests measure deltas while holding `CALLBACK_TEST_LOCK`.
#[cfg(test)]
pub(super) mod ledger {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    thread_local! {
        pub(crate) static ACQUIRED: Cell<usize> = const { Cell::new(0) };
        pub(crate) static RELEASED: Cell<usize> = const { Cell::new(0) };
    }
    pub(crate) static OUR_ADD_REF: AtomicUsize = AtomicUsize::new(0);
    pub(crate) static OUR_RELEASE: AtomicUsize = AtomicUsize::new(0);
    pub(crate) fn bump(counter: &'static std::thread::LocalKey<Cell<usize>>) {
        counter.with(|value| value.set(value.get() + 1));
    }
    pub(crate) fn read(counter: &'static std::thread::LocalKey<Cell<usize>>) -> usize {
        counter.with(Cell::get)
    }
    pub(crate) fn bump_ours(counter: &AtomicUsize) {
        counter.fetch_add(1, Ordering::AcqRel);
    }
    pub(crate) fn read_ours(counter: &AtomicUsize) -> usize {
        counter.load(Ordering::Acquire)
    }
}

/// Records one owned COM reference obtained from an out-parameter (test builds only).
#[inline(always)]
pub(super) fn note_acquired() {
    #[cfg(test)]
    ledger::bump(&ledger::ACQUIRED);
}

/// Adds one reference to a borrowed interface that this module will release itself.
///
/// # Safety
///
/// `interface` must be a non-null pointer to a live COM object, so that vtable slot 1 is `AddRef`.
/// The caller then owns one reference and must release it exactly once with `release_interface`.
pub(super) unsafe fn add_ref_interface(interface: *mut c_void) {
    note_acquired();
    // SAFETY: IUnknown::AddRef is vtable slot 1 for every COM interface; callers pass a live pointer.
    unsafe {
        call_vtable::<unsafe extern "system" fn(*mut c_void) -> u32>(interface, 1)(interface)
    };
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WebRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const IID_IUNKNOWN: Guid = Guid {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
const IID_ENVIRONMENT: Guid = Guid {
    data1: 0x4e8a3389,
    data2: 0xc9d8,
    data3: 0x4bd2,
    data4: [0xb6, 0xb5, 0x12, 0x4f, 0xee, 0x6c, 0xc1, 0x4d],
};
const IID_CONTROLLER: Guid = Guid {
    data1: 0x6c4819f3,
    data2: 0xc9b7,
    data3: 0x4260,
    data4: [0x81, 0x27, 0xc9, 0xf5, 0xbd, 0xe7, 0xf6, 0x8c],
};
const IID_EXECUTE_SCRIPT_HANDLER: Guid = Guid {
    data1: 0x49511172,
    data2: 0xcc67,
    data3: 0x4bca,
    data4: [0x99, 0x23, 0x13, 0x71, 0x12, 0xf4, 0xc4, 0xcc],
};

#[link(name = "WebView2LoaderStatic")]
unsafe extern "system" {
    fn GetAvailableCoreWebView2BrowserVersionString(
        browser_executable_folder: *const u16,
        version_info: *mut *mut u16,
    ) -> i32;
    fn CreateCoreWebView2EnvironmentWithOptions(
        browser_executable_folder: *const u16,
        user_data_folder: *const u16,
        environment_options: *mut c_void,
        handler: *mut c_void,
    ) -> i32;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(memory: *mut c_void);
}

#[link(name = "user32")]
unsafe extern "system" {
    fn PostMessageW(hwnd: *mut c_void, message: u32, w: usize, l: isize) -> i32;
}

pub(crate) struct ComApartment {
    initialized: bool,
    _not_send: PhantomData<Rc<()>>,
}

impl ComApartment {
    pub(crate) fn initialize_sta() -> io::Result<Self> {
        const COINIT_APARTMENTTHREADED: u32 = 0x2;
        // SAFETY: initializes COM for this GUI thread and takes no pointers.
        let hr = unsafe { CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED) };
        if hr < 0 {
            return Err(hresult_error(
                "CoInitializeEx(COINIT_APARTMENTTHREADED)",
                hr,
            ));
        }
        Ok(Self {
            initialized: true,
            _not_send: PhantomData,
        })
    }
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: this balances the successful CoInitializeEx on the current thread.
            unsafe { CoUninitialize() };
            self.initialized = false;
        }
    }
}

pub(crate) fn probe_runtime() -> io::Result<Option<String>> {
    let mut version = null_mut();
    // SAFETY: the system loader writes an optional CoTaskMem allocation to version.
    let hr = unsafe { GetAvailableCoreWebView2BrowserVersionString(null(), &mut version) };
    if hr < 0 || version.is_null() {
        if !version.is_null() {
            // SAFETY: successful API output is allocated with CoTaskMem and is freed exactly once.
            unsafe { CoTaskMemFree(version.cast()) };
        }
        return Ok(None);
    }
    // SAFETY: the successful API returns a NUL-terminated UTF-16 string.
    let value = unsafe {
        let mut len = 0usize;
        while *version.add(len) != 0 && len < 32_768 {
            len += 1;
        }
        if len == 32_768 {
            CoTaskMemFree(version.cast());
            return Err(io::Error::other(
                "WebView2 runtime version string is unterminated",
            ));
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(version, len))
    };
    // SAFETY: version is the allocation returned by the preceding loader call.
    unsafe { CoTaskMemFree(version.cast()) };
    Ok(Some(value))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Default)]
struct Shared {
    hwnd: AtomicUsize,
    closing: AtomicBool,
    callback_objects: AtomicUsize,
    pending: AtomicUsize,
    controller: Mutex<Option<usize>>,
    environment: Mutex<Option<usize>>,
    error: Mutex<Option<String>>,
    ready: AtomicBool,
    resource_environment: AtomicUsize,
    attempts: Mutex<resources::AttemptLog>,
    registrations: Mutex<embedded_resources::Registrations>,
    script_results: Mutex<VecDeque<io::Result<String>>>,
    file_messages: Mutex<crate::file_bridge::Queue>,
    file_working: AtomicBool,
    deferred_file_wake: AtomicBool,
    deferred_file_close: AtomicBool,
}

impl Shared {
    fn notify(&self) {
        let hwnd = self.hwnd.load(Ordering::Acquire) as *mut c_void;
        if !hwnd.is_null() {
            // SAFETY: this is a live-window notification; no pointer is retained.
            unsafe { PostMessageW(hwnd, WEBVIEW_MESSAGE, 0, 0) };
        }
    }

    fn fail(&self, message: String) {
        *lock(&self.error) = Some(message);
        self.ready.store(true, Ordering::Release);
        self.notify();
    }

    fn completed(&self) {
        self.pending.fetch_sub(1, Ordering::AcqRel);
        self.notify();
    }
}

pub(crate) struct FileWorkGuard {
    shared: Arc<Shared>,
}

impl Drop for FileWorkGuard {
    fn drop(&mut self) {
        self.shared.file_working.store(false, Ordering::Release);
        if self
            .shared
            .deferred_file_close
            .swap(false, Ordering::AcqRel)
        {
            let hwnd = self.shared.hwnd.load(Ordering::Acquire) as *mut c_void;
            if !hwnd.is_null() {
                // SAFETY: the deferred close targets the still-owned HWND after the modal operation returns.
                unsafe { PostMessageW(hwnd, 0x0010, 0, 0) };
            }
        }
        if self.shared.deferred_file_wake.swap(false, Ordering::AcqRel) {
            self.shared.notify();
        }
    }
}

#[repr(C)]
struct HandlerVtable {
    query_interface: unsafe extern "system" fn(*mut Handler, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut Handler) -> u32,
    release: unsafe extern "system" fn(*mut Handler) -> u32,
    invoke: unsafe extern "system" fn(*mut Handler, i32, *mut c_void) -> i32,
}

#[repr(C)]
struct Handler {
    vtable: *const HandlerVtable,
    refs: AtomicU32,
    kind: CallbackKind,
    shared: *const Shared,
    iid: Guid,
}

#[derive(Clone, Copy)]
enum CallbackKind {
    Environment,
    Controller,
    ExecuteScript,
}

static HANDLER_VTABLE: HandlerVtable = HandlerVtable {
    query_interface: handler_query_interface,
    add_ref: handler_add_ref,
    release: handler_release,
    invoke: handler_invoke,
};

unsafe extern "system" fn handler_query_interface(
    this: *mut Handler,
    iid: *const Guid,
    output: *mut *mut c_void,
) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if this.is_null() || iid.is_null() || output.is_null() {
            return E_POINTER;
        }
        // SAFETY: pointers were checked for null and are COM ABI arguments valid for this call.
        unsafe { *output = null_mut() };
        // SAFETY: iid and this point to the caller's valid COM values.
        let (requested, supported) = unsafe { (*iid, (*this).iid) };
        if requested != IID_IUNKNOWN && requested != supported {
            return E_NOINTERFACE;
        }
        // SAFETY: output is valid and `this` is the interface pointer being queried.
        unsafe {
            *output = this.cast();
            handler_add_ref(this);
        }
        S_OK
    }))
    .unwrap_or(E_NOINTERFACE)
}

unsafe extern "system" fn handler_add_ref(this: *mut Handler) -> u32 {
    if this.is_null() {
        return 0;
    }
    #[cfg(test)]
    ledger::bump_ours(&ledger::OUR_ADD_REF);
    // SAFETY: COM invokes AddRef only on a live handler pointer.
    unsafe {
        (*this)
            .refs
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
    }
}

unsafe extern "system" fn handler_release(this: *mut Handler) -> u32 {
    if this.is_null() {
        return 0;
    }
    #[cfg(test)]
    ledger::bump_ours(&ledger::OUR_RELEASE);
    // SAFETY: COM invokes Release only on a live handler pointer and each reference is released once.
    let remaining = unsafe {
        (*this)
            .refs
            .fetch_sub(1, Ordering::AcqRel)
            .saturating_sub(1)
    };
    if remaining == 0 {
        // SAFETY: the final COM reference owns this allocation.
        unsafe {
            let boxed = Box::from_raw(this);
            // Keep a reconstructed Arc alive while touching Shared: this may be the last
            // reference after the hosting window has closed.
            let shared = Arc::from_raw(boxed.shared);
            shared.callback_objects.fetch_sub(1, Ordering::AcqRel);
            LIVE_CALLBACK_OBJECTS.fetch_sub(1, Ordering::AcqRel);
            drop(shared);
            drop(boxed);
        }
    }
    remaining
}

unsafe extern "system" fn handler_invoke(
    this: *mut Handler,
    error_code: i32,
    result: *mut c_void,
) -> i32 {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if this.is_null() {
            return E_POINTER;
        }
        // SAFETY: COM invokes this method on the live handler and retains the shared Arc for its lifetime.
        let (kind, shared_ptr) = unsafe { ((*this).kind, (*this).shared) };
        if shared_ptr.is_null() {
            return E_POINTER;
        }
        // SAFETY: shared is an Arc allocation held by this handler until its final Release.
        let shared = unsafe { &*shared_ptr };
        if matches!(kind, CallbackKind::ExecuteScript) {
            let script_result = if error_code < 0 {
                Err(hresult_error("ICoreWebView2::ExecuteScript", error_code))
            } else if result.is_null() {
                Err(io::Error::other(
                    "ExecuteScript returned a null JSON result",
                ))
            } else {
                // ExecuteScriptCompletedHandler::Invoke receives an LPCWSTR borrowed for this call.
                match copy_webview_string(result.cast_const().cast()) {
                    Some(value) => Ok(value),
                    None => Err(io::Error::other(
                        "ExecuteScript result string is unterminated",
                    )),
                }
            };
            lock(&shared.script_results).push_back(script_result);
            shared.completed();
            return S_OK;
        }
        let mut callback_error = None;
        if error_code < 0 {
            callback_error =
                Some(hresult_error("WebView2 asynchronous initialization", error_code).to_string());
        } else if result.is_null() {
            callback_error = Some("WebView2 initialization returned a null interface".to_owned());
        } else {
            match kind {
                CallbackKind::Environment => {
                    if !shared.closing.load(Ordering::Acquire) {
                        let env = result;
                        // Keep the environment alive for this host and allow subsequent controllers
                        // to share the same browser environment on the owning STA.
                        // SAFETY: Invoke supplies a live borrowed environment interface.
                        unsafe { add_ref_interface(env) };
                        // The resource callback needs the environment independently of the host's
                        // initialization slot so it remains valid until callbacks are removed.
                        unsafe { add_ref_interface(env) };
                        shared
                            .resource_environment
                            .store(env as usize, Ordering::Release);
                        *lock(&shared.environment) = Some(env as usize);
                        let hwnd = shared.hwnd.load(Ordering::Acquire) as *mut c_void;
                        #[cfg(test)]
                        if leak_probe::VARIANT.load(Ordering::Acquire) == 2 {
                            // measurement variant: environment only, no controller is created
                            shared.completed();
                            return S_OK;
                        }
                        if hwnd.is_null() {
                            callback_error = Some(
                                "WebView2 parent window was destroyed during initialization"
                                    .to_owned(),
                            );
                        } else {
                            // ICoreWebView2Environment::CreateCoreWebView2Controller is vtable slot 3.
                            // Add a temporary Arc ref for the new async handler.
                            // SAFETY: the current handler owns a strong Arc ref to this Shared value.
                            unsafe { Arc::increment_strong_count(shared as *const Shared) };
                            let callback = new_handler(
                                unsafe { Arc::from_raw(shared as *const Shared) },
                                CallbackKind::Controller,
                            );
                            // SAFETY: env is the live callback result; vtable slot and signature follow WebView2.h.
                            let hr = unsafe {
                                call_vtable::<
                                    unsafe extern "system" fn(
                                        *mut c_void,
                                        *mut c_void,
                                        *mut Handler,
                                    )
                                        -> i32,
                                >(env, 3)(env, hwnd, callback)
                            };
                            // Release the caller-owned handler ref; WebView2 retains its own ref when it queues the callback.
                            unsafe { handler_release(callback) };
                            if hr < 0 {
                                shared.pending.fetch_sub(1, Ordering::AcqRel);
                                callback_error = Some(
                                    hresult_error("CreateCoreWebView2Controller", hr).to_string(),
                                );
                            }
                        }
                    }
                }
                CallbackKind::Controller => {
                    if shared.closing.load(Ordering::Acquire) {
                        // AddRef the borrowed completion result before Close/Release.
                        // SAFETY: result is the live non-null controller supplied by the callback.
                        unsafe { add_ref_interface(result) };
                        // SAFETY: result is the live controller passed by the completion callback.
                        if let Err(error) = unsafe { close_and_release_controller(result) } {
                            callback_error = Some(error.to_string());
                        }
                    } else {
                        if let Err(error) = configure_controller(result, shared) {
                            // AddRef the borrowed result so Close and Release balance locally.
                            // SAFETY: `result` is the valid controller supplied to Invoke.
                            unsafe { add_ref_interface(result) };
                            callback_error =
                                Some(match unsafe { close_and_release_controller(result) } {
                                    Ok(()) => error.to_string(),
                                    Err(close_error) => format!("{error}; {close_error}"),
                                });
                        } else {
                            // The result is borrowed by Invoke; AddRef it before storing beyond this call.
                            // SAFETY: IUnknown::AddRef is vtable slot 1 of this non-null COM interface.
                            unsafe { add_ref_interface(result) };
                            *lock(&shared.controller) = Some(result as usize);
                            shared.ready.store(true, Ordering::Release);
                        }
                    }
                }
                CallbackKind::ExecuteScript => {
                    unreachable!("handled before initialization callback dispatch")
                }
            }
        }
        if let Some(message) = callback_error {
            shared.fail(message);
        }
        shared.completed();
        S_OK
    }));
    outcome.unwrap_or(E_NOINTERFACE)
}

fn new_handler(shared: Arc<Shared>, kind: CallbackKind) -> *mut Handler {
    #[cfg(test)]
    ledger::bump_ours(&ledger::OUR_ADD_REF); // the creation reference
    shared.callback_objects.fetch_add(1, Ordering::AcqRel);
    LIVE_CALLBACK_OBJECTS.fetch_add(1, Ordering::AcqRel);
    shared.pending.fetch_add(1, Ordering::AcqRel);
    let (iid, shared_ptr) = (
        match kind {
            CallbackKind::Environment => IID_ENVIRONMENT,
            CallbackKind::Controller => IID_CONTROLLER,
            CallbackKind::ExecuteScript => IID_EXECUTE_SCRIPT_HANDLER,
        },
        Arc::into_raw(shared),
    );
    Box::into_raw(Box::new(Handler {
        vtable: &HANDLER_VTABLE,
        refs: AtomicU32::new(1),
        kind,
        shared: shared_ptr,
        iid,
    }))
}

fn configure_controller(controller: *mut c_void, shared: &Shared) -> io::Result<()> {
    #[cfg(test)]
    if leak_probe::VARIANT.load(Ordering::Acquire) == 1 {
        return Ok(()); // measurement variant: bare controller without our configuration
    }
    // ICoreWebView2Controller::get_CoreWebView2 is SDK vtable slot 25.
    let mut core = null_mut();
    // SAFETY: controller is the live result argument from the completion callback.
    let hr = unsafe {
        call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32>(
            controller, 25,
        )(controller, &mut core)
    };
    check_hresult("ICoreWebView2Controller::get_CoreWebView2", hr)?;
    if !core.is_null() {
        note_acquired();
    }
    if core.is_null() {
        return Err(io::Error::other(
            "ICoreWebView2Controller::get_CoreWebView2 returned null",
        ));
    }
    let result =
        (|| {
            // ICoreWebView2::get_Settings is SDK vtable slot 3.
            let mut settings = null_mut();
            // SAFETY: core is the live interface returned above.
            let hr = unsafe {
                call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32>(
                    core, 3,
                )(core, &mut settings)
            };
            check_hresult("ICoreWebView2::get_Settings", hr)?;
            if !settings.is_null() {
                note_acquired();
            }
            if settings.is_null() {
                return Err(io::Error::other(
                    "ICoreWebView2::get_Settings returned null",
                ));
            }
            let enabled = i32::from(cfg!(debug_assertions));
            // ICoreWebView2Settings::put_AreDevToolsEnabled is SDK vtable slot 12.
            // SAFETY: settings is live and BOOL is represented by i32.
            let set_hr = unsafe {
                call_vtable::<unsafe extern "system" fn(*mut c_void, i32) -> i32>(settings, 12)(
                    settings, enabled,
                )
            };
            let result = check_hresult("ICoreWebView2Settings::put_AreDevToolsEnabled", set_hr)
                .and_then(|()| {
                    // get_AreDevToolsEnabled is SDK vtable slot 11.
                    let mut actual = 0i32;
                    // SAFETY: output is initialized writable storage and settings is live.
                    let get_hr = unsafe {
                        call_vtable::<unsafe extern "system" fn(*mut c_void, *mut i32) -> i32>(
                            settings, 11,
                        )(settings, &mut actual)
                    };
                    check_hresult("ICoreWebView2Settings::get_AreDevToolsEnabled", get_hr)?;
                    if actual != enabled {
                        return Err(io::Error::other(format!(
                            "WebView2 DevTools setting mismatch: expected {enabled}, got {actual}"
                        )));
                    }
                    validate_devtools(actual != 0)
                });
            // SAFETY: get_Settings returned one owned COM reference.
            unsafe { release_interface(settings) };
            result.and_then(|()| {
                embedded_resources::install(core, shared)?;
                // ICoreWebView2::Navigate is SDK vtable slot 5.
                let uri: Vec<u16> = resources::ORIGIN
                    .to_owned()
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                // SAFETY: core is live, URI is NUL-terminated and remains valid for the call.
                let hr = unsafe {
                    call_vtable::<unsafe extern "system" fn(*mut c_void, *const u16) -> i32>(
                        core, 5,
                    )(core, uri.as_ptr())
                };
                check_hresult("ICoreWebView2::Navigate(embedded origin)", hr)
            })
        })();
    // SAFETY: get_CoreWebView2 returned one owned COM reference.
    unsafe { release_interface(core) };
    result
}

unsafe fn call_vtable<F: Copy>(interface: *mut c_void, slot: usize) -> F {
    // SAFETY: callers pass a non-null WebView2 interface and a vtable index defined by the SDK header.
    let table = unsafe { *(interface.cast::<*const *const c_void>()) };
    // SAFETY: each caller provides the matching function-pointer type for the selected ABI slot.
    unsafe { std::mem::transmute_copy(&*table.add(slot)) }
}

fn hresult_error(operation: &str, hr: i32) -> io::Error {
    io::Error::other(format!(
        "{operation} failed with HRESULT 0x{:08x}",
        hr as u32
    ))
}

fn copy_webview_string(value: *const u16) -> Option<String> {
    if value.is_null() {
        return None;
    }
    // SAFETY: ExecuteScriptCompletedHandler::Invoke guarantees a readable NUL-terminated LPCWSTR
    // for the duration of the callback; this function copies it and never assumes ownership.
    unsafe {
        let mut length = 0usize;
        while length < 1_048_576 && *value.add(length) != 0 {
            length += 1;
        }
        if length == 1_048_576 {
            None
        } else {
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                value, length,
            )))
        }
    }
}

unsafe fn release_interface(interface: *mut c_void) {
    if !interface.is_null() {
        #[cfg(test)]
        ledger::bump(&ledger::RELEASED);
        // SAFETY: IUnknown::Release is vtable slot 2 for all COM interfaces.
        unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void) -> u32>(interface, 2)(interface)
        };
    }
}

unsafe fn close_and_release_controller(interface: *mut c_void) -> io::Result<()> {
    if !interface.is_null() {
        // ICoreWebView2Controller::Close follows IUnknown and its 22 interface methods in the SDK vtable.
        // Actual SDK slot is 24 (get_CoreWebView2 is 25).
        // SAFETY: interface is a valid controller supplied by the completion handler.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void) -> i32>(interface, 24)(interface)
        };
        // SAFETY: balance the result reference owned by this callback.
        unsafe { release_interface(interface) };
        check_hresult("ICoreWebView2Controller::Close", hr)?;
    }
    Ok(())
}

pub(crate) struct Environment {
    interface: usize,
    _not_send: PhantomData<Rc<()>>,
}

impl Drop for Environment {
    fn drop(&mut self) {
        // SAFETY: this owns one environment reference, released on its creating STA.
        unsafe { release_interface(self.interface as *mut c_void) };
    }
}

pub(crate) struct WebView {
    shared: Arc<Shared>,
    controller: Option<usize>,
    closed: bool,
    environment: Option<Environment>,
    _not_send: PhantomData<Rc<()>>,
}

impl WebView {
    pub(crate) fn begin(hwnd: *mut c_void, user_data: &Path) -> io::Result<Self> {
        ready_gate(probe_runtime, |_| Self::begin_ready(hwnd, user_data))
    }

    fn begin_ready(hwnd: *mut c_void, user_data: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(user_data)?;
        let hwnd_value = hwnd as usize;
        let shared = Arc::new(Shared {
            hwnd: AtomicUsize::new(hwnd_value),
            ..Shared::default()
        });
        let path: Vec<u16> = user_data.as_os_str().encode_wide().chain(Some(0)).collect();
        let callback = new_handler(Arc::clone(&shared), CallbackKind::Environment);
        // SAFETY: paths are NUL-terminated, callback is a live COM object, loader is statically linked.
        let hr = unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                null(),
                path.as_ptr(),
                null_mut(),
                callback.cast(),
            )
        };
        // The caller-owned ref is released after API returns; the loader owns any queued callback ref.
        unsafe { handler_release(callback) };
        if hr < 0 {
            shared.pending.fetch_sub(1, Ordering::AcqRel);
            return Err(hresult_error(
                "CreateCoreWebView2EnvironmentWithOptions",
                hr,
            ));
        }
        Ok(Self {
            shared,
            controller: None,
            closed: false,
            environment: None,
            _not_send: PhantomData,
        })
    }

    pub(crate) fn take_completed(&mut self) -> io::Result<bool> {
        if let Some(message) = lock(&self.shared.error).take() {
            return Err(io::Error::other(message));
        }
        if self.environment.is_none() {
            if let Some(interface) = lock(&self.shared.environment).take() {
                self.environment = Some(Environment {
                    interface,
                    _not_send: PhantomData,
                });
            }
        }
        if self.controller.is_none() {
            if let Some(controller) = lock(&self.shared.controller).take() {
                self.controller = Some(controller);
            }
        }
        Ok(self.shared.ready.load(Ordering::Acquire) && self.controller.is_some())
    }

    pub(crate) fn pending_callbacks(&self) -> usize {
        self.shared.pending.load(Ordering::Acquire)
    }

    pub(crate) fn take_file_message(&self) -> Option<crate::file_bridge::Request> {
        lock(&self.shared.file_messages).pop()
    }

    pub(crate) fn pending_dirty_hint(&self) -> Option<bool> {
        lock(&self.shared.file_messages).dirty_hint()
    }

    pub(crate) fn take_file_rejection(&self) -> Option<String> {
        lock(&self.shared.file_messages).pop_rejection()
    }

    /// Keep a native file drain exclusive across modal STA dialogs, whose
    /// message pump can dispatch a nested WEBVIEW_MESSAGE callback.
    pub(crate) fn begin_file_work(&self) -> Option<FileWorkGuard> {
        match self.shared.file_working.compare_exchange(
            false,
            true,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Some(FileWorkGuard {
                shared: Arc::clone(&self.shared),
            }),
            Err(_) => {
                self.shared
                    .deferred_file_wake
                    .store(true, Ordering::Release);
                None
            }
        }
    }

    pub(crate) fn file_work_active(&self) -> bool {
        self.shared.file_working.load(Ordering::Acquire)
    }

    pub(crate) fn defer_close_until_file_work_finishes(&self) {
        self.shared
            .deferred_file_close
            .store(true, Ordering::Release);
    }

    pub(crate) fn post_web_message_as_string(&self, message: &str) -> io::Result<()> {
        let Some(controller) = self.controller else {
            return Err(io::Error::other("WebView2 controller is not ready"));
        };
        let mut core = null_mut();
        // SAFETY: controller is an owned live COM interface; slot 25 is get_CoreWebView2.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32>(
                controller as *mut c_void,
                25,
            )(controller as *mut c_void, &mut core)
        };
        check_hresult("ICoreWebView2Controller::get_CoreWebView2", hr)?;
        if !core.is_null() {
            note_acquired();
        }
        if core.is_null() {
            return Err(io::Error::other("WebView2 core interface is null"));
        }
        let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
        // The pinned SDK's ICoreWebView2 vtable places PostWebMessageAsString at slot 33.
        // SAFETY: `message` is NUL-terminated and remains live for the synchronous COM call.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *const u16) -> i32>(core, 33)(
                core,
                message.as_ptr(),
            )
        };
        // SAFETY: get_CoreWebView2 returned one owned interface reference.
        unsafe { release_interface(core) };
        check_hresult("ICoreWebView2::PostWebMessageAsString", hr)
    }

    pub(crate) fn resource_log_snapshot(&self) -> Vec<String> {
        lock(&self.shared.attempts)
            .entries()
            .map(str::to_owned)
            .collect()
    }

    pub(crate) fn dropped_resource_attempts(&self) -> u64 {
        lock(&self.shared.attempts).dropped()
    }

    pub(crate) fn execute_script(&self, script: &str) -> io::Result<()> {
        let Some(controller) = self.controller else {
            return Err(io::Error::other("WebView2 controller is not ready"));
        };
        let mut core = null_mut();
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32>(
                controller as *mut c_void,
                25,
            )(controller as *mut c_void, &mut core)
        };
        check_hresult("ICoreWebView2Controller::get_CoreWebView2", hr)?;
        if !core.is_null() {
            note_acquired();
        }
        if core.is_null() {
            return Err(io::Error::other("WebView2 core interface is null"));
        }
        let script: Vec<u16> = script.encode_utf16().chain(Some(0)).collect();
        let callback = new_handler(Arc::clone(&self.shared), CallbackKind::ExecuteScript);
        // ICoreWebView2::ExecuteScript is SDK vtable slot 29.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *const u16, *mut Handler) -> i32>(
                core, 29,
            )(core, script.as_ptr(), callback)
        };
        unsafe {
            handler_release(callback);
            release_interface(core);
        }
        if hr < 0 {
            self.shared.pending.fetch_sub(1, Ordering::AcqRel);
            check_hresult("ICoreWebView2::ExecuteScript", hr)
        } else {
            Ok(())
        }
    }

    pub(crate) fn take_script_result(&self) -> Option<io::Result<String>> {
        lock(&self.shared.script_results).pop_front()
    }

    pub(crate) fn live_callback_objects() -> usize {
        LIVE_CALLBACK_OBJECTS.load(Ordering::Acquire)
    }

    pub(crate) fn devtools_enabled(&self) -> io::Result<bool> {
        let Some(controller) = self.controller else {
            return Err(io::Error::other("WebView2 controller is not ready"));
        };
        // ICoreWebView2Controller::get_CoreWebView2 is SDK slot 25.
        let mut core = null_mut();
        // SAFETY: controller is a live owned interface.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32>(
                controller as *mut c_void,
                25,
            )(controller as *mut c_void, &mut core)
        };
        check_hresult("ICoreWebView2Controller::get_CoreWebView2", hr)?;
        if !core.is_null() {
            note_acquired();
        }
        if core.is_null() {
            return Err(io::Error::other("WebView2 core interface is null"));
        }
        let result = (|| {
            let mut settings = null_mut();
            // SAFETY: core is a live interface returned by the preceding getter.
            let hr = unsafe {
                call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32>(
                    core, 3,
                )(core, &mut settings)
            };
            check_hresult("ICoreWebView2::get_Settings", hr)?;
            if !settings.is_null() {
                note_acquired();
            }
            if settings.is_null() {
                return Err(io::Error::other("WebView2 settings interface is null"));
            }
            let mut enabled = 0i32;
            // SAFETY: settings is live and enabled is writable BOOL storage.
            let hr = unsafe {
                call_vtable::<unsafe extern "system" fn(*mut c_void, *mut i32) -> i32>(settings, 11)(
                    settings,
                    &mut enabled,
                )
            };
            // SAFETY: get_Settings returned one owned COM reference.
            unsafe { release_interface(settings) };
            check_hresult("ICoreWebView2Settings::get_AreDevToolsEnabled", hr)?;
            Ok(enabled != 0)
        })();
        // SAFETY: get_CoreWebView2 returned one owned COM reference.
        unsafe { release_interface(core) };
        result
    }

    pub(crate) fn set_bounds(&self, width: i32, height: i32) -> io::Result<()> {
        let Some(controller) = self.controller else {
            return Ok(());
        };
        let bounds = WebRect {
            left: 0,
            top: 0,
            right: width.max(0),
            bottom: height.max(0),
        };
        // Controller put_Bounds is SDK vtable slot 6 and takes RECT by value.
        // SAFETY: controller is owned and live until close; bounds is initialized.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, WebRect) -> i32>(
                controller as *mut c_void,
                6,
            )(controller as *mut c_void, bounds)
        };
        check_hresult("ICoreWebView2Controller::put_Bounds", hr)
    }

    pub(crate) fn bounds(&self) -> io::Result<(i32, i32, i32, i32)> {
        let Some(controller) = self.controller else {
            return Err(io::Error::other("WebView2 controller is not ready"));
        };
        let mut bounds = WebRect::default();
        // Controller get_Bounds is SDK vtable slot 5.
        // SAFETY: controller is owned and live; bounds points to writable RECT storage.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *mut WebRect) -> i32>(
                controller as *mut c_void,
                5,
            )(controller as *mut c_void, &mut bounds)
        };
        check_hresult("ICoreWebView2Controller::get_Bounds", hr)?;
        Ok((bounds.left, bounds.top, bounds.right, bounds.bottom))
    }

    pub(crate) fn is_visible(&self) -> io::Result<bool> {
        let Some(controller) = self.controller else {
            return Err(io::Error::other("WebView2 controller is not ready"));
        };
        let mut visible = 0i32;
        // Controller get_IsVisible is SDK vtable slot 3.
        // SAFETY: controller is owned and live; visible points to writable BOOL storage.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *mut i32) -> i32>(
                controller as *mut c_void,
                3,
            )(controller as *mut c_void, &mut visible)
        };
        check_hresult("ICoreWebView2Controller::get_IsVisible", hr)?;
        Ok(visible != 0)
    }

    pub(crate) fn set_visible(&self, visible: bool) -> io::Result<()> {
        let Some(controller) = self.controller else {
            return Ok(());
        };
        // SAFETY: controller is live and BOOL is ABI-compatible with i32.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, i32) -> i32>(
                controller as *mut c_void,
                4,
            )(controller as *mut c_void, i32::from(visible))
        };
        check_hresult("ICoreWebView2Controller::put_IsVisible", hr)
    }

    pub(crate) fn move_focus_programmatic(&self) -> io::Result<()> {
        let Some(controller) = self.controller else {
            return Ok(());
        };
        const PROGRAMMATIC: i32 = 0;
        // SAFETY: controller is live; PROGRAMMATIC is the SDK enum value 0.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, i32) -> i32>(
                controller as *mut c_void,
                12,
            )(controller as *mut c_void, PROGRAMMATIC)
        };
        check_hresult("ICoreWebView2Controller::MoveFocus(PROGRAMMATIC)", hr)
    }

    pub(crate) fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.shared.closing.store(true, Ordering::Release);
        if let Err(error) = embedded_resources::unregister(&self.shared) {
            self.shared.fail(error.to_string());
        }
        let resource_environment = self.shared.resource_environment.swap(0, Ordering::AcqRel);
        if resource_environment != 0 {
            // SAFETY: resource_environment owns its dedicated AddRef, released on this STA.
            unsafe { release_interface(resource_environment as *mut c_void) };
        }
        if let Some(environment) = lock(&self.shared.environment).take() {
            // SAFETY: the shared slot owns this reference; no later callback stores after closing.
            unsafe { release_interface(environment as *mut c_void) };
        }
        if let Some(controller) = self.controller.take() {
            // SAFETY: this value owns the controller reference.
            if let Err(error) = unsafe { close_and_release_controller(controller as *mut c_void) } {
                self.shared.fail(error.to_string());
            }
        }
        if let Some(controller) = lock(&self.shared.controller).take() {
            // SAFETY: the shared slot owns the controller reference.
            if let Err(error) = unsafe { close_and_release_controller(controller as *mut c_void) } {
                self.shared.fail(error.to_string());
            }
        }
        self.environment.take();
    }

    pub(crate) fn close_for_check(&mut self) -> io::Result<()> {
        let pending = self.pending_callbacks();
        self.close();
        if let Some(message) = lock(&self.shared.error).take() {
            return Err(io::Error::other(message));
        }
        if pending != 0 {
            return Err(io::Error::other(format!(
                "WebView2 callbacks still pending during close: {pending}"
            )));
        }
        Ok(())
    }

    pub(crate) fn detach_window(&self) {
        self.shared.hwnd.store(0, Ordering::Release);
    }
}

fn ready_gate<T>(
    probe: impl FnOnce() -> io::Result<Option<T>>,
    create: impl FnOnce(T) -> io::Result<WebView>,
) -> io::Result<WebView> {
    let runtime = probe()?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Microsoft Edge WebView2 Evergreen runtime is unavailable",
        )
    })?;
    create(runtime)
}

impl Drop for WebView {
    fn drop(&mut self) {
        self.close();
    }
}

fn check_hresult(operation: &str, hr: i32) -> io::Result<()> {
    if hr < 0 {
        Err(hresult_error(operation, hr))
    } else {
        Ok(())
    }
}

pub(crate) fn timeout_ms() -> u32 {
    WM_CLOSE_DRAIN_TIMEOUT_MS
}

pub(crate) fn diagnostic() -> String {
    format!(
        "Le runtime Microsoft Edge WebView2 est introuvable ou ne peut pas démarrer.\n\nInstallez le runtime Evergreen depuis :\n{WEBVIEW_RUNTIME_URL}\n\nAppuyez sur Ctrl+C pour copier ce message et cette adresse."
    )
}

pub(crate) fn validate_devtools(enabled: bool) -> io::Result<()> {
    if enabled != cfg!(debug_assertions) {
        Err(io::Error::other(format!(
            "WebView2 DevTools state {enabled} does not match build configuration"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod leak_probe;

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) static CALLBACK_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn debug_tools_expectation_tracks_build_profile() {
        assert!(validate_devtools(cfg!(debug_assertions)).is_ok());
        assert!(validate_devtools(!cfg!(debug_assertions)).is_err());
    }

    #[test]
    fn runtime_missing_diagnostic_contains_copy_hint_and_official_url() {
        let text = diagnostic();
        assert!(text.contains("Ctrl+C"));
        assert!(text.contains(WEBVIEW_RUNTIME_URL));
    }

    #[test]
    fn missing_runtime_never_runs_cache_creation_and_ready_runtime_does() {
        use std::cell::Cell;
        let creations = Cell::new(0usize);
        let missing = ready_gate(
            || Ok::<_, io::Error>(None::<()>),
            |_| {
                creations.set(creations.get() + 1);
                Err(io::Error::other("must not run"))
            },
        );
        assert_eq!(
            missing.err().expect("runtime absent").kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(creations.get(), 0);

        let ready = ready_gate(
            || Ok::<_, io::Error>(Some("runtime")),
            |runtime| {
                assert_eq!(runtime, "runtime");
                creations.set(creations.get() + 1);
                Err(io::Error::other("injected create failure"))
            },
        );
        assert_eq!(
            ready.err().expect("injected HRESULT").to_string(),
            "injected create failure"
        );
        assert_eq!(creations.get(), 1);
    }

    #[test]
    fn callback_state_survives_host_drop_until_final_com_release() {
        let _serial = lock(&CALLBACK_TEST_LOCK);
        let baseline = LIVE_CALLBACK_OBJECTS.load(Ordering::Acquire);
        let shared = Arc::new(Shared::default());
        let handler = new_handler(Arc::clone(&shared), CallbackKind::Environment);
        assert_eq!(shared.callback_objects.load(Ordering::Acquire), 1);
        drop(shared);
        // SAFETY: `handler` owns the remaining Arc reference and its initial COM reference.
        unsafe { handler_release(handler) };
        assert_eq!(LIVE_CALLBACK_OBJECTS.load(Ordering::Acquire), baseline);
    }

    #[test]
    fn query_interface_accepts_unknown_and_exact_callback_iid_only() {
        let _serial = lock(&CALLBACK_TEST_LOCK);
        let baseline = LIVE_CALLBACK_OBJECTS.load(Ordering::Acquire);
        let handler = new_handler(Arc::new(Shared::default()), CallbackKind::Environment);
        let mut output = null_mut();
        // SAFETY: handler, requested IID, and output all live through each call.
        assert_eq!(
            unsafe { handler_query_interface(handler, &IID_IUNKNOWN, &mut output) },
            S_OK
        );
        assert_eq!(output, handler.cast());
        // SAFETY: this is the returned AddRef'd interface pointer.
        assert_eq!(unsafe { handler_release(output.cast()) }, 1);

        output = null_mut();
        // SAFETY: handler and IID are live; output is writable storage.
        assert_eq!(
            unsafe { handler_query_interface(handler, &IID_ENVIRONMENT, &mut output) },
            S_OK
        );
        assert_eq!(output, handler.cast());
        // SAFETY: release the returned reference.
        assert_eq!(unsafe { handler_release(output.cast()) }, 1);

        let unknown = Guid {
            data1: 7,
            ..IID_IUNKNOWN
        };
        output = null_mut();
        // SAFETY: handler, IID, and output are valid.
        assert_eq!(
            unsafe { handler_query_interface(handler, &unknown, &mut output) },
            E_NOINTERFACE
        );
        assert!(output.is_null());
        // SAFETY: this releases the initial handler reference.
        assert_eq!(unsafe { handler_release(handler) }, 0);
        assert_eq!(LIVE_CALLBACK_OBJECTS.load(Ordering::Acquire), baseline);
    }

    #[repr(C)]
    struct FakeController {
        vtable: *const *const c_void,
        refs: AtomicU32,
        closes: AtomicUsize,
    }

    unsafe extern "system" fn fake_add_ref(this: *mut c_void) -> u32 {
        // SAFETY: test vtable is called with a live FakeController pointer.
        unsafe {
            (*this.cast::<FakeController>())
                .refs
                .fetch_add(1, Ordering::Relaxed)
                + 1
        }
    }

    unsafe extern "system" fn fake_release(this: *mut c_void) -> u32 {
        // SAFETY: test vtable is called with a live FakeController pointer.
        unsafe {
            (*this.cast::<FakeController>())
                .refs
                .fetch_sub(1, Ordering::Relaxed)
                - 1
        }
    }

    unsafe extern "system" fn fake_close(this: *mut c_void) -> i32 {
        // SAFETY: test vtable is called with a live FakeController pointer.
        unsafe {
            (*this.cast::<FakeController>())
                .closes
                .fetch_add(1, Ordering::Relaxed)
        };
        S_OK
    }

    #[test]
    fn late_controller_result_is_closed_without_host_context() {
        let _serial = lock(&CALLBACK_TEST_LOCK);
        let mut slots = vec![null(); 26];
        slots[1] = fake_add_ref as *const () as *const c_void;
        slots[2] = fake_release as *const () as *const c_void;
        slots[24] = fake_close as *const () as *const c_void;
        let mut fake = FakeController {
            vtable: slots.as_ptr(),
            refs: AtomicU32::new(1),
            closes: AtomicUsize::new(0),
        };
        let shared = Arc::new(Shared::default());
        shared.closing.store(true, Ordering::Release);
        let handler = new_handler(Arc::clone(&shared), CallbackKind::Controller);
        drop(shared);
        let result = (&mut fake as *mut FakeController).cast::<c_void>();
        // SAFETY: handler and fake COM object remain alive through Invoke.
        assert_eq!(unsafe { handler_invoke(handler, S_OK, result) }, S_OK);
        assert_eq!(fake.closes.load(Ordering::Acquire), 1);
        // Runtime releases its borrowed result after Invoke; handler owns no reference to it.
        // SAFETY: fake has one outstanding initial reference.
        assert_eq!(unsafe { fake_release(result) }, 0);
        // SAFETY: this releases the handler's only reference and its last Shared Arc.
        assert_eq!(unsafe { handler_release(handler) }, 0);
    }
}
