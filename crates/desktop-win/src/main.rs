// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native desktop entry point. The data root is resolved before creating caches.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if let Err(error) = start() {
        if matches!(
            std::env::args_os().nth(1).as_deref(),
            Some(arg) if arg == "--self-check" || arg == "--webview-self-check" || arg == "--resources-self-check"
        ) {
            eprintln!("{error}");
        } else {
            atelier_desktop_win::window::show_error(&error.to_string());
        }
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod win32_helpers {
    use std::ffi::c_void;
    pub type Hwnd = *mut c_void;
    pub type LPARAM = isize;
    pub type WPARAM = usize;
    
    #[repr(C)]
    pub struct COPYDATASTRUCT {
        pub dwId: usize,
        pub cbData: usize,
        pub lpData: *mut c_void,
    }
    
    pub type DWORD = u32;
    pub type LPVOID = *mut c_void;
    pub const MUTEX_ALL_ACCESS: u32 = 0x1F0001;
    pub const FALSE: i32 = 0;
    pub const WM_COPYDATA: u32 = 0x004A;
    pub const WM_USER_OPEN_PATHS: u32 = 0x0400;
    
    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn FindWindowW(class_name: *const u16, window_name: *const u16) -> Hwnd;
        pub fn PostMessageW(hwnd: Hwnd, message: u32, w: WPARAM, l: LPARAM) -> isize;
        pub fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    }
    
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn OpenMutexW(desired_access: u32, inherit_handle: i32, name: *const u16) -> Hwnd;
        pub fn CreateMutexW(named_handle: *const u16, inherit_handle: i32, desired_access: u32) -> Hwnd;
    }
}

#[cfg(windows)]
fn start() -> Result<(), Box<dyn std::error::Error>> {
    use atelier_desktop_win::{data, window};
    use std::path::PathBuf;

    window::initialize_dpi()?;
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let mut args_iter = arguments.iter();
    let first = args_iter.next();
    if first == Some(&std::ffi::OsString::from("--self-check")) {
        let state_file = args_iter
            .next()
            .map(PathBuf::from)
            .ok_or("--self-check requires an isolated state file")?;
        println!("{}", window::self_check(&state_file)?);
        return Ok(());
    }
    if first == Some(&std::ffi::OsString::from("--webview-self-check")) {
        let directory = args_iter
            .next()
            .map(PathBuf::from)
            .ok_or("--webview-self-check requires an isolated target directory")?;
        if args_iter.next().is_some() {
            return Err("--webview-self-check accepts one target directory".into());
        }
        println!(
            "{}",
            window::webview_self_check(
                &directory.join("window-state.ini"),
                &directory.join("webview"),
                100,
            )?
        );
        return Ok(());
    }
    if first == Some(&std::ffi::OsString::from("--resources-self-check")) {
        let directory = args_iter
            .next()
            .map(PathBuf::from)
            .ok_or("--resources-self-check requires an isolated target directory")?;
        if args_iter.next().is_some() {
            return Err("--resources-self-check accepts one target directory".into());
        }
        println!(
            "{}",
            window::resources_self_check(
                &directory.join("window-state.ini"),
                &directory.join("webview"),
            )?
        );
        return Ok(());
    }
    // US-115 : Vérifier si une instance est déjà en cours et transmettre les arguments
    let executable = std::env::current_exe()?;
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    
    match data::resolve_data_paths(&executable, local.as_deref(), data::choose_data_mode)? {
        data::StartupOutcome::Quit => {}
        data::StartupOutcome::MoveRequired => data::show_move_instructions(),
        data::StartupOutcome::Ready(paths) => {
            // Gestion de l'instance unique pour Windows
            #[cfg(windows)]
            {
                use win32_helpers::{CreateMutexW, OpenMutexW, MUTEX_ALL_ACCESS, FALSE, FindWindowW, PostMessageW, COPYDATASTRUCT, WM_COPYDATA, LPARAM, LPVOID};
                
                let mutex_name = "AtelierSingleInstanceMutexV1";
                let mutex_name_wide: Vec<u16> = mutex_name.encode_utf16().chain(Some(0)).collect();
                
                // D'abord essayer d'ouvrir le mutex pour voir si une instance existe
                let existing_mutex = unsafe {
                    OpenMutexW(MUTEX_ALL_ACCESS, FALSE, mutex_name_wide.as_ptr())
                };
                
                if !existing_mutex.is_null() {
                    // Une instance existe déjà - transmettre les chemins
                    if !arguments.is_empty() {
                        // Préparer les chemins à transmettre
                        let paths_str = arguments
                            .iter()
                            .map(|s| s.to_string_lossy())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join("|");
                        
                        if !paths_str.is_empty() {
                            // Utiliser la classe de fenêtre et le titre de la fenêtre principale
                            let window_class = "AtelierWindow";
                            let window_class_wide: Vec<u16> = window_class.encode_utf16().chain(Some(0)).collect();
                            let window_title = format!("Atelier — {}", paths.mode_label());
                            let window_title_wide: Vec<u16> = window_title.encode_utf16().chain(Some(0)).collect();
                            
                            unsafe {
                                let hwnd = FindWindowW(window_class_wide.as_ptr(), window_title_wide.as_ptr());
                                
                                if !hwnd.is_null() {
                                    // Préparer les données pour WM_COPYDATA
                                    let paths_bytes: Vec<u8> = paths_str
                                        .encode_utf16()
                                        .flat_map(|c| vec![(c as u16) as u8, ((c as u16) >> 8) as u8])
                                        .collect();
                                    
                                    let cds = COPYDATASTRUCT {
                                        dwId: 0,
                                        cbData: paths_bytes.len(),
                                        lpData: paths_bytes.as_ptr() as LPVOID,
                                    };
                                    
                                    // Envoyer le message WM_COPYDATA
                                    PostMessageW(
                                        hwnd,
                                        WM_COPYDATA,
                                        0,
                                        &cds as *const COPYDATASTRUCT as LPARAM
                                    );
                                }
                            }
                        }
                    }
                    // Terminer cette instance puisqu'une autre tourne déjà
                    return Ok(());
                } else {
                    // Aucune instance existante - créer le mutex
                    let mutex = unsafe {
                        CreateMutexW(mutex_name_wide.as_ptr(), FALSE, MUTEX_ALL_ACCESS)
                    };
                    
                    // Vérifier si le mutex a été créé correctement
                    if mutex.is_null() {
                        // Erreur : une autre instance a pu créer le mutex en même temps
                        // Réessayer
                        let retry_mutex = unsafe {
                            OpenMutexW(MUTEX_ALL_ACCESS, FALSE, mutex_name_wide.as_ptr())
                        };
                        if !retry_mutex.is_null() {
                            // Une instance existe maintenant - transmettre les chemins et quitter
                            if !arguments.is_empty() {
                                let paths_str = arguments
                                    .iter()
                                    .map(|s| s.to_string_lossy())
                                    .filter(|s| !s.is_empty())
                                    .collect::<Vec<_>>()
                                    .join("|");
                                
                                if !paths_str.is_empty() {
                                    let window_class = "AtelierWindow";
                                    let window_class_wide: Vec<u16> = window_class.encode_utf16().chain(Some(0)).collect();
                                    let window_title = format!("Atelier — {}", paths.mode_label());
                                    let window_title_wide: Vec<u16> = window_title.encode_utf16().chain(Some(0)).collect();
                                    
                                    unsafe {
                                        let hwnd = FindWindowW(window_class_wide.as_ptr(), window_title_wide.as_ptr());
                                        
                                        if !hwnd.is_null() {
                                            let paths_bytes: Vec<u8> = paths_str
                                                .encode_utf16()
                                                .flat_map(|c| vec![(c as u16) as u8, ((c as u16) >> 8) as u8])
                                                .collect();
                                            
                                            let cds = COPYDATASTRUCT {
                                                dwId: 0,
                                                cbData: paths_bytes.len(),
                                                lpData: paths_bytes.as_ptr() as LPVOID,
                                            };
                                            
                                            PostMessageW(
                                                hwnd,
                                                WM_COPYDATA,
                                                0,
                                                &cds as *const COPYDATASTRUCT as LPARAM
                                            );
                                        }
                                    }
                                }
                            }
                            return Ok(());
                        }
                    }
                }
            }
            
            // Créer la fenêtre avec les chemins chargés
            let title = format!("Atelier — {}", paths.mode_label());
            window::run(&paths.window_state_file(), &title, &paths.webview_dir())?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn finish_startup(
    outcome: atelier_desktop_win::data::StartupOutcome,
    open_window: impl FnOnce(&std::path::Path, &str, &std::path::Path) -> std::io::Result<()>,
    show_move: impl FnOnce(),
) -> std::io::Result<()> {
    use atelier_desktop_win::data;
    match outcome {
        data::StartupOutcome::Ready(paths) => {
            let title = format!("Atelier — {}", paths.mode_label());
            open_window(&paths.window_state_file(), &title, &paths.webview_dir())?;
        }
        data::StartupOutcome::MoveRequired => show_move(),
        data::StartupOutcome::Quit => {}
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {}

#[cfg(all(test, windows))]
mod tests {
    use super::finish_startup;
    use atelier_desktop_win::data::{StartupChoice, resolve_data_paths};
    use std::cell::Cell;

    #[test]
    fn resolved_storage_controls_normal_startup_and_local_title() {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let root = temp.join(format!(
            "atelier-startup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let executable = root.join("atelier.exe");
        let portable =
            resolve_data_paths(&executable, None, |_| panic!("portable is writable")).unwrap();
        finish_startup(
            portable,
            |path, title, webview| {
                assert_eq!(path, root.join("atelier-data/window-state.ini"));
                assert_eq!(title, "Atelier — Portable");
                assert_eq!(webview, root.join("atelier-data/webview"));
                Ok(())
            },
            || panic!("portable must open a window"),
        )
        .unwrap();
        std::fs::remove_dir(root.join("atelier-data")).unwrap();
        std::fs::write(root.join("atelier-data"), b"blocked").unwrap();
        let local_root = root.join("local");
        let local =
            resolve_data_paths(&executable, Some(&local_root), |_| StartupChoice::Local).unwrap();
        let error = finish_startup(
            local,
            |path, title, webview| {
                assert_eq!(path, local_root.join("Atelier/window-state.ini"));
                assert_eq!(title, "Atelier — Mode local");
                assert_eq!(webview, local_root.join("Atelier/webview"));
                Err(std::io::Error::other("window creation failed"))
            },
            || panic!("local must open a window"),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "window creation failed");
        for choice in [StartupChoice::Move, StartupChoice::Quit] {
            let moved = Cell::new(false);
            let outcome = resolve_data_paths(&executable, None, |_| choice).unwrap();
            finish_startup(
                outcome,
                |_, _, _| panic!("move/quit must not open a window"),
                || moved.set(true),
            )
            .unwrap();
            assert_eq!(moved.get(), choice == StartupChoice::Move);
        }
        assert!(root.is_absolute() && root.starts_with(&temp) && root != temp);
        std::fs::remove_dir_all(root).unwrap();
    }
}
