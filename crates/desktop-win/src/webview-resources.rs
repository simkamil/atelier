// SPDX-License-Identifier: AGPL-3.0-or-later
//! WebView2 resource/navigation handlers. ABI slot indices are counted in WebView2.h.

use super::resources::{self, Decision};
use super::*;

const IID_NAVIGATION_HANDLER: Guid = Guid {
    data1: 0x9adbe429,
    data2: 0xf36d,
    data3: 0x432b,
    data4: [0x9d, 0xdc, 0xf8, 0x88, 0x1f, 0xbd, 0x76, 0xe3],
};
const IID_NEW_WINDOW_HANDLER: Guid = Guid {
    data1: 0xd4c185fe,
    data2: 0xc81c,
    data3: 0x4989,
    data4: [0x97, 0xaf, 0x2d, 0x3f, 0xa7, 0xab, 0x56, 0x51],
};
const IID_RESOURCE_HANDLER: Guid = Guid {
    data1: 0xab00b74c,
    data2: 0x15f1,
    data3: 0x4646,
    data4: [0x80, 0xe8, 0xe7, 0x63, 0x41, 0xd2, 0x5d, 0x71],
};
const IID_WEB_MESSAGE_HANDLER: Guid = Guid {
    data1: 0x57213f19,
    data2: 0x00e6,
    data3: 0x49fa,
    data4: [0x8e, 0x07, 0x89, 0x8e, 0xa0, 0x1e, 0xcb, 0xd2],
};
const IID_CORE_WEBVIEW_22: Guid = Guid {
    data1: 0xdb75dfc7,
    data2: 0xa857,
    data3: 0x4632,
    data4: [0xa3, 0x98, 0x69, 0x69, 0xdd, 0xe2, 0x6c, 0x0a],
};

#[derive(Clone, Copy)]
enum EventKind {
    Navigation,
    NewWindow,
    Resource,
    WebMessage,
}

#[repr(C)]
struct EventVtable {
    query_interface:
        unsafe extern "system" fn(*mut EventHandler, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut EventHandler) -> u32,
    release: unsafe extern "system" fn(*mut EventHandler) -> u32,
    invoke: unsafe extern "system" fn(*mut EventHandler, *mut c_void, *mut c_void) -> i32,
}

#[repr(C)]
struct EventHandler {
    vtable: *const EventVtable,
    refs: AtomicU32,
    kind: EventKind,
    shared: *const Shared,
    iid: Guid,
}

static EVENT_VTABLE: EventVtable = EventVtable {
    query_interface: event_query,
    add_ref: event_add_ref,
    release: event_release,
    invoke: event_invoke,
};

unsafe extern "system" fn event_query(
    this: *mut EventHandler,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> i32 {
    if this.is_null() || iid.is_null() || out.is_null() {
        return E_POINTER;
    }
    // SAFETY: COM provides writable output and valid IID; this is live during QueryInterface.
    unsafe {
        *out = null_mut();
    }
    if unsafe { *iid } != IID_IUNKNOWN && unsafe { *iid } != unsafe { (*this).iid } {
        return E_NOINTERFACE;
    }
    unsafe {
        *out = this.cast();
        event_add_ref(this);
    }
    S_OK
}
unsafe extern "system" fn event_add_ref(this: *mut EventHandler) -> u32 {
    if this.is_null() {
        return 0;
    }
    #[cfg(test)]
    ledger::bump_ours(&ledger::OUR_ADD_REF);
    unsafe {
        (*this)
            .refs
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
    }
}
unsafe extern "system" fn event_release(this: *mut EventHandler) -> u32 {
    if this.is_null() {
        return 0;
    }
    #[cfg(test)]
    ledger::bump_ours(&ledger::OUR_RELEASE);
    let count = unsafe {
        (*this)
            .refs
            .fetch_sub(1, Ordering::AcqRel)
            .saturating_sub(1)
    };
    if count == 0 {
        unsafe {
            let handler = Box::from_raw(this);
            let shared = Arc::from_raw(handler.shared);
            shared.callback_objects.fetch_sub(1, Ordering::AcqRel);
            LIVE_CALLBACK_OBJECTS.fetch_sub(1, Ordering::AcqRel);
            drop(shared);
        }
    }
    count
}

fn new_event(shared: &Shared, kind: EventKind) -> *mut EventHandler {
    #[cfg(test)]
    ledger::bump_ours(&ledger::OUR_ADD_REF); // the creation reference
    shared.callback_objects.fetch_add(1, Ordering::AcqRel);
    LIVE_CALLBACK_OBJECTS.fetch_add(1, Ordering::AcqRel);
    let iid = match kind {
        EventKind::Navigation => IID_NAVIGATION_HANDLER,
        EventKind::NewWindow => IID_NEW_WINDOW_HANDLER,
        EventKind::Resource => IID_RESOURCE_HANDLER,
        EventKind::WebMessage => IID_WEB_MESSAGE_HANDLER,
    };
    // SAFETY: callers invoke this while an owning Arc held by a WebView2 handler is live.
    unsafe {
        Arc::increment_strong_count(shared as *const Shared);
    }
    let raw = shared as *const Shared;
    Box::into_raw(Box::new(EventHandler {
        vtable: &EVENT_VTABLE,
        refs: AtomicU32::new(1),
        kind,
        shared: raw,
        iid,
    }))
}

unsafe extern "system" fn event_invoke(
    this: *mut EventHandler,
    _sender: *mut c_void,
    args: *mut c_void,
) -> i32 {
    if this.is_null() || args.is_null() {
        return E_POINTER;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (kind, shared_ptr) = unsafe { ((*this).kind, (*this).shared) };
        if shared_ptr.is_null() {
            return E_POINTER;
        }
        let shared = unsafe { &*shared_ptr };
        if shared.closing.load(Ordering::Acquire) {
            return S_OK;
        }
        match kind {
            EventKind::Navigation => navigation_starting(shared, args),
            EventKind::NewWindow => new_window_requested(shared, args),
            EventKind::Resource => resource_requested(shared, args),
            EventKind::WebMessage => web_message_received(shared, args),
        }
    }));
    result.unwrap_or(E_NOINTERFACE)
}

fn web_message_received(shared: &Shared, args: *mut c_void) -> i32 {
    const MAX_MESSAGE_CHARS: usize = 768;
    const MAX_FILE_MESSAGE_CHARS: usize = 175_000;
    const MAX_BLOCKED_URI_CHARS: usize = 512;
    let mut source = null_mut();
    // get_Source is slot 3; WebView2 returns a CoTaskMem-owned LPWSTR.
    let hr = unsafe {
        call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32>(args, 3)(
            args,
            &mut source,
        )
    };
    if hr < 0 {
        return hr;
    }
    let source = wide_string(source).unwrap_or_default();
    if source != resources::ORIGIN {
        return S_OK;
    }

    let mut message = null_mut();
    // TryGetWebMessageAsString is slot 5 and allocates an owned LPWSTR on success.
    let hr = unsafe {
        call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32>(args, 5)(
            args,
            &mut message,
        )
    };
    if hr < 0 {
        return S_OK;
    }
    let Some(message) = wide_string_bounded(message, MAX_FILE_MESSAGE_CHARS) else {
        return S_OK;
    };
    if message.starts_with("us120-file|") {
        let mut queue = lock(&shared.file_messages);
        if queue.accept(&source, &message).is_ok() || queue.has_rejections() {
            shared.notify();
        }
        return S_OK;
    }
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return S_OK;
    }
    let mut parts = message.splitn(3, '|');
    if parts.next() != Some("us112-csp") {
        return S_OK;
    }
    let Some(directive) = parts.next() else {
        return S_OK;
    };
    let Some(blocked_uri) = parts.next() else {
        return S_OK;
    };
    if blocked_uri.is_empty()
        || blocked_uri.chars().count() > MAX_BLOCKED_URI_CHARS
        || blocked_uri.contains('|')
        || !matches!(
            directive,
            "default-src"
                | "base-uri"
                | "object-src"
                | "frame-src"
                | "form-action"
                | "script-src"
                | "script-src-elem"
                | "script-src-attr"
                | "worker-src"
                | "style-src"
                | "style-src-elem"
                | "style-src-attr"
                | "font-src"
                | "img-src"
                | "connect-src"
                | "navigate-to"
        )
    {
        return S_OK;
    }
    lock(&shared.attempts).record_csp_violation(directive, blocked_uri);
    S_OK
}

fn wide_string(ptr: *mut u16) -> Option<String> {
    wide_string_bounded(ptr, 32_768)
}

fn wide_string_bounded(ptr: *mut u16, limit: usize) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: WebView2 string getters return CoTaskMem-owned, NUL-terminated UTF-16.
    let value = unsafe {
        let mut n = 0usize;
        while n < limit && *ptr.add(n) != 0 {
            n += 1;
        }
        if n == limit {
            None
        } else {
            String::from_utf16(std::slice::from_raw_parts(ptr, n)).ok()
        }
    };
    // SAFETY: the returned string allocation is released with CoTaskMemFree.
    unsafe {
        CoTaskMemFree(ptr.cast());
    }
    value
}

fn log(shared: &Shared, url: &str, decision: Decision) {
    lock(&shared.attempts).record(url, decision);
}

fn navigation_starting(shared: &Shared, args: *mut c_void) -> i32 {
    let mut uri = null_mut();
    let hr = unsafe {
        call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32>(args, 3)(
            args, &mut uri,
        )
    };
    if hr < 0 {
        let refusal = refuse_navigation(shared, args, None);
        if !uri.is_null() {
            unsafe { CoTaskMemFree(uri.cast()) };
        }
        return if refusal < 0 { refusal } else { hr };
    }
    let Some(url) = wide_string(uri) else {
        return refuse_navigation(shared, args, None);
    };
    let decision = resources::classify(&url, "GET");
    let allowed = matches!(decision, Decision::Serve(_) | Decision::NotFound);
    if !allowed {
        return refuse_navigation(shared, args, Some(&url));
    }
    // Only the initial internal document may start. WebView2 may run a nested
    // STA message loop while dialogs are open; refusing every later document
    // navigation prevents dirty state loss and stale IDs reaching a new page.
    let file_work_active = shared
        .file_working
        .load(std::sync::atomic::Ordering::Acquire);
    if !lock(&shared.file_messages).begin_page_navigation(file_work_active) {
        return refuse_navigation(shared, args, Some(&url));
    }
    S_OK
}

fn refuse_navigation(shared: &Shared, args: *mut c_void, url: Option<&str>) -> i32 {
    log(shared, url.unwrap_or("<unclassified>"), Decision::Refused);
    // ICoreWebView2NavigationStartingEventArgs::put_Cancel is SDK slot 8.
    unsafe { call_vtable::<unsafe extern "system" fn(*mut c_void, i32) -> i32>(args, 8)(args, 1) }
}

fn new_window_requested(shared: &Shared, args: *mut c_void) -> i32 {
    let mut uri = null_mut();
    let hr = unsafe {
        call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32>(args, 3)(
            args, &mut uri,
        )
    };
    if hr < 0 {
        let refusal = refuse_new_window(shared, args, None);
        if !uri.is_null() {
            unsafe { CoTaskMemFree(uri.cast()) };
        }
        return if refusal < 0 { refusal } else { hr };
    }
    let Some(url) = wide_string(uri) else {
        return refuse_new_window(shared, args, None);
    };
    refuse_new_window(shared, args, Some(&url))
}

fn refuse_new_window(shared: &Shared, args: *mut c_void, url: Option<&str>) -> i32 {
    log(shared, url.unwrap_or("<unclassified>"), Decision::Refused);
    // ICoreWebView2NewWindowRequestedEventArgs::put_Handled is SDK slot 6.
    unsafe { call_vtable::<unsafe extern "system" fn(*mut c_void, i32) -> i32>(args, 6)(args, 1) }
}

fn refuse_resource_request(shared: &Shared, args: *mut c_void) -> i32 {
    log(shared, "<unclassified>", Decision::Refused);
    let env = shared.resource_environment.load(Ordering::Acquire) as *mut c_void;
    if env.is_null() {
        return E_POINTER;
    }
    make_response(
        env,
        args,
        403,
        "Forbidden",
        "text/plain; charset=utf-8",
        b"Forbidden",
        b"Forbidden".len(),
    )
}

fn resource_requested(shared: &Shared, args: *mut c_void) -> i32 {
    let mut request = null_mut();
    let hr = unsafe {
        call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32>(args, 3)(
            args,
            &mut request,
        )
    };
    if hr < 0 {
        let refusal = refuse_resource_request(shared, args);
        if !request.is_null() {
            note_acquired();
            unsafe { release_interface(request) };
        }
        return refusal;
    }
    if request.is_null() {
        return refuse_resource_request(shared, args);
    }
    note_acquired();
    let mut uri = null_mut();
    let mut method = null_mut();
    let result = (|| {
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32>(request, 3)(
                request, &mut uri,
            )
        };
        if hr < 0 {
            return refuse_resource_request(shared, args);
        }
        let Some(url) = wide_string(uri) else {
            uri = null_mut();
            return refuse_resource_request(shared, args);
        };
        uri = null_mut();
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32>(request, 5)(
                request,
                &mut method,
            )
        };
        if hr < 0 {
            return refuse_resource_request(shared, args);
        }
        let Some(method_name) = wide_string(method) else {
            method = null_mut();
            return refuse_resource_request(shared, args);
        };
        method = null_mut();
        let decision = resources::classify(&url, &method_name);
        log(shared, &url, decision);
        let (status, reason, mime, bytes, content_length) = match decision {
            Decision::Serve(index) => {
                let resource = &resources::RESOURCES[index];
                (
                    200,
                    "OK",
                    resource.mime,
                    if method_name == "HEAD" {
                        &[][..]
                    } else {
                        resource.bytes
                    },
                    resource.bytes.len(),
                )
            }
            Decision::NotFound => (
                404,
                "Not Found",
                "text/plain; charset=utf-8",
                &b"Not found"[..],
                b"Not found".len(),
            ),
            Decision::MethodNotAllowed => (
                405,
                "Method Not Allowed",
                "text/plain; charset=utf-8",
                &b"Method not allowed"[..],
                b"Method not allowed".len(),
            ),
            Decision::Refused => (
                403,
                "Forbidden",
                "text/plain; charset=utf-8",
                &b"Forbidden"[..],
                b"Forbidden".len(),
            ),
        };
        let env = shared.resource_environment.load(Ordering::Acquire) as *mut c_void;
        if env.is_null() {
            return E_POINTER;
        }
        make_response(env, args, status, reason, mime, bytes, content_length)
    })();
    if !uri.is_null() {
        unsafe {
            CoTaskMemFree(uri.cast());
        }
    }
    if !method.is_null() {
        unsafe {
            CoTaskMemFree(method.cast());
        }
    }
    unsafe {
        release_interface(request);
    }
    result
}

fn make_response(
    env: *mut c_void,
    args: *mut c_void,
    status: i32,
    reason: &str,
    mime: &str,
    bytes: &[u8],
    content_length: usize,
) -> i32 {
    let stream =
        unsafe { SHCreateMemStream(bytes.as_ptr(), bytes.len().min(u32::MAX as usize) as u32) };
    if stream.is_null() {
        return 0x8007_000e_u32 as i32;
    }
    note_acquired();
    let reason_w: Vec<u16> = reason.encode_utf16().chain(Some(0)).collect();
    let headers = format!(
        "Content-Type: {mime}\r\nContent-Length: {}\r\nContent-Security-Policy: {}\r\nX-Content-Type-Options: nosniff\r\n",
        content_length,
        resources::CSP
    );
    let headers_w: Vec<u16> = headers.encode_utf16().chain(Some(0)).collect();
    let mut response = null_mut();
    // ICoreWebView2Environment::CreateWebResourceResponse is SDK slot 4.
    let hr = unsafe {
        call_vtable::<
            unsafe extern "system" fn(
                *mut c_void,
                *mut c_void,
                i32,
                *const u16,
                *const u16,
                *mut *mut c_void,
            ) -> i32,
        >(env, 4)(
            env,
            stream,
            status,
            reason_w.as_ptr(),
            headers_w.as_ptr(),
            &mut response,
        )
    };
    unsafe {
        release_interface(stream);
    }
    if hr < 0 {
        return hr;
    }
    if response.is_null() {
        return E_POINTER;
    }
    note_acquired();
    // ICoreWebView2WebResourceRequestedEventArgs::put_Response is SDK slot 5.
    let result = unsafe {
        call_vtable::<unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32>(args, 5)(
            args, response,
        )
    };
    unsafe {
        release_interface(response);
    }
    result
}

#[link(name = "shlwapi")]
unsafe extern "system" {
    fn SHCreateMemStream(init: *const u8, init_size: u32) -> *mut c_void;
}

#[derive(Default)]
struct Token {
    value: i64,
    active: bool,
}
#[derive(Default)]
pub(super) struct Registrations {
    core: usize,
    core22: usize,
    navigation: Token,
    new_window: Token,
    resource: Token,
    web_message: Token,
    filter: bool,
}

pub(super) fn install(core: *mut c_void, shared: &Shared) -> io::Result<()> {
    // Keep one core reference for event removal, always before Close.
    unsafe { add_ref_interface(core) };
    let mut regs = Registrations {
        core: core as usize,
        ..Registrations::default()
    };
    let mut core22 = null_mut();
    // ICoreWebView2_22 is required for source-kind-aware filtering of worker requests.
    let hr = unsafe {
        call_vtable::<unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32>(
            core, 0,
        )(core, &IID_CORE_WEBVIEW_22, &mut core22)
    };
    if let Err(error) = check_hresult("QueryInterface(ICoreWebView2_22)", hr) {
        unsafe {
            release_interface(core);
        }
        return Err(error);
    }
    if core22.is_null() {
        unsafe {
            release_interface(core);
        }
        return Err(io::Error::other(
            "ICoreWebView2_22 returned a null interface",
        ));
    }
    note_acquired();
    regs.core22 = core22 as usize;
    let wildcard: Vec<u16> = "*".encode_utf16().chain(Some(0)).collect();
    // Request-source All is 0xffffffff; resource context 0 is All (SDK slot 123).
    let hr = unsafe {
        call_vtable::<unsafe extern "system" fn(*mut c_void, *const u16, i32, u32) -> i32>(
            core22, 123,
        )(core22, wildcard.as_ptr(), 0, u32::MAX)
    };
    if let Err(e) = check_hresult("AddWebResourceRequestedFilter(*)", hr) {
        unsafe {
            release_interface(core22);
            release_interface(core);
        }
        return Err(e);
    }
    regs.filter = true;
    let specs = [
        (EventKind::Navigation, 7usize),
        (EventKind::NewWindow, 44),
        (EventKind::Resource, 55),
        (EventKind::WebMessage, 34),
    ];
    for (kind, slot) in specs {
        let handler = new_event(shared, kind);
        let mut token = 0i64;
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *mut EventHandler, *mut i64) -> i32>(
                core, slot,
            )(core, handler, &mut token)
        };
        unsafe {
            event_release(handler);
        }
        if let Err(e) = check_hresult("ICoreWebView2 event registration", hr) {
            return match unregister_core(core, &regs) {
                Ok(()) => Err(e),
                Err(cleanup) => Err(io::Error::other(format!(
                    "{e}; event cleanup failed: {cleanup}"
                ))),
            };
        }
        let token = Token {
            value: token,
            active: true,
        };
        match kind {
            EventKind::Navigation => regs.navigation = token,
            EventKind::NewWindow => regs.new_window = token,
            EventKind::Resource => regs.resource = token,
            EventKind::WebMessage => regs.web_message = token,
        }
    }
    *lock(&shared.registrations) = regs;
    Ok(())
}

pub(super) fn unregister(shared: &Shared) -> io::Result<()> {
    let regs = std::mem::take(&mut *lock(&shared.registrations));
    if regs.core == 0 {
        return Ok(());
    }
    unregister_core(regs.core as *mut c_void, &regs)
}

fn unregister_core(core: *mut c_void, regs: &Registrations) -> io::Result<()> {
    let mut first_error = None;
    for (slot, token) in [
        (8usize, &regs.navigation),
        (45, &regs.new_window),
        (56, &regs.resource),
        (35, &regs.web_message),
    ] {
        if token.active {
            let hr = unsafe {
                call_vtable::<unsafe extern "system" fn(*mut c_void, i64) -> i32>(core, slot)(
                    core,
                    token.value,
                )
            };
            if let Err(error) = check_hresult("ICoreWebView2 event removal", hr) {
                first_error.get_or_insert(error);
            }
        }
    }
    if regs.filter {
        let wildcard: Vec<u16> = "*".encode_utf16().chain(Some(0)).collect();
        let core22 = regs.core22 as *mut c_void;
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *const u16, i32, u32) -> i32>(
                core22, 124,
            )(core22, wildcard.as_ptr(), 0, u32::MAX)
        };
        if let Err(error) = check_hresult("RemoveWebResourceRequestedFilter(*)", hr) {
            first_error.get_or_insert(error);
        }
    }
    if regs.core22 != 0 {
        unsafe {
            release_interface(regs.core22 as *mut c_void);
        }
    }
    unsafe {
        release_interface(core);
    }
    if let Some(error) = first_error {
        Err(error)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const E_FAIL: i32 = 0x8000_4005_u32 as i32;

    #[repr(C)]
    struct FakeArgs {
        vtable: *const *const c_void,
        flag: i32,
    }

    unsafe extern "system" fn failing_get_uri(_this: *mut c_void, value: *mut *mut u16) -> i32 {
        unsafe { *value = null_mut() };
        E_FAIL
    }

    unsafe extern "system" fn set_flag(this: *mut c_void, value: i32) -> i32 {
        unsafe { (*this.cast::<FakeArgs>()).flag = value };
        S_OK
    }

    #[repr(C)]
    struct FakeResourceArgs {
        vtable: *const *const c_void,
        response: *mut c_void,
    }

    #[repr(C)]
    struct FakeEnvironment {
        vtable: *const *const c_void,
        response: *mut c_void,
        status: i32,
    }

    #[repr(C)]
    struct FakeResponse {
        vtable: *const *const c_void,
    }

    unsafe extern "system" fn failing_get_request(
        _this: *mut c_void,
        request: *mut *mut c_void,
    ) -> i32 {
        unsafe { *request = null_mut() };
        E_FAIL
    }

    unsafe extern "system" fn fake_create_response(
        this: *mut c_void,
        _stream: *mut c_void,
        status: i32,
        _reason: *const u16,
        _headers: *const u16,
        response: *mut *mut c_void,
    ) -> i32 {
        let environment = unsafe { &mut *this.cast::<FakeEnvironment>() };
        environment.status = status;
        unsafe { *response = environment.response };
        S_OK
    }

    unsafe extern "system" fn fake_put_response(this: *mut c_void, response: *mut c_void) -> i32 {
        unsafe { (*this.cast::<FakeResourceArgs>()).response = response };
        S_OK
    }

    unsafe extern "system" fn fake_release(_this: *mut c_void) -> u32 {
        1
    }

    fn args_with_failed_getter(setter_slot: usize) -> (FakeArgs, Vec<*const c_void>) {
        let mut slots = vec![null(); setter_slot + 1];
        slots[3] = failing_get_uri as *const () as *const c_void;
        slots[setter_slot] = set_flag as *const () as *const c_void;
        let args = FakeArgs {
            vtable: slots.as_ptr(),
            flag: 0,
        };
        (args, slots)
    }

    #[test]
    fn failed_navigation_and_new_window_uri_getters_still_refuse_by_default() {
        let _serial = crate::webview::lock(&crate::webview::tests::CALLBACK_TEST_LOCK);
        let shared = Shared::default();
        let (mut navigation, _navigation_vtable) = args_with_failed_getter(8);
        let hr = navigation_starting(&shared, (&mut navigation as *mut FakeArgs).cast());
        assert_eq!(hr, E_FAIL);
        assert_eq!(navigation.flag, 1);

        let (mut new_window, _new_window_vtable) = args_with_failed_getter(6);
        let hr = new_window_requested(&shared, (&mut new_window as *mut FakeArgs).cast());
        assert_eq!(hr, E_FAIL);
        assert_eq!(new_window.flag, 1);

        let attempts = lock(&shared.attempts);
        let entries: Vec<_> = attempts.entries().collect();
        assert_eq!(
            entries,
            ["<unclassified> [refused]", "<unclassified> [refused]"]
        );
        assert!(entries.iter().all(|entry| !entry.contains("private")));
    }

    #[test]
    fn failed_resource_request_getter_installs_a_forbidden_response() {
        let _serial = crate::webview::lock(&crate::webview::tests::CALLBACK_TEST_LOCK);
        let mut response_slots = [null(); 3];
        response_slots[2] = fake_release as *const () as *const c_void;
        let mut response = FakeResponse {
            vtable: response_slots.as_ptr(),
        };

        let mut environment_slots = [null(); 5];
        environment_slots[4] = fake_create_response as *const () as *const c_void;
        let mut environment = FakeEnvironment {
            vtable: environment_slots.as_ptr(),
            response: (&mut response as *mut FakeResponse).cast(),
            status: 0,
        };

        let mut args_slots = [null(); 6];
        args_slots[3] = failing_get_request as *const () as *const c_void;
        args_slots[5] = fake_put_response as *const () as *const c_void;
        let mut args = FakeResourceArgs {
            vtable: args_slots.as_ptr(),
            response: null_mut(),
        };

        let shared = Shared::default();
        shared.resource_environment.store(
            (&mut environment as *mut FakeEnvironment) as usize,
            Ordering::Release,
        );
        let hr = resource_requested(&shared, (&mut args as *mut FakeResourceArgs).cast());

        assert_eq!(hr, S_OK);
        assert_eq!(environment.status, 403);
        assert_eq!(args.response, (&mut response as *mut FakeResponse).cast());
        assert_eq!(
            lock(&shared.attempts).entries().collect::<Vec<_>>(),
            ["<unclassified> [refused]"]
        );
    }
}
