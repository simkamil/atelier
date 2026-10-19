// SPDX-License-Identifier: AGPL-3.0-or-later
//! Documents récents (US-320) : liste ordonnée, épinglage, retrait, persistance dans `recent.ini`.
//!
//! Le script ne voit jamais un chemin pour agir : il désigne une entrée par un numéro propre à la
//! session (`id`) ; seul le natif relie ce numéro à un chemin. Le fichier est écrit de façon
//! atomique (fichier temporaire du même dossier puis renommage) et relu sans panique : une ligne
//! invalide est ignorée, un fichier absent donne une liste vide.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Nombre maximal d'entrées conservées (épinglées comprises).
pub const MAX_ENTRIES: usize = 30;
/// Longueur maximale d'un chemin retenu, en caractères ; au-delà l'entrée n'est pas mémorisée.
pub const MAX_PATH_CHARS: usize = 1024;
/// Taille maximale lue dans `recent.ini`.
const MAX_FILE_BYTES: u64 = 256 * 1024;
const HEADER: &str = "atelier-recent=1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentEntry {
    /// Numéro de session, jamais persisté.
    pub id: u64,
    pub path: PathBuf,
    pub pinned: bool,
    /// Dernière utilisation, secondes depuis l'époque Unix.
    pub used: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecentList {
    entries: Vec<RecentEntry>,
    next_id: u64,
}

/// Secondes écoulées depuis l'époque Unix (0 si l'horloge est antérieure).
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Clé de comparaison : les chemins Windows ne distinguent pas les majuscules.
fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

/// Un chemin est retenu s'il est absolu, en Unicode valide, sans caractère de contrôle et borné.
fn acceptable(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    path.is_absolute()
        && !text.is_empty()
        && text.chars().count() <= MAX_PATH_CHARS
        && !text.chars().any(char::is_control)
}

impl RecentList {
    /// Relit le texte de `recent.ini` : en-tête puis une ligne `<épinglé 0|1>\t<secondes>\t<chemin>`.
    pub fn parse(text: &str) -> Self {
        let mut list = Self::default();
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some(HEADER) {
            return list;
        }
        let mut parsed: Vec<(PathBuf, bool, u64)> = Vec::new();
        for line in lines {
            let mut fields = line.splitn(3, '\t');
            let (Some(pinned), Some(used), Some(path)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let pinned = match pinned {
                "0" => false,
                "1" => true,
                _ => continue,
            };
            let Ok(used) = used.parse::<u64>() else {
                continue;
            };
            let path = PathBuf::from(path);
            if acceptable(&path) && !parsed.iter().any(|(known, _, _)| key(known) == key(&path)) {
                parsed.push((path, pinned, used));
            }
        }
        // Le fichier est écrit dans l'ordre d'affichage : les premières lignes reçoivent les numéros les
        // plus grands, ce qui garde l'ordre des entrées de même date d'un lancement à l'autre.
        for (path, pinned, used) in parsed.into_iter().rev() {
            list.push(path, pinned, used);
        }
        list.trim();
        list
    }

    pub fn to_text(&self) -> String {
        let mut text = format!("{HEADER}\n");
        for entry in self.ordered() {
            text.push_str(&format!(
                "{}\t{}\t{}\n",
                u8::from(entry.pinned),
                entry.used,
                entry.path.display()
            ));
        }
        text
    }

    fn push(&mut self, path: PathBuf, pinned: bool, used: u64) {
        self.next_id += 1;
        self.entries.push(RecentEntry {
            id: self.next_id,
            path,
            pinned,
            used,
        });
    }

    /// Entrées dans l'ordre d'affichage : épinglées d'abord, puis par utilisation décroissante.
    pub fn ordered(&self) -> Vec<RecentEntry> {
        let mut sorted = self.entries.clone();
        sorted.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.used.cmp(&a.used))
                .then(b.id.cmp(&a.id))
        });
        sorted
    }

    /// Ramène la liste à `MAX_ENTRIES` : on retire d'abord les plus anciennes non épinglées.
    fn trim(&mut self) {
        while self.entries.len() > MAX_ENTRIES {
            let order = self.ordered();
            let victim = order
                .iter()
                .rev()
                .find(|entry| !entry.pinned)
                .or_else(|| order.last())
                .map(|entry| entry.id);
            match victim {
                Some(id) => self.entries.retain(|entry| entry.id != id),
                None => break,
            }
        }
    }

    /// Note l'utilisation de `path` : crée l'entrée ou la remonte (l'épinglage est conservé).
    /// Renvoie `false` si le chemin n'est pas retenu.
    pub fn touch(&mut self, path: &Path, now: u64) -> bool {
        if !acceptable(path) {
            return false;
        }
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| key(&entry.path) == key(path))
        {
            entry.used = now;
            entry.path = path.to_path_buf();
        } else {
            self.push(path.to_path_buf(), false, now);
            self.trim();
        }
        true
    }

    pub fn path_of(&self, id: u64) -> Option<PathBuf> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.path.clone())
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> bool {
        self.entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .map(|entry| entry.pinned = pinned)
            .is_some()
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        self.entries.len() != before
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Lit `file` ; absent, trop gros ou illisible : liste vide.
    pub fn load(file: &Path) -> Self {
        match fs::metadata(file) {
            Ok(meta) if meta.is_file() && meta.len() <= MAX_FILE_BYTES => {}
            _ => return Self::default(),
        }
        fs::read(file)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map_or_else(Self::default, |text| Self::parse(&text))
    }

    /// Écrit `file` de façon atomique : temporaire du même dossier, vidage sur disque, renommage.
    pub fn save(&self, file: &Path) -> io::Result<()> {
        let temporary = file.with_extension("ini.tmp");
        let result = (|| {
            let mut out = fs::File::create(&temporary)?;
            out.write_all(self.to_text().as_bytes())?;
            out.sync_all()?;
            drop(out);
            fs::rename(&temporary, file)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute(name: &str) -> PathBuf {
        std::env::temp_dir().join("atelier-recent-tests").join(name)
    }

    #[test]
    fn touch_orders_by_recency_and_deduplicates_case_insensitively() {
        let mut list = RecentList::default();
        assert!(list.touch(&absolute("a.docx"), 10));
        assert!(list.touch(&absolute("b.docx"), 20));
        assert!(list.touch(&absolute("A.DOCX"), 30));
        let order = list.ordered();
        assert_eq!(order.len(), 2);
        assert_eq!(order[0].path, absolute("A.DOCX"));
        assert_eq!(order[1].path, absolute("b.docx"));
    }

    #[test]
    fn pinned_entries_stay_first_and_survive_trimming() {
        let mut list = RecentList::default();
        list.touch(&absolute("old.docx"), 1);
        let old = list.ordered()[0].id;
        assert!(list.set_pinned(old, true));
        for index in 0..(MAX_ENTRIES + 5) {
            list.touch(&absolute(&format!("f{index}.docx")), 100 + index as u64);
        }
        assert_eq!(list.len(), MAX_ENTRIES);
        let order = list.ordered();
        assert_eq!(order[0].id, old);
        assert!(order[0].pinned);
        assert!(list.set_pinned(old, false));
        assert!(!list.set_pinned(9_999, true));
    }

    #[test]
    fn remove_drops_one_entry_and_unknown_ids_are_refused() {
        let mut list = RecentList::default();
        list.touch(&absolute("a.txt"), 1);
        let id = list.ordered()[0].id;
        assert_eq!(list.path_of(id), Some(absolute("a.txt")));
        assert!(list.remove(id));
        assert!(!list.remove(id));
        assert!(list.is_empty());
        assert_eq!(list.path_of(id), None);
    }

    #[test]
    fn unsuitable_paths_are_not_remembered() {
        let mut list = RecentList::default();
        assert!(!list.touch(Path::new("relatif.docx"), 1));
        assert!(!list.touch(&absolute(&"x".repeat(MAX_PATH_CHARS + 1)), 1));
        assert!(!list.touch(&absolute("a\u{1}b.docx"), 1));
        assert!(list.is_empty());
    }

    #[test]
    fn entries_of_the_same_second_keep_their_order_across_a_restart() {
        let mut list = RecentList::default();
        for name in ["a.txt", "b.txt", "c.txt"] {
            list.touch(&absolute(name), 7);
        }
        let before: Vec<PathBuf> = list.ordered().into_iter().map(|e| e.path).collect();
        let mut again = RecentList::parse(&list.to_text());
        for _ in 0..3 {
            let after: Vec<PathBuf> = again.ordered().into_iter().map(|e| e.path).collect();
            assert_eq!(before, after);
            again = RecentList::parse(&again.to_text());
        }
    }

    #[test]
    fn text_round_trip_keeps_order_pins_and_dates() {
        let mut list = RecentList::default();
        list.touch(&absolute("un.docx"), 5);
        list.touch(&absolute("deux espaces .txt"), 9);
        let id = list.ordered()[1].id;
        list.set_pinned(id, true);
        let again = RecentList::parse(&list.to_text());
        let (first, second) = (list.ordered(), again.ordered());
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(&second) {
            assert_eq!((&a.path, a.pinned, a.used), (&b.path, b.pinned, b.used));
        }
    }

    #[test]
    fn parse_ignores_bad_lines_missing_header_and_duplicates() {
        let dir = absolute("");
        let good = dir.join("ok.docx");
        let text = format!(
            "atelier-recent=1\n2\t5\t{p}\nx\t5\t{p}\n1\tabc\t{p}\n1\t7\t{p}\n1\t8\t{p}\n1\t9\trelatif.docx\n",
            p = good.display()
        );
        let list = RecentList::parse(&text);
        assert_eq!(list.len(), 1);
        assert_eq!(list.ordered()[0].used, 7);
        assert!(RecentList::parse(&format!("1\t7\t{}\n", good.display())).is_empty());
        assert!(RecentList::parse("").is_empty());
    }

    #[test]
    fn save_then_load_persists_and_a_missing_file_is_an_empty_list() {
        let dir = absolute("persist");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("recent.ini");
        assert!(RecentList::load(&file).is_empty());
        let mut list = RecentList::default();
        list.touch(&dir.join("doc.docx"), 42);
        let id = list.ordered()[0].id;
        list.set_pinned(id, true);
        list.save(&file).unwrap();
        assert!(!dir.join("recent.ini.tmp").exists());
        let loaded = RecentList::load(&file);
        assert_eq!(loaded.len(), 1);
        let entry = &loaded.ordered()[0];
        assert_eq!((entry.used, entry.pinned), (42, true));
        let _ = fs::remove_dir_all(&dir);
    }
}
