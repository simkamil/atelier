// SPDX-License-Identifier: AGPL-3.0-or-later
//! Windows Common Item Dialogs, owned by the calling STA.

use crate::files::{FileError, FileErrorCode};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::ptr::{NonNull, null_mut};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFilter {
    label: String,
    extensions: Vec<String>,
}

impl FileFilter {
    pub fn new(
        label: impl Into<String>,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, FileError> {
        let label = label.into();
        let extensions: Vec<String> = extensions.into_iter().map(Into::into).collect();
        if label.trim().is_empty()
            || label.contains('\0')
            || extensions.is_empty()
            || extensions.iter().any(|extension| {
                extension.is_empty()
                    || extension.contains('\0')
                    || extension.starts_with('.')
                    || extension.contains('*')
                    || extension.contains('?')
                    || extension.contains(';')
                    || extension.contains('/')
                    || extension.contains('\\')
                    || extension.chars().any(|character| {
                        !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
                    })
            })
        {
            return Err(error(
                FileErrorCode::InvalidFilter,
                "files.error.invalid_filter",
            ));
        }
        Ok(Self { label, extensions })
    }

    fn pattern(&self) -> String {
        self.extensions
            .iter()
            .map(|extension| format!("*.{extension}"))
            .collect::<Vec<_>>()
            .join(";")
    }
}

/// A valid parent HWND. Creation is unsafe because the caller must keep the
/// window alive on this STA until the modal dialog returns.
pub struct DialogOwner {
    hwnd: NonNull<c_void>,
    _not_send: PhantomData<Rc<()>>,
}

impl DialogOwner {
    /// # Safety
    /// `hwnd` must be a live HWND created on the current thread and remain live
    /// until every dialog using this owner has returned.
    pub unsafe fn from_raw(hwnd: *mut c_void) -> Option<Self> {
        NonNull::new(hwnd).map(|hwnd| Self {
            hwnd,
            _not_send: PhantomData,
        })
    }
}

/// COM apartment initialization for native dialogs. This value is !Send/!Sync.
pub struct StaFileDialogs {
    #[cfg(windows)]
    initialized: bool,
    _not_send: PhantomData<Rc<()>>,
}

impl StaFileDialogs {
    /// Initialize COM as STA on the calling thread. Must be created on the UI
    /// thread and dropped there; no dialog is shown by construction.
    #[cfg(windows)]
    pub fn initialize_current_thread() -> Result<Self, FileError> {
        const COINIT_APARTMENTTHREADED: u32 = 0x2;
        // SAFETY: COM accepts a null reserved pointer and this type balances S_OK/S_FALSE.
        let hr = unsafe { CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED) };
        if hr < 0 {
            let mut value = error(
                FileErrorCode::ComInitializationFailed,
                "files.error.com_init_failed",
            );
            value.hresult = Some(hr);
            return Err(value);
        }
        Ok(Self {
            initialized: true,
            _not_send: PhantomData,
        })
    }

    /// Dialogs are Windows-only. This stub keeps the type name visible to
    /// non-Windows builds that compile shared API documentation or tests.
    #[cfg(not(windows))]
    pub fn initialize_current_thread() -> Result<Self, FileError> {
        Err(error(
            FileErrorCode::DialogFailed,
            "files.error.windows_only",
        ))
    }

    #[cfg(windows)]
    pub fn open(
        &mut self,
        owner: Option<&DialogOwner>,
        title: &str,
        filters: &[FileFilter],
        selected_filter: usize,
    ) -> Result<Option<PathBuf>, FileError> {
        self.show(false, owner, title, filters, selected_filter, None)
    }

    #[cfg(windows)]
    pub fn save(
        &mut self,
        owner: Option<&DialogOwner>,
        title: &str,
        filters: &[FileFilter],
        selected_filter: usize,
        suggested_name: Option<&str>,
    ) -> Result<Option<PathBuf>, FileError> {
        self.show(true, owner, title, filters, selected_filter, suggested_name)
    }

    #[cfg(not(windows))]
    pub fn open(
        &mut self,
        _: Option<&DialogOwner>,
        _: &str,
        _: &[FileFilter],
        _: usize,
    ) -> Result<Option<PathBuf>, FileError> {
        Err(error(
            FileErrorCode::DialogFailed,
            "files.error.windows_only",
        ))
    }

    #[cfg(not(windows))]
    pub fn save(
        &mut self,
        _: Option<&DialogOwner>,
        _: &str,
        _: &[FileFilter],
        _: usize,
        _: Option<&str>,
    ) -> Result<Option<PathBuf>, FileError> {
        Err(error(
            FileErrorCode::DialogFailed,
            "files.error.windows_only",
        ))
    }

    #[cfg(windows)]
    fn show(
        &mut self,
        save: bool,
        owner: Option<&DialogOwner>,
        title: &str,
        filters: &[FileFilter],
        selected_filter: usize,
        suggested_name: Option<&str>,
    ) -> Result<Option<PathBuf>, FileError> {
        if title.contains('\0')
            || suggested_name.is_some_and(|name| name.contains('\0'))
            || filters.is_empty()
            || selected_filter >= filters.len()
        {
            return Err(error(
                FileErrorCode::InvalidFilter,
                "files.error.invalid_dialog_options",
            ));
        }
        let count = u32::try_from(filters.len())
            .map_err(|_| error(FileErrorCode::InvalidFilter, "files.error.invalid_filter"))?;
        let owned_filters: Vec<(Vec<u16>, Vec<u16>)> = filters
            .iter()
            .map(|filter| (wide(&filter.label), wide(&filter.pattern())))
            .collect();
        let specs: Vec<FilterSpec> = owned_filters
            .iter()
            .map(|(label, pattern)| FilterSpec {
                name: label.as_ptr(),
                spec: pattern.as_ptr(),
            })
            .collect();
        let title_wide = wide(title);
        let suggested_wide = suggested_name.map(wide);
        let clsid = if save {
            CLSID_FILE_SAVE_DIALOG
        } else {
            CLSID_FILE_OPEN_DIALOG
        };
        let iid = if save {
            IID_FILE_SAVE_DIALOG
        } else {
            IID_FILE_OPEN_DIALOG
        };
        let dialog = create_dialog(&clsid, &iid)?;
        let base = dialog.as_ptr();
        // SAFETY: `base` is an owned COM interface returned by CoCreateInstance
        // for the requested IFileOpenDialog/IFileSaveDialog IID, and this method
        // runs on the STA that owns it. Each function-pointer typedef below
        // matches the Windows SDK ABI for its documented vtable slot (including
        // the IModalWindow::Show base slot). `FilterSpec` has the SDK field
        // order and pointer-sized ABI, while every label, pattern, title, name,
        // and filter array remains alive and immovable for each synchronous
        // call. Out-pointers are initialized to null and only adopted as owned
        // COM/task-allocator values after the corresponding call succeeds;
        // `ComPtr` and `CoTaskString` release those values on every return path.
        unsafe {
            check_hr(
                "IFileDialog::SetFileTypes",
                call_vtable::<SetFileTypes>(base, 4)(base, count, specs.as_ptr()),
            )?;
            check_hr(
                "IFileDialog::SetFileTypeIndex",
                call_vtable::<SetFileTypeIndex>(base, 5)(base, (selected_filter + 1) as u32),
            )?;
            let options_fn = call_vtable::<GetOptions>(base, 10);
            let mut options = 0u32;
            check_hr("IFileDialog::GetOptions", options_fn(base, &mut options))?;
            options |= FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
            options |= if save {
                FOS_OVERWRITEPROMPT
            } else {
                FOS_FILEMUSTEXIST
            };
            check_hr(
                "IFileDialog::SetOptions",
                call_vtable::<SetOptions>(base, 9)(base, options),
            )?;
            check_hr(
                "IFileDialog::SetTitle",
                call_vtable::<SetWideString>(base, 17)(base, title_wide.as_ptr()),
            )?;
            if save {
                let extension = &filters[selected_filter].extensions[0];
                let extension_wide = wide(extension);
                check_hr(
                    "IFileDialog::SetDefaultExtension",
                    call_vtable::<SetWideString>(base, 22)(base, extension_wide.as_ptr()),
                )?;
                if let Some(name) = suggested_wide.as_ref() {
                    check_hr(
                        "IFileDialog::SetFileName",
                        call_vtable::<SetWideString>(base, 15)(base, name.as_ptr()),
                    )?;
                }
            }
            let parent = owner.map_or(null_mut(), |owner| owner.hwnd.as_ptr());
            let hr = call_vtable::<Show>(base, 3)(base, parent);
            if !interpret_show_hresult(hr)? {
                return Ok(None);
            }
            let mut item = null_mut();
            check_hr(
                "IFileDialog::GetResult",
                call_vtable::<GetResult>(base, 20)(base, &mut item),
            )?;
            let item = ComPtr::from_raw(item)
                .ok_or_else(|| error(FileErrorCode::DialogFailed, "files.error.empty_result"))?;
            let mut path = null_mut();
            check_hr(
                "IShellItem::GetDisplayName",
                call_vtable::<GetDisplayName>(item.as_ptr(), 5)(
                    item.as_ptr(),
                    SIGDN_FILESYSPATH,
                    &mut path,
                ),
            )?;
            if path.is_null() {
                return Err(error(
                    FileErrorCode::DialogFailed,
                    "files.error.empty_result",
                ));
            }
            let path = CoTaskString(path);
            use std::os::windows::ffi::OsStringExt;
            let mut length = 0usize;
            while *path.0.add(length) != 0 {
                length += 1;
            }
            let path = PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(
                path.0, length,
            )));
            if !path.is_absolute() {
                return Err(error(
                    FileErrorCode::DialogFailed,
                    "files.error.invalid_path",
                ));
            }
            Ok(Some(path))
        }
    }
}

impl Drop for StaFileDialogs {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.initialized {
            // SAFETY: initialize_current_thread acquired one COM apartment count.
            unsafe { CoUninitialize() };
        }
    }
}

fn error(code: FileErrorCode, message_key: &'static str) -> FileError {
    FileError {
        code,
        message_key,
        windows_code: None,
        hresult: None,
        stage: None,
        cleanup_failed: false,
    }
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[cfg(windows)]
const CLSID_FILE_OPEN_DIALOG: Guid = Guid {
    data1: 0xDC1C5A9C,
    data2: 0xE88A,
    data3: 0x4DDE,
    data4: [0xA5, 0xA1, 0x60, 0xF8, 0x2A, 0x20, 0xAE, 0xF7],
};
#[cfg(windows)]
const CLSID_FILE_SAVE_DIALOG: Guid = Guid {
    data1: 0xC0B4E2F3,
    data2: 0xBA21,
    data3: 0x4773,
    data4: [0x8D, 0xBA, 0x33, 0x5E, 0xC9, 0x46, 0xEB, 0x8B],
};
#[cfg(windows)]
const IID_FILE_OPEN_DIALOG: Guid = Guid {
    data1: 0xD57C7288,
    data2: 0xD4AD,
    data3: 0x4768,
    data4: [0xBE, 0x02, 0x9D, 0x96, 0x95, 0x32, 0xD9, 0x60],
};
#[cfg(windows)]
const IID_FILE_SAVE_DIALOG: Guid = Guid {
    data1: 0x84BCCD23,
    data2: 0x5FDE,
    data3: 0x4CDB,
    data4: [0xAE, 0xA4, 0xAF, 0x64, 0xB8, 0x3D, 0x78, 0xAB],
};
#[cfg(windows)]
const CLSCTX_INPROC_SERVER: u32 = 1;
#[cfg(any(windows, test))]
const HRESULT_CANCELLED: i32 = 0x8007_04C7u32 as i32;
#[cfg(windows)]
const FOS_OVERWRITEPROMPT: u32 = 0x2;
#[cfg(windows)]
const FOS_FORCEFILESYSTEM: u32 = 0x40;
#[cfg(windows)]
const FOS_PATHMUSTEXIST: u32 = 0x800;
#[cfg(windows)]
const FOS_FILEMUSTEXIST: u32 = 0x1000;
#[cfg(windows)]
const SIGDN_FILESYSPATH: u32 = 0x8005_8000;

#[cfg(windows)]
#[repr(C)]
struct FilterSpec {
    name: *const u16,
    spec: *const u16,
}

#[cfg(any(windows, test))]
struct ComPtr(NonNull<c_void>);
#[cfg(any(windows, test))]
impl ComPtr {
    unsafe fn from_raw(pointer: *mut c_void) -> Option<Self> {
        NonNull::new(pointer).map(Self)
    }
    fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}
#[cfg(any(windows, test))]
impl Drop for ComPtr {
    fn drop(&mut self) {
        // IUnknown::Release is slot 2. Ownership is one COM reference.
        unsafe {
            call_vtable::<Release>(self.as_ptr(), 2)(self.as_ptr());
        }
    }
}

#[cfg(windows)]
struct CoTaskString(*mut u16);
#[cfg(windows)]
impl Drop for CoTaskString {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(self.0.cast()) };
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(any(windows, test))]
unsafe fn call_vtable<T: Copy>(interface: *mut c_void, slot: usize) -> T {
    // SAFETY: caller supplies a live COM interface and a slot verified against SDK declarations.
    let vtable = unsafe { *(interface.cast::<*const *const c_void>()) };
    // SAFETY: vtable slots have pointer-sized function entries.
    let function = unsafe { *vtable.add(slot) };
    // SAFETY: this function pointer type is chosen for the exact documented slot.
    unsafe { std::mem::transmute_copy(&function) }
}

#[cfg(any(windows, test))]
fn check_hr(_operation: &'static str, hr: i32) -> Result<(), FileError> {
    if hr >= 0 {
        return Ok(());
    }
    let mut value = error(FileErrorCode::DialogFailed, "files.error.dialog_failed");
    value.hresult = Some(hr);
    Err(value)
}

pub fn is_dialog_cancelled_hresult(hr: i32) -> bool {
    hr == HRESULT_CANCELLED
}

fn interpret_show_hresult(hr: i32) -> Result<bool, FileError> {
    if is_dialog_cancelled_hresult(hr) {
        return Ok(false);
    }
    check_hr("IFileDialog::Show", hr)?;
    Ok(true)
}

#[cfg(windows)]
fn create_dialog(class_id: &Guid, iid: &Guid) -> Result<ComPtr, FileError> {
    let mut output = null_mut();
    // SAFETY: GUID pointers and output storage remain valid for this synchronous COM call.
    let hr =
        unsafe { CoCreateInstance(class_id, null_mut(), CLSCTX_INPROC_SERVER, iid, &mut output) };
    check_hr("CoCreateInstance", hr)?;
    // SAFETY: successful CoCreateInstance returns one owned interface reference.
    unsafe { ComPtr::from_raw(output) }
        .ok_or_else(|| error(FileErrorCode::DialogFailed, "files.error.empty_interface"))
}

#[cfg(any(windows, test))]
type Release = unsafe extern "system" fn(*mut c_void) -> u32;
#[cfg(windows)]
type Show = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;
#[cfg(windows)]
type SetFileTypes = unsafe extern "system" fn(*mut c_void, u32, *const FilterSpec) -> i32;
#[cfg(windows)]
type SetFileTypeIndex = unsafe extern "system" fn(*mut c_void, u32) -> i32;
#[cfg(windows)]
type GetOptions = unsafe extern "system" fn(*mut c_void, *mut u32) -> i32;
#[cfg(windows)]
type SetOptions = unsafe extern "system" fn(*mut c_void, u32) -> i32;
#[cfg(windows)]
type SetWideString = unsafe extern "system" fn(*mut c_void, *const u16) -> i32;
#[cfg(windows)]
type GetResult = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32;
#[cfg(windows)]
type GetDisplayName = unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16) -> i32;

#[cfg(windows)]
#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(
        class: *const Guid,
        outer: *mut c_void,
        context: u32,
        iid: *const Guid,
        result: *mut *mut c_void,
    ) -> i32;
    fn CoTaskMemFree(memory: *mut c_void);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_filters_before_any_native_call() {
        assert!(FileFilter::new("Open XML", ["docx", "odt"]).is_ok());
        for (label, extensions) in [
            ("", vec!["txt"]),
            ("Text", vec![""]),
            ("Text", vec![".txt"]),
            ("Text", vec!["*.*"]),
            ("Text", vec!["a;*.b"]),
        ] {
            assert_eq!(
                FileFilter::new(label, extensions).unwrap_err().code,
                FileErrorCode::InvalidFilter
            );
        }
    }

    #[test]
    fn filter_pattern_has_only_validated_wildcards() {
        let filter = FileFilter::new("Text", ["txt", "md"]).unwrap();
        assert_eq!(filter.pattern(), "*.txt;*.md");
    }

    #[test]
    fn cancel_hresult_is_distinct_and_owned_interfaces_release_once() {
        assert!(!interpret_show_hresult(0x8007_04C7u32 as i32).unwrap());
        assert!(interpret_show_hresult(0).unwrap());
        let failure = interpret_show_hresult(0x8007_0005u32 as i32).unwrap_err();
        assert_eq!(failure.code, FileErrorCode::DialogFailed);
        assert_eq!(failure.hresult, Some(0x8007_0005u32 as i32));

        #[repr(C)]
        struct FakeCom {
            vtable: *const *const c_void,
            releases: std::sync::atomic::AtomicUsize,
        }
        unsafe extern "system" fn release(this: *mut c_void) -> u32 {
            // SAFETY: the fake interface remains alive for the duration of this test.
            unsafe {
                (*this.cast::<FakeCom>())
                    .releases
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            0
        }
        let mut table = [std::ptr::null(); 3];
        table[2] = release as *const () as *const c_void;
        let mut fake = FakeCom {
            vtable: table.as_ptr(),
            releases: std::sync::atomic::AtomicUsize::new(0),
        };
        // SAFETY: this test supplies a vtable with IUnknown::Release in slot 2.
        drop(unsafe { ComPtr::from_raw((&mut fake as *mut FakeCom).cast()) }.unwrap());
        assert_eq!(fake.releases.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
}
