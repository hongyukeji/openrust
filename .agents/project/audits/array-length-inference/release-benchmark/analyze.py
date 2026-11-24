import os
from pathlib import Path
import json,statistics,math,random
out=Path(os.environ.get('ARRAY_PERF_DIR', Path.cwd()/'target/tmp/array-release'));samples=json.loads((out/'samples.json').read_text());rng=random.Random(192731);summary={}
for case,rows in samples.items():
 a=[r['before']['elapsed_ns']/r['before']['iterations'] for r in rows];b=[r['after']['elapsed_ns']/r['after']['iterations'] for r in rows]
 ratios=[math.log(y/x) for x,y in zip(a,b) if x>0 and y>0]
 row={'samples_per_version':len(rows),'before_median_ns':statistics.median(a),'after_median_ns':statistics.median(b),'before_p95_ns':sorted(a)[int(.95*(len(a)-1))],'after_p95_ns':sorted(b)[int(.95*(len(b)-1))],'zero_duration_pairs':sum(x==0 or y==0 for x,y in zip(a,b))}
 if case!='noop_cold':
  assert len(ratios)==len(rows),case
  boots=[]
  for _ in range(10000):boots.append(math.exp(statistics.median(rng.choices(ratios,k=len(ratios))))-1)
  boots.sort();row.update(paired_median_change_pct=(math.exp(statistics.median(ratios))-1)*100,paired_bootstrap_ci95_pct=[boots[250]*100,boots[9749]*100])
 else:row['relative_change']='not defined at timer floor; absolute control only'
 summary[case]=row
methodology={'seed':192731,'warm_pairs':31,'cold_pairs':151,'bootstrap_resamples':10000,'order':'randomized paired process calls','argv0':'array-perf (identical in both arms)','time_binary':'standard allocator, no counters','alloc_binary':'separate counted System allocator, five repeats','practical_regression_threshold_pct':5,'cold_overhead_control':'noop, absolute only; zero measurements never divided'}
(out/'summary.json').write_text(json.dumps({'timing':summary,'methodology':methodology},indent=2))
print(json.dumps(summary,indent=2))
