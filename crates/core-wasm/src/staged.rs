// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cooperative, resumable engine operations (US-092).
//!
//! A WASM call cannot be interrupted, so a long operation is split into
//! bounded steps. The host begins a job, calls `Step` repeatedly (yielding to
//! its task queue between calls, where it can notice an abort) and sends
//! `Cancel` to drop the job. Running every step gives exactly the bytes of the
//! one-shot operation. Jobs live outside `EditorSession`, so neither a step nor
//! a cancellation can change the editing session.
//!
//! Commands are `Editor` payloads (`ATED`, version 1) with these opcodes:
//! * 23 `Begin`: `u16 kind` (1 open DOCX preview, 2 export DOCX), `u16 0`, DOCX bytes.
//! * 24 `Step`: `u64 job id`.
//! * 25 `Cancel`: `u64 job id`; cancelling an unknown job is a no-op.
//!
//! Progress responses are `ATJB`: `u16 1`, `u16 state` (0 running, 1
//! cancelled), `u32 done`, `u32 total` (always 1000), `u64 job id`. The final
//! `Step` response of an open is the `ATRP` preview of opcode 18; of an export
//! it is `ATSV`: `u16 1`, `u16 0`, `u32 notice count`, DOCX bytes.

use crate::editor::{EditorError, Writer, encode_preview};
use crate::protocol::{Message, MessageType};
use crate::rich_preview::{PROGRESS_TOTAL, RenderJob, StepBudget, import_preview_docx};
use atelier_fmt_ooxml::{DocxWriteOptions, ImportedDocx, write_docx};

pub const OP_BEGIN: u16 = 23;
pub const OP_STEP: u16 = 24;
pub const OP_CANCEL: u16 = 25;
pub const KIND_OPEN: u16 = 1;
pub const KIND_EXPORT: u16 = 2;

const MAX_INPUT: usize = 8 * 1024 * 1024;
const MAX_RESPONSE: usize = 32 * 1024 * 1024;
/// Work per step: small enough that the host regains control quickly.
pub const STEP_BUDGET: StepBudget = StepBudget {
    paragraphs: 8,
    placements: 2_048,
    pages: 1,
};

enum Stage {
    Import,
    Render(RenderJob),
    Write,
    Verify { bytes: Vec<u8>, notices: u32 },
    Finished,
}

struct Job {
    id: u64,
    kind: u16,
    input: Vec<u8>,
    imported: Option<ImportedDocx>,
    stage: Stage,
    progress: u32,
}

/// The single staged job slot of one engine instance.
#[derive(Default)]
pub struct StagedEngine {
    job: Option<Job>,
    next_id: u64,
}

/// True when `payload` is a staged-operation command.
pub fn is_staged(payload: &[u8]) -> bool {
    payload.len() >= 8
        && &payload[..4] == b"ATED"
        && matches!(
            u16::from_le_bytes([payload[6], payload[7]]),
            OP_BEGIN | OP_STEP | OP_CANCEL
        )
}

fn progress_frame(state: u16, done: u32, id: u64) -> Result<Vec<u8>, EditorError> {
    let mut out = Writer::with_capacity(24)?;
    out.bytes(b"ATJB");
    out.u16(1);
    out.u16(state);
    out.u32(done);
    out.u32(PROGRESS_TOTAL);
    out.u64(id);
    out.finish(24)
}

fn job_id(body: &[u8]) -> Result<u64, EditorError> {
    <[u8; 8]>::try_from(body)
        .map(u64::from_le_bytes)
        .map_err(|_| EditorError("EDITOR_INVALID_COMMAND"))
}

impl StagedEngine {
    /// Answers one staged `Editor` command message.
    pub fn respond(&mut self, message: &Message) -> Message {
        let (kind, payload) = match self.transact(&message.payload) {
            Ok(payload) => (MessageType::EditorResult, payload),
            Err(error) => (MessageType::Failed, error.0.as_bytes().to_vec()),
        };
        Message {
            kind,
            request_id: message.request_id,
            payload,
        }
    }

    /// True while a job holds engine state.
    pub fn has_job(&self) -> bool {
        self.job.is_some()
    }

    fn transact(&mut self, payload: &[u8]) -> Result<Vec<u8>, EditorError> {
        if !is_staged(payload) || u16::from_le_bytes([payload[4], payload[5]]) != 1 {
            return Err(EditorError("EDITOR_INVALID_COMMAND"));
        }
        let opcode = u16::from_le_bytes([payload[6], payload[7]]);
        let body = &payload[8..];
        match opcode {
            OP_BEGIN => self.begin(body),
            OP_STEP => self.step(job_id(body)?),
            _ => {
                let id = job_id(body)?;
                if self.job.as_ref().is_some_and(|job| job.id == id) {
                    self.job = None;
                }
                progress_frame(1, 0, id)
            }
        }
    }

    fn begin(&mut self, body: &[u8]) -> Result<Vec<u8>, EditorError> {
        // A new job always replaces (and so frees) any abandoned one.
        self.job = None;
        if body.len() < 4 || body.len() - 4 > MAX_INPUT || body[2] != 0 || body[3] != 0 {
            return Err(EditorError("EDITOR_INVALID_COMMAND"));
        }
        let kind = u16::from_le_bytes([body[0], body[1]]);
        if kind != KIND_OPEN && kind != KIND_EXPORT {
            return Err(EditorError("EDITOR_INVALID_COMMAND"));
        }
        let mut input = Vec::new();
        input
            .try_reserve_exact(body.len() - 4)
            .map_err(|_| EditorError("EDITOR_LIMIT"))?;
        input.extend_from_slice(&body[4..]);
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        self.job = Some(Job {
            id,
            kind,
            input,
            imported: None,
            stage: Stage::Import,
            progress: 0,
        });
        progress_frame(0, 0, id)
    }

    fn step(&mut self, id: u64) -> Result<Vec<u8>, EditorError> {
        let Some(job) = self.job.as_mut().filter(|job| job.id == id) else {
            return Err(EditorError("EDITOR_JOB_UNKNOWN"));
        };
        match advance(job) {
            Ok(None) => progress_frame(0, job.progress, id),
            Ok(Some(done)) => {
                self.job = None;
                Ok(done)
            }
            Err(error) => {
                self.job = None;
                Err(error)
            }
        }
    }
}

fn advance(job: &mut Job) -> Result<Option<Vec<u8>>, EditorError> {
    let failure = if job.kind == KIND_OPEN {
        EditorError("EDITOR_PREVIEW_FAILED")
    } else {
        EditorError("EDITOR_EXPORT_FAILED")
    };
    match std::mem::replace(&mut job.stage, Stage::Finished) {
        Stage::Import => {
            let imported = import_preview_docx(&job.input).map_err(|_| failure)?;
            job.input = Vec::new();
            if job.kind == KIND_OPEN {
                let render = RenderJob::new(&imported).map_err(|_| failure)?;
                job.progress = render.progress();
                job.stage = Stage::Render(render);
            } else {
                job.progress = 300;
                job.stage = Stage::Write;
            }
            job.imported = Some(imported);
            Ok(None)
        }
        Stage::Render(mut render) => {
            let imported = job.imported.as_mut().ok_or(failure)?;
            match render.step(imported, STEP_BUDGET).map_err(|_| failure)? {
                Some(preview) => encode_preview(preview).map(Some),
                None => {
                    job.progress = render.progress().max(job.progress);
                    job.stage = Stage::Render(render);
                    Ok(None)
                }
            }
        }
        Stage::Write => {
            let imported = job.imported.take().ok_or(failure)?;
            let written =
                write_docx(&imported, &DocxWriteOptions::default()).map_err(|_| failure)?;
            let notices = u32::try_from(written.notices.len()).unwrap_or(u32::MAX);
            job.progress = 700;
            job.stage = Stage::Verify {
                bytes: written.bytes,
                notices,
            };
            Ok(None)
        }
        Stage::Verify { bytes, notices } => {
            import_preview_docx(&bytes).map_err(|_| failure)?;
            let size = 16usize
                .checked_add(bytes.len())
                .filter(|size| *size <= MAX_RESPONSE)
                .ok_or(EditorError("EDITOR_RESPONSE_LIMIT"))?;
            let mut out = Writer::with_capacity(size)?;
            out.bytes(b"ATSV");
            out.u16(1);
            out.u16(0);
            out.u32(notices);
            out.bytes(&bytes);
            out.finish(MAX_RESPONSE).map(Some)
        }
        Stage::Finished => Err(failure),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE: &[u8] = include_bytes!("../../../tests/docx-reference/basic.docx");

    fn command(opcode: u16, body: &[u8]) -> Message {
        let mut payload = b"ATED".to_vec();
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&opcode.to_le_bytes());
        payload.extend_from_slice(body);
        Message {
            kind: MessageType::Editor,
            request_id: 7,
            payload,
        }
    }

    fn begin(engine: &mut StagedEngine, kind: u16, bytes: &[u8]) -> u64 {
        let mut body = kind.to_le_bytes().to_vec();
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(bytes);
        let reply = engine.respond(&command(OP_BEGIN, &body));
        assert_eq!(reply.kind, MessageType::EditorResult);
        assert_eq!(&reply.payload[..4], b"ATJB");
        u64::from_le_bytes(reply.payload[16..24].try_into().unwrap())
    }

    /// Runs steps to the end; returns progress values and the final payload.
    fn run(engine: &mut StagedEngine, id: u64) -> (Vec<u32>, Vec<u8>) {
        let mut seen = Vec::new();
        loop {
            let reply = engine.respond(&command(OP_STEP, &id.to_le_bytes()));
            assert_eq!(reply.kind, MessageType::EditorResult);
            if &reply.payload[..4] != b"ATJB" {
                return (seen, reply.payload);
            }
            seen.push(u32::from_le_bytes(reply.payload[8..12].try_into().unwrap()));
        }
    }

    #[test]
    fn staged_open_equals_one_shot_with_monotonic_progress() {
        let mut engine = StagedEngine::default();
        let id = begin(&mut engine, KIND_OPEN, REFERENCE);
        let (seen, payload) = run(&mut engine, id);
        assert!(seen.len() >= 2, "{seen:?}");
        assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]), "{seen:?}");
        assert!(seen.iter().all(|value| *value <= PROGRESS_TOTAL));
        let one_shot =
            encode_preview(crate::rich_preview::preview_docx(REFERENCE).unwrap()).unwrap();
        assert_eq!(payload, one_shot);
        assert!(!engine.has_job());
    }

    #[test]
    fn small_budgets_match_unlimited_preview() {
        let mut imported = import_preview_docx(REFERENCE).unwrap();
        let mut job = RenderJob::new(&imported).unwrap();
        let budget = StepBudget {
            paragraphs: 1,
            placements: 7,
            pages: 1,
        };
        let mut steps = 0;
        let stepped = loop {
            steps += 1;
            if let Some(preview) = job.step(&mut imported, budget).unwrap() {
                break preview;
            }
        };
        assert!(steps > 3);
        let one_shot = crate::rich_preview::preview_docx(REFERENCE).unwrap();
        assert_eq!(stepped, one_shot);
    }

    #[test]
    fn staged_export_is_a_valid_docx_and_deterministic() {
        let mut engine = StagedEngine::default();
        let id = begin(&mut engine, KIND_EXPORT, REFERENCE);
        let (seen, payload) = run(&mut engine, id);
        assert_eq!(seen, vec![300, 700]);
        assert_eq!(&payload[..4], b"ATSV");
        let direct = write_docx(
            &import_preview_docx(REFERENCE).unwrap(),
            &DocxWriteOptions::default(),
        )
        .unwrap();
        assert_eq!(&payload[12..], direct.bytes.as_slice());
        let again = begin(&mut engine, KIND_EXPORT, REFERENCE);
        assert_eq!(run(&mut engine, again).1, payload);
        import_preview_docx(&payload[12..]).expect("exported bytes reopen");
    }

    #[test]
    fn cancel_frees_the_job_and_the_engine_stays_usable() {
        let mut engine = StagedEngine::default();
        let id = begin(&mut engine, KIND_OPEN, REFERENCE);
        let first = engine.respond(&command(OP_STEP, &id.to_le_bytes()));
        assert_eq!(&first.payload[..4], b"ATJB");
        assert!(engine.has_job());
        let cancelled = engine.respond(&command(OP_CANCEL, &id.to_le_bytes()));
        assert_eq!(cancelled.kind, MessageType::EditorResult);
        assert_eq!(
            u16::from_le_bytes([cancelled.payload[6], cancelled.payload[7]]),
            1
        );
        assert!(!engine.has_job());
        let stale = engine.respond(&command(OP_STEP, &id.to_le_bytes()));
        assert_eq!(stale.kind, MessageType::Failed);
        assert_eq!(stale.payload, b"EDITOR_JOB_UNKNOWN");
        // Cancelling twice is harmless, and a new job runs to completion.
        assert_eq!(
            engine.respond(&command(OP_CANCEL, &id.to_le_bytes())).kind,
            MessageType::EditorResult
        );
        let next = begin(&mut engine, KIND_OPEN, REFERENCE);
        assert_ne!(next, id);
        assert_eq!(&run(&mut engine, next).1[..4], b"ATRP");
    }

    #[test]
    fn begin_replaces_abandoned_job_and_bad_input_fails_cleanly() {
        let mut engine = StagedEngine::default();
        let old = begin(&mut engine, KIND_OPEN, REFERENCE);
        let fresh = begin(&mut engine, KIND_OPEN, REFERENCE);
        let stale = engine.respond(&command(OP_STEP, &old.to_le_bytes()));
        assert_eq!(stale.payload, b"EDITOR_JOB_UNKNOWN");
        assert!(engine.has_job());
        engine.respond(&command(OP_CANCEL, &fresh.to_le_bytes()));
        let bad = begin(&mut engine, KIND_OPEN, &[1, 2, 3]);
        let failed = engine.respond(&command(OP_STEP, &bad.to_le_bytes()));
        assert_eq!(failed.kind, MessageType::Failed);
        assert_eq!(failed.payload, b"EDITOR_PREVIEW_FAILED");
        assert!(!engine.has_job(), "a failed job frees its state");
        for body in [&b"\x09\x00\x00\x00"[..], b"\x01\x00\x01\x00", b"\x01"] {
            assert_eq!(
                engine.respond(&command(OP_BEGIN, body)).kind,
                MessageType::Failed
            );
        }
        assert_eq!(
            engine.respond(&command(OP_STEP, &[1, 2])).kind,
            MessageType::Failed
        );
    }
}
