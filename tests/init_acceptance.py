"""隔离源码中验证 init、重复执行和初始化后的真实 HTTP；由 mysql_acceptance.py 提供数据库。"""
import json
import hashlib
import re
import base64
import socketserver
import threading
import pty
import select
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request


def api_probe(source, runtime, path, token, logfile):
    with socket.socket() as sock:
        sock.bind(('127.0.0.1',0))
        port=sock.getsockname()[1]
    env=runtime | {'APP_HOST':'127.0.0.1','APP_PORT':str(port)}
    with logfile.open('w') as log:
        process=subprocess.Popen(['cargo','run','--quiet','--locked'],cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        try:
            base=f'http://127.0.0.1:{port}'
            for _ in range(120):
                try:
                    urllib.request.urlopen(base+'/up',timeout=1).close()
                    break
                except urllib.error.URLError:
                    time.sleep(0.25)
            else: raise RuntimeError('default-prefix application did not start')
            try:
                urllib.request.urlopen(urllib.request.Request(base+path,headers={'Accept':'application/json'}),timeout=5)
                raise AssertionError('anonymous API access accepted')
            except urllib.error.HTTPError as error: assert error.code==401
            with urllib.request.urlopen(urllib.request.Request(base+path,headers={'Accept':'application/json','Authorization':'Bearer '+token}),timeout=5) as response:
                assert response.status==200
                assert json.loads(response.read())['id']==9871
        finally:
            os.killpg(process.pid,signal.SIGINT)
            try:process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid,signal.SIGKILL);process.wait()


def main():
    root = Path(__file__).resolve().parents[1]
    stage = Path(os.environ['OPENRUST_INIT_STAGE'])
    source = stage / 'source'
    source.mkdir(parents=True, exist_ok=False)
    checkout = subprocess.run(['git', 'rev-parse', '--show-toplevel'], cwd=root, capture_output=True, text=True)
    in_checkout = checkout.returncode == 0 and Path(checkout.stdout.strip()).resolve() == root
    tracked = subprocess.run(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=root, capture_output=True) if in_checkout else None
    if tracked is not None and tracked.returncode == 0:
        files = tracked.stdout.decode().split('\0')
    else:
        files=[]
        for base, directories, names in os.walk(root):
            if Path(base) == root:
                directories[:] = [name for name in directories if name not in ('target','node_modules','.git','tmp','storage')]
            files.extend(str((Path(base)/name).relative_to(root)) for name in names)

    for name in files:
        if not name or name.startswith(('.agents/', 'docs/', 'tests/', '.github/')):
            continue
        path = root / name
        if path.is_file() and not path.is_symlink():
            destination = source / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, destination)
    url = urllib.parse.urlsplit(os.environ['OPENRUST_TEST_MYSQL_URL'])
    # This is the harness-owned ephemeral database, never the user's development database.
    env_text = (source / '.env.example').read_text()
    env_text += '\n' + '\n'.join([
        'DB_CONNECTION=mysql', f'DB_HOST={url.hostname}', f'DB_PORT={url.port}',
        f'DB_DATABASE={url.path.lstrip("/")}', f'DB_USERNAME={urllib.parse.unquote(url.username)}',
        f'DB_PASSWORD={urllib.parse.unquote(url.password)}', 'APP_NAME="Init acceptance"',
        'API_PREFIX=api', 'MAIL_FROM_ADDRESS=hello@example.test',
    ]) + '\n'
    if os.environ.get('OPENRUST_INIT_REDIS_PORT'):
        env_text += '\n' + '\n'.join(['CACHE_STORE=redis', 'SESSION_DRIVER=redis', 'QUEUE_CONNECTION=redis', 'REDIS_HOST=127.0.0.1', f'REDIS_PORT={os.environ["OPENRUST_INIT_REDIS_PORT"]}', 'REDIS_DB=0']) + '\n'
    if os.environ.get('OPENRUST_INIT_TEST_INTEGRATIONS'):
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0))
            reverb_port=probe.getsockname()[1]
        env_text += '\n' + '\n'.join(['REVERB_ENABLED=true', 'REVERB_SERVER_HOST=127.0.0.1', f'REVERB_SERVER_PORT={reverb_port}', 'REVERB_HOST=127.0.0.1', f'REVERB_PORT={reverb_port}', 'REVERB_SCHEME=http', 'BROADCAST_CONNECTION=reverb', 'MAIL_MAILER=smtp', 'MAIL_HOST=127.0.0.1', f'MAIL_PORT={os.environ["OPENRUST_INIT_SMTP_PORT"]}', 'MAIL_SCHEME=smtp', 'MAIL_USERNAME=null', 'MAIL_PASSWORD=null']) + '\n'
    (source / '.env').write_text(env_text)
    (source / '.env').chmod(0o600)
    runtime = os.environ.copy()
    runtime['APP_BASE_PATH'] = str(source)
    runtime['RUSTUP_TOOLCHAIN'] = '1.94.0'
    binary = Path(runtime['CARGO_TARGET_DIR']) / 'debug/main'
    initial_binary = Path(os.environ['OPENRUST_INIT_BINARY'])
    initial_extensions = ['--extensions=api,broadcasting,reverb,grpc'] if os.environ.get('OPENRUST_INIT_TEST_INTEGRATIONS') else ['--extensions=api']
    with (stage / 'init.log').open('w') as log:
        subprocess.run([str(initial_binary), 'artisan', 'init', '--no-interaction', *initial_extensions], env=runtime, cwd=source, stdout=log, stderr=subprocess.STDOUT, check=True)
    if initial_extensions == ['--extensions=api']:
        assert not (source / 'routes/channels.rs').exists()
        assert not (source / 'routes/grpc.rs').exists()
    key_lines = [line for line in (source / '.env').read_text().splitlines() if line.startswith('APP_KEY=')]
    assert len(key_lines) == 1 and key_lines[0] != 'APP_KEY='
    reverb_credentials = [line for line in (source / '.env').read_text().splitlines() if line.startswith(('REVERB_APP_ID=', 'REVERB_APP_KEY=', 'REVERB_APP_SECRET='))]
    if os.environ.get('OPENRUST_INIT_TEST_INTEGRATIONS'):
        assert len(reverb_credentials) == 3
    migrations = sorted((source / 'database/migrations').glob('*.rs'))
    plain='init-acceptance-token'
    hashed=hashlib.sha256(plain.encode()).hexdigest()
    sql=("INSERT INTO users (id,name,email,password,created_at,updated_at) VALUES (9871,'API acceptance','init@example.test','not-used',NOW(),NOW());"
         "INSERT INTO personal_access_tokens (id,tokenable_type,tokenable_id,name,token,abilities,created_at,updated_at) VALUES (9871,'app::models::User',9871,'acceptance','"+hashed+"','[\"*\"]',NOW(),NOW());")
    subprocess.run(['docker','exec',os.environ['OPENRUST_ACCEPTANCE_MYSQL_CONTAINER'],'sh','-ec','MYSQL_PWD="$MYSQL_ROOT_PASSWORD" mysql -uroot "$MYSQL_DATABASE" -e "$1"','sh',sql],check=True,stdout=subprocess.DEVNULL)
    token='9871|'+plain
    api_probe(source,runtime,'/api/user',token,stage/'default-api.log')
    with (source / '.env').open('a') as changed:
        changed.write('\nAPI_PREFIX=api/admin\n')
    with (stage / 'repeat.log').open('w') as log:
        subprocess.run([str(binary), 'artisan', 'init', '--extensions=api,broadcasting,reverb,grpc', '--force', '--no-interaction'], env=runtime, cwd=source, stdout=log, stderr=subprocess.STDOUT, check=True)
    assert (source / 'routes/channels.rs').is_file()
    assert (source / 'routes/grpc.rs').is_file()
    assert key_lines == [line for line in (source / '.env').read_text().splitlines() if line.startswith('APP_KEY=')]
    assert migrations == sorted((source / 'database/migrations').glob('*.rs'))
    assert reverb_credentials == [line for line in (source / '.env').read_text().splitlines() if line.startswith(('REVERB_APP_ID=', 'REVERB_APP_KEY=', 'REVERB_APP_SECRET='))]
    assert 'API_PREFIX=api' in (source / '.env').read_text()
    if os.environ.get('OPENRUST_INIT_TEST_INTERACTIVE'):
        master, slave = pty.openpty()
        interactive = subprocess.Popen([str(binary), 'artisan', 'init'], cwd=source, env=runtime, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
        os.close(slave)
        deadline = time.monotonic() + 120
        transcript = bytearray()
        pending = b''
        answers = 0
        try:
            while interactive.poll() is None and time.monotonic() < deadline:
                if not select.select([master], [], [], 0.2)[0]:
                    continue
                try:
                    chunk = os.read(master, 65536)
                except OSError:
                    break
                transcript.extend(chunk)
                (stage / 'interactive.progress.log').write_bytes(transcript)
                pending += chunk
                pending = re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]',b'',pending).replace(b'\r',b'')
                if pending.endswith(b': ') or pending.endswith(b'[Y/n] ') or pending.endswith(b'[y/N] '):
                    answer = b'api,broadcasting,reverb,grpc\n' if b'EXTENSIONS [' in pending else ('Init acceptance 中文\n'.encode() if b'APP_NAME [' in pending else b'\n')
                    os.write(master, answer)
                    pending = b''
                    answers += 1
            interactive.wait(timeout=10)
            assert interactive.returncode == 0, 'interactive init failed'
            assert answers >= 16, answers
            assert b'DB_PASSWORD [Enter keeps existing value]' in transcript
            (stage / 'interactive.log').write_bytes(transcript)
            assert key_lines == [line for line in (source / '.env').read_text().splitlines() if line.startswith('APP_KEY=')]
            assert 'Init acceptance 中文' in (source / '.env').read_text()
        finally:
            (stage / 'interactive.log').write_bytes(transcript)
            if interactive.poll() is None:
                os.killpg(interactive.pid, signal.SIGKILL)
                interactive.wait()
            os.close(master)
    saved_env = (source / '.env').read_text()
    try:
        (source / '.env').write_text(saved_env + '\nDB_PREFIX=' + 'x' * 100 + '\n')
        with (stage / 'migration-failure.log').open('w') as log:
            failure = subprocess.run([str(binary), 'artisan', 'init', '--extensions=api,broadcasting,reverb,grpc', '--no-interaction'], cwd=source, env=runtime, stdout=log, stderr=subprocess.STDOUT)
        assert failure.returncode != 0, 'migration failure must not be reported as successful initialization'
        assert (source / 'routes/api.rs').is_file()
        assert migrations == sorted((source / 'database/migrations').glob('*.rs'))
        assert key_lines == [line for line in (source / '.env').read_text().splitlines() if line.startswith('APP_KEY=')]
    finally:
        (source / '.env').write_text(saved_env)
    default_environment=(source / '.env').read_bytes()
    selected='\n'.join('APP_KEY=' if line.startswith('APP_KEY=') else ('APP_ENV=production' if line.startswith('APP_ENV=') else line) for line in saved_env.splitlines())+'\n'
    (source / '.env.acceptance').write_text(selected)
    (source / '.env.acceptance').chmod(0o600)
    with (stage / 'selected-environment.log').open('w') as log:
        subprocess.run([str(binary),'artisan','init','--extensions=api,broadcasting,reverb,grpc','--env=acceptance','--force','--no-interaction'],cwd=source,env=runtime,stdout=log,stderr=subprocess.STDOUT,check=True)
    assert (source / '.env').read_bytes()==default_environment
    selected_keys=[line for line in (source / '.env.acceptance').read_text().splitlines() if line.startswith('APP_KEY=')]
    assert len(selected_keys)==1 and selected_keys[0]!='APP_KEY='
    assert selected_keys!=key_lines
    if os.environ.get('OPENRUST_INIT_REDIS_PORT'):
        with (stage / 'worker.log').open('w') as log:
            subprocess.run([str(binary), 'artisan', 'queue:work', 'redis', '--stop-when-empty'], cwd=source, env=runtime, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=20)
    sock = socket.socket()
    sock.bind(('127.0.0.1', 0))
    runtime['APP_PORT'] = str(sock.getsockname()[1])
    runtime['APP_HOST'] = '127.0.0.1'
    sock.close()
    # Prove the promised developer entry rather than only launching the artifact directly.
    with (stage / 'server.log').open('w') as log:
        process = subprocess.Popen(['cargo', 'run', '--quiet', '--locked'], cwd=source, env=runtime, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            base = f'http://127.0.0.1:{runtime["APP_PORT"]}'
            for _ in range(120):
                if process.poll() is not None:
                    raise RuntimeError('cargo run exited before becoming ready; inspect server.log')
                try:
                    with urllib.request.urlopen(base + '/up', timeout=1) as response:
                        assert response.status == 200
                    break
                except (urllib.error.URLError, TimeoutError):
                    time.sleep(0.25)
            else:
                raise RuntimeError('HTTP startup timed out')
            request = urllib.request.Request(base + '/api/admin/user', headers={'Accept': 'application/json'})
            try:
                urllib.request.urlopen(request, timeout=5)
                raise AssertionError('protected user endpoint accepted an anonymous request')
            except urllib.error.HTTPError as error:
                assert error.code == 401, error.code
                assert isinstance(json.loads(error.read()), dict)
            authenticated=urllib.request.Request(base+'/api/admin/user',headers={'Accept':'application/json','Authorization':'Bearer '+token})
            with urllib.request.urlopen(authenticated,timeout=5) as response:
                assert response.status == 200
                assert json.loads(response.read())['id'] == 9871
            for browser_locale, resource_locale, heading in [
                ('en-US', 'en', 'Let’s get started'),
                ('zh-CN', 'zh-Hans', '开始使用'),
                ('zh-TW', 'zh-Hant', '開始使用'),
            ]:
                request = urllib.request.Request(base + '/', headers={'Accept-Language': browser_locale})
                with urllib.request.urlopen(request, timeout=5) as response:
                    body = response.read().decode()
                    assert response.status == 200
                    assert response.headers['Content-Language'] == resource_locale
                    assert f'lang="{resource_locale}"' in body and heading in body
                    assert f'/docs/{resource_locale}/README.md' in body
            assert (source / 'vendor/openrust/framework/src/localization/lang/zh-Hans/welcome.json').is_file()
            assert (source / 'vendor/openrust/framework/src/localization/lang/zh-Hant/welcome.json').is_file()
            assert not (source / 'lang/zh_CN').exists()
            assert not (source / 'lang/zh_TW').exists()
            if os.environ.get('OPENRUST_INIT_TEST_INTEGRATIONS'):
                key=next(line.split('=',1)[1] for line in reverb_credentials if line.startswith('REVERB_APP_KEY='))
                key=json.loads(key) if key.startswith('"') else key
                with socket.create_connection(('127.0.0.1', reverb_port), timeout=5) as ws:
                    ws.sendall((f'GET /app/{key}?protocol=7&client=init-acceptance&version=1 HTTP/1.1\r\nHost: 127.0.0.1:{reverb_port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: ' + base64.b64encode(b'init-acceptance!').decode() + '\r\n\r\n').encode())
                    received=ws.recv(8192)
                    assert b'101 Switching Protocols' in received, received[:100]
                    if b'pusher:connection_established' not in received:
                        received += ws.recv(8192)
                    assert b'pusher:connection_established' in received
                print('PASS: embedded Reverb WebSocket 101 / connection_established, generated credentials preserved; SMTP connection checks without sending mail')
            print('PASS: init -> migrations -> repeated init preserves key and migrations -> cargo run -> /up 200, / 200, /api/admin/user 401')
        finally:
            os.killpg(process.pid, signal.SIGINT)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)


def integrations_main():
    global main
    original=main
    if not os.environ.get('OPENRUST_INIT_TEST_INTEGRATIONS'):
        return
    class Smtp(socketserver.StreamRequestHandler):
        def handle(self):
            self.wfile.write(b'220 localhost ESMTP\r\n')
            while True:
                line=self.rfile.readline()
                if not line:
                    break
                verb=line.split(None,1)[0].upper()
                if verb in (b'MAIL',b'RCPT',b'DATA'):
                    self.server.deliveries += 1
                if verb in (b'EHLO',b'HELO'):
                    self.wfile.write(b'250-localhost\r\n250 HELP\r\n')
                elif verb==b'QUIT':
                    self.wfile.write(b'221 Bye\r\n')
                    break
                else:
                    if verb==b'NOOP': self.server.checks += 1
                    self.wfile.write(b'250 OK\r\n')
    class Server(socketserver.ThreadingTCPServer):
        allow_reuse_address=True
        daemon_threads=True
    def wrapped():
        with Server(('127.0.0.1',0),Smtp) as server:
            server.deliveries=0
            server.checks=0
            os.environ['OPENRUST_INIT_SMTP_PORT']=str(server.server_address[1])
            thread=threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                original()
                assert server.checks >= 1
                assert server.deliveries == 0
            finally:
                server.shutdown()
    main=wrapped


if __name__ == '__main__':
    integrations_main()
    if os.environ.get('OPENRUST_INIT_USE_REDIS'):
        name = f'openrust-init-redis-{os.getpid()}'
        image = subprocess.check_output(['docker', 'image', 'inspect', '--format', '{{.Id}}', 'redis:8'], text=True).strip()
        try:
            subprocess.run(['docker', 'run', '--rm', '--detach', '--name', name, '--tmpfs', '/data:rw,size=128m', '--publish', '127.0.0.1::6379', image], check=True, stdout=subprocess.DEVNULL)
            port = subprocess.check_output(['docker', 'port', name, '6379/tcp'], text=True).strip().rsplit(':',1)[1]
            os.environ['OPENRUST_INIT_REDIS_PORT'] = port
            main()
        finally:
            subprocess.run(['docker', 'container', 'rm', '--force', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    else:
        main()
