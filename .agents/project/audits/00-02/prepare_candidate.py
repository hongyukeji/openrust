"""Reconstruct the reviewed candidate without modifying the source workspace."""

from pathlib import Path
import hashlib
import io
import json
import shutil
import subprocess
import sys
import tarfile


audit = Path(__file__).resolve().parent
workspace = audit.parents[3]
destination = Path(sys.argv[1]).resolve()
if destination.exists():
    raise SystemExit('Choose a new candidate directory.')
base = (audit / 'base-head.txt').read_text().strip()
files = json.loads((audit / 'candidate-sha256.json').read_text())
archive = subprocess.check_output(['git', 'archive', base], cwd=workspace)
destination.mkdir(parents=True)
with tarfile.open(fileobj=io.BytesIO(archive)) as contents:
    contents.extractall(destination, filter='data')
for name in (audit / 'deleted-files.txt').read_text().splitlines():
    target = destination / name
    if target.exists():
        target.unlink()
    if target.parent.exists() and not any(target.parent.iterdir()):
        target.parent.rmdir()

for name in files:
    target = destination / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(workspace / name, target)
subprocess.run(
    ['git', 'apply', str(audit / 'scope-exclusions.patch')],
    cwd=destination,
    check=True,
)
for name, expected in files.items():
    actual = hashlib.sha256((destination / name).read_bytes()).hexdigest()
    if actual != expected:
        raise SystemExit(f'Candidate differs from reviewed content: {name}')
print(f'Candidate verified: {len(files)} selected paths over {base[:8]}')
