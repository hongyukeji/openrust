#!/usr/bin/env python3
"""显式生成模块 Protobuf；--check 验证重生字节一致，不给框架增加构建依赖。"""
from pathlib import Path
import argparse
import json
import os
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--check", action="store_true", help="仅比较生成结果，不写回源码")
arguments = parser.parse_args()
module = Path(__file__).resolve().parent.parent
header = "// 由 app/example/proto/generate.py 使用锁定的 tonic/prost 工具生成。\n"
with tempfile.TemporaryDirectory(prefix="openrust-example-proto-", dir="/tmp") as directory:
    tool = Path(directory)
    (tool / "src").mkdir()
    output = tool / "generated"
    output.mkdir()
    shutil.copy(module / "proto/generator.toml", tool / "Cargo.toml")
    shutil.copy(module / "proto/generator.lock", tool / "Cargo.lock")
    (tool / "src/main.rs").write_text(
        'fn main() -> Result<(), Box<dyn std::error::Error>> {\n'
        'let mut config = tonic_prost_build::Config::new();\n'
        'config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);\n'
        'tonic_prost_build::configure()\n'
        f'.out_dir({json.dumps(str(output))})\n'
        '.compile_with_config(config,\n'
        f'&[{json.dumps(str(module / "proto/echo.proto"))}],\n'
        f'&[{json.dumps(str(module / "proto"))}])?;\nOk(())\n}}\n'
    )
    subprocess.run(
        ["cargo", "run", "--offline", "--locked", "--manifest-path", str(tool / "Cargo.toml")],
        check=True,
        env=os.environ.copy(),
    )
    generated = header.encode() + (output / "example.v1.rs").read_bytes()
    destination = module / "generated/echo.rs"
    if arguments.check:
        if destination.read_bytes() != generated:
            raise SystemExit("生成结果不同，请运行 generate.py 并检查 diff")
        print("Protobuf 重生字节一致")
    else:
        destination.parent.mkdir(exist_ok=True)
        destination.write_bytes(generated)
