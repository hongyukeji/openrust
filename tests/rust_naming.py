"""核对 workspace 命名策略，并用真实编译器验证错误命名会被拒绝。"""
from pathlib import Path
import re
import json
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
LINTS = ("non_snake_case", "non_camel_case_types", "non_upper_case_globals")


def main():
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    policy = manifest["workspace"]["lints"]["rust"]
    for lint in LINTS:
        assert policy.get(lint) == "deny", f"workspace 必须拒绝 {lint}"
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--locked", "--format-version", "1"], cwd=ROOT))
    manifests = [Path(package["manifest_path"]) for package in metadata["packages"] if package["id"] in metadata["workspace_members"]]
    for path in manifests:
        data = tomllib.loads(path.read_text())
        assert data.get("lints") == {"workspace": True}, f"未继承统一命名检查：{path}"
    paths = list(ROOT.glob("*.rs"))
    for folder in ("app", "bootstrap", "config", "routes", "database", "tests"):
        paths.extend((ROOT / folder).rglob("*.rs"))
    for manifest_path in manifests:
        if manifest_path.parent != ROOT:
            paths.extend(manifest_path.parent.rglob("*.rs"))
    attribute = re.compile(r"#\s*!?\[\s*(?:allow|expect)\s*\(([^\]]*)\)\s*\]", re.S)
    for path in paths:
        for match in attribute.finditer(path.read_text()):
            assert not any(re.search(rf"\b{lint}\b", match[1]) for lint in LINTS), f"不必要的命名豁免：{path}"
    temporary = ROOT / "target/tmp/rust-naming-policy"
    temporary.mkdir(parents=True, exist_ok=True)
    # 读取真实策略生成最小 Cargo 消费者，检查三种 lint 的继承与失败诊断。
    with tempfile.TemporaryDirectory(dir=temporary) as directory:
        fixture = Path(directory)
        (fixture / "member/src").mkdir(parents=True)
        lines = "\n".join(f'{lint} = "{policy[lint]}"' for lint in LINTS)
        (fixture / "Cargo.toml").write_text('[workspace]\nmembers = ["member"]\nresolver = "3"\n[workspace.lints.rust]\n' + lines + "\n")
        (fixture / "member/Cargo.toml").write_text('[package]\nname = "naming-policy-probe"\nversion = "0.0.0"\nedition = "2024"\n[lints]\nworkspace = true\n')
        source = fixture / "member/src/lib.rs"
        command = ["cargo", "check", "--offline", "--manifest-path", str(fixture / "Cargo.toml")]
        for lint, body in zip(LINTS, ("pub fn badName() {}", "pub struct Bad_name;", "pub const bad_name: u8 = 0;")):
            source.write_text(body + "\n")
            result = subprocess.run(command, capture_output=True, text=True)
            assert result.returncode != 0 and (lint in result.stderr or lint.replace("_", "-") in result.stderr), f"{lint} 未生效：{result.stderr}"
        source.write_text("pub struct NativeName;\npub fn native_name() {}\npub const NATIVE_NAME: u8 = 0;\nunsafe extern \"C\" { pub fn AttachConsole(process_id: u32) -> i32; }\n")
        subprocess.run(command, check=True)
    print(f"{len(manifests)} 个包继承命名检查；三种错误命名拒绝、原生命名消费者通过")


if __name__ == "__main__":
    main()
