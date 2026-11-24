"""在项目内隔离副本中验证单点升级及业务应用独立版本，可选运行真实消费者。"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--source', required=True, type=Path)
parser.add_argument('--runtime', action='store_true')
args = parser.parse_args()
source = args.source.resolve()
if source == ROOT / 'target/tmp' or not source.is_relative_to(ROOT / 'target/tmp') or not (source / 'Cargo.toml').is_file():
    parser.error('只能验证项目 target/tmp 内的隔离源码')
logs = source.parent / 'versions'
logs.mkdir(exist_ok=True)
manifest = source / 'Cargo.toml'
lock = source / 'Cargo.lock'
original = manifest.read_text()
original_lock = lock.read_bytes()
parsed = tomllib.loads(original)
canonical = parsed['workspace']['package']['version']
application_version = parsed['package']['version']
independent = application_version if isinstance(application_version, str) else None
probe = '999.0.0-workspace-probe' if canonical != '999.0.0-workspace-probe' else '999.0.1-workspace-probe'
results = {'canonical': canonical, 'probe': probe, 'business': '9.8.7', 'checks': []}


def run(name, command):
    result = subprocess.run(command, cwd=source, capture_output=True, text=True)
    (logs / (name + '.log')).write_text(result.stdout + result.stderr)
    results['checks'].append({'name': name, 'command': command, 'exit': result.returncode})
    assert result.returncode == 0, (name, (result.stdout + result.stderr)[-6000:])
    print(name, 'PASS', flush=True)
    return result.stdout


def metadata(name, expected, business=None):
    data = json.loads(run(name, ['cargo', 'metadata', '--offline', '--format-version', '1']))
    members = set(data['workspace_members'])
    packages = [p for p in data['packages'] if p['id'] in members]
    expected_count = len(list((source / 'vendor/openrust').glob('*/Cargo.toml'))) + 1
    assert len(packages) == expected_count
    for package in packages:
        value = business if business and Path(package['manifest_path']) == manifest else expected
        assert package['version'] == value, (package['name'], package['version'], value)
    results[name] = {p['name']: p['version'] for p in packages}


try:
    metadata('default', canonical, independent)
    header = original.index('[workspace.package]')
    tail = original.find('\n[', header + 1)
    tail = len(original) if tail == -1 else tail
    section = original[header:tail]
    section, count = re.subn(r'(?m)^version\s*=\s*[^\n]+$', f'version = "{probe}"', section)
    assert count == 1
    bumped = original[:header] + section + original[tail:]
    manifest.write_text(bumped)
    metadata('single_change', probe, independent)
    start = bumped.index('[package]')
    end = bumped.find('\n[', start + 1)
    end = len(bumped) if end == -1 else end
    package, count = re.subn(r'(?m)^version(?:\.workspace)?\s*=\s*[^\n]+$', 'version = "9.8.7"', bumped[start:end])
    assert count == 1
    manifest.write_text(bumped[:start] + package + bumped[end:])
    metadata('independent_application', probe, '9.8.7')
    if args.runtime:
        run('application', ['cargo', 'test', '--locked', '-p', 'openrust', '--test', 'framework_version', '--test', 'application_skeleton', '--test', 'resource_generator', '--test', 'console', '--test', 'init', '--test', 'startup', '--', '--test-threads=1'])
        run('framework', ['cargo', 'test', '--locked', '-p', 'openrust-framework', '--test', 'application_version', '--test', 'installer_user_model'])
        run('socialite', ['cargo', 'test', '--locked', '-p', 'openrust-socialite', '--test', 'github'])
        run('settings', ['cargo', 'test', '--locked', '-p', 'openrust-settings', '--test', 'provider'])
finally:
    manifest.write_text(original)
    lock.write_bytes(original_lock)
    (logs / 'results.json').write_text(json.dumps(results, ensure_ascii=False, indent=2) + '\n')
