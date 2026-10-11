// SPDX-License-Identifier: AGPL-3.0-or-later
//! Resolve the one storage root used by the Windows shell before startup.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "data_win32.rs"]
mod win32;

#[cfg(windows)]
pub use win32::{choose_data_mode, show_move_instructions, show_startup_error};

static PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const PROBE_ATTEMPTS: u8 = 16;

/// Why storage could not be resolved or prepared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
    ExecutableNotAbsolute {
        path: PathBuf,
    },
    ExecutableParentUnavailable {
        path: PathBuf,
    },
    LocalAppDataUnavailable,
    LocalAppDataNotAbsolute {
        path: PathBuf,
    },
    UnsafeStoragePath {
        path: PathBuf,
    },
    StorageUnavailable {
        path: PathBuf,
        operation: &'static str,
        detail: String,
    },
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExecutableNotAbsolute { path } => write!(
                f,
                "Le chemin de l’exécutable doit être absolu : {}",
                path.display()
            ),
            Self::ExecutableParentUnavailable { path } => write!(
                f,
                "Impossible de trouver le dossier de l’exécutable : {}",
                path.display()
            ),
            Self::LocalAppDataUnavailable => write!(
                f,
                "Le dossier LOCALAPPDATA est indisponible. Le démarrage est interrompu."
            ),
            Self::LocalAppDataNotAbsolute { path } => write!(
                f,
                "Le chemin LOCALAPPDATA doit être absolu : {}",
                path.display()
            ),
            Self::UnsafeStoragePath { path } => write!(
                f,
                "Le chemin de données n’est pas un dossier sûr : {}",
                path.display()
            ),
            Self::StorageUnavailable {
                path,
                operation,
                detail,
            } => write!(
                f,
                "Impossible de {operation} dans {} : {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for DataError {}

/// The user's explicit decision after portable storage fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupChoice {
    Local,
    Move,
    Quit,
}

/// Whether all application-managed data stays beside the executable or uses LOCALAPPDATA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataMode {
    Portable,
    Local,
}

/// Resolved locations, all rooted in one directory. Accessors do not create anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    pub root: PathBuf,
    pub mode: DataMode,
}

impl DataPaths {
    pub fn preferences_file(&self) -> PathBuf {
        self.root.join("config.ini")
    }

    pub fn window_state_file(&self) -> PathBuf {
        self.root.join("window-state.ini")
    }

    pub fn recovery_dir(&self) -> PathBuf {
        self.root.join("recovery")
    }

    pub fn recent_file(&self) -> PathBuf {
        self.root.join("recent.ini")
    }

    pub fn crash_dir(&self) -> PathBuf {
        self.root.join("crashes")
    }

    pub fn webview_dir(&self) -> PathBuf {
        self.root.join("webview")
    }

    /// Short French label suitable for a visible window title.
    pub fn mode_label(&self) -> &'static str {
        match self.mode {
            DataMode::Portable => "Portable",
            DataMode::Local => "Mode local",
        }
    }
}

/// Final startup decision after resolving and probing the selected data root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupOutcome {
    Ready(DataPaths),
    MoveRequired,
    Quit,
}

/// Resolve portable storage first; consult local storage only after an explicit Local choice.
pub fn resolve_data_paths(
    executable: &Path,
    local_appdata: Option<&Path>,
    choose: impl FnOnce(&DataError) -> StartupChoice,
) -> Result<StartupOutcome, DataError> {
    let portable_result = portable_root(executable).and_then(|root| {
        prepare_root(&root)?;
        Ok(DataPaths {
            root,
            mode: DataMode::Portable,
        })
    });

    match portable_result {
        Ok(paths) => Ok(StartupOutcome::Ready(paths)),
        Err(portable_error) => match choose(&portable_error) {
            StartupChoice::Move => Ok(StartupOutcome::MoveRequired),
            StartupChoice::Quit => Ok(StartupOutcome::Quit),
            StartupChoice::Local => {
                let base = local_appdata.ok_or(DataError::LocalAppDataUnavailable)?;
                if !base.is_absolute() {
                    return Err(DataError::LocalAppDataNotAbsolute {
                        path: base.to_path_buf(),
                    });
                }
                let root = base.join("Atelier");
                prepare_root(&root)?;
                Ok(StartupOutcome::Ready(DataPaths {
                    root,
                    mode: DataMode::Local,
                }))
            }
        },
    }
}

fn portable_root(executable: &Path) -> Result<PathBuf, DataError> {
    if !executable.is_absolute() {
        return Err(DataError::ExecutableNotAbsolute {
            path: executable.to_path_buf(),
        });
    }
    let parent = executable
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| DataError::ExecutableParentUnavailable {
            path: executable.to_path_buf(),
        })?;
    Ok(parent.join("atelier-data"))
}

fn prepare_root(root: &Path) -> Result<(), DataError> {
    fs::create_dir_all(root)
        .map_err(|error| io_error(root, "créer le dossier de données", error))?;
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| io_error(root, "inspecter le dossier de données", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DataError::UnsafeStoragePath {
            path: root.to_path_buf(),
        });
    }
    probe_writable(root)
}

fn probe_writable(root: &Path) -> Result<(), DataError> {
    probe_writable_with(root, next_probe_path)
}

fn probe_writable_with(
    root: &Path,
    mut next_probe: impl FnMut(&Path) -> PathBuf,
) -> Result<(), DataError> {
    for _ in 0..PROBE_ATTEMPTS {
        let probe = next_probe(root);
        let mut file = match open_probe_exclusive(&probe) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(root, "vérifier l’écriture", error)),
        };

        let write_result = file
            .write_all(b"atelier-storage-probe\n")
            .and_then(|()| file.sync_all());
        drop(file);

        let cleanup_result = fs::remove_file(&probe);
        if let Err(write_error) = write_result {
            return match cleanup_result {
                Ok(()) => Err(io_error(
                    root,
                    "écrire ou synchroniser la sonde",
                    write_error,
                )),
                Err(cleanup_error) => Err(io_error(
                    root,
                    "nettoyer la sonde d’écriture après un échec",
                    cleanup_error,
                )),
            };
        }
        cleanup_result.map_err(|error| io_error(root, "supprimer la sonde d’écriture", error))?;
        return Ok(());
    }

    Err(DataError::StorageUnavailable {
        path: root.to_path_buf(),
        operation: "trouver un nom sûr pour la sonde d’écriture",
        detail: "trop de collisions successives".to_owned(),
    })
}

fn next_probe_path(root: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = PROBE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    root.join(format!(
        ".atelier-write-probe-{}-{timestamp:x}-{sequence:x}",
        std::process::id()
    ))
}

fn open_probe_exclusive(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn io_error(path: &Path, operation: &'static str, error: io::Error) -> DataError {
    DataError::StorageUnavailable {
        path: path.to_path_buf(),
        operation,
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let temp_root = std::env::temp_dir();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos();
            let path = temp_root.join(format!(
                "atelier-data-test-{}-{timestamp:x}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            assert!(path.is_absolute(), "temporary test path must be absolute");
            assert!(path.starts_with(&temp_root), "test path must stay in temp");
            fs::create_dir(&path).expect("create unique test directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let temp_root = std::env::temp_dir();
            if self.0.is_absolute()
                && self.0.starts_with(&temp_root)
                && self
                    .0
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("atelier-data-test-"))
            {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    fn executable_in(parent: &Path) -> PathBuf {
        parent.join("atelier.exe")
    }

    #[test]
    fn portable_paths_share_root_and_accessors_create_no_subdirectories() {
        let temp = TempDir::new();
        let executable = executable_in(&temp.0);
        let outcome = resolve_data_paths(&executable, None, |_| panic!("choice not expected"))
            .expect("portable root is writable");
        let StartupOutcome::Ready(paths) = outcome else {
            panic!("expected ready portable outcome");
        };

        assert_eq!(paths.root, temp.0.join("atelier-data"));
        assert_eq!(paths.mode, DataMode::Portable);
        assert_eq!(paths.mode_label(), "Portable");
        for child in [
            paths.preferences_file(),
            paths.window_state_file(),
            paths.recovery_dir(),
            paths.recent_file(),
            paths.crash_dir(),
            paths.webview_dir(),
        ] {
            assert!(child.starts_with(&paths.root));
            assert!(!child.exists());
        }
        assert_eq!(
            fs::read_dir(&paths.root).expect("read data root").count(),
            0,
            "write probe must be removed before returning"
        );
    }

    #[test]
    fn local_choice_is_explicit_and_uses_one_local_root() {
        let temp = TempDir::new();
        let executable = executable_in(&temp.0);
        let blocked_portable = temp.0.join("atelier-data");
        fs::write(&blocked_portable, b"keep me").expect("make portable location unavailable");
        let local_appdata = temp.0.join("local");
        let calls = AtomicUsize::new(0);

        let outcome = resolve_data_paths(&executable, Some(&local_appdata), |error| {
            calls.fetch_add(1, Ordering::Relaxed);
            assert!(error.to_string().contains("atelier-data"));
            StartupChoice::Local
        })
        .expect("local path is writable");

        let StartupOutcome::Ready(paths) = outcome else {
            panic!("expected ready local outcome");
        };
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(paths.root, local_appdata.join("Atelier"));
        assert_eq!(paths.mode, DataMode::Local);
        assert_eq!(paths.mode_label(), "Mode local");
        assert_eq!(
            fs::read(blocked_portable).expect("preserved blocker"),
            b"keep me"
        );
    }

    #[test]
    fn move_and_quit_never_create_or_inspect_local_path() {
        for choice in [StartupChoice::Move, StartupChoice::Quit] {
            let temp = TempDir::new();
            let executable = executable_in(&temp.0);
            fs::write(temp.0.join("atelier-data"), b"blocked")
                .expect("make portable location unavailable");
            let local_appdata = temp.0.join("local");
            let calls = AtomicUsize::new(0);

            let outcome = resolve_data_paths(&executable, Some(&local_appdata), |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                choice
            })
            .expect("move or quit is not an error");

            assert_eq!(calls.load(Ordering::Relaxed), 1);
            assert!(!local_appdata.exists());
            assert_eq!(
                outcome,
                match choice {
                    StartupChoice::Move => StartupOutcome::MoveRequired,
                    StartupChoice::Quit => StartupOutcome::Quit,
                    StartupChoice::Local => unreachable!(),
                }
            );
        }
    }

    #[test]
    fn inaccessible_local_storage_is_final_and_does_not_retry_the_choice() {
        let temp = TempDir::new();
        let executable = executable_in(&temp.0);
        fs::write(temp.0.join("atelier-data"), b"blocked")
            .expect("make portable location unavailable");
        let local_appdata = temp.0.join("local");
        fs::write(&local_appdata, b"also blocked").expect("block local appdata");
        let calls = AtomicUsize::new(0);

        let error = resolve_data_paths(&executable, Some(&local_appdata), |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            StartupChoice::Local
        })
        .expect_err("local location is not a directory");

        assert!(error.to_string().contains("Atelier"));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn relative_local_appdata_fails_without_creating_it() {
        let temp = TempDir::new();
        let executable = executable_in(&temp.0);
        fs::write(temp.0.join("atelier-data"), b"blocked")
            .expect("make portable location unavailable");

        let error = resolve_data_paths(&executable, Some(Path::new("relative-local")), |_| {
            StartupChoice::Local
        })
        .expect_err("relative LOCALAPPDATA is rejected");

        assert!(matches!(error, DataError::LocalAppDataNotAbsolute { .. }));
        assert!(!temp.0.join("relative-local").exists());
    }

    #[test]
    fn local_choice_without_local_appdata_fails_without_fallback() {
        let temp = TempDir::new();
        let executable = executable_in(&temp.0);
        fs::write(temp.0.join("atelier-data"), b"blocked")
            .expect("make portable location unavailable");
        let calls = AtomicUsize::new(0);

        let error = resolve_data_paths(&executable, None, |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            StartupChoice::Local
        })
        .expect_err("missing LOCALAPPDATA is final");

        assert_eq!(error, DataError::LocalAppDataUnavailable);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn relative_executable_is_reported_to_the_single_choice_callback() {
        let calls = AtomicUsize::new(0);
        let outcome = resolve_data_paths(Path::new("relative\\atelier.exe"), None, |error| {
            calls.fetch_add(1, Ordering::Relaxed);
            assert!(matches!(error, DataError::ExecutableNotAbsolute { .. }));
            StartupChoice::Quit
        })
        .expect("quit after an unusable executable path");

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(outcome, StartupOutcome::Quit);
    }

    #[test]
    fn executable_without_a_parent_is_reported_to_the_choice_callback() {
        #[cfg(windows)]
        let root_executable = Path::new(r"C:\");
        #[cfg(not(windows))]
        let root_executable = Path::new("/");

        assert!(root_executable.is_absolute());
        let outcome = resolve_data_paths(root_executable, None, |error| {
            assert!(matches!(
                error,
                DataError::ExecutableParentUnavailable { .. }
            ));
            StartupChoice::Quit
        })
        .expect("quit after parentless executable path");
        assert_eq!(outcome, StartupOutcome::Quit);
    }

    #[test]
    fn probe_retries_collision_then_cleans_its_file_without_touching_existing_file() {
        let temp = TempDir::new();
        let root = temp.0.join("data");
        fs::create_dir_all(&root).expect("create root");
        let existing = root.join("probe-collision");
        fs::write(&existing, b"keep me").expect("create collision file");
        let successful_probe = root.join("probe-success");
        let mut candidates = [existing.clone(), successful_probe.clone()].into_iter();

        probe_writable_with(&root, |_| {
            candidates.next().expect("probe attempts are bounded")
        })
        .expect("second probe name is available");

        assert_eq!(
            fs::read(existing).expect("collision file remains"),
            b"keep me"
        );
        assert!(!successful_probe.exists(), "successful probe is cleaned up");
    }

    #[test]
    fn probe_stops_after_sixteen_collisions_and_preserves_the_existing_file() {
        let temp = TempDir::new();
        let root = temp.0.join("data");
        fs::create_dir_all(&root).expect("create root");
        let existing = root.join("probe-collision");
        fs::write(&existing, b"keep me").expect("create collision file");
        let mut generated = 0;

        let error = probe_writable_with(&root, |_| {
            generated += 1;
            existing.clone()
        })
        .expect_err("all bounded probe names collide");

        assert_eq!(generated, PROBE_ATTEMPTS);
        assert!(error.to_string().contains("collisions"));
        assert_eq!(
            fs::read(existing).expect("collision file remains"),
            b"keep me"
        );
    }
}
