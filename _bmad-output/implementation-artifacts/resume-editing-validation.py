# SPDX-License-Identifier: AGPL-3.0-or-later
"""Resume a failed local validation with identical sources and isolated native profiles.

Only the diagnostic output directories change. The original driver's command
list, result collection and final invariants are reused, and the failed attempt
is retained in its own evidence file and in the completed report.
"""
import ast
import hashlib
import json
from pathlib import Path
import time

ART = Path(__file__).resolve().parent
ROOT = ART.parents[1]
driver = ART/'validate-editing-ergonomics.py'
proof = ART/'editing-ergonomics-validation.json'
previous = json.loads(proof.read_text(encoding='utf-8'))
assert previous['source_bytes_unchanged'] and previous['lock_bytes_unchanged']
assert all(hashlib.sha256((ROOT/p).read_bytes()).hexdigest()==h
           for p,h in previous['source_sha256'].items())
assert previous['checks'][-1]['name']=='native_webview_debug'
assert previous['checks'][-1]['exit_code']!=0
assert 'initialization callback timed out' in previous['failed_output']
assert all(c['exit_code']==0 for c in previous['checks'][:-1])
(ART/'editing-ergonomics-retry-validation.json').write_bytes(proof.read_bytes())
tree = ast.parse(driver.read_text(encoding='utf-8'),filename=str(driver))
loop_index = next(i for i,n in enumerate(tree.body) if isinstance(n,ast.With))
context = {'__file__':str(driver),'__name__':'__main__'}
exec(compile(ast.Module(body=tree.body[:loop_index],type_ignores=[]),str(driver),'exec'),context)
assert context['source_hashes']==previous['source_sha256']
context['evidence'] = {k:v for k,v in previous.items() if k!='failed_output'}
context['evidence']['checks'] = previous['checks'][:-1]
context['evidence']['retry_attempts'] = [{'failed_check':previous['checks'][-1],
    'reason':'WebView2 initialization callback timed out; retry with isolated native profile directories',
    'failed_evidence':'editing-ergonomics-retry-validation.json'}]
pending = context['commands'][len(previous['checks'])-1:]
suffix = str(time.time_ns())
for name,argv in pending:
    if name.startswith(('native_webview_','embedded_resources_')):
        index = argv.index('-TargetDirectory')+1
        argv[index] = str(ROOT/'target'/f'editing-{name}-{suffix}')
context['pending_commands'] = pending
with_node = tree.body[loop_index]
assert isinstance(with_node.items[0].context_expr,ast.Call)
assert with_node.items[0].context_expr.args[0].value=='w'
with_node.items[0].context_expr.args[0].value='a'
for_node = with_node.body[0]
assert isinstance(for_node,ast.For) and for_node.iter.id=='commands'
for_node.iter = ast.Name(id='pending_commands',ctx=ast.Load())
tail = ast.Module(body=tree.body[loop_index:],type_ignores=[])
ast.fix_missing_locations(tail)
exec(compile(tail,str(driver),'exec'),context)
