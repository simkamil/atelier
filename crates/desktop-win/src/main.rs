// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native desktop entry point. The data root is resolved before creating caches.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if let Err(error) = start() {
        if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--self-check")) {
            eprintln!("{error}");
        } else {
            atelier_desktop_win::window::show_error(&error.to_string());
        }
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn start() -> Result<(), Box<dyn std::error::Error>> {
    use atelier_desktop_win::{data, window};
    use std::path::PathBuf;

    window::initialize_dpi()?;
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() == Some(std::ffi::OsStr::new("--self-check")) {
        let state_file = arguments
            .next()
            .map(PathBuf::from)
            .ok_or("--self-check requires an isolated state file")?;
        println!("{}", window::self_check(&state_file)?);
        return Ok(());
    }
    let executable = std::env::current_exe()?;
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    finish_startup(
        data::resolve_data_paths(&executable, local.as_deref(), data::choose_data_mode)?,
        window::run,
        data::show_move_instructions,
    )?;
    Ok(())
}

#[cfg(windows)]
fn finish_startup(
    outcome: atelier_desktop_win::data::StartupOutcome,
    open_window: impl FnOnce(&std::path::Path, &str) -> std::io::Result<()>,
    show_move: impl FnOnce(),
) -> std::io::Result<()> {
    use atelier_desktop_win::data;
    match outcome {
        data::StartupOutcome::Ready(paths) => {
            let title = format!("Atelier — {}", paths.mode_label());
            open_window(&paths.window_state_file(), &title)?;
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
            |path, title| {
                assert_eq!(path, root.join("atelier-data/window-state.ini"));
                assert_eq!(title, "Atelier — Portable");
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
            |path, title| {
                assert_eq!(path, local_root.join("Atelier/window-state.ini"));
                assert_eq!(title, "Atelier — Mode local");
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
                |_, _| panic!("move/quit must not open a window"),
                || moved.set(true),
            )
            .unwrap();
            assert_eq!(moved.get(), choice == StartupChoice::Move);
        }
        assert!(root.is_absolute() && root.starts_with(&temp) && root != temp);
        std::fs::remove_dir_all(root).unwrap();
    }
}
