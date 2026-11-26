"""Exercise Telescope using the native server and a real browser."""
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request


def main():
    binary = str(Path(sys.argv[1]).resolve())
    for disabled in [False, True]:
        with tempfile.TemporaryDirectory(prefix='openrust-telescope-http-') as directory:
            root = Path(directory)
            with sqlite3.connect(root / 'database.sqlite') as database:
                database.execute(
                    'CREATE TABLE jobs (id INTEGER PRIMARY KEY AUTOINCREMENT, queue TEXT NOT NULL,'
                    ' payload TEXT NOT NULL, attempts INTEGER NOT NULL, reserved_at INTEGER,'
                    ' available_at INTEGER NOT NULL, created_at INTEGER NOT NULL)'
                )
            (root / 'sessions').mkdir()
            env = dict(os.environ, APP_BASE_PATH=directory, LOG_CHANNEL='null', TELESCOPE_PROBE_DISABLED='1' if disabled else '0')
            migrated = subprocess.run([binary, 'artisan', 'migrate', '--force'], env=env, capture_output=True, text=True, timeout=30)
            assert migrated.returncode == 0, (migrated.stdout, migrated.stderr)
            with socket.socket() as socket_probe:
                socket_probe.bind(('127.0.0.1', 0))
                port = socket_probe.getsockname()[1]
            base = f'http://127.0.0.1:{port}'
            endpoint = base + '/diagnostics/telescope'
            server = subprocess.Popen([binary, 'artisan', 'serve', '--host=127.0.0.1', f'--port={port}', '--tries=0'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            def request(path, headers=None):
                try:
                    return urllib.request.urlopen(urllib.request.Request(base + path, headers=headers or {}), timeout=2)
                except urllib.error.HTTPError as error:
                    return error
            try:
                until = time.monotonic() + 20
                while True:
                    try:
                        assert request('/diagnostics/telescope').status == 403
                        break
                    except OSError:
                        assert server.poll() is None, server.stderr.read().decode()
                        assert time.monotonic() < until, 'server startup timed out'
                        time.sleep(.1)
                assert request('/telescope', {'x-test-operator':'operator'}).status == 404
                assert request('/diagnostics/telescope', {'x-test-operator':'operator','Host':'other.example'}).status == 404
                assert request('/api/probe').status == 200
                if disabled:
                    with sqlite3.connect(root / 'database.sqlite') as database:
                        assert database.execute('SELECT COUNT(*) FROM telescope_entries').fetchone()[0] == 0
                    print('PASS: disabled Telescope stores no HTTP entries')
                else:
                    subprocess.run(['node', str(Path(__file__).with_name('dashboard_browser.mjs')), endpoint, binary], env=env, check=True, timeout=75)
            finally:
                server.terminate()
                try:
                    server.communicate(timeout=8)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.communicate(timeout=5)
    print('PASS: native custom path, domain, disabled configuration and server cleanup')


if __name__ == '__main__':
    main()
