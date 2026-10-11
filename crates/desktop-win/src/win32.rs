// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hand-written Win32 bindings and the single-owner HWND lifecycle.

use super::{
    MonitorArea, Rect, WindowState, encode_state, load_state, minimum_size, restore_state,
};
use std::ffi::c_void;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};

type Hwnd = *mut c_void;
type Hinstance = *mut c_void;
type Hmonitor = *mut c_void;
type Hkey = *mut c_void;
type Lparam = isize;
type Wparam = usize;
type Lresult = isize;

const WM_SIZE: u32 = 0x0005;
const WM_CLOSE: u32 = 0x0010;
const WM_DESTROY: u32 = 0x0002;
const WM_NCCREATE: u32 = 0x0081;
const WM_NCDESTROY: u32 = 0x0082;
const WM_DPICHANGED: u32 = 0x02e0;
const WM_SETTINGCHANGE: u32 = 0x001a;
const WM_THEMECHANGED: u32 = 0x031a;
const WM_GETMINMAXINFO: u32 = 0x0024;
const GWLP_USERDATA: i32 = -21;
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
    fn PostQuitMessage(code: i32);
    fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
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

struct Context {
    state_file: PathBuf,
    title: Vec<u16>,
    client: (i32, i32),
    dpi: u32,
    persisted: bool,
    callback_error: Option<io::Error>,
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
                        }
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
                        };
                        (*context).record(save(hwnd, &snapshot));
                        (*context).persisted = true;
                    }
                    // SAFETY: hwnd is the live window receiving WM_CLOSE; Context remains boxed while destroy callbacks run.
                    if DestroyWindow(hwnd) == 0 {
                        (*context).record(Err(error_from_last("DestroyWindow(WM_CLOSE)")));
                        PostQuitMessage(1);
                    }
                    return 0;
                }
                WM_DESTROY => {
                    if !(*context).persisted {
                        let snapshot = Context {
                            state_file: (*context).state_file.clone(),
                            title: Vec::new(),
                            client: (0, 0),
                            dpi: (*context).dpi,
                            persisted: false,
                            callback_error: None,
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
        // SAFETY: GetCurrentProcessId is a pointer-free query for a process-unique class name.
        let name = wide(std::ffi::OsStr::new(&format!("AtelierWindow-{}", unsafe {
            GetCurrentProcessId()
        })));
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
    _class: RegisteredClass,
}
impl Drop for OwnedWindow {
    fn drop(&mut self) {
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
    }))
}

pub fn run(state_file: &Path, title: &str) -> io::Result<()> {
    let saved = load_state(state_file)?;
    let mut context = create_context(state_file, title, saved)?;
    let initial = restore_state(saved, &monitors()?, context.dpi);
    let window = create_window(&mut context, initial)?;
    let hwnd = window.hwnd;
    // SAFETY: hwnd is the live, owned window created above; this pointer-free call obtains its monitor DPI.
    context.dpi = unsafe { GetDpiForWindow(hwnd) }.clamp(48, 960);
    let screens = monitors_with_window_dpi(initial.normal, context.dpi)?;
    let state = restore_state(saved, &screens, context.dpi);
    apply_state(hwnd, state, true)?;
    apply_theme(hwnd)?;
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
        return Err(error);
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
    for component in relative_parent.components() {
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
                std::fs::create_dir(&child)?;
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
    Ok(current.join(file_name))
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
        SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, WPF_RESTORETOMAXIMIZED, atomic_write_to_temp,
        placement_is_maximized, validate_self_check_path,
    };
    use std::io;
    use std::path::{Path, PathBuf};

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
