"""Restore the accepted unit from its immutable local commit, without reading workspace WIP."""
from pathlib import Path
import hashlib
import io
import json
import subprocess
import sys
import tarfile

audit = Path(__file__).resolve().parent
workspace = audit.parents[3]
destination = Path(sys.argv[1]).resolve()
if destination.exists():
    raise SystemExit('Choose a new candidate directory.')
revision = (audit / 'git-commit.txt').read_text().strip()
files = json.loads((audit / 'candidate-sha256.json').read_text())
archive = subprocess.check_output(['git', 'archive', revision], cwd=workspace)
destination.mkdir(parents=True)
with tarfile.open(fileobj=io.BytesIO(archive)) as contents:
    contents.extractall(destination, filter='data')
for name, expected in files.items():
    actual = hashlib.sha256((destination / name).read_bytes()).hexdigest()
    if actual != expected:
        raise SystemExit(f'Candidate differs from reviewed content: {name}')
print(f'Candidate verified: {len(files)} selected paths from {revision[:8]}')
