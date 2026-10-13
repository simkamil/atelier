# SPDX-License-Identifier: AGPL-3.0-or-later
"""Validate and export the full UI shell graph after explicit parent GO."""
from collections import defaultdict
from pathlib import Path
import hashlib
import json
import os
import re
import subprocess
import sys
import networkx as nx

from graphify.cargo_introspect import introspect_cargo
from graphify.export import to_html
from graphify.diagnostics import diagnose_extraction

if sys.argv[1:] not in (["--finalize"], ["--finalize-zoom"], ["--finalize-navigation"], ["--finalize-replacement"], ["--finalize-docx"], ["--finalize-docx-merged"], ["--finalize-docx-long"]):
    raise SystemExit("Usage: py ui-shell-graph-receipt.py --finalize (only after parent GO)")
zoom_run = sys.argv[1:] == ["--finalize-zoom"]
navigation_run = sys.argv[1:] == ["--finalize-navigation"]
replacement_run = sys.argv[1:] == ["--finalize-replacement"]
docx_run = sys.argv[1:] == ["--finalize-docx"]
docx_long_run = sys.argv[1:] == ["--finalize-docx-long"]
docx_merged_run = sys.argv[1:] == ["--finalize-docx-merged"]

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
OUT = ROOT / "graphify-out"
BASELINE = "0327fd1851d80394f7241339197d7ee2b2a9c582"
GRAPH_FILES = (
    "GRAPH_REPORT.md",
    "graph.json",
    "graph.html",
    "story-dependencies.json",
    "unresolved-structural-references.json",
)

raw = json.loads((OUT / "graph.json").read_text(encoding="utf-8"))
extraction = json.loads((OUT / ".graphify_extract.json").read_text(encoding="utf-8"))
edges = raw.get("links", raw.get("edges", []))
node_ids = {node["id"] for node in raw["nodes"]}
assert len(node_ids) == len(raw["nodes"]), "duplicate graph node IDs"
assert all(edge["source"] in node_ids and edge["target"] in node_ids for edge in edges)
us_text = (ROOT / "US-V0-V1.md").read_text(encoding="utf-8")
headings = list(re.finditer(r"^#### (US-\d+) — ([^\n]+)", us_text, re.M))
expected_status = {}
for index, match in enumerate(headings):
    block = us_text[match.end():headings[index + 1].start() if index + 1 < len(headings) else len(us_text)]
    checks = re.findall(r"^- \[([ x])\]", block, re.M)
    story_id = "us_v0_v1_" + match.group(1).lower().replace("-", "_")
    expected_status[story_id] = "done" if checks and all(value == "x" for value in checks) else "open"
stories = {node["id"]: node for node in raw["nodes"] if node["id"].startswith("us_v0_v1_us_")}
assert len(expected_status) == 158, (len(expected_status), "backlog story count")
assert set(stories) == set(expected_status), "graph story set differs from US-V0-V1.md"
status_mismatches = [key for key, value in expected_status.items() if stories[key].get("status") != value]
assert not status_mismatches, f"story statuses differ from backlog: {status_mismatches[:20]}"
done = sum(value == "done" for value in expected_status.values())
status_counts = {"done": done, "open": len(expected_status) - done}

# Ensure a code-only CLI refresh did not discard baseline semantic concepts
# grounded in documentation. The refresh helper rehydrates these from baseline.
baseline_bytes = subprocess.run(
    ["git", "show", f"{BASELINE}:graphify-out/graph.json"],
    cwd=ROOT,
    check=True,
    capture_output=True,
).stdout
baseline = json.loads(baseline_bytes)
baseline_docs = [
    node
    for node in baseline["nodes"]
    if node.get("_origin") == "semantic"
    and node.get("source_file", "").replace("\\", "/").startswith("docs/")
    and (ROOT / node["source_file"].replace("\\", "/")).is_file()
]
current_semantic = {
    node["id"]: node
    for node in raw["nodes"]
    if node.get("_origin") == "semantic"
}
missing_concepts = [
    node["id"]
    for node in baseline_docs
    if node["id"] not in current_semantic
    or current_semantic[node["id"]].get("source_file", "").replace("\\", "/")
    != node["source_file"].replace("\\", "/")
]
assert not missing_concepts, f"baseline documentation concepts were pruned: {missing_concepts[:20]}"
baseline_semantic_ids = {node["id"] for node in baseline_docs}
current_edge_keys = {
    (edge["source"], edge["target"], edge.get("relation")) for edge in edges
}
baseline_edge_keys = {
    (edge["source"], edge["target"], edge.get("relation"))
    for edge in baseline.get("links", baseline.get("edges", []))
    if edge["source"] in baseline_semantic_ids
    and edge["target"] in baseline_semantic_ids
}
missing_concept_edges = baseline_edge_keys - current_edge_keys
assert not missing_concept_edges, (
    f"baseline documentation concept edges were pruned: {sorted(missing_concept_edges)[:20]}"
)

# graph.json is canonical; preserve every exported node and edge in the HTML.
graph = nx.node_link_graph(raw, edges="links" if "links" in raw else "edges")
assert graph.number_of_nodes() == len(node_ids)
assert graph.number_of_edges() == len(edges)
html_limit = max(5000, ((len(node_ids) + 999) // 1000) * 1000)
os.environ["GRAPHIFY_VIZ_NODE_LIMIT"] = str(html_limit)
communities = defaultdict(list)
for node in raw["nodes"]:
    communities[node.get("community", 0)].append(node["id"])
labels = {
    int(key): value
    for key, value in json.loads((OUT / ".graphify_labels.json").read_text(encoding="utf-8")).items()
}
assert to_html(graph, dict(communities), str(OUT / "graph.html"), community_labels=labels)

health = diagnose_extraction(
    json.loads((OUT / ".graphify_extract.json").read_text(encoding="utf-8")),
    root=str(ROOT),
)
assert health["dangling_endpoint_edges"] == 0
assert health["missing_endpoint_edges"] == 0
cargo = introspect_cargo(ROOT)
adjacent = {node["id"]: [] for node in cargo["nodes"]}
for edge in cargo["edges"]:
    adjacent[edge["source"]].append(edge["target"])
visited = set()


def visit(node, active):
    assert node not in active, "Cargo dependency cycle"
    if node in visited:
        return
    for child in adjacent[node]:
        visit(child, active | {node})
    visited.add(node)


for node in adjacent:
    visit(node, set())
metadata = json.loads(subprocess.run(['cargo', 'metadata', '--format-version=1', '--no-deps', '--locked', '--offline'], cwd=ROOT, check=True, capture_output=True).stdout)
assert {node['label'] for node in cargo['nodes']} == {package['name'] for package in metadata['packages']}

missing_sources = [node['id'] for node in raw['nodes'] if node.get('source_file') and not (ROOT/node['source_file']).is_file()]
assert not missing_sources, f"missing located sources: {missing_sources[:20]}"
for url in ('/editor-shell.mjs', '/ui-components.mjs', '/ui-components.css', '/ribbon.mjs', '/ribbon.css', '/theme.mjs', '/theme.css') + (('/zoom.mjs', '/zoom-controls.mjs', '/zoom-controls.css') if zoom_run else ()) + (('/navigation-tools.mjs', '/search-pane.mjs', '/search-pane.css', '/statistics-controls.mjs', '/statistics-controls.css') if navigation_run else ()) + (('/page-navigation.mjs','/replace-controls.mjs','/replace-controls.css','/document-status.mjs','/document-status.css') if replacement_run else ()):
    route = 'http_route_' + re.sub(r'[^a-z0-9]+', '_', url.casefold()).strip('_')
    assert route in node_ids, f"missing UI route: {url}"
    # The simple graph merges multiple relations for one endpoint pair; the raw
    # extraction preserves both serves_embedded_bytes and serves_source_file.
    assert any(edge['source']==route and edge.get('relation')=='serves_embedded_bytes' for edge in extraction['edges'])
    assert any(route in (edge['source'],edge['target']) and edge.get('relation') in ('serves_embedded_bytes','serves_source_file') for edge in edges)

for name in GRAPH_FILES:
    path = OUT / name
    path.write_bytes(path.read_bytes().replace(b"\r\n", b"\n"))
cost_path = OUT / 'cost.json'
cost = json.loads(cost_path.read_text(encoding='utf-8')) if cost_path.is_file() else None
COST_BASELINE = "94df23fb037145d75542f54dbc0215d0daf929bc"
baseline_report = subprocess.run(['git', 'show', f'{COST_BASELINE}:graphify-out/GRAPH_REPORT.md'], cwd=ROOT, check=True, capture_output=True, text=True, encoding='utf-8').stdout
historical = re.search(r'Coût CLI cumulé enregistré : (\d+) tokens en entrée et (\d+) en sortie', baseline_report)
evidence = {
    "baseline_commit": BASELINE,
    "nodes": len(node_ids),
    "edges": len(edges),
    "stories": len(stories),
    "done": done,
    "baseline_document_semantic_nodes": len(baseline_docs),
    "baseline_document_semantic_edges": len(baseline_edge_keys),
    "html_nodes": graph.number_of_nodes(),
    "html_node_limit": html_limit,
    "dangling_edges": health["dangling_endpoint_edges"],
    "missing_endpoint_edges": health["missing_endpoint_edges"],
    "same_endpoint_groups_preserved_in_raw_extract": health["same_endpoint_group_count"],
    "cargo_packages": len(cargo["nodes"]),
    "cargo_dependency_edges": len(cargo["edges"]),
    "cargo_dependency_cycles": 0,
    "cli_refreshed": True,
    "recorded_cumulative_semantic_tokens": {"input": cost['total_input_tokens'], "output": cost['total_output_tokens']} if cost else None,
    "historical_baseline_reported_tokens": {"input": int(historical[1]), "output": int(historical[2])} if historical else None,
    "historical_token_provenance": f"{COST_BASELINE}:graphify-out/GRAPH_REPORT.md",
    "local_cost_meter_available": cost is not None,
    "this_final_cli_refresh_llm_tokens": 0,
    "unreported_semantic_runs": len(cost.get("unreported_runs", [])) if cost else None,
    "api_report_llm_tokens": 0,
    "ui_stories": {key: stories['us_v0_v1_us_'+key].get('status') for key in ('104', '105', '106')},
    "replacement_story_status": {key: stories['us_v0_v1_us_'+key].get('status') for key in ('341','344')} if replacement_run else None,
    "zoom_story_status": stories['us_v0_v1_us_342'].get('status') if zoom_run else None,
    "navigation_story_status": {key: stories['us_v0_v1_us_'+key].get('status') for key in ('340','343')} if navigation_run else None,
    "missing_located_sources": len(missing_sources),
    "ui_latency_certified": False,
    "graph_sha256": {
        f"graphify-out/{name}": hashlib.sha256((OUT / name).read_bytes()).hexdigest()
        for name in GRAPH_FILES
    },
    "graph_tools_sha256": {
        f"_bmad-output/implementation-artifacts/{name}": hashlib.sha256((ART / name).read_bytes()).hexdigest()
        for name in ("merged-branches-graph-refresh.py", "merged-branches-graph-receipt.py")
    },
}
(ART / ("docx-long-guides-graph.json" if docx_long_run else "docx-theme-merged-graph.json" if docx_merged_run else "docx-theme-graph.json" if docx_run else "replacement-status-graph.json" if replacement_run else "navigation-graph.json" if navigation_run else "zoom-graph.json" if zoom_run else "merged-branches-graph.json")).write_text(
    json.dumps(evidence, ensure_ascii=False, indent=2) + "\n",
    encoding="utf-8",
    newline="\n",
)
print(json.dumps(evidence, ensure_ascii=False))
