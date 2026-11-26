"""Prepare pinned, SHA-checked PHP sources outside the repository. Usage: prepare.py OUTPUT_DIR"""
import hashlib
import io
import json
import pathlib
import shutil
import sys
import urllib.request
import zipfile

reference = pathlib.Path(__file__).resolve().parent
output = pathlib.Path(sys.argv[1]).resolve()
output.mkdir(parents=True, exist_ok=True)

def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "OpenRust-Passport-reference"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()

def checked(data, expected):
    if hashlib.sha256(data).hexdigest() != expected:
        raise RuntimeError("Pinned reference SHA mismatch")
    return data

for filename in ["redirect-input.json", "league-sha256.json", "php-vendor-sha256.json", "oracle.php"]:
    shutil.copyfile(reference / filename, output / filename)
upstream = output / "upstream"
upstream.mkdir(exist_ok=True)
for filename, digest in json.loads((reference / "league-sha256.json").read_text()).items():
    data = fetch("https://raw.githubusercontent.com/thephpleague/oauth2-server/9d2f6fc0a0b5aa1bb02506971d3a4ecff2c6526c/" + filename)
    (upstream / ("league__" + filename.replace("/", "__"))).write_bytes(checked(data, digest))
for package, pin in json.loads((reference / "php-vendor-sha256.json").read_text()).items():
    registry = json.loads(fetch("https://repo.packagist.org/p2/" + package + ".json"))
    selected = next(item for item in registry["packages"][package] if item["version"].removeprefix("v") == pin["version"])
    assert selected["dist"]["reference"] == pin["reference"]
    data = checked(fetch(selected["dist"]["url"]), pin["sha256"])
    destination = output / "php-vendor" / package.replace("/", "__")
    destination.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        for member in archive.infolist():
            relative = pathlib.PurePosixPath(member.filename)
            assert ".." not in relative.parts and not relative.is_absolute()
            relative = pathlib.Path(*relative.parts[1:])
            if not relative.parts:
                continue
            target = destination / relative
            if member.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(archive.read(member))
print("Prepared SHA-checked fixed PHP references")
