// SPDX-License-Identifier: AGPL-3.0-or-later
//! The fuzz target interface and the outcome used as a coverage substitute.

use crate::rng::fnv1a;
use std::fmt::Debug;

/// What a target reports after consuming one input without panicking.
///
/// There is no code-coverage instrumentation. The fuzzer keeps an input in its
/// working corpus when `(class, size bucket)` has not been seen before:
/// `class` identifies the accepted/rejected path (the error variant and its
/// static reason, digits removed) and the bucket is the base-2 logarithm of
/// `output_size`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Outcome {
    pub class: u64,
    /// Size in bytes of what the analyzer produced (decoded pixels, inflated
    /// bytes, text, event count, ...). Also compared with the memory budget.
    pub output_size: u64,
}

impl Outcome {
    /// The analyzer accepted the input.
    pub fn accepted(output_size: u64) -> Self {
        Self {
            class: 0,
            output_size,
        }
    }

    /// The analyzer rejected the input with the given error. Pass the error's
    /// `Debug` form; numbers are dropped so that sizes do not create classes.
    pub fn rejected(error: &dyn Debug) -> Self {
        Self::rejected_with(error, 0)
    }

    /// Rejection that still produced `output_size` bytes (partial results).
    pub fn rejected_with(error: &dyn Debug, output_size: u64) -> Self {
        let text: String = format!("{error:?}")
            .chars()
            .filter(|c| !c.is_ascii_digit())
            .collect();
        Self {
            class: fnv1a(text.as_bytes()) | 1,
            output_size,
        }
    }

    /// A free-form class label, for targets with several result kinds.
    pub fn labelled(label: &str, output_size: u64) -> Self {
        Self {
            class: fnv1a(label.as_bytes()) | 1,
            output_size,
        }
    }

    /// The corpus-retention signal.
    pub fn signal(&self) -> (u64, u32) {
        (self.class, u64::BITS - self.output_size.leading_zeros())
    }
}

/// A function under test. Implementations must be deterministic and must not
/// keep state between calls, so that replay reproduces a failure.
pub trait FuzzTarget: Send + Sync {
    fn name(&self) -> &str;

    /// Run the analyzer on `input` with its default limits.
    fn run(&self, input: &[u8]) -> Outcome;

    /// Tokens inserted by the dictionary mutation.
    fn dictionary(&self) -> &[&'static [u8]] {
        &[]
    }

    /// Built-in seed inputs (small, valid or near-valid files).
    fn seeds(&self) -> Vec<Vec<u8>> {
        Vec::new()
    }
}

/// A target made of plain functions: adding one takes a few lines.
///
/// ```text
/// FnTarget::new("xml", run_xml).dictionary(XML_TOKENS).seeds(xml_seeds)
/// ```
pub struct FnTarget {
    name: &'static str,
    run: fn(&[u8]) -> Outcome,
    dictionary: &'static [&'static [u8]],
    seeds: fn() -> Vec<Vec<u8>>,
}

impl FnTarget {
    pub fn new(name: &'static str, run: fn(&[u8]) -> Outcome) -> Self {
        Self {
            name,
            run,
            dictionary: &[],
            seeds: Vec::new,
        }
    }

    pub fn dictionary(mut self, tokens: &'static [&'static [u8]]) -> Self {
        self.dictionary = tokens;
        self
    }

    pub fn seeds(mut self, seeds: fn() -> Vec<Vec<u8>>) -> Self {
        self.seeds = seeds;
        self
    }
}

impl FuzzTarget for FnTarget {
    fn name(&self) -> &str {
        self.name
    }
    fn run(&self, input: &[u8]) -> Outcome {
        (self.run)(input)
    }
    fn dictionary(&self) -> &[&'static [u8]] {
        self.dictionary
    }
    fn seeds(&self) -> Vec<Vec<u8>> {
        (self.seeds)()
    }
}
