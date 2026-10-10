#!/usr/bin/env python3
"""Generate deterministic, dependency-free ZIP/XML reference fixtures for Rust tests."""
from __future__ import annotations
import base64, hashlib, json, struct, zlib
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parents[2] / "tests" / "format-reference"
ZIP = ROOT / "zip"
XML = ROOT / "xml"

def crc(data: bytes) -> int:
    return zlib.crc32(data) & 0xffffffff

def zip_archive(items, *, descriptor=None, zip64=False):
    """items: (filename bytes, data bytes, method, utf8 flag); descriptor: signed bool/None."""
    out = bytearray(); central=[]
    for name,data,method,utf8 in items:
        flag=(0x800 if utf8 else 0) | (8 if descriptor is not None else 0)
        raw=zlib.compressobj(6,zlib.DEFLATED,-15).compress(data)+zlib.compressobj(6,zlib.DEFLATED,-15).flush() if False else None
        if method == 0: packed=data
        elif method == 8:
            c=zlib.compressobj(6,zlib.DEFLATED,-15); packed=c.compress(data)+c.flush()
        else: raise ValueError(method)
        checksum=crc(data); offset=len(out)
        extra=b""
        csize=len(packed); usize=len(data)
        if zip64:
            extra=struct.pack('<HHQQ',1,16,usize,csize)
        if descriptor is None:
            lcrc,lc,lu=checksum,csize,usize
        else: lcrc,lc,lu=0,0,0
        out += struct.pack('<IHHHHHIIIHH',0x04034b50,45 if zip64 else 20,flag,method,0,33,lcrc,lc,lu,len(name),len(extra))+name+extra+packed
        if descriptor is not None:
            if descriptor: out += struct.pack('<I',0x08074b50)
            out += struct.pack('<III',checksum,csize,usize)
        cextra=b''
        if zip64: cextra=struct.pack('<HHQQQ',1,24,usize,csize,offset)
        central.append(struct.pack('<IHHHHHHIIIHHHHHII',0x02014b50,45 if zip64 else 20,45 if zip64 else 20,flag,method,0,33,checksum,csize,usize,len(name),len(cextra),0,0,0,0,offset)+name+cextra)
    cdoff=len(out); cd=b''.join(central); out+=cd
    count=len(central)
    if zip64:
        zoff=len(out); out+=struct.pack('<IQHHIIQQQQ',0x06064b50,44,45,45,0,0,count,count,len(cd),cdoff)
        out+=struct.pack('<IIQI',0x07064b50,0,zoff,1)
        out+=struct.pack('<IHHHHIIH',0x06054b50,0,0,0xffff,0xffff,0xffffffff,0xffffffff,0)
    else: out+=struct.pack('<IHHHHIIH',0x06054b50,0,0,count,count,len(cd),cdoff,0)
    return bytes(out)

def save_zip(name, blob, entries, note='', writer_options=None):
    p=ZIP/name; p.write_bytes(blob)
    fixture=dict(path=f'zip/{name}',sha256=hashlib.sha256(blob).hexdigest(),kind='zip',entries=entries,note=note)
    if writer_options is not None:
        fixture['writer_options']=writer_options
    specs.append(fixture)

def save_xml(name, blob, expected, note=''):
    p=XML/name; p.write_bytes(blob)
    specs.append(dict(path=f'xml/{name}',sha256=hashlib.sha256(blob).hexdigest(),kind='xml',expected=expected,note=note))

specs=[]
# Baseline archive covers classic/store/deflate/UTF-8 and an empty file.
base=zip_archive([(b'plain.txt',b'plain payload',0,True),(b'data.txt',b'alpha '*20,8,True),(b'empty',b'',0,True),("été.txt".encode(),b'utf8 name',0,True)])
save_zip('valid-mixed.zip',base,[{'name':n.decode('utf-8'),'data_utf8':d.decode('utf-8'),'method':m,'crc32':f'{crc(d):08x}'} for n,d,m in [(b'plain.txt',b'plain payload',0),(b'data.txt',b'alpha '*20,8),(b'empty',b'',0),('été.txt'.encode(),b'utf8 name',0)]], 'Classic EOCD; stored/deflate; UTF-8 flag; empty entry.')
save_zip('valid-cp437.zip',zip_archive([(b'caf\x82.txt',b'CP437 name',0,False)]),[{'name':'café.txt','data_utf8':'CP437 name','method':0,'crc32':f'{crc(b"CP437 name"):08x}'}],'Raw filename byte 0x82 is CP437 é; UTF-8 flag clear.')
save_zip('valid-descriptor-signed.zip',zip_archive([(b'desc.txt',b'descriptor payload',8,True)],descriptor=True),[{'name':'desc.txt','data_utf8':'descriptor payload','method':8,'crc32':f'{crc(b"descriptor payload"):08x}'}],'General-purpose bit 3; signed data descriptor.')
save_zip('valid-descriptor-unsigned.zip',zip_archive([(b'desc.txt',b'descriptor payload',8,True)],descriptor=False),[{'name':'desc.txt','data_utf8':'descriptor payload','method':8,'crc32':f'{crc(b"descriptor payload"):08x}'}],'General-purpose bit 3; unsigned data descriptor.')
save_zip('valid-force-zip64.zip',zip_archive([(b'small.txt',b'zip64 small',0,True)],zip64=True),[{'name':'small.txt','data_utf8':'zip64 small','method':0,'crc32':f'{crc(b"zip64 small"):08x}'}],'ZIP64 EOCD/locator and per-entry extra despite tiny data.')
# Invalid-name and corrupt/truncated records, plus a deliberately advertised giant output.
save_zip('unsafe-traversal.zip',zip_archive([(b'../escape.txt',b'x',0,True)]),[{'name':'../escape.txt','data_utf8':'x','method':0,'crc32':f'{crc(b"x"):08x}','expected_name_safety':'unsafe'}],'Structurally valid; name must be flagged unsafe, never extracted implicitly.')
b=bytearray(zip_archive([(b'bad.txt',b'CRC corruption target',0,True)])); marker=b'CRC corruption target'; at=b.find(marker); b[at]^=1
save_zip('bad-crc.zip',bytes(b),[{'name':'bad.txt','expected_error':'crc_mismatch'}],'Payload byte altered; central/local advertised CRC remains original.')
full=zip_archive([(b'truncated.txt',b'payload',0,True)])
save_zip('truncated-payload.zip',full[:-24],[{'name':'truncated.txt','expected_error':'truncated_or_missing_central_directory'}],'Tail removed across archive directory/end records.')
# Central directory size fields claim an output above reasonable test limits, with no payload expansion.
b=bytearray(zip_archive([(b'limit.bin',b'x',0,True)])); pos=b.find(b'PK\x01\x02'); struct.pack_into('<I',b,pos+24,0x7fffffff); struct.pack_into('<I',b,pos+20,0x7fffffff)
save_zip('limit-advertised-size.zip',bytes(b),[{'name':'limit.bin','expected_error':'declared_size_limit'}],'Central directory advertises huge sizes; reject by configured limits before allocation.')

# Immutable outputs captured once from the stable Rust Writer. The wire bytes,
# rather than a second ZIP encoder, make these fixtures useful as writer goldens.
golden_entries=[
    {'name':'café.txt','data_utf8':'stored golden: café\n','method':0,'crc32':f'{crc("stored golden: café\n".encode()):08x}'},
    {'name':'東京/🙂.xml','data_utf8':'<r>zip golden 🙂</r>','method':8,'crc32':f'{crc("<r>zip golden 🙂</r>".encode()):08x}'},
]
for filename,force_zip64,encoded in [
    ('writer-golden-classic.zip',False,'UEsDBC0AAAAAAAAAIQACejbYFQAAABUAAAAIAAAAY2Fmgi50eHRzdG9yZWQgZ29sZGVuOiBjYWbDqQpQSwMELQAACAgAAAAhAHH357g6AAAAFgAAAA8AAADmnbHkuqwv8J+Zgi54bWwFwQEEADAIADAAAACAAIADQAIAAAAAvztXCRAgAAAgEAAAAAAAAAAAAEAAgDY08qdw5e/D0JWBy2gAUEsBAi0ALQAAAAAAAAAhAAJ6NtgVAAAAFQAAAAgAAAAAAAAAAAAAAAAAAAAAAGNhZoIudHh0UEsBAi0ALQAACAgAAAAhAHH357g6AAAAFgAAAA8AAAAAAAAAAAAAAAAAOwAAAOadseS6rC/wn5mCLnhtbFBLBQYAAAAAAgACAHMAAACiAAAAAAA='),
    ('writer-golden-zip64.zip',True,'UEsDBC0AAAAAAAAAIQACejbY//////////8IABQAY2Fmgi50eHQBABAAFQAAAAAAAAAVAAAAAAAAAHN0b3JlZCBnb2xkZW46IGNhZsOpClBLAwQtAAAICAAAACEAcffnuP//////////DwAUAOadseS6rC/wn5mCLnhtbAEAEAAWAAAAAAAAADoAAAAAAAAABcEBBAAwCAAwAAAAgACAA0ACAAAAAL87VwkQIAAAIBAAAAAAAAAAAABAAIA2NPKncOXvw9CVgctoAFBLAQItAC0AAAAAAAAAIQACejbY//////////8IABwAAAAAAAAAAAAAAP////9jYWaCLnR4dAEAGAAVAAAAAAAAABUAAAAAAAAAAAAAAAAAAABQSwECLQAtAAAICAAAACEAcffnuP//////////DwAcAAAAAAAAAAAAAAD/////5p2x5LqsL/CfmYIueG1sAQAYABYAAAAAAAAAOgAAAAAAAABPAAAAAAAAAFBLBgYsAAAAAAAAAC0ALQAAAAAAAAAAAAIAAAAAAAAAAgAAAAAAAACrAAAAAAAAAMoAAAAAAAAAUEsGBwAAAAB1AQAAAAAAAAEAAABQSwUGAAAAAP///////////////wAA'),
]:
    golden=base64.b64decode(encoded,validate=True)
    save_zip(filename,golden,golden_entries,
             'Captured from the stable atelier-zip Writer; immutable wire bytes; validate independently with Python zipfile.',
             {'force_zip64':force_zip64,'timestamp':None,'compression_level':'Normal','entries':[{'name':e['name'],'method':'Store' if e['method']==0 else 'Deflate','payload_utf8':e['data_utf8']} for e in golden_entries]})

# XML exact octets. Expected field describes semantic events / parser compatibility.
save_xml('utf8-boundaries.xml','<?xml version="1.0"?><r>é𝄞&amp; &#x1F642;</r>'.encode(),{'valid':True,'root':'r','text':'é𝄞& 🙂','encoding':'utf-8'},'Contains 2/4-byte UTF-8, predefined and numeric references.')
save_xml('utf16le-bom.xml',b'\xff\xfe'+('<?xml version="1.0" encoding="UTF-16"?><r>é𝄞</r>').encode('utf-16le'),{'valid':True,'root':'r','text':'é𝄞','encoding':'utf-16le'},'UTF-16LE BOM; readers should handle split code units/surrogate pair.')
save_xml('utf16be-bom.xml',b'\xfe\xff'+('<?xml version="1.0" encoding="UTF-16"?><r>é𝄞</r>').encode('utf-16be'),{'valid':True,'root':'r','text':'é𝄞','encoding':'utf-16be'},'UTF-16BE BOM.')
save_xml('namespaces.xml',b'<root xmlns="urn:base" xmlns:p="urn:one" plain="v" p:a="1"><p:item p:a="2"><inner xmlns:p="urn:two" p:a="3"/></p:item><clear xmlns=""><item/></clear></root>',{'valid':True,'expanded_names':[{'element':'{urn:base}root','attrs':{'plain':'v','{urn:one}a':'1'}},{'element':'{urn:one}item','attrs':{'{urn:one}a':'2'}},{'element':'{urn:base}inner','attrs':{'{urn:two}a':'3'}},{'element':'clear','attrs':{}},{'element':'item','attrs':{}}]},'Default namespace applies to elements only; nested prefix shadow and empty-default reset. Namespaced attribute tuples use ElementTree expanded syntax.')
save_xml('misc-constructs.xml',b'<?xml version="1.0"?><?build stable?><!DOCTYPE r [<!ELEMENT r ANY>]><!--comment--><r><![CDATA[a<&]]>&#65;&amp;</r>',{'valid':True,'root':'r','text':'a<&A&','pi':'build stable','comment':'comment','cdata':'text-equivalent','doctype':'no-expansion'},'Benign internal subset, PI, comment, CDATA and references; ElementTree/minidom may omit lexical distinctions.')
save_xml('normalization.xml',b'<r a="x\t y\r\nz&#xD;">a\r\nb\rc</r>',{'valid':True,'root':'r','attributes':{'a':'x  y z\r'},'text':'a\nb\nc'},'XML 1.0 EOL normalization: literal CRLF/CR in content become LF; literal tab/space/CRLF in CDATA attribute become spaces, while numeric CR reference is preserved.')
save_xml('predefined-numeric-entities.xml',b'<r>&lt;&gt;&apos;&quot;&amp;&#x41;&#65;</r>',{'valid':True,'root':'r','text':'<>' + chr(39) + '"&AA'},'Five predefined entities and decimal/hex character references.')
for name,blob,why in [
 ('malformed-mismatched.xml',b'<r><x></r>','mismatched_tags'),
 ('malformed-unknown-entity.xml',b'<r>&notDeclared;</r>','unknown_entity'),
 ('malformed-invalid-char.xml',b'<r>\x01</r>','illegal_xml_char'),
 ('malformed-truncated-utf8.xml',b'<r>\xc3</r>','invalid_utf8'),
 ('malformed-unbound-prefix.xml',b'<p:r/>','unbound_namespace_prefix'),
 ('malformed-duplicate-expanded-attr.xml',b'<r xmlns:a="u" xmlns:b="u" a:x="1" b:x="2"/>','duplicate_expanded_attribute')]:
    save_xml(name,blob,{'valid':False,'error':why},'Must fail strict XML parsing; expected error category is semantic, exact diagnostics vary.')
save_xml('doctype-no-expansion.xml',b'<!DOCTYPE r [<!ENTITY a "&a;&a;">]><r>&a;</r>',{'valid':False,'error':'doctype_entity_not_expanded'},'Security corpus: implementation must not expand entities. ElementTree may raise recursive-entity error; minidom behavior differs.')

manifest={'schema_version':1,'generated_by':'_bmad-output/implementation-artifacts/generate-format-references.py','deterministic':True,'zip_semantics':'ZIP names map to UTF-8 strings; methods 0=stored, 8=deflate; crc32 lowercase hex. Error names are semantic categories, implementation error enums may differ.','xml_semantics':'Expected expanded element/attribute names use ElementTree {uri}local convention. Lexical CDATA distinction may be omitted by standard-library parsers.','fixtures':specs}
(ROOT/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
