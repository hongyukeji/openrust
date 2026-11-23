"""Fetch the pinned Precognition PHP reference. Usage: prepare_precognition.py OUTPUT."""
import hashlib
import json
from pathlib import Path
import sys
import urllib.request

project = Path(__file__).resolve().parents[3]
reference = project / ".agents/project/references/framework/tests"
manifest = json.loads((reference / "fixtures/precognition/source-manifest.json").read_text())
output = Path(sys.argv[1]).resolve()
if not output.is_relative_to(project / "target/tmp"):
    raise ValueError("输出必须位于项目target/tmp内")
output.mkdir(parents=True, exist_ok=True)
for entry in manifest["sources"]:
    repo = entry.get("repository", manifest["framework"])
    ref = entry.get("ref", manifest["framework_ref"])
    path = entry["path"]
    if "repository" in entry:
        path = path.removeprefix("framework/").removeprefix("docs/")
    url = f"https://raw.githubusercontent.com/{repo}/{ref}/{path}"
    with urllib.request.urlopen(url, timeout=30) as response:
        data = response.read()
    if hashlib.sha256(data).hexdigest() != entry["sha256"]:
        raise RuntimeError(f"Pinned reference SHA mismatch: {entry['path']}")
    target = output / entry["path"]
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)
print(output / "src/Illuminate/Http/Concerns/CanBePrecognitive.php")
