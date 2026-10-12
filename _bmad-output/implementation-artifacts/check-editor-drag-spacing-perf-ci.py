# SPDX-License-Identifier: AGPL-3.0-or-later
"""Read the published lot's CI and annotations without exposing Git credentials."""
from pathlib import Path
import json,subprocess,sys,urllib.request,urllib.error

ROOT=Path(__file__).resolve().parents[2]
ART=Path(__file__).resolve().parent
REPO='simkamil/mon-bureau'
head=sys.argv[1] if len(sys.argv)>1 else subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
credential=subprocess.run(['git','credential','fill'],cwd=ROOT,input=f'url=https://github.com/{REPO}.git\n\n',text=True,capture_output=True)
fields=dict(line.split('=',1) for line in credential.stdout.splitlines() if '=' in line)
token=fields.get('password')
if credential.returncode or not token: raise SystemExit('Existing Git credential unavailable.')
def get(path):
    request=urllib.request.Request(f'https://api.github.com/repos/{REPO}/{path}',headers={
        'Accept':'application/vnd.github+json','User-Agent':'Atelier-Codex',
        'Authorization':f'Bearer {token}','X-GitHub-Api-Version':'2022-11-28'})
    try:
        with urllib.request.urlopen(request,timeout=30) as response: return json.load(response)
    except urllib.error.HTTPError as error: raise SystemExit(f'GitHub CI: HTTP {error.code}') from None
    except urllib.error.URLError: raise SystemExit('GitHub CI: network request failed') from None
record={'date':'2026-10-12','repository':REPO,'source_commit':head,'runs':[],
        'local_validation':'editor-drag-spacing-perf-validation.json',
        'interpretation':'Local tests do not establish remote success. No billing settings changed.'}
for run in get('actions/runs?per_page=20')['workflow_runs']:
    if run['head_sha']!=head: continue
    item={key:run.get(key) for key in ('id','name','status','conclusion','html_url','updated_at')}
    item['jobs']=[]
    for job in get(f"actions/runs/{run['id']}/jobs?per_page=100")['jobs']:
        detail={key:job.get(key) for key in ('id','name','status','conclusion','runner_name','steps')}
        check=get(f"check-runs/{job['id']}")
        detail['annotations']=get(f"check-runs/{job['id']}/annotations?per_page=100") if check.get('output',{}).get('annotations_count',0) else []
        item['jobs'].append(detail)
    record['runs'].append(item)
(ART/'editor-drag-spacing-perf-ci.json').write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps({'commit':head,'runs':[{'url':r['html_url'],'status':r['status'],'conclusion':r['conclusion'],
 'jobs':[{'name':j['name'],'steps':len(j['steps'] or []),'runner':j['runner_name'],
          'annotations':[a.get('message') for a in j['annotations']]} for j in r['jobs']]} for r in record['runs']]}))
