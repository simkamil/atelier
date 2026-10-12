# SPDX-License-Identifier: AGPL-3.0-or-later
"""Trace frozen editor matrices to test executions recorded by final validation."""
from pathlib import Path
import hashlib
import json
import re

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
REPORT = ART / 'editor-drag-spacing-perf-validation.json'
report = json.loads(REPORT.read_text(encoding='utf-8'))
assert report['passed'] and report['source_bytes_unchanged'] and report['lock_bytes_unchanged'], 'final validation must pass before auditing'
checks = {entry['name']: entry for entry in report['checks']}
windows = set(checks['windows_tests']['tests'])
release = set(checks['layout_release_tests']['tests'])
modules = set(checks['frontend_helpers']['test_modules'])
assert checks['frontend_helpers']['exit_code'] == 0
assert checks['editor_browser']['exit_code'] == 0
assert checks['editor_real_abi']['exit_code'] == 0
executed_commands = {name: entry['argv'] for name, entry in checks.items()}
browser_assertions = set(checks['editor_browser']['assertions'])

drag = {
    'move': (['move_drop_before_and_after_selection_preserves_unicode_crlf_and_bom', 'drop_move_before_after_and_copy_inside_are_single_reversible_transactions'], ['tests/text-drag-drop.mjs'], ['drop indicator is a visible two-pixel insertion line', 'one undo restores moved UTF-8 text and original directed selection']),
    'copy': (['move_inside_or_onto_source_is_an_exact_noop_but_copy_inside_inserts', 'drop_move_before_after_and_copy_inside_are_single_reversible_transactions'], ['tests/text-drag-drop.mjs'], ['Ctrl copy is a single undoable edit']),
    'inside': (['move_inside_or_onto_source_is_an_exact_noop_but_copy_inside_inserts'], ['tests/text-drag-drop.mjs'], []),
    'external': (['external_drop_uses_source_zero_and_inserts_at_target'], ['tests/text-drag-drop.mjs'], ['external drop exports exact Unicode CRLF and literal markup without typography correction', 'one undo restores the original document and selection after external insertion']),
    'marker': ([], ['tests/text-drag-drop.mjs'], ['drag position probe preserves the selection revision and text', 'drop indicator is a visible two-pixel insertion line', 'drop clears insertion marker']),
    'selection': ([], ['tests/mouse-selection.mjs', 'tests/text-drag-drop.mjs'], ['double-click selects a whole accented word through engine unit selection']),
    'lifecycle': ([], ['tests/text-drag-drop.mjs'], ['Escape cancels the complete drag without mutation or residual marker']),
    'stale': (['stale_or_malformed_commands_fail_without_publishing_state'], ['tests/text-drag-drop.mjs'], ['drag position probe preserves the selection revision and text']),
    'malformed': (['stale_or_malformed_commands_fail_without_publishing_state'], [('editor_real_abi', 'tests/editor-abi.mjs')], []),
}
spacing = {
    'punctuation': (['isolated_replacement_converts_ascii_space_and_restores_directed_selection', 'french_spacing_replacement_undo_keeps_raw_punctuation_then_redoes_spacing'], ['tests/french-spacing.mjs'], ['French correction keeps a UTF-8 caret after punctuation']),
    'quotes': (['french_spacing_replacement_undo_keeps_raw_punctuation_then_redoes_spacing'], ['tests/french-spacing.mjs'], ['French quote interiors use nonbreaking spaces']),
    'unicode': (['isolated_replacement_converts_ascii_space_and_restores_directed_selection'], ['tests/french-spacing.mjs'], ['French correction keeps a UTF-8 caret after punctuation']),
    'boundaries': ([], ['tests/french-spacing.mjs'], []),
    'disabled': ([], ['tests/french-spacing.mjs'], ['French spacing can be disabled for the current editor']),
    'exempt': ([], ['tests/french-spacing.mjs'], ['pasted punctuation and guillemets remain exact', 'composition punctuation stays exact with automatic spacing enabled', 'replacing a selection does not add an automatic transaction']),
    'undo': (['isolated_replacement_undo_redo_keeps_typing_on_separate_groups', 'french_spacing_replacement_undo_keeps_raw_punctuation_then_redoes_spacing'], ['tests/french-spacing.mjs'], ['immediate undo removes only automatic spacing and keeps raw punctuation', 'redo restores automatic spacing', 'subsequent typing stays outside substitution undo group']),
    'failure': (['invalid_offsets_and_budget_or_text_limit_fail_without_state_changes', 'stale_or_malformed_commands_fail_without_publishing_state'], [('editor_real_abi', 'tests/editor-abi.mjs')], []),
}
perf = {
    'same': (['identical_update_reuses_layouts_and_pagination_without_stream_work'], [], []),
    'local': (['local_edit_in_a_300_page_document_stops_at_a_real_stable_boundary', 'deterministic_mixed_edit_sequence_matches_fresh_shaping_and_pagination'], [], []),
    'global': (['global_invalidations_are_reported_and_structure_reuses_unchanged_geometry', 'global_tabs_invalidate_geometry_and_rule_changes_match_full_pagination', 'same_manager_font_mutations_invalidate_only_after_success'], [], []),
    'invalid': (['failures_and_duplicate_ids_leave_the_last_published_result_usable', 'layout_and_pagination_failures_preserve_published_state_and_cache'], [], []),
    'benchmark': ([], [], []),
}

test_sources = {}
for path in (ROOT / 'crates').rglob('*.rs'):
    source = path.read_text(encoding='utf-8')
    for name in re.findall(r'#\[test\]\s*fn\s+(\w+)\(', source):
        test_sources.setdefault(name, []).append(path.relative_to(ROOT).as_posix())

def read_matrix(filename):
    text = (ART / filename).read_text(encoding='utf-8').split('## I/O & Edge-Case Matrix', 1)[1]
    text = text.split('</frozen-after-approval>', 1)[0]
    return [line.split('|')[1].strip() for line in text.splitlines() if line.startswith('| ')][1:]

rows = []
specs = [('editor-drag-spec.md', drag), ('editor-spacing-spec.md', spacing), ('editor-incremental-perf-spec.md', perf)]
for filename, mapping in specs:
    assert set(read_matrix(filename)) == set(mapping), (filename, read_matrix(filename), list(mapping))
    for scenario, (rust_names, js_modules, browser_terms) in mapping.items():
        evidence = {}
        for name in rust_names:
            assert name in windows, (scenario, name, 'not executed in windows_tests')
            assert name in test_sources, (scenario, name, 'source test missing')
            evidence[name] = test_sources[name]
        layout_tests = [name for name in rust_names if any(p.startswith('crates/text-layout/') for p in test_sources[name])]
        for name in layout_tests:
            assert name in release, (scenario, name, 'not executed in layout_release_tests')
        executed_modules = []
        for module in js_modules:
            if isinstance(module, tuple):
                gate, path = module
                assert gate in executed_commands and path in executed_commands[gate], (scenario, gate, path, 'not executed')
                executed_modules.append({'gate': gate, 'module': path})
            else:
                assert module in modules, (scenario, module, 'not executed in frontend_helpers')
                executed_modules.append({'gate': 'frontend_helpers', 'module': module})
        browser_matches = sorted(item for item in browser_assertions if item in browser_terms)
        assert set(browser_terms) <= browser_assertions, (scenario, set(browser_terms) - browser_assertions, 'browser assertions not recorded as executed')
        row = {'spec': filename, 'scenario': scenario, 'executed_windows_tests': rust_names,
               'executed_release_tests': layout_tests, 'test_sources': evidence,
               'executed_node_modules': executed_modules, 'executed_browser_assertions': browser_matches,
               'status': 'functional_verified' if (rust_names or executed_modules or browser_matches) else 'benchmark_verified'}
        if scenario == 'benchmark':
            bench = checks['incremental_benchmark']
            assert bench['exit_code'] == 0
            data_path = ART / 'editor-incremental-perf-benchmark.json'
            data = json.loads(data_path.read_text(encoding='utf-8'))
            row.update(evidence=['editor-incremental-perf-benchmark.json', 'editor-drag-spacing-perf-validation.json'],
                       benchmark_command=bench['argv'], benchmark_pages=300, benchmark_lines_per_paragraph=52,
                       benchmark_data_sha256=hashlib.sha256(data_path.read_bytes()).hexdigest(),
                       limitation='CPU timings do not certify 16 ms interactive typing latency.')
        rows.append(row)

record = {'date': report['date'], 'matrix_rows': len(rows), 'all_rows_traced': True,
          'runtime_tests_actually_executed': True,
          'validation_sha256': hashlib.sha256(REPORT.read_bytes()).hexdigest(),
          'native_windows_drop_certified': False, 'ui_16ms_certified': False,
          'limitations': ['DOM drag/drop events are synthetic; native Windows interapplication drop remains untested.',
                          'Release CPU benchmark does not certify 16 ms UI typing latency.'],
          'rows': rows}
output = ART / 'editor-drag-spacing-perf-matrix.json'
output.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8', newline='\n')
print(json.dumps({'rows': len(rows), 'all_rows_traced': True, 'native_windows_drop_certified': False, 'ui_16ms_certified': False}))
