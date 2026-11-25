"""验证本次引入的 SDK／HMAC 依赖固定值与随包保留的许可原文。"""
import hashlib
import json
from pathlib import Path
import tomllib
ROOT=Path(__file__).resolve().parents[1]
lock=tomllib.loads((ROOT/"Cargo.lock").read_text())["package"]
pins=json.loads((ROOT/".agents/project/audits/webhooks/dependency-pins.json").read_text())
for pin in pins:
    actual=next(p for p in lock if p["name"]==pin["crate"] and p["version"]==pin["version"])
    assert actual["checksum"]==pin["checksum"],pin["crate"]
    for kind in ("client","server"):
        root=ROOT/f"vendor/openrust/webhook-{kind}"
        data=(root/"LICENSES"/pin["license_file"]).read_bytes()
        assert hashlib.sha256(data).hexdigest()==pin["license_sha256"],(kind,pin["crate"])
        assert b"Copyright" in data
        assert (root/"LICENSE").read_bytes()==(ROOT/"LICENSE").read_bytes()
        assert (root/"LICENSES/Spatie-MIT.txt").exists()
        assert "1.3.0" in (root/"THIRD_PARTY_NOTICES.md").read_text()
print("两项新增依赖的版本／校验和及两个包的 MIT／ISC 许可原文通过")
