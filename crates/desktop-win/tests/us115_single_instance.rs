// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-115 : lance réellement deux processus `atelier.exe` (sondes de diagnostic) sur un dossier de
//! données isolé sous `target/` : la seconde instance transmet ses chemins par `WM_COPYDATA` puis sort.

#![cfg(windows)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn exe() -> &'static str {
    env!("CARGO_BIN_EXE_atelier")
}

fn isolated(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("us115-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn wait_exit(child: &mut Child, limit: Duration) -> std::process::ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            panic!("process did not exit within {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn start_primary(root: &Path, output: &Path) -> Child {
    let mut child = Command::new(exe())
        .arg("--instance-probe-primary")
        .arg(root)
        .arg(output)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        // Garde le tube ouvert jusqu'à la sortie de la sonde (elle écrit encore son résultat).
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        let _ = sender.send(line);
        let mut rest = String::new();
        while reader.read_line(&mut rest).is_ok_and(|n| n > 0) {}
    });
    let line = receiver
        .recv_timeout(Duration::from_secs(15))
        .unwrap_or_else(|_| {
            let _ = child.kill();
            panic!("primary probe never became ready");
        });
    assert_eq!(line.trim(), "primary-ready");
    child
}

fn secondary(
    root: &Path,
    cwd: &Path,
    args: &[&str],
) -> (std::process::ExitStatus, String, Duration) {
    let start = Instant::now();
    let mut child = Command::new(exe())
        .arg("--instance-probe-secondary")
        .arg(root)
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let status = wait_exit(&mut child, Duration::from_secs(20));
    let mut text = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut text).unwrap();
    (status, text.trim().to_owned(), start.elapsed())
}

#[test]
fn second_instance_forwards_absolute_paths_and_exits_while_first_receives_them() {
    let root = isolated("forward");
    let output = root.join("received.txt");
    let mut primary = start_primary(&root, &output);

    let absolute = root.join("é dossier").join("doc ñ.txt");
    let (status, text, elapsed) = secondary(
        &root,
        &root,
        &[absolute.to_str().unwrap(), "relatif.txt", "--ignored"],
    );
    assert!(status.success(), "second instance failed: {status:?}");
    assert_eq!(text, "forwarded");
    assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");

    let primary_status = wait_exit(&mut primary, Duration::from_secs(15));
    assert!(primary_status.success(), "primary: {primary_status:?}");
    let received = std::fs::read_to_string(&output).unwrap();
    let lines: Vec<&str> = received.lines().collect();
    assert_eq!(lines.len(), 2, "{received:?}");
    assert_eq!(Path::new(lines[0]), absolute);
    assert_eq!(Path::new(lines[1]), root.join("relatif.txt"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn different_data_roots_do_not_see_each_other_and_mutex_is_released_on_exit() {
    let first = isolated("root-a");
    let second = isolated("root-b");
    let mut primary = start_primary(&first, &first.join("out.txt"));
    // Autre racine : pas d'instance existante, donc cette sonde devient la première.
    let (status, text, _) = secondary(&second, &second, &[]);
    assert!(status.success());
    assert_eq!(text, "became-primary");
    // Même racine que la première : transmission.
    let (status, text, _) = secondary(&first, &first, &[]);
    assert!(status.success());
    assert_eq!(text, "forwarded");
    // Le premier processus a reçu une charge vide valide (simple mise au premier plan) puis s'arrête.
    assert!(wait_exit(&mut primary, Duration::from_secs(15)).success());
    // Mutex libéré à la sortie : la racine est de nouveau disponible.
    let (_, text, _) = secondary(&first, &first, &[]);
    assert_eq!(text, "became-primary");
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}
