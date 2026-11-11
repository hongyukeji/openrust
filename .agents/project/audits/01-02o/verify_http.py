"""Exercise the standalone application over TCP in a relocated deployment."""
import contextlib
import http.client
import json
import os
from pathlib import Path
import re
import select
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory

source = Path(sys.argv[1]).resolve()
audit = Path(__file__).resolve().parent
config_keys = set(re.findall(r'"([A-Z][A-Z0-9_]+)"', '\n'.join(p.read_text() for p in (source / 'config').glob('*.rs'))))
config_keys.update(('APP_BASE_PATH', 'APP_ENV', 'GRPC_HOST', 'GRPC_PORT'))
env = {key: value for key, value in os.environ.items() if key not in config_keys}
results = []

with TemporaryDirectory(prefix='openrust-00-02-') as directory:
    root = Path(directory)
    app = root / 'release'
    app.mkdir()
    shutil.copy2(source / 'target/release/main', app / 'main')
    working = root / 'foreign-cwd'
    working.mkdir()

    def write(name, value):
        path = app / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)

    write('resources/views/welcome.html', 'welcome from named view')
    write('public/index.html', 'static index must not shadow GET /')
    write('public/site.txt', 'website file')
    write('public/storage/collision.txt', 'website wins')
    write('storage/app/public/collision.txt', 'disk loses')
    write('storage/app/public/disk.txt', 'disk file')
    write('public/storage/tmp/collision.txt', 'nested website wins')
    write('public/private.txt', 'inside root')
    write('private.txt', 'secret outside root')
    if os.name == 'posix':
        (app / 'public/escape.txt').symlink_to(app / 'private.txt')

    for prefix in ['/storage', '/storage/tmp/']:
        write('.env', f'APP_HOST=127.0.0.1\nAPP_PORT=0\nAPP_URL=http://localhost\nAPP_DEBUG=false\nPUBLIC_STORAGE_URL=http://localhost{prefix}\n')
        with subprocess.Popen([app / 'main'], cwd=working, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as process:
            try:
                assert select.select([process.stdout], [], [], 15)[0], 'startup timeout'
                line = process.stdout.readline()
                match = re.search(r'http://127\.0\.0\.1:(\d+)', line)
                assert match, (line, process.poll())
                port = int(match[1])

                def request(method, path, status, expected=None, headers=None, media=None):
                    with contextlib.closing(http.client.HTTPConnection('127.0.0.1', port, timeout=5)) as client:
                        client.request(method, path, headers=headers or {})
                        response = client.getresponse()
                        body = response.read()
                        assert response.status == status, (method, path, response.status, body)
                        if expected is not None:
                            assert body == expected.encode(), (path, body)
                        if media:
                            assert response.getheader('content-type').startswith(media), (path, response.getheaders())
                        results.append(dict(prefix=prefix, method=method, path=path, status=status))
                        return dict(response.getheaders()), body

                request('GET', '/', 200, 'welcome from named view', media='text/html')
                request('HEAD', '/', 200, '')
                write('resources/views/welcome.html', 'updated named view')
                request('GET', '/', 200, 'updated named view')
                request('GET', '/up', 200, '{"status":"up"}', {'Accept': 'application/json'}, 'application/json')
                _, body = request('GET', '/up', 200, media='text/html')
                assert b'Application up' in body
                request('HEAD', '/up', 200, '', {'Accept': 'application/json'}, 'application/json')
                disk_prefix = prefix.rstrip('/')
                for name, value in [('collision.txt', 'website wins' if prefix == '/storage' else 'nested website wins'), ('disk.txt', 'disk file')]:
                    request('GET', f'{disk_prefix}/{name}', 200, value)
                    request('HEAD', f'{disk_prefix}/{name}', 200, '')
                request('GET', '/site.txt', 200, 'website file')
                headers, _ = request('GET', f'{disk_prefix}/disk.txt', 200, 'disk file')
                modified = next(value for key, value in headers.items() if key.lower() == 'last-modified')
                for method in ['GET', 'HEAD']:
                    request(method, f'{disk_prefix}/disk.txt', 304, '', {'If-Modified-Since': modified})
                for path, accept, media in [
                    ('/api/missing', 'text/html', 'application/json'),
                    ('/missing', 'text/html', 'text/html'),
                    ('/missing', 'application/json', 'application/json'),
                    ('/missing', 'application/json;q=0.1, text/html;q=0.9', 'text/html'),
                ]:
                    request('GET', path, 404, headers={'Accept': accept}, media=media)
                    request('HEAD', path, 404, '', {'Accept': accept}, media)
                for path in [disk_prefix, disk_prefix + '/', '/%2e%2e/private.txt', '/escape.txt']:
                    request('GET', path, 404)
                request('POST', '/site.txt', 404)
                request('POST', '/', 405)
                (app / 'resources/views/welcome.html').unlink()
                request('GET', '/', 500, headers={'Accept': 'application/json'}, media='application/json')
                write('resources/views/welcome.html', 'welcome from named view')
                for arguments in [('inspire',), ('list',), ('help', 'inspire')]:
                    command = subprocess.run([app / 'main', 'artisan', *arguments], cwd=working, env=env, text=True, capture_output=True, timeout=15)
                    assert command.returncode == 0, command.stderr
                    assert command.stdout
            finally:
                process.terminate()
                process.wait(timeout=10)
                assert process.returncode == 0, process.stderr.read()

(audit / 'http-results.json').write_text(json.dumps(dict(http=results, artisan_invocations=6), indent=2) + '\n')
print(f'PASS: {len(results)} real HTTP requests and 6 Artisan invocations')
