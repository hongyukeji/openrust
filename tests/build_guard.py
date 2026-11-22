"""在项目内运行验收命令，并在磁盘停线时停止构建、持锁清理本项目产物。"""

import argparse
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time

STOP_BYTES = 31_250_000 * 1024


class Project:
    def __init__(self, root, free=None, active=None):
        self.root = Path(root).resolve()
        self.free = free or (lambda: shutil.disk_usage(self.root).free)
        self.active = active or self.active_builds

    def local(self, path):
        path = Path(path).resolve()
        if not path.is_relative_to(self.root / 'target' / 'tmp') or path == self.root / 'target' / 'tmp':
            raise ValueError('临时目录和构建目录必须位于项目 target/tmp/ 内')
        return path

    def active_builds(self):
        output = subprocess.check_output(['ps', '-axo', 'pid=,comm=,args='], text=True)
        builds = []
        for line in output.splitlines():
            parts = line.strip().split(None, 2)
            if len(parts) < 3:
                continue
            pid, command, args = parts
            name = Path(command).name
            rust = name in {'cargo', 'rustc', 'rustdoc', 'cargo-clippy', 'clippy-driver'}
            docs = name in {'node', 'npm'} and bool(re.search(r'vitepress.*build|npm.*run.*build', args))
            if not (rust or docs):
                continue
            proc = Path('/proc') / pid / 'cwd'
            try:
                if proc.exists():
                    cwd = proc.resolve()
                else:
                    result = subprocess.run(['lsof', '-a', '-p', pid, '-d', 'cwd', '-Fn'], capture_output=True, text=True)
                    locations = [line[1:] for line in result.stdout.splitlines() if line.startswith('n')]
                    if not locations:
                        if subprocess.run(['ps', '-p', pid], stdout=subprocess.DEVNULL).returncode:
                            continue
                        raise RuntimeError(f'无法确认构建进程 {pid} 的归属')
                    cwd = Path(locations[0]).resolve()
                if cwd.is_relative_to(self.root):
                    builds.append(int(pid))
            except FileNotFoundError as error:
                raise RuntimeError('无法检查构建进程归属') from error
        return builds

    def cleanup(self):
        guard = self.root / 'target' / 'tmp' / '.build-cleanup.lock'
        guard.parent.mkdir(parents=True, exist_ok=True)
        with guard.open('a+b') as coordination:
            fcntl.flock(coordination, fcntl.LOCK_EX)
            active = self.active()
            if active:
                raise RuntimeError(f'仍有项目构建进程，等待其安全退出后重试：{active}')
            profiles = sorted({path.parent for path in (self.root / 'target').rglob('.cargo-lock') if path.parent.name in {'debug', 'release'}})
            handles = []
            inodes = {}
            try:
                for profile in profiles:
                    if profile.is_symlink() or not profile.resolve().is_relative_to(self.root / 'target'):
                        raise RuntimeError('构建目录归属检查失败')
                    for name in ['.cargo-lock', '.cargo-build-lock']:
                        path = profile / name
                        if path.exists():
                            handle = path.open('rb')
                            handles.append(handle)
                            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                            inodes[path] = path.stat().st_ino
                # 获取所有锁后再次检查，防止扫描期间启动了协作构建。
                if self.active():
                    raise RuntimeError('持锁后发现新的项目构建进程，请安全退出后重试')
                before = self.free()
                for profile in profiles:
                    for path in profile.iterdir():
                        if path.name in {'.cargo-lock', '.cargo-build-lock'}:
                            continue
                        self.remove(path)
                    for name in ['.rustc_info.json', '.future-incompat-report.json']:
                        path = profile.parent / name
                        if path.is_file():
                            path.unlink()
                docs = []
                for base in [self.root / 'docs' / '.vitepress', *(self.root / 'target').rglob('.vitepress')]:
                    for name in ['dist', 'cache']:
                        path = base / name
                        if path.is_dir():
                            if path.is_symlink() or not path.resolve().is_relative_to(self.root):
                                raise RuntimeError('文档产物目录归属检查失败')
                            self.remove(path)
                            docs.append(str(path.relative_to(self.root)))
                if any(path.stat().st_ino != inode for path, inode in inodes.items()):
                    raise RuntimeError('Cargo 锁文件 inode 发生变化')
                after = self.free()
                event = {'before_free_bytes': before, 'after_free_bytes': after, 'released_bytes': after - before,
                         'profiles': [str(path.relative_to(self.root)) for path in profiles], 'docs': docs,
                         'lock_inodes_preserved': True}
                with (guard.parent / 'build-guard-events.jsonl').open('a') as log:
                    log.write(json.dumps(event, ensure_ascii=False) + '\n')
                print(f'已持锁清理本项目产物，释放 {after - before} 字节，可用 {after} 字节', flush=True)
                if after <= STOP_BYTES:
                    raise RuntimeError('清理后空间仍未高于32 GB停线，请先释放非项目构建产物以外的空间')
                return event
            finally:
                for handle in reversed(handles):
                    handle.close()

    @staticmethod
    def remove(path):
        if path.is_dir() and not path.is_symlink():
            shutil.rmtree(path)
        else:
            path.unlink()


def signal_group(child, signum):
    try:
        os.killpg(child.pid, signum)
    except ProcessLookupError:
        pass


def stop(child, signum):
    signal_group(child, signum)
    try:
        child.wait(timeout=30)
    except subprocess.TimeoutExpired:
        signal_group(child, signal.SIGTERM)
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            signal_group(child, signal.SIGKILL)
            child.wait()
    # 收尾仍存活的同组子进程；不向其他协作者发信号。
    signal_group(child, signal.SIGTERM)


def run(project, command, env, cancelled=lambda: None, interval=0.5):
    if project.free() <= STOP_BYTES:
        project.cleanup()
    with subprocess.Popen(command, env=env, cwd=project.root, start_new_session=True) as child:
        triggered = False
        while child.poll() is None:
            if cancelled():
                stop(child, cancelled())
                break
            if project.free() <= STOP_BYTES:
                triggered = True
                print('磁盘触及32 GB停线，正在安全停止当前验收构建', flush=True)
                stop(child, signal.SIGINT)
                break
            time.sleep(interval)
        code = child.wait()
        # 正常、失败与取消退出都复查；短构建也不能漏掉退出瞬间越线。
        if triggered or project.free() <= STOP_BYTES:
            project.cleanup()
        if cancelled():
            return 128 + cancelled()
        return 75 if triggered else (code if code >= 0 else 128 - code)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workflow', required=True)
    parser.add_argument('--target-dir')
    parser.add_argument('--tmp-dir')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if not re.fullmatch(r'[a-z0-9][a-z0-9_-]*', args.workflow):
        parser.error('工作流名称只允许小写字母、数字、横线和下划线')
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('请在 -- 后提供验收命令')
    project = Project(Path(__file__).resolve().parents[1])
    base = project.root / 'target' / 'tmp' / args.workflow
    target = project.local(args.target_dir or base / 'cargo-target')
    temporary = project.local(args.tmp_dir or base / 'tmp')
    target.mkdir(parents=True, exist_ok=True)
    temporary.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, TMPDIR=str(temporary), CARGO_TARGET_DIR=str(target), RUSTUP_TOOLCHAIN='1.94.0')
    received = None

    def cancel(signum, _frame):
        nonlocal received
        received = received or signum

    for signum in [signal.SIGINT, signal.SIGTERM, signal.SIGHUP]:
        signal.signal(signum, cancel)
    try:
        return run(project, command, env, lambda: received)
    except (OSError, ValueError, RuntimeError) as error:
        print(f'验收守卫停止：{error}', file=sys.stderr)
        return 75


if __name__ == '__main__':
    sys.exit(main())
