"""Run native Unix Horizon against two dedicated Redis instances and promote the replica."""
import contextlib
import json
import os
from pathlib import Path
import secrets
import select
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import uuid


class Endpoint(socketserver.ThreadingTCPServer):
    daemon_threads = True
    block_on_close = False

    def __init__(self, target):
        super().__init__(('127.0.0.1', 0), Forward)
        self.target = target
        self.pairs = set()
        self.lock = threading.Lock()

    def change(self, target):
        with self.lock:
            self.target = target
            for pair in self.pairs.copy():
                for connection in pair:
                    with contextlib.suppress(OSError):
                        connection.shutdown(socket.SHUT_RDWR)


class Forward(socketserver.BaseRequestHandler):
    def handle(self):
        try:
            with self.server.lock:
                target = self.server.target
                if target is None:
                    return
                upstream = socket.create_connection(target, timeout=1)
                upstream.settimeout(None)
                pair = (self.request, upstream)
                self.server.pairs.add(pair)
            with upstream:
                try:
                    while True:
                        readable, _, _ = select.select(pair, (), ())
                        for source in readable:
                            data = source.recv(65536)
                            if not data:
                                return
                            destination = pair[0] if source is upstream else pair[1]
                            destination.sendall(data)
                finally:
                    with self.server.lock:
                        self.server.pairs.discard(pair)
        except OSError:
            return


def until(test, *, seconds=25, label='condition'):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if outcome := test():
            return outcome
        time.sleep(0.10)
    raise RuntimeError(f'timed out waiting for {label}')


def docker(*args, env=None, check=True, input=None):
    result = subprocess.run(['docker', *args], env=env, input=input, text=True, capture_output=True, timeout=25)
    if check and result.returncode:
        raise RuntimeError(f'dedicated Docker fixture failed during {args[0]}')
    return result.stdout.strip()


def exercise(binary, scenario):
    batch = secrets.token_hex(8)
    network = f'openrust-horizon-promotion-{batch}'
    leader = f'{network}-primary'
    follower = f'{network}-replica'
    password = secrets.token_hex(24)
    redis_env = dict(os.environ, REDISCLI_AUTH=password)
    image = docker('image', 'inspect', 'redis:8', '--format', '{{.Id}}')

    def redis(name, *args, input=None):
        return docker('exec', '-i', '-e', 'REDISCLI_AUTH', name, 'redis-cli', '--raw', *args, env=redis_env, input=input)

    def address(name):
        return ('127.0.0.1', int(docker('port', name, '6379/tcp').rsplit(':', 1)[1]))

    with contextlib.ExitStack() as cleanup:
        docker('network', 'create', '--label', f'org.openrust.acceptance.run={batch}', network)
        cleanup.callback(docker, 'network', 'rm', network, check=False)
        for name, alias in ((leader, 'fixture-primary'), (follower, 'fixture-replica')):
            docker('run', '--detach', '--name', name, '--network', network, '--network-alias', alias,
                   '--publish', '127.0.0.1::6379', '--tmpfs', '/data:rw,size=64m',
                   '--label', f'org.openrust.acceptance.run={batch}', image, 'redis-server',
                   '--save', '', '--appendonly', 'no', '--requirepass', password, '--masterauth', password)
            cleanup.callback(docker, 'rm', '-f', name, check=False)
        assert redis(follower, 'REPLICAOF', 'fixture-primary', '6379') == 'OK'
        until(lambda: 'master_link_status:up' in redis(follower, 'INFO', 'replication'), label='live Redis replication')
        version = next(line for line in redis(leader, 'INFO', 'server').splitlines() if line.startswith('redis_version:'))
        print(version, flush=True)

        proxy = cleanup.enter_context(Endpoint(address(leader)))
        thread = threading.Thread(target=proxy.serve_forever, daemon=True)
        thread.start()
        cleanup.callback(proxy.shutdown)
        cleanup.callback(proxy.change, None)
        root = Path(cleanup.enter_context(tempfile.TemporaryDirectory(prefix='horizon-promotion-')))
        (root / 'database.sqlite').touch()
        ready = f'{root.name}:queues:default'
        env = dict(os.environ, APP_BASE_PATH=str(root), APP_ENV='testing', LOG_CHANNEL='null',
                   HORIZON_PREFIX='horizon-promotion:', HORIZON_PROBE_MASTER_BASENAME='Promotion',
                   HORIZON_REQUIRE_REDIS='1', HORIZON_PROBE_BLOCK_FOR='5',
                   HORIZON_PROBE_FAST_TERMINATION='1' if scenario == 'prolonged' else '0',
                   HORIZON_TEST_REDIS_URL=f'redis://:{password}@127.0.0.1:{proxy.server_address[1]}/0')

        def run(*args, accepted=(0,), **variables):
            result = subprocess.run([binary, *args], env=dict(env, **variables), text=True,
                                    capture_output=True, timeout=12)
            if result.returncode not in accepted:
                raise RuntimeError(f'fixture command {args} exited {result.returncode}')
            return result.stdout.strip()

        def statuses():
            return json.loads(run('artisan', 'horizon:status', '--json', accepted=(0, 1, 2)))

        def dispatch(**variables):
            return run(HORIZON_PROBE_DISPATCH='{"record_start":true}',
                       HORIZON_PROBE_DISPATCH_UUID='1', **variables)

        def direct_dispatch(node):
            return dispatch(HORIZON_TEST_REDIS_URL=f'redis://:{password}@127.0.0.1:{address(node)[1]}/0')

        def completed(job):
            path = root / 'completed.log'
            return path.exists() and path.read_text().splitlines().count(job) == 1

        def remains_pending(node, job):
            payload = json.loads(redis(node, 'LINDEX', ready, '0'))
            assert json.loads(payload['payload'])['uuid'] == job
            assert payload['attempts'] == 0
            assert redis(node, 'ZCARD', f'{ready}:reserved') == '0'
            assert job not in (root / 'started.log').read_text().splitlines()

        run(HORIZON_PROBE_PREPARE_DATABASE='1')
        master = subprocess.Popen([binary, 'artisan', 'horizon'], env=env,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        def stop_master():
            if master.poll() is None:
                master.terminate()
                try:
                    master.wait(timeout=12)
                except subprocess.TimeoutExpired:
                    master.kill()
                    master.wait()
        cleanup.callback(stop_master)

        previous = until(lambda: next((row for row in statuses()
                                      if row['pid'] == master.pid and row['workers']), None), label='live Horizon process tree')
        first = dispatch()
        until(lambda: completed(first), label='queue work before promotion')
        reply = redis(leader, input=f'SET promotion-fence:{batch} ready\nWAIT 1 6000\n')
        assert reply.splitlines() == ['OK', '1'], reply
        assert redis(follower, 'GET', f'promotion-fence:{batch}') == 'ready'
        owner_key, = redis(follower, '--scan', '--pattern', '*master-owner:*').splitlines()
        prior_token = redis(leader, 'GET', owner_key)
        assert prior_token and redis(follower, 'GET', owner_key) == prior_token
        proxy.change(None)

        def worker_exited():
            for worker in previous['workers']:
                try:
                    os.kill(worker['pid'], 0)
                except ProcessLookupError:
                    continue
                return False
            return True

        if scenario == 'prolonged':
            pending = direct_dispatch(leader)
            assert master.wait(timeout=25) != 0
            until(worker_exited, label='workers reaped after sustained metadata outage')
            remains_pending(leader, pending)
            proxy.change(address(leader))
            run('artisan', 'queue:work', 'redis', '--once', '--force')
            assert completed(pending)
            print('PASS: sustained metadata loss reaps the fast-termination tree and preserves pending work')
            return

        assert redis(follower, 'REPLICAOF', 'NO', 'ONE') == 'OK'
        replacement = None
        if scenario == 'missing-owner':
            assert redis(follower, 'DEL', owner_key) == '1'
        if scenario == 'replacement-owner':
            replacement = str(uuid.uuid4())
            assert redis(follower, 'SET', owner_key, replacement, 'EX', '30') == 'OK'
        pending = direct_dispatch(follower)
        # Existing clients lose their connections and new clients cannot connect during promotion.
        time.sleep(3)
        assert master.poll() is None, f'Master exited during short interruption: {master.returncode}'
        remains_pending(follower, pending)
        docker('stop', '--time', '1', leader)
        proxy.change(address(follower))
        assert redis(follower, 'ROLE').splitlines()[0] == 'master'

        if scenario != 'short':
            assert master.wait(timeout=15) != 0
            until(worker_exited, label='workers reaped after ownership loss')
            remains_pending(follower, pending)
            assert redis(follower, 'GET', owner_key) == (replacement or '')
            run('artisan', 'queue:work', 'redis', '--once', '--force')
            assert completed(pending)
            print(f'PASS: {scenario} rejects previous ownership and preserves pending work')
            return

        recovered = until(lambda: next((row for row in statuses()
                                       if row['pid'] == master.pid and row['workers']), None),
                          label='Horizon metadata on promoted Redis')
        assert recovered['name'] == previous['name']
        until(lambda: completed(pending), label='queue work after promotion')
        assert completed(first)
        run('artisan', 'horizon:terminate')
        assert master.wait(timeout=15) == 0
        assert statuses() == []
        print('PASS: same Master resumes on promoted Redis and both jobs finish once')


def main():
    if os.name != 'posix':
        raise SystemExit('This process-reaping fixture requires POSIX process probes.')
    binary = str(Path(sys.argv[1]).resolve())
    for scenario in sys.argv[2:] or ('short', 'missing-owner', 'replacement-owner', 'prolonged'):
        assert scenario in ('short', 'missing-owner', 'replacement-owner', 'prolonged')
        exercise(binary, scenario)


if __name__ == '__main__':
    main()
