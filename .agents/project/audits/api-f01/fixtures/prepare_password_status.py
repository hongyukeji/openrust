#!/usr/bin/env python3
"""Verify pinned Fortify controller bytes, then reproduce mixed timeout results."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

PHP = 'php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9'
CONTROLLER = 'src/Http/Controllers/ConfirmedPasswordStatusController.php'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('upstream', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    fixtures = Path(__file__).resolve().parent
    upstream = args.upstream.resolve()
    manifest = json.loads((fixtures / 'upstream-manifest.json').read_text())
    if manifest['ref'] != '3db382a3d5d6a0d537de32a190ce3b945e621713':
        raise RuntimeError('Source pin mismatch')
    if hashlib.sha256((upstream / CONTROLLER).read_bytes()).hexdigest() != manifest['files'][CONTROLLER]:
        raise RuntimeError('Controller hash mismatch')
    result = subprocess.run(['docker', 'run', '--rm', '--network', 'none',
                             '-v', str(fixtures) + ':/fixtures:ro',
                             '-v', str(upstream) + ':/upstream:ro', PHP,
                             'php', '/fixtures/password_status.php', '/upstream/' + CONTROLLER],
                            check=True, stdout=subprocess.PIPE)
    cases = json.loads(result.stdout)
    if len(cases) != 279:
        raise RuntimeError('Unexpected oracle case count')
    args.output.write_bytes(result.stdout)
    print(hashlib.sha256(result.stdout).hexdigest())


if __name__ == '__main__':
    main()
