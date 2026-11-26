"""通过原生 Artisan、真实 TCP HTTP 和实际 SQLite 记录验收敏感路由。"""
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
    project = Path(__file__).resolve().parents[3]
    work = project / 'target' / 'tmp' / 'telescope-sensitive-http'
    work.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='application-', dir=work) as directory:
        root = Path(directory)
        database = root / 'database.sqlite'
        sqlite3.connect(database).close()
        env = dict(os.environ, APP_BASE_PATH=str(root), LOG_CHANNEL='null', TMPDIR=str(root))
        subprocess.run([binary, 'artisan', 'migrate', '--force'], env=env, check=True, timeout=60)
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0))
            port = probe.getsockname()[1]
        base = f'http://127.0.0.1:{port}'
        with (work / f'{os.getpid()}-server.log').open('wb') as output:
            server = subprocess.Popen([binary, 'artisan', 'serve', '--host=127.0.0.1', f'--port={port}', '--tries=0'], env=env, stdout=output, stderr=subprocess.STDOUT)
            def request(path, sensitive=False):
                body = json.dumps({'password': 'PRIVATE-PASSWORD', 'code': 'PRIVATE-BODY'}).encode() if sensitive else None
                headers = {'Content-Type': 'application/json', 'Authorization': 'Bearer PRIVATE-BEARER', 'Cookie': 'session=PRIVATE-SESSION', 'X-Auth-Confirmation': 'PRIVATE-PROOF'}
                try:
                    response = urllib.request.urlopen(urllib.request.Request(base + path, data=body, headers=headers), timeout=5)
                except urllib.error.HTTPError as error:
                    response = error
                return response.status, response.read()
            def entries(after):
                with sqlite3.connect(database) as connection:
                    return connection.execute('SELECT type, content FROM telescope_entries WHERE rowid > ? ORDER BY rowid', (after,)).fetchall()
            def sequence():
                with sqlite3.connect(database) as connection:
                    return connection.execute('SELECT COALESCE(MAX(rowid), 0) FROM telescope_entries').fetchone()[0]
            def wait_entries(after, count):
                until = time.monotonic() + 5
                while True:
                    rows = entries(after)
                    if sum(kind == 'request' for kind, _ in rows) >= count:
                        return rows
                    assert time.monotonic() < until, rows
                    time.sleep(.05)
            try:
                until = time.monotonic() + 20
                while True:
                    try:
                        assert request('/missing')[0] == 404
                        break
                    except OSError:
                        assert server.poll() is None, 'server exited; inspect preserved server log'
                        assert time.monotonic() < until, 'server startup timed out'
                        time.sleep(.1)
                wait_entries(0, 1)
                before = sequence()
                status, body = request('/api/reset/PRIVATE-PATH?code=PRIVATE-QUERY', True)
                assert status == 200, (status, body)
                assert json.loads(body)['token'] == 'PRIVATE-TOKEN', body
                assert request('/api/reset/fail?code=PRIVATE-QUERY', True)[0] == 403
                rows = wait_entries(before, 2)
                assert len(rows) == 2, rows
                for kind, content in rows:
                    assert kind == 'request' and 'PRIVATE' not in content, (kind, content)
                    value = json.loads(content)
                    assert value['sensitive'] is True and value['uri'] == '/api/reset/{token}', value
                    assert value['payload'] == '[redacted]' and value['response'] == '[redacted]', value
                before = sequence()
                assert request('/api/ordinary')[0] == 200
                rows = wait_entries(before, 1)
                assert {'request', 'query', 'mail'} <= {kind for kind, _ in rows}, rows
                assert any('PRIVATE-MAIL' in content for kind, content in rows if kind == 'mail'), rows
                print('PASS: sensitive 200/403 retain only safe metadata; real SQL, mail, headers, query and body omitted; ordinary recording and HTTP response preserved', flush=True)
            finally:
                server.terminate()
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=5)
    print('PASS: native server stopped and isolated database removed; logs retained', flush=True)


if __name__ == '__main__':
    main()
