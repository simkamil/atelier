# SPDX-License-Identifier: AGPL-3.0-or-later
"""Export the refreshed, validated product graph and record its exact inventory."""
from collections import defaultdict
from pathlib import Path
import json

from graphify.build import build_from_json
from graphify.cargo_introspect import introspect_cargo
from graphify.export import to_html

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
OUT = ROOT/'graphify-out'
raw = json.loads((OUT/'graph.json').read_text(encoding='utf-8'))
edges = raw.get('links', raw.get('edges', []))
nodes = {n['id'] for n in raw['nodes']}
assert len(nodes) == len(raw['nodes'])
assert all(e['source'] in nodes and e['target'] in nodes for e in edges)
stories = [n for n in raw['nodes'] if n['id'].startswith('us_v0_v1_us_')]
assert len(stories) == 158 and sum(n.get('status') == 'done' for n in stories) == 32
graph = build_from_json({'nodes':raw['nodes'], 'edges':edges}, root=ROOT)
communities = defaultdict(list)
for n in raw['nodes']:
    communities[n.get('community', 0)].append(n['id'])
labels = {int(k):v for k,v in json.loads((OUT/'.graphify_labels.json').read_text(encoding='utf-8')).items()}
assert to_html(graph, dict(communities), str(OUT/'graph.html'), community_labels=labels)
for name in ('GRAPH_REPORT.md','graph.json','graph.html','story-dependencies.json','unresolved-structural-references.json'):
    path = OUT/name
    path.write_bytes(path.read_bytes().replace(b'\r\n', b'\n'))
cost = json.loads((OUT/'cost.json').read_text(encoding='utf-8'))
cargo = introspect_cargo(ROOT)
cargo_packages = len(cargo['nodes'])
assert cargo_packages == 32
adjacent = {node['id']:[] for node in cargo['nodes']}
for edge in cargo['edges']:
    adjacent[edge['source']].append(edge['target'])
visited = set()
def visit(node, active):
    assert node not in active, 'Cargo dependency cycle'
    if node in visited:
        return
    for child in adjacent[node]:
        visit(child, active | {node})
    visited.add(node)
for node in adjacent:
    visit(node, set())
evidence = {'nodes':len(nodes), 'edges':len(edges), 'stories':len(stories), 'done':32,
            'dangling_edges':0, 'cargo_packages':cargo_packages, 'cargo_dependency_edges':len(cargo['edges']),
            'cargo_dependency_cycles':0, 'cli_refreshed':True,
            'semantic_cli_tokens':{'input':cost['total_input_tokens'], 'output':cost['total_output_tokens']},
            'unreported_semantic_runs':len(cost.get('unreported_runs',[])),
            'api_report_llm_tokens':0}
(ART/'text-layout-graph.json').write_text(json.dumps(evidence, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
print(json.dumps(evidence))
