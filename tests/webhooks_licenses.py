"""验证 Webhook 的依赖固定值、许可原文及可选的本地运行镜像。"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tomllib

ROOT=Path(__file__).resolve().parents[1]
PACKAGES = ("webhook-client", "webhook-server")


def verify_image(image, packages, rust_copyright):
    """比对容器实际用户可以读取的声明和原文，包含未选包的排除。"""
    expected = {
        name: (ROOT / name).read_bytes()
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md")
    }
    if rust_copyright:
        expected["COPYRIGHT-library.html"] = rust_copyright.read_bytes()
    for package in packages:
        package_root = ROOT / "vendor" / f"openrust-{package}"
        files = [package_root / "THIRD_PARTY_NOTICES.md", *(package_root / "LICENSES").glob("*.txt")]
        for path in files:
            expected[f"licenses/{package}/{path.relative_to(package_root)}"] = path.read_bytes()

    raw = subprocess.check_output([
        "docker", "run", "--rm", "--pull", "never", "--network", "none", "--read-only",
        "--entrypoint", "/bin/sh", image, "-ec",
        'cd /app; if [ -d licenses ]; then set -- "$@" licenses; fi; tar -cf - "$@"',
        "sh", *(name for name in expected if "/" not in name),
    ])
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:") as archive:
        found = {member.name: archive.extractfile(member).read() for member in archive if member.isfile()}
    missing = expected.keys() - found.keys()
    unexpected = found.keys() - expected.keys()
    assert not missing and not unexpected, (sorted(missing), sorted(unexpected))
    for name, data in expected.items():
        assert found[name] == data, name
    print(f"{image} 镜像中的 {len(expected)} 个许可文件与源码逐字节一致")


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--image", help="比对指定的本地 Docker 运行镜像")
parser.add_argument("--package", action="append", choices=PACKAGES, default=[], help="镜像实际选用的 Webhook 包")
parser.add_argument("--rust-copyright", type=Path, help="构建工具链的 Rust 标准库版权报告原件")
args = parser.parse_args()
if (args.package or args.rust_copyright) and not args.image:
    parser.error("--package 和 --rust-copyright 需要 --image")

lock=tomllib.loads((ROOT/"Cargo.lock").read_text())["package"]
pins=json.loads((ROOT/".agents/project/audits/webhooks/dependency-pins.json").read_text())
for pin in pins:
    actual=next(p for p in lock if p["name"]==pin["crate"] and p["version"]==pin["version"])
    assert actual["checksum"]==pin["checksum"],pin["crate"]
    for kind in ("client","server"):
        root=ROOT/f"vendor/openrust-webhook-{kind}"
        data=(root/"LICENSES"/pin["license_file"]).read_bytes()
        assert hashlib.sha256(data).hexdigest()==pin["license_sha256"],(kind,pin["crate"])
        assert b"Copyright" in data
        assert (root/"LICENSE").read_bytes()==(ROOT/"LICENSE").read_bytes()
        assert (root/"LICENSES/Spatie-MIT.txt").exists()
        assert "1.3.0" in (root/"THIRD_PARTY_NOTICES.md").read_text()
print("两项新增依赖的版本／校验和及两个包的 MIT／ISC 许可原文通过")
if args.image:
    verify_image(args.image, args.package, args.rust_copyright)
