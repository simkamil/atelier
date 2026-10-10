# SPDX-License-Identifier: AGPL-3.0-or-later
"""Agent-only image corpus, mutation and font integrity verification."""
from pathlib import Path
import hashlib
import json
import subprocess
import shutil
import tempfile
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT/'_bmad-output/implementation-artifacts'
WORK = ROOT/'target/images-fonts-oracle'


def main():
    paths = sorted({ROOT/'Cargo.lock', ROOT/'.github/workflows/ci.yml', ROOT/'tests/check-assets.ps1',
                    *(ROOT/'crates/image').rglob('*.rs'), *(ROOT/'crates/font').rglob('*.rs'),
                    *(p for folder in ('data/fonts', 'tests/image-reference') for p in (ROOT/folder).rglob('*') if p.is_file()),
                    ART/'image-mutation-oracle.rs', ART/'generate-image-references.py', Path(__file__).resolve()})
    hashes = {p.relative_to(ROOT).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    manifest = json.loads((ROOT/'tests/image-reference/manifest.json').read_text())
    for case in manifest['cases']:
        assert hashlib.sha256((ROOT/'tests/image-reference'/case['file']).read_bytes()).hexdigest() == case['sha256']
        if case['valid']:
            assert hashlib.sha256((ROOT/'tests/image-reference'/case['reference']).read_bytes()).hexdigest() == case['reference_sha256']
    # Regenerate all references, independently, and demand byte-exact equality.
    subprocess.run(['py', '-X', 'utf8', str(ART/'generate-image-references.py')], cwd=ROOT, check=True, capture_output=True)
    assert all(hashlib.sha256((ROOT/p).read_bytes()).hexdigest()==h for p,h in hashes.items())
    build = subprocess.run(['cargo','build','-p','atelier-image','--locked','--offline','--message-format=json'], cwd=ROOT, capture_output=True, text=True, check=True)
    library = None
    for line in build.stdout.splitlines():
        item = json.loads(line)
        if item.get('reason')=='compiler-artifact' and item['target']['name']=='atelier_image':
            library = next(f for f in item['filenames'] if f.endswith('.rlib'))
    assert library
    driver = WORK/'image-mutation-oracle.exe'
    subprocess.run(['rustc','--edition=2024',str(ART/'image-mutation-oracle.rs'),'-L','dependency=target/debug/deps','--extern',f'atelier_image={library}','-o',str(driver)], cwd=ROOT, check=True)
    result = subprocess.run([str(driver),str(ROOT/'tests/image-reference')], cwd=ROOT, check=True, text=True, capture_output=True)
    report = {'date':'2026-10-10','source_sha256':hashes,'image_cases':len(manifest['cases']),
              'valid_image_cases':sum(c['valid'] for c in manifest['cases']),
              'invalid_pngsuite':sum(not c['valid'] for c in manifest['cases']),
              'golden_regeneration_identical':True, **json.loads(result.stdout)}
    # Inventory integrity/type checks run in native PowerShell, also in CI.
    subprocess.run(['pwsh','-NoProfile','-File',str(ROOT/'tests/check-assets.ps1')],cwd=ROOT,check=True,capture_output=True)
    copied=Path(tempfile.mkdtemp(prefix='asset-negative-',dir=WORK))
    for folder in ('data/fonts','tests/image-reference'):
        shutil.copytree(ROOT/folder,copied/folder)
    font_path=copied/'data/fonts/manifest.json'
    image_path=copied/'tests/image-reference/manifest.json'
    original_font=font_path.read_bytes(); original_image=image_path.read_bytes()
    mutations=[('missing_path',lambda f,i:f['assets'][0].pop('path')),
               ('weight_type',lambda f,i:f['assets'][0].update(weight='400')),
               ('unknown_family',lambda f,i:f['assets'][0].update(family='Unknown')),
               ('duplicate_face',lambda f,i:f['assets'].__setitem__(1,dict(f['assets'][0]))),
               ('font_hash',lambda f,i:f['assets'][0].update(sha256='0'*64)),
               ('path_escape',lambda f,i:f['assets'][0].update(path='data/fonts/../../../Cargo.lock')),
               ('short_commit',lambda f,i:f['assets'][0]['upstream'].update(commit='abcd')),
               ('lost_image',lambda f,i:i['cases'].pop()),
               ('image_hash',lambda f,i:i['cases'][0].update(sha256='0'*64)),
               ('reference_hash',lambda f,i:i['cases'][0].update(reference_sha256='0'*64)),
               ('tsv_disagreement',lambda f,i:i['cases'][0].update(format='gif'))]
    for label,mutate in mutations:
        f=json.loads(original_font); i=json.loads(original_image); mutate(f,i)
        font_path.write_text(json.dumps(f),encoding='utf-8'); image_path.write_text(json.dumps(i),encoding='utf-8')
        bad=subprocess.run(['pwsh','-NoProfile','-File',str(ROOT/'tests/check-assets.ps1'),'-RootPath',str(copied)],cwd=ROOT,capture_output=True)
        assert bad.returncode==1, label
    report['asset_guard_negative_cases']=len(mutations)
    # FontTools independently checks the metadata declared in the inventory.
    font_manifest = json.loads((ROOT/'data/fonts/manifest.json').read_text())
    faces = font_manifest['assets']
    assert len(faces)==20, font_manifest.keys()
    report['font_manifest_faces']=len(faces)
    report['fonttools_faces']=[]
    for entry in faces:
        relative = entry['path']
        path = ROOT/relative
        assert hashlib.sha256(path.read_bytes()).hexdigest()==entry['sha256']
        face = TTFont(path)
        assert face['name'].getDebugName(1)==entry['family']
        assert face['OS/2'].usWeightClass==entry['weight']
        assert bool(face['head'].macStyle & 2)==(entry['style']=='italic')
        assert face['name'].getDebugName(5)==entry['font_version']
        assert len(entry['upstream']['commit'])==40
        assert (ROOT/'data/fonts'/entry['license_file']).is_file()
        report['fonttools_faces'].append({'file':relative,'family':face['name'].getDebugName(1),
            'subfamily':face['name'].getDebugName(2),'version':face['name'].getDebugName(5),
            'weight':face['OS/2'].usWeightClass, 'italic':bool(face['head'].macStyle & 2),
            'glyphs':face['maxp'].numGlyphs})
    report['source_bytes_unchanged']=all(hashlib.sha256((ROOT/p).read_bytes()).hexdigest()==h for p,h in hashes.items())
    assert report['source_bytes_unchanged']
    (ART/'images-fonts-oracle.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n', encoding='utf-8',newline='\n')
    print(json.dumps({k:v for k,v in report.items() if k not in ('source_sha256','fonttools_faces')}))


if __name__=='__main__':
    main()
