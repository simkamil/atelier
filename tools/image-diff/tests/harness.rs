// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

use atelier_image::ImageError;
use atelier_image_diff::{
    Config, DIFF_MARK, DiffError, Image, MatchError, MaxDifferent, Outcome, Tolerance,
    artifact_paths, check_image_matches, compare, diff_image, load_png, save_png,
};
use std::path::PathBuf;
use std::process::Command;

fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Image {
    Image {
        width,
        height,
        pixels: rgba.repeat((width * height) as usize),
    }
}

fn set(image: &mut Image, x: u32, y: u32, rgba: [u8; 4]) {
    let i = ((y * image.width + x) * 4) as usize;
    image.pixels[i..i + 4].copy_from_slice(&rgba);
}

fn get(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * image.width + x) * 4) as usize;
    image.pixels[i..i + 4].try_into().unwrap()
}

/// Fresh directory under `target/`, unique per test.
fn temp_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("image-diff-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn identical_images_match_exactly() {
    let a = solid(4, 3, [10, 20, 30, 255]);
    let report = compare(&a, &a.clone(), &Tolerance::exact()).unwrap();
    assert!(report.passed);
    assert_eq!(report.different_pixels, 0);
    assert_eq!(report.max_difference, [0; 4]);
    assert_eq!(report.bounds, None);
    assert_eq!(report.total_pixels, 12);
}

#[test]
fn one_pixel_under_and_over_channel_tolerance() {
    let a = solid(4, 4, [100, 100, 100, 255]);
    let mut b = a.clone();
    set(&mut b, 2, 1, [103, 100, 100, 255]);
    let at = compare(&a, &b, &Tolerance::channel(3)).unwrap();
    assert!(at.passed, "difference equal to the tolerance is accepted");
    assert_eq!(at.different_pixels, 0);
    assert_eq!(at.max_difference, [3, 0, 0, 0]);
    let over = compare(&a, &b, &Tolerance::channel(2)).unwrap();
    assert!(!over.passed);
    assert_eq!(over.different_pixels, 1);
    assert_eq!(
        over.bounds.map(|b| (b.x, b.y, b.width, b.height)),
        Some((2, 1, 1, 1))
    );
    assert!((over.mean_difference[0] - 3.0 / 16.0).abs() < 1e-12);
}

#[test]
fn proportion_of_pixels_under_and_over_threshold() {
    let a = solid(10, 10, [0, 0, 0, 255]);
    let mut b = a.clone();
    for x in 0..5 {
        set(&mut b, x, 0, [255, 0, 0, 255]);
    }
    // 5 of 100 pixels = 5 %.
    assert!(compare(&a, &b, &Tolerance::new(0, 5.0)).unwrap().passed);
    assert!(!compare(&a, &b, &Tolerance::new(0, 4.9)).unwrap().passed);
    let counted = |n| Tolerance {
        channel: 0,
        max_different: MaxDifferent::Count(n),
    };
    assert!(compare(&a, &b, &counted(5)).unwrap().passed);
    assert!(!compare(&a, &b, &counted(4)).unwrap().passed);
    let report = compare(&a, &b, &Tolerance::new(0, 5.0)).unwrap();
    assert!((report.fraction_different() - 0.05).abs() < 1e-12);
    assert_eq!(
        report.bounds.map(|b| (b.x, b.y, b.width, b.height)),
        Some((0, 0, 5, 1))
    );
}

#[test]
fn invalid_percentage_is_an_error() {
    let a = solid(1, 1, [0; 4]);
    for p in [-1.0, 100.5, f64::NAN] {
        assert_eq!(
            compare(&a, &a, &Tolerance::new(0, p)),
            Err(DiffError::InvalidTolerance)
        );
    }
}

#[test]
fn different_dimensions_fail_without_panic() {
    let a = solid(4, 3, [0; 4]);
    let b = solid(3, 4, [0; 4]);
    let expected = Err(DiffError::DimensionMismatch {
        expected: (4, 3),
        actual: (3, 4),
    });
    assert_eq!(compare(&a, &b, &Tolerance::exact()), expected);
    assert!(diff_image(&a, &b, &Tolerance::exact()).is_err());
    let broken = Image {
        width: 4,
        height: 3,
        pixels: vec![0; 5],
    };
    assert_eq!(
        compare(&a, &broken, &Tolerance::exact()),
        Err(DiffError::InvalidImage)
    );
}

#[test]
fn alpha_is_compared_premultiplied() {
    // Invisible colour: identical.
    let a = solid(1, 1, [255, 0, 0, 0]);
    let b = solid(1, 1, [0, 0, 255, 0]);
    assert!(compare(&a, &b, &Tolerance::exact()).unwrap().passed);
    // A change of alpha counts, in the alpha channel and in the premultiplied colour.
    let c = solid(1, 1, [200, 100, 50, 255]);
    let d = solid(1, 1, [200, 100, 50, 128]);
    let report = compare(&c, &d, &Tolerance::exact()).unwrap();
    assert!(!report.passed);
    assert_eq!(report.max_difference[3], 127);
    assert_eq!(report.max_difference[0], 200 - 100);
    // Same colour at low alpha: a colour change of 10 is only a 1-level premultiplied change.
    let e = solid(1, 1, [100, 0, 0, 25]);
    let f = solid(1, 1, [110, 0, 0, 25]);
    assert_eq!(
        compare(&e, &f, &Tolerance::exact()).unwrap().max_difference[0],
        1
    );
}

#[test]
fn diff_image_marks_only_faulty_pixels() {
    let a = solid(4, 3, [40, 80, 120, 255]);
    let mut b = a.clone();
    set(&mut b, 1, 1, [200, 80, 120, 255]); // beyond tolerance
    set(&mut b, 3, 2, [41, 80, 120, 255]); // within tolerance
    let diff = diff_image(&a, &b, &Tolerance::channel(2)).unwrap();
    assert_eq!((diff.width, diff.height), (4, 3));
    for y in 0..3 {
        for x in 0..4 {
            let marked = get(&diff, x, y) == DIFF_MARK;
            assert_eq!(marked, (x, y) == (1, 1), "pixel ({x},{y})");
            assert_eq!(get(&diff, x, y)[3], 255);
        }
    }
    // Others are the faded rendering: lighter than the original.
    assert!(get(&diff, 0, 0)[0] > 40 && get(&diff, 0, 0)[2] > 120);
}

#[test]
fn update_mode_creates_then_reads_back() {
    let dir = temp_dir("update");
    let reference = dir.join("nested").join("ref.png");
    let image = solid(3, 2, [1, 2, 3, 255]);
    let update = Config {
        update: true,
        ..Config::default()
    };
    let strict = Config::default();
    let tol = Tolerance::exact();
    assert_eq!(
        check_image_matches(&image, &reference, &tol, &update),
        Ok(Outcome::Created)
    );
    assert_eq!(load_png(&reference).unwrap(), image);
    assert!(matches!(
        check_image_matches(&image, &reference, &tol, &strict),
        Ok(Outcome::Matched(r)) if r.passed
    ));
    // A changed image fails without update and rewrites the reference with it.
    let changed = solid(3, 2, [9, 9, 9, 255]);
    assert!(check_image_matches(&changed, &reference, &tol, &strict).is_err());
    assert_eq!(
        check_image_matches(&changed, &reference, &tol, &update),
        Ok(Outcome::Updated)
    );
    assert_eq!(load_png(&reference).unwrap(), changed);
    // Update refuses to run in CI.
    let ci = Config { ci: true, ..update };
    assert_eq!(
        check_image_matches(&changed, &reference, &tol, &ci),
        Err(MatchError::UpdateInCi)
    );
}

#[test]
fn missing_reference_fails_without_creating_it() {
    let dir = temp_dir("missing");
    let reference = dir.join("absent.png");
    let error = check_image_matches(
        &solid(2, 2, [0; 4]),
        &reference,
        &Tolerance::exact(),
        &Config::default(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        MatchError::ReferenceMissing {
            path: reference.clone()
        }
    );
    assert!(error.to_string().contains("ATELIER_UPDATE_REFERENCES=1"));
    assert!(!reference.exists());
}

#[test]
fn corrupt_reference_is_a_typed_error() {
    let dir = temp_dir("corrupt");
    let reference = dir.join("bad.png");
    std::fs::write(&reference, b"not a png at all").unwrap();
    let error = check_image_matches(
        &solid(2, 2, [0; 4]),
        &reference,
        &Tolerance::exact(),
        &Config::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        MatchError::Decode {
            error: ImageError::InvalidData(_) | ImageError::Truncated | ImageError::Unsupported(_),
            ..
        }
    ));
}

#[test]
fn mismatch_writes_artifacts_and_reports_paths_and_numbers() {
    let dir = temp_dir("mismatch");
    let out = dir.join("out");
    let reference = dir.join("ref.png");
    let expected = solid(5, 5, [255, 255, 255, 255]);
    save_png(&reference, &expected).unwrap();
    let mut actual = expected.clone();
    set(&mut actual, 4, 3, [0, 0, 0, 255]);
    let config = Config {
        output_dir: Some(out.clone()),
        ..Config::default()
    };
    let error = check_image_matches(&actual, &reference, &Tolerance::exact(), &config).unwrap_err();
    let (actual_path, diff_path) = artifact_paths(&reference, Some(&out));
    assert!(actual_path.starts_with(&out) && diff_path.starts_with(&out));
    let message = error.to_string();
    assert!(
        message.contains(&actual_path.display().to_string()),
        "{message}"
    );
    assert!(
        message.contains(&diff_path.display().to_string()),
        "{message}"
    );
    assert!(message.contains("different pixels: 1 of 25"), "{message}");
    assert_eq!(load_png(&actual_path).unwrap(), actual);
    let diff = load_png(&diff_path).unwrap();
    assert_eq!(get(&diff, 4, 3), DIFF_MARK);
    assert_ne!(get(&diff, 0, 0), DIFF_MARK);
    // Nothing is written next to the reference when an output directory is given.
    assert!(!dir.join("ref.actual.png").exists());
    // A later success removes stale artifacts.
    assert!(check_image_matches(&expected, &reference, &Tolerance::exact(), &config).is_ok());
    assert!(!actual_path.exists() && !diff_path.exists());
}

#[test]
fn dimension_mismatch_writes_actual_only() {
    let dir = temp_dir("dims");
    let reference = dir.join("ref.png");
    save_png(&reference, &solid(2, 2, [0; 4])).unwrap();
    let error = check_image_matches(
        &solid(3, 2, [0; 4]),
        &reference,
        &Tolerance::exact(),
        &Config::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        MatchError::Incomparable {
            error: DiffError::DimensionMismatch { .. },
            actual_path: Some(_),
            ..
        }
    ));
    assert!(dir.join("ref.actual.png").exists());
    assert!(!dir.join("ref.diff.png").exists());
}

#[test]
fn stale_difference_image_is_removed_when_dimensions_change() {
    let dir = temp_dir("stale-dims");
    let reference = dir.join("ref.png");
    save_png(&reference, &solid(2, 2, [0; 4])).unwrap();
    let config = Config::default();
    // First failure: same size, both artifacts are written.
    let wrong = solid(2, 2, [9, 9, 9, 255]);
    assert!(check_image_matches(&wrong, &reference, &Tolerance::exact(), &config).is_err());
    assert!(dir.join("ref.diff.png").exists());
    // Second failure: another size, so the old difference image no longer applies.
    let resized = solid(3, 2, [0; 4]);
    assert!(check_image_matches(&resized, &reference, &Tolerance::exact(), &config).is_err());
    assert_eq!(load_png(&dir.join("ref.actual.png")).unwrap(), resized);
    assert!(!dir.join("ref.diff.png").exists());
}

#[test]
fn update_removes_artifacts_of_the_earlier_failure() {
    let dir = temp_dir("stale-update");
    let reference = dir.join("ref.png");
    save_png(&reference, &solid(2, 2, [0; 4])).unwrap();
    let wrong = solid(2, 2, [9, 9, 9, 255]);
    let failing = Config::default();
    assert!(check_image_matches(&wrong, &reference, &Tolerance::exact(), &failing).is_err());
    assert!(dir.join("ref.actual.png").exists() && dir.join("ref.diff.png").exists());
    let update = Config {
        update: true,
        ..Config::default()
    };
    assert_eq!(
        check_image_matches(&wrong, &reference, &Tolerance::exact(), &update),
        Ok(Outcome::Updated)
    );
    assert!(!dir.join("ref.actual.png").exists() && !dir.join("ref.diff.png").exists());
    // Same after a resize and after recreating a deleted reference.
    assert!(check_image_matches(&wrong, &reference, &Tolerance::exact(), &failing).is_ok());
    let resized = solid(3, 2, [0; 4]);
    assert!(check_image_matches(&resized, &reference, &Tolerance::exact(), &failing).is_err());
    assert_eq!(
        check_image_matches(&resized, &reference, &Tolerance::exact(), &update),
        Ok(Outcome::Updated)
    );
    assert!(!dir.join("ref.actual.png").exists());
    assert!(check_image_matches(&wrong, &reference, &Tolerance::exact(), &failing).is_err());
    std::fs::remove_file(&reference).unwrap();
    assert_eq!(
        check_image_matches(&wrong, &reference, &Tolerance::exact(), &update),
        Ok(Outcome::Created)
    );
    assert!(!dir.join("ref.actual.png").exists() && !dir.join("ref.diff.png").exists());
}

#[test]
fn same_named_references_do_not_share_artifacts_in_an_output_directory() {
    let out = PathBuf::from("out");
    let a = artifact_paths(&PathBuf::from("a/page.png"), Some(&out));
    let b = artifact_paths(&PathBuf::from("b/page.png"), Some(&out));
    assert_ne!(a.0, b.0);
    assert_ne!(a.1, b.1);
    // Stable, and independent of the separator style.
    assert_eq!(a, artifact_paths(&PathBuf::from(r"a\page.png"), Some(&out)));
    let name = a.0.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("page-") && name.ends_with(".actual.png"),
        "{name}"
    );
    // Next to the reference the plain name is kept.
    assert_eq!(
        artifact_paths(&PathBuf::from("a/page.png"), None),
        (
            PathBuf::from("a/page.actual.png"),
            PathBuf::from("a/page.diff.png")
        )
    );
}

#[test]
fn save_png_leaves_no_temporary_file() {
    let dir = temp_dir("atomic");
    let path = dir.join("image.png");
    save_png(&path, &solid(2, 2, [1, 2, 3, 255])).unwrap();
    // Replacing an existing file works too.
    save_png(&path, &solid(3, 1, [4, 5, 6, 255])).unwrap();
    assert_eq!(load_png(&path).unwrap(), solid(3, 1, [4, 5, 6, 255]));
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["image.png"]);
}

#[test]
fn environment_rules_for_update_and_ci() {
    let config = |update: Option<&str>, ci: Option<&str>, out: Option<&str>| {
        Config::from_lookup(|name| {
            match name {
                "ATELIER_UPDATE_REFERENCES" => update,
                "CI" => ci,
                "ATELIER_REFERENCE_OUT_DIR" => out,
                _ => None,
            }
            .map(Into::into)
        })
    };
    assert_eq!(config(None, None, None), Config::default());
    // Only the exact value `1` enables the update mode.
    assert!(config(Some("1"), None, None).update);
    for value in ["", "0", "false", "true", "yes", " 1", "11"] {
        assert!(!config(Some(value), None, None).update, "{value:?}");
    }
    // Any non-empty value other than `0` and `false` marks continuous integration.
    for value in ["1", "true", "yes"] {
        assert!(config(None, Some(value), None).ci, "{value:?}");
    }
    for value in ["", "0", "false"] {
        assert!(!config(None, Some(value), None).ci, "{value:?}");
    }
    assert_eq!(
        config(None, None, Some("out")).output_dir,
        Some(PathBuf::from("out"))
    );
    assert_eq!(config(None, None, Some("")).output_dir, None);
    // The combination is what the assertion refuses.
    let both = config(Some("1"), Some("true"), None);
    let dir = temp_dir("env-ci");
    assert_eq!(
        check_image_matches(
            &solid(1, 1, [0; 4]),
            &dir.join("r.png"),
            &Tolerance::exact(),
            &both
        ),
        Err(MatchError::UpdateInCi)
    );
    assert!(!dir.join("r.png").exists());
}

// ------------------------------------------------------------------ command line

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_atelier-image-diff"))
        .args(args)
        .env_remove("CI")
        .output()
        .expect("run image-diff CLI");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn command_line_exit_codes() {
    let dir = temp_dir("cli");
    let p = |n: &str| dir.join(n).display().to_string();
    let a = solid(4, 4, [10, 10, 10, 255]);
    let mut b = a.clone();
    set(&mut b, 0, 0, [14, 10, 10, 255]);
    save_png(&dir.join("a.png"), &a).unwrap();
    save_png(&dir.join("b.png"), &b).unwrap();
    save_png(&dir.join("c.png"), &solid(5, 4, [10, 10, 10, 255])).unwrap();
    std::fs::write(dir.join("bad.png"), b"nope").unwrap();

    let (code, stdout, _) = run(&["compare", &p("a.png"), &p("a.png")]);
    assert_eq!(code, 0);
    assert!(stdout.contains("different pixels: 0 of 16"), "{stdout}");

    let (code, stdout, _) = run(&["compare", &p("a.png"), &p("b.png")]);
    assert_eq!(code, 1);
    assert!(
        stdout.contains("max difference (R G B A): 4 0 0 0"),
        "{stdout}"
    );

    let (code, ..) = run(&[
        "compare",
        &p("a.png"),
        &p("b.png"),
        "--channel-tolerance",
        "4",
    ]);
    assert_eq!(code, 0);
    let (code, ..) = run(&["compare", &p("a.png"), &p("b.png"), "--max-different", "10"]);
    assert_eq!(code, 0);

    let diff = p("out.diff.png");
    let (code, ..) = run(&["compare", &p("a.png"), &p("b.png"), "--diff", &diff]);
    assert_eq!(code, 1);
    assert_eq!(
        get(&load_png(&dir.join("out.diff.png")).unwrap(), 0, 0),
        DIFF_MARK
    );

    let (code, stdout, _) = run(&["compare", &p("a.png"), &p("c.png")]);
    assert_eq!(code, 1);
    assert!(stdout.contains("dimensions differ"), "{stdout}");

    // Usage and I/O errors.
    assert_eq!(run(&["compare", &p("a.png")]).0, 2);
    assert_eq!(run(&["compare", &p("a.png"), &p("missing.png")]).0, 2);
    assert_eq!(run(&["compare", &p("a.png"), &p("bad.png")]).0, 2);
    assert_eq!(
        run(&["compare", &p("a.png"), &p("b.png"), "--max-different", "x"]).0,
        2
    );
    assert_eq!(run(&["compare", &p("a.png"), &p("b.png"), "--bogus"]).0, 2);
    assert_eq!(run(&[]).0, 2);

    // update command.
    let (code, ..) = run(&["update", &p("b.png"), &p("a.png")]);
    assert_eq!(code, 0);
    assert_eq!(load_png(&dir.join("a.png")).unwrap(), b);
}
