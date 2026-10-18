// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sublinear paragraph flow for an editor (US-216).
//!
//! [`PageStream`] keeps the laid-out paragraphs of a document prefix, the
//! pagination of a prefix of those paragraphs, one machine checkpoint per
//! paginated paragraph and one signature per page.
//! [`PageStream::replace`] swaps a run of paragraphs (an edit, or paragraphs
//! appended by the caller as it lays the tail out) and repaginates from the
//! page that holds the change. The run ends as soon as the pagination machine
//! is back in a state the previous pagination also went through (the old
//! suffix is then reused with its indices shifted), or after a bounded number
//! of pages when it does not (the usual case when a line is added, which pushes
//! every later line down); [`PageStream::advance`] paginates the rest in
//! bounded slices. Nothing is validated or paginated outside the pages a run
//! visits; the remaining work is index arithmetic and moves of the cached
//! vectors (linear in paragraphs and pages, no shaping, no layout).
//!
//! Once nothing is pending the result is the one
//! [`crate::pagination::paginate`] gives for the whole document, which the
//! property tests of the crate check after every edit.

use crate::ParagraphLayout;
use crate::pagination::{
    Page, Pagination, PaginationCheckpoint, PaginationDiagnostic, PaginationError,
    PaginationOptions, ParagraphRules, ParagraphSource, PlacedLine, paginate_source,
};

/// One laid-out paragraph handed to the stream, with its pagination rules.
#[derive(Clone, Debug)]
pub struct StreamParagraph {
    pub layout: ParagraphLayout,
    pub rules: ParagraphRules,
}

#[derive(Clone, Debug)]
struct Entry {
    layout: ParagraphLayout,
    rules: ParagraphRules,
    generation: u64,
}

/// Work done by one [`PageStream::replace`], for benches and tests.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StreamStats {
    /// Paragraph index the pagination restarted from.
    pub restart_paragraph: Option<usize>,
    /// Pagination stream work units actually executed.
    pub stream_work: usize,
    /// Pages produced again by the pagination run.
    pub pages_recalculated: usize,
    /// Pages taken over from the previous pagination.
    pub pages_reused: usize,
    /// The run stopped on a stabilised page instead of reaching the end.
    pub stopped_early: bool,
    /// The run gave up on its limit; the tail is paginated by `advance`.
    pub gave_up: bool,
}

/// What one [`PageStream::replace`] changed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StreamUpdate {
    pub old_page_count: usize,
    pub page_count: usize,
    /// Indices `< page_count` whose signature differs from the previous
    /// pagination at the same index (sorted).
    pub changed_pages: Vec<usize>,
    /// Paragraphs covered by the pagination after the update.
    pub paginated_paragraphs: usize,
    /// Every laid-out paragraph is paginated.
    pub complete: bool,
    pub stats: StreamStats,
}

/// Laid-out paragraph prefix with incremental pagination.
#[derive(Clone, Debug)]
pub struct PageStream {
    options: PaginationOptions,
    entries: Vec<Entry>,
    checkpoints: Vec<PaginationCheckpoint>,
    pagination: Pagination,
    page_signatures: Vec<u64>,
    next_generation: u64,
    /// Paragraphs `0..paginated` are covered by `pagination` and
    /// `checkpoints`; the last page of the pagination is then provisional
    /// unless `paginated == paragraph_count()`.
    paginated: usize,
}

/// When a pagination run gives up instead of reaching a stabilised page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunLimit {
    /// Stop once this many pages were produced after the restart page (and
    /// two paragraphs past the edit).
    Pages(usize),
    /// Stop at the first paragraph boundary at or after this paragraph.
    Paragraph(usize),
}

/// View of the entries as they will be once a replacement is committed.
struct SplicedSource<'a> {
    entries: &'a [Entry],
    first: usize,
    removed: usize,
    new: &'a [Entry],
}

impl SplicedSource<'_> {
    fn entry(&self, index: usize) -> &Entry {
        if index < self.first {
            &self.entries[index]
        } else if index < self.first + self.new.len() {
            &self.new[index - self.first]
        } else {
            &self.entries[index - self.new.len() + self.removed]
        }
    }
}

impl ParagraphSource for SplicedSource<'_> {
    fn len(&self) -> usize {
        self.entries.len() - self.removed + self.new.len()
    }
    fn layout(&self, index: usize) -> &ParagraphLayout {
        &self.entry(index).layout
    }
    fn rules(&self, index: usize) -> &ParagraphRules {
        &self.entry(index).rules
    }
}

fn shift(value: usize, delta: i64) -> Result<usize, PaginationError> {
    i64::try_from(value)
        .ok()
        .and_then(|v| v.checked_add(delta))
        .and_then(|v| usize::try_from(v).ok())
        .ok_or(PaginationError::ArithmeticOverflow)
}

fn shift_diagnostic(
    diagnostic: &PaginationDiagnostic,
    delta: i64,
) -> Result<PaginationDiagnostic, PaginationError> {
    use PaginationDiagnostic as D;
    Ok(match *diagnostic {
        D::KeepTogetherRelaxed { paragraph_index } => D::KeepTogetherRelaxed {
            paragraph_index: shift(paragraph_index, delta)?,
        },
        D::KeepWithNextRelaxed { paragraph_index } => D::KeepWithNextRelaxed {
            paragraph_index: shift(paragraph_index, delta)?,
        },
        D::WidowOrphanRelaxed { paragraph_index } => D::WidowOrphanRelaxed {
            paragraph_index: shift(paragraph_index, delta)?,
        },
        D::LineExceedsPage {
            paragraph_index,
            line_index,
        } => D::LineExceedsPage {
            paragraph_index: shift(paragraph_index, delta)?,
            line_index,
        },
        D::KeepChainExceedsPage { paragraph_index } => D::KeepChainExceedsPage {
            paragraph_index: shift(paragraph_index, delta)?,
        },
        D::ParagraphSpacingExceedsPage { paragraph_index } => D::ParagraphSpacingExceedsPage {
            paragraph_index: shift(paragraph_index, delta)?,
        },
    })
}

/// Maps a paragraph of the old numbering to the new one; `None` for a paragraph
/// of the replaced range unless the replacement keeps the paragraph count (then
/// the geometry is compared paragraph by paragraph).
fn map_old(old: usize, first: usize, removed: usize, inserted: usize) -> Option<usize> {
    if old < first {
        Some(old)
    } else if old >= first + removed {
        Some(old - removed + inserted)
    } else if removed == inserted {
        Some(old)
    } else {
        None
    }
}

fn empty_pagination() -> Pagination {
    Pagination {
        pages: Vec::new(),
        placements: Vec::new(),
        diagnostics: Vec::new(),
    }
}

impl PageStream {
    pub fn new(options: PaginationOptions) -> Self {
        Self {
            options,
            entries: Vec::new(),
            checkpoints: Vec::new(),
            pagination: empty_pagination(),
            page_signatures: Vec::new(),
            next_generation: 1,
            paginated: 0,
        }
    }

    pub fn options(&self) -> PaginationOptions {
        self.options
    }

    /// Drops every paragraph (the generation counter keeps counting).
    pub fn clear(&mut self, options: PaginationOptions) {
        self.options = options;
        self.entries.clear();
        self.checkpoints.clear();
        self.pagination = empty_pagination();
        self.page_signatures.clear();
        self.paginated = 0;
    }

    /// Paragraphs covered by the pagination.
    pub fn paginated_paragraphs(&self) -> usize {
        self.paginated
    }

    pub fn paragraph_count(&self) -> usize {
        self.entries.len()
    }
    pub fn page_count(&self) -> usize {
        self.pagination.pages.len()
    }
    pub fn pagination(&self) -> &Pagination {
        &self.pagination
    }
    pub fn layout(&self, paragraph: usize) -> Option<&ParagraphLayout> {
        self.entries.get(paragraph).map(|e| &e.layout)
    }
    pub fn rules(&self, paragraph: usize) -> Option<&ParagraphRules> {
        self.entries.get(paragraph).map(|e| &e.rules)
    }
    /// Identifier of one layout instance; it changes whenever the paragraph is
    /// replaced and never otherwise.
    pub fn generation(&self, paragraph: usize) -> Option<u64> {
        self.entries.get(paragraph).map(|e| e.generation)
    }
    /// Signature of the content of a page (geometry and paragraph generations).
    pub fn page_signature(&self, page: usize) -> Option<u64> {
        self.page_signatures.get(page).copied()
    }
    /// Placements (all pages) of one paragraph, in line order.
    pub fn placements_of(&self, paragraph: usize) -> &[PlacedLine] {
        let all = &self.pagination.placements;
        let start = all.partition_point(|p| p.paragraph_index < paragraph);
        let end = all.partition_point(|p| p.paragraph_index <= paragraph);
        &all[start..end]
    }
    /// Placements of one page.
    pub fn page_placements(&self, page: usize) -> &[PlacedLine] {
        self.pagination
            .pages
            .get(page)
            .and_then(|p| self.pagination.placements.get(p.placements.clone()))
            .unwrap_or(&[])
    }

    fn signature(spliced: &SplicedSource<'_>, placements: &[PlacedLine]) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        let mut mix = |value: u64| {
            for byte in value.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        };
        mix(placements.len() as u64);
        for placed in placements {
            mix(spliced.entry(placed.paragraph_index).generation);
            mix(placed.line_index as u64);
            mix(placed.top as u64);
            mix(placed.baseline as u64);
        }
        hash
    }

    /// First paragraph to run again when paragraph `base` changes: the
    /// paragraph before the one that opens its page, rewound across
    /// keep-with-next chains.
    fn restart_index(&self, base: usize) -> usize {
        let placements = &self.pagination.placements;
        let page = placements
            .get(placements.partition_point(|p| p.paragraph_index < base))
            .filter(|p| p.paragraph_index == base)
            .map(|p| p.page_index);
        let first_on_page = page
            .and_then(|page| self.page_placements(page).first())
            .map_or(base, |p| p.paragraph_index);
        let mut restart = first_on_page.saturating_sub(1);
        while restart > 0 && self.entries[restart - 1].rules.keep_with_next {
            restart -= 1;
        }
        restart.min(self.paginated.saturating_sub(1))
    }

    /// Replaces `removed` paragraphs at `first` by `new` and repaginates until
    /// the pagination stabilises or `limit` is reached. `first + removed` must
    /// not exceed the paragraph count; `first` equal to the count appends.
    /// Atomic: on error nothing changes.
    pub fn replace(
        &mut self,
        first: usize,
        removed: usize,
        new: Vec<StreamParagraph>,
        limit: RunLimit,
    ) -> Result<StreamUpdate, PaginationError> {
        let old_len = self.entries.len();
        if first > old_len || removed > old_len - first {
            return Err(PaginationError::InvalidInput(
                "replaced range is outside the stream",
            ));
        }
        let inserted = new.len();
        let delta = inserted as i64 - removed as i64;
        let mut generation = self.next_generation;
        let new_entries: Vec<Entry> = new
            .into_iter()
            .map(|p| {
                generation += 1;
                Entry {
                    layout: p.layout,
                    rules: p.rules,
                    generation: generation - 1,
                }
            })
            .collect();
        let spliced = SplicedSource {
            entries: &self.entries,
            first,
            removed,
            new: &new_entries,
        };
        let new_len = spliced.len();
        let paginated_old = self.paginated;
        let restart = (paginated_old > 0).then(|| self.restart_index(first.min(paginated_old)));
        let seed_checkpoint = restart.map(|r| self.checkpoints[r].clone());
        let restart_page = seed_checkpoint.as_ref().map_or(0, |c| c.page_count);
        let boundary_old = first + removed;
        let old_checkpoints = &self.checkpoints;
        let old_pagination = &self.pagination;
        // The machine is where the previous pagination was, `inserted - removed`
        // paragraphs earlier: the old suffix can be reused.
        let stable = |candidate: &PaginationCheckpoint, current: &Pagination| -> bool {
            let s = candidate.source_index;
            if s < first + inserted || s >= new_len {
                return false;
            }
            let old_index = s - inserted + removed;
            let Some(old) = old_checkpoints.get(old_index) else {
                return false;
            };
            // The gap before a paragraph depends on the rules of the previous
            // one, which the machine state does not hold: it must be a paragraph
            // the edit did not touch.
            let prev_ok = match (candidate.previous_paragraph, old.previous_paragraph) {
                (None, None) => true,
                (Some(n), Some(o)) => {
                    n >= first + inserted && map_old(o, first, removed, inserted) == Some(n)
                }
                _ => false,
            };
            let page_delta = candidate.page_count as i64 - old.page_count as i64;
            prev_ok
                && candidate.page_y == old.page_y
                && candidate.break_after_pending == old.break_after_pending
                && candidate.open_page_placements == old.open_page_placements
                && candidate.keep_chain_height == old.keep_chain_height
                && candidate.has_following_nonempty == old.has_following_nonempty
                && match (
                    current
                        .placements
                        .get(candidate.page_start..candidate.placement_count),
                    old_pagination
                        .placements
                        .get(old.page_start..old.placement_count),
                ) {
                    (Some(n), Some(o)) => {
                        n.len() == o.len()
                            && n.iter().zip(o).all(|(n, o)| {
                                map_old(o.paragraph_index, first, removed, inserted)
                                    == Some(n.paragraph_index)
                                    && n.page_index as i64 == o.page_index as i64 + page_delta
                                    && n.line_index == o.line_index
                                    && n.top == o.top
                                    && n.baseline == o.baseline
                            })
                    }
                    _ => false,
                }
        };
        let restart_at = restart.unwrap_or(0);
        // Nothing changed when only the tail is paginated (`advance`).
        let changed_end = if inserted == 0 && removed == 0 {
            0
        } else {
            first + inserted
        };
        let give_up = |candidate: &PaginationCheckpoint| -> bool {
            // Two paragraphs past the edit at least, so that a page the edit
            // leaves unchanged can still be recognised (the gap before a
            // paragraph depends on the one before it).
            candidate.source_index > restart_at
                && candidate.source_index >= changed_end + 2
                && match limit {
                    RunLimit::Pages(pages) => candidate.page_count > restart_page + pages,
                    RunLimit::Paragraph(index) => candidate.source_index >= index,
                }
        };
        let stop = |candidate: &PaginationCheckpoint, current: &Pagination| -> bool {
            give_up(candidate) || stable(candidate, current)
        };
        let run = paginate_source(
            &spliced,
            self.options,
            true,
            seed_checkpoint
                .as_ref()
                .map(|checkpoint| (checkpoint, &self.pagination)),
            Some(&stop),
        )?;

        let stream_work = run.stream_work;
        let mut pagination = run.pagination;
        let mut checkpoints_new = run.checkpoints;
        let stopped = run.stopped_at;
        let is_stable = stopped
            .as_ref()
            .is_some_and(|candidate| stable(candidate, &pagination));
        let old_signatures = &self.page_signatures;
        let mut page_signatures = Vec::with_capacity(pagination.pages.len() + 1);
        page_signatures
            .extend_from_slice(&old_signatures[..restart_page.min(old_signatures.len())]);
        let mut tail_checkpoints: Vec<PaginationCheckpoint> = Vec::new();
        let mut recomputed_end = pagination.pages.len();
        let mut paginated_new = new_len;
        if let (Some(new_cp), true) = (&stopped, is_stable) {
            let old_index = new_cp.source_index - inserted + removed;
            let old_cp = &self.checkpoints[old_index];
            let page_delta = new_cp.page_count as i64 - old_cp.page_count as i64;
            let placement_delta = new_cp.placement_count as i64 - old_cp.placement_count as i64;
            let diagnostic_delta = new_cp.diagnostic_count as i64 - old_cp.diagnostic_count as i64;
            if new_cp.placement_count != pagination.placements.len()
                || new_cp.page_count != pagination.pages.len()
                || new_cp.diagnostic_count != pagination.diagnostics.len()
            {
                return Err(PaginationError::InvalidInput(
                    "pagination seam is inconsistent",
                ));
            }
            let old = &self.pagination;
            for placed in &old.placements[old_cp.placement_count..] {
                pagination.placements.push(PlacedLine {
                    page_index: shift(placed.page_index, page_delta)?,
                    paragraph_index: shift(placed.paragraph_index, delta)?,
                    line_index: placed.line_index,
                    top: placed.top,
                    baseline: placed.baseline,
                });
            }
            for page in &old.pages[old_cp.page_count..] {
                pagination.pages.push(Page {
                    placements: shift(page.placements.start, placement_delta)?
                        ..shift(page.placements.end, placement_delta)?,
                });
            }
            for diagnostic in &old.diagnostics[old_cp.diagnostic_count..] {
                pagination
                    .diagnostics
                    .push(shift_diagnostic(diagnostic, delta)?);
            }
            for checkpoint in &self.checkpoints[old_index + 1..] {
                let mut moved = checkpoint.clone();
                moved.source_index = shift(checkpoint.source_index, delta)?;
                moved.page_count = shift(checkpoint.page_count, page_delta)?;
                moved.placement_count = shift(checkpoint.placement_count, placement_delta)?;
                moved.page_start = shift(checkpoint.page_start, placement_delta)?;
                moved.diagnostic_count = shift(checkpoint.diagnostic_count, diagnostic_delta)?;
                moved.previous_paragraph = match checkpoint.previous_paragraph {
                    Some(p) if p >= boundary_old => Some(shift(p, delta)?),
                    other => other,
                };
                tail_checkpoints.push(moved);
            }
            recomputed_end = new_cp.page_count + 1;
            paginated_new = shift(paginated_old, delta)?;
        } else if let Some(new_cp) = &stopped {
            // Gave up: the open page becomes the provisional last page.
            paginated_new = new_cp.source_index;
            if pagination.placements.len() > new_cp.page_start {
                pagination.pages.push(Page {
                    placements: new_cp.page_start..pagination.placements.len(),
                });
            }
            recomputed_end = pagination.pages.len();
        }
        let recomputed_end = recomputed_end.min(pagination.pages.len());
        for page in restart_page..recomputed_end {
            let range = pagination.pages[page].placements.clone();
            page_signatures.push(Self::signature(&spliced, &pagination.placements[range]));
        }
        if let (Some(new_cp), true) = (&stopped, is_stable) {
            let old_index = new_cp.source_index - inserted + removed;
            let old_open_page = self.checkpoints[old_index].page_count;
            if old_open_page < old_signatures.len() {
                page_signatures.extend_from_slice(&old_signatures[old_open_page + 1..]);
            }
        }
        if page_signatures.len() != pagination.pages.len() {
            return Err(PaginationError::InvalidInput("page signatures diverged"));
        }
        let old_page_count = self.pagination.pages.len();
        let changed_pages: Vec<usize> = page_signatures
            .iter()
            .enumerate()
            .filter(|(page, signature)| old_signatures.get(*page) != Some(*signature))
            .map(|(page, _)| page)
            .collect();
        let pages_recalculated = recomputed_end - restart_page.min(recomputed_end);
        let stats = StreamStats {
            restart_paragraph: restart,
            stream_work,
            pages_recalculated,
            pages_reused: pagination.pages.len() - pages_recalculated,
            stopped_early: is_stable,
            gave_up: stopped.is_some() && !is_stable,
        };

        // Commit.
        let mut checkpoints = std::mem::take(&mut self.checkpoints);
        checkpoints.truncate(restart.unwrap_or(0));
        checkpoints.append(&mut checkpoints_new);
        checkpoints.append(&mut tail_checkpoints);
        checkpoints.truncate(paginated_new);
        self.entries.splice(first..first + removed, new_entries);
        self.checkpoints = checkpoints;
        self.pagination = pagination;
        self.page_signatures = page_signatures;
        self.next_generation = generation;
        self.paginated = paginated_new;
        Ok(StreamUpdate {
            old_page_count,
            page_count: self.pagination.pages.len(),
            changed_pages,
            paginated_paragraphs: self.paginated,
            complete: self.paginated == self.entries.len(),
            stats,
        })
    }

    /// Paginates the paragraphs that are laid out but not yet paginated, up
    /// to `max_paragraphs` of them.
    pub fn advance(&mut self, max_paragraphs: usize) -> Result<StreamUpdate, PaginationError> {
        if self.paginated >= self.entries.len() {
            return Ok(StreamUpdate {
                old_page_count: self.pagination.pages.len(),
                page_count: self.pagination.pages.len(),
                paginated_paragraphs: self.paginated,
                complete: true,
                ..StreamUpdate::default()
            });
        }
        let at = self.paginated;
        self.replace(
            self.entries.len(),
            0,
            Vec::new(),
            RunLimit::Paragraph(at.saturating_add(max_paragraphs.max(1))),
        )
    }
}
