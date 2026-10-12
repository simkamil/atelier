# SPDX-License-Identifier: AGPL-3.0-or-later
"""Trace every frozen matrix row to executed tests or explicit process evidence."""
from pathlib import Path
import hashlib,json,re

ROOT=Path(__file__).resolve().parents[2]
ART=Path(__file__).resolve().parent
report=json.loads((ART/'rich-page-geometry-validation.json').read_text(encoding='utf-8'))
assert report['passed'] and report['source_bytes_unchanged'] and report['lock_bytes_unchanged']
checks={c['name']:c for c in report['checks']}
executed=set(checks['windows_tests']['tests'])
release=set(checks['layout_release_tests']['tests'])
mapping={
 'spec-us211-tabs.md': {
  'Défaut':['default_tab_uses_2268_grid_and_keeps_terminal_control_source','default_tab_grid_accounts_for_first_line_and_hanging_indents','tab_only_lines_retain_default_and_resolved_font_metrics'],
  'Alignements':['explicit_right_decimal_and_leader_stops_have_numeric_positions','center_and_decimal_without_comma_align_by_segment_width'],
  'Décorations':['bars_are_fixed_and_invalid_settings_fail_without_partial_layout','all_leader_kinds_cover_the_consumed_gap_and_respect_decoration_limits'],
  'Sources':['tabs_survive_wrapping_with_contiguous_utf8_sources','default_tab_uses_2268_grid_and_keeps_terminal_control_source','line_wrapping_does_not_split_a_shaping_cluster','mandatory_lines_without_tabs_respect_a_tight_range_scan_budget'],
  'Marge':['tabs_survive_wrapping_with_contiguous_utf8_sources','custom_stops_precede_the_default_grid_then_automatic_stops_resume'],
  'Malformé':['bars_are_fixed_and_invalid_settings_fail_without_partial_layout'],
 },
 'spec-us212-pagination.md': {
  'Espacements':['normal_and_contextual_spacing_use_resolved_maximum'],
  'Page suivante':['automatic_page_top_drops_spacing_but_explicit_break_preserves_it','keep_together_moves_intact_and_baseline_uses_each_lines_input_offset','break_after_survives_empty_paragraphs_without_creating_a_trailing_page'],
  'Solidarité':['keep_with_next_moves_a_fitting_chain_and_reports_an_impossible_one','keep_together_moves_intact_and_baseline_uses_each_lines_input_offset','widow_rule_moves_a_line_when_the_previous_page_can_keep_its_minimum','five_short_lines_split_three_and_two_without_relaxation','explicit_breaks_take_precedence_over_keep_with_next'],
  'Sauts':['manual_line_break_splits_exactly_once_and_page_ranges_are_contiguous','break_after_survives_empty_paragraphs_without_creating_a_trailing_page','widow_backtracking_never_crosses_an_explicit_manual_break'],
  'Cas extrêmes':['overheight_line_and_unavoidable_widow_are_reported_without_losing_lines','keep_with_next_moves_a_fitting_chain_and_reports_an_impossible_one','automatic_page_top_drops_spacing_but_explicit_break_preserves_it','many_single_line_pages_conserve_sources_under_a_linear_work_budget'],
  'Malformé':['malformed_inputs_and_limits_return_typed_errors'],
 },
 'spec-us214-caret-geometry.md': {
  'Position et point':['hit_test_chooses_the_y_containing_line_before_considering_x','real_tabbed_layout_paginates_and_maps_carets_between_pages','invalid_ranges_geometry_and_exhausted_budgets_fail_without_partial_results'],
  'Unicode':['combining_grapheme_rejects_internal_byte_and_ligature_stops_are_interpolated','same_source_multiglyph_cluster_aggregates_advance_once','independent_glyph_advances_are_not_interpolated_as_one_cluster'],
  'Fin de ligne':['trailing_spaces_have_distinct_zero_advance_positions_and_selection_keeps_pages','crlf_before_and_after_map_to_previous_and_following_lines','terminal_break_needs_an_empty_following_line_and_line_boxes_must_not_overlap'],
  'Sélection':['real_tabbed_layout_paginates_and_maps_carets_between_pages','trailing_spaces_have_distinct_zero_advance_positions_and_selection_keeps_pages','included_paragraph_spacing_is_counted_once_across_the_pipeline'],
  'Vertical':['vertical_motion_retains_preferred_column_across_a_short_line','affinity_aware_upward_moves_keep_the_preceding_soft_wrap_line'],
  'Malformé':['invalid_ranges_geometry_and_exhausted_budgets_fail_without_partial_results','adapter_rejects_forged_page_ranges_and_source_indices'],
 },
 'spec-rich-page-geometry-integration.md': {
  'Chaîne complète':['real_tabbed_layout_paginates_and_maps_carets_between_pages'],
  'Espacement':['included_paragraph_spacing_is_counted_once_across_the_pipeline'],
  'Sélection':['real_tabbed_layout_paginates_and_maps_carets_between_pages','vertical_motion_retains_preferred_column_across_a_short_line'],
  'Régression':['independently_packaged_docx_resolves_styles_and_embedded_font_into_lines'],
  'Publication':[],
 },
}
rows=[]
for spec,scenarios in mapping.items():
    text=(ART/spec).read_text(encoding='utf-8').split('## I/O & Edge-Case Matrix',1)[1].split('</frozen-after-approval>',1)[0]
    actual=[line.split('|')[1].strip() for line in text.splitlines() if line.startswith('| ')][1:]
    assert set(actual)==set(scenarios),(spec,actual)
    for scenario,tests in scenarios.items():
        for test in tests:
            assert test in executed and test in release,(scenario,test)
        row={'spec':spec,'scenario':scenario,'executed_windows_and_release_tests':tests,
             'status':'functional_verified' if tests else 'process_verified'}
        if spec=='spec-us211-tabs.md' and scenario=='Marge':
            row.update(status='deterministic_behavior_verified_word_comparison_open',word_verified=False)
        if spec.endswith('integration.md') and scenario=='Régression':
            row['additional_gates']=['docx_reference_reproducible','packaged_engine','editor_real_abi','worker_real_transfer','editor_browser']
        if scenario=='Publication':
            row['evidence']=['rich-page-geometry-validation.json','rich-page-geometry-review.md','rich-page-geometry-graph.json']
            row['limitations']=['Word fidelity remains unmeasured','Rich library APIs are not yet connected to the UI','musl tests compiled, not executed locally']
        rows.append(row)
record={'date':'2026-10-12','matrix_rows':len(rows),'all_rows_traced':True,
        'runtime_tests_actually_executed':True,'validation_sha256':hashlib.sha256((ART/'rich-page-geometry-validation.json').read_bytes()).hexdigest(),
        'word_fidelity_established':False,'rows':rows}
(ART/'rich-page-geometry-matrix.json').write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps({'rows':len(rows),'all_rows_traced':True,'word_fidelity_established':False}))
