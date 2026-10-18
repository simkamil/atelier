// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native shell foundations. Platform FFI is isolated in dedicated modules.

pub mod data;
pub mod files;
pub mod fonts;
pub mod instance;
pub mod resources;
pub mod url_open;
pub mod window;

#[cfg(windows)]
pub mod file_dialog;

#[cfg(windows)]
pub mod instance_win32;

#[cfg(windows)]
mod bridge_ops;

#[cfg(windows)]
#[path = "file-bridge.rs"]
mod file_bridge;

#[cfg(windows)]
pub(crate) mod webview;

#[cfg(test)]
mod build_date;
