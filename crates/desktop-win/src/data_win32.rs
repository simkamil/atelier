// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native startup dialogs for portable data resolution.

#[cfg(windows)]
use super::DataError;
use super::StartupChoice;
#[cfg(windows)]
use crate::i18n::t;

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::ptr;

#[cfg(windows)]
const MB_OK: u32 = 0x0000;
#[cfg(windows)]
const MB_YESNOCANCEL: u32 = 0x0003;
#[cfg(windows)]
const MB_ICONERROR: u32 = 0x0010;
#[cfg(windows)]
const MB_ICONWARNING: u32 = 0x0030;
#[cfg(windows)]
const MB_SETFOREGROUND: u32 = 0x10000;
const IDYES: i32 = 6;
const IDNO: i32 = 7;
const IDCANCEL: i32 = 2;

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(
        owner: *mut core::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        kind: u32,
    ) -> i32;
}

/// Ask the user to choose Local, Move, or Quit after portable storage fails.
#[cfg(windows)]
pub fn choose_data_mode(error: &DataError) -> StartupChoice {
    let text = t(
        "native.startup.storage.prompt",
        &[("error", &error.to_string())],
    );
    let response = message_box(
        &text,
        &t("native.startup.storage.caption", &[]),
        MB_YESNOCANCEL | MB_ICONWARNING,
    );
    choice_from_message_box(response)
}

/// Display a final startup error after the selected storage location also fails.
#[cfg(windows)]
pub fn show_startup_error(error: &DataError) {
    let text = t(
        "native.startup.error.text",
        &[("error", &error.to_string())],
    );
    let caption = t("native.startup.error.caption", &[]);
    let _ = message_box(&text, &caption, MB_OK | MB_ICONERROR);
}

/// Explain that moving the executable is a manual action, then exit the current launch.
#[cfg(windows)]
pub fn show_move_instructions() {
    let text = t("native.startup.move.text", &[]);
    let caption = t("native.startup.move.caption", &[]);
    let _ = message_box(&text, &caption, MB_OK | MB_ICONWARNING);
}

#[cfg(windows)]
fn message_box(text: &str, caption: &str, kind: u32) -> i32 {
    let text = wide_null(text);
    let caption = wide_null(caption);
    // SAFETY: both pointers refer to live, NUL-terminated UTF-16 buffers for the duration of
    // MessageBoxW. A null owner is valid for a startup dialog shown before window creation.
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            kind | MB_SETFOREGROUND,
        )
    }
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

/// Map the native response without showing a dialog; unknown responses fail closed to Quit.
pub(crate) fn choice_from_message_box(response: i32) -> StartupChoice {
    match response {
        IDYES => StartupChoice::Local,
        IDNO => StartupChoice::Move,
        IDCANCEL => StartupChoice::Quit,
        _ => StartupChoice::Quit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_box_responses_match_the_french_prompt() {
        assert_eq!(choice_from_message_box(IDYES), StartupChoice::Local);
        assert_eq!(choice_from_message_box(IDNO), StartupChoice::Move);
        assert_eq!(choice_from_message_box(IDCANCEL), StartupChoice::Quit);
        assert_eq!(choice_from_message_box(-1), StartupChoice::Quit);
    }
}
