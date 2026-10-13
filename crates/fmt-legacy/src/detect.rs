// SPDX-License-Identifier: AGPL-3.0-or-later
//! Detection of password-protected Microsoft Office documents (US-315).
//!
//! [`detect_protection`] classifies a file without importing it. Every
//! protected Office document of the supported families is a Compound File
//! (CFB): the function opens the container with tight limits, looks at the
//! directory and reads at most a few hundred bytes of one or two streams. It
//! never fails: anything it cannot classify is reported as
//! [`Family::Unknown`] with [`Protection::Undetermined`]. Nothing is
//! decrypted. ODF protection is detected by `atelier-fmt-odf`, not here.
//!
//! Word 97-2003 reuses [`Fib::parse`], the check that `import_doc` runs, so
//! both always agree.

use crate::doc::{DocError, Fib};
use atelier_cfb::{Compound, Limits, is_cfb};

/// Largest number of bytes read from any stream.
const HEAD: usize = 512;
/// Records examined at the start of a BIFF workbook stream.
const MAX_BIFF_RECORDS: usize = 16;

/// Container limits for detection: only the directory is needed.
fn limits() -> Limits {
    Limits {
        max_file_size: 1024 * 1024 * 1024,
        max_entries: 4096,
        max_depth: 8,
        max_stream_size: 1024 * 1024 * 1024,
        max_total_read: 16 * 1024,
    }
}

/// Document family, as far as it can be known without the key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Family {
    /// Encrypted OOXML package (`EncryptionInfo` + `EncryptedPackage`). It is
    /// a docx, xlsx or pptx: which one cannot be known without decrypting.
    EncryptedOoxmlPackage,
    /// Word 97-2003 (`WordDocument` stream).
    Word97,
    /// Excel 97-2003 or 5/95 workbook (`Workbook` or `Book` stream).
    Excel97,
    /// PowerPoint 97-2003 (`PowerPoint Document` stream).
    PowerPoint97,
    /// A Compound File with none of the marks above (including a rights
    /// managed file, whose real family is hidden).
    OtherCompound,
    /// Not a readable Compound File (ZIP, text, empty, truncated, damaged...).
    Unknown,
}

/// Cipher variant, from the version of `EncryptionInfo` or the legacy record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Encryption {
    /// OOXML standard encryption (`EncryptionInfo` 2.2, 3.2 or 4.2).
    Standard,
    /// OOXML agile encryption (4.4, XML descriptor).
    Agile,
    /// OOXML extensible encryption (3.3 or 4.3).
    Extensible,
    /// Binary Office RC4 (Excel `FILEPASS`, `vMajor` 1).
    Rc4,
    /// Binary Office RC4 CryptoAPI (Excel `FILEPASS`, `vMajor` 2 to 4).
    Rc4CryptoApi,
    /// Encrypted, but the variant is not known or not read.
    Unknown,
}

/// Protection state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Protection {
    /// No protection mark found.
    None,
    /// Encrypted with a password.
    Encrypted(Encryption),
    /// XOR obfuscation (Word `fObfuscated`, Excel `FILEPASS` XOR type).
    Obfuscated,
    /// Information Rights Management (a DRM transform, not a password).
    RightsManaged,
    /// Encrypted by a mechanism that is not identified.
    EncryptedUnknown,
    /// The file could not be classified well enough to tell.
    Undetermined,
}

/// Result of [`detect_protection`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Detection {
    pub family: Family,
    pub protection: Protection,
}

impl Detection {
    /// True when the document cannot be opened without a key or a license
    /// (encrypted, obfuscated or rights managed).
    pub fn is_protected(&self) -> bool {
        matches!(
            self.protection,
            Protection::Encrypted(_)
                | Protection::Obfuscated
                | Protection::RightsManaged
                | Protection::EncryptedUnknown
        )
    }
}

fn result(family: Family, protection: Protection) -> Detection {
    Detection { family, protection }
}

/// Reads at most `n` bytes at the start of stream `path`.
fn head(c: &Compound<'_>, path: &str, n: usize) -> Option<Vec<u8>> {
    let e = c.find(path)?;
    if !e.is_stream() {
        return None;
    }
    c.read_entry_range(e, 0, n).ok()
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    let s = b.get(o..o.checked_add(2)?)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    let s = b.get(o..o.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Variant from the version header of `EncryptionInfo` ([MS-OFFCRYPTO] 2.3.1).
fn ooxml_variant(info: Option<&[u8]>) -> Encryption {
    let Some(info) = info else {
        return Encryption::Unknown;
    };
    match (u16_at(info, 0), u16_at(info, 2)) {
        (Some(2..=4), Some(2)) => Encryption::Standard,
        (Some(4), Some(4)) => Encryption::Agile,
        (Some(3 | 4), Some(3)) => Encryption::Extensible,
        _ => Encryption::Unknown,
    }
}

/// Name without the leading control characters that Office puts on some
/// reserved streams (`\x06DataSpaces`, `\x09DRMContent`).
fn plain_name(s: &str) -> &str {
    s.trim_start_matches(|ch: char| ch.is_control())
}

/// Rights managed container: a `DataSpaces` storage holding a DRM transform,
/// or a `DRMContent` stream next to it ([MS-OFFCRYPTO] 2.2.9 and 2.3.3).
fn is_rights_managed(c: &Compound<'_>) -> bool {
    let root = c.root();
    let mut has_spaces = false;
    for e in c.children(root) {
        if e.is_storage() && plain_name(e.name()) == "DataSpaces" {
            has_spaces = true;
            let transforms = c
                .children(e)
                .find(|x| x.is_storage() && plain_name(x.name()) == "TransformInfo");
            if let Some(ti) = transforms {
                if c.children(ti)
                    .any(|t| t.is_storage() && plain_name(t.name()).starts_with("DRM"))
                {
                    return true;
                }
            }
        }
    }
    has_spaces
        && c.children(root)
            .any(|e| e.is_stream() && plain_name(e.name()) == "DRMContent")
}

/// Word: protection from the FIB flags, through the check of `import_doc`.
fn word_protection(word: &[u8]) -> Protection {
    if word.len() < 32 {
        return Protection::Undetermined;
    }
    match Fib::parse(word) {
        Err(DocError::PasswordProtected { obfuscated: true }) => Protection::Obfuscated,
        Err(DocError::PasswordProtected { obfuscated: false }) => {
            Protection::Encrypted(Encryption::Unknown)
        }
        Err(DocError::NotWordDocument) => Protection::Undetermined,
        _ => Protection::None,
    }
}

/// Excel: walks the first records of the workbook globals looking for
/// `FILEPASS` ([MS-XLS] 2.4.117), which follows the BOF record.
fn excel_protection(book: &[u8]) -> Protection {
    let mut pos = 0usize;
    for index in 0..MAX_BIFF_RECORDS {
        let (Some(id), Some(len)) = (u16_at(book, pos), u16_at(book, pos + 2)) else {
            return Protection::Undetermined;
        };
        // The first record must be a BOF.
        if index == 0 && !matches!(id, 0x0809 | 0x0409 | 0x0209 | 0x0009) {
            return Protection::Undetermined;
        }
        let body = pos + 4;
        match id {
            0x002F => {
                return match u16_at(book, body) {
                    Some(0) => Protection::Obfuscated,
                    Some(1) => match u16_at(book, body + 2) {
                        Some(1) => Protection::Encrypted(Encryption::Rc4),
                        Some(2..=4) => Protection::Encrypted(Encryption::Rc4CryptoApi),
                        _ => Protection::Encrypted(Encryption::Unknown),
                    },
                    _ => Protection::EncryptedUnknown,
                };
            }
            // CodePage, Date1904, InterfaceHdr: records that follow FILEPASS.
            0x0042 | 0x0022 | 0x00E1 => return Protection::None,
            _ => {}
        }
        pos = body + usize::from(len);
    }
    Protection::None
}

/// PowerPoint: the header token of `Current User` is 0xF3D1C4DF when the file
/// is encrypted and 0xE391C05F otherwise ([MS-PPT] 2.3.2).
fn powerpoint_protection(c: &Compound<'_>) -> Protection {
    match head(c, "Current User", 24).and_then(|b| u32_at(&b, 12)) {
        Some(0xF3D1_C4DF) => Protection::Encrypted(Encryption::Unknown),
        Some(0xE391_C05F) => Protection::None,
        _ => Protection::Undetermined,
    }
}

/// Classifies `data` without decrypting or importing it. Never fails and
/// never panics; unreadable input gives [`Family::Unknown`] and
/// [`Protection::Undetermined`].
pub fn detect_protection(data: &[u8]) -> Detection {
    let unknown = result(Family::Unknown, Protection::Undetermined);
    if !is_cfb(data) {
        return unknown;
    }
    let Ok(c) = Compound::open(data, limits()) else {
        return unknown;
    };
    if c.is_stream("EncryptionInfo") && c.is_stream("EncryptedPackage") {
        let info = head(&c, "EncryptionInfo", 8);
        return result(
            Family::EncryptedOoxmlPackage,
            Protection::Encrypted(ooxml_variant(info.as_deref())),
        );
    }
    if is_rights_managed(&c) {
        return result(Family::OtherCompound, Protection::RightsManaged);
    }
    if c.is_stream("WordDocument") {
        let p = head(&c, "WordDocument", HEAD)
            .map_or(Protection::Undetermined, |w| word_protection(&w));
        return result(Family::Word97, p);
    }
    for name in ["Workbook", "Book"] {
        if c.is_stream(name) {
            let p = head(&c, name, HEAD).map_or(Protection::Undetermined, |b| excel_protection(&b));
            return result(Family::Excel97, p);
        }
    }
    if c.is_stream("PowerPoint Document") {
        return result(Family::PowerPoint97, powerpoint_protection(&c));
    }
    result(Family::OtherCompound, Protection::Undetermined)
}
