"""从固定提交创建同基线Release A/B，生产工作区和Git索引保持不变。"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import time

BASE = 'a15c7b891dd8c121cbf936d1f8562638355cfa85'
INITIAL = 'ce75e29be167fe194b47219652a943123a8e9dd5'
FOLLOWUP = '2099a49a6143aab24d8302c99442fd5dc9db2a07'
root = Path.cwd().resolve()
here = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--revision', default=BASE)
parser.add_argument('--prepare-only', action='store_true', help='只核对两组输入，不编译或采样')
parser.add_argument('--output', default=f'target/tmp/array-release-repeat-{time.strftime("%Y%m%d-%H%M%S")}')
args = parser.parse_args()
if not ((root / 'AGENTS.md').is_file() and (root / 'Cargo.toml').is_file()):
    raise SystemExit('请在项目根执行')
out = (root / args.output).resolve()
if not out.is_relative_to(root / 'target/tmp') or out.exists():
    raise SystemExit('输出目录必须是项目target/tmp内尚未使用的目录')
out.mkdir(parents=True)
src = out / 'source'
src.mkdir()
archive = subprocess.Popen(['git', 'archive', args.revision], stdout=subprocess.PIPE)
with tarfile.open(fileobj=archive.stdout, mode='r|') as handle:
    handle.extractall(src, filter='data')
assert archive.wait() == 0
prefix = str(src.relative_to(root))
framework = src / 'vendor/openrust-framework'
app_file = framework / 'src/foundation/application.rs'
stub_file = framework / 'src/console/generator_command.rs'
providers = app_file.read_text().split('let base_providers:')[1].split('\n        ];')[0].count('Box::new(')
stubs = stub_file.read_text().split('pub(crate) const ALL:')[1].split('\n    ];')[0].count('Self::')
assert providers > 0 and stubs > 0

def git(*command):
    return subprocess.check_output(['git', *command])

initial = git('diff', INITIAL + '^', INITIAL, '--', 'vendor/openrust-framework/src').decode()
initial = ''.join('diff --git ' + part for part in initial.split('diff --git ')[1:]
                  if not part.startswith('a/vendor/openrust-framework/src/foundation/application.rs '))
initial = initial.replace('-    pub(crate) const ALL: [Self; 38] = [',
                          f'-    pub(crate) const ALL: [Self; {stubs}] = [')
patches = [initial.encode(), git('diff', FOLLOWUP + '^', FOLLOWUP, '--', 'vendor/openrust-framework/src')]
patch_paths = []
for index, data in enumerate(patches):
    path = out / f'{index}.patch'
    path.write_bytes(data)
    patch_paths.append(path)
for path in reversed(patch_paths):
    subprocess.run(['git', 'apply', '--reverse', '--directory=' + prefix, str(path)], check=True)
app_file.write_text(app_file.read_text().replace('let base_providers: [Box<dyn ServiceProvider>; _]',
                                               f'let base_providers: [Box<dyn ServiceProvider>; {providers}]'))
router = framework / 'src/routing/router.rs'
faker = framework / 'src/database/eloquent/factories/faker_provider.rs'
router.write_text(router.read_text() + '\nimpl Router {\n pub fn __array_perf_methods() -> [Method; 8] { Self::all_methods() }\n}\n')
faker.write_text(faker.read_text() + "\nimpl Faker {\n pub fn __array_perf_elements() -> &'static [Json] { default_elements() }\n}\n")
examples = framework / 'examples'
(examples / 'probe_support').mkdir(parents=True, exist_ok=True)
for origin, target in [('probe_time.rs', 'list_perf_time.rs'), ('probe_alloc.rs', 'list_perf_alloc.rs'),
                       ('probe_support.rs', 'probe_support/mod.rs')]:
    shutil.copyfile(here / origin, examples / target)
for name in ['measure.py', 'analyze.py', 'fault_probe.py', 'inspect.py', 'argv_control.py']:
    shutil.copyfile(here / name, out / name)
(out / 'bin').mkdir()
env = dict(os.environ, ARRAY_PERF_DIR=str(out), CARGO_PROFILE_RELEASE_INCREMENTAL='true')
guard = [sys.executable, '-B', 'tests/build_guard.py', '--workflow', 'array-release',
         '--target-dir', str(out / 'cargo-target'), '--tmp-dir', str(out / 'tmp'), '--']
build = ['cargo', '+1.94.0', 'build', '--locked', '--release', '--manifest-path', str(src / 'Cargo.toml'),
         '-p', 'openrust-framework', '--example', 'list_perf_time', '--example', 'list_perf_alloc']
core_files = sorted(set((git('diff', '--name-only', INITIAL + '^', INITIAL, '--', 'vendor/openrust-framework/src') +
                             git('diff', '--name-only', FOLLOWUP + '^', FOLLOWUP, '--', 'vendor/openrust-framework/src')).decode().splitlines()))
input_hashes = {}
for version in ['before', 'after']:
    if version == 'after':
        subprocess.run(['git', 'apply', '--directory=' + prefix, str(patch_paths[0])], check=True)
        app_file.write_text(app_file.read_text().replace(f'let base_providers: [Box<dyn ServiceProvider>; {providers}]',
                                                       'let base_providers: [Box<dyn ServiceProvider>; _]'))
        subprocess.run(['git', 'apply', '--directory=' + prefix, str(patch_paths[1])], check=True)
    input_hashes[version] = {name: hashlib.sha256((src / name).read_bytes()).hexdigest() for name in core_files}
    if args.prepare_only:
        continue
    with (out / f'build-{version}.log').open('w') as log:
        subprocess.run(guard + build, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    for kind in ['time', 'alloc']:
        shutil.copy2(out / 'cargo-target/release/examples' / f'list_perf_{kind}', out / 'bin' / f'{version}_{kind}')
    print(version, 'Release二进制已构建', flush=True)
(out / 'input-sha256.json').write_text(json.dumps(input_hashes, indent=2))
if args.prepare_only:
    print('两组源文件SHA：', out / 'input-sha256.json')
    raise SystemExit(0)
(out / 'provenance.json').write_text(json.dumps({
    'revision': args.revision, 'providers_in_both': providers, 'stubs_in_both': stubs,
    'binary_sha256': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (out / 'bin').iterdir()},
}, indent=2))
with (out / 'measurement.log').open('w') as log:
    subprocess.run(guard + [sys.executable, '-B', str(out / 'measure.py')], env=env,
                   stdout=log, stderr=subprocess.STDOUT, check=True)
with (out / 'faults.log').open('w') as log:
    subprocess.run(guard + [sys.executable, '-B', str(out / 'fault_probe.py')], env=env,
                   stdout=log, stderr=subprocess.STDOUT, check=True)
if sys.platform == 'darwin':
    subprocess.run(guard + [sys.executable, '-B', str(out / 'inspect.py')], env=env, check=True)
print('结果：', out / 'summary.json')
