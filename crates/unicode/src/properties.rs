// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(non_upper_case_globals)]
//! Unicode 17.0.0 properties and mappings. Algorithms using these data live in
//! their own stories; this module only provides direct database lookups.

/// One SpecialCasing.txt row. Conditions and sequences are preserved without evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConditionalCasingRule {
    /// Source scalar.
    pub code_point: u32,
    /// UCD condition expression, such as `Final_Sigma` or `lt More_Above`.
    pub condition: &'static str,
    /// Lowercase result sequence.
    pub lower: &'static [u32],
    /// Titlecase result sequence.
    pub title: &'static [u32],
    /// Uppercase result sequence.
    pub upper: &'static [u32],
}

/// An official NameAliases.txt value, kept distinct from the Name property.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NameAlias {
    /// Alias text.
    pub name: &'static str,
    /// UCD alias kind, such as `control` or `abbreviation`.
    pub kind: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

/// Unicode Character Database version represented by these tables.
pub const UNICODE_VERSION: &str = "17.0.0";

fn lookup(c: char, pages: &[u16], data: &[[u8; 256]], values: &[&'static str]) -> &'static str {
    let n = c as usize;
    let page = pages[n >> 8] as usize;
    values[data[page][n & 255] as usize]
}
fn mapped(c: char, table: &'static [(u32, &'static [u32])]) -> Option<&'static [u32]> {
    table
        .binary_search_by_key(&(c as u32), |(scalar, _)| *scalar)
        .ok()
        .map(|i| table[i].1)
}

/// General_Category value (for example `Lu` or `Nd`).
pub fn general_category(c: char) -> &'static str {
    lookup(
        c,
        general_category_PAGES,
        general_category_DATA,
        general_category_VALUES,
    )
}
/// Canonical_Combining_Class numeric value.
pub fn canonical_combining_class(c: char) -> u8 {
    lookup(
        c,
        canonical_combining_class_PAGES,
        canonical_combining_class_DATA,
        canonical_combining_class_VALUES,
    )
    .parse()
    .unwrap_or(0)
}
/// Line_Break value.
pub fn line_break_class(c: char) -> &'static str {
    lookup(c, line_break_PAGES, line_break_DATA, line_break_VALUES)
}
/// East_Asian_Width value: N, Na, A, H, W or F (Unicode 17.0.0).
pub fn east_asian_width(c: char) -> &'static str {
    lookup(
        c,
        east_asian_width_PAGES,
        east_asian_width_DATA,
        east_asian_width_VALUES,
    )
}
/// Grapheme_Cluster_Break value.
pub fn grapheme_cluster_break_property(c: char) -> &'static str {
    lookup(
        c,
        grapheme_cluster_break_PAGES,
        grapheme_cluster_break_DATA,
        grapheme_cluster_break_VALUES,
    )
}
/// Word_Break value.
pub fn word_break_property(c: char) -> &'static str {
    lookup(c, word_break_PAGES, word_break_DATA, word_break_VALUES)
}
/// Indic_Conjunct_Break value.
pub fn indic_conjunct_break_property(c: char) -> &'static str {
    lookup(
        c,
        indic_conjunct_break_PAGES,
        indic_conjunct_break_DATA,
        indic_conjunct_break_VALUES,
    )
}
/// Emoji property membership.
pub fn is_emoji(c: char) -> bool {
    lookup(c, emoji_PAGES, emoji_DATA, emoji_VALUES) == "Y"
}
/// Emoji_Presentation property membership.
pub fn is_emoji_presentation(c: char) -> bool {
    lookup(
        c,
        emoji_presentation_PAGES,
        emoji_presentation_DATA,
        emoji_presentation_VALUES,
    ) == "Y"
}
/// Emoji_Modifier property membership.
pub fn is_emoji_modifier(c: char) -> bool {
    lookup(
        c,
        emoji_modifier_PAGES,
        emoji_modifier_DATA,
        emoji_modifier_VALUES,
    ) == "Y"
}
/// Emoji_Modifier_Base property membership.
pub fn is_emoji_modifier_base(c: char) -> bool {
    lookup(
        c,
        emoji_modifier_base_PAGES,
        emoji_modifier_base_DATA,
        emoji_modifier_base_VALUES,
    ) == "Y"
}
/// Emoji_Component property membership.
pub fn is_emoji_component(c: char) -> bool {
    lookup(
        c,
        emoji_component_PAGES,
        emoji_component_DATA,
        emoji_component_VALUES,
    ) == "Y"
}
/// Extended_Pictographic property membership.
pub fn is_extended_pictographic(c: char) -> bool {
    lookup(
        c,
        extended_pictographic_PAGES,
        extended_pictographic_DATA,
        extended_pictographic_VALUES,
    ) == "Y"
}
/// Cased property membership.
pub fn is_cased(c: char) -> bool {
    lookup(c, cased_PAGES, cased_DATA, cased_VALUES) == "Y"
}
/// Case_Ignorable property membership.
pub fn is_case_ignorable(c: char) -> bool {
    lookup(
        c,
        case_ignorable_PAGES,
        case_ignorable_DATA,
        case_ignorable_VALUES,
    ) == "Y"
}
/// White_Space property membership.
pub fn is_white_space(c: char) -> bool {
    lookup(c, white_space_PAGES, white_space_DATA, white_space_VALUES) == "Y"
}
/// Unicode character name. Hangul, CJK, and other algorithmic names are expanded at generation time.
pub fn name(c: char) -> Option<&'static str> {
    NAMES
        .binary_search_by_key(&(c as u32), |(scalar, _)| *scalar)
        .ok()
        .map(|i| NAMES[i].1)
}
/// All official aliases for a scalar, separate from the Unicode Name property.
pub fn name_aliases(c: char) -> &'static [NameAlias] {
    NAME_ALIASES
        .binary_search_by_key(&(c as u32), |(scalar, _)| *scalar)
        .ok()
        .map(|i| NAME_ALIASES[i].1)
        .unwrap_or(&[])
}

/// Simple uppercase mapping, if the scalar has an explicit mapping.
pub fn simple_uppercase(c: char) -> Option<&'static [u32]> {
    mapped(c, SIMPLE_UPPERCASE)
}
/// Simple lowercase mapping, if the scalar has an explicit mapping.
pub fn simple_lowercase(c: char) -> Option<&'static [u32]> {
    mapped(c, SIMPLE_LOWERCASE)
}
/// Simple titlecase mapping, if the scalar has an explicit mapping.
pub fn simple_titlecase(c: char) -> Option<&'static [u32]> {
    mapped(c, SIMPLE_TITLECASE)
}
/// Full uppercase mapping, including unconditional multi-scalar special casing.
pub fn full_uppercase(c: char) -> Option<&'static [u32]> {
    mapped(c, FULL_UPPERCASE).or_else(|| simple_uppercase(c))
}
/// Full lowercase mapping, including unconditional multi-scalar special casing.
pub fn full_lowercase(c: char) -> Option<&'static [u32]> {
    mapped(c, FULL_LOWERCASE).or_else(|| simple_lowercase(c))
}
/// Full titlecase mapping, including unconditional multi-scalar special casing.
pub fn full_titlecase(c: char) -> Option<&'static [u32]> {
    mapped(c, FULL_TITLECASE).or_else(|| simple_titlecase(c))
}
/// Default full case-fold mapping.
pub fn case_fold(c: char) -> Option<&'static [u32]> {
    mapped(c, CASE_FOLD)
}
/// Simple case-fold mapping.
pub fn simple_case_fold(c: char) -> Option<&'static [u32]> {
    mapped(c, SIMPLE_CASE_FOLD)
}
/// Turkic case-fold override, if present.
pub fn turkic_case_fold(c: char) -> Option<&'static [u32]> {
    mapped(c, TURKIC_CASE_FOLD)
}
/// Case fold selected for Turkic casing, falling back to the default full fold.
pub fn case_fold_turkic(c: char) -> Option<&'static [u32]> {
    turkic_case_fold(c).or_else(|| case_fold(c))
}
/// Canonical or tagged decomposition mapping (tag is `None` for canonical mappings).
pub fn decomposition(c: char) -> Option<(Option<&'static str>, &'static [u32])> {
    DECOMPOSITIONS
        .binary_search_by_key(&(c as u32), |(scalar, _, _)| *scalar)
        .ok()
        .map(|i| (DECOMPOSITIONS[i].1, DECOMPOSITIONS[i].2))
}
/// All conditional SpecialCasing rows with separate lower, title, and upper scalar mappings.
pub fn conditional_casing_rules() -> &'static [ConditionalCasingRule] {
    CONDITIONAL_CASE
}
/// Canonical composition exclusions.
pub fn is_composition_exclusion(c: char) -> bool {
    COMPOSITION_EXCLUSIONS.binary_search(&(c as u32)).is_ok()
}
/// Canonical composition for a starter and combining scalar, before blocking rules are applied.
pub fn canonical_composition(first: char, second: char) -> Option<char> {
    CANONICAL_COMPOSITIONS
        .binary_search_by_key(&(first as u32, second as u32), |(a, b, _)| (*a, *b))
        .ok()
        .and_then(|i| char::from_u32(CANONICAL_COMPOSITIONS[i].2))
}
