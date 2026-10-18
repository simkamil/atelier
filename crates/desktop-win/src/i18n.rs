// SPDX-License-Identifier: AGPL-3.0-or-later
//! Catalogue de messages du shell natif (US-102, `docs/native-i18n.md`).
//!
//! Lit le même fichier `web/i18n/fr-FR.txt` que l'interface web (embarqué avec `include_bytes!`,
//! déjà servi par l'exe) avec le même format `clé=valeur` et les mêmes paramètres nommés `{nom}`
//! que `web/i18n.mjs`. Les pluriels ICU ne sont pas pris en charge : aucune chaîne native n'en a
//! besoin, et un message qui en contient est « invalide » côté natif.
//!
//! Comportement sans panique : une clé absente ou un message invalide rend `⟦clé⟧` (comme le JS) ;
//! un paramètre manquant laisse le repère `{nom}` visible ; les valeurs de paramètres sont
//! insérées telles quelles, jamais réinterprétées.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Catalogue embarqué (le même octet pour octet que celui servi à la page web).
const EMBEDDED_CATALOG: &[u8] = include_bytes!("../../../web/i18n/fr-FR.txt");

/// Un incident d'analyse du catalogue (numérotation des lignes à partir de 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub line: usize,
    pub code: &'static str,
}

/// Messages d'un catalogue : `clé -> valeur brute` (compilée à la demande).
#[derive(Debug, Default)]
pub struct Catalog {
    entries: HashMap<String, String>,
    pub problems: Vec<Problem>,
}

#[derive(Debug, PartialEq, Eq)]
enum Node {
    Text(String),
    Param(String),
}

fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Même grammaire que `compileMessage` de `web/i18n.mjs`, sans pluriels.
fn compile(source: &str) -> Result<Vec<Node>, &'static str> {
    let chars: Vec<char> = source.chars().collect();
    let mut nodes = Vec::new();
    let mut text = String::new();
    let mut at = 0;
    while at < chars.len() {
        match chars[at] {
            '\\' => {
                match chars.get(at + 1) {
                    Some('n') => text.push('\n'),
                    Some(c @ ('\\' | '{' | '}' | '#')) => text.push(*c),
                    Some(_) => return Err("unknown escape"),
                    None => return Err("dangling backslash"),
                }
                at += 2;
            }
            '{' => {
                if !text.is_empty() {
                    nodes.push(Node::Text(std::mem::take(&mut text)));
                }
                at += 1;
                while chars.get(at).is_some_and(|c| c.is_whitespace()) {
                    at += 1;
                }
                let start = at;
                while chars
                    .get(at)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '='))
                {
                    at += 1;
                }
                let name: String = chars[start..at].iter().collect();
                if !valid_name(&name) {
                    return Err("invalid parameter name");
                }
                while chars.get(at).is_some_and(|c| c.is_whitespace()) {
                    at += 1;
                }
                match chars.get(at) {
                    Some('}') => at += 1,
                    Some(',') => return Err("plural is not supported natively"),
                    _ => return Err("expected '}' or ','"),
                }
                nodes.push(Node::Param(name));
            }
            '}' => return Err("unbalanced '}'"),
            c => {
                text.push(c);
                at += 1;
            }
        }
    }
    if !text.is_empty() {
        nodes.push(Node::Text(text));
    }
    Ok(nodes)
}

impl Catalog {
    /// Analyse `key=value` ; ne panique jamais. Les lignes invalides sont signalées et ignorées,
    /// la dernière occurrence d'une clé l'emporte, un BOM initial est ignoré.
    pub fn parse(text: &str) -> Self {
        let mut catalog = Self::default();
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        for (index, raw) in text.split('\n').enumerate() {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            let number = index + 1;
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                catalog.problems.push(Problem {
                    line: number,
                    code: "invalid-line",
                });
                continue;
            };
            if !valid_key(key) {
                catalog.problems.push(Problem {
                    line: number,
                    code: "invalid-key",
                });
                continue;
            }
            if catalog
                .entries
                .insert(key.to_owned(), value.to_owned())
                .is_some()
            {
                catalog.problems.push(Problem {
                    line: number,
                    code: "duplicate-key",
                });
            }
            if let Err(code) = compile(value) {
                // Seul le code est gardé : les natifs sont vérifiés par test, pas au vol.
                catalog.problems.push(Problem { line: number, code });
            }
        }
        catalog
    }

    pub fn has(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Texte de `key` avec les paramètres nommés `params`. Jamais de panique.
    pub fn render(&self, key: &str, params: &[(&str, &str)]) -> String {
        let marker = || format!("⟦{key}⟧");
        let Some(source) = self.entries.get(key) else {
            return marker();
        };
        let Ok(nodes) = compile(source) else {
            return marker();
        };
        let mut out = String::new();
        for node in nodes {
            match node {
                Node::Text(text) => out.push_str(&text),
                Node::Param(name) => match params.iter().find(|(n, _)| *n == name) {
                    Some((_, value)) => out.push_str(value),
                    None => {
                        out.push('{');
                        out.push_str(&name);
                        out.push('}');
                    }
                },
            }
        }
        out
    }
}

/// Catalogue embarqué, analysé une seule fois.
pub fn embedded() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| Catalog::parse(&String::from_utf8_lossy(EMBEDDED_CATALOG)))
}

/// Texte du shell natif pour `key` (paramètres nommés `{nom}`). Repli : `⟦clé⟧`.
pub fn t(key: &str, params: &[(&str, &str)]) -> String {
    embedded().render(key, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &str = include_str!("../../../tests/i18n-vectors.txt");

    fn unescape_expected(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some(other) => out.push(other),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Vecteurs communs avec `tests/i18n.mjs` : `message<TAB>nom=valeur;…<TAB>attendu` ou
    /// `!<TAB>message` (message invalide).
    #[test]
    fn shared_vectors_match_the_web_parser() {
        let mut checked = 0;
        for line in VECTORS.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            if fields[0] == "!" {
                assert!(compile(fields[1]).is_err(), "should be invalid: {line}");
            } else {
                let params: Vec<(&str, &str)> = fields[1]
                    .split(';')
                    .filter_map(|pair| pair.split_once('='))
                    .collect();
                let catalog = Catalog::parse(&format!("k={}", fields[0]));
                assert_eq!(
                    catalog.render("k", &params),
                    unescape_expected(fields[2]),
                    "vector: {line}"
                );
            }
            checked += 1;
        }
        assert!(checked >= 10, "vectors file is not empty");
    }

    #[test]
    fn catalog_parsing_matches_the_web_rules() {
        let text =
            "\u{feff}# comment\r\n\r\na.b-c_d=Un\\nDeux\r\nbad line\n=x\n-k=x\nk e=x\nk=1\nk=2\n";
        let catalog = Catalog::parse(text);
        assert_eq!(catalog.render("a.b-c_d", &[]), "Un\nDeux");
        assert_eq!(catalog.render("k", &[]), "2", "last occurrence wins");
        assert_eq!(
            catalog.problems,
            vec![
                Problem {
                    line: 4,
                    code: "invalid-line"
                },
                Problem {
                    line: 5,
                    code: "invalid-key"
                },
                Problem {
                    line: 6,
                    code: "invalid-key"
                },
                Problem {
                    line: 7,
                    code: "invalid-key"
                },
                Problem {
                    line: 9,
                    code: "duplicate-key"
                },
            ]
        );
        // La première `=` sépare, les suivantes appartiennent à la valeur.
        assert_eq!(Catalog::parse("e=a=b").render("e", &[]), "a=b");
    }

    #[test]
    fn parameters_escapes_and_fallbacks_never_panic() {
        let catalog = Catalog::parse(
            "p=Bonjour {nom}, {nom} !\nq=\\{x\\} \\# \\\\\nbad=C:\\temp\nplural={n, plural, one {#} other {#}}\n",
        );
        assert_eq!(
            catalog.render("p", &[("nom", "{x}\\n#")]),
            "Bonjour {x}\\n#, {x}\\n# !"
        );
        assert_eq!(catalog.render("p", &[]), "Bonjour {nom}, {nom} !");
        assert_eq!(catalog.render("q", &[]), "{x} # \\");
        assert_eq!(catalog.render("bad", &[]), "⟦bad⟧");
        assert_eq!(catalog.render("plural", &[("n", "1")]), "⟦plural⟧");
        assert_eq!(catalog.render("absent", &[]), "⟦absent⟧");
        assert!(catalog.has("p") && !catalog.has("absent"));
    }

    #[test]
    fn embedded_catalog_loads_and_native_messages_render() {
        let catalog = embedded();
        assert!(catalog.has("native.app.name"));
        assert_eq!(t("native.app.name", &[]), "Atelier");
        assert_eq!(t("native.missing.key", &[]), "⟦native.missing.key⟧");
    }

    /// Chaque clé `native.*` écrite en littéral dans `src/**/*.rs` existe dans le catalogue et se
    /// compile (hors fichier de tests des vecteurs).
    #[test]
    fn every_native_key_used_exists() {
        let catalog = embedded();
        let mut used = Vec::new();
        collect_native_keys(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut used,
        );
        assert!(used.len() >= 10, "native keys found: {used:?}");
        for key in used {
            assert!(catalog.has(&key), "clé native absente du catalogue : {key}");
            let source = catalog.entries.get(&key).unwrap();
            assert!(compile(source).is_ok(), "message natif invalide : {key}");
        }
    }

    fn collect_native_keys(dir: &std::path::Path, used: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_native_keys(&path, used);
            } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("i18n.rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                let mut rest = text.as_str();
                while let Some(at) = rest.find("\"native.") {
                    let after = &rest[at + 1..];
                    let end = after.find('"').unwrap();
                    let key = &after[..end];
                    if key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                    {
                        used.push(key.to_owned());
                    }
                    rest = &after[end..];
                }
            }
        }
    }
}
