"""在 prepare.py 的隔离副本中复验路由契约；由 build_guard.py 调用。"""
from pathlib import Path
import os,subprocess,json,sys
b=Path(__file__).resolve().parents[5]/'target/tmp/route-compiled-prototype';source=b/'source';variant=sys.argv[1];env=dict(os.environ,CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',ROUTE_PROTOTYPE_CACHE_OUTPUT=str(b/f'cache-{variant}'))
base=['cargo','test','--locked','-p','openrust-framework']
commands=[('routing',base+sum([['--test',name] for name in ['routing_literal_patterns','routing_methods','routing_groups','routing_shortcuts','routing_named','request_input','route_metadata','form_request_unknown_fields','validation_aware_tutorial']],[])),('binding',base+['--test','eloquent_route_binding','eloquent_bindings_run_in_http_middleware_on_sqlite','--','--exact']),('application',['cargo','test','--locked','-p','openrust','--test','startup','--test','console','--test','resource_generator','--','--test-threads=1']),('fmt',['cargo','fmt','--all','--check']),('clippy',['cargo','clippy','--locked','-p','openrust-framework','-p','openrust','--all-targets','--','-D','warnings'])]
results=[]
for name,cmd in commands:
 with (b/f'{variant}-{name}.log').open('w') as log:r=subprocess.run(cmd,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT)
 results.append(dict(name=name,exit=r.returncode,command=cmd));(b/f'{variant}-gates.json').write_text(json.dumps(results,indent=2))
 print(variant,name,r.returncode,flush=True)
 if r.returncode:print((b/f'{variant}-{name}.log').read_text()[-4000:]);sys.exit(r.returncode)
