# SPDX-License-Identifier: AGPL-3.0-or-later
from pathlib import Path
import json,re,subprocess
import sys
from graphify.detect import detect
from graphify.extract import extract
from graphify.cargo_introspect import introspect_cargo
from graphify.build import build_from_json
from graphify.cluster import cluster,score_all
from graphify.analyze import god_nodes,surprising_connections,suggest_questions
from graphify.report import generate
from graphify.export import to_json
from graphify.diagnostics import diagnose_extraction
from graphify.cache import save_semantic_cache
from graphify.llm import _extraction_system
if sys.argv[1:] == ['--inspect-ast']:
    snapshot=Path(__file__).resolve().parents[2]/'target/ui-final-ast/graphify-out/graph.json'
    graph=json.loads(snapshot.read_text(encoding='utf-8'))
    print(json.dumps({'path':snapshot.as_posix(),'nodes':len(graph['nodes']),'edges':len(graph.get('links',graph.get('edges',[])))},ensure_ascii=False))
    raise SystemExit(0)
if sys.argv[1:] == ['--inspect-routes']:
    project=Path(__file__).resolve().parents[2]
    resource_file=project/'crates/desktop-win/src/resources.rs'
    source=resource_file.read_text(encoding='utf-8')
    routes={match.group(1) for match in re.finditer(r'Resource\s*\{\s*path:\s*"([^"]+)".*?bytes:\s*include_bytes!\("([^"]+)"\).*?\}',source,re.S)}
    refs=[]
    for file in sorted((project/'web').glob('*')):
        if not file.is_file() or file.suffix.lower() not in ('.html','.mjs','.css'):
            continue
        content=file.read_text(encoding='utf-8')
        refs.extend({'file':file.relative_to(project).as_posix(),'url':match.group(1)} for match in re.finditer(r'(?:from\s*|import\s*\(?\s*|new\s+Worker\s*\(|fetch\s*\(|src\s*=\s*|href\s*=\s*|url\s*\(\s*)["\'](/[^"\'?#]*)',content))
    missing=sorted({ref['url'] for ref in refs if ref['url'] not in routes})
    print(json.dumps({'native_route_count':len(routes),'web_root_url_references':len(refs),'missing_web_routes':missing},ensure_ascii=False))
    raise SystemExit(0)
if sys.argv[1:] != ['--finalize']:
    raise SystemExit('Usage: py ui-shell-graph-refresh.py --inspect-ast | --inspect-routes | --finalize (finalize only after parent GO)')
root=Path(__file__).resolve().parents[2]
out=root/'graphify-out'
doc=json.loads((out/'story-dependencies.json').read_text(encoding='utf-8'))
# Preserve source-grounded semantic concepts introduced by the story agents.
# Their documentation nodes complement, rather than replace, the Rust AST.
previous=json.loads((out/'graph.json').read_text(encoding='utf-8'))
# CLI semantic extraction omitted attribution for two explicitly referenced
# concepts. Ground them in the actual documentation before preservation.
concept_sources={'uax_29':('docs/us-011-segmentation.md','uax29'),
                 'us_090_wasm_abi':('docs/us-091-binary-protocol.md','us090')}
for node in previous['nodes']:
    if not node.get('source_file') and node['id'] in concept_sources:
        relative,needle=concept_sources[node['id']]
        lines=(root/relative).read_text(encoding='utf-8').splitlines()
        line=next(i+1 for i,value in enumerate(lines) if needle in re.sub(r'[^a-z0-9]','',value.lower()))
        node['source_file']=relative
        node['source_location']=str(line)
known_hyperedges={entry['id'] for entry in doc.get('hyperedges',[])}
for entry in previous.get('hyperedges',[]):
    if entry['id'] not in known_hyperedges:
        doc.setdefault('hyperedges',[]).append(entry)
        known_hyperedges.add(entry['id'])
document_paths={'docs/DEVELOPMENT.md','docs/us-014-encodings.md','docs/us-060-geometry.md','docs/us-070-indexed-tree.md','docs/us-020-021-compression.md','docs/us-071-transactions.md','docs/us-080-display-list.md','docs/us-022-023-zip.md','docs/us-030-031-xml.md','crates/compress/tests/corpus/mixed-utf8.txt','crates/compress/tests/corpus/repetitive.txt','.github/workflows/ci.yml'}
document_paths.update({'crates/font/tests/fixtures/README.md', 'crates/font/tests/fixtures/fixtures.json', 'docs/us-050-opentype-tables.md', 'docs/us-073-selection.md', 'docs/us-074-recovery-journal.md', 'docs/MODEL-FONT-PROGRESS.md', 'docs/FORMATS-PROGRESS.md', 'docs/us-032-opaque-xml.md', 'docs/us-072-undo-history.md', 'docs/us-081-svg-backend.md', 'docs/EDITING-PROGRESS.md'})
document_paths.update(['tests/image-reference/README.md', 'data/fonts/README.md', 'docs/us-040-png.md', 'docs/us-043-gif-bmp.md', 'docs/us-056-embedded-fonts.md', 'docs/IMAGES-FONTS-PROGRESS.md'])
prototype_docs=['docs/us-012-line-breaks.md','docs/us-054-latin-shaping.md','docs/us-055-font-manager.md','docs/us-011-segmentation.md','docs/us-053-kerning-ligatures.md','docs/us-091-binary-protocol.md','tests/font-layout-reference/README.md','docs/us-010-ucd-tables.md','docs/us-051-truetype-outlines.md','docs/us-090-wasm-abi.md','docs/PROTOTYPE-PROGRESS.md','tests/unicode-reference/README.md','data/ucd/README.md']
prototype_docs.extend(['docs/us-092-worker.md','docs/us-110-win32-window.md','docs/us-116-portable-data.md'])
prototype_docs.extend(['docs/us-101-canvas-renderer.md','docs/us-103-text-input.md','docs/us-111-webview2.md','data/webview2/README.md'])
prototype_docs.extend(['web/index.html','web/app.css','web/i18n/fr-FR.txt'])
prototype_docs.extend(['docs/us-120-editor-engine.md','docs/us-120-editor-ui.md','docs/us-113-file-bridge.md'])
prototype_docs.extend(['docs/us-112-embedded-resources.md','docs/us-113-file-access.md','docs/us-120-plain-text-model.md','data/engine/README.md'])
prototype_docs.extend(['docs/us-221-keyboard-editing.md','docs/us-222-mouse-selection.md','docs/us-225-plain-clipboard.md'])
prototype_docs.extend(['docs/us-013-case-normalization.md','docs/us-041-png-encoder.md','docs/us-052-cff-outlines.md','docs/us-200-rich-text-model.md'])
prototype_docs.extend(['docs/us-042-jpeg.md','docs/us-061-software-rasterizer.md','docs/us-201-style-engine.md','docs/us-300-opc-package.md','crates/image/tests/fixtures/jpeg/README.md'])
prototype_docs.extend(['docs/us-301-docx-import.md','docs/us-210-rich-line-layout.md','tests/docx-reference/README.md','tests/text-layout-reference/README.md'])
prototype_docs.extend(['docs/us-211-tabs.md','docs/us-212-pagination.md','docs/us-214-caret-geometry.md'])
prototype_docs.extend(['docs/us-202-default-document.md','docs/us-213-incremental-layout.md','docs/us-215-paragraph-borders.md'])
prototype_docs.extend(['docs/us-004-fuzzer.md','docs/us-082-raster-backend.md','docs/us-311-cfb-container.md','tests/cfb-reference/README.md','tests/font-cff-reference/README.md'])
prototype_docs.extend(['docs/us-104-105-106-interface.md','web/ui-components.mjs','web/ui-components.css','web/ribbon.mjs','web/ribbon.css','web/theme.mjs','web/theme.css','tests/ui-components-browser.mjs','tests/ribbon-browser.mjs','tests/theme.mjs','tests/theme-browser.mjs'])
prototype_docs.extend(['web/editor-shell.mjs','tests/ui-browser.mjs','tests/ui-browser.html'])
prototype_docs.extend(['docs/us-224-text-drag-drop.md','docs/us-228-french-spacing.md'])
prototype_docs.extend(['docs/rich-docx-preview.md','docs/SUBAGENT-WORKFLOW.md'])
document_paths.update(prototype_docs)
# Retain already-extracted JPEG fixture concepts at their real source paths.
# These versioned inputs are also exercised by the independent pixel tests.
document_paths.update(p.relative_to(root).as_posix() for p in (root/'crates/image/tests/fixtures/jpeg').glob('*.jpg'))
# The incremental CLI can restore cached semantic nodes for the three versioned
# raster PNGs. Preserve their real source paths, like the JPEG fixture concepts;
# reference_images_are_unchanged verifies these exact files in the workspace tests.
document_paths.update(p.relative_to(root).as_posix() for p in (root/'crates/raster/tests/reference').glob('*.png'))
document_paths.update({'web/index.html','web/app.css','web/app.mjs','web/i18n/fr-FR.txt'})
document_paths.update({'docs/us-104-105-106-interface.md','web/ui-components.mjs','web/ui-components.css','web/ribbon.mjs','web/ribbon.css','web/theme.mjs','web/theme.css','web/editor-shell.mjs','tests/ui-components-browser.mjs','tests/ribbon-browser.mjs','tests/theme.mjs','tests/theme-browser.mjs','tests/ui-browser.mjs','tests/ui-browser.html'})
document_paths.update({'docs/us-092-worker.md', 'docs/us-110-win32-window.md', 'docs/us-116-portable-data.md'})
document_paths.update({'README.md','web/README.md','tests/README.md','US-V0-V1.md'})
document_paths.update(p.relative_to(root).as_posix() for p in (root/'tests').glob('*browser.html'))
browser_html_lines = (root/'tests/worker-browser.html').read_text(encoding='utf-8').splitlines()
for node in previous['nodes'] + doc['nodes']:
    if node.get('source_file','').replace('\\','/') == 'tests/worker-browser.html':
        if node['id'] == 'tests_worker_browser_html':
            node['source_location'] = str(next(i+1 for i,line in enumerate(browser_html_lines) if '<h1>' in line))
        elif node['id'] == 'tests_worker_browser_result_pre':
            node['source_location'] = str(next(i+1 for i,line in enumerate(browser_html_lines) if '<pre id="result"' in line))
document_paths.update(p.relative_to(root).as_posix() for p in (root/'data/fonts/licenses').glob('*.txt'))
# Preserve source-grounded semantic documentation concepts from the frozen
# pre-CLI graph. A code-only refresh may already have pruned them from the
# current graph, so use the lot baseline as the recovery source.
baseline_commit='72874af6736729c52b715103108b77cab26ba215'
baseline_graph=json.loads(subprocess.run(
    ['git','show',f'{baseline_commit}:graphify-out/graph.json'],
    cwd=root,check=True,capture_output=True,text=True,encoding='utf-8'
).stdout)
baseline_semantic_nodes=[]
for node in baseline_graph['nodes']:
    source=node.get('source_file','').replace('\\','/')
    if node.get('_origin')!='semantic' or not source.startswith('docs/'):
        continue
    source_path=(root/source).resolve()
    try:
        relative=source_path.relative_to(root).as_posix()
    except ValueError:
        continue
    if source_path.is_file():
        baseline_semantic_nodes.append(node)
        document_paths.add(relative)
previous_ids={node['id'] for node in previous['nodes']}
for node in baseline_semantic_nodes:
    if node['id'] not in previous_ids:
        previous['nodes'].append(node)
        previous_ids.add(node['id'])
baseline_semantic_ids={node['id'] for node in baseline_semantic_nodes}
edge_key=lambda edge:(edge['source'],edge['target'],edge.get('relation'))
previous_edge_key=('links' if 'links' in previous else 'edges')
previous_edges=previous.setdefault(previous_edge_key,[])
known_previous_edges={edge_key(edge) for edge in previous_edges}
for edge in baseline_graph.get('links',baseline_graph.get('edges',[])):
    if edge['source'] in baseline_semantic_ids or edge['target'] in baseline_semantic_ids:
        if edge_key(edge) not in known_previous_edges:
            previous_edges.append(edge)
            known_previous_edges.add(edge_key(edge))
known={node['id'] for node in doc['nodes']}
preserved=set(baseline_semantic_ids)
for node in previous['nodes']:
    source=node.get('source_file')
    if not source or node.get('_origin')=='ast':
        continue
    try:
        relative=(root/source).resolve().relative_to(root).as_posix()
    except ValueError:
        continue
    if relative in document_paths and node['id'] not in known:
        if node['id']=='atelier_geom':
            node['rationale']='Local workspace dependency providing geometric types like Point and Rect.'
        doc['nodes'].append({k:v for k,v in node.items() if k not in {'community','community_name'}})
        known.add(node['id'])
        preserved.add(node['id'])
existing_edges={(edge['source'],edge['target'],edge.get('relation')) for edge in doc['edges']}
for edge in previous.get('links',previous.get('edges',[])):
    key=(edge['source'],edge['target'],edge.get('relation'))
    if (edge['source'] in preserved or edge['target'] in preserved) and key not in existing_edges:
        doc['edges'].append(edge)
        existing_edges.add(key)
us=(root/'US-V0-V1.md').read_text(encoding='utf-8')
headings=list(re.finditer(r'^#### (US-\d+) — ([^\n]+)',us,re.M))
for i,m in enumerate(headings):
    block=us[m.end():headings[i+1].start() if i+1<len(headings) else len(us)]
    node_id='us_v0_v1_'+m.group(1).lower().replace('-','_')
    line=us[:m.start()].count('\n')+1
    checkboxes=re.findall(r'^- \[([ x])\]',block,re.M)
    for node in doc['nodes']:
        if node['id']==node_id:
            node['source_location']=str(line)
            node['status']='done' if checkboxes and all(c=='x' for c in checkboxes) else 'open'
    reference=re.search(r'^Réf :',block,re.M)
    if reference:
        ref_line=us[:m.end()+reference.start()].count('\n')+1
        for edge in doc['edges']:
            if edge.get('source')==node_id:
                edge['source_location']=str(ref_line)
for rel in prototype_docs+['tests/image-reference/README.md', 'data/fonts/README.md', 'docs/us-040-png.md', 'docs/us-043-gif-bmp.md', 'docs/us-056-embedded-fonts.md', 'docs/IMAGES-FONTS-PROGRESS.md']+[p.relative_to(root).as_posix() for p in (root/'data/fonts/licenses').glob('*.txt')]+['crates/font/tests/fixtures/README.md','README.md','docs/DEVELOPMENT.md','docs/us-014-encodings.md','docs/us-060-geometry.md','docs/us-070-indexed-tree.md','docs/us-020-021-compression.md','docs/us-071-transactions.md','docs/us-080-display-list.md','docs/us-022-023-zip.md','docs/us-030-031-xml.md','docs/FORMATS-PROGRESS.md','docs/us-032-opaque-xml.md','docs/us-072-undo-history.md','docs/us-081-svg-backend.md','docs/EDITING-PROGRESS.md','docs/us-050-opentype-tables.md','docs/us-073-selection.md','docs/us-074-recovery-journal.md','docs/MODEL-FONT-PROGRESS.md','web/README.md','data/README.md','tests/README.md','.github/workflows/ci.yml']:
    p=root/rel
    if p.is_file():
        contents=p.read_text(encoding='utf-8')
        node_id='document_'+rel.replace('/','_').replace('.','_').lower()
        doc['nodes']=[n for n in doc['nodes'] if n['id']!=node_id]
        doc['nodes'].append({'id':node_id,'label':rel,'file_type':'document','source_file':str(p),'source_location':'1'})
        if 'SPEC.md' in contents:
            doc['edges']=[e for e in doc['edges'] if not(e['source']==node_id and e['target']=='spec_document')]
            doc['edges'].append({'source':node_id,'target':'spec_document','relation':'references','confidence':'EXTRACTED','confidence_score':1.0,'source_file':str(p),'source_location':str(contents[:contents.index('SPEC.md')].count('\n')+1)})
code=[p for folder in ['crates','tools'] for p in (root/folder).rglob('*.rs') if 'target' not in p.parts]
code += [root/'tools/package-engine.py']
code += [root/'crates/image/tests/fixtures/jpeg/generate.py']
code += [root/'tools/generate-docx-reference.py', root/'tools/word-line-reference.ps1', root/'tests/docx-reference/expected.json']
code += [root/'tools/generate-cfb-reference.py', root/'tools/generate-cff-reference.py', root/'tests/cfb-reference/expected.json', root/'tests/font-cff-reference/manifest.json']
code += [p for folder in ['web','tests'] for p in (root/folder).rglob('*.mjs')]
# Keep the byte-exact fixture manifest's JSON AST alongside the Rust AST.
# The Graphify CLI indexes it too; dropping it would lose located source nodes.
manifest=root/'tests/format-reference/manifest.json'
font_manifest=root/'crates/font/tests/fixtures/fixtures.json'
code_inputs=code+[p for p in (manifest,font_manifest,root/'tests/image-reference/manifest.json',root/'data/fonts/manifest.json',root/'data/ucd/17.0.0/manifest.json',root/'tests/unicode-reference/manifest.json',root/'tests/font-outline-reference/manifest.json',root/'tests/font-layout-reference/manifest.json',root/'tools/bench/references/wasm-size.json',root/'tests/check-assets.ps1',root/'tests/check-ucd.ps1',root/'tests/check-webview-assets.ps1',root/'tests/check-webview-host.ps1',root/'data/webview2/manifest.json',root/'data/engine/manifest.json',root/'tests/check-embedded-resources.ps1') if p.is_file()]
ast=extract(code_inputs,cache_root=root,root=root,parallel=False)
cargo=introspect_cargo(root)
merged={'nodes':ast['nodes']+cargo['nodes']+doc['nodes'],'edges':ast['edges']+cargo['edges']+doc['edges'],'hyperedges':doc.get('hyperedges',[]),'input_tokens':0,'output_tokens':0}
node_ids={n['id'] for n in merged['nodes']}
# Each include_bytes route directly names an actual compile-time resource.
resource_source=root/'crates/desktop-win/src/resources.rs'
resource_contents=resource_source.read_text(encoding='utf-8')
for match in re.finditer(r'include_bytes!\("([^"]+)"\)',resource_contents):
    asset=(resource_source.parent/match.group(1)).resolve()
    relative=asset.relative_to(root).as_posix()
    asset_id='embedded_'+relative.replace('/','_').replace('.','_').replace('-','_')
    if asset_id not in node_ids:
        merged['nodes'].append({'id':asset_id,'label':relative,'type':'embedded_asset','embedded_path':relative,'source_file':'crates/desktop-win/src/resources.rs','source_location':str(resource_contents[:match.start()].count('\n')+1)})
        node_ids.add(asset_id)
    merged['edges'].append({'source':'crates_desktop_win_src_resources','target':asset_id,'relation':'embeds','confidence':'EXTRACTED','confidence_score':1.0,'source_file':'crates/desktop-win/src/resources.rs','source_location':str(resource_contents[:match.start()].count('\n')+1)})

# Preserve the real URL -> native route -> embedded bytes -> source file chain.
route_targets={}
for match in re.finditer(r'Resource\s*\{\s*path:\s*"([^"]+)".*?bytes:\s*include_bytes!\("([^"]+)"\).*?\}',resource_contents,re.S):
    url,include_path=match.groups()
    asset=(resource_source.parent/include_path).resolve()
    relative=asset.relative_to(root).as_posix()
    asset_id='embedded_'+relative.replace('/','_').replace('.','_').replace('-','_')
    route_id='http_route_'+(re.sub(r'[^a-z0-9]+','_',url.casefold()).strip('_') or 'root')
    route_targets[url]=relative
    line=str(resource_contents[:match.start()].count('\n')+1)
    if route_id not in node_ids:
        merged['nodes'].append({'id':route_id,'label':url,'type':'http_route','source_file':'crates/desktop-win/src/resources.rs','source_location':line,'_origin':'extracted'})
        node_ids.add(route_id)
    merged['edges'].append({'source':'crates_desktop_win_src_resources','target':route_id,'relation':'serves_url','confidence':'EXTRACTED','confidence_score':1.0,'source_file':'crates/desktop-win/src/resources.rs','source_location':line})
    merged['edges'].append({'source':route_id,'target':asset_id,'relation':'serves_embedded_bytes','confidence':'EXTRACTED','confidence_score':1.0,'source_file':'crates/desktop-win/src/resources.rs','source_location':line})
    file_id=relative.rsplit('.',1)[0].replace('/','_').replace('-','_')
    if file_id not in node_ids:
        file_id='document_'+relative.replace('/','_').replace('.','_').replace('-','_').lower()
    if file_id in node_ids:
        merged['edges'].append({'source':route_id,'target':file_id,'relation':'serves_source_file','confidence':'EXTRACTED','confidence_score':1.0,'source_file':'crates/desktop-win/src/resources.rs','source_location':line})

def source_node_id(relative):
    module_id=relative.rsplit('.',1)[0].replace('/','_').replace('-','_')
    if module_id in node_ids:
        return module_id
    document_id='document_'+relative.replace('/','_').replace('.','_').replace('-','_').lower()
    return document_id if document_id in node_ids else None

for folder in ('web',):
    for source in sorted((root/folder).glob('*')):
        if not source.is_file() or source.suffix.lower() not in ('.html','.mjs','.css'):
            continue
        relative=source.relative_to(root).as_posix()
        source_id=source_node_id(relative)
        if not source_id:
            continue
        content=source.read_text(encoding='utf-8')
        for reference in re.finditer(r'(?:from\s*|import\s*\(?\s*|new\s+Worker\s*\(|fetch\s*\(|src\s*=\s*|href\s*=\s*|url\s*\(\s*)["\'](/[^"\'?#]*)',content):
            url=reference.group(1)
            route_id='http_route_'+(re.sub(r'[^a-z0-9]+','_',url.casefold()).strip('_') or 'root')
            if route_id in node_ids:
                merged['edges'].append({'source':source_id,'target':route_id,'relation':'requests_url','confidence':'EXTRACTED','confidence_score':1.0,'source_file':relative,'source_location':str(content[:reference.start()].count('\n')+1)})

# Ground import-specifier placeholders in an actual served file or import site.
# A node: standard library specifier is not a local filesystem source path.
for node in merged['nodes']:
    source=node.get('source_file','')
    requested=source.split('?',1)[0]
    if requested in route_targets:
        node['request_url']=source
        node['source_file']=route_targets[requested]
        node['source_location']='1'
    elif source.startswith('node:'):
        for importer in code:
            if importer.suffix != '.mjs':
                continue
            content=importer.read_text(encoding='utf-8')
            if source in content:
                node['external_module']=source
                node['source_file']=importer.relative_to(root).as_posix()
                node['source_location']=str(content[:content.index(source)].count('\n')+1)
                break

# Remove hyperedges whose referenced fixture concepts were aggregated above.
# Keep every resolvable member; build_from_json otherwise only warns and drops.
valid_hyperedges=[]
for entry in doc.get('hyperedges',[]):
    kept=[n for n in entry.get('nodes',[]) if n in node_ids]
    if kept:
        entry=dict(entry,nodes=kept)
        valid_hyperedges.append(entry)
doc['hyperedges']=valid_hyperedges
merged['hyperedges']=valid_hyperedges
# Resolve verified workspace imports that the Rust extractor leaves unqualified.
# Preserve std and wildcard placeholders in diagnostics instead of inventing nodes.
local_imports={
    'atelier_compress':'crates_compress_src_lib',
    'atelier_image':'crates_image_src_lib',
    'embedded':'crates_font_src_embedded',
    'atelier_font':'crates_font_src_lib',
    'atelier_unicode':'crates_unicode_src_lib',
    'atelier_ucd_gen':'tools_ucd_gen_src_lib',
    'atelier_xml':'crates_xml_src_lib',
    'atelier_bench':'tools_bench_src_lib',
    'crc32':'crates_compress_src_lib_crc32',
    'parse_csv':'tools_bench_src_lib_parse_csv',
    'history':'crates_model_src_history',
    'svg':'crates_render_src_svg',
}
for edge in merged['edges']:
    if edge.get('relation')!='imports_from' or edge['target'] in node_ids:
        continue
    replacement=local_imports.get(edge['target'])
    if edge['target']=='crate':
        source=Path(edge.get('source_file',''))
        if len(source.parts)>=3 and source.parts[0] in ('crates','tools'):
            prefix='_'.join(source.parts[:2]).replace('-','_')+'_src_'
            replacement=next((prefix+stem for stem in ('lib','main') if prefix+stem in node_ids),None)
    if replacement in node_ids:
        edge['target']=replacement
# File modules must retain a located link to their declaring root module.
for source in code:
    for match in re.finditer(r'^(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;',source.read_text(encoding='utf-8'),re.M):
        target=source.parent/(match.group(1)+'.rs')
        # An explicit #[path] attribute names the module source in Rust.
        prefix=source.read_text(encoding='utf-8')[:match.start()]
        attribute=re.search(r'#\[path\s*=\s*"([^"]+)"\]\s*$',prefix)
        if attribute:
            target=source.parent/attribute.group(1)
        if not target.is_file():
            continue
        source_id=source.relative_to(root).with_suffix('').as_posix().replace('/','_').replace('-','_')
        target_id=target.relative_to(root).with_suffix('').as_posix().replace('/','_').replace('-','_')
        if source_id in node_ids and target_id in node_ids:
            merged['edges'].append({'source':source_id,'target':target_id,'relation':'declares_module','confidence':'EXTRACTED','confidence_score':1.0,'source_file':source.relative_to(root).as_posix(),'source_location':str(source.read_text(encoding='utf-8')[:match.start()].count('\n')+1)})
unresolved=[e for e in merged['edges'] if e['source'] not in node_ids or e['target'] not in node_ids]
(out/'unresolved-structural-references.json').write_text(json.dumps(unresolved,ensure_ascii=False,indent=2),encoding='utf-8')
merged['edges']=[e for e in merged['edges'] if e not in unresolved]
g=build_from_json(merged,root=root)
communities=cluster(g)
cohesion=score_all(g,communities)
labels={k:' / '.join(g.nodes[n].get('label',n) for n in sorted(v,key=lambda n:g.degree(n),reverse=True)[:2]) for k,v in communities.items()}
gods=god_nodes(g)
surprises=surprising_connections(g,communities)
questions=suggest_questions(g,communities,labels)
detection=detect(root,extra_excludes=['target','_bmad','_bmad-output','graphify-out'])
# Preserve a recovery copy without depending on another machine's temp folder.
import shutil,tempfile
backup_marker=out/'.graphify_scope_backup'
if not backup_marker.exists() or not Path(backup_marker.read_text(encoding='utf-8')).is_file():
    backup=Path(tempfile.mkdtemp(prefix='atelier-graph-scope-'))/'graph-before-product-scope.json'
    shutil.copyfile(out/'graph.json',backup)
    backup_marker.write_text(str(backup),encoding='utf-8')
# The incremental CLI adds unresolved std-type placeholders and may choose a
# different ID for an existing manifest. Allow only those verified reductions;
# never remove a located source symbol without an equivalent replacement.
old_graph=json.loads((out/'graph.json').read_text(encoding='utf-8'))
removed=[n for n in old_graph['nodes'] if n['id'] not in g]
def source_key(node):
    source=node.get('source_file')
    return (str((root/source).resolve()).casefold(),node.get('label')) if source else None
located={source_key(data):node_id for node_id,data in g.nodes(data=True) if source_key(data)}
cleanup=[]
verified_symbol_replacements={
    'crates_model_src_lib_indexedtree_t_allocate':'crates_model_src_lib_radixarena_t_allocate',
    'crates_model_src_lib_remove_all_then_insert_never_reuses_id_or_slot':'crates_model_src_lib_remove_all_then_insert_keeps_id_unique_and_reuses_slot',
    # The compressor's writer now borrows caller storage; Graphify includes
    # the explicit lifetime in the impl identifier. Verified same methods.
    'crates_compress_src_lib_bitwriter_finish':'crates_compress_src_lib_bitwriter_a_finish',
    'crates_compress_src_lib_bitwriter_write':'crates_compress_src_lib_bitwriter_a_write',
    # ZIP split per-entry size/ratio checks from the cumulative check, which now
    # runs after duplicate classification. Both replacement functions exist.
    'crates_zip_src_lib_size_limit':'crates_zip_src_lib_entry_size_limit',
}
for node in removed:
    key=source_key(node)
    replacement=located.get(key) if key and node.get('type')=='package' else None
    placeholder=node.get('_origin')=='ast' and not key and not node.get('source_location')
    moved=verified_symbol_replacements.get(node['id'])
    if moved in g and g.nodes[moved].get('source_file')==node.get('source_file'):
        replacement=moved
    # Corpus pixels are represented by the independently verified manifest,
    # not visual LLM concepts. Only prune verified fixture paths; retain the
    # CLI backup and record every replacement in normalization diagnostics.
    source=node.get('source_file','').replace('\\','/')
    # Query memories are generated context, excluded from the product map.
    # Preserve their concepts through the corresponding located product nodes.
    if source.startswith('graphify-out/memory/'):
        query_equivalents = {
            'rasterizer': ('crates_raster_src_lib_rasterizer', 'Rasterizer'),
            'us_311': ('us_v0_v1_us_311', 'US-311'),
            'us_052': ('us_v0_v1_us_052', 'US-052'),
            'us_082': ('us_v0_v1_us_082', 'US-082'),
        }
        if node['id'] in query_equivalents:
            candidate, prefix = query_equivalents[node['id']]
            assert candidate in g and g.nodes[candidate]['label'].startswith(prefix)
            assert g.nodes[candidate].get('source_file')
            replacement = candidate
    if source.startswith('tests/image-reference/') and Path(source).suffix.lower() in ('.png','.gif','.bmp'):
        fixture_manifest=json.loads((root/'tests/image-reference/manifest.json').read_text(encoding='utf-8'))
        if source.removeprefix('tests/image-reference/') in {c['file'] for c in fixture_manifest['cases']}:
            replacement='tests_image_reference_manifest' if 'tests_image_reference_manifest' in g else 'document_tests_image_reference_readme_md'
    if source.lower() == 'data/webview2/include/webview2.h':
        # System SDK symbols are deliberately excluded by .graphifyignore;
        # preserve their exact provenance via the checked manifest, not product AST.
        import hashlib
        sdk_manifest=json.loads((root/'data/webview2/manifest.json').read_text(encoding='utf-8'))
        assert hashlib.sha256((root/source).read_bytes()).hexdigest() == 'dff1e3181ec7ec203a34ef6efa966590e0ef0ba1a5c3fe3b69da6508c2f8a02e'
        replacement='document_data_webview2_readme_md'
    if source.lower() == 'data/webview2/license.txt':
        import hashlib
        assert hashlib.sha256((root/source).read_bytes()).hexdigest() == '0af8f1b807512aae39c2ac1aa4d0cae65cabecb6fd554b8439a5162a0d6eca55'
        replacement='document_data_webview2_readme_md'
    if node['id']=='sil_ofl_1_1' and not source:
        replacement='document_data_fonts_readme_md'
    if not placeholder and replacement is None:
        raise RuntimeError(f'Refused to remove located graph node: {node["id"]}')
    cleanup.append({'id':node['id'],'reason':'unresolved AST placeholder' if placeholder else 'equivalent located node','replacement':replacement})
if cleanup:
    import tempfile
    cleanup_backup=Path(tempfile.mkdtemp(prefix='atelier-graph-normalization-'))/'graph.json'
    shutil.copyfile(out/'graph.json',cleanup_backup)
    (out/'graph-normalization.json').write_text(json.dumps({'backup':str(cleanup_backup),'removed':cleanup},ensure_ascii=False,indent=2),encoding='utf-8')
if not to_json(g,communities,out/'graph.json',community_labels=labels,force=bool(cleanup)):
    raise RuntimeError('Refused to overwrite graph with smaller graph')
report=generate(g,communities,cohesion,labels,gods,surprises,detection,{'input':0,'output':0},str(root),suggested_questions=questions)
report += f'\nRéférences AST externes non résolues exclues du graphe : {len(unresolved)} (détail dans unresolved-structural-references.json). Le graphe simple peut fusionner plusieurs relations entre les mêmes symboles ; les relations brutes restent dans .graphify_extract.json.\n'
cost_path=out/'cost.json'
if cost_path.is_file():
    cost=json.loads(cost_path.read_text(encoding='utf-8'))
    report += f'\nCoût CLI cumulé enregistré : {cost.get("total_input_tokens",0)} tokens en entrée et {cost.get("total_output_tokens",0)} en sortie ; la génération locale de ce rapport par API utilise 0 token LLM.\n'
    if cost.get('unreported_runs'):
        report += f'\nTentatives CLI dont les tokens ne sont pas rapportés : {len(cost["unreported_runs"])}. Les compteurs ci-dessus couvrent uniquement les usages observés ; aucune estimation inventée n’est ajoutée.\n'
else:
    baseline_report=subprocess.run(['git','show',f'{baseline_commit}:graphify-out/GRAPH_REPORT.md'],cwd=root,check=True,capture_output=True,text=True,encoding='utf-8').stdout
    historical=re.search(r'Coût CLI cumulé enregistré : (\d+) tokens en entrée et (\d+) en sortie',baseline_report)
    if historical:
        report += f'\nHistorique conservé depuis GRAPH_REPORT.md au commit {baseline_commit} : {historical[1]} tokens en entrée, {historical[2]} en sortie. Le compteur cost.json ignoré n’est pas présent dans ce worktree ; aucune nouvelle mesure cumulée n’est prétendue. Cette mise à jour AST/API utilise 0 token LLM.\n'
report += f'\n## Périmètre de cette mise à jour\n\nLa carte combine les dépendances documentaires déclarées (158 US), l’AST des sources Rust/JavaScript et les métadonnées Cargo réellement présentes. Les crates vides ne représentent pas des fonctionnalités implémentées. La commande `py -m graphify . --code-only --no-cluster --no-viz` a rafraîchi les sources dans une sortie de contrôle ignorée ; les API locales régénèrent la carte canonique en préservant les concepts documentaires du commit {baseline_commit}. Les URLs des imports web et leurs routes natives sont reliées aux sources réellement servies. Cargo.toml est couvert par les métadonnées de ses {len(cargo["nodes"])} paquets, même si son extraction AST produit zéro nœud. Cette génération locale par API n’utilise pas de LLM ; les coûts historiques restent dans cost.json.\n'
(out/'GRAPH_REPORT.md').write_text(report,encoding='utf-8')
(out/'story-dependencies.json').write_text(json.dumps(doc,ensure_ascii=False,indent=2),encoding='utf-8')
(out/'.graphify_extract.json').write_text(json.dumps(merged,ensure_ascii=False,indent=2),encoding='utf-8')
(out/'.graphify_detect.json').write_text(json.dumps(detection,ensure_ascii=False),encoding='utf-8')
(out/'.graphify_labels.json').write_text(json.dumps(labels,ensure_ascii=False),encoding='utf-8')
(out/'.graphify_analysis.json').write_text(json.dumps({'communities':communities,'cohesion':cohesion,'gods':gods,'surprises':surprises,'questions':questions},ensure_ascii=False),encoding='utf-8')
save_semantic_cache(doc['nodes'],doc['edges'],doc.get('hyperedges',[]),root=root,prompt=_extraction_system(deep=False))
health=diagnose_extraction(merged,root=str(root))
print(json.dumps({'nodes':len(g),'edges':g.number_of_edges(),'code_files':len(code),'communities':len(communities),'health':{k:health[k] for k in ['dangling_endpoint_edges','missing_endpoint_edges','same_endpoint_group_count']}},ensure_ascii=False))
