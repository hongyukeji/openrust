import os
from pathlib import Path
import subprocess,resource,random,json,statistics
out=Path(os.environ.get('ARRAY_PERF_DIR', Path.cwd()/'target/tmp/array-release'));rng=random.Random(9871);rows=[]
for i in range(101):
 cases=[(v,n) for v in ['before','after'] for n in [0,1]];rng.shuffle(cases);row={}
 for v,n in cases:
  a=resource.getrusage(resource.RUSAGE_CHILDREN)
  p=subprocess.run(['array-perf','faker_new_cold',str(n)],executable=str(out/'bin'/f'{v}_time'),capture_output=True,text=True,check=True)
  b=resource.getrusage(resource.RUSAGE_CHILDREN)
  row[f'{v}_{n}']={'minor_faults':b.ru_minflt-a.ru_minflt,'major_faults':b.ru_majflt-a.ru_majflt,'user_s':b.ru_utime-a.ru_utime,'system_s':b.ru_stime-a.ru_stime,'timing':json.loads(p.stdout)}
 rows.append(row)
summary={}
for v in ['before','after']:
 summary[v]={}
 for metric in ['minor_faults','major_faults']:
  summary[v][metric]={'zero_iteration_median':statistics.median(r[f'{v}_0'][metric] for r in rows),'first_constructor_median':statistics.median(r[f'{v}_1'][metric] for r in rows),'paired_increment_median':statistics.median(r[f'{v}_1'][metric]-r[f'{v}_0'][metric] for r in rows)}
(out/'page-faults.json').write_text(json.dumps({'scope':'whole child process; n=0 control versus n=1 constructor, not function-local hardware counters','summary':summary,'samples':rows},indent=2))
print(json.dumps(summary,indent=2))
