# SPDX-License-Identifier: AGPL-3.0-or-later
"""Trace frozen behavior rows to source tests actually executed by final gates."""
from pathlib import Path
import hashlib,json,re

ROOT=Path(__file__).resolve().parents[2]
ART=Path(__file__).resolve().parent
report=json.loads((ART/'rich-incremental-borders-validation.json').read_text(encoding='utf-8'))
assert report['passed'] and report['source_bytes_unchanged'] and report['lock_bytes_unchanged']
checks={c['name']:c for c in report['checks']}
executed=set(checks['windows_tests']['tests'])
release=set(checks['layout_release_tests']['tests'])
mapping={
 'spec-us202-default-document.md':{
  'Fabrique':['factory_builds_resolvable_default_document_and_exact_metadata'],
  'Géométrie':['factory_builds_resolvable_default_document_and_exact_metadata'],
  'Résolution':['factory_builds_resolvable_default_document_and_exact_metadata'],
  'Catalogue':['catalog_has_all_required_french_names_kinds_and_valid_next_targets'],
  'Suite':['catalog_has_all_required_french_names_kinds_and_valid_next_targets'],
  'Sans interligne':['no_spacing_is_independent_and_factory_instances_do_not_share_mutations'],
  'Isolation':['no_spacing_is_independent_and_factory_instances_do_not_share_mutations'],
 },
 'spec-us213-incremental-layout.md':{
  'Initial / identique':['identical_update_reuses_layouts_and_pagination_without_stream_work'],
  'Frappe locale':['local_edit_in_a_300_page_document_stops_at_a_real_stable_boundary'],
  'Propagation':['deterministic_mixed_edit_sequence_matches_fresh_shaping_and_pagination','global_tabs_invalidate_geometry_and_rule_changes_match_full_pagination'],
  'Contraintes':['deterministic_mixed_edit_sequence_matches_fresh_shaping_and_pagination'],
   'Cache':['global_invalidations_are_reported_and_structure_reuses_unchanged_geometry','global_tabs_invalidate_geometry_and_rule_changes_match_full_pagination','local_edit_with_global_pagination_change_uses_a_full_declared_fallback','same_manager_font_mutations_invalidate_only_after_success','resolved_property_edit_reshapes_only_its_paragraph_and_matches_fresh'],
   'Structure':['global_invalidations_are_reported_and_structure_reuses_unchanged_geometry','failures_and_duplicate_ids_leave_the_last_published_result_usable','insert_and_delete_keep_stable_id_layouts_and_match_fresh_results'],
   'Erreur':['failures_and_duplicate_ids_leave_the_last_published_result_usable','layout_and_pagination_failures_preserve_published_state_and_cache'],
  'Mesure':[],
 },
 'spec-us215-paragraph-borders.md':{
   'Cadre isolé':['isolated_frame_uses_explicit_column_bounds_and_border_spacing','frame_horizontal_edges_reach_side_centerlines_with_different_spacings'],
   'Cadre partagé':['adjacent_equal_borders_share_frame_but_keep_independent_shading','between_edge_checks_exact_and_insufficient_spacing_envelopes'],
  'Trames distinctes':['adjacent_equal_borders_share_frame_but_keep_independent_shading'],
   'Ruptures':['no_border_zero_line_or_changed_column_interrupts_a_group','changed_border_color_or_spacing_separates_adjacent_frames'],
  'Multipage':['a_group_split_across_pages_gets_closed_local_frames_and_no_intra_paragraph_between'],
  'Styles':['solid_double_dashed_and_dotted_styles_have_bounded_deterministic_geometry','circle_diameter_does_not_truncate_i64_and_geometry_errors_are_typed'],
  'Rendu':['adapted_commands_round_trip_and_render_as_svg_in_paint_order','fill_paths_use_nonzero_closed_rectangles'],
   'Malformé':['malformed_placements_source_order_and_dash_budgets_return_errors','malformed_inputs_and_resource_ceilings_return_typed_errors','absent_page_and_malformed_primitives_fail_without_returning_commands'],
   'Limites':['malformed_placements_source_order_and_dash_budgets_return_errors','malformed_inputs_and_resource_ceilings_return_typed_errors'],
 },
 'spec-rich-incremental-borders.md':{
  'Révisions':['cache_tokens_distinguish_managers_and_survive_moves','successful_mutations_renew_tokens_and_failures_preserve_them','concurrent_manager_creation_does_not_alias_tokens'],
  'Pipeline':['default_profile_incremental_pages_and_border_display_list_share_geometry'],
  'Architecture':[],
  'Livraison':[],
 },
}
test_sources={}
for path in (ROOT/'crates').rglob('*.rs'):
    text=path.read_text(encoding='utf-8')
    for name in re.findall(r'#\[test\]\s*fn\s+(\w+)\(',text):
        test_sources.setdefault(name,[]).append(path.relative_to(ROOT).as_posix())
rows=[]
for spec,scenarios in mapping.items():
    text=(ART/spec).read_text(encoding='utf-8').split('## I/O & Edge-Case Matrix',1)[1].split('</frozen-after-approval>',1)[0]
    actual=[line.split('|')[1].strip() for line in text.splitlines() if line.startswith('| ')][1:]
    assert set(actual)==set(scenarios),(spec,actual)
    for scenario,tests in scenarios.items():
        paths={}
        release_tests=[]
        for test in tests:
            assert test in executed,(scenario,test,'not executed')
            assert test in test_sources,(scenario,test,'no source test')
            paths[test]=test_sources[test]
            if any(p.startswith('crates/text-layout/') for p in paths[test]):
                assert test in release,(scenario,test,'no release execution')
                release_tests.append(test)
        row={'spec':spec,'scenario':scenario,'executed_windows_tests':tests,'executed_release_tests':release_tests,'test_sources':paths,'status':'functional_verified' if tests else 'process_verified'}
        if scenario=='Mesure':
            assert checks['incremental_benchmark']['exit_code']==0
            row.update(evidence=['rich-incremental-borders-benchmark.json','rich-incremental-borders-host.json','rich-incremental-borders-validation.json'],scope='CPU workload only; UI typing latency/reference PC not certified')
        if scenario=='Architecture':
            graph=json.loads((ART/'rich-incremental-borders-graph.json').read_text(encoding='utf-8'))
            assert graph['cargo_dependency_cycles']==0
            row['evidence']=['rich-incremental-borders-graph.json','rich-incremental-borders-impact.json']
        if scenario=='Livraison':
            row['evidence']=['rich-incremental-borders-validation.json','rich-incremental-borders-review.md']
        rows.append(row)
record={'date':'2026-10-12','matrix_rows':len(rows),'all_rows_traced':True,'runtime_tests_actually_executed':True,
 'validation_sha256':hashlib.sha256((ART/'rich-incremental-borders-validation.json').read_bytes()).hexdigest(),
 'word_fidelity_established':False,'ui_latency_certified':False,'rows':rows}
(ART/'rich-incremental-borders-matrix.json').write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps({'rows':len(rows),'all_rows_traced':True,'word_fidelity_established':False,'ui_latency_certified':False}))
