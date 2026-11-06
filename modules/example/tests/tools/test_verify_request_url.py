"""防止 URL 验收依赖旧配置、继承部署凭据或写入非自有目录。"""
import importlib.util
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('verify_request_url', Path(__file__).with_name('verify-request-url.py'))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class PreparationTests(unittest.TestCase):
    def test_registered_modes_without_legacy_configuration(self):
        with tempfile.TemporaryDirectory(dir='/tmp') as source, tempfile.TemporaryDirectory(
                dir='/tmp', prefix='openrust-c04d6-url-network-') as temporary:
            root, owned = Path(source).resolve(), Path(temporary)
            module = root / 'modules/example'
            module.mkdir(parents=True)
            marker = module / 'source.rs'
            marker.write_text('preserve')
            profiles = []
            for trusted in (True, False):
                cwd, config, port, profile = guard.prepare(root, owned, trusted)
                self.assertEqual(list(config.iterdir()), [])
                self.assertFalse((cwd / 'modules_statuses.json').exists())
                self.assertEqual((cwd / 'modules/example').resolve(), module)
                self.assertEqual(profile['MODULES_ACTIVATOR'], 'registered')
                self.assertEqual(profile['SESSION_DRIVER'], 'null')
                self.assertGreater(port, 0)
                profiles.append(profile)
            self.assertEqual(profiles[0]['APP_SERVER_TRUSTED_PROXY_RANGES'], '["127.0.0.1/32"]')
            self.assertEqual(profiles[1]['APP_SERVER_TRUSTED_PROXY_RANGES'], '[]')
            guard.clear_sensitive(owned)
            self.assertEqual(marker.read_text(), 'preserve')
            self.assertFalse((owned / 'trusted/modules/example').exists())

    def test_reject_foreign_directory_before_writing(self):
        with tempfile.TemporaryDirectory(dir='/tmp') as temporary:
            owned = Path(temporary)
            with self.assertRaises(guard.Failure):
                guard.prepare(owned, owned, True)
            self.assertEqual(list(owned.iterdir()), [])
            with self.assertRaises(guard.Failure):
                guard.clear_sensitive(owned)

    def test_reject_symlink_owned_directory(self):
        with tempfile.TemporaryDirectory(dir='/tmp', prefix='openrust-c04d6-url-network-') as temporary:
            owned = Path(temporary)
            link = owned / 'link'
            link.symlink_to(owned, target_is_directory=True)
            with self.assertRaises(guard.Failure):
                guard.validate_owned_directory(link)

    def test_environment_drops_parent_credentials_and_switches(self):
        inherited = {'PATH': '/bin', 'HOME': '/home/test', 'TMPDIR': '/tmp',
                     'DB_PASSWORD': 'not-inherited', 'DATABASE_URL': 'not-inherited',
                     'APP_ENV': 'production', 'SESSION_DRIVER': 'database', 'MODULES_ACTIVATOR': 'file'}
        profile = {'MODULES_ACTIVATOR': 'registered', 'SESSION_DRIVER': 'null'}
        with patch.dict(os.environ, inherited, clear=True):
            actual = guard.server_environment(Path('/isolated/config'), profile)
            self.assertEqual(actual, {'PATH': '/bin', 'HOME': '/home/test', 'TMPDIR': '/tmp',
                                     'APP_CONFIG_DIR': '/isolated/config', 'APP_ENV': 'test',
                                     'RUST_LOG': 'off', **profile})
            self.assertEqual(dict(os.environ), inherited)

    def test_resource_warning_and_hard_boundaries(self):
        with tempfile.TemporaryDirectory(dir='/tmp') as temporary:
            root = Path(temporary)
            (root / 'target').mkdir()
            cases = [(79_000_000_000, .08, False), (80_000_000_000, .08, True),
                     (0, .079, True), (0, .05, True), (100_000_000_000, .50, None),
                     (0, .049, None)]
            for size, free, expected in cases:
                with self.subTest(size=size, free=free), patch.dict(os.environ, {'CARGO_TARGET_DIR': str(root / 'target')}), patch.object(
                        guard.subprocess, 'run', return_value=SimpleNamespace(stdout=f'{size // 1024}\tpath')), patch.object(
                        guard.os, 'statvfs', return_value=SimpleNamespace(f_bavail=int(free * 1000), f_frsize=1, f_blocks=1000)):
                    if expected is None:
                        with self.assertRaises(guard.Failure):
                            guard.resources(root)
                    else:
                        result = guard.resources(root)
                        self.assertEqual(result['soft_warning'], expected)
                        self.assertEqual(len(result['target_bytes']), 1)


if __name__ == '__main__':
    unittest.main()
