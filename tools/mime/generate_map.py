"""从固定 Symfony MimeTypes.php 生成有序 Rust MIME 映射。"""

import hashlib
import re
import sys
from pathlib import Path

SOURCE_SHA256 = "c52bc8bfba35f33b7c93537b3b0ddd6ec2776f24c846ae8b33c3e0f507e20d6f"
SOURCE_COMMIT = "7576ce3b2b4d3a2a7fe7020a07a392065d6ffd40"


def entries(source: str, name: str) -> list[tuple[str, list[str]]]:
    section = source.split(f"private const {name} = [", 1)[1]
    if name == "MAP":
        section = section.split("private const REVERSE_MAP", 1)[0]
    found = re.findall(r"'([^']+)'\s*=>\s*\[([^\]]*)\]", section)
    assert len(found) == len(re.findall(r"=>\s*\[", section)), name
    return [(key, re.findall(r"'([^']+)'", values)) for key, values in found]


def generate(data: bytes) -> str:
    assert hashlib.sha256(data).hexdigest() == SOURCE_SHA256
    source = data.decode()
    output = [
        "//! Symfony Mime v8.0.0 有序映射，源提交 " + SOURCE_COMMIT + ".",
        "//! 源文件 SHA-256 " + SOURCE_SHA256 + ".",
        "//! Copyright (c) 2010-present Fabien Potencier；MIT，见 `mime_map.LICENSE`。",
        "",
    ]
    for name in ("MAP", "REVERSE_MAP"):
        output.append(f"pub(super) static {name}: &[(&str, &[&str])] = &[")
        for key, values in entries(source, name):
            strings = ", ".join(f'"{value}"' for value in values)
            output.append(f'    ("{key}", &[{strings}]),')
        output.append("];\n")
    return "\n".join(output)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: generate_map.py FIXED_MIMETYPES.php OUTPUT.rs")
    Path(sys.argv[2]).write_text(generate(Path(sys.argv[1]).read_bytes()))
