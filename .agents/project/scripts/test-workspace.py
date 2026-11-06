#!/usr/bin/env python3
"""显式区分日常MySQL与发布多数据库的workspace运行测试。"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--database-scope', required=True,
                        choices=('routine-mysql', 'release-backends'))
    parser.add_argument('--print-command', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[3]
    manifest = tomllib.loads((root / 'Cargo.toml').read_text())
    command = ['cargo', 'test', '--workspace', '--locked']
    if args.database_scope == 'release-backends':
        command.append('--all-features')
    else:
        url = os.environ.get('OPENRUST_TEST_DATABASE_URL', '')
        if url and not url.startswith('mysql://'):
            parser.error('routine-mysql不允许非MySQL测试数据库URL')
        features = manifest['features']
        selected = sorted(set(features) - {'default', 'db-postgres', 'db-sqlite'})
        pending, visited = list(selected), set()
        while pending:
            feature = pending.pop()
            if feature in visited:
                continue
            visited.add(feature)
            if feature in {'db-postgres', 'db-sqlite'} or feature.endswith(('/sqlx-postgres', '/sqlx-sqlite')):
                parser.error('日常feature传递启用了非MySQL后端：' + feature)
            pending.extend(features.get(feature, []))
        command.extend(['--features', ','.join(selected)])
    environment = os.environ.copy()
    environment.setdefault('OPENRUST_TEST_DATABASE_STATE_DIR',
                           str(root / 'storage/framework/testing/databases'))
    environment.setdefault('OPENRUST_TEST_ARTIFACT_DIR',
                           str(root / 'tmp/test-artifacts'))
    for key in ('OPENRUST_TEST_DATABASE_STATE_DIR', 'OPENRUST_TEST_ARTIFACT_DIR'):
        environment[key] = str((root / environment[key]).resolve())
    print(json.dumps({'database_scope': args.database_scope, 'command': command,
                      'database_state_dir': environment['OPENRUST_TEST_DATABASE_STATE_DIR'],
                      'release_matrix_complete': False}, ensure_ascii=False), flush=True)
    if args.print_command:
        return 0
    return subprocess.run(command, cwd=root, env=environment, check=False).returncode


if __name__ == '__main__':
    raise SystemExit(main())
