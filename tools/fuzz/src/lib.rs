// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
//! Home-made mutational fuzzer for the Atelier analyzers.
//!
//! There is no coverage instrumentation and no third-party tool: inputs are
//! mutated with a seeded generator, run on a watched worker thread, and kept
//! when their [`Outcome`] signal is new. See `docs/us-004-fuzzer.md`.

pub mod campaign;
pub mod mutate;
pub mod rng;
pub mod runner;
pub mod target;
pub mod targets;

pub use campaign::{Config, Found, Summary, fuzz, minimize};
pub use runner::{Failure, FailureKind, Runner};
pub use target::{FnTarget, FuzzTarget, Outcome};
pub use targets::{find, registry};
