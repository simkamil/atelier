# SPDX-License-Identifier: AGPL-3.0-or-later
"""Validate the refreshed editor graph and record its exact inventory."""
from collections import defaultdict
from pathlib import Path
import hashlib
import json
import os
import subprocess
import networkx as nx

from graphify.cargo_introspect import introspect_cargo
from graphify.export import to_html
from graphify.diagnostics import diagnose_extraction

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
OUT = ROOT / "graphify-out"
BASELINE = "3fc5d860e673fc683f2fb92bcc73f1d621556286"
GRAPH_FILES = (
    "GRAPH_REPORT.md",
    "graph.json",
    "graph.html",
    "story-dependencies.json",
    "unresolved-structural-references.json",
)

raw = json.loads((OUT / "graph.json").read_text(encoding="utf-8"))
edges = raw.get("links", raw.get("edges", []))
node_ids = {node["id"] for node in raw["nodes"]}
assert len(node_ids) == len(raw["nodes"]), "duplicate graph node IDs"
assert all(edge["source"] in node_ids and edge["target"] in node_ids for edge in edges)
stories = [node for node in raw["nodes"] if node["id"].startswith("us_v0_v1_us_")]
done = sum(node.get("status") == "done" for node in stories)
assert len(stories) == 158 and done == 48, (len(stories), done)

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
assert len(cargo["nodes"]) == 33

benchmark_path = ART / "editor-incremental-perf-benchmark.json"
benchmark = json.loads(benchmark_path.read_text(encoding="utf-8"))
assert benchmark["workload"]["pages"] == 300
assert benchmark["workload"]["lines_per_paragraph"] == 52
assert len(benchmark["after_optimization"]) >= 1
assert all(
    not result.get("target_16_ms_met", False)
    for result in benchmark["after_optimization"]
)

for name in GRAPH_FILES:
    path = OUT / name
    path.write_bytes(path.read_bytes().replace(b"\r\n", b"\n"))
cost = json.loads((OUT / "cost.json").read_text(encoding="utf-8"))
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
    "recorded_cumulative_semantic_tokens": {
        "input": cost["total_input_tokens"],
        "output": cost["total_output_tokens"],
    },
    "this_final_cli_refresh_llm_tokens": 0,
    "unreported_semantic_runs": len(cost.get("unreported_runs", [])),
    "api_report_llm_tokens": 0,
    "incremental_benchmark": "_bmad-output/implementation-artifacts/editor-incremental-perf-benchmark.json",
    "ui_latency_certified": False,
    "graph_sha256": {
        f"graphify-out/{name}": hashlib.sha256((OUT / name).read_bytes()).hexdigest()
        for name in GRAPH_FILES
    },
}
(ART / "rich-docx-preview-graph.json").write_text(
    json.dumps(evidence, ensure_ascii=False, indent=2) + "\n",
    encoding="utf-8",
    newline="\n",
)
print(json.dumps(evidence, ensure_ascii=False))
