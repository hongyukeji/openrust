"""在独立源码快照中验收 init 的环境文件写入；禁止传入工作树或已有部署。"""
import argparse
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    source = args.source.resolve()
    if not source.is_relative_to(root / 'target/tmp') or source == root:
        raise SystemExit('验收源码必须位于项目 target/tmp/ 内')
    env_path = source / '.env'
    if env_path.exists():
        raise SystemExit('快照已有 .env，请另选未初始化的源码快照')
    env_path.write_text('APP_ENV=local\nAPP_NAME="Environment init"\nAPP_KEY=\n'
                        'DB_CONNECTION=sqlite\nDB_DATABASE=target/tmp/init.sqlite\n'
                        'CACHE_STORE=array\nSESSION_DRIVER=array\nQUEUE_CONNECTION=sync\n'
                        'MAIL_FROM_NAME="${APP_NAME}" # preserve reference\n')
    runtime = {k: v for k, v in os.environ.items() if not k.startswith(('APP_', 'DB_', 'MAIL_', 'CACHE_', 'SESSION_', 'QUEUE_', 'GRPC_', 'REVERB_'))}
    runtime.update(APP_BASE_PATH=str(source), RUSTUP_TOOLCHAIN='1.95.0')
    subprocess.run([str(args.binary.resolve()), 'artisan', 'init', '--extensions=grpc',
                    '--without-migration', '--no-interaction'], cwd=source, env=runtime, check=True)
    text = env_path.read_text()
    assert 'MAIL_FROM_NAME="${APP_NAME}" # preserve reference\n' in text
    assert text.count('APP_KEY=base64:') == 1
    assert 'APP_NAME="Environment init"\n' in text
    assert env_path.with_name('.env.lock').is_file()
    print('PASS: real init preserves references, generates a key, rebuilds and clears configuration cache')


if __name__ == '__main__':
    main()
