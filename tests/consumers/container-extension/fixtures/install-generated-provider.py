#!/usr/bin/env python3
"""把公开 Provider 验收 fixture 接入一个刚生成的临时应用。"""

from pathlib import Path
import re
import sys


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: install-generated-provider.py <generated-app-root>", file=sys.stderr)
        return 2
    root = Path(sys.argv[1]).resolve()
    fixture = Path(__file__).resolve().parent
    app_file = root / "bootstrap/app.rs"
    registry_file = root / "bootstrap/providers.rs"
    modules_file = root / "app/providers/mod.rs"
    provider_file = root / "app/providers/container_extension.rs"
    test_file = root / "tests/container_extension.rs"
    required = (app_file, registry_file, modules_file, root / "app/mod.rs",
                root / "config/mod.rs", root / "Cargo.toml")
    if not all(path.is_file() for path in required):
        print("target is not a generated OpenRust application", file=sys.stderr)
        return 2

    original = app_file.read_text()
    registry = registry_file.read_text()
    modules = modules_file.read_text()
    provider_anchor = "    providers.push(Box::new(crate::app::providers::AppServiceProvider));"
    module_anchor = "pub mod app;"
    module_line = "pub mod container_extension;"
    registration_line = (
        "    providers.push(Box::new(crate::app::providers::container_extension::GeneratedExtensionProvider));"
    )
    if registry.count(provider_anchor) != 1 or modules.count(module_anchor) != 1:
        print("generated bootstrap template differs from fixture anchors", file=sys.stderr)
        return 2
    application = re.search(r"impl openrust::foundation::ApplicationDefinition for ([A-Za-z_][A-Za-z0-9_]*)", original)
    if application is None:
        print("generated application type was not found", file=sys.stderr)
        return 2
    module_count = modules.splitlines().count(module_line)
    registration_count = registry.splitlines().count(registration_line)
    if module_count > 1 or registration_count > 1 or module_count != registration_count:
        print("generated bootstrap has partial fixture registration", file=sys.stderr)
        return 2
    installed = module_count == 1
    updated_modules = modules if installed else modules.replace(
        module_anchor, module_anchor + "\n" + module_line, 1
    )
    updated_registry = registry if installed else registry.replace(
        provider_anchor, provider_anchor + "\n" + registration_line, 1
    )
    copies = (
        (provider_file, (fixture / "generated-provider.rs").read_bytes()),
        (
            test_file,
            (fixture / "generated-provider-test.rs")
            .read_text()
            .replace("OpenRustGeneratedApplication", application.group(1))
            .encode(),
        ),
    )
    for target, contents in copies:
        if target.exists() and target.read_bytes() != contents:
            print(f"refusing to overwrite changed file: {target}", file=sys.stderr)
            return 2

    for target, contents in copies:
        target.write_bytes(contents)
    modules_file.write_text(updated_modules)
    registry_file.write_text(updated_registry)
    print(f"installed generated Provider fixture in {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
