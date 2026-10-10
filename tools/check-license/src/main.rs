// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const SPDX: &str = "SPDX-License-Identifier: AGPL-3.0-or-later";
const ROOTS: [&str; 2] = ["crates", "tools"];

fn main() -> ExitCode {
    let root = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    match check_tree(&root) {
        Ok(()) => {
            println!("All Rust source files have the required SPDX header.");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for error in errors {
                eprintln!("{error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn check_tree(root: &Path) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for source_root in ROOTS {
        let path = root.join(source_root);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                errors.push(format!("{}: missing source root", path.display()));
                continue;
            }
            Err(error) => {
                errors.push(format!(
                    "{}: cannot access source root: {error}",
                    path.display()
                ));
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            errors.push(format!(
                "{}: symlink source root is not allowed",
                path.display()
            ));
            continue;
        }
        if !metadata.is_dir() {
            errors.push(format!(
                "{}: source root is not a directory",
                path.display()
            ));
            continue;
        }
        if let Err(error) = visit(&path, &mut errors) {
            errors.push(format!(
                "{}: cannot read source root: {error}",
                path.display()
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn visit(path: &Path, errors: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            errors.push(format!(
                "{}: symlink source entry is not allowed",
                path.display()
            ));
            continue;
        }
        if file_type.is_dir() {
            if matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some("target" | "graphify-out" | "_bmad" | "_bmad-output")
            ) {
                continue;
            }
            visit(&path, errors)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            match fs::read_to_string(&path) {
                Ok(contents)
                    if contents
                        .lines()
                        .take(3)
                        .any(|line| line.trim() == format!("// {SPDX}")) => {}
                Ok(_) => errors.push(format!(
                    "{}: missing or incorrect SPDX header",
                    path.display()
                )),
                Err(error) => {
                    errors.push(format!("{}: cannot read source: {error}", path.display()))
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{SPDX, check_tree};
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestTree(PathBuf);

    impl TestTree {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("atelier-license-{}-{unique}", std::process::id()));
            fs::create_dir_all(path.join("crates/example/src")).expect("create source tree");
            fs::create_dir_all(path.join("tools")).expect("create tools source root");
            Self(path)
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn accepts_a_valid_header() {
        let tree = TestTree::new();
        fs::write(
            tree.0.join("crates/example/src/lib.rs"),
            format!("// {SPDX}\n#![forbid(unsafe_code)]\n"),
        )
        .expect("write source");
        assert!(check_tree(&tree.0).is_ok());
    }

    #[test]
    fn rejects_missing_and_incorrect_headers_and_reports_the_paths() {
        let tree = TestTree::new();
        let missing_path = tree.0.join("crates/example/src/missing.rs");
        let incorrect_path = tree.0.join("crates/example/src/incorrect.rs");
        fs::write(&missing_path, "#![forbid(unsafe_code)]\n").expect("write source");
        fs::write(
            &incorrect_path,
            "// SPDX-License-Identifier: MIT\nfn main() {}\n",
        )
        .expect("write source with incorrect identifier");
        let errors = check_tree(&tree.0).expect_err("missing header must fail");
        for path in [&missing_path, &incorrect_path] {
            let file_name = path
                .file_name()
                .expect("source file name")
                .to_string_lossy();
            assert!(
                errors.iter().any(|error| {
                    error.contains("missing or incorrect SPDX header")
                        && error.contains(file_name.as_ref())
                }),
                "expected a diagnostic for {}; got {errors:?}",
                path.display()
            );
        }
    }

    #[test]
    fn reports_invalid_utf8_instead_of_succeeding() {
        let tree = TestTree::new();
        let path = tree.0.join("crates/example/src/lib.rs");
        fs::write(&path, [0xff, 0xfe]).expect("write invalid UTF-8");
        let errors = check_tree(&tree.0).expect_err("invalid UTF-8 must fail");
        assert!(
            errors
                .iter()
                .any(|error| error.contains("cannot read source") && error.contains("lib.rs"))
        );
    }

    #[test]
    fn ignores_generated_and_agent_work_directories() {
        let tree = TestTree::new();
        for directory in [
            "crates/example/target",
            "crates/example/graphify-out",
            "tools/check-license/_bmad",
            "tools/check-license/_bmad-output",
        ] {
            let path = tree.0.join(directory).join("generated.rs");
            fs::create_dir_all(path.parent().expect("parent directory"))
                .expect("create generated directory");
            fs::write(path, [0xff, 0xfe]).expect("write invalid generated source");
        }
        assert!(check_tree(&tree.0).is_ok());
    }

    #[test]
    fn rejects_symlinked_files_and_directories_when_supported() {
        let tree = TestTree::new();
        let target_file = tree.0.join("external.rs");
        let target_directory = tree.0.join("external-dir");
        fs::write(&target_file, format!("// {SPDX}\n")).expect("write symlink target");
        fs::create_dir(&target_directory).expect("create symlink directory target");

        let file_link = tree.0.join("crates/example/src/linked.rs");
        let directory_link = tree.0.join("crates/example/linked-dir");
        let file_link_created = create_symlink(&target_file, &file_link, false).is_ok();
        let directory_link_created =
            create_symlink(&target_directory, &directory_link, true).is_ok();
        if !file_link_created && !directory_link_created {
            return;
        }

        let errors = check_tree(&tree.0).expect_err("symlink source entries must fail");
        for (link, created) in [
            (&file_link, file_link_created),
            (&directory_link, directory_link_created),
        ] {
            if created {
                let file_name = link.file_name().expect("symlink name").to_string_lossy();
                assert!(errors.iter().any(|error| {
                    error.contains("symlink source entry is not allowed")
                        && error.contains(file_name.as_ref())
                }));
            }
        }
    }

    fn create_symlink(target: &Path, link: &Path, is_directory: bool) -> io::Result<()> {
        #[cfg(unix)]
        {
            let _ = is_directory;
            std::os::unix::fs::symlink(target, link)
        }
        #[cfg(windows)]
        {
            if is_directory {
                std::os::windows::fs::symlink_dir(target, link)
            } else {
                std::os::windows::fs::symlink_file(target, link)
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, link, is_directory);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "symlink creation is unsupported on this platform",
            ))
        }
    }
}
