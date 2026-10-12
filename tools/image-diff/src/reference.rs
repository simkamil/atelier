// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reference-image assertions: loading, failure artifacts and the explicit update mode.

use crate::{DiffError, Image, Report, Tolerance, compare, diff_image};
use atelier_image::{DecodeLimits, ImageError, decode_png, encode_png_rgba};
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Environment variable that enables the update mode when set to `1`.
pub const UPDATE_ENV: &str = "ATELIER_UPDATE_REFERENCES";
/// Optional environment variable naming the directory for `*.actual.png` and `*.diff.png`.
pub const OUT_DIR_ENV: &str = "ATELIER_REFERENCE_OUT_DIR";

/// Behaviour of the assertion. The default never writes a reference.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Config {
    /// Rewrite references that are missing or outside the tolerance instead of failing.
    pub update: bool,
    /// Continuous integration: the update mode is refused.
    pub ci: bool,
    /// Directory for the failure artifacts; next to the reference when `None`.
    pub output_dir: Option<PathBuf>,
}

impl Config {
    /// Reads `ATELIER_UPDATE_REFERENCES` (`1` enables the update), `ATELIER_REFERENCE_OUT_DIR`
    /// and `CI` (any non-empty value other than `0`/`false` marks continuous integration).
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        Self {
            update: var(UPDATE_ENV).is_some_and(|v| v == "1"),
            ci: var("CI").is_some_and(|v| v != "0" && v != "false"),
            output_dir: var(OUT_DIR_ENV).map(PathBuf::from),
        }
    }
}

/// Successful outcomes.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The image matches the reference within the tolerance.
    Matched(Report),
    /// The reference did not exist and was created (update mode).
    Created,
    /// The reference differed (or was unreadable) and was rewritten (update mode).
    Updated,
}

/// Failures of the assertion.
#[derive(Clone, Debug, PartialEq)]
pub enum MatchError {
    /// The update mode was requested in continuous integration.
    UpdateInCi,
    /// The reference does not exist and the update mode is off.
    ReferenceMissing { path: PathBuf },
    /// A file could not be read or written.
    Io { path: PathBuf, message: String },
    /// A reference file is not a valid PNG.
    Decode { path: PathBuf, error: ImageError },
    /// The image could not be encoded.
    Encode(ImageError),
    /// The images cannot be compared (dimensions, invalid buffer, invalid tolerance).
    Incomparable {
        error: DiffError,
        reference: PathBuf,
        actual_path: Option<PathBuf>,
    },
    /// The images differ beyond the tolerance.
    Mismatch {
        reference: PathBuf,
        actual_path: PathBuf,
        diff_path: PathBuf,
        report: Box<Report>,
    },
}

impl fmt::Display for MatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UpdateInCi => write!(
                f,
                "{UPDATE_ENV} is refused when CI is set; regenerate references locally"
            ),
            Self::ReferenceMissing { path } => write!(
                f,
                "reference image {} does not exist; create it by running the test once with \
                 {UPDATE_ENV}=1 and review the file before committing it",
                path.display()
            ),
            Self::Io { path, message } => write!(f, "{}: {message}", path.display()),
            Self::Decode { path, error } => {
                write!(
                    f,
                    "reference image {} is not a valid PNG: {error}",
                    path.display()
                )
            }
            Self::Encode(error) => write!(f, "cannot encode the image: {error}"),
            Self::Incomparable {
                error,
                reference,
                actual_path,
            } => {
                write!(f, "cannot compare with {}: {error}", reference.display())?;
                if let Some(actual) = actual_path {
                    write!(f, "\n  actual image: {}", actual.display())?;
                }
                Ok(())
            }
            Self::Mismatch {
                reference,
                actual_path,
                diff_path,
                report,
            } => write!(
                f,
                "image differs from reference {}\n{report}\n  actual image: {}\n  difference image: {}\n  \
                 if the change is intended, regenerate with {UPDATE_ENV}=1",
                reference.display(),
                actual_path.display(),
                diff_path.display()
            ),
        }
    }
}
impl std::error::Error for MatchError {}

fn io_error(path: &Path, error: &std::io::Error) -> MatchError {
    MatchError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

/// Reads and decodes a PNG file.
pub fn load_png(path: &Path) -> Result<Image, MatchError> {
    let bytes = fs::read(path).map_err(|e| io_error(path, &e))?;
    decode_png(&bytes, DecodeLimits::default()).map_err(|error| MatchError::Decode {
        path: path.to_path_buf(),
        error,
    })
}

/// Encodes and writes a PNG file, creating missing parent directories.
pub fn save_png(path: &Path, image: &Image) -> Result<(), MatchError> {
    let bytes = encode_png_rgba(image, DecodeLimits::default()).map_err(MatchError::Encode)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| io_error(parent, &e))?;
    }
    fs::write(path, bytes).map_err(|e| io_error(path, &e))
}

/// Paths of `<name>.actual.png` and `<name>.diff.png` for a reference `<name>.png`, next to the
/// reference or in `output_dir`.
pub fn artifact_paths(reference: &Path, output_dir: Option<&Path>) -> (PathBuf, PathBuf) {
    let stem = reference
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_owned());
    let dir = match output_dir {
        Some(dir) => dir.to_path_buf(),
        None => reference
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default(),
    };
    (
        dir.join(format!("{stem}.actual.png")),
        dir.join(format!("{stem}.diff.png")),
    )
}

/// Compares `actual` with the reference PNG at `reference` and applies `config`.
///
/// On a mismatch the obtained image and a difference image are written (see
/// [`artifact_paths`]); in update mode the reference is written instead. A missing reference
/// is an error unless the update mode is on.
pub fn check_image_matches(
    actual: &Image,
    reference: &Path,
    tolerance: &Tolerance,
    config: &Config,
) -> Result<Outcome, MatchError> {
    if config.update && config.ci {
        return Err(MatchError::UpdateInCi);
    }
    let bytes = match fs::read(reference) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            if config.update {
                save_png(reference, actual)?;
                return Ok(Outcome::Created);
            }
            return Err(MatchError::ReferenceMissing {
                path: reference.to_path_buf(),
            });
        }
        Err(e) => return Err(io_error(reference, &e)),
    };
    let expected = match decode_png(&bytes, DecodeLimits::default()) {
        Ok(image) => image,
        Err(_) if config.update => {
            save_png(reference, actual)?;
            return Ok(Outcome::Updated);
        }
        Err(error) => {
            return Err(MatchError::Decode {
                path: reference.to_path_buf(),
                error,
            });
        }
    };
    let (actual_path, diff_path) = artifact_paths(reference, config.output_dir.as_deref());
    let report = match compare(&expected, actual, tolerance) {
        Ok(report) => report,
        Err(error) => {
            if config.update && !matches!(error, DiffError::InvalidTolerance) {
                save_png(reference, actual)?;
                return Ok(Outcome::Updated);
            }
            let written = save_png(&actual_path, actual).ok().map(|()| actual_path);
            return Err(MatchError::Incomparable {
                error,
                reference: reference.to_path_buf(),
                actual_path: written,
            });
        }
    };
    if report.passed {
        // Stale artifacts of an earlier failure would mislead.
        let _ = fs::remove_file(&actual_path);
        let _ = fs::remove_file(&diff_path);
        return Ok(Outcome::Matched(report));
    }
    if config.update {
        save_png(reference, actual)?;
        return Ok(Outcome::Updated);
    }
    save_png(&actual_path, actual)?;
    let diff =
        diff_image(&expected, actual, tolerance).map_err(|error| MatchError::Incomparable {
            error,
            reference: reference.to_path_buf(),
            actual_path: Some(actual_path.clone()),
        })?;
    save_png(&diff_path, &diff)?;
    Err(MatchError::Mismatch {
        reference: reference.to_path_buf(),
        actual_path,
        diff_path,
        report: Box::new(report),
    })
}

/// Test assertion with an explicit [`Config`]; panics with the full message on failure.
pub fn assert_image_matches_with(
    actual: &Image,
    reference: impl AsRef<Path>,
    tolerance: &Tolerance,
    config: &Config,
) {
    if let Err(error) = check_image_matches(actual, reference.as_ref(), tolerance, config) {
        panic!("{error}");
    }
}

/// Test assertion configured from the environment ([`Config::from_env`]). A relative path is
/// relative to the working directory of the test (the crate root under `cargo test`); the
/// [`assert_image_matches!`](crate::assert_image_matches) macro resolves it against
/// `CARGO_MANIFEST_DIR` explicitly.
pub fn assert_image_matches(actual: &Image, reference: impl AsRef<Path>, tolerance: &Tolerance) {
    assert_image_matches_with(actual, reference, tolerance, &Config::from_env());
}
