// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_unicode::properties as ucd;

#[test]
fn unicode_17_properties_and_algorithmic_names_are_available() {
    assert_eq!(ucd::UNICODE_VERSION, "17.0.0");
    assert_eq!(ucd::general_category('A'), "Lu");
    assert_eq!(ucd::canonical_combining_class('\u{0301}'), 230);
    assert!(ucd::is_white_space('\u{202F}'));
    assert!(ucd::is_cased('A'));
    assert_eq!(ucd::line_break_class('\u{00A0}'), "GL");
    assert_eq!(ucd::grapheme_cluster_break_property('\u{200D}'), "ZWJ");
    assert_eq!(ucd::word_break_property('\u{0301}'), "Extend");
    assert_eq!(ucd::indic_conjunct_break_property('\u{094D}'), "Linker");
    assert_eq!(ucd::name('\u{AC00}'), Some("HANGUL SYLLABLE GA"));
    assert_eq!(ucd::name('\u{4E00}'), Some("CJK UNIFIED IDEOGRAPH-4E00"));
    assert_eq!(ucd::name('\0'), None);
    assert!(
        ucd::name_aliases('\0')
            .iter()
            .any(|alias| alias.name == "NULL" && alias.kind == "control")
    );
    assert!(ucd::is_extended_pictographic('😀'));
    assert!(ucd::is_emoji('😀'));
    assert!(ucd::is_emoji_presentation('😀'));
    assert!(ucd::is_emoji_modifier('🏻'));
    assert!(ucd::is_emoji_modifier_base('👍'));
    assert!(ucd::is_emoji_component('\u{200D}'));
}

#[test]
fn mappings_and_unapplied_special_casing_rules_are_preserved() {
    assert_eq!(ucd::simple_lowercase('A'), Some(&[0x61][..]));
    assert_eq!(ucd::full_lowercase('İ'), Some(&[0x69, 0x307][..]));
    assert_eq!(ucd::case_fold('ß'), Some(&[0x73, 0x73][..]));
    assert_eq!(ucd::case_fold_turkic('I'), Some(&[0x131][..]));
    assert_eq!(ucd::decomposition('é'), Some((None, &[0x65, 0x301][..])));
    assert!(ucd::is_composition_exclusion('\u{0958}'));
    assert_eq!(ucd::canonical_composition('e', '\u{0301}'), Some('é'));
    assert!(
        ucd::conditional_casing_rules()
            .iter()
            .any(|rule| { rule.code_point == 0x49 && rule.condition.contains("More_Above") })
    );
}
