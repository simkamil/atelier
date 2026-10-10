// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_unicode::properties as p;
use std::io::{self, Write};
fn sequence(value: Option<&[u32]>) -> String {
    value.map_or_else(|| "~".into(), |s| s.iter().map(|n| format!("{n:X}")).collect::<Vec<_>>().join(","))
}
fn main() {
    let mut out = io::BufWriter::new(io::stdout().lock());
    for n in 0..=0x10ffff {
        let Some(c) = char::from_u32(n) else { continue };
        let flags = [p::is_emoji(c), p::is_emoji_presentation(c), p::is_emoji_modifier(c), p::is_emoji_modifier_base(c), p::is_emoji_component(c), p::is_extended_pictographic(c), p::is_cased(c), p::is_case_ignorable(c), p::is_white_space(c)];
        write!(out, "{n:X}\t{}\t{}\t{}\t{}\t{}\t{}", p::general_category(c), p::canonical_combining_class(c), p::line_break_class(c), p::grapheme_cluster_break_property(c), p::word_break_property(c), p::indic_conjunct_break_property(c)).unwrap();
        for flag in flags { write!(out, "\t{}", if flag { "Y" } else { "N" }).unwrap(); }
        write!(out, "\t{}", p::name(c).unwrap_or("~")).unwrap();
        for value in [p::simple_uppercase(c), p::simple_lowercase(c), p::simple_titlecase(c), p::full_uppercase(c), p::full_lowercase(c), p::full_titlecase(c), p::case_fold(c), p::simple_case_fold(c), p::turkic_case_fold(c), p::case_fold_turkic(c)] { write!(out, "\t{}", sequence(value)).unwrap(); }
        let decomposition = p::decomposition(c).map_or_else(|| "~".into(), |(tag, s)| format!("{}:{}", tag.unwrap_or(""), sequence(Some(s))));
        let composition = p::decomposition(c).filter(|(tag,s)| tag.is_none() && s.len()==2).and_then(|(_,s)| p::canonical_composition(char::from_u32(s[0]).unwrap(),char::from_u32(s[1]).unwrap())).map_or_else(|| "~".into(),|c|format!("{:X}", c as u32));
        writeln!(out,"\t{decomposition}\t{}\t{composition}",if p::is_composition_exclusion(c) {"Y"} else {"N"}).unwrap();
    }
    for rule in p::conditional_casing_rules() { writeln!(out,"conditional\t{:X}\t{}\t{}\t{}\t{}",rule.code_point,rule.condition,sequence(Some(rule.lower)),sequence(Some(rule.title)),sequence(Some(rule.upper))).unwrap(); }
}
