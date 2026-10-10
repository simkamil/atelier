from pathlib import Path
import json,re
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
root=Path.cwd()
out=root/'graphify-out'
doc=json.loads((out/'story-dependencies.json').read_text(encoding='utf-8'))
# Preserve source-grounded semantic concepts introduced by the story agents.
# Their documentation nodes complement, rather than replace, the Rust AST.
previous=json.loads((out/'graph.json').read_text(encoding='utf-8'))
known_hyperedges={entry['id'] for entry in doc.get('hyperedges',[])}
for entry in previous.get('hyperedges',[]):
    if entry['id'] not in known_hyperedges:
        doc.setdefault('hyperedges',[]).append(entry)
        known_hyperedges.add(entry['id'])
document_paths={'docs/DEVELOPMENT.md','docs/us-014-encodings.md','docs/us-060-geometry.md','docs/us-070-indexed-tree.md','docs/us-020-021-compression.md','docs/us-071-transactions.md','docs/us-080-display-list.md','crates/compress/tests/corpus/mixed-utf8.txt','crates/compress/tests/corpus/repetitive.txt','.github/workflows/ci.yml'}
known={node['id'] for node in doc['nodes']}
preserved=set()
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
for rel in ['README.md','docs/DEVELOPMENT.md','docs/us-014-encodings.md','docs/us-060-geometry.md','docs/us-070-indexed-tree.md','docs/us-020-021-compression.md','docs/us-071-transactions.md','docs/us-080-display-list.md','web/README.md','data/README.md','tests/README.md','.github/workflows/ci.yml']:
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
ast=extract(code,cache_root=root,root=root,parallel=False)
cargo=introspect_cargo(root)
merged={'nodes':ast['nodes']+cargo['nodes']+doc['nodes'],'edges':ast['edges']+cargo['edges']+doc['edges'],'hyperedges':doc.get('hyperedges',[]),'input_tokens':0,'output_tokens':0}
node_ids={n['id'] for n in merged['nodes']}
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
}
for node in removed:
    key=source_key(node)
    replacement=located.get(key) if key and node.get('type')=='package' else None
    placeholder=node.get('_origin')=='ast' and not key and not node.get('source_location')
    moved=verified_symbol_replacements.get(node['id'])
    if moved in g and g.nodes[moved].get('source_file')==node.get('source_file'):
        replacement=moved
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
report += f'\n## Périmètre de cette mise à jour\n\nLa carte combine les dépendances documentaires déclarées (158 US), l’AST des sources Rust et les métadonnées Cargo réellement présentes. Les crates vides ne représentent pas des fonctionnalités implémentées. La commande `py -m graphify . --cargo` a réussi après mise en cache complète des documents et exclusion du runtime agent ; les API locales de Graphify régénèrent ce rapport et ses libellés. Cargo.toml est couvert par les métadonnées de ses {len(cargo["nodes"])} paquets, même si son extraction AST produit zéro nœud. Le coût LLM de cette mise à jour est nul ; le coût historique reste dans cost.json.\n'
(out/'GRAPH_REPORT.md').write_text(report,encoding='utf-8')
(out/'story-dependencies.json').write_text(json.dumps(doc,ensure_ascii=False,indent=2),encoding='utf-8')
(out/'.graphify_extract.json').write_text(json.dumps(merged,ensure_ascii=False,indent=2),encoding='utf-8')
(out/'.graphify_detect.json').write_text(json.dumps(detection,ensure_ascii=False),encoding='utf-8')
(out/'.graphify_labels.json').write_text(json.dumps(labels,ensure_ascii=False),encoding='utf-8')
(out/'.graphify_analysis.json').write_text(json.dumps({'communities':communities,'cohesion':cohesion,'gods':gods,'surprises':surprises,'questions':questions},ensure_ascii=False),encoding='utf-8')
save_semantic_cache(doc['nodes'],doc['edges'],doc.get('hyperedges',[]),root=root,prompt=_extraction_system(deep=False))
health=diagnose_extraction(merged,root=str(root))
print(json.dumps({'nodes':len(g),'edges':g.number_of_edges(),'code_files':len(code),'communities':len(communities),'health':{k:health[k] for k in ['dangling_endpoint_edges','missing_endpoint_edges','same_endpoint_group_count']}},ensure_ascii=False))
