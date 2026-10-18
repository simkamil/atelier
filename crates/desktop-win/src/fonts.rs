// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-113 : polices système. Le natif lit les dossiers de polices (système puis utilisateur), extrait
//! famille et style de la table `name` (sans GDI ni DirectWrite) et n'expose au JS que des
//! identifiants opaques ; le JS ne fournit jamais de chemin. Toute lecture est bornée.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

/// Taille maximale d'une police transmise (les polices CJK atteignent quelques dizaines de Mio).
pub const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;
/// Nombre maximal d'entrées (faces) retenues.
pub const MAX_FONTS: usize = 4096;
const MAX_NAME_TABLE: usize = 256 * 1024;
const MAX_TABLES: usize = 128;
const MAX_FACES_PER_FILE: usize = 32;
const MAX_NAME_RECORDS: usize = 2048;
const MAX_TEXT_CHARS: usize = 128;

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontEntry {
    pub family: String,
    pub style: String,
    pub face: u32,
    pub size: u64,
    modified: Option<SystemTime>,
    path: PathBuf,
}

/// Instantané de la liste des polices. Les identifiants (`f<génération>-<index>`) ne sont valables
/// que pour cet instantané ; un nouveau balayage change de génération.
#[derive(Debug)]
pub struct FontCatalog {
    generation: u64,
    entries: Vec<FontEntry>,
}

/// Dossiers de polices : système (`%WINDIR%\Fonts`) puis utilisateur
/// (`%LOCALAPPDATA%\Microsoft\Windows\Fonts`).
pub fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let windir = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot"));
    if let Some(windir) = windir {
        dirs.push(PathBuf::from(windir).join("Fonts"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
    }
    dirs
}

fn is_font_extension(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy().to_ascii_lowercase();
        matches!(extension.as_str(), "ttf" | "otf" | "ttc" | "otc")
    })
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn clean(text: &str) -> String {
    let replaced: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    replaced.trim().chars().take(MAX_TEXT_CHARS).collect()
}

fn decode_name(platform: u16, raw: &[u8]) -> Option<String> {
    let text = match platform {
        0 | 3 => {
            if raw.len() % 2 != 0 {
                return None;
            }
            let units: Vec<u16> = raw
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16(&units).ok()?
        }
        // Macintosh Roman : seuls les octets ASCII sont conservés.
        1 if raw.is_ascii() => String::from_utf8_lossy(raw).into_owned(),
        _ => return None,
    };
    let text = clean(&text);
    (!text.is_empty()).then_some(text)
}

/// Famille et style d'une table `name` ; préfère les identifiants typographiques (16, 17) puis
/// 1 et 2, et la plateforme Windows en anglais américain puis toute langue Windows, Unicode, Mac.
pub(crate) fn parse_name_table(table: &[u8]) -> Option<(String, String)> {
    let count = usize::from(be16(table, 2)?);
    let strings = usize::from(be16(table, 4)?);
    if count > MAX_NAME_RECORDS {
        return None;
    }
    // (rang, texte) par identifiant ; plus petit rang = meilleur.
    let mut best: [Option<(u8, String)>; 4] = [None, None, None, None];
    for index in 0..count {
        let at = 6 + index * 12;
        let platform = be16(table, at)?;
        let language = be16(table, at + 4)?;
        let name_id = be16(table, at + 6)?;
        let length = usize::from(be16(table, at + 8)?);
        let offset = usize::from(be16(table, at + 10)?);
        let slot = match name_id {
            16 => 0,
            1 => 1,
            17 => 2,
            2 => 3,
            _ => continue,
        };
        let rank = match (platform, language) {
            (3, 0x0409) => 0,
            (3, _) => 1,
            (0, _) => 2,
            (1, _) => 3,
            _ => continue,
        };
        if best[slot]
            .as_ref()
            .is_some_and(|(current, _)| *current <= rank)
        {
            continue;
        }
        let start = strings.checked_add(offset)?;
        let Some(raw) = table.get(start..start.checked_add(length)?) else {
            continue;
        };
        if let Some(text) = decode_name(platform, raw) {
            best[slot] = Some((rank, text));
        }
    }
    let [typographic_family, family, typographic_style, style] = best;
    let family = typographic_family.or(family)?.1;
    let style = typographic_style
        .or(style)
        .map_or_else(|| "Regular".to_owned(), |(_, text)| text);
    Some((family, style))
}

fn read_exact_at<R: Read + Seek>(reader: &mut R, at: u64, length: usize) -> Option<Vec<u8>> {
    reader.seek(SeekFrom::Start(at)).ok()?;
    let mut buffer = vec![0u8; length];
    reader.read_exact(&mut buffer).ok()?;
    Some(buffer)
}

/// Familles et styles de chaque face d'un fichier sfnt (TrueType, OpenType, collection). Ne lit que
/// l'en-tête, le répertoire des tables et la table `name` (bornée) ; ignore silencieusement un
/// fichier illisible ou malformé.
pub(crate) fn parse_faces<R: Read + Seek>(reader: &mut R, file_len: u64) -> Vec<(String, String)> {
    let Some(header) = read_exact_at(reader, 0, 12) else {
        return Vec::new();
    };
    let mut offsets: Vec<u64> = vec![0];
    if &header[0..4] == b"ttcf" {
        let count = be32(&header, 8).unwrap_or(0) as usize;
        if count == 0 || count > MAX_FACES_PER_FILE {
            return Vec::new();
        }
        let Some(list) = read_exact_at(reader, 12, count * 4) else {
            return Vec::new();
        };
        offsets = (0..count)
            .filter_map(|index| be32(&list, index * 4).map(u64::from))
            .collect();
    }
    let mut faces = Vec::new();
    for offset in offsets {
        if offset.saturating_add(12) > file_len {
            continue;
        }
        let Some(head) = read_exact_at(reader, offset, 12) else {
            continue;
        };
        let version = be32(&head, 0).unwrap_or(0);
        if !matches!(
            version,
            0x0001_0000 | 0x4f54_544f | 0x7472_7565 | 0x7479_7031
        ) {
            continue;
        }
        let tables = usize::from(be16(&head, 4).unwrap_or(0));
        if tables == 0 || tables > MAX_TABLES {
            continue;
        }
        let Some(directory) = read_exact_at(reader, offset + 12, tables * 16) else {
            continue;
        };
        let record = (0..tables)
            .map(|index| &directory[index * 16..index * 16 + 16])
            .find(|record| &record[0..4] == b"name");
        let Some(record) = record else { continue };
        let table_offset = u64::from(be32(record, 8).unwrap_or(u32::MAX));
        let table_length = be32(record, 12).unwrap_or(u32::MAX) as usize;
        if !(6..=MAX_NAME_TABLE).contains(&table_length)
            || table_offset.saturating_add(table_length as u64) > file_len
        {
            continue;
        }
        let Some(table) = read_exact_at(reader, table_offset, table_length) else {
            continue;
        };
        if let Some(face) = parse_name_table(&table) {
            faces.push(face);
        }
    }
    faces
}

impl FontCatalog {
    /// Balaie les dossiers fournis. `previous` permet de ne pas relire les fichiers inchangés
    /// (même chemin, taille et date de modification).
    pub fn scan(dirs: &[PathBuf], previous: Option<&FontCatalog>) -> Self {
        let mut entries: Vec<FontEntry> = Vec::new();
        'dirs: for dir in dirs {
            let Ok(listing) = fs::read_dir(dir) else {
                continue;
            };
            for item in listing.flatten() {
                let path = item.path();
                // `DirEntry::file_type` ne suit pas les liens : liens symboliques et jonctions sont ignorés.
                let Ok(kind) = item.file_type() else { continue };
                if !kind.is_file() || !is_font_extension(&path) {
                    continue;
                }
                let Ok(meta) = item.metadata() else { continue };
                let (size, modified) = (meta.len(), meta.modified().ok());
                if size == 0 || size > MAX_FONT_BYTES {
                    continue;
                }
                let cached: Vec<&FontEntry> = previous
                    .map(|catalog| {
                        catalog
                            .entries
                            .iter()
                            .filter(|e| e.path == path && e.size == size && e.modified == modified)
                            .collect()
                    })
                    .unwrap_or_default();
                if !cached.is_empty() {
                    entries.extend(cached.into_iter().cloned());
                } else if let Ok(mut file) = crate::files::open_read_without_reparse(&path) {
                    for (face, (family, style)) in
                        parse_faces(&mut file, size).into_iter().enumerate()
                    {
                        entries.push(FontEntry {
                            family,
                            style,
                            face: face as u32,
                            size,
                            modified,
                            path: path.clone(),
                        });
                    }
                }
                if entries.len() >= MAX_FONTS {
                    break 'dirs;
                }
            }
        }
        entries.sort_by(|a, b| {
            (a.family.to_lowercase(), &a.style, &a.path, a.face).cmp(&(
                b.family.to_lowercase(),
                &b.style,
                &b.path,
                b.face,
            ))
        });
        entries.truncate(MAX_FONTS);
        Self {
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            entries,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn id(&self, index: usize) -> String {
        format!("f{}-{index}", self.generation)
    }

    /// Une ligne par face : `id<TAB>famille<TAB>style<TAB>face<TAB>octets`.
    pub fn list_tsv(&self) -> String {
        let mut out = String::new();
        for (index, entry) in self.entries.iter().enumerate() {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                self.id(index),
                entry.family,
                entry.style,
                entry.face,
                entry.size
            ));
        }
        out
    }

    fn lookup(&self, id: &str) -> Result<&FontEntry, &'static str> {
        let body = id.strip_prefix('f').ok_or("fonts.error.unknown_id")?;
        let (generation, index) = body.split_once('-').ok_or("fonts.error.unknown_id")?;
        let number = |text: &str| {
            (!text.is_empty() && text.len() <= 20 && text.bytes().all(|b| b.is_ascii_digit()))
                .then(|| text.parse::<u64>().ok())
                .flatten()
        };
        if number(generation) != Some(self.generation) {
            return Err("fonts.error.unknown_id");
        }
        let index = usize::try_from(number(index).ok_or("fonts.error.unknown_id")?)
            .map_err(|_| "fonts.error.unknown_id")?;
        self.entries.get(index).ok_or("fonts.error.unknown_id")
    }

    /// Octets du fichier de la police désignée, bornés par `MAX_FONT_BYTES` et refusés si le
    /// fichier a changé depuis le balayage.
    pub fn read(&self, id: &str) -> Result<Vec<u8>, &'static str> {
        let entry = self.lookup(id)?;
        // Les points d'analyse (liens, jonctions) ne sont pas suivis, comme dans le courtier.
        let file = crate::files::open_read_without_reparse(&entry.path)
            .map_err(|_| "fonts.error.read_failed")?;
        let length = file
            .metadata()
            .map_err(|_| "fonts.error.read_failed")?
            .len();
        if length > MAX_FONT_BYTES {
            return Err("fonts.error.too_large");
        }
        if length != entry.size {
            return Err("fonts.error.changed");
        }
        let mut bytes = Vec::with_capacity(length as usize);
        file.take(MAX_FONT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "fonts.error.read_failed")?;
        if bytes.len() as u64 != length {
            return Err("fonts.error.changed");
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_be_bytes).collect()
    }

    /// Table `name` : (plateforme, langue, identifiant, octets).
    fn name_table(records: &[(u16, u16, u16, Vec<u8>)]) -> Vec<u8> {
        let mut table = vec![0, 0];
        table.extend((records.len() as u16).to_be_bytes());
        table.extend((6 + records.len() as u16 * 12).to_be_bytes());
        let mut strings: Vec<u8> = Vec::new();
        for (platform, language, id, raw) in records {
            table.extend(platform.to_be_bytes());
            table.extend(if *platform == 3 { 1u16 } else { 0 }.to_be_bytes());
            table.extend(language.to_be_bytes());
            table.extend(id.to_be_bytes());
            table.extend((raw.len() as u16).to_be_bytes());
            table.extend((strings.len() as u16).to_be_bytes());
            strings.extend(raw);
        }
        table.extend(strings);
        table
    }

    fn sfnt(version: u32, table: &[u8]) -> Vec<u8> {
        let mut font = version.to_be_bytes().to_vec();
        font.extend(1u16.to_be_bytes());
        font.extend([0u8; 6]);
        font.extend(b"name");
        font.extend(0u32.to_be_bytes());
        font.extend(28u32.to_be_bytes());
        font.extend((table.len() as u32).to_be_bytes());
        font.extend(table);
        font
    }

    fn faces(bytes: Vec<u8>) -> Vec<(String, String)> {
        let length = bytes.len() as u64;
        parse_faces(&mut Cursor::new(bytes), length)
    }

    #[test]
    fn parses_family_and_style_preferring_typographic_names() {
        let table = name_table(&[
            (3, 0x0409, 1, utf16("Cambria Bold")),
            (3, 0x0409, 2, utf16("Regular")),
            (3, 0x0409, 16, utf16("Cambria")),
            (3, 0x0409, 17, utf16("Bold")),
            (3, 0x040c, 16, utf16("Autre")),
        ]);
        assert_eq!(
            faces(sfnt(0x0001_0000, &table)),
            vec![("Cambria".to_owned(), "Bold".to_owned())]
        );
        let opentype = name_table(&[
            (1, 0, 1, b"Mac Name".to_vec()),
            (3, 0x040c, 1, utf16("Élégante")),
        ]);
        assert_eq!(
            faces(sfnt(0x4f54_544f, &opentype)),
            vec![("Élégante".to_owned(), "Regular".to_owned())]
        );
    }

    #[test]
    fn control_characters_and_length_are_sanitised() {
        let long = "x".repeat(300);
        let table = name_table(&[
            (3, 0x0409, 1, utf16("A\tB\nC")),
            (3, 0x0409, 2, utf16(&long)),
        ]);
        let (family, style) = parse_name_table(&table).unwrap();
        assert_eq!(family, "A B C");
        assert_eq!(style.chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn malformed_inputs_never_panic() {
        assert!(faces(Vec::new()).is_empty());
        assert!(faces(vec![0xff; 11]).is_empty());
        assert!(faces(b"ttcf\0\x01\0\0\xff\xff\xff\xff".to_vec()).is_empty());
        assert!(faces(b"\0\x01\0\0\0\x01\0\0\0\0\0\0".to_vec()).is_empty());
        // Table `name` hors du fichier, ou trop grande.
        let mut font = sfnt(0x0001_0000, &name_table(&[(3, 0x0409, 1, utf16("A"))]));
        font[24..28].copy_from_slice(&0x00ff_ffffu32.to_be_bytes());
        assert!(faces(font.clone()).is_empty());
        font[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(faces(font).is_empty());
        // Enregistrements tronqués, déclarés trop nombreux, chaîne hors table.
        assert_eq!(parse_name_table(&[0, 0, 0xff, 0xff, 0, 6]), None);
        assert_eq!(parse_name_table(&[0, 0, 0, 1, 0, 18, 0, 3]), None);
        let mut broken = name_table(&[(3, 0x0409, 1, utf16("A"))]);
        broken[16..18].copy_from_slice(&0x7fffu16.to_be_bytes());
        assert_eq!(parse_name_table(&broken), None);
        assert_eq!(parse_name_table(&[]), None);
        // Longueur de chaîne impaire en UTF-16.
        let odd = name_table(&[(3, 0x0409, 1, vec![0, 65, 0])]);
        assert_eq!(parse_name_table(&odd), None);
    }

    #[test]
    fn collection_exposes_every_face() {
        let first = name_table(&[(3, 0x0409, 1, utf16("Alpha"))]);
        let second = name_table(&[(3, 0x0409, 1, utf16("Beta"))]);
        let mut file = b"ttcf".to_vec();
        file.extend(0x0001_0000u32.to_be_bytes());
        file.extend(2u32.to_be_bytes());
        let base = 12 + 8;
        let one = sfnt(0x0001_0000, &first);
        file.extend((base as u32).to_be_bytes());
        file.extend(((base + one.len()) as u32).to_be_bytes());
        // Les décalages internes des tables sont relatifs au fichier : on les recalcule.
        let mut a = one.clone();
        a[20..24].copy_from_slice(&((base + 28) as u32).to_be_bytes());
        let mut b = sfnt(0x0001_0000, &second);
        b[20..24].copy_from_slice(&((base + one.len() + 28) as u32).to_be_bytes());
        file.extend(a);
        file.extend(b);
        assert_eq!(
            faces(file),
            vec![
                ("Alpha".to_owned(), "Regular".to_owned()),
                ("Beta".to_owned(), "Regular".to_owned())
            ]
        );
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atelier-fonts-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_font(dir: &Path, name: &str, family: &str) -> PathBuf {
        let table = name_table(&[(3, 0x0409, 1, utf16(family))]);
        let mut font = sfnt(0x0001_0000, &table);
        font[20..24].copy_from_slice(&28u32.to_be_bytes());
        let path = dir.join(name);
        fs::write(&path, font).unwrap();
        path
    }

    #[test]
    fn catalog_lists_opaque_ids_and_reads_bounded_bytes() {
        let dir = temp_dir("catalog");
        write_font(&dir, "b.ttf", "Zeta");
        write_font(&dir, "a.OTF", "Alpha");
        fs::write(dir.join("notes.txt"), b"not a font").unwrap();
        fs::write(dir.join("broken.ttf"), b"garbage garbage garbage").unwrap();
        let catalog = FontCatalog::scan(std::slice::from_ref(&dir), None);
        assert_eq!(catalog.len(), 2);
        let tsv = catalog.list_tsv();
        let lines: Vec<&str> = tsv.lines().collect();
        assert!(lines[0].contains("\tAlpha\tRegular\t0\t"), "{tsv}");
        assert!(lines[1].contains("\tZeta\tRegular\t0\t"), "{tsv}");
        assert!(!tsv.contains(dir.to_string_lossy().as_ref()));
        let id = catalog.id(0);
        let bytes = catalog.read(&id).unwrap();
        assert_eq!(&bytes[..4], &[0, 1, 0, 0]);
        // Identifiants invalides, d'une autre génération ou fournis comme chemin.
        for bad in [
            "",
            "f",
            "f1",
            "f1-",
            "x1-0",
            "../a.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
            "f99999999999999999999999-0",
            "f-1-0",
        ] {
            assert_eq!(catalog.read(bad), Err("fonts.error.unknown_id"), "{bad}");
        }
        let other = FontCatalog::scan(std::slice::from_ref(&dir), Some(&catalog));
        assert_eq!(other.len(), 2);
        assert_eq!(catalog.read(&other.id(0)), Err("fonts.error.unknown_id"));
        assert_eq!(
            other.read(&format!("f{}-9", other.generation)),
            Err("fonts.error.unknown_id")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn changed_or_missing_files_are_refused() {
        let dir = temp_dir("changed");
        let path = write_font(&dir, "a.ttf", "Alpha");
        let catalog = FontCatalog::scan(std::slice::from_ref(&dir), None);
        let id = catalog.id(0);
        let mut grown = fs::read(&path).unwrap();
        grown.extend([0u8; 16]);
        fs::write(&path, grown).unwrap();
        assert_eq!(catalog.read(&id), Err("fonts.error.changed"));
        fs::remove_file(&path).unwrap();
        assert_eq!(catalog.read(&id), Err("fonts.error.read_failed"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_directories_give_an_empty_catalog() {
        let catalog = FontCatalog::scan(&[PathBuf::from("Z:/does/not/exist")], None);
        assert!(catalog.is_empty());
        assert_eq!(catalog.list_tsv(), "");
    }

    #[cfg(windows)]
    #[test]
    fn real_system_fonts_are_listed_without_paths() {
        let catalog = FontCatalog::scan(&system_font_dirs(), None);
        assert!(
            !catalog.is_empty(),
            "a Windows machine has at least one font"
        );
        let tsv = catalog.list_tsv();
        assert!(!tsv.to_ascii_lowercase().contains("c:\\"));
        assert!(
            tsv.lines()
                .all(|line| line.split('\t').count() == 5 && line.starts_with('f'))
        );
    }
}
