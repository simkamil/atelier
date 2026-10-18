// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-113 : ouverture d'une URL dans le navigateur par défaut. Seuls `http:` et `https:` sont
//! acceptés ; l'URL est bornée et limitée aux caractères ASCII visibles d'une URL déjà encodée.
//! Les clés d'erreur sont stables et traduisibles (`url.error.*`).

pub const MAX_URL_BYTES: usize = 2048;

/// Valide l'URL sans la modifier. Refus : vide, trop longue, caractère hors ASCII visible ou
/// dangereux pour une ligne de commande, schéma autre que http/https, hôte vide ou à identifiants.
pub fn validate_url(url: &str) -> Result<&str, &'static str> {
    if url.is_empty() {
        return Err("url.error.invalid");
    }
    if url.len() > MAX_URL_BYTES {
        return Err("url.error.too_long");
    }
    let forbidden = |byte: u8| {
        !(0x21..=0x7e).contains(&byte)
            || matches!(
                byte,
                b'"' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'}' | b'|'
            )
    };
    if url.bytes().any(forbidden) {
        return Err("url.error.invalid");
    }
    let lower = url.to_ascii_lowercase();
    let rest = if let Some(rest) = lower.strip_prefix("https://") {
        rest
    } else if let Some(rest) = lower.strip_prefix("http://") {
        rest
    } else if let Some(scheme_end) = lower.find(':').filter(|_| !lower.starts_with("//")) {
        // Un schéma (javascript:, file:, ms-msdt:, mailto: …) autre que http/https.
        let scheme = &lower[..scheme_end];
        let scheme_like = !scheme.is_empty()
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        return Err(if scheme_like {
            "url.error.scheme_not_allowed"
        } else {
            "url.error.invalid"
        });
    } else {
        return Err("url.error.invalid");
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty()
        || authority.starts_with(':')
        || !authority
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
    {
        return Err("url.error.invalid");
    }
    Ok(url)
}

/// Traduit la valeur renvoyée par `ShellExecuteW` (valeur d'un HINSTANCE : `> 32` = succès).
pub fn map_shell_result(code: usize) -> Result<(), &'static str> {
    match code {
        33.. => Ok(()),
        2 | 3 | 31 => Err("url.error.no_handler"),
        5 | 26 | 27 => Err("url.error.access_denied"),
        _ => Err("url.error.launch_failed"),
    }
}

/// Valide puis lance via `launcher` (injectable en test) ; le lanceur renvoie le code `ShellExecuteW`.
pub fn open_url_with(url: &str, launcher: impl FnOnce(&str) -> usize) -> Result<(), &'static str> {
    let url = validate_url(url)?;
    map_shell_result(launcher(url))
}

/// Ouvre l'URL validée dans le navigateur par défaut (fil UI, STA).
#[cfg(windows)]
pub fn open_in_browser(url: &str) -> Result<(), &'static str> {
    open_url_with(url, |url| {
        use std::ffi::c_void;
        use std::ptr::null_mut;
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn ShellExecuteW(
                hwnd: *mut c_void,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show: i32,
            ) -> *mut c_void;
        }
        let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        let file: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
        // SAFETY: les deux chaînes sont terminées par NUL et vivantes pendant l'appel synchrone ;
        // les paramètres optionnels sont nuls ; SW_SHOWNORMAL = 1. Le résultat est un HINSTANCE
        // dont seule la valeur numérique est interprétée (> 32 = succès), jamais déréférencée.
        let result = unsafe {
            ShellExecuteW(
                null_mut(),
                operation.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        result as usize
    })
}

#[cfg(not(windows))]
pub fn open_in_browser(url: &str) -> Result<(), &'static str> {
    validate_url(url)?;
    Err("url.error.launch_failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https_only() {
        for good in [
            "https://example.org",
            "http://example.org/a?b=c#d",
            "HTTPS://Example.ORG:8443/path%20x",
            "https://[::1]:8080/",
            "https://a-b.example.org/%C3%A9",
        ] {
            assert_eq!(validate_url(good), Ok(good), "{good}");
        }
        for (bad, key) in [
            ("", "url.error.invalid"),
            ("javascript:alert(1)", "url.error.scheme_not_allowed"),
            (
                "file:///C:/Windows/notepad.exe",
                "url.error.scheme_not_allowed",
            ),
            ("mailto:a@b.c", "url.error.scheme_not_allowed"),
            ("ms-msdt:/id", "url.error.scheme_not_allowed"),
            ("ftp://example.org", "url.error.scheme_not_allowed"),
            ("example.org", "url.error.invalid"),
            ("//example.org", "url.error.invalid"),
            ("https://", "url.error.invalid"),
            ("https:///path", "url.error.invalid"),
            ("https://user:pw@example.org", "url.error.invalid"),
            ("https://exa mple.org", "url.error.invalid"),
            ("https://example.org/\"x", "url.error.invalid"),
            ("https://example.org/\r\nx", "url.error.invalid"),
            ("https://exämple.org", "url.error.invalid"),
            ("https://exa%6Dple.org", "url.error.invalid"),
            ("https://example.org/a\\b", "url.error.invalid"),
        ] {
            assert_eq!(validate_url(bad), Err(key), "{bad:?}");
        }
    }

    #[test]
    fn length_is_bounded() {
        let ok = format!("https://e.org/{}", "a".repeat(MAX_URL_BYTES - 14));
        assert_eq!(ok.len(), MAX_URL_BYTES);
        assert!(validate_url(&ok).is_ok());
        assert_eq!(validate_url(&format!("{ok}a")), Err("url.error.too_long"));
    }

    #[test]
    fn shell_results_map_to_stable_keys() {
        assert_eq!(map_shell_result(42), Ok(()));
        assert_eq!(map_shell_result(33), Ok(()));
        assert_eq!(map_shell_result(31), Err("url.error.no_handler"));
        assert_eq!(map_shell_result(2), Err("url.error.no_handler"));
        assert_eq!(map_shell_result(5), Err("url.error.access_denied"));
        assert_eq!(map_shell_result(0), Err("url.error.launch_failed"));
        assert_eq!(map_shell_result(8), Err("url.error.launch_failed"));
    }

    #[test]
    fn launcher_only_sees_validated_urls() {
        let mut seen = None;
        assert_eq!(
            open_url_with("https://example.org/x", |url| {
                seen = Some(url.to_owned());
                33
            }),
            Ok(())
        );
        assert_eq!(seen.as_deref(), Some("https://example.org/x"));
        let called = std::cell::Cell::new(false);
        assert_eq!(
            open_url_with("javascript:1", |_| {
                called.set(true);
                33
            }),
            Err("url.error.scheme_not_allowed")
        );
        assert!(!called.get(), "a refused URL must never reach the shell");
        assert_eq!(
            open_url_with("https://example.org", |_| 31),
            Err("url.error.no_handler")
        );
    }
}
