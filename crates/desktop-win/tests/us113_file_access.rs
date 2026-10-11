// SPDX-License-Identifier: AGPL-3.0-or-later
//! Focused US-113 integration test entry point.

#[cfg(windows)]
use atelier_desktop_win::file_dialog;

#[cfg(windows)]
#[test]
fn native_dialog_cancel_is_not_reported_as_a_system_error() {
    assert!(file_dialog::is_dialog_cancelled_hresult(
        0x8007_04C7u32 as i32
    ));
    assert!(!file_dialog::is_dialog_cancelled_hresult(
        0x8007_0005u32 as i32
    ));
}
