// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bookmark name rules (US-291).
//!
//! Certain (documented by Microsoft): a name is made of letters, digits and
//! underscores, has no space, starts with a letter and is unique without
//! regard to case. Hypotheses, each isolated in one function below:
//! - the limit is 40 *characters* ([`MAX_NAME_CHARS`]), counted after NFC
//!   normalization; Word probably counts UTF-16 units (a character outside
//!   the BMP would count 2), so such a name may be accepted here and refused
//!   by Word;
//! - "letter" and "digit" are the Unicode classes `Alphabetic` and
//!   `Alphanumeric` (accented letters are accepted);
//! - a name starting with an underscore is hidden (internal: `_Toc…`,
//!   `_GoBack`); users cannot type one, the library can create one with
//!   [`NameMode::Internal`].

use atelier_unicode::case::to_case_folded;
use atelier_unicode::normalization::nfc;
use core::fmt;

/// Maximum length of a name, in characters.
pub const MAX_NAME_CHARS: usize = 40;

/// Who is naming the bookmark.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameMode {
    /// Typed in the Bookmark dialog: must start with a letter.
    User,
    /// Internal bookmark: may (and for a hidden one, must) start with `_`.
    Internal,
}

/// Why a name is refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NameError {
    Empty,
    /// The first character is not a letter (or an underscore, internal mode).
    BadFirstChar(char),
    /// A character other than a letter, a digit or an underscore (a space...).
    BadChar {
        ch: char,
        index: usize,
    },
    TooLong {
        chars: usize,
    },
    /// Internal mode: the name must start with an underscore.
    NotInternal,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty bookmark name"),
            Self::BadFirstChar(c) => write!(f, "a bookmark name cannot start with {c:?}"),
            Self::BadChar { ch, index } => write!(f, "invalid character {ch:?} at {index}"),
            Self::TooLong { chars } => {
                write!(f, "{chars} characters, at most {MAX_NAME_CHARS} allowed")
            }
            Self::NotInternal => write!(f, "an internal bookmark name starts with an underscore"),
        }
    }
}
impl std::error::Error for NameError {}

/// The stored form of a typed name: NFC (a decomposed `e` + accent is the
/// same name as the composed letter).
pub fn normalize_name(name: &str) -> String {
    if name.is_ascii() {
        name.to_owned()
    } else {
        nfc(name)
    }
}

/// Checks a name against the rules of Word (see the module documentation).
/// The name is judged in its NFC form ([`normalize_name`]); error indices and
/// the length refer to that form.
pub fn validate_bookmark_name(name: &str, mode: NameMode) -> Result<(), NameError> {
    let name = normalize_name(name);
    let name = name.as_str();
    let mut chars = name.chars().enumerate();
    let Some((_, first)) = chars.next() else {
        return Err(NameError::Empty);
    };
    match mode {
        NameMode::User if !first.is_alphabetic() => return Err(NameError::BadFirstChar(first)),
        NameMode::Internal if first != '_' => return Err(NameError::NotInternal),
        _ => {}
    }
    for (index, ch) in chars {
        if !(ch.is_alphanumeric() || ch == '_') {
            return Err(NameError::BadChar { ch, index });
        }
    }
    let count = name.chars().count();
    if count > MAX_NAME_CHARS {
        return Err(NameError::TooLong { chars: count });
    }
    Ok(())
}

/// Hidden (internal) bookmark: the name starts with an underscore.
pub fn is_hidden_name(name: &str) -> bool {
    name.starts_with('_')
}

/// The key under which two names are "the same bookmark": canonical
/// equivalence and case are ignored, accents are not.
pub fn name_key(name: &str) -> String {
    if name.is_ascii() {
        name.to_ascii_lowercase()
    } else {
        nfc(&to_case_folded(&nfc(name)))
    }
}
