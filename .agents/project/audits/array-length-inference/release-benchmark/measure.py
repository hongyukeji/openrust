import os
from pathlib import Path
import subprocess,json,time,random,statistics,math,os
root=Path.cwd();out=Path(os.environ.get('ARRAY_PERF_DIR', root/'target/tmp/array-release'));bins=out/'bin'
cases={'methods':1000000,'routes_get':100,'routes_any':100,'application_create':2000,'faker_new':100000,'faker_format':300000,'elements':30000000,'faker_new_cold':1,'faker_format_cold':1,'elements_cold':1,'noop_cold':1}
seed=192731; rng=random.Random(seed); samples={k:[] for k in cases};allocation={}
def call(version,kind,case,n):
 p=subprocess.run(['array-perf',case,str(n)],executable=str(bins/f'{version}_{kind}'),capture_output=True,text=True,timeout=60)
 if p.returncode:raise RuntimeError((version,kind,case,p.stderr))
 return json.loads(p.stdout)
for v in ['before','after']:
 assert call(v,'alloc','allocator_self_test',1)['self_test']
 assert call(v,'time','verify',1)['verified']
 allocation[v]={}
 for case in cases:
  n=1 if case.endswith('cold') else 10 if case.startswith('routes') else 100
  allocation[v][case]=[call(v,'alloc',case,n) for _ in range(5)]
  rows=allocation[v][case]
  assert all({k:w for k,w in r.items() if k not in ['elapsed_ns']}==rows[0] for r in rows),(v,case,'allocation results vary')
(out/'allocations.json').write_text(json.dumps(allocation,indent=2))
print('Allocation counters self-tested; all cases repeatable',flush=True)
if os.environ.get('ARRAY_PERF_CHECK_ONLY') == '1':
 import sys
 sys.exit(0)
# 交错两组，每对随机顺序；冷测点每次由全新进程执行且只调用一次。
start=time.monotonic()
for iteration in range(151):
 names=list(cases);rng.shuffle(names)
 for case in names:
  if not case.endswith('cold') and iteration>=31:continue
  order=['before','after'];rng.shuffle(order);row={}
  for version in order:row[version]=call(version,'time',case,cases[case])
  samples[case].append(row)
 if iteration%10==0:
  (out/'samples.json').write_text(json.dumps(samples,indent=2));print('round',iteration+1,'seconds',round(time.monotonic()-start,1),flush=True)
(out/'samples.json').write_text(json.dumps(samples,indent=2))

import sys
subprocess.run([sys.executable,'-B',str(out/'analyze.py')],check=True)
