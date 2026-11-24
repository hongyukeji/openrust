import os
from pathlib import Path
import subprocess,json,random,statistics,math
out=Path(os.environ.get('ARRAY_PERF_DIR',Path.cwd()/'target/tmp/array-release'));rng=random.Random(4937);rows=[]
for i in range(151):
 order=['before','after'];rng.shuffle(order);row={}
 for v in order:
  p=subprocess.run(['array-perf','faker_new_cold','1'],executable=str(out/'bin'/f'{v}_time'),capture_output=True,text=True,check=True)
  row[v]=json.loads(p.stdout)
 rows.append(row)
a=[r['before']['elapsed_ns'] for r in rows];b=[r['after']['elapsed_ns'] for r in rows];log=[math.log(y/x) for x,y in zip(a,b)];boots=[]
for _ in range(10000):boots.append((math.exp(statistics.median(rng.choices(log,k=len(log))))-1)*100)
boots.sort();r={'case':'faker_new_cold','fixed_argv0':'array-perf','samples_per_version':151,'before_median_ns':statistics.median(a),'after_median_ns':statistics.median(b),'paired_median_change_pct':(math.exp(statistics.median(log))-1)*100,'paired_bootstrap_ci95_pct':[boots[250],boots[9749]]}
(out/'argv-control.json').write_text(json.dumps({'summary':r,'samples':rows},indent=2));print(json.dumps(r,indent=2))
