// SPDX-License-Identifier: AGPL-3.0-or-later
//! Strict, deterministic reader and source generator for the frozen Unicode 17 UCD.
//! This crate intentionally uses only the Rust standard library.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

pub const UNICODE_VERSION: &str = "17.0.0";
pub const MAX_INPUT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 128 * 1024 * 1024;
const LIMIT: u32 = 0x10_FFFF;

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;
fn err(message: impl Into<String>) -> Error {
    Error(message.into())
}

#[derive(Clone, Debug)]
struct Range {
    first: u32,
    last: u32,
    value: String,
}
type ConditionalMapping = (u32, String, Vec<u32>, Vec<u32>, Vec<u32>);
type ValueAliases = BTreeMap<&'static str, BTreeSet<String>>;
#[derive(Default)]
struct Data {
    props: BTreeMap<&'static str, Vec<Range>>,
    missing: BTreeMap<&'static str, Vec<Range>>,
    names: BTreeMap<u32, String>,
    aliases: BTreeMap<u32, Vec<(String, String)>>,
    mappings: BTreeMap<&'static str, BTreeMap<u32, Vec<u32>>>,
    decompositions: BTreeMap<u32, (Option<String>, Vec<u32>)>,
    conditionals: Vec<ConditionalMapping>,
    exclusions: BTreeSet<u32>,
    compositions: Vec<(u32, u32, u32)>,
}

fn source(root: &Path, rel: &str) -> Result<String> {
    let path = root.join(rel);
    let meta = fs::metadata(&path)
        .map_err(|e| err(format!("required UCD file {}: {e}", path.display())))?;
    if meta.len() > MAX_INPUT_BYTES {
        return Err(err(format!(
            "input limit exceeded: {} ({} bytes)",
            path.display(),
            meta.len()
        )));
    }
    let text = fs::read_to_string(&path)
        .map_err(|e| err(format!("{} is not readable UTF-8: {e}", path.display())))?;
    if rel != "UnicodeData.txt" {
        let version_matches = text.lines().take(20).any(|line| {
            line.contains(UNICODE_VERSION)
                || (rel == "emoji/emoji-data.txt" && line.contains("Version: 17.0"))
        });
        if !version_matches {
            return Err(err(format!(
                "{} does not identify Unicode {UNICODE_VERSION}",
                path.display()
            )));
        }
    }
    Ok(text)
}
fn cp(s: &str) -> Result<u32> {
    let n = u32::from_str_radix(s, 16).map_err(|_| err(format!("invalid code point {s:?}")))?;
    if n > LIMIT {
        return Err(err(format!("code point out of range: U+{n:X}")));
    }
    Ok(n)
}
fn scalar(s: &str) -> Result<u32> {
    let n = cp(s)?;
    if (0xD800..=0xDFFF).contains(&n) {
        return Err(err(format!(
            "mapping target U+{n:04X} is not a Unicode scalar"
        )));
    }
    Ok(n)
}
fn validate_gc_ccc(category: &str, ccc: &str) -> Result<()> {
    const CATEGORIES: &[&str] = &[
        "Lu", "Ll", "Lt", "Lm", "Lo", "Mn", "Mc", "Me", "Nd", "Nl", "No", "Pc", "Pd", "Ps", "Pe",
        "Pi", "Pf", "Po", "Sm", "Sc", "Sk", "So", "Zs", "Zl", "Zp", "Cc", "Cf", "Cs", "Co", "Cn",
    ];
    if !CATEGORIES.contains(&category) {
        return Err(err(format!("invalid General_Category value {category:?}")));
    }
    ccc.parse::<u8>()
        .map_err(|_| err(format!("invalid Canonical_Combining_Class value {ccc:?}")))?;
    Ok(())
}
fn validate_range_pair(
    first_name: &str,
    last_name: &str,
    first_fields: &[String],
    last_fields: &[&str],
) -> Result<()> {
    let first_base = first_name
        .strip_suffix(", First>")
        .ok_or_else(|| err("UnicodeData range start lacks First marker"))?;
    let last_base = last_name
        .strip_suffix(", Last>")
        .ok_or_else(|| err("UnicodeData range end lacks Last marker"))?;
    if first_base != last_base
        || first_fields.len() != last_fields.len()
        || first_fields[2..]
            .iter()
            .zip(&last_fields[2..])
            .any(|(first, last)| first != last)
    {
        return Err(err(
            "UnicodeData First/Last names or properties do not match",
        ));
    }
    Ok(())
}
fn span(s: &str) -> Result<(u32, u32)> {
    let mut it = s.split("..");
    let a = cp(it.next().unwrap_or(""))?;
    let b = match it.next() {
        Some(v) => cp(v)?,
        None => a,
    };
    if it.next().is_some() || a > b {
        return Err(err(format!("invalid or inverted range {s:?}")));
    }
    Ok((a, b))
}
fn fields(line: &str) -> Vec<&str> {
    line.split('#')
        .next()
        .unwrap_or("")
        .split(';')
        .map(str::trim)
        .collect()
}
fn parse_property_record<'a>(
    line: &'a str,
    path: &str,
    line_no: usize,
    exact_fields: Option<usize>,
) -> Result<Option<Vec<&'a str>>> {
    let content = line.split('#').next().unwrap_or("").trim();
    if content.is_empty() {
        return Ok(None);
    }
    let record: Vec<_> = content.split(';').map(str::trim).collect();
    if record.len() < 2 || record.iter().any(|field| field.is_empty()) {
        return Err(err(format!(
            "{path}:{}: malformed property record",
            line_no + 1
        )));
    }
    if exact_fields.is_some_and(|count| record.len() != count) {
        return Err(err(format!(
            "{path}:{}: malformed property field count",
            line_no + 1
        )));
    }
    span(record[0]).map_err(|e| err(format!("{path}:{}: {e}", line_no + 1)))?;
    Ok(Some(record))
}
fn property_alias_key(name: &str) -> Option<&'static str> {
    match name {
        "line_break" => Some("lb"),
        "east_asian_width" => Some("ea"),
        "grapheme_cluster_break" => Some("GCB"),
        "word_break" => Some("WB"),
        "indic_conjunct_break" => Some("InCB"),
        _ => None,
    }
}
fn validate_enum_value(aliases: &ValueAliases, property: &'static str, value: &str) -> Result<()> {
    if !aliases
        .get(property)
        .is_some_and(|values| values.contains(value))
    {
        return Err(err(format!("invalid {property} property value {value:?}")));
    }
    Ok(())
}
fn normalize_binary_value(value: &str) -> Result<&'static str> {
    match value {
        "Y" | "Yes" | "T" | "True" => Ok("Y"),
        "N" | "No" | "F" | "False" => Ok("N"),
        _ => Err(err(format!(
            "invalid binary property value {value:?}; expected Y or N"
        ))),
    }
}
fn property_value_aliases(root: &Path) -> Result<ValueAliases> {
    let text = source(root, "PropertyValueAliases.txt")?;
    let mut result = ValueAliases::new();
    for (line_no, line) in text.lines().enumerate() {
        let content = line.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let record: Vec<_> = content.split(';').map(str::trim).collect();
        if record.len() < 3 || record.iter().any(|field| field.is_empty()) {
            return Err(err(format!(
                "PropertyValueAliases.txt:{}: malformed alias row",
                line_no + 1
            )));
        }
        if !matches!(record[0], "lb" | "ea" | "GCB" | "WB" | "InCB") {
            continue;
        }
        let key = match record[0] {
            "lb" => "lb",
            "ea" => "ea",
            "GCB" => "GCB",
            "WB" => "WB",
            "InCB" => "InCB",
            _ => unreachable!(),
        };
        let values = result.entry(key).or_default();
        values.extend(
            record[1..]
                .iter()
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string()),
        );
    }
    for key in ["lb", "ea", "GCB", "WB", "InCB"] {
        if !result.get(key).is_some_and(|values| !values.is_empty()) {
            return Err(err(format!(
                "PropertyValueAliases.txt has no values for {key}"
            )));
        }
    }
    Ok(result)
}
fn push_property(data: &mut Data, name: &'static str, range: &str, value: &str) -> Result<()> {
    let (first, last) = span(range)?;
    data.props.entry(name).or_default().push(Range {
        first,
        last,
        value: value.into(),
    });
    Ok(())
}
fn property_file(
    data: &mut Data,
    root: &Path,
    path: &str,
    name: &'static str,
    default: &str,
    wanted: Option<&str>,
    aliases: &ValueAliases,
) -> Result<()> {
    let txt = source(root, path)?;
    if let Some(key) = property_alias_key(name) {
        validate_enum_value(aliases, key, default)?;
    } else if wanted.is_some() {
        normalize_binary_value(default)?;
    }
    // @missing is explicit UCD data, not an assumed numeric zero.
    let missing: Vec<_> = txt
        .lines()
        .filter_map(|l| l.trim().strip_prefix("# @missing:"))
        .collect();
    if missing.is_empty() && wanted.is_none() {
        return Err(err(format!("{path} has no @missing directive")));
    }
    for (line_no, line) in txt.lines().enumerate() {
        let Some(raw) = line.trim().strip_prefix("# @missing:") else {
            continue;
        };
        let record: Vec<_> = raw.split(';').map(str::trim).collect();
        let expected = if wanted.is_some() { 3 } else { 2 };
        if record.len() != expected || record[0].is_empty() || record[1].is_empty() {
            return Err(err(format!(
                "{path}:{}: malformed @missing record",
                line_no + 1
            )));
        }
        let (a, b) = span(record[0]).map_err(|e| err(format!("{path}:{}: {e}", line_no + 1)))?;
        if let Some(w) = wanted {
            if record[1] != w {
                continue;
            }
            let value = normalize_binary_value(record[2])?;
            data.missing.entry(name).or_default().push(Range {
                first: a,
                last: b,
                value: value.into(),
            });
        } else {
            validate_enum_value(
                aliases,
                property_alias_key(name).ok_or_else(|| err("missing property alias key"))?,
                record[1],
            )?;
            data.missing.entry(name).or_default().push(Range {
                first: a,
                last: b,
                value: record[1].into(),
            });
        }
    }
    for (line_no, line) in txt.lines().enumerate() {
        let strict_columns = if wanted.is_none() || path == "emoji/emoji-data.txt" {
            Some(2)
        } else {
            None
        };
        let Some(f) = parse_property_record(line, path, line_no, strict_columns)? else {
            continue;
        };
        if let Some(w) = wanted {
            if f[1] != w {
                continue;
            }
            push_property(data, name, f[0], "Y")?;
        } else {
            validate_enum_value(
                aliases,
                property_alias_key(name).ok_or_else(|| err("missing property alias key"))?,
                f[1],
            )?;
            push_property(data, name, f[0], f[1])?;
        }
    }
    Ok(())
}
fn unicode_data(data: &mut Data, root: &Path) -> Result<()> {
    let txt = source(root, "UnicodeData.txt")?;
    let mut pending: Option<(u32, Vec<String>)> = None;
    for (line_no, line) in txt.lines().enumerate() {
        let f: Vec<_> = line.split(';').collect();
        if f.len() != 15 {
            return Err(err(format!(
                "UnicodeData.txt:{}: expected 15 fields",
                line_no + 1
            )));
        }
        let n = cp(f[0])?;
        let name = f[1];
        validate_gc_ccc(f[2], f[3])?;
        if name.ends_with(", First>") {
            if pending.is_some() {
                return Err(err("nested UnicodeData First range"));
            }
            pending = Some((n, f.iter().map(|x| (*x).to_string()).collect()));
            continue;
        }
        if name.ends_with(", Last>") {
            let (start, first) = pending
                .take()
                .ok_or_else(|| err("UnicodeData Last without First"))?;
            if n < start {
                return Err(err("inverted UnicodeData First/Last range"));
            }
            validate_range_pair(&first[1], name, &first, &f)?;
            push_property(
                data,
                "general_category",
                &format!("{start:X}..{n:X}"),
                &first[2],
            )?;
            push_property(
                data,
                "canonical_combining_class",
                &format!("{start:X}..{n:X}"),
                &first[3],
            )?;
            continue;
        }
        if pending.is_some() {
            return Err(err("UnicodeData range missing Last"));
        }
        insert_unicode_record(
            data,
            n,
            &f.iter().map(|x| (*x).to_string()).collect::<Vec<_>>(),
        )?;
    }
    if pending.is_some() {
        return Err(err("unterminated UnicodeData First range"));
    }
    Ok(())
}
fn insert_unicode_record(d: &mut Data, n: u32, f: &[String]) -> Result<()> {
    validate_gc_ccc(&f[2], &f[3])?;
    if !f[1].starts_with('<') {
        d.names.insert(n, f[1].clone());
    }
    push_property(d, "general_category", &format!("{n:X}"), &f[2])?;
    push_property(
        d,
        "canonical_combining_class",
        &format!("{n:X}"),
        f[3].as_str(),
    )?;
    for (field, key) in [
        (12, "simple_uppercase"),
        (13, "simple_lowercase"),
        (14, "simple_titlecase"),
    ] {
        if !f[field].is_empty() {
            d.mappings
                .entry(key)
                .or_default()
                .insert(n, vec![scalar(&f[field])?]);
        }
    }
    if !f[5].is_empty() {
        let (tag, seq) = if f[5].starts_with('<') {
            let end = f[5].find('>').ok_or_else(|| err("bad decomposition tag"))?;
            (Some(f[5][1..end].to_string()), &f[5][end + 1..])
        } else {
            (None, f[5].as_str())
        };
        let vals = seq
            .split_whitespace()
            .map(scalar)
            .collect::<Result<Vec<_>>>()?;
        if vals.is_empty() {
            return Err(err("empty decomposition mapping"));
        }
        d.decompositions.insert(n, (tag, vals));
    }
    Ok(())
}
fn sequences(data: &mut Data, root: &Path) -> Result<()> {
    let txt = source(root, "CaseFolding.txt")?;
    for (i, l) in txt.lines().enumerate() {
        let f = fields(l);
        if f.len() < 3 || f[0].is_empty() {
            continue;
        }
        let n = cp(f[0])?;
        let status = f[1];
        if !matches!(status, "C" | "F" | "S" | "T") {
            return Err(err(format!(
                "CaseFolding.txt:{}: invalid status {status}",
                i + 1
            )));
        }
        let vals = f[2]
            .split_whitespace()
            .map(scalar)
            .collect::<Result<Vec<_>>>()?;
        if vals.is_empty() {
            return Err(err(format!("CaseFolding.txt:{}: empty mapping", i + 1)));
        }
        if matches!(status, "C" | "S" | "T") && vals.len() != 1 {
            return Err(err(format!(
                "CaseFolding.txt:{}: simple mapping must have one scalar",
                i + 1
            )));
        }
        if status == "C" || status == "S" {
            data.mappings
                .entry("simple_case_fold")
                .or_default()
                .insert(n, vals.clone());
        }
        if status == "C" || status == "F" {
            data.mappings
                .entry("case_fold")
                .or_default()
                .insert(n, vals.clone());
        }
        if status == "T" {
            data.mappings
                .entry("turkic_case_fold")
                .or_default()
                .insert(n, vals);
        }
    }
    let txt = source(root, "SpecialCasing.txt")?;
    for l in txt.lines() {
        let f = fields(l);
        if f.len() < 5 || f[0].is_empty() {
            continue;
        }
        let n = cp(f[0])?;
        let mut maps = Vec::new();
        for seq in &f[1..4] {
            maps.push(
                seq.split_whitespace()
                    .map(scalar)
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        if f[4].is_empty() {
            for (key, map) in [
                ("full_lowercase", &maps[0]),
                ("full_titlecase", &maps[1]),
                ("full_uppercase", &maps[2]),
            ] {
                if !map.is_empty() {
                    data.mappings.entry(key).or_default().insert(n, map.clone());
                }
            }
        } else {
            data.conditionals.push((
                n,
                f[4].to_string(),
                maps.remove(0),
                maps.remove(0),
                maps.remove(0),
            ));
        }
    }
    let txt = source(root, "CompositionExclusions.txt")?;
    for l in txt.lines() {
        let f = fields(l);
        if !f[0].is_empty() {
            let (a, b) = span(f[0])?;
            for n in a..=b {
                data.exclusions.insert(n);
            }
        }
    }
    let txt = source(root, "DerivedNormalizationProps.txt")?;
    for l in txt.lines() {
        let f = fields(l);
        if f.len() >= 2 && f[1] == "Full_Composition_Exclusion" {
            let (a, b) = span(f[0])?;
            for n in a..=b {
                data.exclusions.insert(n);
            }
        }
    }
    for (&composite, (tag, sequence)) in &data.decompositions {
        if tag.is_none() && sequence.len() == 2 && !data.exclusions.contains(&composite) {
            data.compositions
                .push((sequence[0], sequence[1], composite));
        }
    }
    data.compositions.sort_unstable();
    Ok(())
}
fn names(root: &Path, data: &mut Data) -> Result<()> {
    let txt = source(root, "extracted/DerivedName.txt")?;
    for l in txt.lines() {
        let f = fields(l);
        if f.len() < 2 || f[0].is_empty() {
            continue;
        }
        let (a, b) = span(f[0])?;
        if a != b {
            if !f[1].contains('*') {
                return Err(err("DerivedName range lacks an algorithmic pattern"));
            }
            for n in a..=b {
                data.names.insert(n, f[1].replace('*', &format!("{n:04X}")));
            }
        } else {
            let name = if f[1].contains('*') {
                f[1].replace('*', &format!("{a:04X}"))
            } else {
                f[1].to_string()
            };
            data.names.insert(a, name);
        }
    }
    Ok(())
}
fn name_aliases(root: &Path, data: &mut Data) -> Result<()> {
    let txt = source(root, "NameAliases.txt")?;
    for (line_no, line) in txt.lines().enumerate() {
        let f = fields(line);
        if f.len() < 3 || f[0].is_empty() {
            continue;
        }
        let scalar = cp(f[0])?;
        if f[1].is_empty()
            || !matches!(
                f[2],
                "correction" | "control" | "alternate" | "figment" | "abbreviation"
            )
        {
            return Err(err(format!(
                "NameAliases.txt:{}: malformed alias record",
                line_no + 1
            )));
        }
        data.aliases
            .entry(scalar)
            .or_default()
            .push((f[1].into(), f[2].into()));
    }
    Ok(())
}
fn load(root: &Path) -> Result<Data> {
    let mut d = Data::default();
    let aliases = property_value_aliases(root)?;
    unicode_data(&mut d, root)?;
    let prop_root = root;
    property_file(
        &mut d,
        prop_root,
        "EastAsianWidth.txt",
        "east_asian_width",
        "N",
        None,
        &aliases,
    )?;
    property_file(
        &mut d,
        prop_root,
        "LineBreak.txt",
        "line_break",
        "XX",
        None,
        &aliases,
    )?;
    property_file(
        &mut d,
        prop_root,
        "auxiliary/GraphemeBreakProperty.txt",
        "grapheme_cluster_break",
        "Other",
        None,
        &aliases,
    )?;
    property_file(
        &mut d,
        prop_root,
        "auxiliary/WordBreakProperty.txt",
        "word_break",
        "Other",
        None,
        &aliases,
    )?;
    // InCB is a three-field derived core property with explicit None default.
    validate_enum_value(&aliases, "InCB", "None")?;
    let dc = source(root, "DerivedCoreProperties.txt")?;
    let mut incb_missing = false;
    for (line_no, l) in dc.lines().enumerate() {
        let t = l.trim();
        if let Some(raw) = t.strip_prefix("# @missing:") {
            let f: Vec<_> = raw.split(';').map(str::trim).collect();
            if f.len() != 3 || f.iter().any(|field| field.is_empty()) {
                return Err(err(format!(
                    "DerivedCoreProperties.txt:{}: malformed @missing record",
                    line_no + 1
                )));
            }
            let (a, b) = span(f[0])
                .map_err(|e| err(format!("DerivedCoreProperties.txt:{}: {e}", line_no + 1)))?;
            if f[1] == "InCB" {
                incb_missing = true;
                validate_enum_value(&aliases, "InCB", f[2])?;
                d.missing
                    .entry("indic_conjunct_break")
                    .or_default()
                    .push(Range {
                        first: a,
                        last: b,
                        value: f[2].into(),
                    });
            }
        }
        if !l.trim_start().starts_with('#') && !l.trim().is_empty() {
            let f: Vec<_> = l
                .split('#')
                .next()
                .unwrap_or("")
                .split(';')
                .map(str::trim)
                .collect();
            if !(2..=3).contains(&f.len()) || f.iter().any(|field| field.is_empty()) {
                return Err(err(format!(
                    "DerivedCoreProperties.txt:{}: malformed property record",
                    line_no + 1
                )));
            }
            span(f[0])
                .map_err(|e| err(format!("DerivedCoreProperties.txt:{}: {e}", line_no + 1)))?;
            if f[1] == "InCB" {
                if f.len() != 3 {
                    return Err(err(format!(
                        "DerivedCoreProperties.txt:{}: malformed InCB record",
                        line_no + 1
                    )));
                }
                validate_enum_value(&aliases, "InCB", f[2])?;
                push_property(&mut d, "indic_conjunct_break", f[0], f[2])?;
            }
        }
    }
    if !incb_missing {
        return Err(err("DerivedCoreProperties.txt lacks InCB @missing"));
    }
    for (name, value) in [
        ("emoji", "Emoji"),
        ("emoji_presentation", "Emoji_Presentation"),
        ("emoji_modifier", "Emoji_Modifier"),
        ("emoji_modifier_base", "Emoji_Modifier_Base"),
        ("emoji_component", "Emoji_Component"),
        ("extended_pictographic", "Extended_Pictographic"),
    ] {
        property_file(
            &mut d,
            root,
            "emoji/emoji-data.txt",
            name,
            "N",
            Some(value),
            &aliases,
        )?;
    }
    for (path, prop) in [
        ("DerivedCoreProperties.txt", "Cased"),
        ("DerivedCoreProperties.txt", "Case_Ignorable"),
        ("PropList.txt", "White_Space"),
    ] {
        property_file(
            &mut d,
            root,
            path,
            match prop {
                "Cased" => "cased",
                "Case_Ignorable" => "case_ignorable",
                _ => "white_space",
            },
            "N",
            Some(prop),
            &aliases,
        )?;
    }
    sequences(&mut d, root)?;
    names(root, &mut d)?;
    name_aliases(root, &mut d)?;
    for rs in d.props.values_mut() {
        rs.sort_by_key(|r| (r.first, r.last));
        let mut last = None;
        for r in rs.iter() {
            if last.is_some_and(|v| r.first <= v) {
                return Err(err(format!(
                    "overlapping property range near U+{:X}",
                    r.first
                )));
            }
            last = Some(r.last);
        }
    }
    Ok(d)
}

fn emit_page_table(
    out: &mut String,
    name: &str,
    ranges: &[Range],
    missing: &[Range],
    default: &str,
) -> Result<()> {
    let mut values = BTreeSet::new();
    values.insert(default.to_owned());
    for r in ranges.iter().chain(missing) {
        values.insert(r.value.clone());
    }
    if values.len() > (u8::MAX as usize + 1) {
        return Err(err(format!("too many values for {name}")));
    }
    let vals: Vec<_> = values.into_iter().collect();
    let ids: BTreeMap<_, _> = vals
        .iter()
        .enumerate()
        .map(|(i, v)| (v.clone(), i as u8))
        .collect();
    let mut all = vec![ids[default]; (LIMIT + 1) as usize];
    for r in missing {
        for x in r.first..=r.last {
            all[x as usize] = ids[&r.value];
        }
    }
    for r in ranges {
        for x in r.first..=r.last {
            all[x as usize] = ids[&r.value];
        }
    }
    let mut page_ids = Vec::new();
    let mut pages: Vec<Vec<u8>> = Vec::new();
    let mut seen: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    for page in all.chunks_exact(256) {
        let key = page.to_vec();
        let id = *seen.entry(key.clone()).or_insert_with(|| {
            pages.push(key);
            pages.len() - 1
        });
        page_ids.push(id as u16);
    }
    writeln!(
        out,
        "pub(super) static {name}_VALUES: &[&str] = &{:?};",
        vals
    )
    .unwrap();
    writeln!(
        out,
        "pub(super) static {name}_PAGES: &[u16] = &{:?};",
        page_ids
    )
    .unwrap();
    writeln!(out, "pub(super) static {name}_DATA: &[[u8; 256]] = &[",).unwrap();
    for p in pages {
        out.push('[');
        for (i, b) in p.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write!(out, "{b}").unwrap();
        }
        out.push_str("],\n");
    }
    out.push_str("];\n");
    Ok(())
}
fn emit(root: &Path, d: &Data) -> Result<Vec<u8>> {
    let mut s = String::from(
        "// SPDX-License-Identifier: AGPL-3.0-or-later\n// Generated by atelier-ucd-gen from Unicode 17.0.0. Do not edit.\n",
    );
    let defs = [
        ("general_category", "Cn"),
        ("canonical_combining_class", "0"),
        ("line_break", "XX"),
        ("east_asian_width", "N"),
        ("grapheme_cluster_break", "Other"),
        ("word_break", "Other"),
        ("indic_conjunct_break", "None"),
        ("emoji", "N"),
        ("emoji_presentation", "N"),
        ("emoji_modifier", "N"),
        ("emoji_modifier_base", "N"),
        ("emoji_component", "N"),
        ("extended_pictographic", "N"),
        ("cased", "N"),
        ("case_ignorable", "N"),
        ("white_space", "N"),
    ];
    for (name, default) in defs {
        let rs = d
            .props
            .get(name)
            .ok_or_else(|| err(format!("missing property {name}")))?;
        let missing = d.missing.get(name).map(Vec::as_slice).unwrap_or(&[]);
        emit_page_table(&mut s, name, rs, missing, default)?;
    }
    // Store variable-length mappings and names in deterministic sorted pools.
    for (key, map) in &d.mappings {
        writeln!(
            s,
            "pub(super) static {}: &[(u32, &[u32])] = &[",
            key.to_ascii_uppercase()
        )
        .unwrap();
        for (n, seq) in map {
            writeln!(
                s,
                "(0x{n:X}, &[{}]),",
                seq.iter()
                    .map(|x| format!("0x{x:X}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .unwrap();
        }
        s.push_str("];\n");
    }
    writeln!(s, "pub(super) static NAMES: &[(u32, &str)] = &[").unwrap();
    for (n, name) in &d.names {
        writeln!(s, "(0x{n:X}, {:?}),", name).unwrap();
    }
    s.push_str("];\n");
    writeln!(
        s,
        "pub(super) static NAME_ALIASES: &[(u32, &[NameAlias])] = &["
    )
    .unwrap();
    for (scalar, aliases) in &d.aliases {
        let values = aliases
            .iter()
            .map(|(name, kind)| format!("NameAlias {{ name: {name:?}, kind: {kind:?} }}"))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(s, "(0x{scalar:X}, &[{values}]),").unwrap();
    }
    s.push_str("];\n");
    writeln!(
        s,
        "pub(super) static DECOMPOSITIONS: &[(u32, Option<&str>, &[u32])] = &["
    )
    .unwrap();
    for (n, (tag, seq)) in &d.decompositions {
        let vals = seq.iter().map(|x| format!("0x{x:X}")).collect::<Vec<_>>();
        writeln!(
            s,
            "(0x{n:X}, {}, &[{}]),",
            tag.as_ref()
                .map_or("None".to_string(), |t| format!("Some({t:?})")),
            vals.join(",")
        )
        .unwrap();
    }
    s.push_str("];\n");
    writeln!(
        s,
        "pub(super) static CONDITIONAL_CASE: &[ConditionalCasingRule] = &["
    )
    .unwrap();
    for (n, cond, lower, title, upper) in &d.conditionals {
        let sequence = |items: &[u32]| {
            items
                .iter()
                .map(|x| format!("0x{x:X}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        writeln!(
            s,
            "ConditionalCasingRule {{ code_point: 0x{n:X}, condition: {cond:?}, lower: &[{}], title: &[{}], upper: &[{}] }},",
            sequence(lower), sequence(title), sequence(upper)
        )
        .unwrap();
    }
    s.push_str("];\n");
    writeln!(
        s,
        "pub(super) static COMPOSITION_EXCLUSIONS: &[u32] = &[{}];",
        d.exclusions
            .iter()
            .map(|x| format!("0x{x:X}"))
            .collect::<Vec<_>>()
            .join(",")
    )
    .unwrap();
    writeln!(
        s,
        "pub(super) static CANONICAL_COMPOSITIONS: &[(u32, u32, u32)] = &["
    )
    .unwrap();
    for (a, b, c) in &d.compositions {
        writeln!(s, "(0x{a:X}, 0x{b:X}, 0x{c:X}),").unwrap();
    }
    s.push_str("];\n");
    let _ = root;
    if s.len() > MAX_OUTPUT_BYTES {
        return Err(err(format!("output limit exceeded: {} bytes", s.len())));
    }
    Ok(s.into_bytes())
}

/// Parse the official UCD tree and return deterministic Rust source.
pub fn generate(ucd: &Path) -> Result<Vec<u8>> {
    let root = fs::canonicalize(ucd)
        .map_err(|e| err(format!("invalid UCD directory {}: {e}", ucd.display())))?;
    let readme = source(&root, "ReadMe.txt")?;
    if !readme.contains(UNICODE_VERSION) {
        return Err(err(format!("UCD tree is not Unicode {UNICODE_VERSION}")));
    }
    let d = load(&root)?;
    emit(&root, &d)
}

/// Generate source and either atomically publish it or compare it with the checked-in version.
pub fn run(ucd: &Path, output: &Path, check: bool) -> Result<()> {
    let bytes = generate(ucd)?;
    if check {
        let current =
            fs::read(output).map_err(|e| err(format!("cannot read {}: {e}", output.display())))?;
        if current != bytes {
            return Err(err(format!(
                "generated output differs from {}",
                output.display()
            )));
        }
        return Ok(());
    }
    let parent = output.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| err(format!("cannot create output directory: {e}")))?;
    let mut tmp: PathBuf = output.as_os_str().to_owned().into();
    tmp.set_extension("rs.tmp");
    fs::write(&tmp, &bytes).map_err(|e| err(format!("cannot write temporary output: {e}")))?;
    if let Err(e) = fs::rename(&tmp, output) {
        let _ = fs::remove_file(&tmp);
        return Err(err(format!("cannot publish output: {e}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_inverted_and_out_of_range() {
        assert!(span("FFFF..0000").is_err());
        assert!(span("110000").is_err());
        assert!(scalar("D800").is_err());
    }
    #[test]
    fn validates_category_combining_class_and_first_last_records() {
        assert!(validate_gc_ccc("LX", "0").is_err());
        assert!(validate_gc_ccc("Lu", "256").is_err());
        let first = vec![
            "3400".to_string(),
            "<CJK, First>".to_string(),
            "Lo".to_string(),
            "0".to_string(),
        ];
        let matching = ["4DBF", "<CJK, Last>", "Lo", "0"];
        assert!(validate_range_pair(&first[1], matching[1], &first, &matching).is_ok());
        let wrong = ["4DBF", "<Other, Last>", "Lo", "0"];
        assert!(validate_range_pair(&first[1], wrong[1], &first, &wrong).is_err());
        let wrong_property = ["4DBF", "<CJK, Last>", "So", "0"];
        assert!(
            validate_range_pair(&first[1], wrong_property[1], &first, &wrong_property).is_err()
        );
    }
    #[test]
    fn version_is_fixed() {
        assert_eq!(UNICODE_VERSION, "17.0.0");
    }
    #[test]
    fn malformed_property_rows_values_and_defaults_are_rejected() {
        assert!(parse_property_record("0041;", "fixture", 0, None).is_err());
        assert!(parse_property_record("0041; Lu; extra", "fixture", 0, Some(2)).is_err());
        let mut aliases = ValueAliases::new();
        aliases.insert("lb", ["AL".to_string()].into_iter().collect());
        assert!(validate_enum_value(&aliases, "lb", "INVALID").is_err());
        assert!(
            property_file(
                &mut Data::default(),
                Path::new("."),
                "not-needed",
                "line_break",
                "INVALID",
                None,
                &aliases
            )
            .is_err()
        );
    }
    #[test]
    fn byte_value_table_accepts_all_256_ids() {
        let mut ranges = Vec::new();
        for value in 0..255 {
            ranges.push(Range {
                first: value,
                last: value,
                value: format!("{value:03}"),
            });
        }
        let mut out = String::new();
        assert!(emit_page_table(&mut out, "test", &ranges, &[], "default").is_ok());
    }
    #[test]
    fn check_refuses_missing_or_altered_output() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/ucd/17.0.0");
        let output =
            std::env::temp_dir().join(format!("atelier-ucd-gen-{}-missing.rs", std::process::id()));
        let _ = fs::remove_file(&output);
        assert!(run(&root, &output, true).is_err());
        fs::write(&output, b"altered").unwrap();
        assert!(run(&root, &output, true).is_err());
        let _ = fs::remove_file(&output);
    }
    #[test]
    fn wrong_version_fails_without_publishing_partial_output() {
        let root = std::env::temp_dir().join(format!("atelier-ucd-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("ReadMe.txt"), "Unicode Version 16.0.0\n").unwrap();
        let output = root.join("generated.rs");
        assert!(run(&root, &output, false).is_err());
        assert!(!output.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
