// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native file capabilities and bounded, atomic file access.
//!
//! This module is transport-independent. A caller may only grant a capability
//! after a native choice or another trusted native decision; web content must
//! only receive the opaque ticket, never a path.

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_READ_BYTES: u64 = 1 << 30;
pub const MAX_FILE_TICKETS: usize = 4096;

// Ticket IDs are process-wide so a ticket from one broker can never address a
// grant with the same numeric key in a different broker instance.
static NEXT_TICKET_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileErrorCode {
    InvalidPath,
    InvalidLimit,
    NotFound,
    NotRegularFile,
    ReparsePoint,
    TooLarge,
    ReadFailed,
    WriteFailed,
    UnknownTicket,
    WrongCapability,
    TicketExhausted,
    DestinationChanged,
    DestinationIsDirectory,
    InvalidFilter,
    DialogFailed,
    ComInitializationFailed,
    BrokerFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStage {
    ValidateDestination,
    CreateTemporary,
    WriteTemporary,
    FlushTemporary,
    ReplaceDestination,
    PublishNewDestination,
    CleanupTemporary,
}

/// Stable, translatable error data. Visible messages deliberately omit paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileError {
    pub code: FileErrorCode,
    pub message_key: &'static str,
    pub windows_code: Option<u32>,
    pub hresult: Option<i32>,
    pub stage: Option<SaveStage>,
    pub cleanup_failed: bool,
}

impl FileError {
    fn new(code: FileErrorCode, message_key: &'static str) -> Self {
        Self {
            code,
            message_key,
            windows_code: None,
            hresult: None,
            stage: None,
            cleanup_failed: false,
        }
    }

    fn io(code: FileErrorCode, key: &'static str, error: &io::Error) -> Self {
        Self {
            windows_code: error
                .raw_os_error()
                .and_then(|value| u32::try_from(value).ok()),
            ..Self::new(code, key)
        }
    }

    fn at_stage(mut self, stage: SaveStage) -> Self {
        self.stage = Some(stage);
        self
    }
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message_key)
    }
}

impl std::error::Error for FileError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReadTicket(u64);

impl ReadTicket {
    /// Numeric transport representation; it contains no path or file metadata.
    pub fn id(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WriteTicket(u64);

impl WriteTicket {
    /// Numeric transport representation; it contains no path or file metadata.
    pub fn id(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Capability {
    Read { max_bytes: u64 },
    Write,
}

#[derive(Debug, Clone)]
struct Grant {
    path: PathBuf,
    capability: Capability,
    target: ExpectedTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExpectedTarget {
    Missing,
    Existing(FileIdentity),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(not(windows))]
    length: u64,
    #[cfg(not(windows))]
    modified: Option<std::time::SystemTime>,
    #[cfg(windows)]
    volume: u32,
    #[cfg(windows)]
    index: u64,
}

impl FileIdentity {
    fn from_path(path: &Path, metadata: &fs::Metadata) -> Result<Self, FileError> {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use std::ptr::null_mut;
            type Handle = *mut std::ffi::c_void;
            #[repr(C)]
            #[derive(Default)]
            struct FileInfo {
                attributes: u32,
                creation_low: u32,
                creation_high: u32,
                access_low: u32,
                access_high: u32,
                write_low: u32,
                write_high: u32,
                volume: u32,
                size_high: u32,
                size_low: u32,
                links: u32,
                index_high: u32,
                index_low: u32,
            }
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn CreateFileW(
                    name: *const u16,
                    access: u32,
                    share: u32,
                    security: *mut std::ffi::c_void,
                    creation: u32,
                    flags: u32,
                    template: Handle,
                ) -> Handle;
                fn GetFileInformationByHandle(handle: Handle, information: *mut FileInfo) -> i32;
                fn CloseHandle(handle: Handle) -> i32;
            }
            const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: `wide` is NUL-terminated and remains live for this call.
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    0x80,
                    0x7,
                    null_mut(),
                    3,
                    0x0020_0000,
                    null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                let error = io::Error::last_os_error();
                return Err(FileError::io(
                    FileErrorCode::InvalidPath,
                    "files.error.invalid_path",
                    &error,
                ));
            }
            let mut info = FileInfo::default();
            // SAFETY: handle is owned here and info is initialized writable storage.
            let success = unsafe { GetFileInformationByHandle(handle, &mut info) };
            let error = (success == 0).then(io::Error::last_os_error);
            // SAFETY: release the handle acquired above on all successful opens.
            unsafe { CloseHandle(handle) };
            if let Some(error) = error {
                return Err(FileError::io(
                    FileErrorCode::InvalidPath,
                    "files.error.invalid_path",
                    &error,
                ));
            }
            let _ = metadata;
            Ok(Self {
                volume: info.volume,
                index: (u64::from(info.index_high) << 32) | u64::from(info.index_low),
            })
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Ok(Self {
                length: metadata.len(),
                modified: metadata.modified().ok(),
            })
        }
    }
}

/// In-memory capability registry. IDs monotonically increase and never wrap.
pub struct FileBroker {
    grants: Mutex<HashMap<u64, Grant>>,
    write_lock: Mutex<()>,
}

impl Default for FileBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl FileBroker {
    pub fn new() -> Self {
        Self {
            grants: Mutex::new(HashMap::new()),
            write_lock: Mutex::new(()),
        }
    }

    /// Grant read access to a path selected by trusted native code.
    pub fn grant_read(&self, path: &Path, max_bytes: u64) -> Result<ReadTicket, FileError> {
        if max_bytes == 0 || max_bytes > MAX_READ_BYTES {
            return Err(FileError::new(
                FileErrorCode::InvalidLimit,
                "files.error.invalid_limit",
            ));
        }
        validate_existing_file(path)?;
        self.insert(path, Capability::Read { max_bytes })
            .map(ReadTicket)
    }

    /// Grant save access to the explicitly selected destination.
    pub fn grant_write(&self, path: &Path) -> Result<WriteTicket, FileError> {
        validate_destination(path)?;
        self.insert(path, Capability::Write).map(WriteTicket)
    }

    /// Read at most the ticket's bound, detecting both stale metadata and growth.
    pub fn read(&self, ticket: ReadTicket) -> Result<Vec<u8>, FileError> {
        let grant = self.lookup(ticket.0, CapabilityKind::Read)?;
        let Capability::Read { max_bytes } = grant.capability else {
            return Err(FileError::new(
                FileErrorCode::WrongCapability,
                "files.error.wrong_capability",
            ));
        };
        validate_existing_file(&grant.path)?;
        if grant.target != ExpectedTarget::Existing(identity_for(&grant.path)?) {
            return Err(FileError::new(
                FileErrorCode::DestinationChanged,
                "files.error.destination_changed",
            ));
        }
        let file = open_read_without_reparse(&grant.path).map_err(|error| {
            FileError::io(FileErrorCode::ReadFailed, "files.error.read_failed", &error)
        })?;
        Self::read_opened_file(file, &grant.target, max_bytes)
    }

    /// Read from the already-opened handle only after checking that handle's
    /// identity against the grant. The path may have changed after path checks.
    fn read_opened_file(
        mut file: File,
        expected: &ExpectedTarget,
        max_bytes: u64,
    ) -> Result<Vec<u8>, FileError> {
        let metadata = file.metadata().map_err(|error| {
            FileError::io(FileErrorCode::ReadFailed, "files.error.read_failed", &error)
        })?;
        if !metadata.is_file() {
            return Err(FileError::new(
                FileErrorCode::NotRegularFile,
                "files.error.not_regular_file",
            ));
        }
        reject_reparse_metadata(&metadata)?;
        if expected != &ExpectedTarget::Existing(identity_for_file(&file, &metadata)?) {
            return Err(FileError::new(
                FileErrorCode::DestinationChanged,
                "files.error.destination_changed",
            ));
        }
        if metadata.len() > max_bytes {
            return Err(FileError::new(
                FileErrorCode::TooLarge,
                "files.error.too_large",
            ));
        }
        read_bounded(&mut file, max_bytes, metadata.len())
    }

    /// Atomically save bytes at the path explicitly authorized by this ticket.
    pub fn write(&self, ticket: WriteTicket, bytes: &[u8]) -> Result<(), FileError> {
        // Keep grant snapshot, publication and expected-identity refresh in one
        // critical section so same-ticket writes cannot publish out of order.
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let grant = self.lookup(ticket.0, CapabilityKind::Write)?;
        if grant.capability != Capability::Write {
            return Err(FileError::new(
                FileErrorCode::WrongCapability,
                "files.error.wrong_capability",
            ));
        }
        let refreshed_target =
            save_atomically_with(&grant.path, &grant.target, bytes, &mut SystemAtomicFs)?;
        if let Some(current) = self
            .grants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&ticket.0)
        {
            current.target = refreshed_target;
        }
        Ok(())
    }

    pub fn revoke_read(&self, ticket: ReadTicket) -> bool {
        self.revoke(ticket.0)
    }

    pub fn revoke_write(&self, ticket: WriteTicket) -> bool {
        self.revoke(ticket.0)
    }

    fn insert(&self, path: &Path, capability: Capability) -> Result<u64, FileError> {
        let target = match capability {
            Capability::Read { .. } => ExpectedTarget::Existing(identity_for(path)?),
            Capability::Write => match fs::symlink_metadata(path) {
                Ok(metadata) => {
                    reject_reparse_metadata(&metadata)?;
                    if metadata.is_dir() {
                        return Err(FileError::new(
                            FileErrorCode::DestinationIsDirectory,
                            "files.error.destination_is_directory",
                        ));
                    }
                    ExpectedTarget::Existing(FileIdentity::from_path(path, &metadata)?)
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => ExpectedTarget::Missing,
                Err(error) => {
                    return Err(FileError::io(
                        FileErrorCode::InvalidPath,
                        "files.error.invalid_path",
                        &error,
                    ));
                }
            },
        };
        let id = loop {
            let current = NEXT_TICKET_ID.load(Ordering::Relaxed);
            let next = current.checked_add(1).ok_or_else(|| {
                FileError::new(
                    FileErrorCode::TicketExhausted,
                    "files.error.ticket_exhausted",
                )
            })?;
            if NEXT_TICKET_ID
                .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break current;
            }
        };
        let mut grants = self
            .grants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if grants.len() >= MAX_FILE_TICKETS {
            return Err(FileError::new(
                FileErrorCode::BrokerFull,
                "files.error.broker_full",
            ));
        }
        grants.insert(
            id,
            Grant {
                path: path.to_path_buf(),
                capability,
                target,
            },
        );
        Ok(id)
    }

    fn lookup(&self, id: u64, expected: CapabilityKind) -> Result<Grant, FileError> {
        let grant = self
            .grants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                FileError::new(FileErrorCode::UnknownTicket, "files.error.unknown_ticket")
            })?;
        let matches = matches!(
            (expected, grant.capability),
            (CapabilityKind::Read, Capability::Read { .. })
                | (CapabilityKind::Write, Capability::Write)
        );
        if !matches {
            return Err(FileError::new(
                FileErrorCode::WrongCapability,
                "files.error.wrong_capability",
            ));
        }
        Ok(grant)
    }

    fn revoke(&self, id: u64) -> bool {
        self.grants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id)
            .is_some()
    }
}

#[derive(Clone, Copy)]
enum CapabilityKind {
    Read,
    Write,
}

fn validate_existing_file(path: &Path) -> Result<(), FileError> {
    validate_absolute_path(path)?;
    validate_components(path, true)?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| FileError::io(FileErrorCode::NotFound, "files.error.not_found", &error))?;
    reject_reparse_metadata(&metadata)?;
    if !metadata.is_file() {
        return Err(FileError::new(
            FileErrorCode::NotRegularFile,
            "files.error.not_regular_file",
        ));
    }
    Ok(())
}

fn validate_destination(path: &Path) -> Result<(), FileError> {
    validate_absolute_path(path)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| FileError::new(FileErrorCode::InvalidPath, "files.error.invalid_path"))?;
    validate_components(parent, true)?;
    let parent_meta = fs::symlink_metadata(parent).map_err(|error| {
        FileError::io(
            FileErrorCode::InvalidPath,
            "files.error.invalid_path",
            &error,
        )
    })?;
    reject_reparse_metadata(&parent_meta)?;
    if !parent_meta.is_dir() {
        return Err(FileError::new(
            FileErrorCode::InvalidPath,
            "files.error.invalid_path",
        ));
    }
    let name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| FileError::new(FileErrorCode::InvalidPath, "files.error.invalid_path"))?;
    if name == "." || name == ".." {
        return Err(FileError::new(
            FileErrorCode::InvalidPath,
            "files.error.invalid_path",
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            reject_reparse_metadata(&metadata)?;
            if metadata.is_dir() {
                return Err(FileError::new(
                    FileErrorCode::DestinationIsDirectory,
                    "files.error.destination_is_directory",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(FileError::io(
                FileErrorCode::InvalidPath,
                "files.error.invalid_path",
                &error,
            ));
        }
    }
    Ok(())
}

fn validate_absolute_path(path: &Path) -> Result<(), FileError> {
    if !path.is_absolute() || path.as_os_str().to_string_lossy().contains('\0') {
        Err(FileError::new(
            FileErrorCode::InvalidPath,
            "files.error.invalid_path",
        ))
    } else {
        Ok(())
    }
}

fn validate_components(path: &Path, include_leaf: bool) -> Result<(), FileError> {
    let mut current = PathBuf::new();
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {
                current.push(component.as_os_str())
            }
            Component::ParentDir => {
                return Err(FileError::new(
                    FileErrorCode::InvalidPath,
                    "files.error.invalid_path",
                ));
            }
            Component::Normal(part) => current.push(part),
        }
        let should_check = include_leaf || components.peek().is_some();
        if should_check && current.exists() {
            let metadata = fs::symlink_metadata(&current).map_err(|error| {
                FileError::io(
                    FileErrorCode::InvalidPath,
                    "files.error.invalid_path",
                    &error,
                )
            })?;
            reject_reparse_metadata(&metadata)?;
        }
    }
    Ok(())
}

fn reject_reparse_metadata(metadata: &fs::Metadata) -> Result<(), FileError> {
    if metadata.file_type().is_symlink() || is_reparse_point(metadata) {
        Err(FileError::new(
            FileErrorCode::ReparsePoint,
            "files.error.reparse_point",
        ))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
}

#[cfg(not(windows))]
fn is_reparse_point(_: &fs::Metadata) -> bool {
    false
}

fn identity_for(path: &Path) -> Result<FileIdentity, FileError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| FileError::io(FileErrorCode::NotFound, "files.error.not_found", &error))?;
    reject_reparse_metadata(&metadata)?;
    FileIdentity::from_path(path, &metadata)
}

fn identity_for_file(file: &File, metadata: &fs::Metadata) -> Result<FileIdentity, FileError> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)]
        #[derive(Default)]
        struct FileInfo {
            attributes: u32,
            creation_low: u32,
            creation_high: u32,
            access_low: u32,
            access_high: u32,
            write_low: u32,
            write_high: u32,
            volume: u32,
            size_high: u32,
            size_low: u32,
            links: u32,
            index_high: u32,
            index_low: u32,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetFileInformationByHandle(
                handle: *mut std::ffi::c_void,
                information: *mut FileInfo,
            ) -> i32;
        }
        let mut info = FileInfo::default();
        // SAFETY: `file` owns a live handle and `info` is initialized writable storage.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut info) } == 0 {
            return Err(FileError::io(
                FileErrorCode::InvalidPath,
                "files.error.invalid_path",
                &io::Error::last_os_error(),
            ));
        }
        let _ = metadata;
        Ok(FileIdentity {
            volume: info.volume,
            index: (u64::from(info.index_high) << 32) | u64::from(info.index_low),
        })
    }
    #[cfg(not(windows))]
    {
        let _ = file;
        Ok(FileIdentity {
            length: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

#[cfg(windows)]
fn open_read_without_reparse(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

fn read_bounded(
    reader: &mut impl Read,
    max_bytes: u64,
    metadata_len: u64,
) -> Result<Vec<u8>, FileError> {
    if metadata_len > max_bytes {
        return Err(FileError::new(
            FileErrorCode::TooLarge,
            "files.error.too_large",
        ));
    }
    let sentinel_limit = max_bytes
        .checked_add(1)
        .ok_or_else(|| FileError::new(FileErrorCode::InvalidLimit, "files.error.invalid_limit"))?;
    let reserve = usize::try_from(metadata_len).unwrap_or(0);
    let mut bytes = Vec::with_capacity(reserve);
    reader
        .take(sentinel_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            FileError::io(FileErrorCode::ReadFailed, "files.error.read_failed", &error)
        })?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > max_bytes) {
        return Err(FileError::new(
            FileErrorCode::TooLarge,
            "files.error.too_large",
        ));
    }
    Ok(bytes)
}

#[cfg(not(windows))]
fn open_read_without_reparse(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

trait AtomicFs {
    fn create_temp(&mut self, path: &Path) -> io::Result<File>;
    fn write_temp(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<()>;
    fn sync_temp(&mut self, file: &File) -> io::Result<()>;
    fn replace_existing(&mut self, temp: &Path, destination: &Path) -> io::Result<()>;
    fn publish_new(&mut self, temp: &Path, destination: &Path) -> io::Result<()>;
    fn remove_temp(&mut self, path: &Path) -> io::Result<()>;
}

struct SystemAtomicFs;

impl AtomicFs for SystemAtomicFs {
    fn create_temp(&mut self, path: &Path) -> io::Result<File> {
        OpenOptions::new().write(true).create_new(true).open(path)
    }

    fn write_temp(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        file.write_all(bytes)
    }

    fn sync_temp(&mut self, file: &File) -> io::Result<()> {
        file.sync_all()
    }

    fn replace_existing(&mut self, temp: &Path, destination: &Path) -> io::Result<()> {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use std::ptr::null_mut;
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn ReplaceFileW(
                    replaced: *const u16,
                    replacement: *const u16,
                    backup: *const u16,
                    flags: u32,
                    exclude: *mut std::ffi::c_void,
                    reserved: *mut std::ffi::c_void,
                ) -> i32;
            }
            let replaced: Vec<u16> = destination
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();
            let replacement: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: both strings are NUL-terminated and live for the call.
            let ok = unsafe {
                ReplaceFileW(
                    replaced.as_ptr(),
                    replacement.as_ptr(),
                    null_mut(),
                    0,
                    null_mut(),
                    null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
        #[cfg(not(windows))]
        fs::rename(temp, destination)
    }

    fn publish_new(&mut self, temp: &Path, destination: &Path) -> io::Result<()> {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn MoveFileW(existing: *const u16, new: *const u16) -> i32;
            }
            let existing: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let new: Vec<u16> = destination
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();
            // MoveFileW on the same volume is atomic and fails if `new` exists.
            // SAFETY: both strings are NUL-terminated and live for the call.
            let ok = unsafe { MoveFileW(existing.as_ptr(), new.as_ptr()) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            // A same-filesystem hard link publishes complete bytes without replacing a race winner.
            fs::hard_link(temp, destination)?;
            fs::remove_file(temp)
        }
    }

    fn remove_temp(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}

fn save_atomically_with(
    destination: &Path,
    expected: &ExpectedTarget,
    bytes: &[u8],
    fs_ops: &mut impl AtomicFs,
) -> Result<ExpectedTarget, FileError> {
    validate_destination(destination)
        .map_err(|error| error.at_stage(SaveStage::ValidateDestination))?;
    let current = match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            reject_reparse_metadata(&metadata)
                .map_err(|error| error.at_stage(SaveStage::ValidateDestination))?;
            if metadata.is_dir() {
                return Err(FileError::new(
                    FileErrorCode::DestinationIsDirectory,
                    "files.error.destination_is_directory",
                )
                .at_stage(SaveStage::ValidateDestination));
            }
            ExpectedTarget::Existing(FileIdentity::from_path(destination, &metadata)?)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => ExpectedTarget::Missing,
        Err(error) => {
            return Err(FileError::io(
                FileErrorCode::InvalidPath,
                "files.error.invalid_path",
                &error,
            )
            .at_stage(SaveStage::ValidateDestination));
        }
    };
    if &current != expected {
        return Err(FileError::new(
            FileErrorCode::DestinationChanged,
            "files.error.destination_changed",
        )
        .at_stage(SaveStage::ValidateDestination));
    }
    let parent = destination
        .parent()
        .expect("validated absolute destination has parent");
    let temp = next_temp_path(parent);
    let mut temp_file = fs_ops.create_temp(&temp).map_err(|error| {
        FileError::io(
            FileErrorCode::WriteFailed,
            "files.error.save_failed",
            &error,
        )
        .at_stage(SaveStage::CreateTemporary)
    })?;
    let mut result = fs_ops.write_temp(&mut temp_file, bytes).map_err(|error| {
        FileError::io(
            FileErrorCode::WriteFailed,
            "files.error.save_failed",
            &error,
        )
        .at_stage(SaveStage::WriteTemporary)
    });
    if result.is_ok() {
        result = fs_ops.sync_temp(&temp_file).map_err(|error| {
            FileError::io(
                FileErrorCode::WriteFailed,
                "files.error.save_failed",
                &error,
            )
            .at_stage(SaveStage::FlushTemporary)
        });
    }
    let published_target = if result.is_ok() {
        match identity_for(&temp) {
            Ok(identity) => Some(ExpectedTarget::Existing(identity)),
            Err(error) => {
                drop(temp_file);
                return cleanup_after_failure(
                    fs_ops,
                    &temp,
                    error.at_stage(SaveStage::FlushTemporary),
                );
            }
        }
    } else {
        None
    };
    drop(temp_file);
    if result.is_ok() {
        // Recheck immediately before the path-based atomic publication.
        let now = match fs::symlink_metadata(destination) {
            Ok(metadata) => {
                if let Err(error) = reject_reparse_metadata(&metadata) {
                    return cleanup_after_failure(
                        fs_ops,
                        &temp,
                        error.at_stage(SaveStage::ValidateDestination),
                    );
                }
                if !metadata.is_file() {
                    return cleanup_after_failure(
                        fs_ops,
                        &temp,
                        FileError::new(
                            FileErrorCode::DestinationIsDirectory,
                            "files.error.destination_is_directory",
                        )
                        .at_stage(SaveStage::ValidateDestination),
                    );
                }
                let identity = match FileIdentity::from_path(destination, &metadata) {
                    Ok(identity) => identity,
                    Err(error) => {
                        return cleanup_after_failure(
                            fs_ops,
                            &temp,
                            error.at_stage(SaveStage::ValidateDestination),
                        );
                    }
                };
                ExpectedTarget::Existing(identity)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => ExpectedTarget::Missing,
            Err(error) => {
                return cleanup_after_failure(
                    fs_ops,
                    &temp,
                    FileError::io(
                        FileErrorCode::InvalidPath,
                        "files.error.invalid_path",
                        &error,
                    )
                    .at_stage(SaveStage::ValidateDestination),
                );
            }
        };
        if &now != expected {
            return cleanup_after_failure(
                fs_ops,
                &temp,
                FileError::new(
                    FileErrorCode::DestinationChanged,
                    "files.error.destination_changed",
                )
                .at_stage(SaveStage::ValidateDestination),
            );
        }
        let publish = match expected {
            ExpectedTarget::Existing(_) => {
                fs_ops
                    .replace_existing(&temp, destination)
                    .map_err(|error| {
                        FileError::io(
                            FileErrorCode::WriteFailed,
                            "files.error.save_failed",
                            &error,
                        )
                        .at_stage(SaveStage::ReplaceDestination)
                    })
            }
            ExpectedTarget::Missing => fs_ops.publish_new(&temp, destination).map_err(|error| {
                FileError::io(
                    FileErrorCode::WriteFailed,
                    "files.error.save_failed",
                    &error,
                )
                .at_stage(SaveStage::PublishNewDestination)
            }),
        };
        if let Err(error) = publish {
            result = Err(error);
        }
    }
    match result {
        Ok(()) => match published_target {
            Some(target) => Ok(target),
            None => Err(
                FileError::new(FileErrorCode::WriteFailed, "files.error.save_failed")
                    .at_stage(SaveStage::PublishNewDestination),
            ),
        },
        Err(error) => cleanup_after_failure(fs_ops, &temp, error),
    }
}

fn cleanup_after_failure(
    fs_ops: &mut impl AtomicFs,
    temp: &Path,
    mut error: FileError,
) -> Result<ExpectedTarget, FileError> {
    if let Err(cleanup) = fs_ops.remove_temp(temp) {
        if cleanup.kind() != io::ErrorKind::NotFound {
            error.cleanup_failed = true;
            if error.windows_code.is_none() {
                error.windows_code = cleanup
                    .raw_os_error()
                    .and_then(|value| u32::try_from(value).ok());
            }
            if error.stage.is_none() {
                error.stage = Some(SaveStage::CleanupTemporary);
            }
        }
    }
    Err(error)
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn next_temp_path(parent: &Path) -> PathBuf {
    let id = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".atelier-save-{}-{id:016x}.tmp",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir();
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = root.join(format!(
                "atelier-us113-file-test-{}-{stamp:x}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            assert!(path.is_absolute() && path.starts_with(&root));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let root = std::env::temp_dir();
            if self.0.is_absolute()
                && self.0.starts_with(root)
                && self.0.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with("atelier-us113-file-test-")
                })
            {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn broker_reads_exact_binary_bytes_and_revokes_tickets() {
        let dir = TestDir::new();
        let path = dir.0.join("binary.dat");
        fs::write(&path, [0, 0xff, 0x10, 0]).unwrap();
        let broker = FileBroker::new();
        let ticket = broker.grant_read(&path, 4).unwrap();
        assert_eq!(broker.read(ticket).unwrap(), [0, 0xff, 0x10, 0]);
        assert!(broker.revoke_read(ticket));
        assert_eq!(
            broker.read(ticket).unwrap_err().code,
            FileErrorCode::UnknownTicket
        );
    }

    #[test]
    fn opened_substitute_handle_is_rejected_before_reading_bytes() {
        let dir = TestDir::new();
        let selected = dir.0.join("selected.bin");
        let substitute = dir.0.join("substitute.bin");
        fs::write(&selected, b"authorized-content").unwrap();
        fs::write(&substitute, b"different-substitute-content").unwrap();
        let broker = FileBroker::new();
        let ticket = broker.grant_read(&selected, 128).unwrap();
        let grant = broker.lookup(ticket.id(), CapabilityKind::Read).unwrap();
        let substituted_handle = open_read_without_reparse(&substitute).unwrap();

        assert_eq!(
            FileBroker::read_opened_file(substituted_handle, &grant.target, 128)
                .unwrap_err()
                .code,
            FileErrorCode::DestinationChanged
        );
    }

    #[test]
    fn tickets_cannot_cross_broker_instances_for_read_or_write() {
        let dir = TestDir::new();
        let source = dir.0.join("source.bin");
        fs::write(&source, b"source").unwrap();
        let first = FileBroker::new();
        let second = FileBroker::new();
        let first_read = first.grant_read(&source, 32).unwrap();
        let second_read = second.grant_read(&source, 32).unwrap();
        assert_ne!(first_read.id(), second_read.id());
        assert_eq!(
            second.read(first_read).unwrap_err().code,
            FileErrorCode::UnknownTicket
        );

        let first_target = dir.0.join("first-target.bin");
        let second_target = dir.0.join("second-target.bin");
        let first_write = first.grant_write(&first_target).unwrap();
        let second_write = second.grant_write(&second_target).unwrap();
        assert_ne!(first_write.id(), second_write.id());
        assert_eq!(
            second.write(first_write, b"cross-broker").unwrap_err().code,
            FileErrorCode::UnknownTicket
        );
        assert!(!second_target.exists());
        assert_eq!(second.read(second_read).unwrap(), b"source");
    }

    #[test]
    fn bounded_reader_detects_growth_after_metadata_and_honors_exact_limit() {
        let mut exact = Cursor::new(b"four".to_vec());
        assert_eq!(read_bounded(&mut exact, 4, 3).unwrap(), b"four");
        let mut grown = Cursor::new(b"five!".to_vec());
        assert_eq!(
            read_bounded(&mut grown, 4, 2).unwrap_err().code,
            FileErrorCode::TooLarge
        );
        let mut too_large = Cursor::new(b"five!".to_vec());
        assert_eq!(
            read_bounded(&mut too_large, 4, 5).unwrap_err().code,
            FileErrorCode::TooLarge
        );
        assert_eq!(
            read_bounded(&mut Cursor::new(Vec::<u8>::new()), 0, 0).unwrap(),
            b""
        );
    }

    #[test]
    fn wrong_right_invalid_paths_and_symlinks_are_rejected() {
        let dir = TestDir::new();
        let file = dir.0.join("source.txt");
        fs::write(&file, b"ok").unwrap();
        let broker = FileBroker::new();
        let read = broker.grant_read(&file, 10).unwrap();
        let write = broker.grant_write(&dir.0.join("destination.txt")).unwrap();
        assert_eq!(
            broker
                .lookup(read.0, CapabilityKind::Write)
                .unwrap_err()
                .code,
            FileErrorCode::WrongCapability
        );
        assert_eq!(
            broker
                .lookup(write.0, CapabilityKind::Read)
                .unwrap_err()
                .code,
            FileErrorCode::WrongCapability
        );
        assert_eq!(
            broker
                .grant_read(Path::new("relative"), 10)
                .unwrap_err()
                .code,
            FileErrorCode::InvalidPath
        );
        assert_eq!(
            broker
                .grant_read(&file, MAX_READ_BYTES + 1)
                .unwrap_err()
                .code,
            FileErrorCode::InvalidLimit
        );
        let link = dir.0.join("link.txt");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&file, &link).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&file, &link).is_ok();
        if linked {
            assert_eq!(
                broker.grant_read(&link, 10).unwrap_err().code,
                FileErrorCode::ReparsePoint
            );
            assert_eq!(
                broker.grant_write(&link).unwrap_err().code,
                FileErrorCode::ReparsePoint
            );
        }
        assert_eq!(
            broker.read(ReadTicket(u64::MAX)).unwrap_err().code,
            FileErrorCode::UnknownTicket
        );
        assert_eq!(
            broker.write(WriteTicket(u64::MAX), b"x").unwrap_err().code,
            FileErrorCode::UnknownTicket
        );
    }

    #[test]
    fn existing_and_new_saves_publish_exact_bytes() {
        let dir = TestDir::new();
        let old = dir.0.join("existing.bin");
        fs::write(&old, b"old-complete").unwrap();
        let broker = FileBroker::new();
        let ticket = broker.grant_write(&old).unwrap();
        broker.write(ticket, b"new-complete").unwrap();
        assert_eq!(fs::read(&old).unwrap(), b"new-complete");
        broker.write(ticket, b"second-existing-write").unwrap();
        assert_eq!(fs::read(&old).unwrap(), b"second-existing-write");
        let new = dir.0.join("new.bin");
        let ticket = broker.grant_write(&new).unwrap();
        broker.write(ticket, b"created-complete").unwrap();
        assert_eq!(fs::read(&new).unwrap(), b"created-complete");
        broker.write(ticket, b"second-new-write").unwrap();
        assert_eq!(fs::read(&new).unwrap(), b"second-new-write");
    }

    #[test]
    fn ticket_registry_is_bounded_and_revocation_frees_capacity_without_reusing_ids() {
        let dir = TestDir::new();
        let broker = FileBroker::new();
        let mut last = None;
        for index in 0..MAX_FILE_TICKETS {
            last = Some(
                broker
                    .grant_write(&dir.0.join(format!("dest-{index}.bin")))
                    .unwrap(),
            );
        }
        let last = last.unwrap();
        assert_eq!(broker.grants.lock().unwrap().len(), MAX_FILE_TICKETS);
        assert_eq!(
            broker
                .grant_write(&dir.0.join("overflow.bin"))
                .unwrap_err()
                .code,
            FileErrorCode::BrokerFull
        );
        assert!(broker.revoke_write(last));
        let next = broker.grant_write(&dir.0.join("after-revoke.bin")).unwrap();
        assert!(next.id() > last.id());
        assert_eq!(broker.grants.lock().unwrap().len(), MAX_FILE_TICKETS);
    }

    #[derive(Clone, Copy)]
    enum FailAt {
        Write,
        Sync,
        Replace,
        Publish,
    }
    struct FaultFs {
        inner: SystemAtomicFs,
        fail: FailAt,
    }
    impl AtomicFs for FaultFs {
        fn create_temp(&mut self, path: &Path) -> io::Result<File> {
            self.inner.create_temp(path)
        }
        fn write_temp(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
            if matches!(self.fail, FailAt::Write) {
                file.write_all(&bytes[..bytes.len().min(2)])?;
                return Err(io::Error::other("injected partial write"));
            }
            self.inner.write_temp(file, bytes)
        }
        fn sync_temp(&mut self, file: &File) -> io::Result<()> {
            if matches!(self.fail, FailAt::Sync) {
                return Err(io::Error::other("injected flush failure"));
            }
            self.inner.sync_temp(file)
        }
        fn replace_existing(&mut self, temp: &Path, destination: &Path) -> io::Result<()> {
            if matches!(self.fail, FailAt::Replace) {
                return Err(io::Error::other("injected replace failure"));
            }
            self.inner.replace_existing(temp, destination)
        }
        fn publish_new(&mut self, temp: &Path, destination: &Path) -> io::Result<()> {
            if matches!(self.fail, FailAt::Publish) {
                fs::write(destination, b"concurrent-winner")?;
                return self.inner.publish_new(temp, destination);
            }
            self.inner.publish_new(temp, destination)
        }
        fn remove_temp(&mut self, path: &Path) -> io::Result<()> {
            self.inner.remove_temp(path)
        }
    }

    fn only_temp_files(directory: &Path) -> usize {
        fs::read_dir(directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".atelier-save-")
            })
            .count()
    }

    #[test]
    fn every_prepublication_failure_preserves_old_bytes_and_cleans_own_temp() {
        for fail in [FailAt::Write, FailAt::Sync, FailAt::Replace] {
            let dir = TestDir::new();
            let destination = dir.0.join("chosen.txt");
            fs::write(&destination, b"original-complete").unwrap();
            let expected = ExpectedTarget::Existing(identity_for(&destination).unwrap());
            let error = save_atomically_with(
                &destination,
                &expected,
                b"replacement",
                &mut FaultFs {
                    inner: SystemAtomicFs,
                    fail,
                },
            )
            .unwrap_err();
            assert!(matches!(
                error.stage,
                Some(
                    SaveStage::WriteTemporary
                        | SaveStage::FlushTemporary
                        | SaveStage::ReplaceDestination
                )
            ));
            assert_eq!(fs::read(&destination).unwrap(), b"original-complete");
            assert_eq!(only_temp_files(&dir.0), 0);
        }
    }

    #[test]
    fn concurrent_new_destination_is_never_overwritten_and_temp_is_removed() {
        let dir = TestDir::new();
        let destination = dir.0.join("new.txt");
        let error = save_atomically_with(
            &destination,
            &ExpectedTarget::Missing,
            b"ours",
            &mut FaultFs {
                inner: SystemAtomicFs,
                fail: FailAt::Publish,
            },
        )
        .unwrap_err();
        assert_eq!(error.stage, Some(SaveStage::PublishNewDestination));
        assert_eq!(fs::read(destination).unwrap(), b"concurrent-winner");
        assert_eq!(only_temp_files(&dir.0), 0);
    }

    #[test]
    fn changed_or_directory_destination_is_refused_before_publication() {
        let dir = TestDir::new();
        let path = dir.0.join("target.txt");
        let expected = ExpectedTarget::Missing;
        fs::write(&path, b"raced").unwrap();
        assert_eq!(
            save_atomically_with(&path, &expected, b"no", &mut SystemAtomicFs)
                .unwrap_err()
                .code,
            FileErrorCode::DestinationChanged
        );
        let directory = dir.0.join("subdir");
        fs::create_dir(&directory).unwrap();
        assert_eq!(
            FileBroker::new().grant_write(&directory).unwrap_err().code,
            FileErrorCode::DestinationIsDirectory
        );
    }

    #[test]
    fn existing_destination_disappearing_or_becoming_a_directory_is_refused() {
        let dir = TestDir::new();
        let disappeared = dir.0.join("disappeared.txt");
        fs::write(&disappeared, b"original").unwrap();
        let broker = FileBroker::new();
        let missing_ticket = broker.grant_write(&disappeared).unwrap();
        fs::remove_file(&disappeared).unwrap();
        assert_eq!(
            broker
                .write(missing_ticket, b"replacement")
                .unwrap_err()
                .code,
            FileErrorCode::DestinationChanged
        );
        assert!(!disappeared.exists());

        let directory_path = dir.0.join("becomes-directory");
        fs::write(&directory_path, b"original").unwrap();
        let directory_ticket = broker.grant_write(&directory_path).unwrap();
        fs::remove_file(&directory_path).unwrap();
        fs::create_dir(&directory_path).unwrap();
        assert_eq!(
            broker
                .write(directory_ticket, b"replacement")
                .unwrap_err()
                .code,
            FileErrorCode::DestinationIsDirectory
        );
        assert!(directory_path.is_dir());
        assert_eq!(only_temp_files(&dir.0), 0);
    }
}
