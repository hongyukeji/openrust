"""Reproduce fixed PHP references and reject tampered inputs, using PHP 8.5.12.

Run from the repository root:
python3 -B tests/extension_reference_acceptance.py --upstream target/tmp/.../upstream --framework target/tmp/.../framework
All writable fixtures stay under project target/tmp/. No external account is used.
"""
import argparse
import json
import math
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--upstream', type=Path, required=True)
    parser.add_argument('--framework', type=Path, required=True)
    args = parser.parse_args()
    project = Path(__file__).resolve().parents[1]
    temporary = project / 'target' / 'tmp' / 'extension-reference-acceptance'
    temporary.mkdir(parents=True, exist_ok=True)
    cases = [
        ('activitylog', 'changes_reference', 'spatie-laravel-activitylog', 'changes.json'),
        ('activitylog', 'support_reference', 'spatie-laravel-activitylog', 'support.json'),
        ('horizon', 'autoscaler_reference', 'laravel-horizon', 'autoscaler.json'),
        ('horizon', 'worker_reference', None, None),
        ('pennant', 'reference', 'laravel-pennant', 'aggregation.json'),
        ('pulse', 'reference', 'laravel-pulse', 'preaggregation.json'),
        ('scout', 'engine_reference', 'laravel-scout', 'collection-engine.json'),
    ]
    def php(script, *sources, valid=True):
        command = ['docker', 'run', '--rm', '-v', f'{project}:{project}:ro', '-w', str(project),
                   'php:8.5.12-cli-alpine', 'php', str(script), *map(str, sources)]
        output = subprocess.run(command, capture_output=True, text=True, timeout=40)
        if valid:
            assert output.returncode == 0, output.stderr + output.stdout
            return json.loads(output.stdout)
        assert 'Unexpected pinned reference source:' in output.stdout + output.stderr, output.stdout + output.stderr
        assert '"cases"' not in output.stdout, 'tampered source produced reference output'
    def equivalent(left, right):
        if isinstance(left, float) and isinstance(right, float):
            return math.isclose(left, right, rel_tol=1e-13, abs_tol=1e-13)
        if type(left) is not type(right):
            return False
        if isinstance(left, dict):
            return left.keys() == right.keys() and all(equivalent(left[key], right[key]) for key in left)
        if isinstance(left, list):
            return len(left) == len(right) and all(equivalent(a, b) for a, b in zip(left, right))
        return left == right
    for package, name, upstream, fixture in cases:
        tests = project / 'vendor/openrust' / package / 'tests'
        script = tests / f'{name}.php'
        root = args.upstream.resolve() / upstream if upstream else args.framework.resolve() / 'src/Illuminate/Queue/Console/WorkCommand.php'
        sources = [root, args.framework.resolve()] if package == 'scout' else [root]
        actual = php(script, *sources)
        if fixture:
            expected = json.loads((tests / 'fixtures' / fixture).read_text())
            assert equivalent(actual, expected), (package, name, 'fixture differs')
        else:
            assert actual['commit'] == '1449bf241017087fbd38c118ecaabef3f6797fdc'
            assert len(actual['cases']) == 50
        manifest = json.loads((tests / f'{name}.sources.json').read_text())
        entries = manifest.get('sources', [manifest])
        # Mutate each required file independently: guard must run before require.
        for index, entry in enumerate(entries):
            for relative in entry['files']:
                with tempfile.TemporaryDirectory(dir=temporary) as directory:
                    copy = Path(directory) / 'source'
                    original = sources[index].parent if name == 'worker_reference' else sources[index]
                    copy.mkdir()
                    for file in entry['files']:
                        target = copy / file
                        target.parent.mkdir(parents=True, exist_ok=True)
                        shutil.copy2(original / file, target)
                    with (copy / relative).open('a') as changed:
                        changed.write('\n// changed upstream revision\n')
                    invalid = list(sources)
                    invalid[index] = copy / 'WorkCommand.php' if name == 'worker_reference' else copy
                    php(script, *invalid, valid=False)
        print(f'PASS {package}/{name}: fixture reproduced and every required input rejects tampering', flush=True)


if __name__ == '__main__':
    main()
