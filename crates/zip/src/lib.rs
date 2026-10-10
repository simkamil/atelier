// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, in-memory ZIP reader and deterministic writer.
//!
//! The reader indexes entries from the central directory and validates each
//! local header and payload only when that entry is requested. This keeps a
//! damaged member from making healthy members inaccessible.

#![forbid(unsafe_code)]

use atelier_compress::{self as compression, Format, InflateState, Level, Status};
use atelier_unicode::{self as unicode, Encoding};

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const EOCD: u32 = 0x0605_4b50;
const ZIP64_EOCD: u32 = 0x0606_4b50;
const ZIP64_LOCATOR: u32 = 0x0706_4b50;
const DESCRIPTOR: u32 = 0x0807_4b50;
const ZIP64_EXTRA: u16 = 1;
const UTF8_FLAG: u16 = 1 << 11;
const DESCRIPTOR_FLAG: u16 = 1 << 3;

/// Resource limits applied while indexing and reading an archive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_entries: usize,
    pub max_name_bytes: usize,
    pub max_entry_size: u64,
    pub max_total_size: u64,
    /// Maximum uncompressed/compressed ratio. A zero compressed stream is
    /// permitted only when it produces no output.
    pub max_compression_ratio: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_name_bytes: 4_096,
            max_entry_size: 256 * 1024 * 1024,
            max_total_size: 1024 * 1024 * 1024,
            max_compression_ratio: 200,
        }
    }
}

/// Errors that prevent an archive from being opened or a new archive written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ZipError {
    InvalidArchive(&'static str),
    Unsupported(&'static str),
    LimitExceeded { limit: &'static str },
    InvalidName,
    Compression(&'static str),
    SizeOverflow,
    NoUsableEntries,
}

impl core::fmt::Display for ZipError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidArchive(s) => write!(f, "invalid ZIP archive: {s}"),
            Self::Unsupported(s) => write!(f, "unsupported ZIP feature: {s}"),
            Self::LimitExceeded { limit } => write!(f, "ZIP limit exceeded: {limit}"),
            Self::InvalidName => f.write_str("invalid or unsafe ZIP entry name"),
            Self::Compression(s) => write!(f, "ZIP compression failed: {s}"),
            Self::SizeOverflow => f.write_str("ZIP size or offset overflow"),
            Self::NoUsableEntries => f.write_str("ZIP archive contains no usable entries"),
        }
    }
}
impl std::error::Error for ZipError {}

/// A member-specific error. Errors are kept on the entry so other members
/// remain readable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryError {
    InvalidHeader,
    Truncated,
    UnsupportedMethod(u16),
    UnsupportedFeature(&'static str),
    InvalidUtf8Name,
    NameTooLong,
    UnsafeName,
    DuplicateName,
    LimitExceeded { limit: &'static str },
    SizeMismatch,
    CrcMismatch,
    InvalidDeflate,
    InvalidDescriptor,
    AllocationFailed,
}

impl core::fmt::Display for EntryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ZIP entry error: {self:?}")
    }
}
impl std::error::Error for EntryError {}

/// A central-directory entry, or a locally recovered entry in an incomplete archive.
#[derive(Clone, Debug)]
pub struct Entry<'a> {
    name: String,
    method: u16,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    is_name_safe: bool,
    is_duplicate: bool,
    error: Option<EntryError>,
    archive: &'a [u8],
    limits: Limits,
    flags: u16,
}

impl<'a> Entry<'a> {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn method(&self) -> u16 {
        self.method
    }
    pub fn crc32(&self) -> u32 {
        self.crc32
    }
    pub fn compressed_size(&self) -> u64 {
        self.compressed_size
    }
    pub fn uncompressed_size(&self) -> u64 {
        self.uncompressed_size
    }
    pub fn local_header_offset(&self) -> u64 {
        self.local_header_offset
    }
    pub fn is_name_safe(&self) -> bool {
        self.is_name_safe
    }
    pub fn is_duplicate(&self) -> bool {
        self.is_duplicate
    }
    pub fn error(&self) -> Option<&EntryError> {
        self.error.as_ref()
    }
    /// Decompresses the member, checks its declared size and CRC, and returns
    /// the bytes only if every check succeeds.
    pub fn read_all(&self) -> Result<Vec<u8>, EntryError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        read_entry(self.archive, self, self.limits)
    }
}

/// An indexed archive. `is_complete` is false when only a bounded local-header
/// recovery was possible or a central directory was truncated.
#[derive(Debug)]
pub struct Archive<'a> {
    bytes: &'a [u8],
    limits: Limits,
    entries: Vec<Entry<'a>>,
    pub is_complete: bool,
}

impl<'a> Archive<'a> {
    pub fn open(bytes: &'a [u8], limits: Limits) -> Result<Self, ZipError> {
        let mut archive = Self {
            bytes,
            limits,
            entries: Vec::new(),
            is_complete: false,
        };
        if let Some((cd_offset, cd_size, count, end)) = find_directory(bytes)? {
            let cd_end = cd_offset
                .checked_add(cd_size)
                .filter(|&n| n <= end && n <= bytes.len() as u64);
            if let Some(cd_end) = cd_end {
                if let (Ok(start), Ok(stop)) = (usize::try_from(cd_offset), usize::try_from(cd_end))
                {
                    archive.index_central(start, stop, count)?;
                    archive.is_complete = archive.entries.len() == count as usize
                        && archive
                            .entries
                            .iter()
                            .all(|entry| entry.error != Some(EntryError::InvalidHeader))
                        && cd_end <= bytes.len() as u64;
                }
            }
        }
        if archive.entries.is_empty() && !archive.is_complete {
            archive.recover_locals()?;
            if archive.entries.is_empty() {
                return Err(ZipError::NoUsableEntries);
            }
        }
        archive.mark_duplicates();
        archive.apply_cumulative_limits();
        if !archive.entries.is_empty() && archive.entries.iter().all(|entry| entry.error.is_some())
        {
            return Err(ZipError::NoUsableEntries);
        }
        Ok(archive)
    }

    pub fn entries(&self) -> &[Entry<'a>] {
        &self.entries
    }

    /// Prefers a usable entry with this canonical slash-separated name.
    /// If only rejected entries match, returns the first for its diagnostic.
    pub fn entry(&self, name: &str) -> Option<&Entry<'a>> {
        if !is_safe_name(name) {
            return None;
        }
        let key = canonical_name(name);
        self.entries
            .iter()
            .find(|entry| entry.error.is_none() && canonical_name(&entry.name) == key)
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|entry| entry.is_name_safe && canonical_name(&entry.name) == key)
            })
    }

    fn index_central(&mut self, mut pos: usize, end: usize, expected: u64) -> Result<(), ZipError> {
        if expected > self.limits.max_entries as u64 {
            return Err(ZipError::LimitExceeded {
                limit: "entry count",
            });
        }
        for _ in 0..expected {
            let Some(fixed_end) = pos.checked_add(46).filter(|&x| x <= end) else {
                break;
            };
            let bad_signature = read_u32(self.bytes, pos) != Some(CENTRAL);
            let flags = read_u16(self.bytes, pos + 8).unwrap_or(0);
            let method = read_u16(self.bytes, pos + 10).unwrap_or(u16::MAX);
            let crc = read_u32(self.bytes, pos + 16).unwrap_or(0);
            let name_len = read_u16(self.bytes, pos + 28).unwrap_or(0) as usize;
            let extra_len = read_u16(self.bytes, pos + 30).unwrap_or(0) as usize;
            let comment_len = read_u16(self.bytes, pos + 32).unwrap_or(0) as usize;
            let record_end = fixed_end
                .checked_add(name_len)
                .and_then(|x| x.checked_add(extra_len))
                .and_then(|x| x.checked_add(comment_len));
            let Some(record_end) = record_end.filter(|&x| x <= end) else {
                break;
            };
            let raw_name = &self.bytes[fixed_end..fixed_end + name_len];
            let extra = &self.bytes[fixed_end + name_len..fixed_end + name_len + extra_len];
            let mut compressed = read_u32(self.bytes, pos + 20).unwrap_or(0) as u64;
            let mut uncompressed = read_u32(self.bytes, pos + 24).unwrap_or(0) as u64;
            let mut local_offset = read_u32(self.bytes, pos + 42).unwrap_or(0) as u64;
            let mut disk_start = read_u16(self.bytes, pos + 34).unwrap_or(u16::MAX);
            if compressed == u32::MAX as u64
                || uncompressed == u32::MAX as u64
                || local_offset == u32::MAX as u64
                || disk_start == u16::MAX
            {
                if let Some(z64) = zip64_extra(extra) {
                    let mut cursor = 0;
                    if uncompressed == u32::MAX as u64 {
                        uncompressed = take_u64(z64, &mut cursor).unwrap_or(u64::MAX);
                    }
                    if compressed == u32::MAX as u64 {
                        compressed = take_u64(z64, &mut cursor).unwrap_or(u64::MAX);
                    }
                    if local_offset == u32::MAX as u64 {
                        local_offset = take_u64(z64, &mut cursor).unwrap_or(u64::MAX);
                    }
                    if disk_start == u16::MAX {
                        disk_start = take_u32(z64, &mut cursor)
                            .and_then(|n| u16::try_from(n).ok())
                            .unwrap_or(u16::MAX);
                    }
                }
            }
            let (name, name_error) = if name_len > self.limits.max_name_bytes {
                (String::new(), Some(EntryError::NameTooLong))
            } else {
                decode_name(raw_name, flags)
            };
            let mut error = name_error;
            if bad_signature {
                error.get_or_insert(EntryError::InvalidHeader);
            }
            if name_len > self.limits.max_name_bytes {
                error.get_or_insert(EntryError::NameTooLong);
            }
            if flags & 1 != 0 {
                error.get_or_insert(EntryError::UnsupportedFeature("encryption"));
            }
            if flags & !(UTF8_FLAG | DESCRIPTOR_FLAG | 0x0006) != 0 {
                error.get_or_insert(EntryError::UnsupportedFeature("general-purpose flags"));
            }
            if !matches!(method, 0 | 8) {
                error.get_or_insert(EntryError::UnsupportedMethod(method));
            }
            if !is_safe_name(&name) {
                error.get_or_insert(EntryError::UnsafeName);
            }
            if disk_start != 0 {
                error.get_or_insert(EntryError::UnsupportedFeature("multi-disk entry"));
            }
            if local_offset > self.bytes.len() as u64 {
                error.get_or_insert(EntryError::Truncated);
            }
            if let Some(limit) = entry_size_limit(uncompressed, compressed, self.limits) {
                error.get_or_insert(limit);
            }
            self.entries.push(Entry {
                name,
                method,
                crc32: crc,
                compressed_size: compressed,
                uncompressed_size: uncompressed,
                local_header_offset: local_offset,
                is_name_safe: false,
                is_duplicate: false,
                error,
                archive: self.bytes,
                limits: self.limits,
                flags,
            });
            if let Some(entry) = self.entries.last_mut() {
                entry.is_name_safe = is_safe_name(&entry.name);
            }
            pos = record_end;
        }
        Ok(())
    }

    fn recover_locals(&mut self) -> Result<(), ZipError> {
        let mut pos = 0usize;
        while let Some(end_fixed) = pos.checked_add(30).filter(|&x| x <= self.bytes.len()) {
            if read_u32(self.bytes, pos) != Some(LOCAL) {
                break;
            }
            let flags = read_u16(self.bytes, pos + 6).unwrap_or(0);
            let method = read_u16(self.bytes, pos + 8).unwrap_or(u16::MAX);
            let crc = read_u32(self.bytes, pos + 14).unwrap_or(0);
            let comp = read_u32(self.bytes, pos + 18).unwrap_or(0) as u64;
            let uncomp = read_u32(self.bytes, pos + 22).unwrap_or(0) as u64;
            let nl = read_u16(self.bytes, pos + 26).unwrap_or(0) as usize;
            let el = read_u16(self.bytes, pos + 28).unwrap_or(0) as usize;
            let data = end_fixed
                .checked_add(nl)
                .and_then(|x| x.checked_add(el))
                .filter(|&x| x <= self.bytes.len());
            let Some(data) = data else { break };
            // A descriptor with no trustworthy central sizes cannot be safely skipped.
            if flags & DESCRIPTOR_FLAG != 0 || comp == u32::MAX as u64 || uncomp == u32::MAX as u64
            {
                break;
            }
            let data_end = data
                .checked_add(usize::try_from(comp).ok().ok_or(ZipError::SizeOverflow)?)
                .filter(|&x| x <= self.bytes.len());
            let Some(data_end) = data_end else { break };
            let raw = &self.bytes[end_fixed..end_fixed + nl];
            if self.entries.len() >= self.limits.max_entries {
                return Err(ZipError::LimitExceeded {
                    limit: "entry count",
                });
            }
            let (name, mut error) = if nl > self.limits.max_name_bytes {
                (String::new(), Some(EntryError::NameTooLong))
            } else {
                decode_name(raw, flags)
            };
            if nl > self.limits.max_name_bytes {
                error.get_or_insert(EntryError::NameTooLong);
            }
            if flags & 1 != 0 {
                error.get_or_insert(EntryError::UnsupportedFeature("encryption"));
            }
            if flags & !(UTF8_FLAG | DESCRIPTOR_FLAG | 0x0006) != 0 {
                error.get_or_insert(EntryError::UnsupportedFeature("general-purpose flags"));
            }
            if !matches!(method, 0 | 8) {
                error.get_or_insert(EntryError::UnsupportedMethod(method));
            }
            if !is_safe_name(&name) {
                error.get_or_insert(EntryError::UnsafeName);
            }
            if let Some(limit) = entry_size_limit(uncomp, comp, self.limits) {
                error.get_or_insert(limit);
            }
            let mut entry = Entry {
                name,
                method,
                crc32: crc,
                compressed_size: comp,
                uncompressed_size: uncomp,
                local_header_offset: pos as u64,
                is_name_safe: false,
                is_duplicate: false,
                error,
                archive: self.bytes,
                limits: self.limits,
                flags,
            };
            entry.is_name_safe = is_safe_name(&entry.name);
            self.entries.push(entry);
            pos = data_end;
        }
        Ok(())
    }

    fn mark_duplicates(&mut self) {
        let mut all_names = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        for entry in &mut self.entries {
            if !entry.is_name_safe {
                entry.error.get_or_insert(EntryError::UnsafeName);
                continue;
            }
            let key = canonical_name(&entry.name);
            entry.is_duplicate = !all_names.insert(key.clone());
            if entry.error.is_some() {
                continue;
            }
            if !names.insert(key) {
                entry.error.get_or_insert(EntryError::DuplicateName);
            }
        }
    }

    fn apply_cumulative_limits(&mut self) {
        let mut total = 0u64;
        for entry in &mut self.entries {
            if entry.error.is_some() {
                continue;
            }
            if let Some(error) = cumulative_limit(entry.uncompressed_size, total, self.limits) {
                entry.error = Some(error);
            } else {
                // cumulative_limit checked this addition, so it cannot overflow.
                total += entry.uncompressed_size;
            }
        }
    }
}

fn read_entry(bytes: &[u8], entry: &Entry<'_>, limits: Limits) -> Result<Vec<u8>, EntryError> {
    if let Some(error) = entry_size_limit(entry.uncompressed_size, entry.compressed_size, limits) {
        return Err(error);
    }
    let offset = usize::try_from(entry.local_header_offset).map_err(|_| EntryError::Truncated)?;
    let fixed_end = offset
        .checked_add(30)
        .filter(|&x| x <= bytes.len())
        .ok_or(EntryError::Truncated)?;
    if read_u32(bytes, offset) != Some(LOCAL) {
        return Err(EntryError::InvalidHeader);
    }
    let flags = read_u16(bytes, offset + 6).ok_or(EntryError::Truncated)?;
    let method = read_u16(bytes, offset + 8).ok_or(EntryError::Truncated)?;
    let local_crc = read_u32(bytes, offset + 14).ok_or(EntryError::Truncated)?;
    let local_comp = read_u32(bytes, offset + 18).ok_or(EntryError::Truncated)? as u64;
    let local_uncomp = read_u32(bytes, offset + 22).ok_or(EntryError::Truncated)? as u64;
    let nl = read_u16(bytes, offset + 26).ok_or(EntryError::Truncated)? as usize;
    let el = read_u16(bytes, offset + 28).ok_or(EntryError::Truncated)? as usize;
    if nl > limits.max_name_bytes {
        return Err(EntryError::NameTooLong);
    }
    if flags & 1 != 0 {
        return Err(EntryError::UnsupportedFeature("encryption"));
    }
    if flags != entry.flags {
        return Err(EntryError::InvalidHeader);
    }
    if method != entry.method {
        return Err(EntryError::InvalidHeader);
    }
    if flags & DESCRIPTOR_FLAG == 0 && local_crc != entry.crc32 {
        return Err(EntryError::InvalidHeader);
    }
    let data_start = fixed_end
        .checked_add(nl)
        .and_then(|x| x.checked_add(el))
        .filter(|&x| x <= bytes.len())
        .ok_or(EntryError::Truncated)?;
    let (local_name, name_error) = decode_name(&bytes[fixed_end..fixed_end + nl], flags);
    if name_error.is_some() || local_name != entry.name {
        return Err(EntryError::InvalidHeader);
    }
    if flags & DESCRIPTOR_FLAG == 0 {
        let (actual_comp, actual_uncomp) =
            if local_comp == u32::MAX as u64 || local_uncomp == u32::MAX as u64 {
                let extra = &bytes[fixed_end + nl..data_start];
                let z64 = zip64_extra(extra).ok_or(EntryError::InvalidHeader)?;
                let mut cursor = 0;
                let uncomp = if local_uncomp == u32::MAX as u64 {
                    take_u64(z64, &mut cursor).ok_or(EntryError::InvalidHeader)?
                } else {
                    local_uncomp
                };
                let comp = if local_comp == u32::MAX as u64 {
                    take_u64(z64, &mut cursor).ok_or(EntryError::InvalidHeader)?
                } else {
                    local_comp
                };
                (comp, uncomp)
            } else {
                (local_comp, local_uncomp)
            };
        if actual_comp != entry.compressed_size || actual_uncomp != entry.uncompressed_size {
            return Err(EntryError::InvalidHeader);
        }
    }
    let comp_len =
        usize::try_from(entry.compressed_size).map_err(|_| EntryError::LimitExceeded {
            limit: "compressed size",
        })?;
    let data_end = data_start
        .checked_add(comp_len)
        .filter(|&x| x <= bytes.len())
        .ok_or(EntryError::Truncated)?;
    let compressed = &bytes[data_start..data_end];
    let expected =
        usize::try_from(entry.uncompressed_size).map_err(|_| EntryError::LimitExceeded {
            limit: "entry size",
        })?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(expected)
        .map_err(|_| EntryError::AllocationFailed)?;
    match entry.method {
        0 => {
            if compressed.len() != expected {
                return Err(EntryError::SizeMismatch);
            }
            output.extend_from_slice(compressed);
        }
        8 => {
            let mut state = InflateState::new(Format::Deflate, entry.uncompressed_size);
            let mut input_at = 0usize;
            let mut scratch = [0u8; 8192];
            loop {
                let input = &compressed[input_at..];
                let progress = state
                    .push(input, &mut scratch, true)
                    .map_err(|_| EntryError::InvalidDeflate)?;
                input_at = input_at
                    .checked_add(progress.consumed)
                    .ok_or(EntryError::SizeMismatch)?;
                let new_len = output
                    .len()
                    .checked_add(progress.produced)
                    .ok_or(EntryError::SizeMismatch)?;
                if new_len > expected {
                    return Err(EntryError::SizeMismatch);
                }
                output.extend_from_slice(&scratch[..progress.produced]);
                match progress.status {
                    Status::Done => {
                        if input_at != compressed.len() {
                            return Err(EntryError::SizeMismatch);
                        }
                        break;
                    }
                    Status::NeedOutput => {
                        if progress.consumed == 0 && progress.produced == 0 {
                            return Err(EntryError::InvalidDeflate);
                        }
                    }
                    Status::NeedInput => return Err(EntryError::Truncated),
                }
            }
        }
        method => return Err(EntryError::UnsupportedMethod(method)),
    }
    if output.len() != expected {
        return Err(EntryError::SizeMismatch);
    }
    if compression::crc32(&output) != entry.crc32 {
        return Err(EntryError::CrcMismatch);
    }
    if flags & DESCRIPTOR_FLAG != 0 {
        validate_descriptor(bytes, data_end, entry)?;
    }
    Ok(output)
}

fn validate_descriptor(bytes: &[u8], pos: usize, entry: &Entry<'_>) -> Result<(), EntryError> {
    // A ZIP64 descriptor contains ordinary u64 sizes, never u32 sentinels.
    // Try the unsigned form too: the CRC itself may equal the signature.
    for p in [
        Some(pos),
        pos.checked_add(4)
            .filter(|_| read_u32(bytes, pos) == Some(DESCRIPTOR)),
    ]
    .into_iter()
    .flatten()
    {
        if read_u32(bytes, p) != Some(entry.crc32) {
            continue;
        }
        let Some(sizes) = p.checked_add(4) else {
            continue;
        };
        let short = entry.compressed_size < u32::MAX as u64
            && entry.uncompressed_size < u32::MAX as u64
            && read_u32(bytes, sizes).map(u64::from) == Some(entry.compressed_size)
            && sizes
                .checked_add(4)
                .and_then(|p| read_u32(bytes, p))
                .map(u64::from)
                == Some(entry.uncompressed_size);
        let wide = read_u64(bytes, sizes) == Some(entry.compressed_size)
            && sizes.checked_add(8).and_then(|p| read_u64(bytes, p))
                == Some(entry.uncompressed_size);
        if short || wide {
            return Ok(());
        }
    }
    Err(EntryError::InvalidDescriptor)
}

/// ZIP compression method accepted by the deterministic writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    Store,
    Deflate,
}

/// Writer configuration. DOS date/time zero is used unless `timestamp` is set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteOptions {
    pub timestamp: Option<(u16, u16)>,
    pub force_zip64: bool,
    pub compression_level: Level,
}
impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            timestamp: None,
            force_zip64: false,
            compression_level: Level::Normal,
        }
    }
}

struct Pending {
    name: String,
    raw_name: Vec<u8>,
    uncompressed_len: usize,
    compressed: Vec<u8>,
    method: Method,
    crc: u32,
}

/// Deterministic ZIP writer preserving insertion order.
pub struct Writer {
    options: WriteOptions,
    entries: Vec<Pending>,
}
impl Writer {
    pub fn new(options: WriteOptions) -> Self {
        Self {
            options,
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, name: &str, bytes: &[u8], method: Method) -> Result<(), ZipError> {
        if !is_safe_name(name)
            || name.len() > u16::MAX as usize
            || !self
                .entries
                .iter()
                .all(|e| canonical_name(&e.name) != canonical_name(name))
        {
            return Err(ZipError::InvalidName);
        }
        // Prefer CP437 whenever it round-trips; otherwise preserve UTF-8 bytes.
        let raw_name = match unicode::encode(name, Encoding::Cp437) {
            Ok(encoded) if unicode::decode(&encoded, Encoding::Cp437) == name => encoded,
            _ => name.as_bytes().to_vec(),
        };
        let compressed = match method {
            Method::Store => bytes.to_vec(),
            Method::Deflate => {
                let capacity = bytes
                    .len()
                    .checked_add(bytes.len() / 8)
                    .and_then(|x| x.checked_add(128))
                    .ok_or(ZipError::SizeOverflow)?;
                let mut out = vec![0; capacity];
                let len =
                    match compression::compress(bytes, self.options.compression_level, &mut out) {
                        Ok(len) => len,
                        Err(compression::CompressError::OutputTooSmall { needed }) => {
                            out.resize(needed, 0);
                            compression::compress(bytes, self.options.compression_level, &mut out)
                                .map_err(|_| ZipError::Compression("output buffer"))?
                        }
                    };
                out.truncate(len);
                out
            }
        };
        self.entries.push(Pending {
            name: name.to_owned(),
            raw_name,
            uncompressed_len: bytes.len(),
            compressed,
            method,
            crc: compression::crc32(bytes),
        });
        Ok(())
    }

    pub fn finish(self) -> Result<Vec<u8>, ZipError> {
        let (dos_time, dos_date) = self.options.timestamp.unwrap_or((0, (1 << 5) | 1));
        let force = self.options.force_zip64;
        let mut out = Vec::new();
        let mut central = Vec::new();
        for entry in self.entries {
            let offset = u64::try_from(out.len()).map_err(|_| ZipError::SizeOverflow)?;
            let comp = entry.compressed.len() as u64;
            let uncomp = entry.uncompressed_len as u64;
            let zip64 = entry_needs_zip64(force, comp, uncomp, offset);
            let flags = if std::str::from_utf8(&entry.raw_name).is_ok()
                && entry.raw_name == entry.name.as_bytes()
            {
                UTF8_FLAG
            } else {
                0
            };
            let method = if entry.method == Method::Store { 0 } else { 8 };
            let local_extra = if zip64 {
                zip64_sizes(uncomp, comp)
            } else {
                Vec::new()
            };
            put_u32(&mut out, LOCAL);
            put_u16(&mut out, 45);
            put_u16(&mut out, flags);
            put_u16(&mut out, method);
            put_u16(&mut out, dos_time);
            put_u16(&mut out, dos_date);
            put_u32(&mut out, entry.crc);
            put_u32(&mut out, if zip64 { u32::MAX } else { comp as u32 });
            put_u32(&mut out, if zip64 { u32::MAX } else { uncomp as u32 });
            put_u16(&mut out, entry.raw_name.len() as u16);
            put_u16(&mut out, local_extra.len() as u16);
            out.extend_from_slice(&entry.raw_name);
            out.extend_from_slice(&local_extra);
            out.extend_from_slice(&entry.compressed);
            let central_extra = if zip64 {
                zip64_sizes_and_offset(uncomp, comp, offset)
            } else {
                Vec::new()
            };
            put_u32(&mut central, CENTRAL);
            put_u16(&mut central, 45);
            put_u16(&mut central, 45);
            put_u16(&mut central, flags);
            put_u16(&mut central, method);
            put_u16(&mut central, dos_time);
            put_u16(&mut central, dos_date);
            put_u32(&mut central, entry.crc);
            put_u32(&mut central, if zip64 { u32::MAX } else { comp as u32 });
            put_u32(&mut central, if zip64 { u32::MAX } else { uncomp as u32 });
            put_u16(&mut central, entry.raw_name.len() as u16);
            put_u16(&mut central, central_extra.len() as u16);
            put_u16(&mut central, 0);
            put_u16(&mut central, 0);
            put_u16(&mut central, 0);
            put_u32(&mut central, 0);
            put_u32(&mut central, if zip64 { u32::MAX } else { offset as u32 });
            central.extend_from_slice(&entry.raw_name);
            central.extend_from_slice(&central_extra);
        }
        let cd_offset = out.len() as u64;
        out.extend_from_slice(&central);
        let cd_size = central.len() as u64;
        let count = central_entry_count(&central)?;
        let use64 = directory_needs_zip64(
            force,
            count,
            cd_size,
            cd_offset,
            central_has_zip64(&central),
        );
        if use64 {
            let z64_offset = out.len() as u64;
            put_u32(&mut out, ZIP64_EOCD);
            put_u64(&mut out, 44);
            put_u16(&mut out, 45);
            put_u16(&mut out, 45);
            put_u32(&mut out, 0);
            put_u32(&mut out, 0);
            put_u64(&mut out, count);
            put_u64(&mut out, count);
            put_u64(&mut out, cd_size);
            put_u64(&mut out, cd_offset);
            put_u32(&mut out, ZIP64_LOCATOR);
            put_u32(&mut out, 0);
            put_u64(&mut out, z64_offset);
            put_u32(&mut out, 1);
        }
        put_u32(&mut out, EOCD);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u16(&mut out, if use64 { u16::MAX } else { count as u16 });
        put_u16(&mut out, if use64 { u16::MAX } else { count as u16 });
        put_u32(&mut out, if use64 { u32::MAX } else { cd_size as u32 });
        put_u32(&mut out, if use64 { u32::MAX } else { cd_offset as u32 });
        put_u16(&mut out, 0);
        Ok(out)
    }
}

fn find_directory(bytes: &[u8]) -> Result<Option<(u64, u64, u64, u64)>, ZipError> {
    let start = bytes.len().saturating_sub(65_557);
    let mut eocd_pos = None;
    if bytes.len() >= 22 {
        for p in (start..=bytes.len() - 22).rev() {
            if read_u32(bytes, p) == Some(EOCD) {
                let comment = read_u16(bytes, p + 20).unwrap_or(u16::MAX) as usize;
                if p.checked_add(22 + comment) == Some(bytes.len()) {
                    eocd_pos = Some(p);
                    break;
                }
            }
        }
    }
    let Some(p) = eocd_pos else { return Ok(None) };
    if read_u16(bytes, p + 4) != Some(0)
        || read_u16(bytes, p + 6) != Some(0)
        || read_u16(bytes, p + 8) != read_u16(bytes, p + 10)
    {
        return Err(ZipError::Unsupported("multi-disk archive"));
    }
    let count16 = read_u16(bytes, p + 10).unwrap_or(0);
    let size32 = read_u32(bytes, p + 12).unwrap_or(0);
    let offset32 = read_u32(bytes, p + 16).unwrap_or(0);
    if count16 == u16::MAX || size32 == u32::MAX || offset32 == u32::MAX {
        if p < 20 || read_u32(bytes, p - 20) != Some(ZIP64_LOCATOR) {
            return Ok(None);
        }
        let zoff = read_u64(bytes, p - 12).ok_or(ZipError::InvalidArchive("ZIP64 locator"))?;
        let z = usize::try_from(zoff).map_err(|_| ZipError::InvalidArchive("ZIP64 offset"))?;
        if z.checked_add(56).is_none_or(|end| end > p - 20) {
            return Err(ZipError::InvalidArchive("ZIP64 end record range"));
        }
        if read_u32(bytes, p - 16) != Some(0)
            || read_u32(bytes, p - 4) != Some(1)
            || read_u32(bytes, z + 16) != Some(0)
            || read_u32(bytes, z + 20) != Some(0)
            || read_u64(bytes, z + 24) != read_u64(bytes, z + 32)
        {
            return Err(ZipError::Unsupported("multi-disk archive"));
        }
        let record_len =
            read_u64(bytes, z + 4).ok_or(ZipError::InvalidArchive("ZIP64 record length"))?;
        if record_len < 44
            || zoff.checked_add(12).and_then(|n| n.checked_add(record_len)) != Some((p - 20) as u64)
        {
            return Err(ZipError::InvalidArchive("ZIP64 record length"));
        }
        if read_u32(bytes, z) != Some(ZIP64_EOCD) {
            return Ok(None);
        }
        let count = read_u64(bytes, z + 32).ok_or(ZipError::InvalidArchive("ZIP64 end record"))?;
        let size = read_u64(bytes, z + 40).ok_or(ZipError::InvalidArchive("ZIP64 end record"))?;
        let offset = read_u64(bytes, z + 48).ok_or(ZipError::InvalidArchive("ZIP64 end record"))?;
        Ok(Some((offset, size, count, zoff)))
    } else {
        Ok(Some((
            offset32 as u64,
            size32 as u64,
            count16 as u64,
            p as u64,
        )))
    }
}

fn decode_name(raw: &[u8], flags: u16) -> (String, Option<EntryError>) {
    if flags & UTF8_FLAG != 0 {
        match core::str::from_utf8(raw) {
            Ok(name) => (name.to_owned(), None),
            Err(_) => (String::new(), Some(EntryError::InvalidUtf8Name)),
        }
    } else {
        (unicode::decode(raw, Encoding::Cp437), None)
    }
}
fn is_safe_name(name: &str) -> bool {
    if name.is_empty()
        || name.starts_with('/')
        || name.starts_with('\\')
        || name.contains("//")
        || name.contains(':')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        return false;
    }
    name.trim_end_matches('/')
        .split('/')
        .all(|part| part != "." && part != ".." && safe_windows_component(part))
}
fn canonical_name(name: &str) -> String {
    name.replace('\\', "/")
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect::<Vec<_>>()
        .join("/")
}
fn entry_size_limit(uncompressed: u64, compressed: u64, limits: Limits) -> Option<EntryError> {
    if uncompressed > limits.max_entry_size {
        return Some(EntryError::LimitExceeded {
            limit: "entry output size",
        });
    }
    if compressed == 0 {
        if uncompressed != 0 {
            return Some(EntryError::LimitExceeded {
                limit: "compression ratio",
            });
        }
    } else if (uncompressed as u128) > (compressed as u128) * (limits.max_compression_ratio as u128)
    {
        return Some(EntryError::LimitExceeded {
            limit: "compression ratio",
        });
    }
    None
}
fn cumulative_limit(uncompressed: u64, total: u64, limits: Limits) -> Option<EntryError> {
    if total
        .checked_add(uncompressed)
        .is_some_and(|n| n <= limits.max_total_size)
    {
        None
    } else {
        Some(EntryError::LimitExceeded {
            limit: "total output size",
        })
    }
}
fn entry_needs_zip64(force: bool, compressed: u64, uncompressed: u64, offset: u64) -> bool {
    force
        || compressed >= u32::MAX as u64
        || uncompressed >= u32::MAX as u64
        || offset >= u32::MAX as u64
}
fn directory_needs_zip64(
    force: bool,
    count: u64,
    size: u64,
    offset: u64,
    entries_need_zip64: bool,
) -> bool {
    force
        || count >= u16::MAX as u64
        || size >= u32::MAX as u64
        || offset >= u32::MAX as u64
        || entries_need_zip64
}
fn safe_windows_component(component: &str) -> bool {
    if component.is_empty()
        || component.ends_with('.')
        || component.ends_with(' ')
        || component
            .chars()
            .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control())
    {
        return false;
    }
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) || matches!(
        stem.as_str(),
        "COM¹" | "COM²" | "COM³" | "LPT¹" | "LPT²" | "LPT³"
    );
    !reserved
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}
fn zip64_extra(mut extra: &[u8]) -> Option<&[u8]> {
    while extra.len() >= 4 {
        let id = u16::from_le_bytes([extra[0], extra[1]]);
        let n = u16::from_le_bytes([extra[2], extra[3]]) as usize;
        if n > extra.len() - 4 {
            return None;
        }
        if id == ZIP64_EXTRA {
            return Some(&extra[4..4 + n]);
        }
        extra = &extra[4 + n..];
    }
    None
}
fn take_u64(b: &[u8], p: &mut usize) -> Option<u64> {
    let v = read_u64(b, *p)?;
    *p = p.checked_add(8)?;
    Some(v)
}
fn read_u16(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(p..p.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn read_u32(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(p..p.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn take_u32(b: &[u8], p: &mut usize) -> Option<u32> {
    let v = read_u32(b, *p)?;
    *p = p.checked_add(4)?;
    Some(v)
}
fn read_u64(b: &[u8], p: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        b.get(p..p.checked_add(8)?)?.try_into().ok()?,
    ))
}
fn put_u16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes())
}
fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes())
}
fn put_u64(b: &mut Vec<u8>, v: u64) {
    b.extend_from_slice(&v.to_le_bytes())
}
fn zip64_sizes(uncomp: u64, comp: u64) -> Vec<u8> {
    let mut x = Vec::new();
    put_u16(&mut x, ZIP64_EXTRA);
    put_u16(&mut x, 16);
    put_u64(&mut x, uncomp);
    put_u64(&mut x, comp);
    x
}
fn zip64_sizes_and_offset(uncomp: u64, comp: u64, offset: u64) -> Vec<u8> {
    let mut x = Vec::new();
    put_u16(&mut x, ZIP64_EXTRA);
    put_u16(&mut x, 24);
    put_u64(&mut x, uncomp);
    put_u64(&mut x, comp);
    put_u64(&mut x, offset);
    x
}
fn central_entry_count(b: &[u8]) -> Result<u64, ZipError> {
    let mut p = 0;
    let mut n = 0u64;
    while p < b.len() {
        let name = read_u16(b, p + 28).ok_or(ZipError::InvalidArchive("central record"))? as usize;
        let extra = read_u16(b, p + 30).ok_or(ZipError::InvalidArchive("central record"))? as usize;
        let comment =
            read_u16(b, p + 32).ok_or(ZipError::InvalidArchive("central record"))? as usize;
        p = p
            .checked_add(46 + name + extra + comment)
            .ok_or(ZipError::SizeOverflow)?;
        n += 1;
    }
    Ok(n)
}
fn central_has_zip64(b: &[u8]) -> bool {
    let mut p = 0;
    while p + 46 <= b.len() {
        if read_u32(b, p) != Some(CENTRAL) {
            return false;
        }
        let name = read_u16(b, p + 28).unwrap_or(0) as usize;
        let extra = read_u16(b, p + 30).unwrap_or(0) as usize;
        if zip64_extra(b.get(p + 46 + name..p + 46 + name + extra).unwrap_or(&[])).is_some() {
            return true;
        }
        p += 46 + name + extra + read_u16(b, p + 32).unwrap_or(0) as usize;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn fixture(name: &str) -> Vec<u8> {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("../../tests/format-reference/zip");
        p.push(name);
        std::fs::read(p).unwrap()
    }
    fn read_fixture(name: &str, member: &str) -> Result<Vec<u8>, EntryError> {
        let data = fixture(name);
        let zip = Archive::open(&data, Limits::default()).unwrap();
        zip.entry(member).unwrap().read_all()
    }

    #[test]
    fn reads_reference_archive_and_encodings() {
        let data = fixture("valid-mixed.zip");
        let a = Archive::open(&data, Limits::default()).unwrap();
        assert!(a.is_complete);
        assert_eq!(a.entries().len(), 4);
        assert_eq!(a.entries()[0].read_all().unwrap(), b"plain payload");
        assert!(
            String::from_utf8(a.entries()[1].read_all().unwrap())
                .unwrap()
                .starts_with("alpha")
        );
        assert_eq!(a.entries()[3].name, "été.txt");
        assert_eq!(
            read_fixture("valid-cp437.zip", "café.txt").unwrap(),
            b"CP437 name"
        );
        assert_eq!(
            read_fixture("valid-descriptor-signed.zip", "desc.txt").unwrap(),
            b"descriptor payload"
        );
        assert_eq!(
            read_fixture("valid-descriptor-unsigned.zip", "desc.txt").unwrap(),
            b"descriptor payload"
        );
        assert_eq!(
            read_fixture("valid-force-zip64.zip", "small.txt").unwrap(),
            b"zip64 small"
        );
    }
    #[test]
    fn keeps_errors_local_and_flags_unsafe_names() {
        let bad = fixture("bad-crc.zip");
        let a = Archive::open(&bad, Limits::default()).unwrap();
        assert_eq!(
            a.entry("bad.txt").unwrap().read_all(),
            Err(EntryError::CrcMismatch)
        );
        let unsafe_zip = fixture("unsafe-traversal.zip");
        assert_eq!(
            Archive::open(&unsafe_zip, Limits::default()).unwrap_err(),
            ZipError::NoUsableEntries
        );
        let mut writer = Writer::new(WriteOptions::default());
        writer.add("healthy", b"ok", Method::Store).unwrap();
        writer.add("unsafe", b"x", Method::Store).unwrap();
        let mut mixed = writer.finish().unwrap();
        let eocd = mixed.len() - 22;
        let central = read_u32(&mixed, eocd + 16).unwrap() as usize;
        let second = central + 46 + read_u16(&mixed, central + 28).unwrap() as usize;
        mixed[second + 46..second + 51].copy_from_slice(b"../x!");
        let a = Archive::open(&mixed, Limits::default()).unwrap();
        assert_eq!(a.entries()[0].read_all().unwrap(), b"ok");
        assert!(!a.entries()[1].is_name_safe);
        assert_eq!(a.entries()[1].error, Some(EntryError::UnsafeName));
    }
    #[test]
    fn applies_limits_before_read_all_and_detects_partial_archive() {
        let data = fixture("limit-advertised-size.zip");
        let limits = Limits {
            max_entry_size: 16,
            ..Limits::default()
        };
        assert_eq!(
            Archive::open(&data, limits).unwrap_err(),
            ZipError::NoUsableEntries
        );
        let truncated = fixture("truncated-payload.zip");
        let a = Archive::open(&truncated, Limits::default()).unwrap();
        assert!(!a.is_complete);
    }
    #[test]
    fn deterministic_writer_round_trips_and_can_force_zip64() {
        for force in [false, true] {
            let mut w = Writer::new(WriteOptions {
                force_zip64: force,
                ..WriteOptions::default()
            });
            w.add("a.txt", b"hello hello hello", Method::Deflate)
                .unwrap();
            w.add("été.txt", b"x", Method::Store).unwrap();
            let bytes = w.finish().unwrap();
            let a = Archive::open(&bytes, Limits::default()).unwrap();
            assert!(a.is_complete);
            assert_eq!(a.entries()[0].name, "a.txt");
            assert_eq!(a.entries()[0].read_all().unwrap(), b"hello hello hello");
            assert_eq!(a.entries()[1].name, "été.txt");
            assert_eq!(a.entries()[1].read_all().unwrap(), b"x");
        }
        let make = || {
            let mut w = Writer::new(WriteOptions::default());
            w.add("x", b"y", Method::Store).unwrap();
            w.finish().unwrap()
        };
        assert_eq!(make(), make());
    }
    #[test]
    fn refuses_unsafe_writer_names() {
        let mut w = Writer::new(WriteOptions::default());
        assert_eq!(
            w.add("../x", b"x", Method::Store),
            Err(ZipError::InvalidName)
        );
    }

    #[test]
    fn empty_archive_is_complete_and_directory_names_are_safe() {
        let bytes = Writer::new(WriteOptions::default()).finish().unwrap();
        let archive = Archive::open(&bytes, Limits::default()).unwrap();
        assert!(archive.is_complete);
        assert!(archive.entries().is_empty());
        let mut writer = Writer::new(WriteOptions::default());
        writer.add("folder/", b"", Method::Store).unwrap();
        assert_eq!(
            writer.add("nul\0name", b"", Method::Store),
            Err(ZipError::InvalidName)
        );
        let bytes = writer.finish().unwrap();
        assert_eq!(
            Archive::open(&bytes, Limits::default()).unwrap().entries()[0]
                .read_all()
                .unwrap(),
            b""
        );
    }

    #[test]
    fn zip64_descriptors_have_u64_sizes_without_sentinels() {
        let bytes = fixture("valid-mixed.zip");
        let archive = Archive::open(&bytes, Limits::default()).unwrap();
        let entry = &archive.entries()[0];
        for signed in [false, true] {
            let mut descriptor = Vec::new();
            if signed {
                put_u32(&mut descriptor, DESCRIPTOR);
            }
            put_u32(&mut descriptor, entry.crc32);
            put_u64(&mut descriptor, entry.compressed_size);
            put_u64(&mut descriptor, entry.uncompressed_size);
            assert_eq!(validate_descriptor(&descriptor, 0, entry), Ok(()));
            let last = descriptor.len() - 1;
            descriptor[last] ^= 1;
            assert_eq!(
                validate_descriptor(&descriptor, 0, entry),
                Err(EntryError::InvalidDescriptor)
            );
        }
    }

    fn zip64_descriptor_archive(signed: bool) -> Vec<u8> {
        let name = b"wide.txt";
        let data = b"ZIP64 data descriptor";
        let crc = compression::crc32(data);
        let flags = UTF8_FLAG | DESCRIPTOR_FLAG;
        let mut bytes = Vec::new();
        put_u32(&mut bytes, LOCAL);
        put_u16(&mut bytes, 45);
        put_u16(&mut bytes, flags);
        put_u16(&mut bytes, 0);
        put_u16(&mut bytes, 0);
        put_u16(&mut bytes, 33);
        put_u32(&mut bytes, 0);
        put_u32(&mut bytes, 0);
        put_u32(&mut bytes, 0);
        put_u16(&mut bytes, name.len() as u16);
        put_u16(&mut bytes, 0);
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(data);
        if signed {
            put_u32(&mut bytes, DESCRIPTOR);
        }
        put_u32(&mut bytes, crc);
        put_u64(&mut bytes, data.len() as u64);
        put_u64(&mut bytes, data.len() as u64);
        let cd_offset = bytes.len() as u64;
        let mut extra = Vec::new();
        put_u16(&mut extra, ZIP64_EXTRA);
        put_u16(&mut extra, 16);
        put_u64(&mut extra, data.len() as u64);
        put_u64(&mut extra, data.len() as u64);
        put_u32(&mut bytes, CENTRAL);
        put_u16(&mut bytes, 45);
        put_u16(&mut bytes, 45);
        put_u16(&mut bytes, flags);
        put_u16(&mut bytes, 0);
        put_u16(&mut bytes, 0);
        put_u16(&mut bytes, 33);
        put_u32(&mut bytes, crc);
        put_u32(&mut bytes, u32::MAX);
        put_u32(&mut bytes, u32::MAX);
        put_u16(&mut bytes, name.len() as u16);
        put_u16(&mut bytes, extra.len() as u16);
        put_u16(&mut bytes, 0);
        put_u16(&mut bytes, 0);
        put_u16(&mut bytes, 0);
        put_u32(&mut bytes, 0);
        put_u32(&mut bytes, 0);
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&extra);
        let cd_size = bytes.len() as u64 - cd_offset;
        let z64_offset = bytes.len() as u64;
        put_u32(&mut bytes, ZIP64_EOCD);
        put_u64(&mut bytes, 44);
        put_u16(&mut bytes, 45);
        put_u16(&mut bytes, 45);
        put_u32(&mut bytes, 0);
        put_u32(&mut bytes, 0);
        put_u64(&mut bytes, 1);
        put_u64(&mut bytes, 1);
        put_u64(&mut bytes, cd_size);
        put_u64(&mut bytes, cd_offset);
        put_u32(&mut bytes, ZIP64_LOCATOR);
        put_u32(&mut bytes, 0);
        put_u64(&mut bytes, z64_offset);
        put_u32(&mut bytes, 1);
        put_u32(&mut bytes, EOCD);
        put_u16(&mut bytes, 0);
        put_u16(&mut bytes, 0);
        put_u16(&mut bytes, u16::MAX);
        put_u16(&mut bytes, u16::MAX);
        put_u32(&mut bytes, u32::MAX);
        put_u32(&mut bytes, u32::MAX);
        put_u16(&mut bytes, 0);
        bytes
    }

    #[test]
    fn complete_zip64_signed_and_unsigned_descriptors_read() {
        for signed in [false, true] {
            let bytes = zip64_descriptor_archive(signed);
            let archive = Archive::open(&bytes, Limits::default()).unwrap();
            assert!(archive.is_complete);
            assert_eq!(
                archive.entries()[0].read_all().unwrap(),
                b"ZIP64 data descriptor"
            );
        }
    }

    #[test]
    fn damaged_middle_entry_does_not_hide_healthy_neighbors() {
        let mut writer = Writer::new(WriteOptions::default());
        for (name, data) in [
            ("one", b"first".as_slice()),
            ("two", b"middle".as_slice()),
            ("three", b"last".as_slice()),
        ] {
            writer.add(name, data, Method::Store).unwrap();
        }
        let mut bytes = writer.finish().unwrap();
        let indexed = Archive::open(&bytes, Limits::default()).unwrap();
        let middle = &indexed.entries()[1];
        let local = middle.local_header_offset as usize;
        let name_len = read_u16(&bytes, local + 26).unwrap() as usize;
        let payload = local + 30 + name_len;
        bytes[payload] ^= 0x20;
        let archive = Archive::open(&bytes, Limits::default()).unwrap();
        assert_eq!(archive.entries()[0].read_all().unwrap(), b"first");
        assert_eq!(
            archive.entries()[1].read_all(),
            Err(EntryError::CrcMismatch)
        );
        assert_eq!(archive.entries()[2].read_all().unwrap(), b"last");
    }

    #[test]
    fn damaged_central_record_with_known_length_does_not_hide_later_records() {
        let mut writer = Writer::new(WriteOptions::default());
        for (name, data) in [
            ("one", b"first".as_slice()),
            ("two", b"middle".as_slice()),
            ("three", b"last".as_slice()),
        ] {
            writer.add(name, data, Method::Store).unwrap();
        }
        let mut bytes = writer.finish().unwrap();
        let eocd = bytes.len() - 22;
        let central = read_u32(&bytes, eocd + 16).unwrap() as usize;
        let second = central + 46 + read_u16(&bytes, central + 28).unwrap() as usize;
        bytes[second] = 0;
        let archive = Archive::open(&bytes, Limits::default()).unwrap();
        assert_eq!(archive.entries().len(), 3);
        assert_eq!(archive.entries()[0].read_all().unwrap(), b"first");
        assert_eq!(
            archive.entries()[1].error(),
            Some(&EntryError::InvalidHeader)
        );
        assert_eq!(archive.entries()[2].read_all().unwrap(), b"last");
        assert!(!archive.is_complete);
    }

    #[test]
    fn rejected_declared_sizes_do_not_consume_the_cumulative_budget() {
        let mut writer = Writer::new(WriteOptions::default());
        writer.add("large", b"0123456789", Method::Store).unwrap();
        writer.add("healthy", b"1234", Method::Store).unwrap();
        writer.add("over-total", b"123456", Method::Store).unwrap();
        let bytes = writer.finish().unwrap();
        let limits = Limits {
            max_total_size: 5,
            ..Limits::default()
        };
        let archive = Archive::open(&bytes, limits).unwrap();
        assert_eq!(
            archive.entries()[0].error,
            Some(EntryError::LimitExceeded {
                limit: "total output size"
            })
        );
        assert_eq!(archive.entries()[1].read_all().unwrap(), b"1234");
        assert_eq!(
            archive.entries()[2].error,
            Some(EntryError::LimitExceeded {
                limit: "total output size"
            })
        );
    }

    #[test]
    fn duplicate_entries_do_not_consume_cumulative_budget_in_central_or_recovery_index() {
        let mut writer = Writer::new(WriteOptions::default());
        for name in ["one", "two", "tri"] {
            writer.add(name, b"x", Method::Store).unwrap();
        }
        let bytes = writer.finish().unwrap();
        let limits = Limits {
            max_total_size: 2,
            ..Limits::default()
        };

        let mut central_bytes = bytes.clone();
        let eocd = central_bytes.len() - 22;
        let central = read_u32(&central_bytes, eocd + 16).unwrap() as usize;
        let second_central =
            central + 46 + read_u16(&central_bytes, central + 28).unwrap() as usize;
        central_bytes[second_central + 46..second_central + 49].copy_from_slice(b"one");
        let indexed = Archive::open(&central_bytes, limits).unwrap();
        assert_eq!(
            indexed.entries()[1].error(),
            Some(&EntryError::DuplicateName)
        );
        assert_eq!(indexed.entries()[0].read_all().unwrap(), b"x");
        assert_eq!(indexed.entries()[2].read_all().unwrap(), b"x");

        let mut recovered = bytes;
        let second_local = 30 + 3 + 1;
        recovered[second_local + 30..second_local + 33].copy_from_slice(b"one");
        let eocd = recovered.len() - 22;
        let central = read_u32(&recovered, eocd + 16).unwrap() as usize;
        recovered.truncate(central);
        let recovered = Archive::open(&recovered, limits).unwrap();
        assert!(!recovered.is_complete);
        assert_eq!(
            recovered.entries()[1].error(),
            Some(&EntryError::DuplicateName)
        );
        assert_eq!(recovered.entries()[0].read_all().unwrap(), b"x");
        assert_eq!(recovered.entries()[2].read_all().unwrap(), b"x");
    }

    #[test]
    fn zip64_switches_at_sentinel_boundaries_without_large_buffers() {
        let m = u32::MAX as u64;
        assert!(!entry_needs_zip64(false, m - 1, m - 1, m - 1));
        assert!(entry_needs_zip64(false, m, 0, 0));
        assert!(entry_needs_zip64(false, 0, m, 0));
        assert!(entry_needs_zip64(false, 0, 0, m));
        assert!(!directory_needs_zip64(
            false,
            u16::MAX as u64 - 1,
            m - 1,
            m - 1,
            false
        ));
        assert!(directory_needs_zip64(false, u16::MAX as u64, 0, 0, false));
        assert!(directory_needs_zip64(false, 1, m, 0, false));
        assert!(directory_needs_zip64(false, 1, 0, m, false));
        assert!(directory_needs_zip64(true, 1, 1, 1, false));
    }

    #[test]
    fn windows_unsafe_names_and_local_unknown_flags_are_rejected() {
        for name in [
            "folder\\file",
            "C:/file",
            "NUL.txt",
            "trail. ",
            "line\nfeed",
            "a//b",
        ] {
            assert!(!is_safe_name(name), "{name:?}");
        }
        let mut writer = Writer::new(WriteOptions::default());
        writer.add("valid.txt", b"x", Method::Store).unwrap();
        let mut bytes = writer.finish().unwrap();
        bytes[6] |= 0x20;
        let archive = Archive::open(&bytes, Limits::default()).unwrap();
        assert_eq!(
            archive.entries()[0].read_all(),
            Err(EntryError::InvalidHeader)
        );
    }

    #[test]
    fn local_name_limit_is_checked_before_decoding() {
        let mut writer = Writer::new(WriteOptions::default());
        writer.add("a", b"x", Method::Store).unwrap();
        let mut bytes = writer.finish().unwrap();
        let eocd_before = bytes.len() - 22;
        let old_central = read_u32(&bytes, eocd_before + 16).unwrap() as usize;
        let old_name_len = read_u16(&bytes, 26).unwrap() as usize;
        assert_eq!(old_name_len, 1);
        let extra_name_bytes = u16::MAX as usize - old_name_len;
        let name_end = 30 + old_name_len;
        bytes.splice(name_end..name_end, vec![b'x'; extra_name_bytes]);
        bytes[26..28].copy_from_slice(&u16::MAX.to_le_bytes());
        let eocd = bytes.len() - 22;
        bytes[eocd + 16..eocd + 20]
            .copy_from_slice(&((old_central + extra_name_bytes) as u32).to_le_bytes());

        let archive = Archive::open(&bytes, Limits::default()).unwrap();
        assert_eq!(
            archive.entries()[0].read_all(),
            Err(EntryError::NameTooLong)
        );
    }

    #[test]
    fn no_usable_indexed_entries_returns_a_global_error() {
        let mut writer = Writer::new(WriteOptions::default());
        writer.add("unknown", b"x", Method::Store).unwrap();
        let mut bytes = writer.finish().unwrap();
        let eocd = bytes.len() - 22;
        let central = read_u32(&bytes, eocd + 16).unwrap() as usize;
        bytes[central + 10..central + 12].copy_from_slice(&99u16.to_le_bytes());
        assert_eq!(
            Archive::open(&bytes, Limits::default()).unwrap_err(),
            ZipError::NoUsableEntries
        );
        let empty = Writer::new(WriteOptions::default()).finish().unwrap();
        assert!(
            Archive::open(&empty, Limits::default())
                .unwrap()
                .entries()
                .is_empty()
        );
    }

    #[test]
    fn central_entry_disk_number_is_checked_independently() {
        let mut writer = Writer::new(WriteOptions::default());
        writer.add("first", b"one", Method::Store).unwrap();
        writer.add("second", b"two", Method::Store).unwrap();
        let mut bytes = writer.finish().unwrap();
        let eocd = bytes.len() - 22;
        let central = read_u32(&bytes, eocd + 16).unwrap() as usize;
        let second = central + 46 + read_u16(&bytes, central + 28).unwrap() as usize;
        bytes[second + 34..second + 36].copy_from_slice(&1u16.to_le_bytes());
        let archive = Archive::open(&bytes, Limits::default()).unwrap();
        assert_eq!(archive.entries()[0].read_all().unwrap(), b"one");
        assert_eq!(
            archive.entries()[1].error,
            Some(EntryError::UnsupportedFeature("multi-disk entry"))
        );

        let mut writer = Writer::new(WriteOptions {
            force_zip64: true,
            ..WriteOptions::default()
        });
        writer.add("wide", b"x", Method::Store).unwrap();
        let mut bytes = writer.finish().unwrap();
        let eocd = bytes.len() - 22;
        let locator = eocd - 20;
        let z64 = read_u64(&bytes, locator + 8).unwrap() as usize;
        let central = read_u64(&bytes, z64 + 48).unwrap() as usize;
        let extra_len = read_u16(&bytes, central + 30).unwrap();
        let extra_value_len = read_u16(
            &bytes,
            central + 46 + read_u16(&bytes, central + 28).unwrap() as usize + 2,
        )
        .unwrap();
        bytes[central + 34..central + 36].copy_from_slice(&u16::MAX.to_le_bytes());
        bytes[central + 30..central + 32].copy_from_slice(&(extra_len + 4).to_le_bytes());
        let extra_value_pos = central + 46 + read_u16(&bytes, central + 28).unwrap() as usize + 4;
        bytes[extra_value_pos..extra_value_pos + 2]
            .copy_from_slice(&(extra_value_len + 4).to_le_bytes());
        bytes.splice(z64..z64, 1u32.to_le_bytes());
        let new_z64 = z64 + 4;
        let cd_size = read_u64(&bytes, new_z64 + 40).unwrap() + 4;
        bytes[new_z64 + 40..new_z64 + 48].copy_from_slice(&cd_size.to_le_bytes());
        bytes[new_z64 + 56 + 8..new_z64 + 56 + 16].copy_from_slice(&(new_z64 as u64).to_le_bytes());
        assert_eq!(
            Archive::open(&bytes, Limits::default()).unwrap_err(),
            ZipError::NoUsableEntries
        );
    }

    #[test]
    fn duplicate_names_are_reported_separately_from_path_safety() {
        let mut writer = Writer::new(WriteOptions::default());
        writer.add("one", b"1", Method::Store).unwrap();
        writer.add("two", b"2", Method::Store).unwrap();
        let mut bytes = writer.finish().unwrap();
        let eocd = bytes.len() - 22;
        let central = read_u32(&bytes, eocd + 16).unwrap() as usize;
        let second = central + 46 + read_u16(&bytes, central + 28).unwrap() as usize;
        bytes[second + 46..second + 49].copy_from_slice(b"one");
        let archive = Archive::open(&bytes, Limits::default()).unwrap();
        assert!(archive.entries()[1].is_duplicate());
        assert!(archive.entries()[1].is_name_safe());
        assert_eq!(
            archive.entries()[1].error(),
            Some(&EntryError::DuplicateName)
        );
    }

    #[test]
    fn rejected_entries_do_not_hide_a_healthy_name_lookup() {
        for unsafe_name in [true, false] {
            let mut writer = Writer::new(WriteOptions::default());
            writer.add("bad", b"rejected", Method::Store).unwrap();
            let healthy = if unsafe_name { "a" } else { "yes" };
            writer.add(healthy, b"healthy", Method::Store).unwrap();
            let mut bytes = writer.finish().unwrap();
            let eocd = bytes.len() - 22;
            let central = read_u32(&bytes, eocd + 16).unwrap() as usize;
            if unsafe_name {
                bytes[30..33].copy_from_slice(b"./a");
                bytes[central + 46..central + 49].copy_from_slice(b"./a");
            } else {
                bytes[30..33].copy_from_slice(b"yes");
                bytes[central + 46..central + 49].copy_from_slice(b"yes");
                bytes[8..10].copy_from_slice(&99u16.to_le_bytes());
                bytes[central + 10..central + 12].copy_from_slice(&99u16.to_le_bytes());
            }
            let archive = Archive::open(&bytes, Limits::default()).unwrap();
            assert!(archive.entries()[0].error().is_some());
            assert_eq!(archive.entries()[1].is_duplicate(), !unsafe_name);
            assert!(archive.entries()[1].error().is_none());
            assert_eq!(
                archive.entry(healthy).unwrap().read_all().unwrap(),
                b"healthy"
            );
        }
    }

    #[test]
    fn writer_matches_independently_validated_wire_archives() {
        // These fixed wire contracts were CRC/content-checked by Python zipfile.
        // Keeping them in Rust CI detects even a shared writer/reader regression.
        for (force_zip64, expected) in [
            (
                false,
                include_bytes!("../../../tests/format-reference/zip/writer-golden-classic.zip")
                    .as_slice(),
            ),
            (
                true,
                include_bytes!("../../../tests/format-reference/zip/writer-golden-zip64.zip")
                    .as_slice(),
            ),
        ] {
            let mut writer = Writer::new(WriteOptions {
                force_zip64,
                ..WriteOptions::default()
            });
            writer
                .add(
                    "café.txt",
                    "stored golden: café\n".as_bytes(),
                    Method::Store,
                )
                .unwrap();
            writer
                .add(
                    "東京/🙂.xml",
                    "<r>zip golden 🙂</r>".as_bytes(),
                    Method::Deflate,
                )
                .unwrap();
            assert_eq!(writer.finish().unwrap(), expected);
        }
    }

    #[test]
    fn hostile_zip64_offset_and_multidisk_are_errors_without_panics() {
        let mut bytes = fixture("valid-force-zip64.zip");
        let locator = bytes.len() - 42;
        bytes[locator + 8..locator + 16].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(Archive::open(&bytes, Limits::default()).is_err());
        let mut bytes = fixture("valid-mixed.zip");
        let end = bytes.len() - 22;
        bytes[end + 4] = 1;
        assert_eq!(
            Archive::open(&bytes, Limits::default()).unwrap_err(),
            ZipError::Unsupported("multi-disk archive")
        );
    }
}
