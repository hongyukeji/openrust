"""使用项目内隔离目录验证停线、进程退出与锁保护；不清理真实构建缓存。"""

import fcntl
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from build_guard import Project, STOP_BYTES, run


class BuildGuardTest(unittest.TestCase):
    def setUp(self):
        self.workspace = Path(__file__).resolve().parents[1]
        base = self.workspace / 'target' / 'tmp' / 'build-guard-tests'
        base.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=base)
        self.root = Path(self.temp.name)
        self.profile = self.root / 'target' / 'tmp' / 'workflow' / 'cargo-target' / 'debug'
        self.profile.mkdir(parents=True)
        self.lock = self.profile / '.cargo-lock'
        self.lock.touch()
        self.artifact = self.profile / 'generated.o'
        self.artifact.write_bytes(b'compiled')
        self.source = self.root / 'source.rs'
        self.source.write_text('source')
        self.project = Project(self.root, active=lambda: [])
        self.env = dict(os.environ, TMPDIR=str(self.root))

    def tearDown(self):
        self.temp.cleanup()

    def command(self, text):
        return [sys.executable, '-B', '-c', text]

    def test_cleanup_preserves_sources_logs_downloads_and_lock_inode(self):
        inode = self.lock.stat().st_ino
        for path in ['target/tmp/run/log.txt', 'target/tmp/downloads/package.zip', 'database/data.db']:
            file = self.root / path
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text('keep')
        docs = self.root / 'docs/.vitepress/dist'
        docs.mkdir(parents=True)
        (docs / 'index.html').write_text('compiled')
        event = self.project.cleanup()
        self.assertFalse(self.artifact.exists())
        self.assertFalse(docs.exists())
        self.assertEqual(self.lock.stat().st_ino, inode)
        self.assertTrue(event['lock_inodes_preserved'])
        self.assertEqual(self.source.read_text(), 'source')
        for path in ['target/tmp/run/log.txt', 'target/tmp/downloads/package.zip', 'database/data.db']:
            self.assertEqual((self.root / path).read_text(), 'keep')

    def test_held_cargo_lock_blocks_deletion(self):
        with self.lock.open('rb') as held:
            fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaises(BlockingIOError):
                self.project.cleanup()
            self.assertTrue(self.artifact.exists())

    def test_active_collaborator_blocks_cleanup(self):
        self.project.active = lambda: [123]
        with self.assertRaises(RuntimeError):
            self.project.cleanup()
        self.assertTrue(self.artifact.exists())

    def test_process_scan_tolerates_non_utf8_arguments(self):
        check_output = subprocess.check_output

        def process_list(command, **kwargs):
            return check_output(self.command('import sys; sys.stdout.buffer.write(b"123 python python \\xff\\n")'), **kwargs)

        with patch('build_guard.subprocess.check_output', side_effect=process_list):
            self.assertEqual(Project(self.root).active_builds(), [])

    def test_process_scan_detects_macos_truncated_command(self):
        listing = '123 /Users/Home/.rus /Users/Home/.rustup/toolchains/1.94.0/bin/cargo check\n'
        cwd = subprocess.CompletedProcess([], 0, stdout=f'p123\nfcwd\nn{self.root}\n')
        with patch('build_guard.subprocess.check_output', return_value=listing), patch('build_guard.subprocess.run', return_value=cwd):
            self.assertEqual(Project(self.root).active_builds(), [123])

    def test_normal_and_failed_runs_preserve_reusable_cache(self):
        for code in [0, 7]:
            result = run(self.project, self.command(f'raise SystemExit({code})'), self.env, interval=.01)
            self.assertEqual(result, code)
            self.assertTrue(self.artifact.exists())

    def test_before_during_and_after_exit_threshold_checks(self):
        for timing, code in [('before', 0), ('during', 0), ('after', 0), ('after', 7)]:
            with self.subTest(timing=timing, code=code):
                marker = self.root / 'finished'
                marker.unlink(missing_ok=True)
                self.artifact.write_text('compiled')
                def free():
                    low = timing == 'before' or marker.exists()
                    if timing == 'after' and marker.exists():
                        try:
                            os.kill(int(marker.read_text()), 0)
                            low = False
                        except ProcessLookupError:
                            pass
                    return STOP_BYTES if self.artifact.exists() and low else STOP_BYTES + 1024

                self.project.free = free
                script = 'import os; from pathlib import Path; Path("finished").write_text(str(os.getpid()));'
                if timing == 'during':
                    script = 'import signal,time; signal.signal(signal.SIGINT, lambda *_: exit(0));' + script + 'time.sleep(30)'
                else:
                    script += f'raise SystemExit({code})'
                result = run(self.project, self.command(script), self.env, interval=.01)
                self.assertEqual(result, 75 if timing == 'during' else code)
                self.assertFalse(self.artifact.exists())
                self.assertTrue(self.source.exists())

    def test_cancel_preserves_cache_and_reaps_own_child(self):
        marker = self.root / 'cancel'
        script = 'import signal,time; from pathlib import Path; signal.signal(signal.SIGTERM, lambda *_: exit(0)); Path("cancel").write_text("ready");time.sleep(30)'
        result = run(self.project, self.command(script), self.env,
                     cancelled=lambda: signal.SIGTERM if marker.exists() else None, interval=.01)
        self.assertEqual(result, 128 + signal.SIGTERM)
        self.assertTrue(self.artifact.exists())

    def test_cli_inherits_project_tmp_target_and_fixed_toolchain(self):
        target = self.workspace / 'target/tmp/build-guard-tests/cli-target'
        temporary = self.workspace / 'target/tmp/build-guard-tests/cli-tmp'
        script = 'import os; from pathlib import Path; assert os.environ["RUSTUP_TOOLCHAIN"]=="1.94.0"; assert Path(os.environ["TMPDIR"]).is_dir(); assert Path(os.environ["CARGO_TARGET_DIR"]).is_dir()'
        result = subprocess.run([sys.executable, '-B', str(self.workspace / 'tests/build_guard.py'),
                                 '--workflow', 'build-guard-tests', '--target-dir', str(target),
                                 '--tmp-dir', str(temporary), '--', *self.command(script)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_external_or_symlinked_targets_are_rejected(self):
        with self.assertRaises(ValueError):
            self.project.local(self.root / 'outside')
        shortcut = self.root / 'target/tmp/link'
        shortcut.symlink_to(self.workspace)
        with self.assertRaises(ValueError):
            self.project.local(shortcut / 'cargo-target')


if __name__ == '__main__':
    unittest.main()
