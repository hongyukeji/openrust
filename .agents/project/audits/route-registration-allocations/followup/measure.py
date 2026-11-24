from pathlib import Path
import subprocess,json,random,statistics,math,time,os
out=Path(os.environ.get('ROUTE_PERF_DIR',Path.cwd()/'target/tmp/route-registration-followup')).resolve()
if not out.is_relative_to((Path.cwd()/'target/tmp').resolve()):
 raise SystemExit('测量目录必须位于项目target/tmp内')
rng=random.Random(872103);cases={'routes_get':100,'routes_get_cold':1,'routes_any':100,'routes_dynamic':10,'routes_duplicate':100,'pattern':100,'regex':100,'match_first':100000,'match_last':100000,'match_miss':100000};data={};samples={c:[] for c in cases}
def run(v,kind,case,n):
 p=subprocess.run(['registration-perf',case,str(n)],executable=str(out/'bin'/f'{v}_{kind}'),capture_output=True,text=True,check=True,timeout=60);return json.loads(p.stdout)
for v in ['before','after']:
 assert run(v,'time','verify',1)['verified'];assert run(v,'alloc','allocator_self_test',1)['self_test'];data[v]={}
 for c in cases:
  data[v][c]=[run(v,'alloc',c,1) for _ in range(5)];assert all(row==data[v][c][0] for row in data[v][c]),(v,c)
(out/'allocations-final.json').write_text(json.dumps(data,indent=2))
for i in range(31):
 names=list(cases);rng.shuffle(names)
 for c in names:
  order=['before','after'];rng.shuffle(order);r={v:run(v,'time',c,cases[c]) for v in order};samples[c].append(r)
 if i%10==0:print('round',i+1,flush=True)
(out/'samples-final.json').write_text(json.dumps(samples,indent=2));summary={}
for c,rows in samples.items():
 a=[r['before']['elapsed_ns']/r['before']['iterations'] for r in rows];b=[r['after']['elapsed_ns']/r['after']['iterations'] for r in rows];ratios=[math.log(y/x) for x,y in zip(a,b)];boots=sorted((math.exp(statistics.median(rng.choices(ratios,k=31)))-1)*100 for _ in range(10000))
 summary[c]={'before_median_ns':statistics.median(a),'after_median_ns':statistics.median(b),'paired_change_pct':(math.exp(statistics.median(ratios))-1)*100,'ci95_pct':[boots[250],boots[9749]],'pairs':31,'before_range_ns':[min(a),max(a)],'after_range_ns':[min(b),max(b)],'before_iqr_ns':[sorted(a)[7],sorted(a)[23]],'after_iqr_ns':[sorted(b)[7],sorted(b)[23]]}
(out/'timing-final.json').write_text(json.dumps(summary,indent=2));print(json.dumps(summary,indent=2),flush=True)
