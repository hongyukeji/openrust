"""Verify command output and exit statuses from source and relocated releases."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory

source = Path(sys.argv[1]).resolve()
audit = Path(__file__).resolve().parent
keys = set(re.findall(r'"([A-Z][A-Z0-9_]+)"', '\n'.join(p.read_text() for p in (source / 'config').glob('*.rs'))))
keys.update(('APP_BASE_PATH', 'APP_ENV', 'GRPC_HOST', 'GRPC_PORT', 'FORCE_COLOR', 'NO_COLOR'))
env = {key: value for key, value in os.environ.items() if key not in keys}
env.update(APP_PORT='invalid', APP_URL='http://localhost', CARGO_NET_OFFLINE='true', RUSTUP_TOOLCHAIN='1.94.0')
results = []


def run(arguments, cwd, code=0, settings=None):
    output = subprocess.run(arguments, cwd=cwd, env=settings or env, text=True, capture_output=True, timeout=30)
    assert output.returncode == code, (arguments, output.returncode, output.stderr)
    results.append(dict(arguments=[str(argument) for argument in arguments], expected_exit=code))
    return output


with TemporaryDirectory(prefix='openrust-00-03-') as directory:
    root = Path(directory)
    foreign = root / 'foreign cwd'
    foreign.mkdir()
    deployment = root / 'relocated release'
    deployment.mkdir()
    shutil.copy2(source / 'target/release/main', deployment / 'main')
    (deployment / '.env').write_text('APP_PORT=invalid\nAPP_URL=http://localhost\n')
    for binary in [source / 'target/debug/main', deployment / 'main']:
        invoke = lambda args, code=0: run([binary, 'artisan', *args], foreign, code)
        default = invoke([])
        explicit = invoke(['list'])
        assert default.stdout == explicit.stdout
        assert 'Available commands:' in explicit.stdout
        for arguments in [['help', 'list'], ['list', '--help']]:
            assert 'Usage: artisan list' in invoke(arguments).stdout
        assert 'Display an inspiring quote' in invoke(['help', 'inspire']).stdout
        assert 'environment is [production].' in invoke(['env']).stdout
        assert 'environment is [staging].' in invoke(['env', '--env=staging']).stdout
        assert 'environment is [local].' in invoke(['--env', 'local', 'env']).stdout
        assert 'requires an environment name' in invoke(['env', '--env'], 1).stderr
        quote = invoke(['inspire'])
        assert not quote.stderr and '— ' in quote.stdout and '\x1b' not in quote.stdout
        assert '\x1b[1m' in invoke(['inspire', '--ansi']).stdout
        assert '\x1b' not in invoke(['--no-ansi', 'inspire']).stdout
        assert '[Planned]' in invoke(['list', '--raw']).stdout
        listing = json.loads(invoke(['list', 'cache', '--format=json', '--short']).stdout)
        assert listing['commands'] and all(command['name'].startswith('cache:') for command in listing['commands'])
        assert '<commands>' in invoke(['list', 'cache', '--format=xml']).stdout
        assert '## cache' in invoke(['list', 'cache', '--format=md']).stdout
        for arguments in [['unknown-command'], ['help', 'unknown-command'], ['list', '--unknown-option'], ['inspire', 'unexpected-argument'], ['migrate:fresh']]:
            failure = invoke(arguments, 1)
            assert failure.stderr and not failure.stdout
        assert 'not implemented yet' in invoke(['help', 'build']).stdout
        usage = run([binary, 'unexpected-mode'], foreign, 2, dict(env, APP_BASE_PATH='relative-invalid'))
        assert 'Usage:' in usage.stderr
    # The source wrapper changes directory itself and forwards to the source build.
    assert 'environment is [staging].' in run([source / 'artisan', 'env', '--env=staging'], foreign).stdout
    assert '— ' in run([source / 'artisan', 'inspire'], foreign).stdout
    assert '— ' in run(['cargo', '+1.94.0', 'run', '--locked', '--offline', '--', 'artisan', 'inspire'], source).stdout

(audit / 'command-results.json').write_text(json.dumps(results, indent=2) + '\n')
print(f'PASS: {len(results)} process invocations, including expected failures')
