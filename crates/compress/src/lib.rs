// SPDX-License-Identifier: AGPL-3.0-or-later
//! Safe, dependency-free DEFLATE and zlib support.
//!
//! The inflater is incremental: it retains a 32 KiB history window and parser
//! state, but never buffers the whole compressed stream or decompressed output.

#![forbid(unsafe_code)]

const WINDOW: usize = 32 * 1024;
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CODE_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Deflate,
    Zlib,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    Fast,
    Normal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    NeedInput,
    NeedOutput,
    Done,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Progress {
    pub consumed: usize,
    pub produced: usize,
    pub status: Status,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InflateError {
    InvalidData(&'static str),
    UnexpectedEof,
    OutputLimit,
}

impl std::fmt::Display for InflateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidData(s) => write!(f, "invalid DEFLATE data: {s}"),
            Self::UnexpectedEof => f.write_str("unexpected end of compressed stream"),
            Self::OutputLimit => f.write_str("decompressed output limit exceeded"),
        }
    }
}
impl std::error::Error for InflateError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompressError {
    OutputTooSmall { needed: usize },
}
impl std::fmt::Display for CompressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutputTooSmall { needed } => write!(f, "output buffer needs {needed} bytes"),
        }
    }
}
impl std::error::Error for CompressError {}

#[derive(Clone, Debug)]
struct Huffman {
    by_len: Vec<Vec<(u16, u16)>>,
    max: u8,
}
impl Huffman {
    fn from_lengths(
        lengths: &[u8],
        allow_empty: bool,
        allow_single_incomplete: bool,
    ) -> Result<Self, InflateError> {
        let max = lengths.iter().copied().max().unwrap_or(0);
        if max == 0 {
            if allow_empty {
                return Ok(Self {
                    by_len: vec![Vec::new()],
                    max: 0,
                });
            }
            return Err(InflateError::InvalidData("empty Huffman tree"));
        }
        if max > 15 {
            return Err(InflateError::InvalidData(
                "Huffman code is longer than 15 bits",
            ));
        }
        let mut count = [0i32; 16];
        for &l in lengths {
            if l > 0 {
                count[l as usize] += 1;
            }
        }
        let mut left = 1i32;
        for &used in count.iter().skip(1) {
            left = (left << 1) - used;
            if left < 0 {
                return Err(InflateError::InvalidData("oversubscribed Huffman tree"));
            }
        }
        if left != 0 && !(allow_single_incomplete && max == 1 && count[1] == 1) {
            return Err(InflateError::InvalidData("incomplete Huffman tree"));
        }
        let mut next = [0u16; 16];
        let mut code = 0u16;
        for bits in 1..=15 {
            code = (code + count[bits - 1] as u16) << 1;
            next[bits] = code;
        }
        let mut by_len = vec![Vec::new(); max as usize + 1];
        for (sym, &len) in lengths.iter().enumerate() {
            if len != 0 {
                let c = next[len as usize];
                next[len as usize] += 1;
                by_len[len as usize].push((reverse_bits(c, len), sym as u16));
            }
        }
        Ok(Self { by_len, max })
    }
}

fn reverse_bits(mut x: u16, n: u8) -> u16 {
    let mut r = 0;
    for _ in 0..n {
        r = (r << 1) | (x & 1);
        x >>= 1;
    }
    r
}

#[derive(Clone, Debug)]
enum Phase {
    ZlibHeader {
        bytes: [u8; 2],
        have: u8,
    },
    BlockHeader,
    StoredLen {
        bytes: [u8; 4],
        have: u8,
    },
    Stored {
        remaining: usize,
    },
    DynamicHead {
        values: [u16; 3],
        index: u8,
    },
    DynamicCodeLengths {
        lengths: [u8; 19],
        index: usize,
        total: usize,
        hlit: usize,
        hdist: usize,
    },
    DynamicLengths {
        code: Huffman,
        lengths: Vec<u8>,
        total: usize,
        hlit: usize,
        hdist: usize,
        repeat: Option<(u8, u8)>,
    },
    Codes {
        lit: Huffman,
        dist: Huffman,
        op: CodeOp,
    },
    Copy {
        lit: Huffman,
        dist: Huffman,
        distance: usize,
        remaining: usize,
    },
    ZlibChecksum {
        bytes: [u8; 4],
        have: u8,
    },
    Done,
    Failed,
}
#[derive(Clone, Copy, Debug)]
enum CodeOp {
    Symbol,
    Literal(u8),
    LengthExtra(usize),
    DistanceSymbol(usize),
    DistanceExtra(usize, usize, u8),
}

/// A bounded, caller-driven DEFLATE or zlib decoder.
pub struct InflateState {
    phase: Phase,
    format: Format,
    max_output: u64,
    output_count: u64,
    bits: u64,
    bit_count: u8,
    window: [u8; WINDOW],
    win_pos: usize,
    adler_a: u32,
    adler_b: u32,
    final_block: bool,
    last_code: u16,
    last_len: u8,
}

impl InflateState {
    pub fn new(format: Format, max_output: u64) -> Self {
        Self {
            phase: if format == Format::Zlib {
                Phase::ZlibHeader {
                    bytes: [0; 2],
                    have: 0,
                }
            } else {
                Phase::BlockHeader
            },
            format,
            max_output,
            output_count: 0,
            bits: 0,
            bit_count: 0,
            window: [0; WINDOW],
            win_pos: 0,
            adler_a: 1,
            adler_b: 0,
            final_block: false,
            last_code: 0,
            last_len: 0,
        }
    }

    /// Consume as much input and produce as much output as possible. Set
    /// `finish` once no more input will arrive.
    pub fn push(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        finish: bool,
    ) -> Result<Progress, InflateError> {
        let mut ip = 0usize;
        let mut op = 0usize;
        loop {
            let phase = std::mem::replace(&mut self.phase, Phase::Failed);
            match phase {
                Phase::Done => {
                    self.phase = Phase::Done;
                    return Ok(Progress {
                        consumed: ip,
                        produced: op,
                        status: Status::Done,
                    });
                }
                Phase::Failed => {
                    return Err(InflateError::InvalidData("decoder is in a failed state"));
                }
                Phase::ZlibHeader {
                    mut bytes,
                    mut have,
                } => {
                    while have < 2 && ip < input.len() {
                        bytes[have as usize] = input[ip];
                        have += 1;
                        ip += 1;
                    }
                    if have < 2 {
                        self.phase = Phase::ZlibHeader { bytes, have };
                        return self.wait(ip, op, finish);
                    }
                    let cmf = bytes[0];
                    let flg = bytes[1];
                    if cmf & 15 != 8
                        || cmf >> 4 > 7
                        || ((cmf as u16) * 256 + flg as u16) % 31 != 0
                        || flg & 32 != 0
                    {
                        return self.fail(InflateError::InvalidData(
                            "unsupported or invalid zlib header",
                        ));
                    }
                    self.phase = Phase::BlockHeader;
                }
                Phase::BlockHeader => {
                    let Some(v) = self.read_bits(3, input, &mut ip) else {
                        self.phase = Phase::BlockHeader;
                        return self.wait(ip, op, finish);
                    };
                    self.final_block = v & 1 != 0;
                    match (v >> 1) & 3 {
                        0 => {
                            self.drop_to_byte();
                            self.phase = Phase::StoredLen {
                                bytes: [0; 4],
                                have: 0,
                            };
                        }
                        1 => {
                            self.phase = Phase::Codes {
                                lit: fixed_lit()?,
                                dist: fixed_dist()?,
                                op: CodeOp::Symbol,
                            }
                        }
                        2 => {
                            self.phase = Phase::DynamicHead {
                                values: [0; 3],
                                index: 0,
                            }
                        }
                        _ => return self.fail(InflateError::InvalidData("reserved block type")),
                    }
                }
                Phase::StoredLen {
                    mut bytes,
                    mut have,
                } => {
                    while have < 4 {
                        let Some(b) = self.read_bits(8, input, &mut ip) else {
                            self.phase = Phase::StoredLen { bytes, have };
                            return self.wait(ip, op, finish);
                        };
                        bytes[have as usize] = b as u8;
                        have += 1;
                    }
                    let len = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
                    if len != (!u16::from_le_bytes([bytes[2], bytes[3]])) as usize {
                        return self
                            .fail(InflateError::InvalidData("stored block length mismatch"));
                    }
                    self.phase = Phase::Stored { remaining: len };
                }
                Phase::Stored { mut remaining } => {
                    while remaining != 0 && op < output.len() {
                        let Some(v) = self.read_bits(8, input, &mut ip) else {
                            self.phase = Phase::Stored { remaining };
                            return self.wait(ip, op, finish);
                        };
                        if !self.emit(v as u8, output, &mut op) {
                            return self.fail(InflateError::OutputLimit);
                        }
                        remaining -= 1;
                    }
                    if remaining != 0 {
                        self.phase = Phase::Stored { remaining };
                        return Ok(Progress {
                            consumed: ip,
                            produced: op,
                            status: Status::NeedOutput,
                        });
                    }
                    self.end_block();
                }
                Phase::DynamicHead {
                    mut values,
                    mut index,
                } => {
                    while index < 3 {
                        let n = [5, 5, 4][index as usize];
                        let Some(v) = self.read_bits(n, input, &mut ip) else {
                            self.phase = Phase::DynamicHead { values, index };
                            return self.wait(ip, op, finish);
                        };
                        values[index as usize] = v as u16;
                        index += 1;
                    }
                    let hlit = values[0] as usize + 257;
                    let hdist = values[1] as usize + 1;
                    let hclen = values[2] as usize + 4;
                    if hlit > 286 || hdist > 32 {
                        return self
                            .fail(InflateError::InvalidData("dynamic tree count out of range"));
                    }
                    self.phase = Phase::DynamicCodeLengths {
                        lengths: [0; 19],
                        index: 0,
                        total: hclen,
                        hlit,
                        hdist,
                    };
                }
                Phase::DynamicCodeLengths {
                    mut lengths,
                    mut index,
                    total,
                    hlit,
                    hdist,
                } => {
                    while index < total {
                        let Some(v) = self.read_bits(3, input, &mut ip) else {
                            self.phase = Phase::DynamicCodeLengths {
                                lengths,
                                index,
                                total,
                                hlit,
                                hdist,
                            };
                            return self.wait(ip, op, finish);
                        };
                        lengths[CODE_ORDER[index]] = v as u8;
                        index += 1;
                    }
                    let code = Huffman::from_lengths(&lengths, false, false)?;
                    self.phase = Phase::DynamicLengths {
                        code,
                        lengths: Vec::with_capacity(hlit + hdist),
                        total: hlit + hdist,
                        hlit,
                        hdist,
                        repeat: None,
                    };
                }
                Phase::DynamicLengths {
                    code,
                    mut lengths,
                    total,
                    hlit,
                    hdist,
                    mut repeat,
                } => {
                    if let Some((value, remaining)) = repeat.take() {
                        if value >= 16 {
                            let symbol = value;
                            let extra = if symbol == 16 {
                                2
                            } else if symbol == 17 {
                                3
                            } else {
                                7
                            };
                            let base = if symbol <= 17 { 3 } else { 11 };
                            let Some(x) = self.read_bits(extra, input, &mut ip) else {
                                self.phase = Phase::DynamicLengths {
                                    code,
                                    lengths,
                                    total,
                                    hlit,
                                    hdist,
                                    repeat: Some((value, remaining)),
                                };
                                return self.wait(ip, op, finish);
                            };
                            let count = base + x as usize;
                            let repeated = if symbol == 16 { remaining } else { 0 };
                            if lengths.len() + count > total {
                                return self.fail(InflateError::InvalidData(
                                    "code length repeat exceeds tree",
                                ));
                            }
                            lengths.extend(std::iter::repeat_n(repeated, count));
                        } else {
                            lengths.extend(std::iter::repeat_n(value, remaining as usize));
                        }
                    }
                    while lengths.len() < total {
                        let Some(sym) = self.decode_symbol(&code, input, &mut ip)? else {
                            self.phase = Phase::DynamicLengths {
                                code,
                                lengths,
                                total,
                                hlit,
                                hdist,
                                repeat: None,
                            };
                            return self.wait(ip, op, finish);
                        };
                        match sym {
                            0..=15 => lengths.push(sym as u8),
                            16 => {
                                if lengths.is_empty() {
                                    return self.fail(InflateError::InvalidData(
                                        "repeat has no previous code length",
                                    ));
                                }
                                let value = *lengths.last().unwrap_or(&0);
                                let Some(x) = self.read_bits(2, input, &mut ip) else {
                                    self.phase = Phase::DynamicLengths {
                                        code,
                                        lengths,
                                        total,
                                        hlit,
                                        hdist,
                                        repeat: Some((16, value)),
                                    };
                                    return self.wait(ip, op, finish);
                                };
                                let count = x as usize + 3;
                                if lengths.len() + count > total {
                                    return self.fail(InflateError::InvalidData(
                                        "code length repeat exceeds tree",
                                    ));
                                }
                                lengths.extend(std::iter::repeat_n(value, count));
                            }
                            17 | 18 => {
                                let extra = if sym == 17 { 3 } else { 7 };
                                let base = if sym == 17 { 3 } else { 11 };
                                let Some(x) = self.read_bits(extra, input, &mut ip) else {
                                    self.phase = Phase::DynamicLengths {
                                        code,
                                        lengths,
                                        total,
                                        hlit,
                                        hdist,
                                        repeat: Some((sym as u8, 0)),
                                    };
                                    return self.wait(ip, op, finish);
                                };
                                let count = base + x as usize;
                                if lengths.len() + count > total {
                                    return self.fail(InflateError::InvalidData(
                                        "zero repeat exceeds tree",
                                    ));
                                }
                                lengths.extend(std::iter::repeat_n(0, count));
                            }
                            _ => {
                                return self
                                    .fail(InflateError::InvalidData("invalid code-length symbol"));
                            }
                        }
                    }
                    if lengths[256] == 0 {
                        return self.fail(InflateError::InvalidData(
                            "literal tree lacks end-of-block code",
                        ));
                    }
                    let lit = Huffman::from_lengths(&lengths[..hlit], false, true)?;
                    let dist = Huffman::from_lengths(&lengths[hlit..], true, true)?;
                    self.phase = Phase::Codes {
                        lit,
                        dist,
                        op: CodeOp::Symbol,
                    };
                }
                Phase::Codes {
                    lit,
                    dist,
                    op: mut code_op,
                } => match code_op {
                    CodeOp::Symbol => {
                        let Some(sym) = self.decode_symbol(&lit, input, &mut ip)? else {
                            self.phase = Phase::Codes {
                                lit,
                                dist,
                                op: code_op,
                            };
                            return self.wait(ip, op, finish);
                        };
                        match sym {
                            0..=255 => {
                                code_op = CodeOp::Literal(sym as u8);
                                self.phase = Phase::Codes {
                                    lit,
                                    dist,
                                    op: code_op,
                                };
                            }
                            256 => self.end_block(),
                            257..=285 => {
                                let i = (sym - 257) as usize;
                                code_op = CodeOp::LengthExtra(i);
                                self.phase = Phase::Codes {
                                    lit,
                                    dist,
                                    op: code_op,
                                };
                            }
                            _ => {
                                return self.fail(InflateError::InvalidData(
                                    "reserved literal/length code",
                                ));
                            }
                        }
                    }
                    CodeOp::Literal(b) => {
                        if op == output.len() {
                            self.phase = Phase::Codes {
                                lit,
                                dist,
                                op: code_op,
                            };
                            return Ok(Progress {
                                consumed: ip,
                                produced: op,
                                status: Status::NeedOutput,
                            });
                        }
                        if !self.emit(b, output, &mut op) {
                            return self.fail(InflateError::OutputLimit);
                        }
                        self.phase = Phase::Codes {
                            lit,
                            dist,
                            op: CodeOp::Symbol,
                        };
                    }
                    CodeOp::LengthExtra(i) => {
                        let Some(x) = self.read_bits(LENGTH_EXTRA[i], input, &mut ip) else {
                            self.phase = Phase::Codes {
                                lit,
                                dist,
                                op: code_op,
                            };
                            return self.wait(ip, op, finish);
                        };
                        let len = LENGTH_BASE[i] as usize + x as usize;
                        self.phase = Phase::Codes {
                            lit,
                            dist,
                            op: CodeOp::DistanceSymbol(len),
                        };
                    }
                    CodeOp::DistanceSymbol(len) => {
                        let Some(ds) = self.decode_symbol(&dist, input, &mut ip)? else {
                            self.phase = Phase::Codes {
                                lit,
                                dist,
                                op: code_op,
                            };
                            return self.wait(ip, op, finish);
                        };
                        if ds >= 30 {
                            return self.fail(InflateError::InvalidData("reserved distance code"));
                        }
                        self.phase = Phase::Codes {
                            lit,
                            dist,
                            op: CodeOp::DistanceExtra(len, ds as usize, DIST_EXTRA[ds as usize]),
                        };
                    }
                    CodeOp::DistanceExtra(len, ds, extra) => {
                        let Some(dx) = self.read_bits(extra, input, &mut ip) else {
                            self.phase = Phase::Codes {
                                lit,
                                dist,
                                op: code_op,
                            };
                            return self.wait(ip, op, finish);
                        };
                        let distance = DIST_BASE[ds] as usize + dx as usize;
                        if distance as u64 > self.output_count.min(WINDOW as u64) {
                            return self.fail(InflateError::InvalidData(
                                "distance exceeds available history",
                            ));
                        }
                        if len as u64 > self.max_output.saturating_sub(self.output_count) {
                            return self.fail(InflateError::OutputLimit);
                        }
                        self.phase = Phase::Copy {
                            lit,
                            dist,
                            distance,
                            remaining: len,
                        };
                    }
                },
                Phase::Copy {
                    lit,
                    dist,
                    distance,
                    mut remaining,
                } => {
                    while remaining != 0 && op < output.len() {
                        let b = self.window[(self.win_pos + WINDOW - distance) % WINDOW];
                        if !self.emit(b, output, &mut op) {
                            return self.fail(InflateError::OutputLimit);
                        }
                        remaining -= 1;
                    }
                    if remaining != 0 {
                        self.phase = Phase::Copy {
                            lit,
                            dist,
                            distance,
                            remaining,
                        };
                        return Ok(Progress {
                            consumed: ip,
                            produced: op,
                            status: Status::NeedOutput,
                        });
                    }
                    self.phase = Phase::Codes {
                        lit,
                        dist,
                        op: CodeOp::Symbol,
                    };
                }
                Phase::ZlibChecksum {
                    mut bytes,
                    mut have,
                } => {
                    while have < 4 {
                        let Some(v) = self.read_bits(8, input, &mut ip) else {
                            self.phase = Phase::ZlibChecksum { bytes, have };
                            return self.wait(ip, op, finish);
                        };
                        bytes[have as usize] = v as u8;
                        have += 1;
                    }
                    let expected = u32::from_be_bytes(bytes);
                    let actual = (self.adler_b << 16) | self.adler_a;
                    if expected != actual {
                        return self.fail(InflateError::InvalidData("zlib Adler-32 mismatch"));
                    }
                    self.phase = Phase::Done;
                }
            }
            if op == output.len() && !matches!(self.phase, Phase::Done | Phase::ZlibChecksum { .. })
            {
                return Ok(Progress {
                    consumed: ip,
                    produced: op,
                    status: Status::NeedOutput,
                });
            }
        }
    }

    fn read_bits(&mut self, n: u8, input: &[u8], ip: &mut usize) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        while self.bit_count < n {
            if *ip == input.len() {
                return None;
            }
            self.bits |= (input[*ip] as u64) << self.bit_count;
            self.bit_count += 8;
            *ip += 1;
        }
        let v = (self.bits & ((1u64 << n) - 1)) as u32;
        self.bits >>= n;
        self.bit_count -= n;
        Some(v)
    }
    fn drop_to_byte(&mut self) {
        let n = self.bit_count % 8;
        self.bits >>= n;
        self.bit_count -= n;
    }
    fn decode_symbol(
        &mut self,
        tree: &Huffman,
        input: &[u8],
        ip: &mut usize,
    ) -> Result<Option<u16>, InflateError> {
        if tree.max == 0 {
            return Err(InflateError::InvalidData("missing distance tree"));
        }
        while self.last_len < tree.max {
            let Some(bit) = self.read_bits(1, input, ip) else {
                return Ok(None);
            };
            self.last_code |= (bit as u16) << self.last_len;
            self.last_len += 1;
            if let Some((_, sym)) = tree.by_len[self.last_len as usize]
                .iter()
                .find(|(c, _)| *c == self.last_code)
            {
                self.last_code = 0;
                self.last_len = 0;
                return Ok(Some(*sym));
            }
        }
        self.last_code = 0;
        self.last_len = 0;
        Err(InflateError::InvalidData("invalid Huffman code"))
    }
    fn emit(&mut self, b: u8, out: &mut [u8], op: &mut usize) -> bool {
        if self.output_count >= self.max_output || *op >= out.len() {
            return false;
        }
        out[*op] = b;
        *op += 1;
        self.window[self.win_pos] = b;
        self.win_pos = (self.win_pos + 1) % WINDOW;
        self.output_count += 1;
        self.adler_a = (self.adler_a + b as u32) % 65521;
        self.adler_b = (self.adler_b + self.adler_a) % 65521;
        true
    }
    fn end_block(&mut self) {
        if self.final_block {
            self.drop_to_byte();
            self.phase = if self.format == Format::Zlib {
                Phase::ZlibChecksum {
                    bytes: [0; 4],
                    have: 0,
                }
            } else {
                Phase::Done
            };
        } else {
            self.phase = Phase::BlockHeader;
        }
    }
    fn wait(
        &mut self,
        consumed: usize,
        produced: usize,
        finish: bool,
    ) -> Result<Progress, InflateError> {
        if finish {
            self.phase = Phase::Failed;
            Err(InflateError::UnexpectedEof)
        } else {
            Ok(Progress {
                consumed,
                produced,
                status: Status::NeedInput,
            })
        }
    }
    fn fail<T>(&mut self, e: InflateError) -> Result<T, InflateError> {
        self.phase = Phase::Failed;
        Err(e)
    }
}

fn fixed_lit() -> Result<Huffman, InflateError> {
    let mut v = vec![0; 288];
    v[..=143].fill(8);
    v[144..=255].fill(9);
    v[256..=279].fill(7);
    v[280..].fill(8);
    Huffman::from_lengths(&v, false, true)
}
fn fixed_dist() -> Result<Huffman, InflateError> {
    Huffman::from_lengths(&[5; 32], false, true)
}

/// Adler-32 checksum used by the zlib wrapper.
pub fn adler32(bytes: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for chunk in bytes.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}
/// CRC-32/ISO-HDLC primitive used by ZIP and other consumers.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = (c >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(c & 1)));
        }
    }
    !c
}

/// Compress one complete input as raw DEFLATE, writing into caller storage.
pub fn compress(input: &[u8], level: Level, output: &mut [u8]) -> Result<usize, CompressError> {
    compress_with_format(input, Format::Deflate, level, output)
}

/// Compress using the explicitly selected raw DEFLATE or zlib framing.
pub fn compress_with_format(
    input: &[u8],
    format: Format,
    level: Level,
    output: &mut [u8],
) -> Result<usize, CompressError> {
    let needed = match format {
        Format::Deflate => encode_dynamic(input, level, output),
        Format::Zlib => {
            if output.len() >= 2 {
                output[..2]
                    .copy_from_slice(&[0x78, if level == Level::Fast { 0x01 } else { 0x9c }]);
            }
            let raw_len = if output.len() >= 2 {
                encode_dynamic(input, level, &mut output[2..])
            } else {
                encode_dynamic(input, level, &mut [])
            };
            let total = 2 + raw_len + 4;
            if total <= output.len() {
                output[2 + raw_len..total].copy_from_slice(&adler32(input).to_be_bytes());
            }
            total
        }
    };
    if needed > output.len() {
        return Err(CompressError::OutputTooSmall { needed });
    }
    Ok(needed)
}

/// Compress and wrap in an RFC 1950 zlib stream.
pub fn compress_zlib(
    input: &[u8],
    level: Level,
    output: &mut [u8],
) -> Result<usize, CompressError> {
    compress_with_format(input, Format::Zlib, level, output)
}

#[derive(Clone, Copy)]
enum Token {
    Lit(u8),
    Match(usize, usize),
}
#[derive(Clone, Copy)]
struct Code {
    bits: u16,
    len: u8,
}
type HuffmanNode = (u64, Option<usize>, Option<usize>, Option<usize>);
fn encode_dynamic(input: &[u8], level: Level, output: &mut [u8]) -> usize {
    let tokens = lz77(input, if level == Level::Fast { 32 } else { 256 });
    let mut lf = vec![0u32; 286];
    let mut df = vec![0u32; 30];
    for t in &tokens {
        match *t {
            Token::Lit(b) => lf[b as usize] += 1,
            Token::Match(l, d) => {
                let (i, _) = length_code(l);
                lf[257 + i] += 1;
                let (j, _) = distance_code(d);
                df[j] += 1;
            }
        }
    }
    lf[256] += 1;
    if df.iter().all(|&x| x == 0) {
        df[0] = 1;
    }
    let ll = make_lengths(&lf, 15);
    let dl = make_lengths(&df, 15);
    let ltree = make_codes(&ll);
    let dtree = make_codes(&dl);
    let nl = ll.iter().rposition(|&x| x != 0).unwrap_or(256) + 1;
    let nd = dl.iter().rposition(|&x| x != 0).unwrap_or(0) + 1;
    let mut seq = ll[..nl].to_vec();
    seq.extend_from_slice(&dl[..nd]);
    let mut cf = vec![0u32; 19];
    for &l in &seq {
        cf[l as usize] += 1;
    }
    let cl = make_lengths(&cf, 7);
    let ctree = make_codes(&cl);
    let mut h = BitWriter::new(output);
    h.write(1, 1);
    h.write(2, 2); // final dynamic block
    let nc = CODE_ORDER
        .iter()
        .rposition(|&i| cl[i] != 0)
        .map(|x| x + 1)
        .unwrap_or(4)
        .max(4);
    h.write((nl - 257) as u32, 5);
    h.write((nd - 1) as u32, 5);
    h.write((nc - 4) as u32, 4);
    for i in 0..nc {
        h.write(cl[CODE_ORDER[i]] as u32, 3);
    }
    for &l in &seq {
        emit_code(&mut h, &ctree, l as usize);
    }
    for t in tokens {
        match t {
            Token::Lit(b) => emit_code(&mut h, &ltree, b as usize),
            Token::Match(l, d) => {
                let (i, x) = length_code(l);
                emit_code(&mut h, &ltree, 257 + i);
                if x.1 > 0 {
                    h.write(x.0, x.1);
                }
                let (j, y) = distance_code(d);
                emit_code(&mut h, &dtree, j);
                if y.1 > 0 {
                    h.write(y.0, y.1);
                }
            }
        }
    }
    emit_code(&mut h, &ltree, 256);
    h.finish()
}

fn lz77(input: &[u8], budget: usize) -> Vec<Token> {
    let mut head = vec![usize::MAX; 65536];
    let mut prev = vec![usize::MAX; input.len()];
    let mut out = Vec::new();
    let mut p = 0;
    while p < input.len() {
        let mut best = (0, 0);
        if p + 2 < input.len() {
            let hash = hash3(input, p);
            let mut q = head[hash];
            let mut tries = budget;
            while q != usize::MAX && p - q <= WINDOW && tries > 0 {
                let mut n = 0;
                while n < 258 && p + n < input.len() && input[q + n] == input[p + n] {
                    n += 1;
                }
                if n >= 3 && n > best.0 {
                    best = (n, p - q);
                    if n == 258 {
                        break;
                    }
                }
                q = prev[q];
                tries -= 1;
            }
        }
        if best.0 >= 3 {
            out.push(Token::Match(best.0, best.1));
            for i in 0..best.0 {
                if p + i + 2 < input.len() {
                    let h = hash3(input, p + i);
                    prev[p + i] = head[h];
                    head[h] = p + i;
                }
            }
            p += best.0;
        } else {
            out.push(Token::Lit(input[p]));
            if p + 2 < input.len() {
                let h = hash3(input, p);
                prev[p] = head[h];
                head[h] = p;
            }
            p += 1;
        }
    }
    out
}
fn hash3(x: &[u8], p: usize) -> usize {
    (((x[p] as usize * 251 + x[p + 1] as usize) * 251 + x[p + 2] as usize) >> 5) & 65535
}
fn length_code(l: usize) -> (usize, (u32, u8)) {
    for i in 0..29 {
        let base = LENGTH_BASE[i] as usize;
        let top = if i == 28 {
            258
        } else {
            base + (1usize << LENGTH_EXTRA[i]) - 1
        };
        if l <= top {
            return (i, ((l - base) as u32, LENGTH_EXTRA[i]));
        }
    }
    (28, (0, 0))
}
fn distance_code(d: usize) -> (usize, (u32, u8)) {
    for i in 0..30 {
        let base = DIST_BASE[i] as usize;
        let top = base + (1usize << DIST_EXTRA[i]) - 1;
        if d <= top {
            return (i, ((d - base) as u32, DIST_EXTRA[i]));
        }
    }
    (29, (0, 13))
}
fn make_lengths(freq: &[u32], max_bits: usize) -> Vec<u8> {
    let mut active: Vec<(u64, usize)> = freq
        .iter()
        .enumerate()
        .filter(|(_, f)| **f > 0)
        .map(|(i, f)| (*f as u64, i))
        .collect();
    if active.is_empty() {
        active.push((1, 0));
    }
    if active.len() == 1 {
        let first = active[0].1;
        let second = if first == 0 { 1 } else { 0 };
        let mut out = vec![0; freq.len()];
        out[first] = 1;
        out[second] = 1;
        return out;
    }
    let mut nodes: Vec<HuffmanNode> = active
        .iter()
        .map(|(f, i)| (*f, None, None, Some(*i)))
        .collect();
    active = active
        .iter()
        .enumerate()
        .map(|(n, (f, _))| (*f, n))
        .collect();
    while active.len() > 1 {
        active.sort_by_key(|x| x.0);
        let (fa, a) = active.remove(0);
        let (fb, b) = active.remove(0);
        let n = nodes.len();
        nodes.push((fa + fb, Some(a), Some(b), None));
        active.push((fa + fb, n));
    }
    let mut lens = vec![0u8; freq.len()];
    let mut stack = vec![(active[0].1, 0usize)];
    let mut max_seen = 0;
    while let Some((n, depth)) = stack.pop() {
        let node = nodes[n];
        if let Some(sym) = node.3 {
            lens[sym] = depth.max(1) as u8;
            max_seen = max_seen.max(depth.max(1));
        } else {
            stack.push((node.1.unwrap_or(0), depth + 1));
            stack.push((node.2.unwrap_or(0), depth + 1));
        }
    }
    if max_seen <= max_bits {
        return lens;
    }
    // Length limiting: clamp every code to `max_bits`, then repair the Kraft
    // sum from the leaf counts. Clamping alone over-subscribes the code when
    // leaves sit more than one level below the limit, so the sum is measured
    // rather than inferred from an overflow count.
    let mut count = vec![0u64; max_seen + 1];
    for &l in &lens {
        if l > 0 {
            count[l as usize] += 1;
        }
    }
    for i in (max_bits + 1)..count.len() {
        count[max_bits] += count[i];
        count[i] = 0;
    }
    count.truncate(max_bits + 1);
    let full = 1u64 << max_bits;
    let mut kraft: u64 = (1..=max_bits).map(|b| count[b] << (max_bits - b)).sum();
    // Over-subscribed: lengthen the deepest code that can still grow.
    while kraft > full {
        let bits = (1..max_bits).rev().find(|&b| count[b] > 0).unwrap_or(1);
        count[bits] -= 1;
        count[bits + 1] += 1;
        kraft -= 1 << (max_bits - bits - 1);
    }
    // Incomplete: shorten the deepest code whose growth still fits.
    while kraft < full {
        let Some(bits) = (2..=max_bits)
            .rev()
            .find(|&b| count[b] > 0 && (1u64 << (max_bits - b)) <= full - kraft)
        else {
            break;
        };
        count[bits] -= 1;
        count[bits - 1] += 1;
        kraft += 1 << (max_bits - bits);
    }
    let mut syms: Vec<usize> = (0..freq.len()).filter(|&i| freq[i] > 0).collect();
    syms.sort_by_key(|&i| (freq[i], i));
    let mut res = vec![0u8; freq.len()];
    let mut idx = 0;
    for bits in (1..=max_bits).rev() {
        for _ in 0..count[bits] {
            if idx < syms.len() {
                res[syms[idx]] = bits as u8;
                idx += 1;
            }
        }
    }
    res
}
fn make_codes(lengths: &[u8]) -> Vec<Code> {
    let mut count = [0u16; 16];
    for &l in lengths {
        if l > 0 {
            count[l as usize] += 1;
        }
    }
    let mut next = [0u16; 16];
    let mut code = 0u16;
    for b in 1..=15 {
        code = (code + count[b - 1]) << 1;
        next[b] = code;
    }
    let mut out = vec![Code { bits: 0, len: 0 }; lengths.len()];
    for (i, &l) in lengths.iter().enumerate() {
        if l > 0 {
            out[i] = Code {
                bits: reverse_bits(next[l as usize], l),
                len: l,
            };
            next[l as usize] += 1;
        }
    }
    out
}
fn emit_code(w: &mut BitWriter<'_>, codes: &[Code], sym: usize) {
    let c = codes[sym];
    w.write(c.bits as u32, c.len);
}
struct BitWriter<'a> {
    output: &'a mut [u8],
    written: usize,
    bits: u64,
    count: u8,
}
impl<'a> BitWriter<'a> {
    fn new(output: &'a mut [u8]) -> Self {
        BitWriter {
            output,
            written: 0,
            bits: 0,
            count: 0,
        }
    }
    fn byte(&mut self, byte: u8) {
        if self.written < self.output.len() {
            self.output[self.written] = byte;
        }
        self.written += 1;
    }
    fn write(&mut self, v: u32, n: u8) {
        self.bits |= (v as u64) << self.count;
        self.count += n;
        while self.count >= 8 {
            self.byte(self.bits as u8);
            self.bits >>= 8;
            self.count -= 8;
        }
    }
    fn finish(mut self) -> usize {
        if self.count > 0 {
            self.byte(self.bits as u8);
        }
        self.written
    }
}

#[cfg(test)]
mod tests {
    use super::make_lengths;

    fn fibonacci(n: usize) -> Vec<u32> {
        let mut freq = vec![1u32; n];
        for i in 2..n {
            freq[i] = freq[i - 1] + freq[i - 2];
        }
        freq
    }

    // Fibonacci frequencies give the deepest possible tree, so they force the
    // length-limiting branch; the limited code must stay complete.
    #[test]
    fn limited_lengths_form_a_complete_code() {
        for (symbols, max_bits) in [(11, 7), (19, 7), (19, 15), (40, 15)] {
            let lens = make_lengths(&fibonacci(symbols), max_bits);
            let kraft: u64 = lens.iter().map(|&l| 1u64 << (max_bits - l as usize)).sum();
            assert!(lens.iter().all(|&l| l >= 1 && l as usize <= max_bits));
            assert_eq!(kraft, 1 << max_bits, "{symbols} symbols, limit {max_bits}");
        }
    }
}
