// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-115 : protocole pur de l'instance unique (aucun FFI). Les chemins voyagent dans un
//! `WM_COPYDATA` : UTF-16 little-endian, chaque chemin terminé par un NUL, `dwData` = [`PROTOCOL_ID`].
//! Tout autre processus peut envoyer n'importe quelle charge : le décodage ne panique jamais.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf, Prefix};

/// `dwData` du `COPYDATASTRUCT` : « ATL1 ». Toute autre valeur est rejetée.
pub const PROTOCOL_ID: usize = 0x4154_4c31;
/// Nombre maximal de chemins par message et de chemins en attente côté fenêtre.
pub const MAX_PATHS: usize = 16;
/// Longueur maximale d'un chemin, en unités UTF-16, NUL final exclu (limite Windows étendue).
pub const MAX_PATH_UNITS: usize = 32_767;
/// Taille maximale d'une charge acceptée (au plus `MAX_PATHS` chemins pleins).
pub const MAX_PAYLOAD_BYTES: usize = MAX_PATHS * (MAX_PATH_UNITS + 1) * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathsError {
    BadProtocol,
    BadSender,
    TooLarge,
    TooMany,
    Malformed,
    NotAbsolute,
    Empty,
    EmbeddedNul,
}

/// Chemins d'ouverture parmi les arguments : les options (`--…`) et les arguments vides sont ignorés.
pub fn launch_arguments(arguments: &[OsString]) -> Vec<&OsString> {
    arguments
        .iter()
        .filter(|a| !a.is_empty() && !a.to_string_lossy().starts_with("--"))
        .collect()
}

/// Rend le chemin absolu par rapport à `cwd`, sans toucher au disque (le fichier peut ne pas exister
/// côté émetteur) ; supprime `.` et `..` lexicalement via `std::path::absolute`.
pub fn normalize(argument: &OsString, cwd: &Path) -> Result<PathBuf, PathsError> {
    if argument.is_empty() {
        return Err(PathsError::Empty);
    }
    let joined = cwd.join(argument);
    let absolute = std::path::absolute(&joined).map_err(|_| PathsError::NotAbsolute)?;
    if is_absolute_windows(&absolute) {
        Ok(absolute)
    } else {
        Err(PathsError::NotAbsolute)
    }
}

/// Un chemin Windows accepté est complet : `X:\…`, `\\serveur\partage\…` ou `\\?\…`.
fn is_absolute_windows(path: &Path) -> bool {
    let mut components = path.components();
    match components.next() {
        Some(Component::Prefix(prefix)) => {
            matches!(
                prefix.kind(),
                Prefix::Disk(_)
                    | Prefix::UNC(..)
                    | Prefix::VerbatimDisk(_)
                    | Prefix::VerbatimUNC(..)
                    | Prefix::Verbatim(_)
            ) && matches!(components.next(), Some(Component::RootDir))
        }
        _ => false,
    }
}

/// Encode les chemins pour `WM_COPYDATA`. Ne garde que des chemins déjà absolus ; au plus
/// [`MAX_PATHS`] chemins de [`MAX_PATH_UNITS`] unités.
pub fn encode_paths(paths: &[PathBuf]) -> Result<Vec<u8>, PathsError> {
    if paths.len() > MAX_PATHS {
        return Err(PathsError::TooMany);
    }
    let mut bytes = Vec::new();
    for path in paths {
        if !is_absolute_windows(path) {
            return Err(PathsError::NotAbsolute);
        }
        let units = encode_units(path);
        if units.is_empty() {
            return Err(PathsError::Empty);
        }
        if units.len() > MAX_PATH_UNITS {
            return Err(PathsError::TooLarge);
        }
        if units.contains(&0) {
            return Err(PathsError::EmbeddedNul);
        }
        for unit in units.iter().chain(std::iter::once(&0)) {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }
    Ok(bytes)
}

#[cfg(windows)]
fn encode_units(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().collect()
}

#[cfg(not(windows))]
fn encode_units(path: &Path) -> Vec<u16> {
    path.to_string_lossy().encode_utf16().collect()
}

/// Décode une charge reçue. Rejette : taille impaire ou excessive, absence de NUL final, chemin vide,
/// trop long, UTF-16 invalide (substitut isolé) ou non absolu, plus de [`MAX_PATHS`] chemins.
/// Une charge vide est valide (simple demande de mise au premier plan).
pub fn decode_paths(bytes: &[u8]) -> Result<Vec<PathBuf>, PathsError> {
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(PathsError::TooLarge);
    }
    if bytes.len() % 2 != 0 {
        return Err(PathsError::Malformed);
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if units.is_empty() {
        return Ok(Vec::new());
    }
    if units.last() != Some(&0) {
        return Err(PathsError::Malformed);
    }
    let mut paths = Vec::new();
    for chunk in units[..units.len() - 1].split(|unit| *unit == 0) {
        if paths.len() == MAX_PATHS {
            return Err(PathsError::TooMany);
        }
        if chunk.is_empty() {
            return Err(PathsError::Empty);
        }
        if chunk.len() > MAX_PATH_UNITS {
            return Err(PathsError::TooLarge);
        }
        let text = String::from_utf16(chunk).map_err(|_| PathsError::Malformed)?;
        let path = PathBuf::from(text);
        if !is_absolute_windows(&path) {
            return Err(PathsError::NotAbsolute);
        }
        paths.push(path);
    }
    Ok(paths)
}

/// Traitement pur d'un `WM_COPYDATA` : `data` est `None` si le pointeur est nul.
/// `sender_is_trusted` : le processus émetteur est bien cette même application.
pub fn process_copydata(
    protocol: usize,
    data: Option<&[u8]>,
    sender_is_trusted: bool,
) -> Result<Vec<PathBuf>, PathsError> {
    if protocol != PROTOCOL_ID {
        return Err(PathsError::BadProtocol);
    }
    if !sender_is_trusted {
        return Err(PathsError::BadSender);
    }
    decode_paths(data.unwrap_or(&[]))
}

/// Clé stable (FNV-1a 64 bits, hexadécimale) de la racine de données : une instance par utilisateur
/// (espace de noms `Local\` de la session) et par dossier de données, donc par mode et par copie portable.
pub fn instance_key(root: &Path) -> String {
    let text = root.to_string_lossy().replace('/', "\\");
    let text = text.strip_prefix("\\\\?\\").unwrap_or(&text);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for unit in text.to_lowercase().encode_utf16() {
        for byte in unit.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

pub fn mutex_name(key: &str) -> String {
    format!("Local\\Atelier.SingleInstance.{key}")
}

pub fn window_class_name(key: &str) -> String {
    format!("AtelierWindow.{key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> PathBuf {
        PathBuf::from(text)
    }

    #[test]
    fn round_trip_keeps_unicode_and_order() {
        let paths = vec![p(r"C:\Docs\é à — 日本.txt"), p(r"\\srv\share\a b.docx")];
        let bytes = encode_paths(&paths).unwrap();
        assert_eq!(decode_paths(&bytes).unwrap(), paths);
        assert_eq!(decode_paths(&[]).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn encode_rejects_relative_empty_too_many_and_too_long() {
        assert_eq!(encode_paths(&[p("a.txt")]), Err(PathsError::NotAbsolute));
        assert_eq!(encode_paths(&[p(r"C:a.txt")]), Err(PathsError::NotAbsolute));
        let many = vec![p(r"C:\a.txt"); MAX_PATHS + 1];
        assert_eq!(encode_paths(&many), Err(PathsError::TooMany));
        assert!(encode_paths(&vec![p(r"C:\a.txt"); MAX_PATHS]).is_ok());
        let long = p(&format!(r"C:\{}", "x".repeat(MAX_PATH_UNITS)));
        assert_eq!(encode_paths(&[long]), Err(PathsError::TooLarge));
        let at_limit = p(&format!(r"C:\{}", "x".repeat(MAX_PATH_UNITS - 3)));
        assert!(encode_paths(&[at_limit]).is_ok());
    }

    #[test]
    fn decode_rejects_hostile_payloads_without_panicking() {
        assert_eq!(decode_paths(&[0x43]), Err(PathsError::Malformed));
        assert_eq!(decode_paths(&[0x43, 0x00]), Err(PathsError::Malformed));
        assert_eq!(decode_paths(&[0, 0]), Err(PathsError::Empty));
        let mut lone = Vec::new();
        for unit in [0x43u16, 0x3a, 0x5c, 0xd800, 0] {
            lone.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_paths(&lone), Err(PathsError::Malformed));
        let mut relative = Vec::new();
        for unit in "dir\\a.txt".encode_utf16().chain([0]) {
            relative.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_paths(&relative), Err(PathsError::NotAbsolute));
        assert_eq!(
            decode_paths(&vec![0x41; MAX_PAYLOAD_BYTES + 2]),
            Err(PathsError::TooLarge)
        );
        let mut doubled = encode_paths(&[p(r"C:\a.txt")]).unwrap();
        doubled.extend_from_slice(&[0, 0]);
        assert_eq!(decode_paths(&doubled), Err(PathsError::Empty));
        let mut many = Vec::new();
        for _ in 0..=MAX_PATHS {
            many.extend(encode_paths(&[p(r"C:\a.txt")]).unwrap());
        }
        assert_eq!(decode_paths(&many), Err(PathsError::TooMany));
        let mut long = Vec::new();
        for unit in format!(r"C:\{}", "x".repeat(MAX_PATH_UNITS))
            .encode_utf16()
            .chain([0])
        {
            long.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_paths(&long), Err(PathsError::TooLarge));
        for seed in 0u32..2000 {
            let bytes: Vec<u8> = (0..(seed % 61))
                .map(|i| (seed.wrapping_mul(2_654_435_761).wrapping_add(i * 97) >> 7) as u8)
                .collect();
            let _ = decode_paths(&bytes);
        }
    }

    #[test]
    fn copydata_validates_protocol_sender_and_payload() {
        let bytes = encode_paths(&[p(r"C:\a.txt")]).unwrap();
        assert_eq!(
            process_copydata(PROTOCOL_ID, Some(&bytes), true).unwrap(),
            vec![p(r"C:\a.txt")]
        );
        assert_eq!(
            process_copydata(PROTOCOL_ID + 1, Some(&bytes), true),
            Err(PathsError::BadProtocol)
        );
        assert_eq!(
            process_copydata(0, Some(&bytes), true),
            Err(PathsError::BadProtocol)
        );
        assert_eq!(
            process_copydata(PROTOCOL_ID, Some(&bytes), false),
            Err(PathsError::BadSender)
        );
        assert_eq!(
            process_copydata(PROTOCOL_ID, None, true).unwrap(),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            process_copydata(PROTOCOL_ID, Some(&[1, 2, 3]), true),
            Err(PathsError::Malformed)
        );
    }

    #[cfg(windows)]
    #[test]
    fn launch_arguments_skip_options_and_normalize_against_cwd() {
        let args: Vec<OsString> = ["--x", "", "a.txt", r"D:\b.txt"].map(OsString::from).into();
        let kept = launch_arguments(&args);
        assert_eq!(kept.len(), 2);
        assert_eq!(
            normalize(kept[0], Path::new(r"C:\w\.\sub")).unwrap(),
            p(r"C:\w\sub\a.txt")
        );
        assert_eq!(
            normalize(&OsString::from(r"..\c.txt"), Path::new(r"C:\w\sub")).unwrap(),
            p(r"C:\w\c.txt")
        );
        assert_eq!(
            normalize(kept[1], Path::new(r"C:\w")).unwrap(),
            p(r"D:\b.txt")
        );
        assert_eq!(
            normalize(&OsString::new(), Path::new(r"C:\w")),
            Err(PathsError::Empty)
        );
    }

    #[test]
    fn key_is_stable_case_insensitive_and_distinguishes_roots() {
        let a = instance_key(Path::new(r"C:\Apps\Atelier\atelier-data"));
        assert_eq!(a, instance_key(Path::new(r"c:\apps\atelier\ATELIER-DATA")));
        assert_eq!(
            a,
            instance_key(Path::new(r"\\?\C:\Apps\Atelier\atelier-data"))
        );
        assert_ne!(
            a,
            instance_key(Path::new(r"C:\Users\x\AppData\Local\Atelier"))
        );
        assert_eq!(a.len(), 16);
        assert!(mutex_name(&a).starts_with("Local\\"));
        assert!(window_class_name(&a).ends_with(&a));
    }
}
