// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deterministic CFB container builder used to produce test fixtures. It lives
//! in the tests only: the crate itself never writes containers.
#![allow(dead_code)]

pub const FREESECT: u32 = 0xffff_ffff;
pub const ENDOFCHAIN: u32 = 0xffff_fffe;
pub const FATSECT: u32 = 0xffff_fffd;
pub const DIFSECT: u32 = 0xffff_fffc;
const NOSTREAM: u32 = 0xffff_ffff;

pub enum Node {
    Storage {
        name: String,
        clsid: [u8; 16],
        children: Vec<Node>,
    },
    Stream {
        name: String,
        data: Vec<u8>,
    },
}

pub fn stream(name: &str, data: &[u8]) -> Node {
    Node::Stream {
        name: name.to_string(),
        data: data.to_vec(),
    }
}

pub fn storage(name: &str, children: Vec<Node>) -> Node {
    Node::Storage {
        name: name.to_string(),
        clsid: [0; 16],
        children,
    }
}

pub fn storage_clsid(name: &str, clsid: [u8; 16], children: Vec<Node>) -> Node {
    Node::Storage {
        name: name.to_string(),
        clsid,
        children,
    }
}

/// Deterministic content: byte i of stream `seed` is a function of both.
pub fn pattern(seed: u32, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| {
            (i as u32)
                .wrapping_mul(31)
                .wrapping_add(seed.wrapping_mul(7)) as u8
        })
        .collect()
}

pub struct Builder {
    pub version: u16,
    pub root_clsid: [u8; 16],
    pub children: Vec<Node>,
    /// Add unused FAT sectors until there are at least this many (forces DIFAT
    /// sectors without huge data).
    pub min_fat_sectors: usize,
    /// Build the sibling tree as a right-leaning list in reverse name order:
    /// valid links, but unbalanced and unsorted.
    pub degenerate_tree: bool,
}

pub struct Built {
    pub bytes: Vec<u8>,
    pub version: u16,
    pub ss: usize,
    pub sector_count: u32,
    pub fat_sectors: Vec<u32>,
    pub difat_sectors: Vec<u32>,
    pub dir_start: u32,
    pub dir_sectors: u32,
    pub minifat_start: u32,
    pub ministream_start: u32,
    pub paths: Vec<(String, u32)>,
    pub starts: Vec<(String, u32)>,
}

struct Ent {
    name: String,
    storage: bool,
    clsid: [u8; 16],
    data: Vec<u8>,
    children: Vec<usize>,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u64,
    path: String,
}

fn fold(s: &str) -> Vec<u16> {
    s.encode_utf16()
        .map(|u| match char::from_u32(u32::from(u)) {
            Some(c) => {
                let mut it = c.to_uppercase();
                match (it.next(), it.next()) {
                    (Some(m), None) if (m as u32) <= 0xffff => m as u16,
                    _ => u,
                }
            }
            None => u,
        })
        .collect()
}

fn put_u16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
pub fn put_u32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn put_u64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}
pub fn get_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl Builder {
    pub fn new(version: u16, children: Vec<Node>) -> Self {
        Self {
            version,
            root_clsid: [0; 16],
            children,
            min_fat_sectors: 0,
            degenerate_tree: false,
        }
    }

    fn flatten(nodes: &[Node], parent_path: &str, ents: &mut Vec<Ent>) -> Vec<usize> {
        let mut ids = Vec::new();
        for n in nodes {
            let id = ents.len();
            let (name, storage, clsid, data) = match n {
                Node::Storage { name, clsid, .. } => (name, true, *clsid, Vec::new()),
                Node::Stream { name, data } => (name, false, [0; 16], data.clone()),
            };
            let path = if parent_path.is_empty() {
                name.clone()
            } else {
                format!("{parent_path}/{name}")
            };
            ents.push(Ent {
                name: name.clone(),
                storage,
                clsid,
                data,
                children: Vec::new(),
                left: NOSTREAM,
                right: NOSTREAM,
                child: NOSTREAM,
                start: if storage { 0 } else { ENDOFCHAIN },
                size: 0,
                path: path.clone(),
            });
            if let Node::Storage { children, .. } = n {
                let kids = Self::flatten(children, &path, ents);
                ents[id].children = kids;
            }
            ids.push(id);
        }
        ids
    }

    /// Links `ids` (sorted) as a balanced tree; returns the subtree root.
    fn balance(ents: &mut [Ent], ids: &[usize]) -> u32 {
        if ids.is_empty() {
            return NOSTREAM;
        }
        let mid = ids.len() / 2;
        let l = Self::balance(ents, &ids[..mid]);
        let r = Self::balance(ents, &ids[mid + 1..]);
        ents[ids[mid]].left = l;
        ents[ids[mid]].right = r;
        ids[mid] as u32
    }

    pub fn build(&self) -> Built {
        let major = self.version;
        let ss: usize = if major == 3 { 512 } else { 4096 };
        let epf = ss / 4;
        let mut ents = vec![Ent {
            name: "Root Entry".into(),
            storage: true,
            clsid: self.root_clsid,
            data: Vec::new(),
            children: Vec::new(),
            left: NOSTREAM,
            right: NOSTREAM,
            child: NOSTREAM,
            start: ENDOFCHAIN,
            size: 0,
            path: String::new(),
        }];
        let top = Self::flatten(&self.children, "", &mut ents);
        ents[0].children = top;
        // Directory ids are the flat indices; link siblings.
        for i in 0..ents.len() {
            let mut kids = ents[i].children.clone();
            kids.sort_by_key(|&k| {
                let f = fold(&ents[k].name);
                (f.len(), f)
            });
            if self.degenerate_tree {
                kids.reverse();
                for w in kids.windows(2) {
                    ents[w[0]].right = w[1] as u32;
                }
                ents[i].child = kids.first().map_or(NOSTREAM, |&k| k as u32);
            } else {
                let r = Self::balance(&mut ents, &kids);
                ents[i].child = r;
            }
        }
        // Sector assignment: large streams, ministream, minifat, directory, FAT, DIFAT.
        let mut next_sector = 0u32;
        let mut chains: Vec<(u32, u32)> = Vec::new();
        for e in ents.iter_mut().skip(1) {
            if e.storage {
                continue;
            }
            e.size = e.data.len() as u64;
            if e.data.len() >= 4096 {
                let n = e.data.len().div_ceil(ss) as u32;
                e.start = next_sector;
                chains.push((next_sector, n));
                next_sector += n;
            }
        }
        let mut mini_data: Vec<u8> = Vec::new();
        let mut mini_fat: Vec<u32> = Vec::new();
        for e in ents.iter_mut().skip(1) {
            if e.storage || e.data.is_empty() || e.data.len() >= 4096 {
                continue;
            }
            let n = e.data.len().div_ceil(64);
            let first = mini_fat.len() as u32;
            e.start = first;
            for k in 0..n {
                mini_fat.push(if k + 1 == n {
                    ENDOFCHAIN
                } else {
                    first + k as u32 + 1
                });
            }
            mini_data.extend_from_slice(&e.data);
            mini_data.resize(mini_fat.len() * 64, 0);
        }
        let ministream_start = if mini_data.is_empty() {
            ENDOFCHAIN
        } else {
            next_sector
        };
        let n_ms = mini_data.len().div_ceil(ss) as u32;
        if n_ms > 0 {
            chains.push((next_sector, n_ms));
            next_sector += n_ms;
        }
        ents[0].start = ministream_start;
        ents[0].size = mini_data.len() as u64;
        let n_mf = (mini_fat.len() * 4).div_ceil(ss) as u32;
        let minifat_start = if n_mf > 0 { next_sector } else { ENDOFCHAIN };
        if n_mf > 0 {
            chains.push((next_sector, n_mf));
            next_sector += n_mf;
        }
        let n_dir = (ents.len() * 128).div_ceil(ss) as u32;
        let dir_start = next_sector;
        chains.push((next_sector, n_dir));
        next_sector += n_dir;
        let mut f = self.min_fat_sectors.max(1);
        let difat_for = |f: usize| {
            if f > 109 {
                (f - 109).div_ceil(epf - 1)
            } else {
                0
            }
        };
        while f * epf < next_sector as usize + f + difat_for(f) {
            f += 1;
        }
        let d = difat_for(f);
        let fat_sectors: Vec<u32> = (0..f as u32).map(|i| next_sector + i).collect();
        let difat_sectors: Vec<u32> = (0..d as u32).map(|i| next_sector + f as u32 + i).collect();
        let total = next_sector as usize + f + d;

        let mut fat = vec![FREESECT; f * epf];
        for (start, n) in chains {
            for k in 0..n {
                fat[(start + k) as usize] = if k + 1 == n {
                    ENDOFCHAIN
                } else {
                    start + k + 1
                };
            }
        }
        for &s in &fat_sectors {
            fat[s as usize] = FATSECT;
        }
        for &s in &difat_sectors {
            fat[s as usize] = DIFSECT;
        }

        let mut bytes = vec![0u8; ss * (total + 1)];
        // Header.
        bytes[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
        put_u16(&mut bytes, 24, 0x3e);
        put_u16(&mut bytes, 26, major);
        put_u16(&mut bytes, 28, 0xfffe);
        put_u16(&mut bytes, 30, if major == 3 { 9 } else { 12 });
        put_u16(&mut bytes, 32, 6);
        put_u32(&mut bytes, 40, if major == 4 { n_dir } else { 0 });
        put_u32(&mut bytes, 44, f as u32);
        put_u32(&mut bytes, 48, dir_start);
        put_u32(&mut bytes, 56, 4096);
        put_u32(&mut bytes, 60, minifat_start);
        put_u32(&mut bytes, 64, n_mf);
        put_u32(
            &mut bytes,
            68,
            difat_sectors.first().copied().unwrap_or(ENDOFCHAIN),
        );
        put_u32(&mut bytes, 72, d as u32);
        for i in 0..109 {
            put_u32(
                &mut bytes,
                76 + 4 * i,
                fat_sectors.get(i).copied().unwrap_or(FREESECT),
            );
        }
        let off = |s: u32| (s as usize + 1) * ss;
        // Data streams.
        for e in ents.iter().skip(1) {
            if !e.storage && e.data.len() >= 4096 {
                bytes[off(e.start)..off(e.start) + e.data.len()].copy_from_slice(&e.data);
            }
        }
        if n_ms > 0 {
            bytes[off(ministream_start)..off(ministream_start) + mini_data.len()]
                .copy_from_slice(&mini_data);
        }
        for (i, v) in mini_fat.iter().enumerate() {
            put_u32(&mut bytes, off(minifat_start) + 4 * i, *v);
        }
        // Directory.
        for (i, e) in ents.iter().enumerate() {
            let o = off(dir_start) + i * 128;
            let units: Vec<u16> = e.name.encode_utf16().collect();
            for (k, u) in units.iter().enumerate() {
                put_u16(&mut bytes, o + 2 * k, *u);
            }
            put_u16(&mut bytes, o + 64, (units.len() as u16 + 1) * 2);
            bytes[o + 66] = if i == 0 {
                5
            } else if e.storage {
                1
            } else {
                2
            };
            bytes[o + 67] = 1;
            put_u32(&mut bytes, o + 68, e.left);
            put_u32(&mut bytes, o + 72, e.right);
            put_u32(&mut bytes, o + 76, e.child);
            bytes[o + 80..o + 96].copy_from_slice(&e.clsid);
            put_u32(&mut bytes, o + 116, e.start);
            put_u64(&mut bytes, o + 120, e.size);
        }
        // Unused directory slots: free entries (NOSTREAM links).
        for i in ents.len()..(n_dir as usize * ss / 128) {
            let o = off(dir_start) + i * 128;
            put_u32(&mut bytes, o + 68, NOSTREAM);
            put_u32(&mut bytes, o + 72, NOSTREAM);
            put_u32(&mut bytes, o + 76, NOSTREAM);
        }
        // FAT and DIFAT sectors.
        for (i, v) in fat.iter().enumerate() {
            put_u32(&mut bytes, off(fat_sectors[0]) + 4 * i, *v);
        }
        for (k, &s) in difat_sectors.iter().enumerate() {
            for j in 0..epf - 1 {
                let v = fat_sectors
                    .get(109 + k * (epf - 1) + j)
                    .copied()
                    .unwrap_or(FREESECT);
                put_u32(&mut bytes, off(s) + 4 * j, v);
            }
            let next = difat_sectors.get(k + 1).copied().unwrap_or(ENDOFCHAIN);
            put_u32(&mut bytes, off(s) + 4 * (epf - 1), next);
        }
        Built {
            bytes,
            version: self.version,
            ss,
            sector_count: total as u32,
            fat_sectors,
            difat_sectors,
            dir_start,
            dir_sectors: n_dir,
            minifat_start,
            ministream_start,
            paths: ents
                .iter()
                .enumerate()
                .map(|(i, e)| (e.path.clone(), i as u32))
                .collect(),
            starts: ents.iter().map(|e| (e.path.clone(), e.start)).collect(),
        }
    }
}

impl Built {
    pub fn sector_off(&self, s: u32) -> usize {
        (s as usize + 1) * self.ss
    }
    /// Byte offset of the FAT slot describing sector `s`.
    pub fn fat_slot(&self, s: u32) -> usize {
        let epf = (self.ss / 4) as u32;
        self.sector_off(self.fat_sectors[(s / epf) as usize]) + 4 * (s % epf) as usize
    }
    pub fn set_fat(&mut self, s: u32, v: u32) {
        let o = self.fat_slot(s);
        put_u32(&mut self.bytes, o, v);
    }
    pub fn get_fat(&self, s: u32) -> u32 {
        get_u32(&self.bytes, self.fat_slot(s))
    }
    pub fn id_of(&self, path: &str) -> u32 {
        self.paths.iter().find(|(p, _)| p == path).expect("path").1
    }
    pub fn start_of(&self, path: &str) -> u32 {
        self.starts.iter().find(|(p, _)| p == path).expect("path").1
    }
    /// Byte offset of a directory entry (all test directories are contiguous).
    pub fn entry_off(&self, id: u32) -> usize {
        self.sector_off(self.dir_start) + id as usize * 128
    }
    pub fn set_u32(&mut self, o: usize, v: u32) {
        put_u32(&mut self.bytes, o, v);
    }
    pub fn set_u16(&mut self, o: usize, v: u16) {
        put_u16(&mut self.bytes, o, v);
    }
    pub fn set_u64(&mut self, o: usize, v: u64) {
        put_u64(&mut self.bytes, o, v);
    }
    pub fn set_entry_u32(&mut self, path: &str, field: usize, v: u32) {
        let o = self.entry_off(self.id_of(path)) + field;
        put_u32(&mut self.bytes, o, v);
    }
}

/// Small deterministic generator (same family as the sibling crates' tests).
pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}
