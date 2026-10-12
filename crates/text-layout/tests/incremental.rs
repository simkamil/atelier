// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{
    embedded::EMBEDDED_FONTS,
    manager::{FontManager, FontSource},
};
use atelier_text_layout::incremental::{
    FallbackReason, IncrementalError, IncrementalOptions, IncrementalSession, ParagraphInput,
};
use atelier_text_layout::pagination::{
    PaginationError, PaginationOptions, ParagraphRules, paginate,
};
use atelier_text_layout::tabs::TabSettings;
use atelier_text_layout::tabs::layout_paragraph_with_tabs;
use atelier_text_layout::{LayoutError, LayoutOptions};
use atelier_text_model::rich::{Paragraph, ResolvedProperties, RichDocument};

fn ids(count: usize) -> Vec<atelier_text_model::rich::NodeId> {
    let mut document = RichDocument::new();
    (0..count).map(|_| document.alloc_id().unwrap()).collect()
}

fn options() -> IncrementalOptions {
    IncrementalOptions {
        layout: LayoutOptions::new(30_000),
        pagination: PaginationOptions {
            page_height: 30_000,
            max_pages: 1000,
            max_work: 2_000_000,
        },
        tabs: TabSettings::default(),
    }
}

fn inputs<'a>(
    ids: &'a [atelier_text_model::rich::NodeId],
    paragraphs: &'a [Paragraph],
    rules: &'a [ParagraphRules],
    properties: &'a ResolvedProperties,
) -> Vec<ParagraphInput<'a>> {
    ids.iter()
        .zip(paragraphs)
        .zip(rules)
        .map(|((&id, paragraph), rule)| ParagraphInput {
            id,
            paragraph,
            properties,
            rules: rule,
        })
        .collect()
}

fn assert_matches_fresh(
    result: &atelier_text_layout::incremental::IncrementalResult,
    inputs: &[ParagraphInput<'_>],
    manager: &FontManager<'_>,
    opts: &IncrementalOptions,
) {
    let layouts = inputs
        .iter()
        .map(|input| {
            layout_paragraph_with_tabs(
                input.paragraph,
                input.properties,
                manager,
                opts.layout,
                &opts.tabs,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let pagination = paginate(
        &layouts
            .iter()
            .map(|layout| layout.layout.clone())
            .collect::<Vec<_>>(),
        &inputs
            .iter()
            .map(|input| input.rules.clone())
            .collect::<Vec<_>>(),
        opts.pagination,
    )
    .unwrap();
    assert_eq!(result.layouts, layouts);
    assert_eq!(result.pagination, pagination);
}

#[test]
fn identical_update_reuses_layouts_and_pagination_without_stream_work() {
    let ids = ids(4);
    let paragraphs = (0..4)
        .map(|i| Paragraph::with_text(&format!("line {i}")))
        .collect::<Vec<_>>();
    let rules = vec![ParagraphRules::default(); 4];
    let properties = ResolvedProperties::default();
    let mut session = IncrementalSession::new();
    let manager = FontManager::new();

    let first = session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            options(),
        )
        .unwrap()
        .clone();
    assert_eq!(
        first.stats.fallback_reason,
        Some(FallbackReason::InitialLayout)
    );
    let second = session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            options(),
        )
        .unwrap();
    assert_eq!(second.stats.paragraphs_reshaped, 0);
    assert_eq!(second.stats.paragraphs_reused, 4);
    assert_eq!(second.stats.pages_recalculated, 0);
    assert_eq!(second.stats.pages_reused, first.pagination.pages.len());
    assert_eq!(second.stats.pagination_stream_work, 0);
    assert_eq!(second.pagination, first.pagination);
}

#[test]
fn same_manager_font_mutations_invalidate_only_after_success() {
    let ids = ids(2);
    let paragraphs = [
        Paragraph::with_text("same input one"),
        Paragraph::with_text("same input two"),
    ];
    let rules = vec![ParagraphRules::default(); 2];
    let properties = ResolvedProperties::default();
    let valid_inputs = inputs(&ids, &paragraphs, &rules, &properties);
    let manager = &mut FontManager::new();
    let mut session = IncrementalSession::new();
    session.update(&valid_inputs, manager, options()).unwrap();

    let registered = manager
        .register(EMBEDDED_FONTS[0].data, 0, FontSource::Document)
        .unwrap();
    let result = session.update(&valid_inputs, manager, options()).unwrap();
    assert_eq!(
        result.stats.fallback_reason,
        Some(FallbackReason::FontRevisionChanged)
    );
    assert_eq!(result.stats.paragraphs_reshaped, 2);
    assert_eq!(result.stats.paragraphs_reused, 0);
    assert_matches_fresh(result, &valid_inputs, manager, &options());

    let revision = manager.cache_revision();
    assert!(manager.register(&[], 0, FontSource::Document).is_err());
    assert_eq!(manager.cache_revision(), revision);
    let result = session.update(&valid_inputs, manager, options()).unwrap();
    assert_eq!(result.stats.paragraphs_reshaped, 0);
    assert_eq!(result.stats.paragraphs_reused, 2);
    assert_eq!(result.stats.fallback_reason, None);
    assert_matches_fresh(result, &valid_inputs, manager, &options());

    manager.remove(registered).unwrap();
    let result = session.update(&valid_inputs, manager, options()).unwrap();
    assert_eq!(
        result.stats.fallback_reason,
        Some(FallbackReason::FontRevisionChanged)
    );
    assert_eq!(result.stats.paragraphs_reshaped, 2);
    assert_matches_fresh(result, &valid_inputs, manager, &options());

    let revision = manager.cache_revision();
    assert!(manager.remove(registered).is_err());
    assert_eq!(manager.cache_revision(), revision);
    let result = session.update(&valid_inputs, manager, options()).unwrap();
    assert_eq!(result.stats.paragraphs_reshaped, 0);
    assert_eq!(result.stats.paragraphs_reused, 2);
    assert_eq!(result.stats.fallback_reason, None);
    assert_matches_fresh(result, &valid_inputs, manager, &options());
}

#[test]
fn insert_and_delete_keep_stable_id_layouts_and_match_fresh_results() {
    let all_ids = ids(4);
    let paragraphs = [
        Paragraph::with_text("first"),
        Paragraph::with_text("inserted"),
        Paragraph::with_text("third"),
        Paragraph::with_text("fourth"),
    ];
    let rules = vec![ParagraphRules::default(); 4];
    let properties = ResolvedProperties::default();
    let manager = FontManager::new();
    let mut session = IncrementalSession::new();
    let initial_paragraphs = [
        paragraphs[0].clone(),
        paragraphs[2].clone(),
        paragraphs[3].clone(),
    ];
    let initial_ids = [all_ids[0], all_ids[2], all_ids[3]];
    let initial_inputs = inputs(&initial_ids, &initial_paragraphs, &rules[..3], &properties);
    session
        .update(&initial_inputs, &manager, options())
        .unwrap();

    let inserted_ids = [all_ids[0], all_ids[1], all_ids[2], all_ids[3]];
    let inserted_paragraphs = [
        paragraphs[0].clone(),
        paragraphs[1].clone(),
        paragraphs[2].clone(),
        paragraphs[3].clone(),
    ];
    let inserted = inputs(&inserted_ids, &inserted_paragraphs, &rules, &properties);
    let result = session.update(&inserted, &manager, options()).unwrap();
    assert_eq!(
        result.stats.fallback_reason,
        Some(FallbackReason::StructureChanged)
    );
    assert_eq!(result.stats.paragraphs_reshaped, 1);
    assert_eq!(result.stats.paragraphs_reused, 3);
    assert_matches_fresh(result, &inserted, &manager, &options());

    let deleted_ids = [all_ids[0], all_ids[2], all_ids[3]];
    let deleted_paragraphs = [
        paragraphs[0].clone(),
        paragraphs[2].clone(),
        paragraphs[3].clone(),
    ];
    let deleted = inputs(&deleted_ids, &deleted_paragraphs, &rules[..3], &properties);
    let result = session.update(&deleted, &manager, options()).unwrap();
    assert_eq!(
        result.stats.fallback_reason,
        Some(FallbackReason::StructureChanged)
    );
    assert_eq!(result.stats.paragraphs_reshaped, 0);
    assert_eq!(result.stats.paragraphs_reused, 3);
    assert_matches_fresh(result, &deleted, &manager, &options());
}

#[test]
fn resolved_property_edit_reshapes_only_its_paragraph_and_matches_fresh() {
    let ids = ids(3);
    let paragraphs = [
        Paragraph::with_text("first"),
        Paragraph::with_text("larger text"),
        Paragraph::with_text("last"),
    ];
    let rules = vec![ParagraphRules::default(); 3];
    let default = ResolvedProperties::default();
    let mut enlarged = ResolvedProperties::default();
    enlarged.character.size_half_points = Some(36);
    let before = ids
        .iter()
        .zip(&paragraphs)
        .zip(&rules)
        .map(|((&id, paragraph), rule)| ParagraphInput {
            id,
            paragraph,
            properties: &default,
            rules: rule,
        })
        .collect::<Vec<_>>();
    let after = vec![
        ParagraphInput {
            id: ids[0],
            paragraph: &paragraphs[0],
            properties: &default,
            rules: &rules[0],
        },
        ParagraphInput {
            id: ids[1],
            paragraph: &paragraphs[1],
            properties: &enlarged,
            rules: &rules[1],
        },
        ParagraphInput {
            id: ids[2],
            paragraph: &paragraphs[2],
            properties: &default,
            rules: &rules[2],
        },
    ];
    let manager = FontManager::new();
    let mut session = IncrementalSession::new();
    session.update(&before, &manager, options()).unwrap();
    let result = session.update(&after, &manager, options()).unwrap();
    assert_eq!(result.stats.paragraphs_reshaped, 1);
    assert_eq!(result.stats.paragraphs_reused, 2);
    assert_eq!(
        result.stats.fallback_reason,
        Some(FallbackReason::BoundaryNotRepresentable)
    );
    assert_matches_fresh(result, &after, &manager, &options());
}

#[test]
fn local_edit_in_a_300_page_document_stops_at_a_real_stable_boundary() {
    let properties = ResolvedProperties::default();
    let manager = FontManager::new();
    let sample = layout_paragraph_with_tabs(
        &Paragraph::with_text("Paragraph 000000"),
        &properties,
        &manager,
        LayoutOptions::new(30_000),
        &TabSettings::default(),
    )
    .unwrap();
    // A4 usable height is about 44,800 1/64 pt units. Round down to a whole
    // number of real text lines so the fixture has exactly 300 pages.
    let line_height = sample.layout.lines[0].height;
    let lines_per_page = 44_800 / line_height;
    let mut page_options = options();
    page_options.pagination.page_height = lines_per_page * line_height;
    let count = 300;
    let ids = ids(count);
    let make_page = |label: &str| {
        let line = format!("{label}\n").repeat(lines_per_page.saturating_sub(1) as usize);
        Paragraph::with_text(&format!("{line}{label}"))
    };
    let mut paragraphs = (0..count)
        .map(|i| make_page(&format!("Paragraph {i:03}")))
        .collect::<Vec<_>>();
    let rules = vec![ParagraphRules::default(); count];
    let mut session = IncrementalSession::new();

    let first = session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            page_options.clone(),
        )
        .unwrap()
        .clone();
    assert_eq!(
        first.pagination.placements.len(),
        count * lines_per_page as usize
    );
    assert_eq!(first.pagination.pages.len(), 300);

    let changed = count / 2;
    paragraphs[changed] = make_page(&format!("Edited paragraph {changed:03}"));
    let result = session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            page_options.clone(),
        )
        .unwrap();
    assert_eq!(result.stats.paragraphs_reshaped, 1);
    assert_eq!(result.stats.paragraphs_reused, count - 1);
    assert!(result.stats.restart_paragraph.unwrap() <= changed);
    assert!(
        result.stats.pages_recalculated < 10,
        "stats: {:?}",
        result.stats
    );
    assert!(result.stats.pages_reused > 290);
    assert_eq!(result.stats.fallback_reason, None);

    let full = paginate(
        &result
            .layouts
            .iter()
            .map(|layout| layout.layout.clone())
            .collect::<Vec<_>>(),
        &rules,
        page_options.pagination,
    )
    .unwrap();
    assert_eq!(result.pagination, full);
}

#[test]
fn global_invalidations_are_reported_and_structure_reuses_unchanged_geometry() {
    let ids = ids(3);
    let paragraphs = (0..3)
        .map(|i| Paragraph::with_text(&format!("p{i}")))
        .collect::<Vec<_>>();
    let rules = vec![ParagraphRules::default(); 3];
    let properties = ResolvedProperties::default();
    let mut session = IncrementalSession::new();
    let manager = FontManager::new();
    session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            options(),
        )
        .unwrap();

    let mut changed_layout = options();
    changed_layout.layout.width += 1;
    let result = session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            changed_layout.clone(),
        )
        .unwrap();
    assert_eq!(
        result.stats.fallback_reason,
        Some(FallbackReason::LayoutOptionsChanged)
    );
    assert_eq!(result.stats.paragraphs_reshaped, 3);

    let mut reordered_ids = ids.clone();
    let mut reordered_paragraphs = paragraphs.clone();
    let mut reordered_rules = rules.clone();
    reordered_ids.swap(0, 2);
    reordered_paragraphs.swap(0, 2);
    reordered_rules.swap(0, 2);
    let result = session
        .update(
            &inputs(
                &reordered_ids,
                &reordered_paragraphs,
                &reordered_rules,
                &properties,
            ),
            &manager,
            changed_layout.clone(),
        )
        .unwrap();
    assert_eq!(
        result.stats.fallback_reason,
        Some(FallbackReason::StructureChanged)
    );
    assert_eq!(result.stats.paragraphs_reshaped, 0);
    assert_eq!(result.stats.paragraphs_reused, 3);

    let other_manager = FontManager::new();
    let result = session
        .update(
            &inputs(
                &reordered_ids,
                &reordered_paragraphs,
                &reordered_rules,
                &properties,
            ),
            &other_manager,
            changed_layout,
        )
        .unwrap();
    assert_eq!(
        result.stats.fallback_reason,
        Some(FallbackReason::FontRevisionChanged)
    );
    assert_eq!(result.stats.paragraphs_reshaped, 3);
}

#[test]
fn failures_and_duplicate_ids_leave_the_last_published_result_usable() {
    let ids = ids(2);
    let paragraphs = vec![Paragraph::with_text("one"), Paragraph::with_text("two")];
    let rules = vec![ParagraphRules::default(); 2];
    let properties = ResolvedProperties::default();
    let manager = FontManager::new();
    let mut session = IncrementalSession::new();
    let published = session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            options(),
        )
        .unwrap()
        .clone();

    let duplicate_ids = [ids[0], ids[0]];
    assert!(matches!(
        session.update(
            &inputs(&duplicate_ids, &paragraphs, &rules, &properties),
            &manager,
            options()
        ),
        Err(IncrementalError::DuplicateParagraphId(_))
    ));
    assert_eq!(session.result().unwrap().pagination, published.pagination);

    let mut invalid = options();
    invalid.pagination.max_work = 1;
    assert!(
        session
            .update(
                &inputs(&ids, &paragraphs, &rules, &properties),
                &manager,
                invalid
            )
            .is_err()
    );
    assert_eq!(session.result().unwrap().pagination, published.pagination);
}

#[test]
fn layout_and_pagination_failures_preserve_published_state_and_cache() {
    let ids = ids(2);
    let paragraphs = [
        Paragraph::with_text("first"),
        Paragraph::with_text("second"),
    ];
    let rules = vec![ParagraphRules::default(); 2];
    let properties = ResolvedProperties::default();
    let valid_inputs = inputs(&ids, &paragraphs, &rules, &properties);
    let manager = FontManager::new();
    let mut session = IncrementalSession::new();
    let published = session
        .update(&valid_inputs, &manager, options())
        .unwrap()
        .clone();

    let mut invalid_layout = options();
    invalid_layout.layout.width = 0;
    assert!(matches!(
        session.update(&valid_inputs, &manager, invalid_layout),
        Err(IncrementalError::Layout(LayoutError::InvalidOptions(
            "width must be positive"
        )))
    ));
    assert_eq!(session.result(), Some(&published));
    let retry = session.update(&valid_inputs, &manager, options()).unwrap();
    assert_eq!(retry.stats.paragraphs_reshaped, 0);
    assert_eq!(retry.stats.paragraphs_reused, 2);
    assert_eq!(retry.stats.pages_recalculated, 0);
    assert_eq!(retry.stats.pages_reused, published.pagination.pages.len());
    let after_layout_retry = retry.clone();

    let mut overflow_rules = rules.clone();
    overflow_rules[0].space_before = i64::MAX;
    let overflow_inputs = inputs(&ids, &paragraphs, &overflow_rules, &properties);
    assert!(matches!(
        session.update(&overflow_inputs, &manager, options()),
        Err(IncrementalError::Pagination(
            PaginationError::ArithmeticOverflow
        ))
    ));
    assert_eq!(session.result(), Some(&after_layout_retry));

    let retry = session.update(&valid_inputs, &manager, options()).unwrap();
    assert_eq!(retry.stats.paragraphs_reshaped, 0);
    assert_eq!(retry.stats.paragraphs_reused, 2);
    assert_eq!(retry.stats.pages_recalculated, 0);
    assert_eq!(retry.stats.pages_reused, published.pagination.pages.len());
}

#[test]
fn global_tabs_invalidate_geometry_and_rule_changes_match_full_pagination() {
    let ids = ids(5);
    let mut paragraphs = (0..5)
        .map(|i| Paragraph::with_text(&format!("p{i} content")))
        .collect::<Vec<_>>();
    paragraphs[4] = Paragraph::with_text("p4\ncontent");
    let mut rules = vec![ParagraphRules::default(); 5];
    let properties = ResolvedProperties::default();
    let manager = FontManager::new();
    let mut session = IncrementalSession::new();
    session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            options(),
        )
        .unwrap();

    rules[2].keep_with_next = true;
    rules[3].keep_together = true;
    rules[4].line_breaks_before = vec![1];
    let result = session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            options(),
        )
        .unwrap();
    assert_eq!(result.stats.paragraphs_reshaped, 0);
    let full = paginate(
        &result
            .layouts
            .iter()
            .map(|layout| layout.layout.clone())
            .collect::<Vec<_>>(),
        &rules,
        options().pagination,
    )
    .unwrap();
    assert_eq!(result.pagination, full);

    let mut tab_options = options();
    tab_options.tabs.default_interval += 1;
    let result = session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            tab_options,
        )
        .unwrap();
    assert_eq!(
        result.stats.fallback_reason,
        Some(FallbackReason::TabSettingsChanged)
    );
    assert_eq!(result.stats.paragraphs_reshaped, paragraphs.len());
}

#[test]
fn local_edit_with_global_pagination_change_uses_a_full_declared_fallback() {
    let ids = ids(4);
    let mut paragraphs = (0..4)
        .map(|i| Paragraph::with_text(&format!("p{i}")))
        .collect::<Vec<_>>();
    let rules = vec![ParagraphRules::default(); 4];
    let properties = ResolvedProperties::default();
    let manager = FontManager::new();
    let mut session = IncrementalSession::new();
    let initial = session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            options(),
        )
        .unwrap()
        .clone();

    paragraphs[2] = Paragraph::with_text("edited paragraph");
    let mut changed = options();
    changed.pagination.page_height += 1;
    let result = session
        .update(
            &inputs(&ids, &paragraphs, &rules, &properties),
            &manager,
            changed.clone(),
        )
        .unwrap();
    assert_eq!(
        result.stats.fallback_reason,
        Some(FallbackReason::PaginationOptionsChanged)
    );
    assert_eq!(result.stats.restart_paragraph, None);
    assert_eq!(
        result.stats.pages_recalculated,
        result.pagination.pages.len()
    );
    let full = paginate(
        &result
            .layouts
            .iter()
            .map(|layout| layout.layout.clone())
            .collect::<Vec<_>>(),
        &rules,
        changed.pagination,
    )
    .unwrap();
    assert_ne!(
        result.pagination.pages,
        Vec::<atelier_text_layout::pagination::Page>::new()
    );
    assert_eq!(result.pagination, full);
    assert!(!initial.pagination.pages.is_empty());
}
