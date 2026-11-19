#!/usr/bin/env python3
"""Run the complete pinned Illuminate validator after verifying its locked dependency trees."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

PHP = 'php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9'
parser = argparse.ArgumentParser()
parser.add_argument('runtime', type=Path)
parser.add_argument('output', type=Path)
args = parser.parse_args()
fixtures = Path(__file__).resolve().parent
manifest = json.loads((fixtures / 'password-rule-manifest.json').read_text())
if hashlib.sha256((args.runtime / 'composer.lock').read_bytes()).hexdigest() != manifest['composer_lock_sha256']:
    raise RuntimeError('Composer lock mismatch')
if hashlib.sha256((args.runtime / 'validation.php').read_bytes()).hexdigest() != manifest['language_sha256']:
    raise RuntimeError('Pinned language source mismatch')
for name, expected in manifest['bootstrap_files'].items():
    if hashlib.sha256((args.runtime / name).read_bytes()).hexdigest() != expected:
        raise RuntimeError('Bootstrap mismatch: ' + name)
for name, package in manifest['packages'].items():
    root = args.runtime / 'vendor' / name
    digest = hashlib.sha256()
    for file in sorted(path for path in root.rglob('*') if path.is_file()):
        digest.update(str(file.relative_to(root)).encode() + b'\0')
        digest.update(hashlib.sha256(file.read_bytes()).digest())
    if digest.hexdigest() != package['tree_sha256']:
        raise RuntimeError('Package tree mismatch: ' + name)
result = subprocess.run(['docker', 'run', '--rm', '--network', 'none',
                         '-v', str(fixtures) + ':/fixtures:ro',
                         '-v', str(args.runtime.resolve()) + ':/runtime:ro', PHP,
                         'php', '/fixtures/password_rule.php', '/runtime'],
                        check=True, stdout=subprocess.PIPE)
if len(json.loads(result.stdout)['cases']) != 771:
    raise RuntimeError('Unexpected oracle case count')
args.output.write_bytes(result.stdout)
print(hashlib.sha256(result.stdout).hexdigest())
