// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only measurements for US-111: COM reference ledger over 100 cycles and a handle-growth
//! probe whose variants strip our own code from the cycle (`ATELIER_PROBE_VARIANT`).
use super::tests::CALLBACK_TEST_LOCK;
use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicU8;
use std::time::{Duration, Instant};

/// 0 = full cycle, 1 = no controller configuration (no settings, events, resources, navigation),
/// 2 = environment only (no controller is created).
pub(super) static VARIANT: AtomicU8 = AtomicU8::new(0);

#[repr(C)]
struct NativeMsg {
    hwnd: *mut c_void,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    x: i32,
    y: i32,
    private: u32,
}

#[repr(C)]
struct HandleEntry {
    object: usize,
    process: usize,
    value: usize,
    access: u32,
    trace: u16,
    type_index: u16,
    attributes: u32,
    reserved: u32,
}

// The shell declares the same user32 functions with its own message type; the ABI is identical.
#[allow(clashing_extern_declarations)]
#[link(name = "user32")]
unsafe extern "system" {
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: *mut c_void,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *mut c_void,
    ) -> *mut c_void;
    fn DestroyWindow(hwnd: *mut c_void) -> i32;
    #[link_name = "PeekMessageW"]
    fn probe_PeekMessageW(
        m: *mut NativeMsg,
        hwnd: *mut c_void,
        lo: u32,
        hi: u32,
        remove: u32,
    ) -> i32;
    #[link_name = "TranslateMessage"]
    fn probe_TranslateMessage(m: *const NativeMsg) -> i32;
    #[link_name = "DispatchMessageW"]
    fn probe_DispatchMessageW(m: *const NativeMsg) -> isize;
    #[link_name = "MsgWaitForMultipleObjectsEx"]
    fn probe_MsgWaitForMultipleObjectsEx(
        n: u32,
        h: *const c_void,
        ms: u32,
        mask: u32,
        flags: u32,
    ) -> u32;
    fn GetGuiResources(process: *mut c_void, flags: u32) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn GetCurrentProcessId() -> u32;
    fn GetProcessHandleCount(process: *mut c_void, count: *mut u32) -> i32;
    fn GetExitCodeProcess(process: *mut c_void, code: *mut u32) -> i32;
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformation(class: u32, buffer: *mut c_void, len: u32, ret: *mut u32) -> i32;
    fn NtQueryObject(
        handle: usize,
        class: u32,
        buffer: *mut c_void,
        len: u32,
        ret: *mut u32,
    ) -> i32;
}

fn pump_for(duration: Duration, until: &mut dyn FnMut() -> bool) -> bool {
    let started = Instant::now();
    loop {
        let mut message = NativeMsg {
            hwnd: null_mut(),
            message: 0,
            w_param: 0,
            l_param: 0,
            time: 0,
            x: 0,
            y: 0,
            private: 0,
        };
        // SAFETY: message is writable; PM_REMOVE drains this STA thread's queue.
        while unsafe { probe_PeekMessageW(&mut message, null_mut(), 0, 0, 1) } != 0 {
            // SAFETY: message was filled by PeekMessageW.
            unsafe {
                probe_TranslateMessage(&message);
                probe_DispatchMessageW(&message);
            }
        }
        if until() {
            return true;
        }
        if started.elapsed() >= duration {
            return false;
        }
        // SAFETY: zero handles; waits for input or posted messages on this thread for at most 10 ms.
        unsafe { probe_MsgWaitForMultipleObjectsEx(0, null(), 10, 0x04ff, 4) };
    }
}

fn hidden_window() -> *mut c_void {
    let class: Vec<u16> = "STATIC".encode_utf16().chain(Some(0)).collect();
    // SAFETY: STATIC is a system class; WS_POPUP without WS_VISIBLE creates a hidden top-level window.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0x8000_0000,
            0,
            0,
            400,
            300,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
        )
    };
    assert!(!hwnd.is_null(), "CreateWindowExW failed");
    hwnd
}

/// One create/use/close cycle on the calling STA thread. `dwell` waits for embedded page requests.
fn run_cycle(dir: &Path, dwell: bool) {
    let hwnd = hidden_window();
    let mut view = WebView::begin(hwnd, dir).expect("begin WebView2");
    if VARIANT.load(Ordering::Acquire) == 2 {
        let shared = Arc::clone(&view.shared);
        assert!(
            pump_for(Duration::from_secs(30), &mut || lock(&shared.environment)
                .is_some()),
            "environment timed out"
        );
        pump_for(Duration::from_secs(5), &mut || {
            shared.pending.load(Ordering::Acquire) == 0
        });
    } else {
        let ready = pump_for(Duration::from_secs(30), &mut || {
            view.take_completed().expect("completion error")
        });
        assert!(ready, "controller timed out");
        if dwell && VARIANT.load(Ordering::Acquire) == 0 {
            pump_for(Duration::from_secs(3), &mut || {
                !view.resource_log_snapshot().is_empty()
            });
        }
    }
    view.close_for_check().expect("close");
    pump_for(Duration::from_millis(100), &mut || false);
    view.detach_window();
    drop(view);
    // SAFETY: this test owns hwnd.
    unsafe { DestroyWindow(hwnd) };
    pump_for(Duration::from_millis(20), &mut || false);
}

fn handle_count() -> u32 {
    let mut count = 0;
    // SAFETY: pseudo-handle and writable output.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) };
    count
}

/// Handle count per kernel object type name for this process (NtQuerySystemInformation class 64).
fn handle_types() -> BTreeMap<String, usize> {
    let mut size = 1usize << 20;
    let mut buffer: Vec<u64>;
    loop {
        buffer = vec![0u64; size / 8];
        let mut needed = 0u32;
        // SAFETY: buffer is writable for `size` bytes.
        let status = unsafe {
            NtQuerySystemInformation(64, buffer.as_mut_ptr().cast(), size as u32, &mut needed)
        };
        if status == 0 {
            break;
        }
        assert!(
            status as u32 == 0xC000_0004,
            "NtQuerySystemInformation 0x{status:x}"
        );
        size = (needed as usize).max(size) * 2;
    }
    let count = buffer[0] as usize;
    // SAFETY: success wrote {count, reserved, entries...} into the buffer.
    let entries = unsafe { buffer.as_ptr().add(2).cast::<HandleEntry>() };
    // SAFETY: trivial query of the current process id.
    let pid = unsafe { GetCurrentProcessId() } as usize;
    let mut names: BTreeMap<u16, String> = BTreeMap::new();
    let mut result = BTreeMap::new();
    for index in 0..count {
        // SAFETY: index < NumberOfHandles reported by the kernel.
        let entry = unsafe { &*entries.add(index) };
        if entry.process != pid {
            continue;
        }
        let name = names.entry(entry.type_index).or_insert_with(|| {
            let mut info = vec![0u64; 512];
            let mut ret = 0u32;
            // SAFETY: ObjectTypeInformation on an own-process handle; the buffer is 4096 bytes.
            let status =
                unsafe { NtQueryObject(entry.value, 2, info.as_mut_ptr().cast(), 4096, &mut ret) };
            if status != 0 {
                return format!("type{}", entry.type_index);
            }
            let length = (info[0] & 0xffff) as usize / 2;
            let pointer = info[1] as *const u16;
            // SAFETY: the kernel points Buffer inside our info buffer.
            String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) })
        });
        *result.entry(name.clone()).or_insert(0) += 1;
    }
    result
}

/// Describes every Process-type handle held by this process: granted access mask and whether the
/// target has exited (`None` when the handle lacks PROCESS_QUERY_LIMITED_INFORMATION).
fn process_handles() -> BTreeMap<String, usize> {
    let mut size = 1usize << 20;
    let mut buffer: Vec<u64>;
    loop {
        buffer = vec![0u64; size / 8];
        let mut needed = 0u32;
        // SAFETY: buffer is writable for `size` bytes.
        let status = unsafe {
            NtQuerySystemInformation(64, buffer.as_mut_ptr().cast(), size as u32, &mut needed)
        };
        if status == 0 {
            break;
        }
        size = (needed as usize).max(size) * 2;
    }
    let count = buffer[0] as usize;
    // SAFETY: success wrote {count, reserved, entries...} into the buffer.
    let entries = unsafe { buffer.as_ptr().add(2).cast::<HandleEntry>() };
    // SAFETY: trivial query of the current process id.
    let pid = unsafe { GetCurrentProcessId() } as usize;
    let mut process_type = None;
    let mut result = BTreeMap::new();
    for index in 0..count {
        // SAFETY: index < NumberOfHandles reported by the kernel.
        let entry = unsafe { &*entries.add(index) };
        if entry.process != pid {
            continue;
        }
        if process_type.is_none() {
            let mut info = vec![0u64; 512];
            let mut ret = 0u32;
            // SAFETY: ObjectTypeInformation on an own-process handle; the buffer is 4096 bytes.
            let status =
                unsafe { NtQueryObject(entry.value, 2, info.as_mut_ptr().cast(), 4096, &mut ret) };
            if status == 0 {
                let length = (info[0] & 0xffff) as usize / 2;
                // SAFETY: the kernel points Buffer inside our info buffer.
                let name = String::from_utf16_lossy(unsafe {
                    std::slice::from_raw_parts(info[1] as *const u16, length)
                });
                if name == "Process" {
                    process_type = Some(entry.type_index);
                }
            }
        }
        if Some(entry.type_index) != process_type {
            continue;
        }
        let mut code = 0u32;
        // SAFETY: own handle value; the call fails harmlessly without the access right.
        let state = if unsafe { GetExitCodeProcess(entry.value as *mut c_void, &mut code) } == 0 {
            "query-denied"
        } else if code == 259 {
            "running"
        } else {
            "exited"
        };
        *result
            .entry(format!("access=0x{:x} {state}", entry.access))
            .or_insert(0) += 1;
    }
    result
}

fn runtime_installed() -> bool {
    matches!(probe_runtime(), Ok(Some(_)))
}

fn probe_dir(tag: &str) -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("us111-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("probe directory");
    dir
}

#[test]
fn com_reference_ledger_balances_over_100_cycles() {
    if !runtime_installed() {
        return;
    }
    let _guard = CALLBACK_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    VARIANT.store(0, Ordering::Release);
    let _apartment = ComApartment::initialize_sta().expect("STA");
    let dir = probe_dir("ledger");
    let baseline = LIVE_CALLBACK_OBJECTS.load(Ordering::Acquire);
    for cycle in 0..100 {
        run_cycle(&dir, cycle < 3);
    }
    pump_for(Duration::from_millis(500), &mut || false);
    let acquired = ledger::read(&ledger::ACQUIRED);
    let released = ledger::read(&ledger::RELEASED);
    let added = ledger::read(&ledger::OUR_ADD_REF);
    let dropped = ledger::read(&ledger::OUR_RELEASE);
    let live = LIVE_CALLBACK_OBJECTS.load(Ordering::Acquire);
    println!(
        "ledger: held acquired={acquired} released={released}; ours AddRef={added} Release={dropped}; live callbacks {baseline}->{live}"
    );
    assert!(acquired > 0 && added > 0, "ledger saw no traffic");
    assert_eq!(
        acquired, released,
        "interface references acquired vs released"
    );
    assert_eq!(added, dropped, "AddRef/Release on our callback objects");
    assert_eq!(live, baseline, "live callback objects after 100 cycles");
    let _ = std::fs::remove_dir_all(dir);
}

/// `ATELIER_PROBE_VARIANT=full|noconfig|envonly ATELIER_PROBE_CYCLES=n cargo test ... -- --ignored --nocapture`
#[test]
#[ignore = "slow measurement against the installed WebView2 runtime"]
fn handle_growth_probe() {
    if !runtime_installed() {
        return;
    }
    let variant = std::env::var("ATELIER_PROBE_VARIANT").unwrap_or_else(|_| "full".into());
    let cycles: usize = std::env::var("ATELIER_PROBE_CYCLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(50);
    VARIANT.store(
        match variant.as_str() {
            "noconfig" => 1,
            "envonly" => 2,
            _ => 0,
        },
        Ordering::Release,
    );
    let _guard = CALLBACK_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _apartment = ComApartment::initialize_sta().expect("STA");
    let dir = probe_dir("growth");
    run_cycle(&dir, false);
    // SAFETY: pseudo-handle.
    let process = unsafe { GetCurrentProcess() };
    let base = handle_count();
    let base_types = handle_types();
    println!("variant={variant} baseline handles={base}");
    for cycle in 1..=cycles {
        run_cycle(&dir, false);
        if cycle <= 3 || cycle % 10 == 0 || cycle == cycles {
            // SAFETY: flags 0 = GDI objects, 1 = USER objects.
            let (gdi, user) = unsafe { (GetGuiResources(process, 0), GetGuiResources(process, 1)) };
            let now = handle_count();
            println!(
                "cycle {cycle}: handles={now} ({:+}) gdi={gdi} user={user}",
                i64::from(now) - i64::from(base)
            );
        }
    }
    println!("process handles: {:?}", process_handles());
    let end_types = handle_types();
    for (name, count) in &end_types {
        let before = base_types.get(name).copied().unwrap_or(0);
        if *count != before {
            println!(
                "type {name}: {before} -> {count} ({:+})",
                *count as i64 - before as i64
            );
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}
