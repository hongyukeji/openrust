"""使用项目内隔离目录验证停线、进程退出与锁保护；不清理真实构建缓存。"""

import fcntl
import contextlib
import io
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from build_guard import Project, STOP_BYTES, run, main, signal_group


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

    def test_cli_system_error_preserves_stop_code_and_origin(self):
        args = ['build_guard.py', '--workflow', 'build-guard-tests', '--target-dir', str(self.profile.parent), '--tmp-dir', str(self.root / 'tmp'), '--', 'ignored']
        output = io.StringIO()
        with patch('sys.argv', args), patch('build_guard.signal.signal'), patch('build_guard.run', side_effect=PermissionError(1, '受控权限失败')), contextlib.redirect_stderr(output):
            self.assertEqual(main(), 75)
        self.assertIn('验收守卫停止', output.getvalue())
        self.assertIn('PermissionError', output.getvalue())
        self.assertIn('in main', output.getvalue())
        self.assertTrue(self.artifact.exists())

    def test_expected_stop_keeps_concise_diagnostic(self):
        args = ['build_guard.py', '--workflow', 'build-guard-tests', '--target-dir', str(self.profile.parent), '--tmp-dir', str(self.root / 'tmp'), '--', 'ignored']
        output = io.StringIO()
        with patch('sys.argv', args), patch('build_guard.signal.signal'), patch('build_guard.run', side_effect=RuntimeError('仍有受控构建')), contextlib.redirect_stderr(output):
            self.assertEqual(main(), 75)
        self.assertIn('仍有受控构建', output.getvalue())
        self.assertNotIn('Traceback', output.getvalue())
        self.assertTrue(self.artifact.exists())

    def test_external_or_symlinked_targets_are_rejected(self):
        with self.assertRaises(ValueError):
            self.project.local(self.root / 'outside')
        shortcut = self.root / 'target/tmp/link'
        shortcut.symlink_to(self.workspace)
        with self.assertRaises(ValueError):
            self.project.local(shortcut / 'cargo-target')


    def test_retired_group_permission_error_requires_verified_no_live_members(self):
        from types import SimpleNamespace
        for listing in ['', '123 Z\n', '123 Z+\n456 S\n']:
            with self.subTest(listing=listing), patch('build_guard.os.killpg', side_effect=PermissionError(1, '受控权限拒绝')), patch('build_guard.subprocess.check_output', return_value=listing):
                signal_group(SimpleNamespace(pid=123), signal.SIGTERM)

    def test_permission_error_with_live_group_is_not_suppressed(self):
        from types import SimpleNamespace
        for listing in ['123 S\n', '123 Z\n123 S+\n']:
            with self.subTest(listing=listing), patch('build_guard.os.killpg', side_effect=PermissionError(1, '受控存活组拒绝')), patch('build_guard.subprocess.check_output', return_value=listing):
                with self.assertRaises(PermissionError):
                    signal_group(SimpleNamespace(pid=123), signal.SIGTERM)

    def test_permission_error_with_failed_group_inspection_is_not_suppressed(self):
        from types import SimpleNamespace
        with patch('build_guard.os.killpg', side_effect=PermissionError(1, '受控权限拒绝')), patch('build_guard.subprocess.check_output', side_effect=OSError('受控进程表失败')):
            with self.assertRaises(PermissionError):
                signal_group(SimpleNamespace(pid=123), signal.SIGTERM)

    @unittest.skipUnless(sys.platform == 'darwin', '验证 macOS 僵尸组的真实 EPERM')
    def test_actual_macos_zombie_only_group_finishes_without_permission_failure(self):
        child = subprocess.Popen(self.command('raise SystemExit(0)'), start_new_session=True, env=self.env)
        try:
            import time
            for _ in range(100):
                listing = subprocess.check_output(['ps', '-axo', 'pid=,pgid=,stat='], text=True)
                group = [line.split() for line in listing.splitlines() if len(line.split()) == 3 and int(line.split()[1]) == child.pid]
                if group and all(row[2].startswith('Z') for row in group):
                    break
                time.sleep(.01)
            self.assertTrue(group)
            self.assertTrue(all(row[2].startswith('Z') for row in group))
            with self.assertRaises(PermissionError):
                os.killpg(child.pid, signal.SIGTERM)
            signal_group(child, signal.SIGTERM)
        finally:
            child.wait(timeout=3)

    def test_completed_stop_with_retired_group_still_performs_threshold_cleanup(self):
        import build_guard
        original = build_guard.signal_group
        def signals(child, signum):
            if child.returncode is None:
                return original(child, signum)
            with patch('build_guard.os.killpg', side_effect=PermissionError(1, '受控组退出竞态')), patch('build_guard.subprocess.check_output', return_value=''):
                return original(child, signum)
        marker = self.root / 'stop-ready'
        script = 'import signal,time; from pathlib import Path; signal.signal(signal.SIGINT, lambda *_: exit(0)); Path("stop-ready").write_text("ready");time.sleep(30)'
        self.project.free = lambda: STOP_BYTES if marker.exists() and self.artifact.exists() else STOP_BYTES + 1024
        with patch('build_guard.signal_group', side_effect=signals):
            self.assertEqual(run(self.project, self.command(script), self.env, interval=.01), 75)
        self.assertFalse(self.artifact.exists())
        self.assertTrue(self.source.exists())


if __name__ == '__main__':
    unittest.main()
