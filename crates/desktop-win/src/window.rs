// SPDX-License-Identifier: AGPL-3.0-or-later
//! Window placement state and display-independent geometry rules.

use std::io;
use std::path::Path;

const STATE_MAGIC: &str = "ATELIER-WINDOW";
const STATE_VERSION: u32 = 1;
const MAX_STATE_BYTES: usize = 128;
const MIN_WIDTH: i32 = 640;
const MIN_HEIGHT: i32 = 420;
const DEFAULT_WIDTH: i32 = 1024;
const DEFAULT_HEIGHT: i32 = 720;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(self) -> i32 {
        self.right.saturating_sub(self.left)
    }
    pub fn height(self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }
    fn center(self) -> (i64, i64) {
        (
            (i64::from(self.left) + i64::from(self.right)) / 2,
            (i64::from(self.top) + i64::from(self.bottom)) / 2,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MonitorArea {
    pub monitor: Rect,
    pub work: Rect,
    pub dpi: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WindowState {
    pub normal: Rect,
    pub dpi: u32,
    pub maximized: bool,
}

impl WindowState {
    pub fn default_for(work: Rect, dpi: u32) -> Self {
        let dpi = valid_dpi(dpi);
        let (min_width, min_height) = minimum_size(work, dpi);
        let width = scale_logical_size(DEFAULT_WIDTH, dpi).clamp(min_width, work.width().max(1));
        let height =
            scale_logical_size(DEFAULT_HEIGHT, dpi).clamp(min_height, work.height().max(1));
        let left = work.left.saturating_add((work.width() - width).max(0) / 2);
        let top = work.top.saturating_add((work.height() - height).max(0) / 2);
        Self {
            normal: Rect {
                left,
                top,
                right: left.saturating_add(width),
                bottom: top.saturating_add(height),
            },
            dpi,
            maximized: false,
        }
    }
}

fn valid_dpi(dpi: u32) -> u32 {
    if (48..=960).contains(&dpi) { dpi } else { 96 }
}

fn scale_logical_size(size: i32, dpi: u32) -> i32 {
    ((i64::from(size) * i64::from(valid_dpi(dpi)) + 48) / 96).clamp(1, i64::from(i32::MAX)) as i32
}

pub(crate) fn minimum_size(work: Rect, dpi: u32) -> (i32, i32) {
    (
        scale_logical_size(MIN_WIDTH, dpi).min(work.width().max(1)),
        scale_logical_size(MIN_HEIGHT, dpi).min(work.height().max(1)),
    )
}

pub(crate) fn decode_state(bytes: &[u8]) -> Option<WindowState> {
    if bytes.len() > MAX_STATE_BYTES {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if lines.next()? != STATE_MAGIC || lines.next()?.parse::<u32>().ok()? != STATE_VERSION {
        return None;
    }
    let left = lines.next()?.parse().ok()?;
    let top = lines.next()?.parse().ok()?;
    let right = lines.next()?.parse().ok()?;
    let bottom = lines.next()?.parse().ok()?;
    let dpi = lines.next()?.parse::<u32>().ok()?;
    let maximized = match lines.next()? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    if lines.next().is_some() || !(48..=960).contains(&dpi) {
        return None;
    }
    let normal = Rect {
        left,
        top,
        right,
        bottom,
    };
    if normal.width() < 1
        || normal.height() < 1
        || normal.width() > 100_000
        || normal.height() > 100_000
    {
        return None;
    }
    Some(WindowState {
        normal,
        dpi,
        maximized,
    })
}

pub(crate) fn encode_state(state: WindowState) -> Vec<u8> {
    format!(
        "{STATE_MAGIC}\n{STATE_VERSION}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        state.normal.left,
        state.normal.top,
        state.normal.right,
        state.normal.bottom,
        valid_dpi(state.dpi),
        u8::from(state.maximized)
    )
    .into_bytes()
}

pub(crate) fn load_state(path: &Path) -> io::Result<Option<WindowState>> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::with_capacity(MAX_STATE_BYTES + 1);
    file.take((MAX_STATE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(decode_state(&bytes))
}

pub(crate) fn restore_state(
    saved: Option<WindowState>,
    monitors: &[MonitorArea],
    fallback_dpi: u32,
) -> WindowState {
    let fallback = monitors.first().copied().unwrap_or(MonitorArea {
        monitor: Rect {
            left: 0,
            top: 0,
            right: 1024,
            bottom: 768,
        },
        work: Rect {
            left: 0,
            top: 0,
            right: 1024,
            bottom: 768,
        },
        dpi: valid_dpi(fallback_dpi),
    });
    let Some(mut state) = saved else {
        return WindowState::default_for(fallback.work, fallback.dpi);
    };
    let (cx, cy) = state.normal.center();
    let chosen = monitors
        .iter()
        .copied()
        .find(|m| {
            cx >= i64::from(m.monitor.left)
                && cx < i64::from(m.monitor.right)
                && cy >= i64::from(m.monitor.top)
                && cy < i64::from(m.monitor.bottom)
        })
        .or_else(|| {
            monitors
                .iter()
                .copied()
                .min_by_key(|m| distance_to_rect(cx, cy, m.monitor))
        })
        .unwrap_or(fallback);
    let old_dpi = valid_dpi(state.dpi);
    let new_dpi = valid_dpi(chosen.dpi);
    let scale = |v: i32| -> i32 {
        ((i64::from(v) * i64::from(new_dpi) + i64::from(old_dpi) / 2) / i64::from(old_dpi))
            .clamp(1, i64::from(i32::MAX)) as i32
    };
    let work = chosen.work;
    let work_w = work.width().max(1);
    let work_h = work.height().max(1);
    let (min_w, min_h) = minimum_size(work, new_dpi);
    let width = scale(state.normal.width()).clamp(min_w, work_w);
    let height = scale(state.normal.height()).clamp(min_h, work_h);
    let left = state
        .normal
        .left
        .clamp(work.left, work.right.saturating_sub(width));
    let top = state
        .normal
        .top
        .clamp(work.top, work.bottom.saturating_sub(height));
    state.normal = Rect {
        left,
        top,
        right: left.saturating_add(width),
        bottom: top.saturating_add(height),
    };
    state.dpi = new_dpi;
    state
}

fn distance_to_rect(x: i64, y: i64, rect: Rect) -> u128 {
    let dx = if x < i64::from(rect.left) {
        i64::from(rect.left) - x
    } else if x > i64::from(rect.right) {
        x - i64::from(rect.right)
    } else {
        0
    };
    let dy = if y < i64::from(rect.top) {
        i64::from(rect.top) - y
    } else if y > i64::from(rect.bottom) {
        y - i64::from(rect.bottom)
    } else {
        0
    };
    (dx as u128) * (dx as u128) + (dy as u128) * (dy as u128)
}

#[cfg(windows)]
#[path = "win32.rs"]
mod win32;

#[cfg(windows)]
pub use win32::{
    bridge_self_check, initialize_dpi, resources_self_check, run, run_with_launch, self_check,
    show_error, webview_self_check,
};

#[cfg(not(windows))]
pub fn initialize_dpi() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Win32 window is only available on Windows",
    ))
}
#[cfg(not(windows))]
pub fn run(_state_file: &Path, _title: &str, _webview_data: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Win32 window is only available on Windows",
    ))
}
#[cfg(not(windows))]
pub fn webview_self_check(
    _state_file: &Path,
    _webview_data: &Path,
    _cycles: usize,
) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "WebView2 self-check is only available on Windows",
    ))
}
#[cfg(not(windows))]
pub fn bridge_self_check(_state_file: &Path, _cache: &Path) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "WebView2 bridge self-check is only available on Windows",
    ))
}
#[cfg(not(windows))]
pub fn resources_self_check(_state_file: &Path, _cache: &Path) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "WebView2 resource self-check is only available on Windows",
    ))
}
#[cfg(not(windows))]
pub fn self_check(_state_file: &Path) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Win32 self-check is only available on Windows",
    ))
}
#[cfg(not(windows))]
pub fn show_error(_message: &str) {}

#[cfg(test)]
mod tests {
    use super::*;
    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }
    #[test]
    fn state_round_trip_and_hostile_state_falls_back() {
        let state = WindowState {
            normal: rect(-1200, 25, -400, 625),
            dpi: 144,
            maximized: true,
        };
        assert_eq!(decode_state(&encode_state(state)), Some(state));
        assert_eq!(decode_state(&[b'X'; MAX_STATE_BYTES + 1]), None);
        assert_eq!(
            decode_state(b"ATELIER-WINDOW\n1\n0\n0\n999999\n2\n96\n0\n"),
            None
        );
        assert_eq!(
            decode_state(b"ATELIER-WINDOW\n2\n0\n0\n1\n1\n96\n0\n"),
            None
        );
    }
    #[test]
    fn restores_negative_and_removed_monitor_geometry_at_target_dpi() {
        let monitors = [
            MonitorArea {
                monitor: rect(0, 0, 1920, 1080),
                work: rect(0, 40, 1920, 1040),
                dpi: 96,
            },
            MonitorArea {
                monitor: rect(-1280, -200, 0, 824),
                work: rect(-1280, -160, 0, 824),
                dpi: 144,
            },
        ];
        let saved = WindowState {
            normal: rect(-1200, -100, -400, 500),
            dpi: 96,
            maximized: true,
        };
        let restored = restore_state(Some(saved), &monitors, 96);
        assert_eq!(restored.dpi, 144);
        assert!(restored.normal.left >= -1280 && restored.normal.top >= -160);
        assert!(restored.normal.right <= 0 && restored.normal.bottom <= 824);
        assert!(restored.normal.width() >= MIN_WIDTH.min(1280));
        let removed = WindowState {
            normal: rect(4000, 4000, 5000, 4800),
            dpi: 96,
            maximized: false,
        };
        let nearest = restore_state(Some(removed), &monitors, 96);
        assert!(nearest.normal.right <= 1920 && nearest.normal.bottom <= 1040);
    }
    #[test]
    fn minimum_size_shrinks_to_fit_a_small_work_area() {
        let area = MonitorArea {
            monitor: rect(-100, -50, 300, 250),
            work: rect(-100, -40, 300, 250),
            dpi: 96,
        };
        let restored = restore_state(None, &[area], 96);
        assert!(restored.normal.width() <= 400 && restored.normal.height() <= 290);
        assert!(restored.normal.left >= -100 && restored.normal.top >= -40);
    }

    #[test]
    fn scales_default_and_minimum_geometry_at_192_dpi() {
        let work = rect(-200, -100, 2200, 1400);
        let default = WindowState::default_for(work, 192);
        assert_eq!(
            (default.normal.width(), default.normal.height()),
            (2048, 1440)
        );
        assert_eq!(minimum_size(work, 192), (1280, 840));

        let saved = WindowState {
            normal: rect(-100, 25, 300, 425),
            dpi: 96,
            maximized: false,
        };
        let screen = MonitorArea {
            monitor: work,
            work,
            dpi: 192,
        };
        let restored = restore_state(Some(saved), &[screen], 192);
        assert_eq!(
            restored.normal.left, saved.normal.left,
            "saved physical x stays unchanged"
        );
        assert_eq!(
            restored.normal.top, saved.normal.top,
            "saved physical y stays unchanged"
        );
        assert_eq!(
            (restored.normal.width(), restored.normal.height()),
            (1280, 840)
        );

        let small = Rect {
            left: 0,
            top: 0,
            right: 900,
            bottom: 600,
        };
        let (min_width, min_height) = minimum_size(small, 192);
        assert_eq!((min_width, min_height), (900, 600));
        let small_default = WindowState::default_for(small, 192);
        assert_eq!(
            (small_default.normal.width(), small_default.normal.height()),
            (900, 600)
        );
        let small_screen = MonitorArea {
            monitor: small,
            work: small,
            dpi: 192,
        };
        let constrained = restore_state(Some(saved), &[small_screen], 192);
        assert!(constrained.normal.width() <= small.width());
        assert!(constrained.normal.height() <= small.height());
    }

    #[cfg(windows)]
    #[test]
    fn hidden_window_self_check_runs_beneath_target() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace directory");
        let path = root.join("target").join(format!(
            "us110-window-self-check-{}.state",
            std::process::id()
        ));
        initialize_dpi().expect("enable per-monitor DPI awareness");
        let result = self_check(&path).expect("hidden Win32 window lifecycle");
        assert!(result.contains("DPI"));
        let _ = std::fs::remove_file(path);
    }
}
