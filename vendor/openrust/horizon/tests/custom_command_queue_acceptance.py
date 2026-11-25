"""Run against dedicated Redis and a native Horizon application."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit
import uuid


RESTART = r'Laravel\Horizon\SupervisorCommands\Restart'
TERMINATE = r'Laravel\Horizon\SupervisorCommands\Terminate'


def run_case(binary, transport_type):
    redis_url = urlsplit(os.environ['HORIZON_TEST_REDIS_URL'])
    redis_container = os.environ.get('HORIZON_ACCEPTANCE_REDIS_CONTAINER')
    if redis_container:
        redis_command = ['docker', 'exec', '-e', 'REDISCLI_AUTH', redis_container, 'redis-cli']
    else:
        assert os.environ['HORIZON_ACCEPTANCE_REDIS_DIRECT'] == '1'
        redis_command = ['redis-cli', '-h', redis_url.hostname, '-p', str(redis_url.port)]

    def redis(*args):
        return subprocess.check_output([*redis_command, '-e', *args],
                                       env=dict(os.environ, REDISCLI_AUTH=redis_url.password),
                                       text=True, timeout=15).strip()

    with tempfile.TemporaryDirectory(prefix='horizon-custom-command-') as base:
        root = Path(base)
        (root / 'database.sqlite').touch()
        env = dict(os.environ, APP_BASE_PATH=base, APP_ENV='testing', LOG_CHANNEL='null',
                   HORIZON_PREFIX='custom-metadata:', HORIZON_PROBE_CUSTOM_COMMAND_QUEUE=transport_type,
                   HORIZON_PROBE_COMMAND_HANDLERS='1' if transport_type == 'file' else '0',
                   HORIZON_PROBE_PRELOAD_MASTER_COMMAND='1')
        processes = []
        stopped = None
        hold = root / 'hold-command-read'

        def run(*args, accepted=(0,), **variables):
            result = subprocess.run([binary, *args], env=dict(env, **variables), capture_output=True,
                                    text=True, timeout=15)
            assert result.returncode in accepted, (args, result)
            return result.stdout.strip()

        def start(*args):
            process = subprocess.Popen([binary, 'artisan', *args], env=env,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            processes.append(process)
            return process

        def queue(name, action, **data):
            return run(HORIZON_PROBE_COMMAND_QUEUE=json.dumps(dict(name=name, action=action,
                                                                    bound=True, **data)))

        def statuses():
            return json.loads(run('artisan', 'horizon:status', '--json', accepted=(0, 1, 2)))

        def until(predicate):
            deadline = time.monotonic() + 16
            while time.monotonic() < deadline:
                if result := predicate():
                    return result
                time.sleep(.15)
            raise AssertionError(statuses())

        run(HORIZON_PROBE_PREPARE_DATABASE='1')
        try:
            for command in ['first', 'second']:
                queue('public-api', 'push', command=command, options={'identifier': command})
            assert json.loads(queue('public-api', 'pending')) == [
                {'command': command, 'options': {'identifier': command}} for command in ['first', 'second']
            ]
            queue('public-api', 'push', command='discard', options=[])
            queue('public-api', 'flush')
            assert json.loads(queue('public-api', 'pending')) == []
            queue('standalone', 'push', command=TERMINATE, options={'status': 17})
            standalone = start('horizon:supervisor', 'standalone', 'redis', '--timeout=5')
            until(lambda: 'is running' in run('artisan', 'horizon:supervisor-status', 'standalone',
                                              accepted=(0, 1, 2)))
            assert standalone.poll() is None
            assert json.loads(queue('standalone', 'target'))['scope'] is None
            run(HORIZON_PROBE_WRAPPER_TERMINATE=json.dumps({'name': 'standalone', 'status': 23}))
            assert standalone.wait(timeout=15) == 23, standalone.stderr.read()

            master = start('horizon')
            ready = until(lambda: next((row for row in statuses() if row['workers']), None))
            name = ready['name']
            assert ready['supervisors'] == [name + ':supervisor-1'], ready
            child = ready['supervisors'][0]
            master_queue = 'master:' + name
            scope = json.loads(queue(master_queue, 'target'))['scope']
            assert uuid.UUID(scope) and json.loads(queue(child, 'target'))['scope'] == scope
            run(HORIZON_PROBE_DEPLOY_PLAN=json.dumps(dict(master=name, environment='public-plan')))
            ready = until(lambda: next((row for row in statuses()
                                        if any(worker['queue'] == 'planned' for worker in row['workers'])),
                                       None))
            assert [child, name + ':planned'] == ready['supervisors'], ready
            run('artisan', 'horizon:pause')
            until(lambda: any(row['status'] == 'paused' for row in statuses()))
            queue(master_queue, 'push', command=r'Probe\Restart' if transport_type == 'file' else RESTART,
                  options=[])
            old_pids = {item['pid'] for item in ready['workers']}
            until(lambda: next((row for row in statuses()
                                if row['status'] == 'running' and len(row['workers']) == 2
                                and old_pids.isdisjoint(item['pid'] for item in row['workers'])), None))
            dispatched = run(HORIZON_PROBE_DISPATCH='{}', HORIZON_PROBE_DISPATCH_UUID='1')
            log = root / 'completed.log'
            until(lambda: log.exists() and dispatched in log.read_text(encoding='utf-8'))

            # 独占 Redis 模拟同名接管。文件后端保留正在进行的领取。
            if transport_type == 'file':
                hold.touch()
                marker = root / 'command-read-scope'
                until(lambda: marker.exists() and marker.read_text() == scope)
                queue(master_queue, 'push', command=r'Probe\Deploy',
                      options={'name': name + ':must-not-run', 'queue': ['default']})
            else:
                os.kill(master.pid, signal.SIGSTOP)
                stopped = master.pid
            metadata = f'{root.name}:custom-metadata:{{horizon}}:'
            transport = f'{root.name}:probe-command-transport:{{horizon}}:'
            owner = metadata + 'master-owner:' + name
            route = metadata + 'command-route:' + master_queue
            replacement = str(uuid.uuid4())
            redis('EVAL', "redis.call('SET',KEYS[1],ARGV[1],'EX',30); "
                  "redis.call('HSET',KEYS[2],'owner',KEYS[1],'token',ARGV[1]); return 1",
                  '2', owner, route, replacement)
            assert json.loads(queue(master_queue, 'target'))['scope'] == replacement
            successor = {'command': 'Successor', 'options': ['retained']}
            queue(master_queue, 'push', **successor)
            key = transport + f'commands:scoped:{replacement}:{master_queue}'
            if transport_type == 'file':
                assert redis('LLEN', key) == '0'
                hold.unlink()
            else:
                assert redis('LLEN', key) == '1'
                os.kill(master.pid, signal.SIGCONT)
                stopped = None
            assert master.wait(timeout=15) != 0
            assert redis('GET', owner) == replacement
            assert json.loads(queue(master_queue, 'pending')) == [successor]
            assert not (root / 'command-handlers.jsonl').exists()
            print(f'PASS: replaceable {transport_type} queue, native deployment, restart and takeover fencing')
        finally:
            hold.unlink(missing_ok=True)
            if stopped is not None:
                os.kill(stopped, signal.SIGCONT)
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                process.stderr.close()


if __name__ == '__main__':
    assert os.environ['HORIZON_REQUIRE_REDIS'] == '1'
    for transport_type in ('1', 'file'):
        run_case(str(Path(sys.argv[1]).resolve()), transport_type)
