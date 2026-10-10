# SPDX-License-Identifier: AGPL-3.0-or-later
"""Independent XML semantics, history model and SVG structure checks (agent only)."""
from pathlib import Path
import base64
import hashlib
import io
import json
import struct
import subprocess
import xml.etree.ElementTree as ET
import zlib

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
WORK = ROOT / 'target/editing-oracle'
DRIVER = WORK / 'editing-oracle.exe'

def run(*args):
    result = subprocess.run([str(DRIVER), *map(str,args)], cwd=ROOT, capture_output=True, text=True, encoding='utf-8')
    if result.returncode:
        raise AssertionError(result.stdout + result.stderr)
    return json.loads(result.stdout)

def png():
    def chunk(kind, data):
        return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data)&0xffffffff)
    # A real, independently encoded, opaque yellow pixel.
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',1,1,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(b'\x00\xff\xff\x00\xff'))+chunk(b'IEND',b'')

def subtree(element):
    tag = 'comment' if element.tag is ET.Comment else 'pi' if element.tag is ET.ProcessingInstruction else element.tag
    return (tag,sorted(element.attrib.items()),element.text or '',[(subtree(c),c.tail or '') for c in element])

def parse(raw):
    return ET.fromstring(raw,parser=ET.XMLParser(target=ET.TreeBuilder(insert_comments=True,insert_pis=True)))

def scopes(raw):
    stack=[]
    pending=[]
    result=[]
    for event,value in ET.iterparse(io.BytesIO(raw),events=('start-ns','start','end')):
        if event=='start-ns':
            pending.append(value)
        elif event=='start':
            current=dict(stack[-1]) if stack else {'xml':'http://www.w3.org/XML/1998/namespace','':''}
            current.update(pending)
            pending=[]
            stack.append(current)
            result.append((value.tag,current))
        else:
            stack.pop()
    return result

def main():
    WORK.mkdir(parents=True,exist_ok=True)
    sources=['_bmad-output/implementation-artifacts/editing-oracle.rs',
             '_bmad-output/implementation-artifacts/check-editing-oracle.py',
             'Cargo.lock', *[p.relative_to(ROOT).as_posix() for name in ('xml','model','render') for p in (ROOT/f'crates/{name}').rglob('*.rs')]]
    hashes={name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in sorted(sources)}
    build=subprocess.run(['cargo','build','-p','atelier-xml','-p','atelier-render','-p','atelier-model','-p','atelier-geom','--locked','--offline','--message-format=json'],cwd=ROOT,check=True,capture_output=True,text=True)
    libraries={}
    for line in build.stdout.splitlines():
        message=json.loads(line)
        if message.get('reason')=='compiler-artifact':
            for filename in message.get('filenames',[]):
                if filename.endswith('.rlib'):
                    libraries[message['target']['name']]=filename
    command=['rustc','--edition=2024',str(ART/'editing-oracle.rs'),'-L','dependency=target/debug/deps','-o',str(DRIVER)]
    for name in ('xml','model','render','geom'):
        command.extend(['--extern',f'atelier_{name}={libraries[f"atelier_{name}"]}'])
    subprocess.run(command,cwd=ROOT,check=True)
    report={'date':'2026-10-10','source_sha256':hashes,'xml_semantic_cases':0,'xml_scope_checks':0,'history_reference_steps':0,'svg':{},'failures':[]}
    report.update(run('history'))
    ns='xmlns:p="urn:one" xmlns:unused="urn:unused" xmlns:ns0="urn:original" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"'
    cases=[
        f'<o {ns}><u xsi:type="p:Widget" ref="ns0:T">p:Text<inner/><p:a/><!--comment--><?pi data?></u><tail/></o>',
        f'<o {ns} xmlns="urn:base"><u xsi:type="p:Widget"><inner xmlns:p="urn:two" xsi:type="p:Other"><p:z/></inner><reset xmlns=""><n p:attr="x">p:Text</n></reset></u><tail/></o>',
        f'<o {ns}><u xmlns:p="urn:local" xsi:type="p:X"/><tail/></o>',
        f'<o {ns}><p:u xml:lang="fr" p:a="x&#13;y&#9;z">préfixe &amp; 🙂&#13;<!--c--><![CDATA[raw<text]]></p:u><tail/></o>',
        '<o><u><inner/><n>unqualified</n></u><tail/></o>',
        '<o xmlns="urn:root"><u xmlns=""><n/><n xmlns="urn:n"><n xmlns=""/></n></u><tail/></o>',
        '<o xmlns:p="urn:p"><p:u xmlns:p="urn:p2"><p:n xmlns:p="urn:p3">p:Name</p:n><p:n>p:Name</p:n></p:u><tail/></o>',
        '<o xmlns:unused="urn:only-in-values"><u value="unused:Type">unused:Value</u><tail/></o>',
    ]
    for index,text in enumerate(cases):
        for encoding,bom in [('utf-8',b''),('utf-16le',b'\xff\xfe'),('utf-16be',b'\xfe\xff')]:
            raw=bom+text.encode(encoding)
            input_path=WORK/f'opaque-{index}-{encoding}.xml'
            output_path=WORK/f'opaque-{index}-{encoding}-out.xml'
            input_path.write_bytes(raw)
            assert run('opaque',input_path,output_path)['following_event']
            out=output_path.read_bytes()
            before=parse(raw)[0]
            after=parse(out)[0]
            assert subtree(before)==subtree(after),(index,encoding,'semantics')
            expected=scopes(raw)[1:-1]  # captured root and descendants, excluding outer and tail
            actual=scopes(out)[1:]
            assert len(expected)==len(actual),(index,encoding,'scope count')
            for (tag,binding),(out_tag,out_binding) in zip(expected,actual):
                assert tag==out_tag
                for prefix,uri in binding.items():
                    assert out_binding.get(prefix,'')==uri,(index,encoding,tag,prefix,uri,out_binding)
                    report['xml_scope_checks']+=1
            # Move the produced subtree beneath an additional conflicting default
            # namespace. This tests preservation of namespace *absence* too.
            text_out=out.decode('utf-8')
            start=text_out.index('>',text_out.index('<ns0:target'))
            hostile=(text_out[:start]+' xmlns="urn:evil-default"'+text_out[start:]).encode()
            assert subtree(parse(hostile)[0])==subtree(before),(index,encoding,'default reset')
            report['xml_semantic_cases']+=1
    image=png()
    image_path=WORK/'yellow.png'
    image_path.write_bytes(image)
    svg_path=WORK/'page.svg'
    report['svg'].update(run('svg',svg_path,image_path))
    svg_raw=svg_path.read_bytes()
    svg_root=parse(svg_raw)
    svg_ns={'s':'http://www.w3.org/2000/svg'}
    assert svg_root.tag=='{http://www.w3.org/2000/svg}svg'
    assert svg_root.attrib['viewBox']=='0 0 300 180'
    uses=svg_root.findall('.//s:use',svg_ns)
    assert len(uses)==2 and uses[0].get('{http://www.w3.org/1999/xlink}href')==uses[1].get('{http://www.w3.org/1999/xlink}href')
    glyph_id=uses[0].get('{http://www.w3.org/1999/xlink}href')[1:]
    assert len(svg_root.findall(f'.//*[@id="{glyph_id}"]'))==1
    images=svg_root.findall('.//s:image',svg_ns)
    assert len(images)==1
    uri=images[0].get('{http://www.w3.org/1999/xlink}href')
    assert uri.startswith('data:image/png;base64,')
    assert base64.b64decode(uri.split(',',1)[1])==image
    assert not svg_root.findall('.//s:script',svg_ns)
    assert not svg_root.findall('.//s:a',svg_ns)
    report['svg'].update({'xml_valid':True,'glyph_uses':len(uses),'unique_glyphs':1,'embedded_png_matches':True,'bytes':len(svg_raw),'sha256':hashlib.sha256(svg_raw).hexdigest(),'visual_file':svg_path.relative_to(ROOT).as_posix()})
    report['source_bytes_unchanged']=all(hashlib.sha256((ROOT/name).read_bytes()).hexdigest()==digest for name,digest in hashes.items())
    assert report['source_bytes_unchanged']
    (ART/'editing-oracle.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
    print(json.dumps({k:v for k,v in report.items() if k!='source_sha256'},ensure_ascii=False))

if __name__=='__main__':
    main()
