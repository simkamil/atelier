// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, read-only, in-memory reader for the Compound File Binary format
//! ([MS-CFB], versions 3 and 4).
//!
//! The container is parsed from a byte slice. Global structures (header,
//! DIFAT, FAT, directory stream) must be sound for `Compound::open` to
//! succeed. Everything else is checked lazily or reported as an [`Anomaly`]:
//! a damaged subtree, mini-FAT or stream does not make healthy streams
//! unreadable. Every size is checked against [`Limits`] and against the file
//! before allocating, and no chain is followed more than once per sector
//! count, so cycles are detected without extra memory.

#![forbid(unsafe_code)]

use std::cell::Cell;

const SIGNATURE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const HEADER_SIZE: usize = 512;
const DIR_ENTRY_SIZE: usize = 128;
const MINI_SECTOR_SIZE: u64 = 64;
const MINI_CUTOFF: u32 = 4096;
const HEADER_DIFAT: usize = 109;

/// First special sector value; every regular sector number is below it.
const MAXREGSECT: u32 = 0xffff_fffa;
/// Sector listing a DIFAT sector in the FAT.
pub const DIFSECT: u32 = 0xffff_fffc;
/// Sector listing a FAT sector in the FAT.
pub const FATSECT: u32 = 0xffff_fffd;
/// End of a sector chain.
pub const ENDOFCHAIN: u32 = 0xffff_fffe;
/// Unallocated sector.
pub const FREESECT: u32 = 0xffff_ffff;
const NOSTREAM: u32 = 0xffff_ffff;

/// Returns true when `data` starts with the CFB signature. Cheap, no parsing.
pub fn is_cfb(data: &[u8]) -> bool {
    data.len() >= SIGNATURE.len() && data[..SIGNATURE.len()] == SIGNATURE
}

/// Resource limits, all checked before the corresponding allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Maximum size of the input slice.
    pub max_file_size: u64,
    /// Maximum number of directory entry slots (used or not).
    pub max_entries: usize,
    /// Maximum storage nesting below the root (root children have depth 1).
    pub max_depth: usize,
    /// Maximum declared size of one stream.
    pub max_stream_size: u64,
    /// Maximum cumulative number of bytes returned by read calls on one
    /// `Compound` value.
    pub max_total_read: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_size: 1024 * 1024 * 1024,
            max_entries: 100_000,
            max_depth: 64,
            max_stream_size: 256 * 1024 * 1024,
            max_total_read: 1024 * 1024 * 1024,
        }
    }
}

/// Errors that prevent opening a container or reading a stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CfbError {
    NotCfb,
    UnsupportedVersion(u16),
    InvalidHeader(&'static str),
    Truncated,
    InvalidFat(&'static str),
    InvalidDifat(&'static str),
    /// The FAT does not contain an entry for a listed allocation sector.
    FatTooShort {
        sector: u32,
    },
    /// A sector listed as a FAT sector is not marked FATSECT in the FAT.
    InvalidFatSectorMarker {
        sector: u32,
        marker: u32,
    },
    /// A sector listed as a DIFAT sector is not marked DIFSECT in the FAT.
    InvalidDifatSectorMarker {
        sector: u32,
        marker: u32,
    },
    ForeignEntry,
    /// A chain refers to a sector outside the file or a special value.
    InvalidSector(u32),
    ChainCycle,
    ChainTooShort,
    ChainTooLong,
    InvalidDirectory(&'static str),
    InvalidMiniStream(&'static str),
    LimitExceeded {
        limit: &'static str,
    },
    NotFound,
    NotAStream,
    OutOfRange,
    AllocationFailed,
}

impl core::fmt::Display for CfbError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotCfb => f.write_str("not a compound file"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported CFB major version {v}"),
            Self::InvalidHeader(s) => write!(f, "invalid CFB header: {s}"),
            Self::Truncated => f.write_str("truncated CFB file"),
            Self::InvalidFat(s) => write!(f, "invalid CFB FAT: {s}"),
            Self::InvalidDifat(s) => write!(f, "invalid CFB DIFAT: {s}"),
            Self::FatTooShort { sector } => {
                write!(f, "CFB FAT has no entry for sector {sector}")
            }
            Self::InvalidFatSectorMarker { sector, marker } => write!(
                f,
                "CFB FAT sector {sector} has marker {marker:#x}, expected FATSECT"
            ),
            Self::InvalidDifatSectorMarker { sector, marker } => write!(
                f,
                "CFB DIFAT sector {sector} has marker {marker:#x}, expected DIFSECT"
            ),
            Self::InvalidSector(s) => write!(f, "invalid CFB sector reference {s:#x}"),
            Self::ForeignEntry => f.write_str("CFB entry belongs to another container"),
            Self::ChainCycle => f.write_str("cycle in CFB sector chain"),
            Self::ChainTooShort => f.write_str("CFB chain shorter than declared size"),
            Self::ChainTooLong => f.write_str("CFB chain longer than declared size"),
            Self::InvalidDirectory(s) => write!(f, "invalid CFB directory: {s}"),
            Self::InvalidMiniStream(s) => write!(f, "invalid CFB mini stream: {s}"),
            Self::LimitExceeded { limit } => write!(f, "CFB limit exceeded: {limit}"),
            Self::NotFound => f.write_str("CFB entry not found"),
            Self::NotAStream => f.write_str("CFB entry is not a stream"),
            Self::OutOfRange => f.write_str("CFB read range out of bounds"),
            Self::AllocationFailed => f.write_str("CFB allocation failed"),
        }
    }
}
impl std::error::Error for CfbError {}

/// A non-fatal problem found while indexing; the affected entry or link is
/// ignored and the rest of the container stays usable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Anomaly {
    /// A header field that should hold a fixed value does not.
    HeaderField(&'static str),
    /// A directory entry is reachable more than once (or through a cycle).
    EntryReferencedTwice(u32),
    /// A sibling or child index is outside the directory.
    InvalidLink(u32),
    /// Name not terminated, odd length, too long, unpaired surrogate, or
    /// containing `/`.
    InvalidName(u32),
    /// Unknown, unallocated or misplaced (second root) entry type.
    InvalidEntryType(u32),
    InvalidColor(u32),
    /// Stream entry's child link is not NOSTREAM.
    StreamHasChildren(u32),
    /// Metadata that CFB readers ignore on stream entries is nonzero.
    IgnoredStreamMetadata(u32),
    /// A storage entry has nonzero stream-only start sector or size fields.
    StorageHasStreamFields(u32),
    /// Root storage creation time is nonzero.
    RootHasCreationTime(u32),
    /// Same name (case-insensitive) as an earlier sibling; the entry is ignored.
    DuplicateName(u32),
    /// The children of this storage were not indexed: nesting too deep.
    DepthExceeded(u32),
    /// Stream size above the 2 GiB allowed for version 3.
    OversizedV3Stream(u32),
}

/// Kind of a directory entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Root,
    Storage,
    Stream,
}

/// A reachable directory entry.
#[derive(Clone, Debug)]
pub struct Entry {
    id: u32,
    name: String,
    key: Vec<u16>,
    kind: EntryKind,
    child: u32,
    clsid: [u8; 16],
    state_bits: u32,
    created: u64,
    modified: u64,
    start_sector: u32,
    size: u64,
    children: Vec<u32>,
}

impl Entry {
    pub fn id(&self) -> u32 {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn kind(&self) -> EntryKind {
        self.kind
    }
    pub fn is_stream(&self) -> bool {
        self.kind == EntryKind::Stream
    }
    /// True for storages and the root.
    pub fn is_storage(&self) -> bool {
        self.kind != EntryKind::Stream
    }
    /// 16-byte CLSID in file byte order.
    pub fn clsid(&self) -> [u8; 16] {
        self.clsid
    }
    pub fn state_bits(&self) -> u32 {
        self.state_bits
    }
    /// Raw FILETIME (0 when unset).
    pub fn created(&self) -> u64 {
        self.created
    }
    /// Raw FILETIME (0 when unset).
    pub fn modified(&self) -> u64 {
        self.modified
    }
    pub fn start_sector(&self) -> u32 {
        self.start_sector
    }
    /// Declared size in bytes (stream size, or mini stream size for the root).
    pub fn size(&self) -> u64 {
        self.size
    }
    /// Directory ids of the children, sorted by the CFB name order.
    pub fn child_ids(&self) -> &[u32] {
        &self.children
    }
}

/// One line of [`Compound::list`].
#[derive(Clone, Debug)]
pub struct Listed<'c> {
    /// Path from the root, components joined by `/`, without leading `/`.
    pub path: String,
    pub depth: usize,
    pub entry: &'c Entry,
}

struct Mini {
    fat: Vec<u32>,
    chain: Vec<u32>,
    size: u64,
}

/// An opened container. Borrows the input bytes.
pub struct Compound<'a> {
    data: &'a [u8],
    limits: Limits,
    major: u16,
    sector_size: usize,
    sector_count: u32,
    fat: Vec<u32>,
    entries: Vec<Option<Entry>>,
    anomalies: Vec<Anomaly>,
    mini: Result<Mini, CfbError>,
    total_read: Cell<u64>,
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u64le(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}

fn div_ceil(a: u64, b: u64) -> u64 {
    a.div_ceil(b)
}

/// Simple per-code-unit uppercase mapping used for name comparison.
fn fold_unit(u: u16) -> u16 {
    let Some(c) = char::from_u32(u32::from(u)) else {
        return u;
    };
    match atelier_unicode::properties::simple_uppercase(c) {
        Some([mapped]) if *mapped <= 0xffff => *mapped as u16,
        _ => u,
    }
}

fn fold_key(units: &[u16]) -> Vec<u16> {
    units.iter().map(|&u| fold_unit(u)).collect()
}

fn reserve<T>(v: &mut Vec<T>, n: usize) -> Result<(), CfbError> {
    v.try_reserve_exact(n)
        .map_err(|_| CfbError::AllocationFailed)
}

fn reserve_growth<T>(v: &mut Vec<T>, n: usize) -> Result<(), CfbError> {
    v.try_reserve(n).map_err(|_| CfbError::AllocationFailed)
}

/// Follows a chain in `table`, calling `f` for each sector. `bound` is the
/// number of valid sectors: visiting more than `bound` sectors means a cycle.
/// Returns the number of sectors visited. `f` returns false to stop early.
fn walk(
    table: &[u32],
    start: u32,
    bound: u32,
    mut f: impl FnMut(u32) -> Result<bool, CfbError>,
) -> Result<u64, CfbError> {
    let mut s = start;
    let mut steps: u64 = 0;
    while s != ENDOFCHAIN {
        if s >= bound {
            return Err(CfbError::InvalidSector(s));
        }
        steps += 1;
        if steps > u64::from(bound) {
            return Err(CfbError::ChainCycle);
        }
        if !f(s)? {
            return Ok(steps);
        }
        s = *table.get(s as usize).ok_or(CfbError::InvalidSector(s))?;
    }
    Ok(steps)
}

impl<'a> Compound<'a> {
    /// Opens a container with the given limits.
    pub fn open(data: &'a [u8], limits: Limits) -> Result<Self, CfbError> {
        if data.len() as u64 > limits.max_file_size {
            return Err(CfbError::LimitExceeded { limit: "file size" });
        }
        if !is_cfb(data) {
            return Err(CfbError::NotCfb);
        }
        if data.len() < HEADER_SIZE {
            return Err(CfbError::Truncated);
        }
        let mut anomalies = Vec::new();
        let major = u16le(data, 26);
        if major != 3 && major != 4 {
            return Err(CfbError::UnsupportedVersion(major));
        }
        if u16le(data, 28) != 0xfffe {
            return Err(CfbError::InvalidHeader("byte order"));
        }
        let shift = u16le(data, 30);
        if shift != if major == 3 { 9 } else { 12 } {
            return Err(CfbError::InvalidHeader("sector shift"));
        }
        if u16le(data, 32) != 6 {
            return Err(CfbError::InvalidHeader("mini sector shift"));
        }
        if u32le(data, 56) != MINI_CUTOFF {
            return Err(CfbError::InvalidHeader("mini stream cutoff"));
        }
        let sector_size = 1usize << shift;
        if data.len() < sector_size {
            return Err(CfbError::Truncated);
        }
        if data[8..24].iter().any(|&b| b != 0) {
            anomalies.push(Anomaly::HeaderField("header CLSID"));
        }
        if u16le(data, 24) != 0x3e {
            anomalies.push(Anomaly::HeaderField("minor version"));
        }
        if data[34..40].iter().any(|&b| b != 0) {
            anomalies.push(Anomaly::HeaderField("reserved"));
        }
        if major == 4 && data[HEADER_SIZE..sector_size].iter().any(|&b| b != 0) {
            anomalies.push(Anomaly::HeaderField("header padding"));
        }
        if (data.len() - sector_size) % sector_size != 0 {
            anomalies.push(Anomaly::HeaderField("partial trailing sector"));
        }
        let count = ((data.len() - sector_size) / sector_size) as u64;
        let sector_count = u32::try_from(count.min(u64::from(MAXREGSECT))).unwrap_or(0);

        let nfat = u32le(data, 44);
        let first_dir = u32le(data, 48);
        let first_minifat = u32le(data, 60);
        let nminifat = u32le(data, 64);
        let first_difat = u32le(data, 68);
        let ndifat = u32le(data, 72);

        let mut c = Self {
            data,
            limits,
            major,
            sector_size,
            sector_count,
            fat: Vec::new(),
            entries: Vec::new(),
            anomalies,
            mini: Err(CfbError::InvalidMiniStream("not loaded")),
            total_read: Cell::new(0),
        };
        c.load_fat(nfat, first_difat, ndifat)?;
        let dir = c.load_directory(first_dir)?;
        c.build_tree(&dir)?;
        let root = c.root();
        let (root_start, root_size) = (root.start_sector, root.size);
        c.mini = c.load_mini(first_minifat, nminifat, root_start, root_size);
        Ok(c)
    }

    fn sector(&self, s: u32) -> Result<&'a [u8], CfbError> {
        if s >= self.sector_count {
            return Err(CfbError::InvalidSector(s));
        }
        let ss = self.sector_size as u64;
        let off = (u64::from(s) + 1) * ss;
        let end = off + ss;
        if end > self.data.len() as u64 {
            return Err(CfbError::Truncated);
        }
        Ok(&self.data[off as usize..end as usize])
    }

    fn load_fat(&mut self, nfat: u32, first_difat: u32, ndifat: u32) -> Result<(), CfbError> {
        let epf = self.sector_size / 4;
        if nfat == 0 {
            return Err(CfbError::InvalidFat("no FAT sector"));
        }
        if nfat > self.sector_count {
            return Err(CfbError::InvalidFat("FAT sector count exceeds file"));
        }
        let expected = if nfat as usize > HEADER_DIFAT {
            div_ceil((nfat as usize - HEADER_DIFAT) as u64, (epf - 1) as u64)
        } else {
            0
        };
        if u64::from(ndifat) != expected {
            return Err(CfbError::InvalidDifat("DIFAT sector count"));
        }
        let mut seen = Vec::new();
        reserve(&mut seen, self.sector_count as usize)?;
        seen.resize(self.sector_count as usize, false);
        let mut fat_sectors: Vec<u32> = Vec::new();
        let mut difat_sectors: Vec<u32> = Vec::new();
        reserve(&mut fat_sectors, nfat as usize)?;
        let count = self.sector_count;
        let add = |s: u32, seen: &mut [bool], fat_sectors: &mut Vec<u32>| {
            if s >= count {
                return Err(CfbError::InvalidSector(s));
            }
            if std::mem::replace(&mut seen[s as usize], true) {
                return Err(CfbError::InvalidFat("sector listed twice"));
            }
            fat_sectors.push(s);
            Ok(())
        };
        for i in 0..HEADER_DIFAT {
            let sector = u32le(self.data, 76 + 4 * i);
            if fat_sectors.len() < nfat as usize {
                add(sector, &mut seen, &mut fat_sectors)?;
            } else if sector != FREESECT {
                return Err(CfbError::InvalidDifat("unused header DIFAT entry"));
            }
        }
        let mut next = first_difat;
        for _ in 0..ndifat {
            let sec = self.sector(next)?;
            if std::mem::replace(&mut seen[next as usize], true) {
                return Err(CfbError::ChainCycle);
            }
            difat_sectors.push(next);
            for i in 0..epf - 1 {
                let sector = u32le(sec, 4 * i);
                if fat_sectors.len() < nfat as usize {
                    add(sector, &mut seen, &mut fat_sectors)?;
                } else if sector != FREESECT {
                    return Err(CfbError::InvalidDifat("unused DIFAT entry"));
                }
            }
            next = u32le(sec, 4 * (epf - 1));
        }
        if ndifat > 0 && next != ENDOFCHAIN {
            return Err(CfbError::InvalidDifat("chain does not end"));
        }
        let mut fat = Vec::new();
        reserve(&mut fat, nfat as usize * epf)?;
        for s in fat_sectors.iter().copied() {
            let sec = self.sector(s)?;
            fat.extend(sec.chunks_exact(4).map(|c| u32le(c, 0)));
        }
        for sector in fat_sectors.iter().copied() {
            let marker = *fat
                .get(sector as usize)
                .ok_or(CfbError::FatTooShort { sector })?;
            if marker != FATSECT {
                return Err(CfbError::InvalidFatSectorMarker { sector, marker });
            }
        }
        for sector in difat_sectors {
            let marker = *fat
                .get(sector as usize)
                .ok_or(CfbError::FatTooShort { sector })?;
            if marker != DIFSECT {
                return Err(CfbError::InvalidDifatSectorMarker { sector, marker });
            }
        }
        if fat
            .iter()
            .skip(self.sector_count as usize)
            .any(|&entry| entry != FREESECT)
        {
            return Err(CfbError::InvalidFat("non-free FAT entry past end of file"));
        }
        self.fat = fat;
        Ok(())
    }

    fn load_directory(&self, first_dir: u32) -> Result<Vec<u8>, CfbError> {
        let per_sector = self.sector_size / DIR_ENTRY_SIZE;
        let max_sectors = self.limits.max_entries.div_ceil(per_sector);
        let mut dir: Vec<u8> = Vec::new();
        let mut n = 0usize;
        let count = walk(&self.fat, first_dir, self.sector_count, |s| {
            n += 1;
            if n > max_sectors {
                return Err(CfbError::LimitExceeded {
                    limit: "directory entries",
                });
            }
            let sec = self.sector(s)?;
            reserve(&mut dir, sec.len())?;
            dir.extend_from_slice(sec);
            Ok(true)
        })?;
        if count == 0 {
            return Err(CfbError::InvalidDirectory("empty directory"));
        }
        Ok(dir)
    }

    fn parse_entry(&self, dir: &[u8], id: u32) -> Result<Entry, Anomaly> {
        let o = id as usize * DIR_ENTRY_SIZE;
        let raw = &dir[o..o + DIR_ENTRY_SIZE];
        let kind = match raw[66] {
            1 => EntryKind::Storage,
            2 => EntryKind::Stream,
            5 if id == 0 => EntryKind::Root,
            _ => return Err(Anomaly::InvalidEntryType(id)),
        };
        if (id == 0) != (kind == EntryKind::Root) {
            return Err(Anomaly::InvalidEntryType(id));
        }
        if raw[67] > 1 {
            return Err(Anomaly::InvalidColor(id));
        }
        let len = usize::from(u16le(raw, 64));
        if !(2..=64).contains(&len) || len % 2 != 0 {
            return Err(Anomaly::InvalidName(id));
        }
        let units: Vec<u16> = raw[..len].chunks_exact(2).map(|c| u16le(c, 0)).collect();
        let (last, body) = units.split_last().ok_or(Anomaly::InvalidName(id))?;
        if *last != 0 || body.contains(&0) || body.contains(&u16::from(b'/')) {
            return Err(Anomaly::InvalidName(id));
        }
        let name = String::from_utf16(body).map_err(|_| Anomaly::InvalidName(id))?;
        let mut size = u64le(raw, 120);
        if self.major == 3 {
            size &= 0xffff_ffff;
        }
        let mut clsid = [0u8; 16];
        clsid.copy_from_slice(&raw[80..96]);
        Ok(Entry {
            id,
            key: fold_key(body),
            name,
            kind,
            child: u32le(raw, 76),
            clsid,
            state_bits: u32le(raw, 96),
            created: u64le(raw, 100),
            modified: u64le(raw, 108),
            start_sector: u32le(raw, 116),
            size,
            children: Vec::new(),
        })
    }

    fn build_tree(&mut self, dir: &[u8]) -> Result<(), CfbError> {
        let n = dir.len() / DIR_ENTRY_SIZE;
        if n > self.limits.max_entries {
            return Err(CfbError::LimitExceeded {
                limit: "directory entries",
            });
        }
        let root = self
            .parse_entry(dir, 0)
            .map_err(|_| CfbError::InvalidDirectory("invalid root entry"))?;
        if root.name != "Root Entry" {
            self.anomalies.push(Anomaly::HeaderField("root name"));
        }
        if root.created != 0 {
            self.anomalies.push(Anomaly::RootHasCreationTime(0));
        }
        let mut entries: Vec<Option<Entry>> = Vec::new();
        reserve(&mut entries, n)?;
        entries.resize_with(n, || None);
        let mut visited = Vec::new();
        reserve(&mut visited, n)?;
        visited.resize(n, false);
        visited[0] = true;
        entries[0] = Some(root);
        let mut anomalies = std::mem::take(&mut self.anomalies);
        let mut work: Vec<(u32, usize)> = Vec::new();
        reserve(&mut work, 1)?;
        work.push((0, 0));
        while let Some((sid, depth)) = work.pop() {
            let child = entries[sid as usize].as_ref().map_or(NOSTREAM, |e| e.child);
            if child == NOSTREAM {
                continue;
            }
            if depth + 1 > self.limits.max_depth {
                anomalies.push(Anomaly::DepthExceeded(sid));
                continue;
            }
            let mut stack = Vec::new();
            reserve(&mut stack, 1)?;
            stack.push(child);
            let mut found: Vec<Entry> = Vec::new();
            while let Some(id) = stack.pop() {
                if id as usize >= n {
                    anomalies.push(Anomaly::InvalidLink(sid));
                    continue;
                }
                if std::mem::replace(&mut visited[id as usize], true) {
                    anomalies.push(Anomaly::EntryReferencedTwice(id));
                    continue;
                }
                let o = id as usize * DIR_ENTRY_SIZE;
                for link in [u32le(dir, o + 68), u32le(dir, o + 72)] {
                    if link != NOSTREAM {
                        reserve_growth(&mut stack, 1)?;
                        stack.push(link);
                    }
                }
                match self.parse_entry(dir, id) {
                    Ok(e) => {
                        match e.kind {
                            EntryKind::Stream => {
                                if e.child != NOSTREAM {
                                    anomalies.push(Anomaly::StreamHasChildren(id));
                                }
                                if e.clsid != [0; 16] || e.created != 0 || e.modified != 0 {
                                    anomalies.push(Anomaly::IgnoredStreamMetadata(id));
                                }
                            }
                            EntryKind::Storage if e.start_sector != 0 || e.size != 0 => {
                                anomalies.push(Anomaly::StorageHasStreamFields(id));
                            }
                            _ => {}
                        }
                        if e.kind == EntryKind::Stream && self.major == 3 && e.size > 0x8000_0000 {
                            anomalies.push(Anomaly::OversizedV3Stream(id));
                        }
                        reserve_growth(&mut found, 1)?;
                        found.push(e);
                    }
                    Err(a) => anomalies.push(a),
                }
            }
            found.sort_unstable_by(|a, b| {
                (a.key.len(), &a.key, a.id).cmp(&(b.key.len(), &b.key, b.id))
            });
            let mut ids: Vec<u32> = Vec::new();
            let mut prev_id: Option<u32> = None;
            for e in found {
                if prev_id.is_some_and(|prev| {
                    entries[prev as usize]
                        .as_ref()
                        .is_some_and(|previous| previous.key == e.key)
                }) {
                    anomalies.push(Anomaly::DuplicateName(e.id));
                    continue;
                }
                reserve_growth(&mut ids, 1)?;
                ids.push(e.id);
                if e.kind == EntryKind::Storage {
                    reserve_growth(&mut work, 1)?;
                    work.push((e.id, depth + 1));
                }
                let id = e.id as usize;
                entries[id] = Some(e);
                prev_id = Some(id as u32);
            }
            if let Some(Some(p)) = entries.get_mut(sid as usize) {
                p.children = ids;
            }
        }
        self.anomalies = anomalies;
        self.entries = entries;
        Ok(())
    }

    fn load_mini(
        &self,
        first_minifat: u32,
        nminifat: u32,
        root_start: u32,
        root_size: u64,
    ) -> Result<Mini, CfbError> {
        let epf = self.sector_size / 4;
        let mut fat = Vec::new();
        let mut sectors = 0u32;
        walk(&self.fat, first_minifat, self.sector_count, |s| {
            sectors += 1;
            let sec = self.sector(s)?;
            reserve(&mut fat, epf)?;
            fat.extend(sec.chunks_exact(4).map(|c| u32le(c, 0)));
            Ok(true)
        })?;
        if sectors != nminifat {
            return Err(CfbError::InvalidMiniStream("mini FAT sector count"));
        }
        let mut chain = Vec::new();
        let n = walk(&self.fat, root_start, self.sector_count, |s| {
            reserve(&mut chain, 1)?;
            chain.push(s);
            Ok(true)
        })?;
        match n.cmp(&div_ceil(root_size, self.sector_size as u64)) {
            std::cmp::Ordering::Less => return Err(CfbError::ChainTooShort),
            std::cmp::Ordering::Greater => return Err(CfbError::ChainTooLong),
            std::cmp::Ordering::Equal => {}
        }
        Ok(Mini {
            fat,
            chain,
            size: root_size,
        })
    }

    // ---- public accessors ----

    /// Major version: 3 or 4.
    pub fn major_version(&self) -> u16 {
        self.major
    }
    pub fn sector_size(&self) -> usize {
        self.sector_size
    }
    /// Non-fatal anomalies found while indexing.
    pub fn anomalies(&self) -> &[Anomaly] {
        &self.anomalies
    }
    pub fn root(&self) -> &Entry {
        self.entries[0].as_ref().expect("root checked in open") // invariant: set by build_tree
    }
    /// CLSID of the root entry.
    pub fn root_clsid(&self) -> [u8; 16] {
        self.root().clsid
    }
    /// Entry by directory id, if reachable from the root.
    pub fn entry(&self, id: u32) -> Option<&Entry> {
        self.entries.get(id as usize).and_then(Option::as_ref)
    }
    /// Children of a storage, in CFB name order.
    pub fn children<'s>(&'s self, e: &'s Entry) -> impl Iterator<Item = &'s Entry> + 's {
        let owned = self
            .entry(e.id)
            .is_some_and(|candidate| core::ptr::eq(candidate, e));
        e.children
            .iter()
            .filter_map(move |&i| owned.then(|| self.entry(i)).flatten())
    }

    /// Whole tree in depth-first order (excluding the root), children sorted.
    pub fn list(&self) -> Vec<Listed<'_>> {
        let mut out = Vec::new();
        let mut stack: Vec<(String, usize, &Entry)> = Vec::new();
        for c in self
            .children(self.root())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            stack.push((c.name.clone(), 1, c));
        }
        while let Some((path, depth, entry)) = stack.pop() {
            for c in self.children(entry).collect::<Vec<_>>().into_iter().rev() {
                stack.push((format!("{path}/{}", c.name), depth + 1, c));
            }
            out.push(Listed { path, depth, entry });
        }
        out
    }

    /// Finds an entry by `/`-separated path. Names compare case-insensitively
    /// per UTF-16 code unit. Empty components are ignored; `""` is the root.
    pub fn find(&self, path: &str) -> Option<&Entry> {
        let mut cur = self.root();
        for comp in path.split('/').filter(|c| !c.is_empty()) {
            if !cur.is_storage() {
                return None;
            }
            let units: Vec<u16> = comp.encode_utf16().collect();
            let key = fold_key(&units);
            cur = self.children(cur).find(|c| c.key == key)?;
        }
        Some(cur)
    }
    pub fn exists(&self, path: &str) -> bool {
        self.find(path).is_some()
    }
    pub fn is_stream(&self, path: &str) -> bool {
        self.find(path).is_some_and(Entry::is_stream)
    }
    pub fn is_storage(&self, path: &str) -> bool {
        self.find(path).is_some_and(Entry::is_storage)
    }

    /// Reads a whole stream.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, CfbError> {
        let e = self.find(path).ok_or(CfbError::NotFound)?;
        self.read_entry(e)
    }
    /// Reads `len` bytes at `offset` (clamped to the end of the stream).
    /// `offset` beyond the end is an error; `offset == size` gives no bytes.
    pub fn read_range(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, CfbError> {
        let e = self.find(path).ok_or(CfbError::NotFound)?;
        self.read_entry_range(e, offset, len)
    }
    pub fn read_entry(&self, e: &Entry) -> Result<Vec<u8>, CfbError> {
        self.read_entry_range(e, 0, usize::MAX)
    }

    pub fn read_entry_range(
        &self,
        e: &Entry,
        offset: u64,
        len: usize,
    ) -> Result<Vec<u8>, CfbError> {
        if !self
            .entry(e.id)
            .is_some_and(|candidate| core::ptr::eq(candidate, e))
        {
            return Err(CfbError::ForeignEntry);
        }
        if !e.is_stream() {
            return Err(CfbError::NotAStream);
        }
        let size = e.size;
        if size > self.limits.max_stream_size {
            return Err(CfbError::LimitExceeded {
                limit: "stream size",
            });
        }
        if offset > size {
            return Err(CfbError::OutOfRange);
        }
        let end = offset.saturating_add(len as u64).min(size);
        let want = (end - offset) as usize;
        let total = self.total_read.get().saturating_add(want as u64);
        if total > self.limits.max_total_read {
            return Err(CfbError::LimitExceeded {
                limit: "total read size",
            });
        }
        if want == 0 {
            return Ok(Vec::new());
        }
        let mini = size < u64::from(MINI_CUTOFF);
        let (table, bound, unit): (&[u32], u32, u64) = if mini {
            let m = self.mini.as_ref().map_err(Clone::clone)?;
            let bound = u32::try_from(m.size / MINI_SECTOR_SIZE).unwrap_or(u32::MAX);
            (&m.fat, bound, MINI_SECTOR_SIZE)
        } else {
            (&self.fat, self.sector_count, self.sector_size as u64)
        };
        // Pass 1: whole chain must match the declared size exactly.
        let n = walk(table, e.start_sector, bound, |_| Ok(true))?;
        match n.cmp(&div_ceil(size, unit)) {
            std::cmp::Ordering::Less => return Err(CfbError::ChainTooShort),
            std::cmp::Ordering::Greater => return Err(CfbError::ChainTooLong),
            std::cmp::Ordering::Equal => {}
        }
        // Pass 2: copy the requested range (allocated only once the chain is sound).
        let mut out = Vec::new();
        reserve(&mut out, want)?;
        let mut index = 0u64;
        walk(table, e.start_sector, bound, |s| {
            let lo = index * unit;
            index += 1;
            let hi = lo + unit;
            if lo >= end {
                return Ok(false);
            }
            if hi <= offset {
                return Ok(true);
            }
            let a = offset.max(lo);
            let b = end.min(hi);
            let chunk = if mini {
                self.mini_slice(s, a - lo, b - a)?
            } else {
                let sec = self.sector(s)?;
                &sec[(a - lo) as usize..(b - lo) as usize]
            };
            out.extend_from_slice(chunk);
            Ok(true)
        })?;
        if out.len() != want {
            return Err(CfbError::Truncated);
        }
        self.total_read.set(total);
        Ok(out)
    }

    fn mini_slice(&self, ms: u32, skip: u64, len: u64) -> Result<&'a [u8], CfbError> {
        let m = self.mini.as_ref().map_err(Clone::clone)?;
        let pos = u64::from(ms) * MINI_SECTOR_SIZE;
        if pos + MINI_SECTOR_SIZE > m.size {
            return Err(CfbError::InvalidSector(ms));
        }
        let sec_index = (pos / self.sector_size as u64) as usize;
        let within = (pos % self.sector_size as u64) as usize;
        let sec = self.sector(*m.chain.get(sec_index).ok_or(CfbError::Truncated)?)?;
        let a = within + skip as usize;
        sec.get(a..a + len as usize).ok_or(CfbError::Truncated)
    }
}
