"""在隔离源码执行一批验收，串行准入、监测磁盘并回收自建 target（macOS/Linux）。"""

import argparse
import fcntl
import hashlib
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
MIN_FREE_KIB = 31_250_000
MAX_IDE_CACHE_KIB = 8 * 1024 * 1024
LOCK = Path(tempfile.gettempdir()) / (
    "openrust-build-" + hashlib.sha256(str(ROOT).encode()).hexdigest()[:16] + ".lock"
)


def terminate(process):
    # 同时回收 Cargo 的 rustc／链接器及命令遗留的后台子进程。
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        process.poll()
        try:
            os.killpg(process.pid, 0)
        except ProcessLookupError:
            process.wait()
            return
        # 主命令可能先退出，仍给子进程的finally／服务回收留出宽限期。
        time.sleep(0.05)
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--min-free-kib", type=int, default=MIN_FREE_KIB)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    source = args.source.resolve()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if source.is_relative_to(ROOT) or not (source / "Cargo.toml").is_file():
        parser.error("--source 必须为活跃工作树之外的独立 Cargo 源码快照")
    if not command or args.min_free_kib < MIN_FREE_KIB:
        parser.error("需要验收命令；磁盘停线不得低于31,250,000 KiB")

    with LOCK.open("a+") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print("已有本项目受管构建运行；本次未启动，请完成并清理上一批后重试。", file=sys.stderr)
            return 75
        ide_target = ROOT / "target"
        if ide_target.exists():
            size = int(subprocess.check_output(["du", "-sk", str(ide_target)], text=True).split()[0])
            if size > MAX_IDE_CACHE_KIB:
                print(f"根target已达{size} KiB，超过8 GiB；先确认闲置并清理，再启动验收。", file=sys.stderr)
                return 75
        if shutil.disk_usage(source.parent).free // 1024 <= args.min_free_kib:
            print("可用空间触及磁盘停线，本次未启动。", file=sys.stderr)
            return 75

        received = []
        handlers = {}
        process = None
        target = None
        before = shutil.disk_usage(source.parent).free // 1024
        try:
            for sig in (signal.SIGINT, signal.SIGTERM):
                handlers[sig] = signal.signal(sig, lambda number, _: received.append(number))
            target = Path(tempfile.mkdtemp(prefix="openrust-build-target-", dir=source.parent))
            print(f"BUILD_TARGET={target}", flush=True)
            env = dict(os.environ, CARGO_TARGET_DIR=str(target), CARGO_INCREMENTAL="0",
                       CARGO_BUILD_JOBS="3", RUSTUP_TOOLCHAIN="1.94.0",
                       CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0")
            process = subprocess.Popen(command, cwd=source, env=env, start_new_session=True)
            while process.poll() is None:
                if received:
                    return 128 + received[0]
                if shutil.disk_usage(source.parent).free // 1024 <= args.min_free_kib:
                    print("运行中触及磁盘停线，终止本批并回收target。", file=sys.stderr)
                    return 75
                time.sleep(0.2)
            return 128 + received[0] if received else (
                process.returncode if process.returncode >= 0 else 128 - process.returncode
            )
        finally:
            if process is not None:
                terminate(process)
            if target is not None:
                # 路径只来自本进程mkdtemp，不接受外部清理路径，不扫描或删除其他批次。
                shutil.rmtree(target)
                after = shutil.disk_usage(source.parent).free // 1024
                print(f"BUILD_CLEANED={target}; available_KiB={after}; "
                      f"net_change_KiB={after - before}", flush=True)
            for sig, handler in handlers.items():
                signal.signal(sig, handler)


if __name__ == "__main__":
    sys.exit(main())
