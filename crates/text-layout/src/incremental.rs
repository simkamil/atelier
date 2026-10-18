// SPDX-License-Identifier: AGPL-3.0-or-later
//! Transactional paragraph-layout and pagination cache (US-213).

use crate::pagination::{Page, Pagination, PaginationOptions, ParagraphRules};
use crate::pagination::{PaginationCheckpoint, PaginationError, paginate_recording};
use crate::tabs::{TabLayout, TabSettings, layout_paragraph_with_tabs};
use crate::{LayoutError, LayoutOptions, ParagraphLayout};
use atelier_font::manager::FontManager;
use atelier_text_model::rich::{NodeId, Paragraph, ResolvedProperties};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

/// One ordered document paragraph. Its ID must be unique within the input.
#[derive(Clone, Copy, Debug)]
pub struct ParagraphInput<'a> {
    pub id: NodeId,
    pub paragraph: &'a Paragraph,
    /// Fully resolved document/style defaults and direct paragraph/run properties.
    pub properties: &'a ResolvedProperties,
    pub rules: &'a ParagraphRules,
}

/// Document-wide options used by shaping and pagination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncrementalOptions {
    pub layout: LayoutOptions,
    pub pagination: PaginationOptions,
    pub tabs: TabSettings,
}

/// Why an update could not reuse an incremental pagination suffix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FallbackReason {
    InitialLayout,
    StructureChanged,
    LayoutOptionsChanged,
    TabSettingsChanged,
    PaginationOptionsChanged,
    FontRevisionChanged,
    BoundaryNotRepresentable,
}

/// Verifiable work performed by the most recent successful update.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IncrementalStats {
    pub paragraphs_reshaped: usize,
    pub paragraphs_reused: usize,
    pub pages_recalculated: usize,
    pub pages_reused: usize,
    pub restart_paragraph: Option<usize>,
    pub fallback_reason: Option<FallbackReason>,
    /// Always zero since US-216: pagination validates paragraphs lazily, as it
    /// reaches them (counted in `pagination_stream_work`).
    pub pagination_preflight_work: usize,
    /// Pagination stream work actually executed after the optional checkpoint.
    pub pagination_stream_work: usize,
}

/// Complete, atomically published incremental layout result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncrementalResult {
    pub layouts: Vec<TabLayout>,
    pub pagination: Pagination,
    pub stats: IncrementalStats,
}

/// Invalid IDs, shaping errors, or pagination errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IncrementalError {
    DuplicateParagraphId(NodeId),
    Layout(LayoutError),
    Pagination(PaginationError),
}

impl fmt::Display for IncrementalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateParagraphId(id) => write!(f, "duplicate paragraph ID {id:?}"),
            Self::Layout(error) => error.fmt(f),
            Self::Pagination(error) => error.fmt(f),
        }
    }
}
impl Error for IncrementalError {}
impl From<LayoutError> for IncrementalError {
    fn from(value: LayoutError) -> Self {
        Self::Layout(value)
    }
}
impl From<PaginationError> for IncrementalError {
    fn from(value: PaginationError) -> Self {
        Self::Pagination(value)
    }
}

#[derive(Clone, Debug)]
struct CachedParagraph {
    id: NodeId,
    paragraph: Paragraph,
    properties: ResolvedProperties,
    rules: ParagraphRules,
}

/// Reusable paragraph layouts and pagination state.
#[derive(Clone, Debug, Default)]
pub struct IncrementalSession {
    cache: Vec<CachedParagraph>,
    result: Option<IncrementalResult>,
    checkpoints: Vec<PaginationCheckpoint>,
    options: Option<IncrementalOptions>,
    font_revision: Option<u64>,
}

impl IncrementalSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the last published result, including after a failed update.
    pub fn result(&self) -> Option<&IncrementalResult> {
        self.result.as_ref()
    }

    /// Layouts and pagination are calculated into temporary state and become
    /// visible together only after every paragraph and page succeeds.
    pub fn update<'a>(
        &mut self,
        inputs: &[ParagraphInput<'a>],
        fonts: &FontManager<'_>,
        options: IncrementalOptions,
    ) -> Result<&IncrementalResult, IncrementalError> {
        let mut ids = BTreeSet::new();
        for input in inputs {
            if !ids.insert(input.id) {
                return Err(IncrementalError::DuplicateParagraphId(input.id));
            }
        }

        let revision = fonts.cache_revision();
        let previous_options = self.options.as_ref();
        let structure_changed = self.result.is_some()
            && (self.cache.len() != inputs.len()
                || self
                    .cache
                    .iter()
                    .zip(inputs)
                    .any(|(old, input)| old.id != input.id));
        let layout_options_changed =
            previous_options.is_some_and(|old| old.layout != options.layout);
        let tabs_changed = previous_options.is_some_and(|old| old.tabs != options.tabs);
        let pagination_options_changed =
            previous_options.is_some_and(|old| old.pagination != options.pagination);
        let font_changed = self.font_revision.is_some_and(|old| old != revision);

        let result_is_reusable = self.result.is_some()
            && !structure_changed
            && !layout_options_changed
            && !tabs_changed
            && !pagination_options_changed
            && !font_changed
            && self.cache.iter().zip(inputs).all(|(old, input)| {
                old.id == input.id
                    && old.paragraph == *input.paragraph
                    && old.properties == *input.properties
                    && old.rules == *input.rules
            });
        if result_is_reusable {
            if let Some(result) = self.result.as_mut() {
                result.stats = IncrementalStats {
                    paragraphs_reused: inputs.len(),
                    pages_reused: result.pagination.pages.len(),
                    ..IncrementalStats::default()
                };
            }
            return match self.result.as_ref() {
                Some(result) => Ok(result),
                None => unreachable!("reusable result was checked above"),
            };
        }

        let old_cache: BTreeMap<_, _> = self
            .cache
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.id, (index, entry)))
            .collect();
        let mut layouts = Vec::with_capacity(inputs.len());
        let mut new_cache = Vec::with_capacity(inputs.len());
        let mut reshaped = 0usize;
        let mut layout_reused = 0usize;
        for input in inputs {
            let previous = old_cache.get(&input.id).copied();
            let can_reuse = !layout_options_changed
                && !tabs_changed
                && !font_changed
                && previous.is_some_and(|(_, old)| {
                    old.id == input.id
                        && old.paragraph == *input.paragraph
                        && old.properties == *input.properties
                });
            let layout = if can_reuse {
                layout_reused += 1;
                let (old_index, _) = previous.expect("reusable entry exists");
                self.result
                    .as_ref()
                    .expect("reusable cache belongs to a published result")
                    .layouts[old_index]
                    .clone()
            } else {
                reshaped += 1;
                layout_paragraph_with_tabs(
                    input.paragraph,
                    input.properties,
                    fonts,
                    options.layout,
                    &options.tabs,
                )?
            };
            new_cache.push(CachedParagraph {
                id: input.id,
                paragraph: input.paragraph.clone(),
                properties: input.properties.clone(),
                rules: input.rules.clone(),
            });
            layouts.push(layout);
        }

        let rules: Vec<_> = inputs.iter().map(|input| input.rules.clone()).collect();
        let pagination_layouts: Vec<&ParagraphLayout> =
            layouts.iter().map(|layout| &layout.layout).collect();
        let old_result = self.result.as_ref();
        let (first_changed, last_changed) = match old_result {
            Some(old_result) if !structure_changed => {
                let first_changed = pagination_layouts
                    .iter()
                    .zip(&old_result.layouts)
                    .zip(&rules)
                    .zip(&self.cache)
                    .position(|(((new_layout, old_layout), new_rule), old_entry)| {
                        *new_layout != &old_layout.layout || *new_rule != old_entry.rules
                    });
                let last_changed = pagination_layouts
                    .iter()
                    .zip(&old_result.layouts)
                    .zip(&rules)
                    .zip(&self.cache)
                    .rposition(|(((new_layout, old_layout), new_rule), old_entry)| {
                        *new_layout != &old_layout.layout || *new_rule != old_entry.rules
                    });
                (first_changed, last_changed)
            }
            _ => (None, None),
        };

        let fallback_reason = if old_result.is_none() {
            Some(FallbackReason::InitialLayout)
        } else if structure_changed {
            Some(FallbackReason::StructureChanged)
        } else if layout_options_changed {
            Some(FallbackReason::LayoutOptionsChanged)
        } else if tabs_changed {
            Some(FallbackReason::TabSettingsChanged)
        } else if font_changed {
            Some(FallbackReason::FontRevisionChanged)
        } else if pagination_options_changed {
            Some(FallbackReason::PaginationOptionsChanged)
        } else {
            None
        };

        let (run, restart, old_stable_checkpoint) =
            if let (Some(start), Some(old_result), Some(last)) = (
                first_changed.filter(|_| fallback_reason.is_none()),
                old_result,
                last_changed,
            ) {
                let restart = safe_restart_index(start, &self.cache, &old_result.pagination);
                if let Some(checkpoint) = self
                    .checkpoints
                    .iter()
                    .find(|checkpoint| checkpoint.source_index == restart)
                {
                    let old_checkpoint_by_index: BTreeMap<_, _> = self
                        .checkpoints
                        .iter()
                        .map(|checkpoint| (checkpoint.source_index, checkpoint))
                        .collect();
                    let mut suffix_equal = vec![false; inputs.len() + 1];
                    suffix_equal[inputs.len()] = true;
                    for index in (0..inputs.len()).rev() {
                        suffix_equal[index] = index + 1 < suffix_equal.len()
                            && suffix_equal[index + 1]
                            && pagination_layouts[index] == &old_result.layouts[index].layout
                            && rules[index] == self.cache[index].rules;
                    }
                    let should_stop = |candidate: &PaginationCheckpoint, current: &Pagination| {
                        candidate.source_index > last
                            && candidate.source_index < inputs.len()
                            && suffix_equal[candidate.source_index]
                            && old_checkpoint_by_index
                                .get(&candidate.source_index)
                                .is_some_and(|old| {
                                    checkpoint_state_equal(candidate, old)
                                        && open_page_equal(
                                            candidate,
                                            current,
                                            old,
                                            &old_result.pagination,
                                        )
                                })
                    };
                    let run = paginate_recording(
                        &pagination_layouts,
                        &rules,
                        options.pagination,
                        true,
                        Some((checkpoint, &old_result.pagination)),
                        Some(&should_stop),
                    )?;
                    let stable = run.stopped_at.as_ref().and_then(|candidate| {
                        old_checkpoint_by_index
                            .get(&candidate.source_index)
                            .copied()
                    });
                    (run, Some(restart), stable.cloned())
                } else {
                    let run = paginate_recording(
                        &pagination_layouts,
                        &rules,
                        options.pagination,
                        true,
                        None,
                        None,
                    )?;
                    (run, Some(0), None)
                }
            } else {
                let run = paginate_recording(
                    &pagination_layouts,
                    &rules,
                    options.pagination,
                    true,
                    None,
                    None,
                )?;
                (run, None, None)
            };

        let mut pagination = run.pagination;
        let mut checkpoints = if restart.is_some() && fallback_reason.is_none() {
            let start = restart.unwrap_or(0);
            self.checkpoints
                .iter()
                .filter(|checkpoint| checkpoint.source_index < start)
                .cloned()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        checkpoints.extend(run.checkpoints.iter().cloned());
        let mut pages_recalculated =
            if let Some(start) = restart.filter(|_| fallback_reason.is_none()) {
                let start_checkpoint = self
                    .checkpoints
                    .iter()
                    .find(|checkpoint| checkpoint.source_index == start);
                start_checkpoint.map_or(pagination.pages.len(), |checkpoint| {
                    pagination.pages.len().saturating_sub(checkpoint.page_count)
                })
            } else {
                pagination.pages.len()
            };
        let stream_work = run.stream_work;
        let preflight_work = run.preflight_work;

        if let (Some(old_result), Some(old_checkpoint), Some(new_checkpoint)) = (
            old_result,
            old_stable_checkpoint.as_ref(),
            run.stopped_at.as_ref(),
        ) {
            splice_suffix(
                &mut pagination,
                &old_result.pagination,
                new_checkpoint,
                old_checkpoint,
            )?;
            pages_recalculated = new_checkpoint
                .page_count
                .saturating_sub(
                    restart
                        .and_then(|index| {
                            self.checkpoints
                                .iter()
                                .find(|cp| cp.source_index == index)
                                .map(|cp| cp.page_count)
                        })
                        .unwrap_or(0),
                )
                .saturating_add(usize::from(new_checkpoint.open_page_placements > 0));
            checkpoints.extend(adjusted_old_suffix(
                &self.checkpoints,
                new_checkpoint,
                old_checkpoint,
            )?);
        } else if restart.is_some() && fallback_reason.is_none() {
            // The stream ran to the end because no stable boundary was safe.
            // This is an explicit whole-suffix fallback, with page work counted.
            let start = restart.unwrap_or(0);
            pages_recalculated = pagination.pages.len().saturating_sub(
                self.checkpoints
                    .iter()
                    .find(|checkpoint| checkpoint.source_index == start)
                    .map_or(0, |checkpoint| checkpoint.page_count),
            );
        }

        let pages_reused = pagination.pages.len().saturating_sub(pages_recalculated);
        let reason =
            if restart.is_some() && fallback_reason.is_none() && old_stable_checkpoint.is_none() {
                Some(FallbackReason::BoundaryNotRepresentable)
            } else {
                fallback_reason
            };
        let stats = IncrementalStats {
            paragraphs_reshaped: reshaped,
            paragraphs_reused: layout_reused,
            pages_recalculated,
            pages_reused,
            restart_paragraph: restart,
            fallback_reason: reason,
            pagination_preflight_work: preflight_work,
            pagination_stream_work: stream_work,
        };
        let result = IncrementalResult {
            layouts,
            pagination,
            stats,
        };

        self.cache = new_cache;
        self.result = Some(result);
        self.checkpoints = checkpoints;
        self.options = Some(options);
        self.font_revision = Some(revision);
        Ok(self
            .result
            .as_ref()
            .expect("successful result was published"))
    }
}

fn checkpoint_state_equal(left: &PaginationCheckpoint, right: &PaginationCheckpoint) -> bool {
    left.source_index == right.source_index
        && left.page_count == right.page_count
        && left.page_y == right.page_y
        && left.previous_paragraph == right.previous_paragraph
        && left.break_after_pending == right.break_after_pending
        && left.open_page_placements == right.open_page_placements
        && left.keep_chain_height == right.keep_chain_height
        && left.has_following_nonempty == right.has_following_nonempty
}

fn open_page_equal(
    new_checkpoint: &PaginationCheckpoint,
    new: &Pagination,
    old_checkpoint: &PaginationCheckpoint,
    old: &Pagination,
) -> bool {
    let Some(new_open) = new
        .placements
        .get(new_checkpoint.page_start..new_checkpoint.placement_count)
    else {
        return false;
    };
    let Some(old_open) = old
        .placements
        .get(old_checkpoint.page_start..old_checkpoint.placement_count)
    else {
        return false;
    };
    new_open == old_open
}

fn safe_restart_index(start: usize, cache: &[CachedParagraph], pagination: &Pagination) -> usize {
    let changed_page = pagination
        .placements
        .iter()
        .filter(|placement| placement.paragraph_index == start)
        .map(|placement| placement.page_index)
        .min();
    let first_on_page = changed_page
        .and_then(|page| {
            pagination
                .placements
                .iter()
                .find(|placement| placement.page_index == page)
                .map(|placement| placement.paragraph_index)
        })
        .unwrap_or(start);
    // Rewind one paragraph as well as the paragraph that opens the affected
    // page. This contains widow/orphan moves from the prior page.
    let mut restart = first_on_page.saturating_sub(1);
    // Rewind across every earlier keep-with-next dependency. This is
    // intentionally conservative when explicit boundaries break the chain.
    while restart > 0 && cache[restart - 1].rules.keep_with_next {
        restart -= 1;
    }
    restart
}

fn splice_suffix(
    target: &mut Pagination,
    old: &Pagination,
    new_checkpoint: &PaginationCheckpoint,
    old_checkpoint: &PaginationCheckpoint,
) -> Result<(), PaginationError> {
    if new_checkpoint.page_count != old_checkpoint.page_count
        || new_checkpoint.placement_count > target.placements.len()
        || old_checkpoint.placement_count > old.placements.len()
        || new_checkpoint.diagnostic_count != target.diagnostics.len()
        || old_checkpoint.diagnostic_count > old.diagnostics.len()
    {
        return Err(PaginationError::InvalidInput(
            "pagination seam is inconsistent",
        ));
    }
    let placement_delta =
        new_checkpoint.placement_count as i128 - old_checkpoint.placement_count as i128;
    let page_start = new_checkpoint.page_start;
    target
        .placements
        .extend_from_slice(&old.placements[old_checkpoint.placement_count..]);
    for (index, page) in old.pages.iter().enumerate().skip(old_checkpoint.page_count) {
        let shift = |value: usize| -> Result<usize, PaginationError> {
            usize::try_from(value as i128 + placement_delta)
                .map_err(|_| PaginationError::ArithmeticOverflow)
        };
        let start = if index == old_checkpoint.page_count {
            page_start
        } else {
            shift(page.placements.start)?
        };
        target.pages.push(Page {
            placements: start..shift(page.placements.end)?,
        });
    }
    target
        .diagnostics
        .extend_from_slice(&old.diagnostics[old_checkpoint.diagnostic_count..]);
    Ok(())
}

fn adjusted_old_suffix(
    checkpoints: &[PaginationCheckpoint],
    new_checkpoint: &PaginationCheckpoint,
    old_checkpoint: &PaginationCheckpoint,
) -> Result<Vec<PaginationCheckpoint>, PaginationError> {
    let placement_delta =
        new_checkpoint.placement_count as i128 - old_checkpoint.placement_count as i128;
    let diagnostic_delta =
        new_checkpoint.diagnostic_count as i128 - old_checkpoint.diagnostic_count as i128;
    checkpoints
        .iter()
        .filter(|checkpoint| checkpoint.source_index > old_checkpoint.source_index)
        .map(|checkpoint| {
            let mut adjusted = checkpoint.clone();
            adjusted.placement_count =
                usize::try_from(checkpoint.placement_count as i128 + placement_delta)
                    .map_err(|_| PaginationError::ArithmeticOverflow)?;
            adjusted.page_start = usize::try_from(checkpoint.page_start as i128 + placement_delta)
                .map_err(|_| PaginationError::ArithmeticOverflow)?;
            adjusted.diagnostic_count =
                usize::try_from(checkpoint.diagnostic_count as i128 + diagnostic_delta)
                    .map_err(|_| PaginationError::ArithmeticOverflow)?;
            Ok(adjusted)
        })
        .collect()
}
