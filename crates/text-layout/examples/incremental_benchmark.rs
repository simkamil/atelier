// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compare full initial layout with a one-paragraph edit in a 300-page document.

use atelier_font::manager::FontManager;
use atelier_text_layout::LayoutOptions;
use atelier_text_layout::incremental::{IncrementalOptions, IncrementalSession, ParagraphInput};
use atelier_text_layout::pagination::{PaginationOptions, ParagraphRules};
use atelier_text_layout::tabs::TabSettings;
use atelier_text_layout::tabs::layout_paragraph_with_tabs;
use atelier_text_model::rich::{Paragraph, ResolvedProperties, RichDocument};
use std::time::Instant;

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = FontManager::new();
    let properties = ResolvedProperties::default();
    let layout_options = LayoutOptions::new(30_000);
    let sample = layout_paragraph_with_tabs(
        &Paragraph::with_text("Atelier paragraph 000000"),
        &properties,
        &manager,
        layout_options,
        &TabSettings::default(),
    )?;
    let line_height = sample.layout.lines[0].height;
    let a4_usable_height = 44_800i64;
    let lines_per_page = a4_usable_height / line_height;
    if lines_per_page == 0 {
        return Err("sample line is taller than the A4 usable height".into());
    }
    // Round down by less than one line so 300 complete pages fit exactly.
    let page_height = lines_per_page * line_height;
    let paragraph_count = 300usize;

    let mut document = RichDocument::new();
    let ids = (0..paragraph_count)
        .map(|_| document.alloc_id())
        .collect::<Result<Vec<_>, _>>()?;
    let make_page = |label: &str| {
        let line = format!("{label} with a short stable line\n")
            .repeat(lines_per_page.saturating_sub(1) as usize);
        Paragraph::with_text(&format!("{line}{label} with a short stable line"))
    };
    let mut paragraphs = (0..paragraph_count)
        .map(|index| make_page(&format!("Atelier paragraph {index:03}")))
        .collect::<Vec<_>>();
    let rules = vec![ParagraphRules::default(); paragraph_count];
    let options = IncrementalOptions {
        layout: layout_options,
        pagination: PaginationOptions {
            page_height,
            max_pages: 400,
            max_work: 8_000_000,
        },
        tabs: TabSettings::default(),
    };
    let mut session = IncrementalSession::new();
    let start = Instant::now();
    let initial = session.update(
        &inputs(&ids, &paragraphs, &rules, &properties),
        &manager,
        options.clone(),
    )?;
    let full_elapsed = start.elapsed();
    if initial.pagination.pages.len() != 300 {
        return Err(format!(
            "fixture produced {} pages instead of 300",
            initial.pagination.pages.len()
        )
        .into());
    }
    let initial_pages = initial.pagination.pages.len();
    let initial_stats = initial.stats.clone();

    let same_inputs = inputs(&ids, &paragraphs, &rules, &properties);
    let mut same_samples = Vec::with_capacity(7);
    for _ in 0..7 {
        let start = Instant::now();
        let same = session.update(&same_inputs, &manager, options.clone())?;
        same_samples.push(start.elapsed().as_secs_f64() * 1000.0);
        if same.stats.paragraphs_reshaped != 0
            || same.stats.pages_recalculated != 0
            || same.stats.pagination_stream_work != 0
        {
            return Err("identical update unexpectedly recalculated work".into());
        }
    }

    let changed = paragraph_count / 2;
    let mut local_samples = Vec::with_capacity(7);
    let mut local_stats = None;
    for iteration in 0..7 {
        paragraphs[changed] = make_page(&format!("Edited {iteration} paragraph {changed:03}"));
        let local_inputs = inputs(&ids, &paragraphs, &rules, &properties);
        let start = Instant::now();
        let incremental = session.update(&local_inputs, &manager, options.clone())?;
        local_samples.push(start.elapsed().as_secs_f64() * 1000.0);
        local_stats = Some(incremental.stats.clone());
    }

    println!(
        "Workload: 300 pages with A4 usable height, {lines_per_page} lines per paragraph, {paragraph_count} paragraphs, 30,000-unit width"
    );
    println!(
        "Full initial: {:.3} ms; {} pages, {} reshaped, {} pagination stream work",
        full_elapsed.as_secs_f64() * 1000.0,
        initial_pages,
        initial_stats.paragraphs_reshaped,
        initial_stats.pagination_stream_work
    );
    println!(
        "Identical update: min {:.3} ms / median {:.3} ms over 7; shaping 0 reshaped / {} reused, pagination 0 recalculated / {} reused, preflight 0, stream 0",
        percentile(&same_samples, 0.0),
        percentile(&same_samples, 0.5),
        paragraph_count,
        initial_pages
    );
    let stats = local_stats.expect("at least one local sample");
    println!(
        "Local edit: min {:.3} ms / median {:.3} ms over 7; shaping {} reshaped / {} reused, pagination {} pages recalculated / {} reused, restart {:?}, fallback {:?}, preflight {}, stream {}",
        percentile(&local_samples, 0.0),
        percentile(&local_samples, 0.5),
        stats.paragraphs_reshaped,
        stats.paragraphs_reused,
        stats.pages_recalculated,
        stats.pages_reused,
        stats.restart_paragraph,
        stats.fallback_reason,
        stats.pagination_preflight_work,
        stats.pagination_stream_work
    );
    println!(
        "Timings are workload-specific and do not establish an interactive UI latency target."
    );
    Ok(())
}

fn percentile(samples: &[f64], percentile: f64) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[index]
}
