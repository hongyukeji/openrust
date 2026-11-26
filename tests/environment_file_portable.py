"""准备环境文件编辑器的原样源码夹具，供 macOS、Linux 和原生 Windows 定向验收。"""
import argparse
from pathlib import Path
import shutil
import subprocess
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare-only', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    stage = root / 'target/tmp/environment-file/portable'
    source = root / 'vendor/openrust-framework'
    packages = tomllib.loads((root / 'Cargo.lock').read_text())['package']
    regex_version = next(item['version'] for item in packages if item['name'] == 'regex')
    (stage / 'src/environment_file').mkdir(parents=True, exist_ok=True)
    (stage / 'tests').mkdir(exist_ok=True)
    # 直接编译交付源码；不替换文件锁、解析器或操作系统实现。
    for name in ('environment_file.rs', 'environment_file/document.rs', 'environment_file/storage.rs', 'environment_file/options.rs'):
        shutil.copyfile(source / 'src/config' / name, stage / 'src' / name)
    shutil.copyfile(source / 'tests/environment_file.rs', stage / 'tests/environment_file.rs')
    (stage / 'src/lib.rs').write_text('mod environment_file;\npub mod config { pub use crate::environment_file::{EnvironmentFile, EnvironmentEditor, EnvironmentFileError, EnvironmentDocument, EnvironmentPosition, EnvironmentWriteOptions}; }\n')
    (stage / 'Cargo.toml').write_text('''[package]
name = "environment-file-portable"
version = "0.0.0"
edition = "2024"
rust-version = "1.95"
[workspace]
[lib]
name = "openrust"
[dependencies]
adotenv = { version = "=1.5.4", default-features = false }
dotenvy = "=0.15.7"
regex = "=REGEX_VERSION"
zeroize = "=1.9.0"
tempfile = "=3.27.0"
[target.'cfg(unix)'.dependencies]
rustix = { version = "=1.1.6", features = ["fs"] }
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "=0.61.2", features = ["Win32_Storage_FileSystem", "Win32_Security"] }
'''.replace('REGEX_VERSION', regex_version))
    print(stage, flush=True)
    if not args.prepare_only:
        subprocess.run(['python3', '-B', str(root / 'tests/build_guard.py'),
                        '--workflow', 'environment-file', '--', 'cargo', 'test',
                        '--manifest-path', str(stage / 'Cargo.toml')], cwd=root, check=True)


if __name__ == '__main__':
    main()
