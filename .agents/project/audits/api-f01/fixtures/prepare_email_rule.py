#!/usr/bin/env python3
"""Check immutable source hashes before running the original Laravel/Egulias email rule."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
PHP = 'php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9'
p = argparse.ArgumentParser()
p.add_argument('sources', type=Path)
p.add_argument('output', type=Path)
a = p.parse_args()
fixtures = Path(__file__).resolve().parent
manifest = json.loads((fixtures / 'email-rule-manifest.json').read_text())
for name, expected in manifest['files'].items():
    if hashlib.sha256((a.sources / name).read_bytes()).hexdigest() != expected:
        raise RuntimeError('Source hash mismatch: ' + name)
result = subprocess.run(['docker', 'run', '--rm', '--network', 'none', '-v', str(fixtures)+':/fixtures:ro', '-v', str(a.sources.resolve())+':/sources:ro', PHP, 'php', '/fixtures/email_rule.php', '/sources'], check=True, stdout=subprocess.PIPE)
json.loads(result.stdout)
a.output.write_bytes(result.stdout)
print(hashlib.sha256(result.stdout).hexdigest())
