// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deterministic, borrowed font registry and family/style matching.

use crate::embedded::EMBEDDED_FONTS;
use crate::{FontError, FontFace};
use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

// Tokens are local to one process/WASM instance; a cache must not persist them.
// Relaxed ordering suffices: uniqueness, rather than publication, is required.
static NEXT_CACHE_REVISION: AtomicU64 = AtomicU64::new(1);

fn next_cache_revision() -> Result<u64, ManagerError> {
    let mut current = NEXT_CACHE_REVISION.load(AtomicOrdering::Relaxed);
    loop {
        let next = current.checked_add(1).ok_or(ManagerError::IdExhausted)?;
        match NEXT_CACHE_REVISION.compare_exchange_weak(
            current,
            next,
            AtomicOrdering::Relaxed,
            AtomicOrdering::Relaxed,
        ) {
            Ok(_) => return Ok(current),
            Err(observed) => current = observed,
        }
    }
}

/// Stable identifier for one registration. IDs are never reused by a manager.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FontId(u64);
impl FontId {
    /// Numeric representation, useful for logs and protocol adapters.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Where the caller obtained the font bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontSource {
    Embedded,
    System,
    Document,
}

/// OpenType slant classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

/// Last-resort generic family request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenericFamily {
    SansSerif,
    Serif,
    Monospace,
}

/// Family and style request. Family names are tried in the supplied order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontRequest {
    pub families: Vec<String>,
    pub weight: u16,
    pub style: FontStyle,
    pub generic: Option<GenericFamily>,
}
impl FontRequest {
    pub fn new<I, S>(families: I, weight: u16, style: FontStyle) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            families: families.into_iter().map(Into::into).collect(),
            weight,
            style,
            generic: None,
        }
    }
    pub fn with_generic(mut self, generic: GenericFamily) -> Self {
        self.generic = Some(generic);
        self
    }
}

/// Resource ceilings enforced before parsing each registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontManagerLimits {
    pub max_faces: usize,
    pub max_total_bytes: usize,
}
impl Default for FontManagerLimits {
    fn default() -> Self {
        Self {
            max_faces: 1024,
            max_total_bytes: 256 * 1024 * 1024,
        }
    }
}

/// Typed registry and matching errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagerError {
    Font(FontError),
    InvalidRequest,
    InvalidFontWeight(u16),
    FaceLimitExceeded,
    ByteLimitExceeded,
    IdExhausted,
    UnknownFontId(FontId),
    /// No face can satisfy the family request, including a completely empty registry.
    EmptyRegistry,
    MissingGlyph(char),
}
impl fmt::Display for ManagerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Font(e) => write!(f, "font parse failed: {e}"),
            Self::InvalidRequest => f.write_str(
                "font request weight must be in 1..=1000 and include a family or generic family",
            ),
            Self::InvalidFontWeight(w) => write!(f, "font weight class {w} is outside 1..=1000"),
            Self::FaceLimitExceeded => f.write_str("font face limit exceeded"),
            Self::ByteLimitExceeded => f.write_str("font byte limit exceeded"),
            Self::IdExhausted => f.write_str("font identifier space exhausted"),
            Self::UnknownFontId(id) => write!(f, "unknown font id {}", id.get()),
            Self::EmptyRegistry => f.write_str("no registered font can satisfy the request"),
            Self::MissingGlyph(ch) => write!(f, "no registered font covers U+{:04X}", *ch as u32),
        }
    }
}
impl Error for ManagerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let Self::Font(e) = self {
            Some(e)
        } else {
            None
        }
    }
}
impl From<FontError> for ManagerError {
    fn from(value: FontError) -> Self {
        Self::Font(value)
    }
}

struct Entry<'a> {
    id: FontId,
    face: FontFace<'a>,
    family: String,
    keys: Vec<String>,
    weight: u16,
    style: FontStyle,
    source: FontSource,
    face_index: usize,
    order: u64,
}

/// Resolution result. The face and its source bytes remain borrowed.
#[derive(Debug)]
pub struct ResolvedFont<'m, 'a> {
    pub id: FontId,
    pub face: &'m FontFace<'a>,
    pub family: &'m str,
    pub weight: u16,
    pub style: FontStyle,
    pub source: FontSource,
    pub face_index: usize,
}

/// A stable registry of borrowed font faces.
pub struct FontManager<'a> {
    entries: Vec<Entry<'a>>,
    limits: FontManagerLimits,
    total_bytes: usize,
    next_id: u64,
    cache_revision: u64,
}
impl<'a> FontManager<'a> {
    /// Construct a registry containing all twenty built-in OFL faces.
    ///
    /// # Panics
    /// Panics if the process-wide cache revision space is exhausted or a built-in
    /// font violates the parser/default limits.
    pub fn new() -> Self {
        let mut manager = Self::with_limits(FontManagerLimits::default());
        for embedded in EMBEDDED_FONTS {
            manager
                .register(embedded.data, 0, FontSource::Embedded)
                .expect("built-in font catalog must satisfy parser and default limits");
        }
        manager
    }
    /// Construct an empty registry with the default public limits.
    ///
    /// # Panics
    /// Panics if the process-wide cache revision space is exhausted.
    pub fn empty() -> Self {
        Self::with_limits(FontManagerLimits::default())
    }
    /// Construct an empty registry with caller-provided limits.
    ///
    /// # Panics
    /// Panics if the process-wide cache revision space is exhausted. Construction
    /// is infallible; subsequent mutations return `ManagerError::IdExhausted`.
    pub fn with_limits(limits: FontManagerLimits) -> Self {
        Self {
            entries: Vec::new(),
            limits,
            total_bytes: 0,
            next_id: 0,
            cache_revision: next_cache_revision().expect("font cache revision space exhausted"),
        }
    }
    /// Current number of registered faces.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Current bytes charged by registrations. Repeated TTC faces each count their full input.
    pub fn registered_bytes(&self) -> usize {
        self.total_bytes
    }
    pub fn limits(&self) -> FontManagerLimits {
        self.limits
    }

    /// In-process geometry cache token. Distinct between managers and renewed
    /// after every successful registration/removal, including fallback changes.
    /// Failed mutations and moves preserve it. Do not persist across processes
    /// or WASM instances; exhaustion rejects mutations instead of reusing tokens.
    /// Infallible constructors panic on exhaustion, as documented on each one.
    pub fn cache_revision(&self) -> u64 {
        self.cache_revision
    }

    /// Parse and register a single sfnt/TTC face, borrowing `bytes` without copying it.
    pub fn register(
        &mut self,
        bytes: &'a [u8],
        face_index: usize,
        source: FontSource,
    ) -> Result<FontId, ManagerError> {
        if self.entries.len() >= self.limits.max_faces {
            return Err(ManagerError::FaceLimitExceeded);
        }
        let new_total = self
            .total_bytes
            .checked_add(bytes.len())
            .ok_or(ManagerError::ByteLimitExceeded)?;
        if new_total > self.limits.max_total_bytes {
            return Err(ManagerError::ByteLimitExceeded);
        }
        let id_value = self
            .next_id
            .checked_add(1)
            .ok_or(ManagerError::IdExhausted)?;
        let face = FontFace::parse_collection(bytes, face_index)?;
        let (family, keys) = family_names(&face);
        let metrics = face.metrics();
        let weight = metrics.weight_class.unwrap_or(400);
        if !(1..=1000).contains(&weight) {
            return Err(ManagerError::InvalidFontWeight(weight));
        }
        let style = face_style(&face)?;
        let revision = next_cache_revision()?;
        let id = FontId(id_value);
        self.entries.push(Entry {
            id,
            face,
            family,
            keys,
            weight,
            style,
            source,
            face_index,
            order: id_value,
        });
        self.total_bytes = new_total;
        self.next_id = id_value;
        self.cache_revision = revision;
        Ok(id)
    }

    /// Remove a registration and release its byte and face budget. Other IDs remain unchanged.
    pub fn remove(&mut self, id: FontId) -> Result<(), ManagerError> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or(ManagerError::UnknownFontId(id))?;
        let revision = next_cache_revision()?;
        let entry = self.entries.remove(index);
        self.total_bytes -= entry.face.source().len();
        self.cache_revision = revision;
        Ok(())
    }

    /// Resolve by ordered family, metric alias, and optional generic fallback.
    pub fn resolve(&self, request: &FontRequest) -> Result<ResolvedFont<'_, 'a>, ManagerError> {
        self.resolve_inner(request, None)
    }
    /// Resolve only among faces whose cmap maps `character` to a non-.notdef glyph.
    pub fn resolve_for_char(
        &self,
        request: &FontRequest,
        character: char,
    ) -> Result<ResolvedFont<'_, 'a>, ManagerError> {
        self.resolve_inner(request, Some(character))
    }
    fn resolve_inner(
        &self,
        request: &FontRequest,
        character: Option<char>,
    ) -> Result<ResolvedFont<'_, 'a>, ManagerError> {
        if !(1..=1000).contains(&request.weight)
            || (request.families.iter().all(|f| normalize(f).is_empty())
                && request.generic.is_none())
        {
            return Err(ManagerError::InvalidRequest);
        }
        if self.entries.is_empty() {
            return Err(ManagerError::EmptyRegistry);
        }
        for family in &request.families {
            let normalized = normalize(family);
            if normalized.is_empty() {
                continue;
            }
            let mut candidates = vec![normalized.clone()];
            if let Some(alias) = metric_alias(&normalized) {
                candidates.push(normalize(alias));
            }
            for candidate in candidates {
                if let Some(entry) = self.best_for_family(request, &candidate, character) {
                    return Ok(self.resolved(entry));
                }
            }
        }
        if let Some(generic) = request.generic {
            let family = generic_family(generic);
            if let Some(entry) = self.best_for_family(request, &normalize(family), character) {
                return Ok(self.resolved(entry));
            }
            // A generic family can be absent: use the first available covering family in
            // registration order, while retaining the requested style/weight matching rules.
            for candidate in self
                .entries
                .iter()
                .filter(|entry| character.is_none_or(|ch| entry.face.glyph_index(ch) != 0))
            {
                if let Some(family_key) = candidate.keys.first() {
                    if let Some(entry) = self.best_for_family(request, family_key, character) {
                        return Ok(self.resolved(entry));
                    }
                }
            }
        }
        if let Some(ch) = character {
            Err(ManagerError::MissingGlyph(ch))
        } else {
            Err(ManagerError::EmptyRegistry)
        }
    }
    fn best_for_family(
        &self,
        request: &FontRequest,
        family: &str,
        character: Option<char>,
    ) -> Option<&Entry<'a>> {
        let mut candidates: Vec<&Entry<'a>> = self
            .entries
            .iter()
            .filter(|entry| {
                entry.keys.iter().any(|key| key == family)
                    && character.is_none_or(|ch| entry.face.glyph_index(ch) != 0)
            })
            .collect();
        candidates.sort_by(|a, b| {
            source_rank(a.source)
                .cmp(&source_rank(b.source))
                .then_with(|| {
                    style_rank(request.style, a.style).cmp(&style_rank(request.style, b.style))
                })
                .then_with(|| {
                    weight_rank(request.weight, a.weight)
                        .cmp(&weight_rank(request.weight, b.weight))
                })
                .then_with(|| a.order.cmp(&b.order))
        });
        candidates.into_iter().next()
    }
    fn resolved<'m>(&'m self, entry: &'m Entry<'a>) -> ResolvedFont<'m, 'a> {
        ResolvedFont {
            id: entry.id,
            face: &entry.face,
            family: &entry.family,
            weight: entry.weight,
            style: entry.style,
            source: entry.source,
            face_index: entry.face_index,
        }
    }
}
impl Default for FontManager<'_> {
    fn default() -> Self {
        Self::new()
    }
}

fn source_rank(source: FontSource) -> u8 {
    match source {
        FontSource::Document => 0,
        FontSource::System => 1,
        FontSource::Embedded => 2,
    }
}
fn style_rank(requested: FontStyle, actual: FontStyle) -> u8 {
    if requested == actual {
        return 0;
    }
    match requested {
        FontStyle::Normal => match actual {
            FontStyle::Oblique => 1,
            FontStyle::Italic => 2,
            FontStyle::Normal => 0,
        },
        FontStyle::Italic => match actual {
            FontStyle::Oblique => 1,
            FontStyle::Normal => 2,
            FontStyle::Italic => 0,
        },
        FontStyle::Oblique => match actual {
            FontStyle::Italic => 1,
            FontStyle::Normal => 2,
            FontStyle::Oblique => 0,
        },
    }
}
fn weight_rank(requested: u16, actual: u16) -> (u8, u16) {
    match requested.cmp(&400) {
        Ordering::Less => {
            if actual <= requested {
                (0, u16::MAX - actual)
            } else {
                (1, actual)
            }
        }
        Ordering::Greater if requested > 500 => {
            if actual >= requested {
                (0, actual)
            } else {
                (1, u16::MAX - actual)
            }
        }
        _ => {
            if (requested..=500).contains(&actual) {
                (0, actual)
            } else if actual < requested {
                (1, u16::MAX - actual)
            } else {
                (2, actual)
            }
        }
    }
}
fn metric_alias(name: &str) -> Option<&'static str> {
    match name {
        "calibri" => Some("Carlito"),
        "cambria" => Some("Caladea"),
        "arial" => Some("Liberation Sans"),
        "times new roman" => Some("Liberation Serif"),
        "courier new" => Some("Liberation Mono"),
        _ => None,
    }
}
fn generic_family(family: GenericFamily) -> &'static str {
    match family {
        GenericFamily::SansSerif => "Carlito",
        GenericFamily::Serif => "Caladea",
        GenericFamily::Monospace => "Liberation Mono",
    }
}
fn normalize(value: &str) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.to_lowercase()
}
fn is_unicode_name(record: &crate::NameRecord) -> bool {
    record.platform_id == 0 || (record.platform_id == 3 && matches!(record.encoding_id, 0 | 1 | 10))
}
fn family_names(face: &FontFace<'_>) -> (String, Vec<String>) {
    let names = face.names();
    let id = if names
        .iter()
        .any(|name| name.name_id == 16 && !name.value.trim().is_empty())
    {
        16
    } else {
        1
    };
    let records: Vec<_> = names
        .iter()
        .filter(|name| name.name_id == id && !name.value.trim().is_empty())
        .collect();
    let unicode: Vec<_> = records
        .iter()
        .copied()
        .filter(|record| is_unicode_name(record))
        .collect();
    let selectable = if unicode.is_empty() {
        &records
    } else {
        &unicode
    };
    let display = selectable
        .iter()
        .copied()
        .filter(|record| record.platform_id == 3 && record.language_id == 0x0409)
        .min_by(|a, b| {
            (a.platform_id, a.encoding_id, a.language_id, &a.value).cmp(&(
                b.platform_id,
                b.encoding_id,
                b.language_id,
                &b.value,
            ))
        })
        .or_else(|| {
            selectable.iter().copied().min_by(|a, b| {
                (a.platform_id, a.language_id, &a.value).cmp(&(
                    b.platform_id,
                    b.language_id,
                    &b.value,
                ))
            })
        })
        .map(|record| record.value.trim().to_owned())
        .unwrap_or_else(|| "Unknown".to_owned());
    let mut keys: Vec<String> = selectable
        .iter()
        .map(|record| normalize(&record.value))
        .filter(|key| !key.is_empty())
        .collect();
    keys.sort();
    keys.dedup();
    (display, keys)
}
fn face_style(face: &FontFace<'_>) -> Result<FontStyle, ManagerError> {
    if let Some(os2) = face.table(*b"OS/2") {
        let version = be_u16(os2, 0);
        if let Some(selection) = be_u16(os2, 62) {
            if selection & (1 << 0) != 0 {
                return Ok(FontStyle::Italic);
            }
            if version.is_some_and(|v| v >= 4) && selection & (1 << 9) != 0 {
                return Ok(FontStyle::Oblique);
            }
            return Ok(FontStyle::Normal);
        }
    }
    if let Some(head) = face.table(*b"head") {
        if be_u16(head, 44).is_some_and(|bits| bits & (1 << 1) != 0) {
            return Ok(FontStyle::Italic);
        }
    }
    Ok(FontStyle::Normal)
}
fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}
