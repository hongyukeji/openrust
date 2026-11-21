"""用真实子进程验证构建准入、退出清理与取消边界，不构建整个workspace。"""

from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


RUNNER = Path(__file__).with_name("build_session.py")
ROOT = RUNNER.parent.parent


class BuildSessionLifecycle(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="openrust-build-lifecycle-")
        self.addCleanup(self.directory.cleanup)
        self.source = Path(self.directory.name) / "source"
        self.source.mkdir()
        (self.source / "Cargo.toml").write_text('[package]\nname="probe"\nversion="0.0.0"\n')
        (self.source / "keep.txt").write_text("source must survive")

    def command(self, code, *options):
        return [sys.executable, "-B", str(RUNNER), "--source", str(self.source),
                *options, "--", sys.executable, "-B", "-c", code]

    def assert_clean(self, output):
        target = Path(next(line.split("=", 1)[1] for line in output.splitlines()
                           if line.startswith("BUILD_TARGET=")))
        self.assertFalse(target.exists())
        self.assertEqual((self.source / "keep.txt").read_text(), "source must survive")
        self.assertIn("BUILD_CLEANED=", output)

    def test_success_and_failure_cleanup_and_propagate_environment(self):
        for status in (0, 37):
            with self.subTest(status=status):
                code = f'''
import os
from pathlib import Path
assert os.environ["CARGO_INCREMENTAL"] == "0"
assert os.environ["CARGO_BUILD_JOBS"] == "3"
assert os.environ["RUSTUP_TOOLCHAIN"] == "1.94.0"
Path(os.environ["CARGO_TARGET_DIR"], "artifact").write_text("generated")
raise SystemExit({status})
'''
                result = subprocess.run(self.command(code), capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, status, result.stderr)
                self.assert_clean(result.stdout)

    def test_low_disk_and_live_worktree_refuse_before_launch(self):
        marker = self.source / "launched"
        command = self.command(f'open({str(marker)!r}, "w").close()',
                               "--min-free-kib", str(10**18))
        result = subprocess.run(command, capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 75)
        self.assertFalse(marker.exists())
        command[command.index(str(self.source))] = str(ROOT)
        result = subprocess.run(command, capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 2)
        self.assertFalse(marker.exists())
        self.assertFalse(list(self.source.parent.glob("openrust-build-target-*")))

    def test_cancel_kills_children_cleans_and_releases_admission_lock(self):
        self.verify_cancel(signal.SIGTERM)

    def test_interrupt_cleans_and_releases_admission_lock(self):
        self.verify_cancel(signal.SIGINT)

    def verify_cancel(self, cancel_signal):
        marker = self.source / "ready"
        child_ready = self.source / "child-ready"
        child_cleaned = self.source / "child-cleaned"
        child_code = f'''
import signal, sys, time
from pathlib import Path
def stop(*_):
    time.sleep(0.2)
    Path({str(child_cleaned)!r}).touch()
    sys.exit(0)
signal.signal(signal.SIGTERM, stop)
Path({str(child_ready)!r}).touch()
time.sleep(120)
'''
        code = f'''
from pathlib import Path
import subprocess, sys, time
child = subprocess.Popen([sys.executable, "-c", {child_code!r}])
while not Path({str(child_ready)!r}).exists():
    time.sleep(0.01)
Path({str(marker)!r}).write_text(str(child.pid))
time.sleep(120)
'''
        process = subprocess.Popen(self.command(code), stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True)
        try:
            deadline = time.monotonic() + 10
            while not marker.exists() and process.poll() is None and time.monotonic() < deadline:
                time.sleep(0.05)
            self.assertTrue(marker.exists())
            other = subprocess.run(self.command('raise AssertionError("must not launch")'),
                                   capture_output=True, text=True, timeout=10)
            self.assertEqual(other.returncode, 75, other.stderr)
            process.send_signal(cancel_signal)
            output, error = process.communicate(timeout=15)
            self.assertEqual(process.returncode, 128 + cancel_signal, error)
            self.assert_clean(output)
            self.assertTrue(child_cleaned.exists(), "取消必须给后台子进程的清理逻辑留下宽限期")
            child_pid = int(marker.read_text())
            state = subprocess.run(["ps", "-p", str(child_pid), "-o", "stat="],
                                   capture_output=True, text=True).stdout.strip()
            self.assertTrue(not state or state.startswith("Z"), state)
            result = subprocess.run(self.command("pass"), capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assert_clean(result.stdout)
        finally:
            if process.poll() is None:
                process.terminate()
                process.communicate(timeout=15)

    def test_running_disk_limit_terminates_and_cleans(self):
        marker = self.source / "started"
        # 不真实填满磁盘：仅注入磁盘读数，启动和回收均使用真实子进程。
        shim = f'''
import sys
from pathlib import Path
from types import SimpleNamespace
sys.path.insert(0, {str(RUNNER.parent)!r})
import build_session
original = build_session.shutil.disk_usage
def disk_usage(path):
    if Path({str(marker)!r}).exists():
        return SimpleNamespace(free=1)
    return original(path)
build_session.shutil.disk_usage = disk_usage
sys.argv = {self.command(f'from pathlib import Path; import time; Path({str(marker)!r}).touch(); time.sleep(120)')[2:]!r}
raise SystemExit(build_session.main())
'''
        result = subprocess.run([sys.executable, "-B", "-c", shim], capture_output=True,
                                text=True, timeout=15)
        self.assertEqual(result.returncode, 75, result.stderr)
        self.assertIn("运行中触及磁盘停线", result.stderr)
        self.assert_clean(result.stdout)

    def test_large_root_cache_refuses_without_deleting_it(self):
        cache = self.source.parent / "ide-cache"
        (cache / "target").mkdir(parents=True)
        keep = cache / "target/keep.txt"
        keep.write_text("another owner's cache")
        shim = f'''
import sys
from pathlib import Path
sys.path.insert(0, {str(RUNNER.parent)!r})
import build_session
build_session.ROOT = Path({str(cache)!r})
original = build_session.subprocess.check_output
def output(command, **kwargs):
    if command[:2] == ["du", "-sk"]:
        return "9000000 target\\n"
    return original(command, **kwargs)
build_session.subprocess.check_output = output
sys.argv = {self.command('raise AssertionError("must not launch")')[2:]!r}
raise SystemExit(build_session.main())
'''
        result = subprocess.run([sys.executable, "-B", "-c", shim], capture_output=True,
                                text=True, timeout=15)
        self.assertEqual(result.returncode, 75, result.stderr)
        self.assertIn("超过8 GiB", result.stderr)
        self.assertEqual(keep.read_text(), "another owner's cache")
        self.assertFalse(list(self.source.parent.glob("openrust-build-target-*")))


if __name__ == "__main__":
    unittest.main()
