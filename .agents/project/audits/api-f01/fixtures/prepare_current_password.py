#!/usr/bin/env python3
"""Verify the complete validator runtime and original bcrypt hasher before current-password comparison."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import sys

PHP = 'php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9'
p = argparse.ArgumentParser()
p.add_argument('runtime', type=Path)
p.add_argument('upstream', type=Path)
p.add_argument('output', type=Path)
a = p.parse_args()
fixtures = Path(__file__).resolve().parent
manifest = json.loads((fixtures / 'current-password-manifest.json').read_text())
if hashlib.sha256((fixtures / 'password-rule-manifest.json').read_bytes()).hexdigest() != manifest['password_runtime_manifest_sha256']:
    raise RuntimeError('Validator runtime manifest mismatch')
for name, expected in manifest['files'].items():
    if hashlib.sha256((a.upstream / name).read_bytes()).hexdigest() != expected:
        raise RuntimeError('Source hash mismatch: ' + name)
with tempfile.TemporaryDirectory(prefix='openrust-current-password-') as directory:
    subprocess.run([sys.executable, str(fixtures / 'prepare_password_rule.py'),
                    str(a.runtime.resolve()), str(Path(directory) / 'password.json')],
                   check=True, stdout=subprocess.PIPE)
result = subprocess.run(['docker', 'run', '--rm', '--network', 'none',
                         '-v', str(fixtures) + ':/fixtures:ro',
                         '-v', str(a.runtime.resolve()) + ':/runtime:ro',
                         '-v', str(a.upstream.resolve()) + ':/upstream:ro', PHP,
                         'php', '/fixtures/current_password.php', '/runtime', '/upstream'],
                        check=True, stdout=subprocess.PIPE)
if len(json.loads(result.stdout)['cases']) != manifest['cases']:
    raise RuntimeError('Unexpected oracle case count')
a.output.write_bytes(result.stdout)
print(hashlib.sha256(result.stdout).hexdigest())
