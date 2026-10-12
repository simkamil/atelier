// SPDX-License-Identifier: AGPL-3.0-or-later
//! Executes a target on a watched worker thread and classifies failures.

use crate::target::{FuzzTarget, Outcome};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Once};
use std::thread;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    Panic,
    Timeout,
    Memory,
}

impl FailureKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Panic => "panic",
            Self::Timeout => "timeout",
            Self::Memory => "memory",
        }
    }
}

/// A detected defect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
    /// `file:line:column` of the panic; empty for other kinds.
    pub location: String,
}

impl Failure {
    /// Deduplication key: the panic location, or the kind alone.
    pub fn key(&self) -> String {
        if self.location.is_empty() {
            self.kind.label().to_string()
        } else {
            format!("{}@{}", self.kind.label(), self.location)
        }
    }

    pub fn describe(&self) -> String {
        if self.location.is_empty() {
            format!("{}: {}", self.kind.label(), self.message)
        } else {
            format!(
                "{} at {}: {}",
                self.kind.label(),
                self.location,
                self.message
            )
        }
    }
}

thread_local! {
    static IN_WORKER: Cell<bool> = const { Cell::new(false) };
    static CAPTURED: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

static HOOK: Once = Once::new();

fn payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Install (once) a hook that records panics raised on worker threads and
/// stays silent for them; panics elsewhere reach the previous hook.
fn install_hook() {
    HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if IN_WORKER.with(Cell::get) {
                let location = info
                    .location()
                    .map(|l| {
                        format!(
                            "{}:{}:{}",
                            l.file().replace('\\', "/"),
                            l.line(),
                            l.column()
                        )
                    })
                    .unwrap_or_else(|| "unknown".to_string());
                let message = payload_message(info.payload());
                CAPTURED.with(|c| *c.borrow_mut() = Some((message, location)));
            } else {
                previous(info);
            }
        }));
    });
}

type Reply = Result<Outcome, Failure>;

struct Worker {
    input: Sender<Vec<u8>>,
    output: mpsc::Receiver<Reply>,
}

fn spawn_worker(target: Arc<dyn FuzzTarget>) -> Worker {
    let (input, requests) = mpsc::channel::<Vec<u8>>();
    let (replies, output) = mpsc::channel::<Reply>();
    thread::Builder::new()
        .name(format!("fuzz-{}", target.name()))
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            IN_WORKER.with(|w| w.set(true));
            while let Ok(data) = requests.recv() {
                let result = panic::catch_unwind(AssertUnwindSafe(|| target.run(&data)));
                let reply = result.map_err(|payload| {
                    let (message, location) = CAPTURED
                        .with(|c| c.borrow_mut().take())
                        .unwrap_or_else(|| (payload_message(&*payload), "unknown".to_string()));
                    Failure {
                        kind: FailureKind::Panic,
                        message,
                        location,
                    }
                });
                if replies.send(reply).is_err() {
                    break;
                }
            }
        })
        .expect("cannot start the fuzz worker thread");
    Worker { input, output }
}

/// Runs inputs one at a time on a worker thread, with a per-input deadline.
///
/// A target that does not return before the deadline is reported as a
/// timeout. Its thread cannot be stopped; it is abandoned (it exits if the
/// target ever returns) and a fresh worker serves the next input.
pub struct Runner {
    target: Arc<dyn FuzzTarget>,
    timeout: Duration,
    max_output: u64,
    worker: Option<Worker>,
    abandoned: usize,
}

impl Runner {
    pub fn new(target: Arc<dyn FuzzTarget>, timeout: Duration, max_output: u64) -> Self {
        install_hook();
        Self {
            target,
            timeout,
            max_output,
            worker: None,
            abandoned: 0,
        }
    }

    /// Number of worker threads left running after a timeout.
    pub fn abandoned_workers(&self) -> usize {
        self.abandoned
    }

    pub fn run(&mut self, input: &[u8]) -> Reply {
        let worker = self
            .worker
            .get_or_insert_with(|| spawn_worker(self.target.clone()));
        if worker.input.send(input.to_vec()).is_err() {
            self.worker = None;
            return Err(Failure {
                kind: FailureKind::Panic,
                message: "worker thread terminated unexpectedly".to_string(),
                location: "unknown".to_string(),
            });
        }
        match worker.output.recv_timeout(self.timeout) {
            Ok(Ok(outcome)) => {
                if outcome.output_size > self.max_output {
                    Err(Failure {
                        kind: FailureKind::Memory,
                        message: format!(
                            "reported output of {} bytes exceeds the {} byte budget",
                            outcome.output_size, self.max_output
                        ),
                        location: String::new(),
                    })
                } else {
                    Ok(outcome)
                }
            }
            Ok(Err(failure)) => Err(failure),
            Err(RecvTimeoutError::Timeout) => {
                self.worker = None;
                self.abandoned += 1;
                Err(Failure {
                    kind: FailureKind::Timeout,
                    message: format!("no result after {} ms", self.timeout.as_millis()),
                    location: String::new(),
                })
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.worker = None;
                Err(Failure {
                    kind: FailureKind::Panic,
                    message: "worker thread terminated unexpectedly".to_string(),
                    location: "unknown".to_string(),
                })
            }
        }
    }
}
