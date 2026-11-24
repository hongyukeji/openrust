"""同输入、argv0及Release配置，分开计时／分配；所有原始样本留在项目工作流内。"""
from pathlib import Path
import json, subprocess, os, random, statistics, math, hashlib
ROOT = Path(__file__).resolve().parents[5]
OUT = Path(os.environ.get('ROUTE_PROTO_DIR', ROOT / 'target/tmp/route-compiled-prototype')).resolve()
assert OUT.is_relative_to(ROOT / 'target/tmp')
rng = random.Random(301124)
variants = ['baseline', 'prototype']
scenarios = [(shape, count) for shape in ['static', 'dynamic', 'overlap'] for count in [128, 1000, 10000]]
phases = ['register', 'register_cold', 'resident', 'resident_warm', 'compile', 'lookup_cold_first', 'lookup_cold_last', 'lookup_warm']

def run(variant, kind, *args):
    result = subprocess.run(['compiled-route-perf', *map(str, args)], executable=str(OUT / 'bin' / f'{variant}_{kind}'), capture_output=True, text=True, timeout=180, check=True)
    return json.loads(result.stdout)

for variant in variants:
    assert run(variant, 'alloc', 'self_test')['self_test']
cache_hashes = {}
allocations = {}
for shape, count in scenarios:
    key = f'{shape}-{count}'
    cache = [run(v, 'time', 'cache', shape, count) for v in variants]
    assert cache[0] == cache[1] and cache[0]['version'] == 1
    cache_hashes[key] = hashlib.sha256(json.dumps(cache[0], sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    rows = {v: {} for v in variants}
    for phase in phases:
        repetitions = {v: [] for v in variants}
        for _ in range(3):
            order = variants.copy(); rng.shuffle(order)
            for variant in order:
                repetitions[variant].append(run(variant, 'alloc', shape, count, phase, 100 if phase == 'lookup_warm' else 1))
        for variant in variants:
            values = repetitions[variant]
            rows[variant][phase] = {'runs': values, 'identical': all(value == values[0] for value in values)}
    allocations[key] = rows
    (OUT / 'allocations.json').write_text(json.dumps(allocations, indent=2))
    print('分配矩阵完成', key, flush=True)
(OUT / 'cache-hashes.json').write_text(json.dumps(cache_hashes, indent=2))
samples = {f'{shape}-{count}': [] for shape, count in scenarios}
for number in range(31):
    order = scenarios.copy(); rng.shuffle(order)
    for shape, count in order:
        variants_order = variants.copy(); rng.shuffle(variants_order)
        pair = {variant: run(variant, 'time', shape, count, 10 if count == 128 else 2 if count == 1000 else 1) for variant in variants_order}
        samples[f'{shape}-{count}'].append(pair)
    (OUT / 'samples.json').write_text(json.dumps(samples, indent=2))
    print('计时轮次', number + 1, '/31', flush=True)
summary = {}
for scenario, pairs in samples.items():
    metrics = ['registration_ns', 'compile_ns', 'compile_cold_ns', 'lookup_cold_first_ns', 'lookup_cold_last_ns']
    metrics += [f'lookup_{index}' for index in range(6)]
    output = {}
    for metric in metrics:
        def value(row):
            return row['lookup_ns'][int(metric.removeprefix('lookup_'))] if metric.startswith('lookup_') and metric.removeprefix('lookup_').isdigit() else row[metric]
        a = [value(pair['baseline']) for pair in pairs]
        b = [value(pair['prototype']) for pair in pairs]
        ratios = [math.log(y/x) for x, y in zip(a,b)] if all(x > 0 and y > 0 for x,y in zip(a,b)) else None
        boots = sorted((math.exp(statistics.median(rng.choices(ratios, k=31))) - 1) * 100 for _ in range(10000)) if ratios else None
        output[metric] = {'baseline_median': statistics.median(a), 'prototype_median': statistics.median(b),
            'paired_change_pct': (math.exp(statistics.median(ratios)) - 1) * 100 if ratios else None, 'ci95_pct': [boots[250], boots[9749]] if boots else None,
            'baseline_range': [min(a), max(a)], 'prototype_range': [min(b), max(b)],
            'baseline_iqr': [sorted(a)[7],sorted(a)[23]], 'prototype_iqr': [sorted(b)[7],sorted(b)[23]]}
    summary[scenario] = output
(OUT / 'timing.json').write_text(json.dumps(summary, indent=2))
print('全部测量完成', flush=True)
