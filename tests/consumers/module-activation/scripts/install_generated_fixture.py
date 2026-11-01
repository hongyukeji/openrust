"""只向全新普通生成应用安装公开模块启停测试，不改 Cargo 或生产装配。"""

import hashlib
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path


def owned_path(root: Path, relative: str, *, directory: bool = False) -> Path:
    candidate = root / relative
    current = root
    for part in Path(relative).parts:
        current /= part
        if current.is_symlink():
            raise SystemExit(f"fixture 路径不能经过符号链接：{relative}")
    if not candidate.resolve().is_relative_to(root):
        raise SystemExit(f"fixture 路径越出所属目录：{relative}")
    if directory and not candidate.is_dir():
        raise SystemExit(f"缺少生成应用真实目录：{relative}")
    return candidate


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("用法：install_generated_fixture.py <全新普通生成应用目录> <framework目录>")
    project = Path(sys.argv[1]).resolve(strict=True)
    framework = Path(sys.argv[2]).resolve(strict=True)
    if project == framework or framework.is_relative_to(project):
        raise SystemExit("fixture 只能安装到独立的生成应用，不能安装到框架根")
    for directory in ("app", "bootstrap", "config", "modules", "routes", "src", "tests"):
        owned_path(project, directory, directory=True)
    manifest = owned_path(project, "Cargo.toml")
    bootstrap = owned_path(project, "bootstrap/app.rs")
    modules = owned_path(project, "modules/mod.rs")
    for relative in (
        "src/main.rs", "bootstrap/providers.rs", "config/mod.rs", "config/modules.rs",
        "routes/mod.rs", "routes/api.rs", "routes/web.rs",
    ):
        if not owned_path(project, relative).is_file():
            raise SystemExit(f"缺少当前生成模板文件：{relative}")
    target = owned_path(project, "tests/module_activation.rs")
    if target.exists() or target.is_symlink():
        raise SystemExit("module-activation fixture 已安装；拒绝覆盖")
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    dependency = data.get("workspace", {}).get("dependencies", {}).get("openrust", {})
    if not isinstance(dependency, dict) or not isinstance(dependency.get("path"), str):
        raise SystemExit("生成应用缺少 workspace openrust 路径依赖")
    resolved = (project / dependency["path"]).resolve(strict=True)
    if resolved != framework:
        raise SystemExit("生成应用的 openrust 依赖不属于指定 framework")
    app_dependency = data.get("dependencies", {}).get("openrust", {})
    if not isinstance(app_dependency, dict) or app_dependency.get("workspace") is not True:
        raise SystemExit("生成应用没有使用同一 workspace openrust 依赖")
    features = dependency.get("features", [])
    if not isinstance(features, list) or any(not isinstance(feature, str) for feature in features):
        raise SystemExit("workspace openrust features 配置无效")
    if any(feature in {"auth", "grpc"} or feature.startswith("db-") for feature in features):
        raise SystemExit("本 fixture 要求普通无 --db/--auth/--grpc 的 fresh 应用，不使用真实数据库")
    source_bootstrap = bootstrap.read_text(encoding="utf-8")
    names = re.findall(r"^pub struct ([A-Za-z_][A-Za-z_0-9]*);$", source_bootstrap, re.MULTILINE)
    if (
        len(names) != 1
        or "pub application_root:" not in source_bootstrap
        or "pub modules: openrust::config::ModulesConfig" not in source_bootstrap
        or ".module(" in modules.read_text(encoding="utf-8")
    ):
        raise SystemExit("应用不是当前的空模块生成模板，不能安全安装 fixture")
    fixture = owned_path(
        framework, "tests/consumers/module-activation/fixtures/generated-module-test.rs"
    )
    source = fixture.read_text(encoding="utf-8")
    marker = "__GENERATED_APPLICATION__"
    if source.count(marker) != 1:
        raise SystemExit("公开 fixture 的应用类型替换标记不唯一")
    rendered = source.replace(marker, names[0])
    rendered = subprocess.run(
        ["rustfmt", "--edition", "2024", "--config", "skip_children=true"],
        input=rendered,
        text=True,
        capture_output=True,
        check=True,
        timeout=15,
    ).stdout
    with target.open("x", encoding="utf-8") as output:
        output.write(rendered)
    print(json.dumps({
        "generated_application": names[0],
        "framework": str(framework),
        "fixture": str(target),
        "manifest_unchanged_sha256": hashlib.sha256(manifest.read_bytes()).hexdigest(),
        "installed_sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
    }, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
