"""使用 Rust 语法树盘点命名与调用位置，字符串和注释单独列出，不自动改写。"""
import concurrent.futures
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]

def snake(name):
    name = re.sub(r'([A-Z]+)([A-Z][a-z])', r'\1_\2', name)
    return re.sub(r'([a-z0-9])([A-Z])', r'\1_\2', name).lower()

def paths():
    names = subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=ROOT).decode().split('\0')
    return sorted({name for name in names if name.endswith('.rs') and (name.startswith(('vendor/openrust-','app/','bootstrap/','config/','routes/','database/','tests/')) or '/' not in name) and (ROOT/name).is_file()})

def parse(name):
    data=(ROOT/name).read_bytes()
    tree=json.loads(subprocess.check_output(['rust-analyzer','parse','--json'],input=data))
    definitions=[];tokens=[];literals=[];lexemes=[]
    def walk(node, ancestors, owner=0):
        kind=node['kind']
        if kind in {'IMPL','TRAIT','MODULE'}:
            owner=node['start'][0]+1
        if node['type']=='Token':
            token=node.get('text','')
            if kind not in {'WHITESPACE','COMMENT'}:
                lexemes.append(dict(kind=kind,text=token,start=node['start'][0],end=node['end'][0],line=node['start'][1]+1,context=ancestors[-4:],external='EXTERN_BLOCK' in ancestors,scope=owner))
            if kind=='IDENT':
                record=dict(file=name,line=node['start'][1]+1,start=node['start'][0],end=node['end'][0],name=token,context=ancestors[-4:],scope=owner)
                tokens.append(record)
                if ancestors and ancestors[-1]=='NAME' and len(ancestors)>1:
                    parent=ancestors[-2]
                    if parent in {'FN','STRUCT','ENUM','TRAIT','TYPE_ALIAS','CONST','STATIC','MODULE','VARIANT','RECORD_FIELD','IDENT_PAT','MACRO_RULES'}:
                        record=dict(record,kind=parent,external='EXTERN_BLOCK' in ancestors)
                        if parent in {'FN','RECORD_FIELD','IDENT_PAT','MODULE','MACRO_RULES'} and any(c.isupper() for c in token) and (token.lstrip('_')[:1].islower() or parent in {'FN','MACRO_RULES'}):
                            definitions.append(dict(record,target=snake(token)))
                        elif parent in {'CONST','STATIC'} and token.upper()!=token:
                            definitions.append(dict(record,target=snake(token).upper()))
            elif kind in {'STRING','COMMENT','BYTE_STRING','C_STRING'}:
                literals.append(dict(file=name,line=node['start'][1]+1,start=node['start'][0],end=node['end'][0],text=token,kind=kind))
            return
        for child in node.get('children',[]):walk(child,ancestors+[kind],owner)
    walk(tree,[])
    declared={d['start'] for d in definitions}
    for index, item in enumerate(lexemes[:-1]):
        next_item=lexemes[index+1]
        if item['kind']=='FN_KW' and 'TOKEN_TREE' in item['context'] and next_item['kind']=='IDENT':
            identifier=next_item['text']
            if any(c.isupper() for c in identifier) and next_item['start'] not in declared:
                definitions.append(dict(file=name,line=next_item['line'],start=next_item['start'],end=next_item['end'],name=identifier,context=next_item['context'],scope=next_item['scope'],kind='FN_MACRO',target=snake(identifier),external=next_item['external']))
    return definitions,tokens,literals

def main():
    output=ROOT/'target/tmp/rust-naming';output.mkdir(parents=True,exist_ok=True)
    definitions=[];tokens=[];literals=[];lexemes=[];names=paths()
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        for defs,refs,strings in pool.map(parse,names):
            definitions+=defs;tokens+=refs;literals+=strings
    mapping={d['name']:d['target'] for d in definitions if d['kind']!='CONST' and not d['external']}
    refs=[t for t in tokens if t['name'] in mapping]
    names_set=set(mapping)
    bridges=[t for t in literals if names_set.intersection(re.findall(r'[A-Za-z_][A-Za-z_0-9]*',t['text']))]
    collisions=[]
    by_target={}
    for old,new in mapping.items():by_target.setdefault(new,[]).append(old)
    for target,old in by_target.items():
        if len(old)>1:collisions.append(dict(target=target,sources=old))
    scope_names={}
    for token in tokens:
        if token['context'][-2:]==['FN','NAME']:
            scope_names.setdefault((token['file'],token['scope']),{}).setdefault(mapping.get(token['name'],token['name']),set()).add(token['name'])
    existing_collisions=[dict(file=file,scope=scope,target=target,sources=sorted(old))
        for (file,scope),targets in scope_names.items() for target,old in targets.items() if len(old)>1]
    result=dict(existing_method_collisions=existing_collisions,files=names,definitions=definitions,identifiers=refs,strings_and_comments=bridges,mapping=mapping,spelling_collisions=collisions)
    (output/'inventory.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps(dict(files=len(names),declarations=len(definitions),spellings=len(mapping),references=len(refs),bridges=len(bridges),collisions=collisions,existing_method_collisions=existing_collisions),ensure_ascii=False))

if __name__=='__main__':main()
