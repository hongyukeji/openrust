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
    provider_file = root / "app/container_extension.rs"
    test_file = root / "tests/container_extension.rs"
    if not app_file.is_file() or not (root / "Cargo.toml").is_file():
        print("target is not a generated OpenRust application", file=sys.stderr)
        return 2

    original = app_file.read_text()
    import_anchor = "use crate::routes;"
    provider_anchor = "        providers.push(Box::new(RoutesProvider));"
    module_line = '#[path = "../app/container_extension.rs"]\nmod container_extension;'
    registration_line = (
        "        providers.push(Box::new(container_extension::GeneratedExtensionProvider));"
    )
    if original.count(import_anchor) != 1 or original.count(provider_anchor) != 1:
        print("generated bootstrap template differs from fixture anchors", file=sys.stderr)
        return 2
    application = re.search(r"impl Application for ([A-Za-z_][A-Za-z0-9_]*)", original)
    if application is None:
        print("generated application type was not found", file=sys.stderr)
        return 2
    installed = module_line in original and registration_line in original
    if (module_line in original) != (registration_line in original):
        print("generated bootstrap has partial fixture registration", file=sys.stderr)
        return 2
    updated = original if installed else original.replace(
        import_anchor, import_anchor + "\n" + module_line, 1
    ).replace(provider_anchor, registration_line + "\n" + provider_anchor, 1)
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
    app_file.write_text(updated)
    print(f"installed generated Provider fixture in {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
