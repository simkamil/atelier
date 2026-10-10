# SPDX-License-Identifier: AGPL-3.0-or-later
"""Read this repository's CI using Git's existing credential, without logging it."""
import json
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

root = Path(__file__).resolve().parents[2]
repo = "simkamil/mon-bureau"
head = sys.argv[1] if len(sys.argv) > 1 else subprocess.check_output(
    ["git", "rev-parse", "origin/main"], cwd=root, text=True
).strip()
credential = subprocess.run(
    ["git", "credential", "fill"], cwd=root,
    input=f"url=https://github.com/{repo}.git\n\n", text=True,
    capture_output=True, check=False,
)
fields = dict(line.split("=", 1) for line in credential.stdout.splitlines() if "=" in line)
token = fields.get("password")
if credential.returncode or not token:
    raise SystemExit("GitHub CI: existing Git credential unavailable.")

def get(path):
    request = urllib.request.Request(
        f"https://api.github.com/repos/{repo}/{path}",
        headers={"Accept": "application/vnd.github+json", "User-Agent": "Atelier-Codex",
                 "X-GitHub-Api-Version": "2022-11-28", "Authorization": f"Bearer {token}"},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        raise SystemExit(f"GitHub CI: HTTP {error.code}.") from None
    except urllib.error.URLError:
        raise SystemExit("GitHub CI: network request failed.") from None

runs = get("actions/runs?per_page=20")
result = {"repository": repo, "head_sha": head, "runs": []}
for run in runs["workflow_runs"]:
    if run["head_sha"] != head:
        continue
    record = {key: run.get(key) for key in ("id", "name", "status", "conclusion", "html_url", "updated_at")}
    record["jobs"] = []
    for job in get(f"actions/runs/{run['id']}/jobs?per_page=100")["jobs"]:
        record["jobs"].append({key: job.get(key) for key in ("id", "name", "status", "conclusion", "html_url", "steps")})
    result["runs"].append(record)
destination = Path(__file__).with_name("github-ci.json")
destination.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
print(json.dumps(result, ensure_ascii=False))
