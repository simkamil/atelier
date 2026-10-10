# SPDX-License-Identifier: AGPL-3.0-or-later
"""Snapshot the batch and prepare independent review inputs; agent-only."""
from pathlib import Path
import hashlib
import json
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
RENDER = ROOT/'_bmad/render/bmad-build/mon-bureau-a5c5c221806f/0ed830b5d74d152590be'
BASE = 'ff7f3a3097ae23792afda188c3b5d0b739b76b5d'
SPECS = [ART/'spec-us022-023-zip.md', ART/'spec-us030-031-xml.md']

def git(*args):
    result = subprocess.run(['git', *args], cwd=ROOT, capture_output=True)
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr.decode('utf-8', errors='replace'))
    return result.stdout

def main():
    standalone = '--local' not in sys.argv[1:]
    # The reviewers must not see claims before their instructed claims pass.
    # Specs and generated review packets are review inputs, not reviewed code.
    excluded = {p.relative_to(ROOT).as_posix() for p in SPECS}
    paths = set(git('diff', '--name-only', '-z', BASE).decode().split('\0'))
    paths.update(git('ls-files', '--others', '--exclude-standard', '-z').decode().split('\0'))
    paths.discard('')
    reviewed = sorted(p for p in paths if p not in excluded and not Path(p).name.startswith('formats-review-'))
    tracked = set(git('ls-files', '-z').decode().split('\0'))
    parts = []
    for path in reviewed:
        if path in tracked:
            parts.append(git('diff', '--binary', BASE, '--', path))
        else:
            parts.append(git('diff', '--no-index', '--binary', '--', '/dev/null', path))
    diff = b''.join(parts).decode('utf-8')
    diff_file = ART/'formats-review-diff.txt'
    diff_file.write_text(diff, encoding='utf-8', newline='\n')
    claims = '\n\n'.join(p.read_text(encoding='utf-8') for p in SPECS)
    claims_file = ART/'formats-review-claims.md'
    claims_file.write_text(claims, encoding='utf-8')
    prompts = {}
    prompts['blind'] = f'''Conduct a review of CONTENT.
Look for what's missing, not only what's wrong.
Compute your finding floor N from the diff file's size: N = min(floor(sqrt(kB) + 1), 10), where kB is the file's size in kilobytes. State the arithmetic in one line, then find at least N issues to fix or improve.
Output a Markdown list of findings only — no severity, priority, or ranking.
If the content is empty, stop and say so.
If you have zero findings, re-check and keep thinking; do not stop with an empty list.

CONTENT: the unified diff at `{diff_file}`. Read that file — it is the content under review.

Do not invoke any skill, and do not spawn subagents of your own — you are the reviewer. Return your findings as text in your final message; do not route them through any findings-reporting tool the host may offer.
'''
    for layer, filename in [('edge', 'edge-case-hunter.md'), ('verification', 'verification-gap.md')]:
        instructions = (RENDER/'review-prompts'/filename).read_text(encoding='utf-8')
        claims_line = f'claims_file (leave unread until your instructions call for it): {claims_file}\n\n' if layer == 'edge' else ''
        prompts[layer] = f'''Read `{instructions}` completely and follow it as your review instructions.

{claims_line}Review content: the unified diff at `{diff_file}`. Read that file — it is the content under review.

Do not invoke any skill, and do not spawn subagents of your own — you are the reviewer. If the instruction file is unreadable, report that exact failure and stop. Return your findings as text in your final message; do not route them through any findings-reporting tool the host may offer.
'''
    if standalone:
        for layer, prompt in prompts.items():
            (ART/f'formats-review-{layer}.md').write_text(prompt, encoding='utf-8')
    source_paths = ['crates/xml/src/lib.rs', 'crates/zip/src/lib.rs', 'crates/zip/Cargo.toml', 'Cargo.lock', '.github/workflows/ci.yml', 'tests/format-reference/manifest.json']
    snapshot = {
        'baseline_commit': BASE,
        'status': 'awaiting independent reviews; stories remain open',
        'reviewers': 'GPT-6 Luna, medium; standalone fallback' if standalone else 'GPT-6 Luna, medium; local parallel review',
        'reviewed_paths': reviewed,
        'review_input_specs': [p.relative_to(ROOT).as_posix() for p in SPECS],
        'source_sha256': {p: hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in source_paths},
        'diff_sha256': hashlib.sha256(diff.encode()).hexdigest(),
        'diff_bytes': len(diff.encode()),
        'prompt_bytes': {layer: len(prompt.encode()) for layer,prompt in prompts.items()} if standalone else {},
        'limitations': ['Binary fixtures use Git binary patches, with exact hashes and semantic expectations in the manifest.',
                        'Review packets and claims are generated review inputs, excluded from their own diff.'],
    }
    (ART/'formats-review-snapshot.json').write_text(json.dumps(snapshot, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({k:snapshot[k] for k in ('status','diff_bytes','prompt_bytes')}, ensure_ascii=False))

if __name__ == '__main__':
    main()
