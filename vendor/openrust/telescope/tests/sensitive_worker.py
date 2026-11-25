"""用独立服务和真实Artisan Worker验证加密通知诊断边界，不连接开发数据库。"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('binary', type=Path)
    parser.add_argument('--backend', choices=['database', 'redis'], required=True)
    parser.add_argument('--mysql', action='store_true')
    args = parser.parse_args()
    project = Path(__file__).resolve().parents[4]
    binary = str(args.binary.resolve())
    work = project / 'target' / 'tmp' / 'telescope-sensitive-worker'
    work.mkdir(parents=True, exist_ok=True)
    run_id = secrets.token_hex(6)
    resources = []
    server = worker = None
    result = {'run': run_id, 'backend': args.backend, 'database': 'mysql' if args.mysql else 'sqlite'}
    def cancelled(*_):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, cancelled)
    def docker(*arguments):
        completed = subprocess.run(['docker', *arguments], capture_output=True, text=True, timeout=30)
        if completed.returncode:
            raise RuntimeError(f'Docker {arguments[0]} failed ({completed.returncode})')
        return completed.stdout.strip()
    def service(image, port, command=(), data=None):
        info, = json.loads(docker('image', 'inspect', image))
        volumes = set(info['Config'].get('Volumes') or {})
        assert volumes <= ({data} if data else set()), 'unexpected image volume'
        name = f'openrust-user-auth-{run_id}-{port}'
        resources.append(name)
        arguments = ['run', '--rm', '-d', '--name', name, '--label', f'org.openrust.acceptance.run={run_id}', '--label', 'openrust.workflow=user-starter', '-p', f'127.0.0.1::{port}']
        if data:
            arguments += ['--mount', f'type=tmpfs,destination={data}']
        if port == 1025:
            arguments += ['-p', '127.0.0.1::8025']
        docker(*arguments, info['Id'], *command)
        actual, = json.loads(docker('inspect', name))
        assert all(mount['Type'] != 'volume' for mount in actual['Mounts']), 'fixture must not use volumes'
        address = docker('port', name, str(port)).splitlines()[0]
        host, bound = address.rsplit(':', 1)
        assert host == '127.0.0.1' and bound.isdigit()
        result.setdefault('images', []).append({'image': info['Id'], 'name': name})
        return name, int(bound)
    def stop(child):
        if child is not None and child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=5)
    try:
        if args.mysql:
            assert os.environ.get('OPENRUST_REQUIRE_MYSQL') == '1'
            container = os.environ['OPENRUST_ACCEPTANCE_MYSQL_CONTAINER']
            info, = json.loads(docker('inspect', container))
            assert info['Config']['Labels'].get('org.openrust.acceptance.run'), 'requires owned MySQL fixture'
        spec = importlib.util.spec_from_file_location('smtp_fixture', project / 'tests' / 'smtp_acceptance.py')
        fixture = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(fixture)
        mailpit, smtp_port = service(fixture.IMAGE, 1025, data="/data")
        ui = 'http://' + docker('port', mailpit, '8025').splitlines()[0]
        redis_name = redis_url = None
        if args.backend == 'redis':
            redis_name, redis_port = service('redis:8', 6379, ('--save', '', '--appendonly', 'no'), '/data')
            redis_url = f'redis://127.0.0.1:{redis_port}/0'
            until = time.monotonic() + 20
            while docker('exec', redis_name, 'redis-cli', 'PING') != 'PONG':
                assert time.monotonic() < until
                time.sleep(.1)
            version = docker('exec', redis_name, 'redis-cli', 'INFO', 'server')
            version = next(line for line in version.splitlines() if line.startswith('redis_version:'))
            assert version.startswith('redis_version:8.')
            result['redis'] = version
        until = time.monotonic() + 20
        while True:
            try:
                json.load(urllib.request.urlopen(ui + '/api/v1/messages', timeout=2))
                break
            except OSError:
                assert time.monotonic() < until
                time.sleep(.1)
        with tempfile.TemporaryDirectory(prefix=f'{run_id}-', dir=work) as directory:
            root = Path(directory)
            (root / 'database.sqlite').touch()
            env = dict(os.environ, APP_BASE_PATH=str(root), LOG_CHANNEL='null', TMPDIR=str(root), USER_STARTER_DIAG_DATABASE='mysql' if args.mysql else 'sqlite', USER_STARTER_DIAG_QUEUE=args.backend, USER_STARTER_DIAG_SMTP_PORT=str(smtp_port))
            if redis_url:
                env['USER_STARTER_DIAG_REDIS_URL'] = redis_url
            subprocess.run([binary, 'artisan', 'migrate', '--force'], env=env, check=True, timeout=60)
            with socket.socket() as probe:
                probe.bind(('127.0.0.1', 0))
                port = probe.getsockname()[1]
            base = f'http://127.0.0.1:{port}'
            def request(path):
                try:
                    response = urllib.request.urlopen(base + path, timeout=10)
                except urllib.error.HTTPError as error:
                    response = error
                return response.status, response.read()
            def stats():
                output = subprocess.check_output([binary, 'artisan', 'probe:diagnostic-stats'], env=dict(env, TELESCOPE_ENABLED='false'), text=True, timeout=30)
                return json.loads(next(line for line in output.splitlines() if line.startswith('{')))
            with (work / f'{run_id}-server.log').open('wb') as output:
                server = subprocess.Popen([binary, 'artisan', 'serve', '--host=127.0.0.1', f'--port={port}', '--tries=0'], env=env, stdout=output, stderr=subprocess.STDOUT)
                until = time.monotonic() + 20
                while True:
                    try:
                        assert request('/missing')[0] == 404
                        break
                    except OSError:
                        assert server.poll() is None
                        assert time.monotonic() < until
                        time.sleep(.1)
                for kind in ['private', 'failure', 'control']:
                    status, body = request('/api/queue/' + kind)
                    assert status == 200 and json.loads(body)['queued'] is True, (kind, status)
                before = stats()
                if redis_name:
                    keys = docker('exec', redis_name, 'redis-cli', '--raw', '--scan', '--pattern', '*queues:user-starter-diagnostics').splitlines()
                    assert len(keys) == 1, keys
                    envelopes = [json.loads(line) for line in docker('exec', redis_name, 'redis-cli', '--raw', 'LRANGE', keys[0], '0', '-1').splitlines()]
                    payloads = [json.loads(envelope['payload']) for envelope in envelopes]
                else:
                    payloads = before['payloads']
                assert len(payloads) == 3
                summaries = []
                for payload in payloads:
                    encrypted = 'encrypted_delivery' in payload['data']
                    hidden = payload.get('context', {}).get('hidden', {}).get('openrust.diagnostics.sensitive') is True
                    assert hidden == encrypted, 'context must identify only encrypted notifications'
                    if encrypted:
                        assert 'PRIVATE-' not in json.dumps(payload), 'plaintext in encrypted payload'
                    summaries.append({'encrypted': encrypted, 'hidden': hidden})
                assert sum(item['encrypted'] for item in summaries) == 2
                result['payloads'] = summaries
                assert before['private_entries'] == 0, 'private data captured during dispatch'
                with (work / f'{run_id}-worker.log').open('wb') as log:
                    worker = subprocess.Popen([binary, 'artisan', 'queue:work', args.backend, '--queue=user-starter-diagnostics', '--tries=1', '--sleep=0', '--max-jobs=3', '--stop-when-empty'], env=env, stdout=log, stderr=subprocess.STDOUT)
                    assert worker.wait(timeout=90) == 0, 'Worker failed; inspect retained log'
                after = stats()
                until = time.monotonic() + 5
                while after['control_mail'] == 0 and time.monotonic() < until:
                    time.sleep(.05)
                    after = stats()
                assert after['private_entries'] == 0, 'private worker data entered diagnostics'
                assert after['control_mail'] == 1 and after['mail_entries'] == 1, 'next ordinary job must still record its mail'
                assert after['failed_jobs'] == 1 and after['jobs'] == 0, 'success, failure and settlement mismatch'
                inbox = json.load(urllib.request.urlopen(ui + '/api/v1/messages', timeout=5))
                assert inbox['total'] == 2, 'SMTP must accept private and control mail, not failed template'
                if redis_name:
                    assert docker('exec', redis_name, 'redis-cli', 'LLEN', keys[0]) == '0'
                result['after'] = {key: value for key, value in after.items() if key != 'payloads'}
                result['smtp_messages'] = inbox['total']
                result['worker_exit'] = 0
                print(f"PASS {args.backend}: real Worker, encrypted context, SMTP success/failure, no private diagnostics and next-job isolation", flush=True)
                stop(server)
    finally:
        stop(worker)
        stop(server)
        cleanup_errors = []
        for name in reversed(resources):
            response = subprocess.run(['docker', 'inspect', name], capture_output=True, text=True, timeout=15)
            if response.returncode:
                if 'No such object' not in response.stderr and 'No such container' not in response.stderr:
                    cleanup_errors.append(name)
                continue
            info, = json.loads(response.stdout)
            if info['Config']['Labels'].get('org.openrust.acceptance.run') != run_id:
                cleanup_errors.append(name)
                continue
            try:
                docker('rm', '-f', name)
            except RuntimeError:
                cleanup_errors.append(name)
        result['containers_removed'] = not cleanup_errors
        result['cleanup_unconfirmed'] = cleanup_errors
        (work / f'{run_id}-result.json').write_text(json.dumps(result, indent=2))
        if cleanup_errors:
            raise RuntimeError('Fixture cleanup could not be confirmed: ' + ', '.join(cleanup_errors))


if __name__ == '__main__':
    main()
