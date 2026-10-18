// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native desktop entry point. The data root is resolved before creating caches.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if let Err(error) = start() {
        if matches!(
            std::env::args_os().nth(1).as_deref(),
            Some(arg) if arg == "--self-check" || arg == "--webview-self-check" || arg == "--resources-self-check"
                || arg == "--instance-probe-primary"
                || arg == "--instance-probe-hold"
                || arg == "--instance-probe-secondary"
        ) {
            eprintln!("{error}");
        } else {
            atelier_desktop_win::window::show_error(&error.to_string());
        }
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn start() -> Result<(), Box<dyn std::error::Error>> {
    use atelier_desktop_win::{data, instance_win32, window};
    use std::path::PathBuf;

    window::initialize_dpi()?;
    let mut arguments = std::env::args_os().skip(1);
    let first = arguments.next();
    if first.as_deref() == Some(std::ffi::OsStr::new("--self-check")) {
        let state_file = arguments
            .next()
            .map(PathBuf::from)
            .ok_or("--self-check requires an isolated state file")?;
        println!("{}", window::self_check(&state_file)?);
        return Ok(());
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("--webview-self-check")) {
        let directory = arguments
            .next()
            .map(PathBuf::from)
            .ok_or("--webview-self-check requires an isolated target directory")?;
        if arguments.next().is_some() {
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
    if first.as_deref() == Some(std::ffi::OsStr::new("--resources-self-check")) {
        let directory = arguments
            .next()
            .map(PathBuf::from)
            .ok_or("--resources-self-check requires an isolated target directory")?;
        if arguments.next().is_some() {
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
    if first.as_deref() == Some(std::ffi::OsStr::new("--bridge-self-check")) {
        let directory = arguments
            .next()
            .map(PathBuf::from)
            .ok_or("--bridge-self-check requires an isolated target directory")?;
        if arguments.next().is_some() {
            return Err("--bridge-self-check accepts one target directory".into());
        }
        println!(
            "{}",
            window::bridge_self_check(
                &directory.join("window-state.ini"),
                &directory.join("webview"),
            )?
        );
        return Ok(());
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("--instance-probe-primary")) {
        let root = arguments
            .next()
            .map(PathBuf::from)
            .ok_or("--instance-probe-primary requires a data root and an output file")?;
        let output = arguments
            .next()
            .map(PathBuf::from)
            .ok_or("--instance-probe-primary requires a data root and an output file")?;
        println!("{}", instance_win32::probe_primary(&root, &output)?);
        return Ok(());
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("--instance-probe-hold")) {
        let root = arguments
            .next()
            .map(PathBuf::from)
            .ok_or("--instance-probe-hold requires a data root and a duration")?;
        let milliseconds: u32 = arguments
            .next()
            .and_then(|value| value.to_str().and_then(|text| text.parse().ok()))
            .ok_or("--instance-probe-hold requires a duration in milliseconds")?;
        println!("{}", instance_win32::probe_hold(&root, milliseconds)?);
        return Ok(());
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("--instance-probe-secondary")) {
        let root = arguments
            .next()
            .map(PathBuf::from)
            .ok_or("--instance-probe-secondary requires a data root")?;
        let rest: Vec<std::ffi::OsString> = arguments.collect();
        let paths = launch_paths(&rest)?;
        println!("{}", instance_win32::probe_secondary(&root, &paths)?);
        return Ok(());
    }
    let executable = std::env::current_exe()?;
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let launch = launch_paths(&arguments)?;
    finish_startup(
        data::resolve_data_paths(&executable, local.as_deref(), data::choose_data_mode)?,
        move |state, title, webview| open_main_window(state, title, webview, launch),
        data::show_move_instructions,
    )?;
    Ok(())
}

/// US-115 : chemins absolus des arguments (options `--…` ignorées), au plus `MAX_PATHS`.
#[cfg(windows)]
fn launch_paths(
    arguments: &[std::ffi::OsString],
) -> Result<Vec<std::path::PathBuf>, Box<dyn std::error::Error>> {
    use atelier_desktop_win::instance;
    let cwd = std::env::current_dir()?;
    let mut paths = Vec::new();
    for argument in instance::launch_arguments(arguments)
        .into_iter()
        .take(instance::MAX_PATHS)
    {
        paths.push(instance::normalize(argument, &cwd).map_err(|error| {
            format!(
                "argument invalide « {} » : {error:?}",
                argument.to_string_lossy()
            )
        })?);
    }
    Ok(paths)
}

/// US-115 : mutex nommé par racine de données ; une instance déjà ouverte reçoit les chemins et
/// celle-ci se termine. Si la première instance ne répond pas (5 s de tentatives), on n'ouvre pas
/// une seconde fenêtre sur le même dossier de données : message d'erreur.
#[cfg(windows)]
fn open_main_window(
    state_file: &std::path::Path,
    title: &str,
    webview_data: &std::path::Path,
    launch: Vec<std::path::PathBuf>,
) -> std::io::Result<()> {
    use atelier_desktop_win::instance_win32::{Acquire, acquire};
    use atelier_desktop_win::{instance, window};
    let root = state_file
        .parent()
        .ok_or_else(|| std::io::Error::other("data root unavailable"))?;
    match acquire(root, &launch)? {
        Acquire::Primary(_guard) => window::run_with_launch(
            state_file,
            title,
            webview_data,
            Some(&instance::instance_key(root)),
            launch,
        ),
        Acquire::Forwarded => Ok(()),
        Acquire::Unreachable => Err(std::io::Error::other(
            "Atelier est déjà ouvert mais ne répond pas. Fermez la fenêtre existante ou terminez le processus, puis relancez.",
        )),
    }
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
