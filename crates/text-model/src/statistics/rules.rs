// SPDX-License-Identifier: AGPL-3.0-or-later
//! Counting rules (US-343): the only place that decides what a word, a
//! character or a paragraph is.
//!
//! Every judgement that is not certain to match Microsoft Word lives in the
//! small classification functions below ([`classify`], [`is_cjk_punctuation`],
//! [`is_ideograph`]) or in [`CountingRules`], so it can be changed in one spot.

use super::Counts;
use atelier_unicode::segmentation::grapheme_boundaries;

/// What a "character" is for the character counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CharUnit {
    /// One Unicode scalar value (code point). `e` + combining acute = 2.
    #[default]
    Scalar,
    /// One extended grapheme cluster (UAX #29). `e` + combining acute = 1,
    /// a family emoji sequence = 1.
    Grapheme,
    /// One UTF-16 code unit. A character outside the BMP counts for 2.
    Utf16,
}

/// The tunable part of the counting rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CountingRules {
    /// Unit of the character counters (default: [`CharUnit::Scalar`]).
    pub char_unit: CharUnit,
    /// Whether a paragraph without any character counts as a paragraph
    /// (default `false`, the rule documented for Word in US-343).
    pub count_empty_paragraphs: bool,
}

impl Default for CountingRules {
    fn default() -> Self {
        Self {
            char_unit: CharUnit::Scalar,
            count_empty_paragraphs: false,
        }
    }
}

/// Role of a character in the counters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CharClass {
    /// Separates words and is a space for the "no spaces" counter: space,
    /// tab, line breaks, other Unicode spaces that may break a line.
    Separator,
    /// Does not separate words ("10 km" is one word) but is a space for the
    /// "no spaces" counter: U+00A0, U+2007, U+202F.
    NoBreakSpace,
    /// Counts for one word on its own (Han, kana).
    Ideograph,
    /// Ideographic punctuation: part of a token but never a word on its own.
    IdeographicPunctuation,
    /// Anything else: a character of a word.
    Other,
}

/// Classifies a character. This is the function to edit to change which
/// characters separate words or count as spaces.
pub fn classify(c: char) -> CharClass {
    match c {
        ' '
        | '\t'
        | '\n'
        | '\r'
        | '\u{b}'
        | '\u{c}'
        | '\u{85}'
        | '\u{1680}'
        | '\u{2000}'..='\u{2006}'
        | '\u{2008}'..='\u{200a}'
        | '\u{2028}'
        | '\u{2029}'
        | '\u{205f}'
        | '\u{3000}' => CharClass::Separator,
        '\u{a0}' | '\u{2007}' | '\u{202f}' => CharClass::NoBreakSpace,
        _ if is_ideograph(c) => CharClass::Ideograph,
        _ if is_cjk_punctuation(c) => CharClass::IdeographicPunctuation,
        _ => CharClass::Other,
    }
}

/// Characters counted one word each: Han, Hiragana, Katakana (not Hangul,
/// which is written with spaces and counted by token).
pub fn is_ideograph(c: char) -> bool {
    matches!(c,
        '\u{2e80}'..='\u{2fdf}'
        | '\u{3005}'..='\u{3007}'
        | '\u{3021}'..='\u{3029}'
        | '\u{3038}'..='\u{303b}'
        | '\u{3041}'..='\u{3096}'
        | '\u{309d}'..='\u{309f}'
        | '\u{30a1}'..='\u{30fa}'
        | '\u{30fc}'..='\u{30ff}'
        | '\u{3105}'..='\u{312f}'
        | '\u{31f0}'..='\u{31ff}'
        | '\u{3400}'..='\u{4dbf}'
        | '\u{4e00}'..='\u{9fff}'
        | '\u{f900}'..='\u{faff}'
        | '\u{ff66}'..='\u{ff9f}'
        | '\u{1b000}'..='\u{1b16f}'
        | '\u{20000}'..='\u{323af}')
}

/// Punctuation of East Asian text, never a word by itself.
pub fn is_cjk_punctuation(c: char) -> bool {
    matches!(c,
        '\u{3001}'..='\u{3004}'
        | '\u{3008}'..='\u{3020}'
        | '\u{3030}'
        | '\u{30fb}'
        | '\u{ff01}'..='\u{ff0f}'
        | '\u{ff1a}'..='\u{ff20}'
        | '\u{ff3b}'..='\u{ff40}'
        | '\u{ff5b}'..='\u{ff65}')
}

/// Marks that stay glued to the ideograph before them (variation selectors,
/// kana voicing marks) and add no word of their own.
fn is_ideograph_modifier(c: char) -> bool {
    matches!(c, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}' | '\u{3099}' | '\u{309a}')
}

/// Whether the character is a space for the "without spaces" counter.
fn is_space(class: CharClass) -> bool {
    matches!(class, CharClass::Separator | CharClass::NoBreakSpace)
}

/// Counts one paragraph. `text` holds the characters of the paragraph without
/// its paragraph mark; a line break is a `\n`, `\u{b}` or `\u{2028}` inside it.
pub fn count_paragraph(text: &str, rules: &CountingRules) -> Counts {
    let mut words = 0usize;
    // Current run of non-ideographic characters of a token.
    let mut run_has_word_char = false;
    let mut after_ideograph = false;
    let mut chars = 0usize;
    let mut chars_with_spaces = 0usize;

    for c in text.chars() {
        let class = classify(c);
        match class {
            CharClass::Separator => {
                words += usize::from(run_has_word_char);
                run_has_word_char = false;
                after_ideograph = false;
            }
            CharClass::NoBreakSpace | CharClass::IdeographicPunctuation => {}
            CharClass::Ideograph => {
                words += usize::from(run_has_word_char) + 1;
                run_has_word_char = false;
                after_ideograph = true;
            }
            CharClass::Other => {
                if after_ideograph && is_ideograph_modifier(c) {
                    // Part of the preceding ideograph.
                } else {
                    run_has_word_char = true;
                    after_ideograph = false;
                }
            }
        }
        if rules.char_unit == CharUnit::Scalar {
            chars_with_spaces += 1;
            chars += usize::from(!is_space(class));
        } else if rules.char_unit == CharUnit::Utf16 {
            chars_with_spaces += c.len_utf16();
            if !is_space(class) {
                chars += c.len_utf16();
            }
        }
    }
    words += usize::from(run_has_word_char);

    if rules.char_unit == CharUnit::Grapheme {
        let index = grapheme_boundaries(text);
        for pair in index.boundaries().windows(2) {
            chars_with_spaces += 1;
            let first = text[pair[0]..pair[1]].chars().next().unwrap_or(' ');
            chars += usize::from(!is_space(classify(first)));
        }
    }

    Counts {
        words,
        characters: chars,
        characters_with_spaces: chars_with_spaces,
        paragraphs: usize::from(!text.is_empty() || rules.count_empty_paragraphs),
    }
}
