# SPDX-License-Identifier: AGPL-3.0-or-later
"""Exercise the actual generator CLI with corrupted official inputs."""
from pathlib import Path
import hashlib
import json
import shutil
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
UCD = ROOT/'data/ucd/17.0.0'
WORK = ROOT/'target/prototype-oracle'/('ucd-negative-'+uuid.uuid4().hex)
CASES = [
    ('wrong_version','ReadMe.txt',lambda text:text.replace('17.0.0','16.0.0')),
    ('invalid_line_class','LineBreak.txt',lambda text:text+'\n0041;Invalid_Class\n'),
    ('malformed_line_record','LineBreak.txt',lambda text:text+'\n0041\n'),
    ('missing_codepoint','LineBreak.txt',lambda text:text+'\n;AL\n'),
    ('range_outside_unicode','LineBreak.txt',lambda text:text+'\n110000;AL\n'),
    ('invalid_missing_default','LineBreak.txt',lambda text:text+'\n# @missing: 0000..10FFFF; Invalid_Class\n'),
    ('invalid_fold_scalar','CaseFolding.txt',lambda text:text+'\n0041; C; D800;\n'),
    ('overlapping_line_range','LineBreak.txt',lambda text:text+'\n0041;AL\n'),
]


def main():
    WORK.mkdir(parents=True)
    subprocess.run(['cargo','build','-p','atelier-ucd-gen','--locked','--offline'],cwd=ROOT,capture_output=True,check=True)
    original = {p.relative_to(UCD):hashlib.sha256(p.read_bytes()).hexdigest() for p in UCD.rglob('*') if p.is_file()}
    command = [str(ROOT/'target/debug/atelier-ucd-gen.exe'),'--ucd']
    results=[]
    for label,relative,alter in CASES:
        directory = WORK/label
        shutil.copytree(UCD,directory)
        path = directory/relative
        path.write_text(alter(path.read_text(encoding='utf-8')),encoding='utf-8',newline='\n')
        output = directory/'output.rs'
        output.write_bytes(b'previous-complete-output')
        run = subprocess.run([*command,str(directory),'--output',str(output)],capture_output=True)
        assert run.returncode!=0,label
        assert output.read_bytes()==b'previous-complete-output',label
        assert b'panicked' not in run.stderr,label
        results.append({'case':label,'exit_code':run.returncode,'existing_output_unchanged':True})
    directory = WORK/'missing_source'
    shutil.copytree(UCD,directory)
    (directory/'LineBreak.txt').unlink()
    output = directory/'output.rs'
    run = subprocess.run([*command,str(directory),'--output',str(output)],capture_output=True)
    assert run.returncode!=0 and not output.exists()
    results.append({'case':'missing_source','exit_code':run.returncode,'partial_output_absent':True})
    output = WORK/'replace.rs'
    output.write_bytes(b'previous')
    for _ in range(2):
        subprocess.run([*command,str(UCD),'--output',str(output)],capture_output=True,check=True)
        assert output.read_bytes()==(ROOT/'crates/unicode/src/generated.rs').read_bytes()
    assert all(hashlib.sha256((UCD/path).read_bytes()).hexdigest()==digest for path,digest in original.items())
    evidence={'cases':results,'windows_replacement_generations':2,'source_bytes_unchanged':True,'all_passed':True}
    (ART/'prototype-ucd-negative.json').write_text(json.dumps(evidence,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({'negative_cases':len(results),'windows_replacement_generations':2,'all_passed':True}))


if __name__=='__main__':main()
