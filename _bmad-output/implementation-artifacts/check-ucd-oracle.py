# SPDX-License-Identifier: AGPL-3.0-or-later
"""Independent scalar-by-scalar oracle, parsed from the official UCD archive."""
from pathlib import Path
import hashlib
import json
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
WORK = ROOT / 'target/prototype-oracle'
UCD = ROOT / 'data/ucd/17.0.0'
LIMIT = 0x110000
LABELS = 'general_category ccc line_break grapheme_break word_break indic_conjunct_break emoji emoji_presentation emoji_modifier emoji_modifier_base emoji_component extended_pictographic cased case_ignorable white_space name simple_uppercase simple_lowercase simple_titlecase full_uppercase full_lowercase full_titlecase case_fold simple_case_fold turkic_case_fold case_fold_turkic decomposition composition_exclusion composition conditional'.split()


def fnv(state, value):
    for byte in value.encode('utf-8'):
        state = ((state ^ byte) * 1099511628211) & 0xffffffffffffffff
    return state


def rows(relative):
    for raw in (UCD / relative).read_text(encoding='utf-8').splitlines():
        row = [part.strip() for part in raw.split('#', 1)[0].split(';')]
        if row[0]:
            yield row


def span(text):
    values = text.split('..')
    return int(values[0], 16), int(values[-1], 16) + 1


def sequence(text):
    return ','.join(f'{int(n,16):X}' for n in text.split())


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    manifest = json.loads((UCD / 'manifest.json').read_text(encoding='utf-8'))
    archive = WORK / 'UCD-17.0.0.zip'
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == manifest['archive_sha256']
    with zipfile.ZipFile(archive) as z:
        for entry in manifest['files']:
            raw = (UCD / entry['path']).read_bytes()
            assert len(raw) == entry['bytes']
            assert hashlib.sha256(raw).hexdigest() == entry['sha256']
            assert raw == z.read(entry['path']), entry['path']
    license_info = manifest['license']
    assert hashlib.sha256((UCD / license_info['path']).read_bytes()).hexdigest() == license_info['sha256']
    props = [[default] * LIMIT for default in ['Cn', '0', 'XX', 'Other', 'Other', 'None'] + ['N'] * 9]
    mappings = [dict() for _ in range(10)]
    names, decompositions, exclusions, compositions = {}, {}, set(), {}
    pending = None
    for row in rows('UnicodeData.txt'):
        n = int(row[0], 16)
        if row[1].endswith(', First>'):
            pending = n
        if row[1].endswith(', Last>'):
            for column, value in ((0, row[2]), (1, row[3])):
                props[column][pending:n+1] = [value] * (n+1-pending)
            pending = None
            continue
        props[0][n], props[1][n] = row[2], row[3]
        for index, field in enumerate((12,13,14)):
            if row[field]: mappings[index][n] = sequence(row[field])
        if row[5]:
            parts = row[5].split()
            tag = parts.pop(0)[1:-1] if parts[0].startswith('<') else ''
            decompositions[n] = tag + ':' + sequence(' '.join(parts))
    for column, filename, wanted in [(2,'LineBreak.txt',None),(3,'auxiliary/GraphemeBreakProperty.txt',None),(4,'auxiliary/WordBreakProperty.txt',None),(5,'DerivedCoreProperties.txt','InCB'), *[(6+i,'emoji/emoji-data.txt',key) for i,key in enumerate(['Emoji','Emoji_Presentation','Emoji_Modifier','Emoji_Modifier_Base','Emoji_Component','Extended_Pictographic'])],(12,'DerivedCoreProperties.txt','Cased'),(13,'DerivedCoreProperties.txt','Case_Ignorable'),(14,'PropList.txt','White_Space')]:
        # Read defaults separately, then apply all explicit records.
        for line in (UCD/filename).read_text(encoding='utf-8').splitlines():
            if '# @missing:' not in line: continue
            row = [v.strip() for v in line.split('# @missing:',1)[1].split(';')]
            if wanted and (len(row)<3 or row[1]!=wanted): continue
            a,b = span(row[0]); value = row[-1]
            props[column][a:b] = [value]*(b-a)
        for row in rows(filename):
            if wanted and row[1] != wanted: continue
            a,b = span(row[0]); value = row[2] if wanted=='InCB' else ('Y' if wanted else row[1])
            props[column][a:b] = [value]*(b-a)
    for row in rows('extracted/DerivedName.txt'):
        a,b = span(row[0])
        for n in range(a,b): names[n] = row[1].replace('*',f'{n:04X}')
    for row in rows('CaseFolding.txt'):
        n,status,value = int(row[0],16),row[1],sequence(row[2])
        if status in ('C','F'): mappings[6][n] = value
        if status in ('C','S'): mappings[7][n] = value
        if status=='T': mappings[8][n] = value
    conditional = []
    for row in rows('SpecialCasing.txt'):
        n = int(row[0],16)
        if row[4]: conditional.append((f'{n:X}',row[4],sequence(row[1]),sequence(row[2]),sequence(row[3])))
        else:
            for index,field in ((3,3),(4,1),(5,2)): mappings[index][n] = sequence(row[field])
    for index in range(3,6): mappings[index] = {**mappings[index-3],**mappings[index]}
    mappings[9] = {**mappings[6], **mappings[8]}
    for row in rows('DerivedNormalizationProps.txt'):
        if row[1]=='Full_Composition_Exclusion': exclusions.update(range(*span(row[0])))
    for row in rows('CompositionExclusions.txt'): exclusions.update(range(*span(row[0])))
    for n,record in decompositions.items():
        tag,value = record.split(':'); pair = value.split(',')
        if not tag and len(pair)==2 and n not in exclusions: compositions[value] = f'{n:X}'
    compiled = subprocess.run(['cargo','build','-p','atelier-unicode','--release','--locked','--offline','--message-format=json'],cwd=ROOT,capture_output=True,text=True,check=True)
    events = [json.loads(line) for line in compiled.stdout.splitlines() if line.startswith('{')]
    library = next(Path(f) for e in events if e.get('reason')=='compiler-artifact' and e['target']['name']=='atelier_unicode' for f in e['filenames'] if f.endswith('.rlib'))
    deps = library.parent if library.parent.name=='deps' else library.parent/'deps'
    driver = WORK/'ucd-oracle.exe'
    subprocess.run(['rustc','--edition=2024','-O',str(ART/'ucd-oracle.rs'),'--extern',f'atelier_unicode={library}','-L',f'dependency={deps}','-o',str(driver)],cwd=ROOT,check=True)
    output = WORK/'ucd-scalars.tsv'
    with output.open('wb') as stream: subprocess.run([str(driver)],stdout=stream,check=True)
    count,actual_conditional = 0,[]
    fingerprints = [14695981039346656037]*30
    with output.open(encoding='utf-8') as stream:
        for line in stream:
            actual = line.rstrip('\n').split('\t')
            if actual[0]=='conditional': actual_conditional.append(tuple(actual[1:])); continue
            n = int(actual[0],16)
            expected = [prop[n] for prop in props] + [names.get(n,'~')] + [mapping.get(n,'~') for mapping in mappings]
            decomp = decompositions.get(n,'~')
            pair = decomp[1:] if decomp.startswith(':') else '~'
            expected += [decomp,'Y' if n in exclusions else 'N',compositions.get(pair,'~')]
            assert actual[1:] == expected, (f'U+{n:04X}',[(i,a,e) for i,(a,e) in enumerate(zip(actual[1:],expected)) if a!=e])
            for index,value in enumerate(expected): fingerprints[index] = fnv(fingerprints[index],value+'\n')
            count += 1
    assert count == LIMIT-0x800
    assert actual_conditional == conditional
    for rule in conditional: fingerprints[29] = fnv(fingerprints[29],'\t'.join(rule)+'\n')
    corpus = ROOT/'tests/unicode-reference'
    corpus.mkdir(parents=True,exist_ok=True)
    raw = ''.join(f'{label}\t{value:016x}\n' for label,value in zip(LABELS,fingerprints)).encode('utf-8')
    (corpus/'checksums.tsv').write_bytes(raw)
    (corpus/'manifest.json').write_text(json.dumps({'unicode_version':'17.0.0','source':'Independent Python parser of the frozen official UCD','scalars':count,'fields':30,'checksum_algorithm':'FNV-1a-64 per field, UTF-8 value followed by LF, ascending scalar order; conditional rows TSV','file':'checksums.tsv','sha256':hashlib.sha256(raw).hexdigest()},indent=2)+'\n',encoding='utf-8')
    evidence = {'unicode_version':'17.0.0','scalars_compared':count,'properties_per_scalar':15,'mapping_lookups_per_scalar':10,'names_compared':len(names),'decompositions_compared':len(decompositions),'conditional_rules_compared':len(conditional),'canonical_compositions_compared':len(compositions),'official_files_verified':len(manifest['files']),'archive_sha256':manifest['archive_sha256'],'all_equal':True,'raw_license_verified':True}
    (ART/'prototype-ucd-oracle.json').write_text(json.dumps(evidence,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(evidence))


if __name__=='__main__': main()
