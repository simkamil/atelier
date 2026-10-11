// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compile-time web resource table and strict request policy for the embedded origin.

use std::collections::VecDeque;

pub const ORIGIN: &str = "https://app.atelier/";
pub const CSP: &str = "default-src 'none'; base-uri 'none'; object-src 'none'; frame-src 'none'; form-action 'none'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src https://app.atelier; navigate-to 'self'";
pub const MAX_LOG_ENTRIES: usize = 64;
const MAX_LOG_ENTRY_CHARS: usize = 192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Serve(usize),
    NotFound,
    MethodNotAllowed,
    Refused,
}

#[derive(Clone, Copy, Debug)]
pub struct Resource {
    pub path: &'static str,
    pub mime: &'static str,
    pub bytes: &'static [u8],
}

// Every URL here is an explicit, immutable route. No directory or prefix serving exists.
pub static RESOURCES: &[Resource] = &[
    Resource {
        path: "/",
        mime: "text/html; charset=utf-8",
        bytes: include_bytes!("../../../web/index.html"),
    },
    Resource {
        path: "/app.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/app.mjs"),
    },
    Resource {
        path: "/app.css",
        mime: "text/css; charset=utf-8",
        bytes: include_bytes!("../../../web/app.css"),
    },
    Resource {
        path: "/i18n/fr-FR.txt",
        mime: "text/plain; charset=utf-8",
        bytes: include_bytes!("../../../web/i18n/fr-FR.txt"),
    },
    Resource {
        path: "/engine-worker.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/engine-worker.mjs"),
    },
    Resource {
        path: "/engine-client.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/engine-client.mjs"),
    },
    Resource {
        path: "/wasm-loader.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/wasm-loader.mjs"),
    },
    Resource {
        path: "/wasm-protocol.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/wasm-protocol.mjs"),
    },
    Resource {
        path: "/editor-protocol.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/editor-protocol.mjs"),
    },
    Resource {
        path: "/editor.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/editor.mjs"),
    },
    Resource {
        path: "/native-host.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/native-host.mjs"),
    },
    Resource {
        path: "/display-list.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/display-list.mjs"),
    },
    Resource {
        path: "/canvas-renderer.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/canvas-renderer.mjs"),
    },
    Resource {
        path: "/text-input.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/text-input.mjs"),
    },
    Resource {
        path: "/mouse-selection.mjs",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../web/mouse-selection.mjs"),
    },
    Resource {
        path: "/atelier_core_wasm.wasm",
        mime: "application/wasm",
        bytes: include_bytes!("../../../data/engine/atelier_core_wasm.wasm"),
    },
    Resource {
        path: "/fonts/Carlito-Regular.ttf",
        mime: "font/ttf",
        bytes: include_bytes!("../../../data/fonts/carlito/Carlito-Regular.ttf"),
    },
    Resource {
        path: "/licenses/Carlito-OFL.txt",
        mime: "text/plain; charset=utf-8",
        bytes: include_bytes!("../../../data/fonts/licenses/Carlito-OFL.txt"),
    },
];

/// Strictly classify a request URL and method. Queries and fragments do not affect routing.
pub fn classify(url: &str, method: &str) -> Decision {
    if !url.starts_with(ORIGIN) {
        return Decision::Refused;
    }
    let rest = &url[ORIGIN.len()..];
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let path = &rest[..end];
    if path
        .bytes()
        .any(|b| b == b'%' || b == b'\\' || b.is_ascii_control())
        || path.split('/').any(|part| part == "." || part == "..")
        || path.contains("//")
        || path.starts_with('/')
    {
        return Decision::Refused;
    }
    match method {
        "GET" | "HEAD" => match RESOURCES
            .iter()
            .position(|r| r.path.strip_prefix('/').unwrap_or(r.path) == path)
        {
            Some(i) => Decision::Serve(i),
            None => Decision::NotFound,
        },
        _ => Decision::MethodNotAllowed,
    }
}

/// A bounded local diagnostic log. It deliberately drops query and fragment values.
#[derive(Default, Debug)]
pub struct AttemptLog {
    entries: VecDeque<String>,
    dropped: u64,
}

impl AttemptLog {
    pub fn record(&mut self, url: &str, decision: Decision) {
        let safe = redact_url(url);
        let suffix = match decision {
            Decision::Refused => " [refused]",
            Decision::NotFound => " [404]",
            Decision::MethodNotAllowed => " [405]",
            Decision::Serve(_) => " [served]",
        };
        let mut entry: String = safe
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_LOG_ENTRY_CHARS.saturating_sub(suffix.len()))
            .collect();
        if entry.is_empty() {
            entry.push_str("<invalid-url>");
        }
        entry.push_str(suffix);
        self.push(entry);
    }
    pub fn record_csp_violation(&mut self, directive: &str, blocked_uri: &str) {
        if !matches!(
            directive,
            "default-src"
                | "base-uri"
                | "object-src"
                | "frame-src"
                | "form-action"
                | "script-src"
                | "script-src-elem"
                | "script-src-attr"
                | "worker-src"
                | "style-src"
                | "style-src-elem"
                | "style-src-attr"
                | "font-src"
                | "img-src"
                | "connect-src"
                | "navigate-to"
        ) {
            return;
        }
        let safe = redact_url(blocked_uri);
        let prefix = format!("CSP {directive} ");
        let suffix = " [blocked]";
        let available = MAX_LOG_ENTRY_CHARS.saturating_sub(prefix.chars().count() + suffix.len());
        let mut entry = prefix;
        entry.extend(safe.chars().take(available));
        entry.push_str(suffix);
        self.push(entry);
    }
    fn push(&mut self, entry: String) {
        if self.entries.len() == MAX_LOG_ENTRIES {
            self.entries.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.entries.push_back(entry);
    }
    pub fn entries(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(String::as_str)
    }
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

fn redact_url(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or("");
    let redacted = if let Some((scheme, authority_and_path)) = without_query.split_once("://") {
        let authority_end = authority_and_path
            .find('/')
            .unwrap_or(authority_and_path.len());
        let authority = &authority_and_path[..authority_end];
        let safe_authority = authority.rsplit('@').next().unwrap_or(authority);
        format!(
            "{scheme}://{safe_authority}{}",
            &authority_and_path[authority_end..]
        )
    } else {
        without_query.to_owned()
    };
    redacted.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_resource_method_and_denial_matrix() {
        assert!(matches!(
            classify("https://app.atelier/", "GET"),
            Decision::Serve(_)
        ));
        assert!(matches!(
            classify("https://app.atelier/app.mjs", "HEAD"),
            Decision::Serve(_)
        ));
        assert_eq!(
            classify("https://app.atelier/missing", "GET"),
            Decision::NotFound
        );
        assert_eq!(
            classify("https://app.atelier/app.mjs", "POST"),
            Decision::MethodNotAllowed
        );
        for url in [
            "https://example.com/",
            "https://app.atelier.evil/",
            "https://user@app.atelier/",
            "https://app.atelier:443/",
            "https://app.atelier/%2e%2e/app.mjs",
            "https://app.atelier/../app.mjs",
            "https://app.atelier//app.mjs",
            "https://app.atelier\\evil/",
        ] {
            assert_eq!(classify(url, "GET"), Decision::Refused, "{url}");
        }
        assert!(matches!(
            classify("https://app.atelier/app.mjs?v=1#x", "GET"),
            Decision::Serve(_)
        ));
        assert!(
            RESOURCES
                .iter()
                .all(|r| !r.mime.is_empty() && !r.bytes.is_empty())
        );
        assert!(
            CSP.contains("default-src 'none'") && CSP.contains("connect-src https://app.atelier")
        );
    }

    #[test]
    fn attempt_log_is_bounded_and_redacts_query_and_fragment() {
        let mut log = AttemptLog::default();
        log.record(
            "https://outside.test/?token=secret#private",
            Decision::Refused,
        );
        assert!(
            log.entries()
                .next()
                .unwrap()
                .contains("https://outside.test/ [refused]")
        );
        assert!(!log.entries().next().unwrap().contains("secret"));
        log.record(
            "https://user:password@outside.test/path?token=secret#private",
            Decision::Refused,
        );
        assert!(
            log.entries()
                .last()
                .unwrap()
                .contains("https://outside.test/path [refused]")
        );
        assert!(!log.entries().last().unwrap().contains("password"));
        log.record_csp_violation(
            "connect-src",
            "https://user:password@outside.test/probe?private=1#fragment",
        );
        assert!(
            log.entries()
                .last()
                .unwrap()
                .contains("CSP connect-src https://outside.test/probe [blocked]")
        );
        assert!(!log.entries().last().unwrap().contains("password"));
        for _ in 0..MAX_LOG_ENTRIES + 3 {
            log.record("https://outside.test/", Decision::Refused);
        }
        assert_eq!(log.entries().count(), MAX_LOG_ENTRIES);
        log.record(
            &format!("https://outside.test/{}", "x".repeat(300)),
            Decision::Refused,
        );
        assert!(log.entries().last().unwrap().len() <= MAX_LOG_ENTRY_CHARS);
        assert_eq!(log.dropped(), 7);
    }
}
