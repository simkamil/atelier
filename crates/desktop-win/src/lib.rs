// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native shell foundations. Platform FFI is isolated in dedicated modules.

pub mod data;
pub mod window;

#[cfg(windows)]
pub(crate) mod webview;
