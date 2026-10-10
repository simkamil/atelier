// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();
    let root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if args.next().is_some() {
        eprintln!("usage: atelier-check-workspace [ROOT]");
        return ExitCode::FAILURE;
    }
    match check(&root) {
        Ok(()) => {
            println!("Cargo.lock contains only workspace packages.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("workspace lock check failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn check(root: &Path) -> Result<(), String> {
    let lock_bytes =
        fs::read(root.join("Cargo.lock")).map_err(|e| format!("cannot read Cargo.lock: {e}"))?;
    let lock = std::str::from_utf8(&lock_bytes)
        .map_err(|_| "Cargo.lock is not valid UTF-8".to_string())?;
    let locked = parse_lock(lock)?;
    if let Some(package) = locked.iter().find(|package| package.source.is_some()) {
        return Err(format!(
            "external package is forbidden: {} {} ({})",
            package.name,
            package.version,
            package.source.as_deref().unwrap_or_default()
        ));
    }
    let output = Command::new("cargo")
        .current_dir(root)
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let json = std::str::from_utf8(&output.stdout)
        .map_err(|_| "cargo metadata returned invalid UTF-8".to_string())?;
    let workspace = workspace_packages(json)?;
    let mut locked_members = BTreeSet::new();
    for package in locked {
        if !workspace.contains(&(package.name.clone(), package.version.clone())) {
            return Err(format!(
                "package is not a workspace member: {} {}",
                package.name, package.version
            ));
        }
        locked_members.insert((package.name, package.version));
    }
    if let Some(member) = workspace.difference(&locked_members).next() {
        return Err(format!(
            "workspace package is missing from Cargo.lock: {} {}",
            member.0, member.1
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct Package {
    name: String,
    version: String,
    source: Option<String>,
}

fn parse_lock(text: &str) -> Result<Vec<Package>, String> {
    let mut format = None;
    let mut packages = Vec::new();
    let mut current: Option<BTreeMap<String, String>> = None;
    let mut in_dependencies = false;
    let mut dependencies_need_close = false;
    let finish = |section: &mut Option<BTreeMap<String, String>>,
                  out: &mut Vec<Package>|
     -> Result<(), String> {
        if let Some(fields) = section.take() {
            let name = fields.get("name").ok_or("package is missing name")?.clone();
            let version = fields
                .get("version")
                .ok_or("package is missing version")?
                .clone();
            let source = fields.get("source").cloned();
            out.push(Package {
                name,
                version,
                source,
            });
        }
        Ok(())
    };
    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if in_dependencies {
            if line == "]" {
                in_dependencies = false;
                dependencies_need_close = false;
            } else if dependencies_need_close {
                return Err(format!(
                    "missing comma between dependencies on line {}",
                    line_no + 1
                ));
            } else if line.strip_suffix(',').unwrap_or(line).starts_with('"') {
                let (entry, trailing) = if let Some(value) = line.strip_suffix(',') {
                    (value, true)
                } else {
                    (line, false)
                };
                if quoted(entry).is_none() {
                    return Err(format!("invalid dependency entry on line {}", line_no + 1));
                }
                dependencies_need_close = !trailing;
            } else {
                return Err(format!(
                    "unsupported dependencies syntax on line {}",
                    line_no + 1
                ));
            }
            continue;
        }
        if line == "[[package]]" {
            finish(&mut current, &mut packages)?;
            current = Some(BTreeMap::new());
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("unsupported Cargo.lock syntax on line {}", line_no + 1))?;
        let key = key.trim();
        let value = value.trim();
        if current.is_none() {
            if key == "version" {
                if format.is_some() {
                    return Err("duplicate top-level lock format version".into());
                }
                format = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| "invalid lock format version")?,
                );
            } else {
                return Err(format!("unsupported top-level lock field '{key}'"));
            }
            continue;
        }
        let fields = current.as_mut().unwrap();
        if key == "dependencies" {
            if fields.contains_key(key) {
                return Err("duplicate package field 'dependencies'".into());
            }
            if value == "[]" {
                fields.insert(key.to_string(), String::new());
                continue;
            }
            if value == "[" {
                fields.insert(key.to_string(), String::new());
                in_dependencies = true;
                continue;
            }
            return Err("unsupported lock dependencies syntax".into());
        }
        if !matches!(key, "name" | "version" | "source" | "checksum") {
            return Err(format!("unsupported package field '{key}'"));
        }
        let decoded = quoted(value)
            .ok_or_else(|| format!("invalid quoted value for '{key}' on line {}", line_no + 1))?;
        if fields.insert(key.to_string(), decoded).is_some() {
            return Err(format!("duplicate package field '{key}'"));
        }
    }
    if in_dependencies {
        return Err("unterminated lock dependencies array".into());
    }
    finish(&mut current, &mut packages)?;
    if format != Some(4) {
        return Err(format!(
            "unsupported Cargo.lock format {:?}; expected 4",
            format
        ));
    }
    if packages.is_empty() {
        return Err("Cargo.lock contains no packages".into());
    }
    let mut identities = BTreeSet::new();
    for p in &packages {
        if !identities.insert((p.name.clone(), p.version.clone(), p.source.clone())) {
            return Err(format!(
                "duplicate package identity: {} {}",
                p.name, p.version
            ));
        }
    }
    Ok(packages)
}

fn quoted(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                _ => return None,
            }
        } else if c == '"' || c.is_control() {
            return None;
        } else {
            out.push(c);
        }
    }
    Some(out)
}

#[derive(Debug)]
enum Json {
    Object(BTreeMap<String, Json>),
    Array(Vec<Json>),
    String(String),
    Null,
    Other,
}
fn workspace_packages(text: &str) -> Result<BTreeSet<(String, String)>, String> {
    let mut parser = JsonParser {
        bytes: text.as_bytes(),
        at: 0,
        depth: 0,
    };
    let root = parser.value()?;
    parser.ws();
    if parser.at != parser.bytes.len() {
        return Err("trailing data in cargo metadata JSON".into());
    }
    let object = as_object(&root).ok_or("cargo metadata root is not an object")?;
    let packages = as_array(
        object
            .get("packages")
            .ok_or("cargo metadata has no packages")?,
    )
    .ok_or("cargo metadata packages is not an array")?;
    let members = as_array(
        object
            .get("workspace_members")
            .ok_or("cargo metadata has no workspace_members")?,
    )
    .ok_or("cargo metadata workspace_members is not an array")?;
    let member_ids: BTreeSet<&str> = members
        .iter()
        .map(|v| as_string(v).ok_or("invalid workspace member id"))
        .collect::<Result<_, _>>()?;
    let mut result = BTreeSet::new();
    for p in packages {
        let p = as_object(p).ok_or("invalid cargo metadata package")?;
        let id = as_string(p.get("id").ok_or("metadata package is missing id")?)
            .ok_or("invalid metadata package id")?;
        if !member_ids.contains(id) {
            continue;
        }
        if !matches!(p.get("source"), Some(Json::Null)) {
            return Err("workspace package unexpectedly has a source".into());
        }
        let name = as_string(p.get("name").ok_or("metadata package is missing name")?)
            .ok_or("invalid metadata package name")?;
        let version = as_string(
            p.get("version")
                .ok_or("metadata package is missing version")?,
        )
        .ok_or("invalid metadata package version")?;
        result.insert((name.to_string(), version.to_string()));
    }
    if result.is_empty() {
        return Err("cargo metadata returned no workspace packages".into());
    }
    Ok(result)
}
fn as_object(j: &Json) -> Option<&BTreeMap<String, Json>> {
    if let Json::Object(v) = j {
        Some(v)
    } else {
        None
    }
}
fn as_array(j: &Json) -> Option<&[Json]> {
    if let Json::Array(v) = j {
        Some(v)
    } else {
        None
    }
}
fn as_string(j: &Json) -> Option<&str> {
    if let Json::String(v) = j {
        Some(v)
    } else {
        None
    }
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: usize,
}
impl JsonParser<'_> {
    fn ws(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }
    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        self.depth += 1;
        if self.depth > 128 {
            return Err("cargo metadata JSON is nested too deeply".into());
        }
        let result = match self.bytes.get(self.at).copied() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => {
                self.literal(b"true")?;
                Ok(Json::Other)
            }
            Some(b'f') => {
                self.literal(b"false")?;
                Ok(Json::Other)
            }
            Some(b'n') => {
                self.literal(b"null")?;
                Ok(Json::Null)
            }
            Some(b'-' | b'0'..=b'9') => {
                self.number()?;
                Ok(Json::Other)
            }
            _ => Err("invalid cargo metadata JSON".into()),
        };
        self.depth -= 1;
        result
    }
    fn object(&mut self) -> Result<Json, String> {
        self.at += 1;
        let mut map = BTreeMap::new();
        self.ws();
        if self.take(b'}') {
            return Ok(Json::Object(map));
        }
        loop {
            self.ws();
            let key = self.string()?;
            self.ws();
            if !self.take(b':') {
                return Err("invalid JSON object".into());
            }
            let val = self.value()?;
            if map.insert(key, val).is_some() {
                return Err("duplicate JSON key".into());
            }
            self.ws();
            if self.take(b'}') {
                break;
            }
            if !self.take(b',') {
                return Err("invalid JSON object separator".into());
            }
        }
        Ok(Json::Object(map))
    }
    fn array(&mut self) -> Result<Json, String> {
        self.at += 1;
        let mut values = Vec::new();
        self.ws();
        if self.take(b']') {
            return Ok(Json::Array(values));
        }
        loop {
            values.push(self.value()?);
            self.ws();
            if self.take(b']') {
                break;
            }
            if !self.take(b',') {
                return Err("invalid JSON array separator".into());
            }
        }
        Ok(Json::Array(values))
    }
    fn string(&mut self) -> Result<String, String> {
        if !self.take(b'"') {
            return Err("expected JSON string".into());
        }
        let mut out = String::new();
        loop {
            let b = *self.bytes.get(self.at).ok_or("unterminated JSON string")?;
            self.at += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = *self.bytes.get(self.at).ok_or("bad JSON escape")?;
                    self.at += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self
                                .bytes
                                .get(self.at..self.at + 4)
                                .ok_or("bad Unicode escape")?;
                            self.at += 4;
                            let s = std::str::from_utf8(hex).map_err(|_| "bad Unicode escape")?;
                            let n = u16::from_str_radix(s, 16).map_err(|_| "bad Unicode escape")?;
                            let ch =
                                char::from_u32(n as u32).ok_or("unsupported surrogate escape")?;
                            out.push(ch)
                        }
                        _ => return Err("bad JSON escape".into()),
                    }
                }
                0..=31 => return Err("control byte in JSON string".into()),
                0x20..=0x7f => out.push(b as char),
                _ => {
                    self.at -= 1;
                    let tail = std::str::from_utf8(&self.bytes[self.at..])
                        .map_err(|_| "invalid UTF-8 in JSON")?;
                    let ch = tail.chars().next().ok_or("invalid UTF-8")?;
                    out.push(ch);
                    self.at += ch.len_utf8();
                }
            }
        }
    }
    fn number(&mut self) -> Result<(), String> {
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| matches!(b, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'))
        {
            self.at += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|_| ())
            .ok_or_else(|| "invalid JSON number".into())
    }
    fn literal(&mut self, expected: &[u8]) -> Result<(), String> {
        if self.bytes.get(self.at..self.at + expected.len()) == Some(expected) {
            self.at += expected.len();
            Ok(())
        } else {
            Err("invalid JSON literal".into())
        }
    }
    fn take(&mut self, b: u8) -> bool {
        if self.bytes.get(self.at) == Some(&b) {
            self.at += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_workspace_lock_and_local_dependency_entries() {
        let s = "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\ndependencies = [\n \"b 1.0.0\",\n]\n\n[[package]]\nname = \"b\"\nversion = \"1.0.0\"\n";
        assert_eq!(parse_lock(s).unwrap().len(), 2);
    }
    #[test]
    fn rejects_external_source_even_if_identity_matches() {
        let s = "version = 4\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\nsource = \"registry+https://example.invalid\"\n";
        assert!(parse_lock(s).unwrap()[0].source.is_some());
    }
    #[test]
    fn rejects_missing_malformed_duplicate_and_unknown_lock_format() {
        for s in [
            "",
            "version = 4\n[[package]]\nname = \"a\"\n",
            "version = 4\n[[package]]\nname = \"a\"\nname = \"b\"\nversion = \"1\"\n",
            "version = 4\nversion = 4\n[[package]]\nname = \"a\"\nversion = \"1\"\n",
            "version = 99\n[[package]]\nname = \"a\"\nversion = \"1\"\n",
        ] {
            assert!(parse_lock(s).is_err(), "{s}");
        }
    }
    #[test]
    fn rejects_invalid_dependency_arrays_and_quoted_values() {
        for lock in [
            "version = 4\n[[package]]\nname = \"a\"\nversion = \"1\"\ndependencies = []\ndependencies = []\n",
            "version = 4\n[[package]]\nname = \"a\"\nversion = \"1\"\ndependencies = [\n \"a\"oops\",\n]\n",
            "version = 4\n[[package]]\nname = \"a\"\nversion = \"1\"\ndependencies = [\n \"a\"\n \"b\",\n]\n",
            "version = 4\n[[package]]\nname = \"a\"\nversion = \"1\"\nsource = \"bad\"value\"\n",
            "version = 4\n[[package]]\nname = \"a\"\nversion = \"1\"\nsource = \"bad\tvalue\"\n",
            "version = 4\n[[package]]\nname = \"a\nname\"\nversion = \"1\"\n",
        ] {
            assert!(
                parse_lock(lock).is_err(),
                "accepted malformed lock: {lock:?}"
            );
        }
    }
    #[test]
    fn parses_json_structure_and_member_ids() {
        let s = r#"{"packages":[{"id":"opaque 1","name":"a","version":"1","source":null}],"workspace_members":["opaque 1"]}"#;
        assert!(
            workspace_packages(s)
                .unwrap()
                .contains(&("a".into(), "1".into()))
        );
    }
    #[test]
    fn rejects_bad_json_and_missing_workspace_fields() {
        for s in ["{", r#"{"packages":[],"workspace_members":[]}"#] {
            assert!(workspace_packages(s).is_err());
        }
    }
}
