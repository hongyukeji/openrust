"""从三语正文提取实际 Rust 示例，生成隔离快照中的编译测试。"""
import argparse
from pathlib import Path
import re

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--source", type=Path, required=True)
source = parser.parse_args().source.resolve()
if not source.is_relative_to(root / "target/tmp"):
    parser.error("源码必须位于项目 target/tmp 下")
parts = []
for locale in ("en", "zh-Hans", "zh-Hant"):
    for chapter, anchor, provider in (("filesystem", "custom-filesystems", "StorageServiceProvider"),
                                      ("sms", "custom-sms-providers", "ExampleSmsProvider")):
        text = (source / "docs" / locale / "digging-deeper" / f"{chapter}.md").read_text()
        section = text.split("{#" + anchor + "}", 1)[1]
        blocks = re.findall(r"```rust\n(.*?)\n```", section, re.S)
        assert len(blocks) == 2, (locale, chapter, len(blocks))
        name = (locale + "_" + chapter).replace("-", "_").lower()
        parts.append(f"mod {name} {{\n{blocks[0]}\n")
        parts.append(f"async fn recipe() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {{\n{blocks[1]}\nOk(())\n}}\n")
        parts.append(f"#[test] fn examples_compile() {{ let _ = recipe; let _ = std::mem::size_of::<{provider}>(); }}\n}}\n")
(source / "tests/driver_extensions_docs_generated.rs").write_text("\n".join(parts))
print("已提取三语存储／短信十二段示例")
