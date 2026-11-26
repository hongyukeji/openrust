"""Run through redis_acceptance.py; exercises native supervisor processes and HTTP."""
import json
import os
from pathlib import Path
import re
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
from urllib.parse import urlsplit
import urllib.request


def main():
    assert os.environ['HORIZON_REQUIRE_REDIS'] == '1'
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix='openrust-horizon-process-') as base:
        root = Path(base)
        with sqlite3.connect(root / 'database.sqlite') as db:
            db.execute('CREATE TABLE failed_jobs (id INTEGER PRIMARY KEY AUTOINCREMENT, uuid TEXT UNIQUE, connection TEXT, queue TEXT, payload TEXT, exception TEXT, failed_at TEXT)')
        env = dict(os.environ, APP_BASE_PATH=base, APP_ENV='testing', LOG_CHANNEL='null',
                   HORIZON_PREFIX='horizon-probe:', HORIZON_PROBE_ASSERT_QUEUED_STARTUP='1')
        redis_url = urlsplit(env['HORIZON_TEST_REDIS_URL'])
        redis_auth = dict(os.environ, REDISCLI_AUTH=redis_url.password)
        redis_container = env.get('HORIZON_ACCEPTANCE_REDIS_CONTAINER')
        if redis_container:
            redis_command = ['docker', 'exec', '-e', 'REDISCLI_AUTH', redis_container, 'redis-cli']
        else:
            # Linux 容器通过验收专用网络直接访问独占 Redis，不挂载 Docker 套接字。
            assert env['HORIZON_ACCEPTANCE_REDIS_DIRECT'] == '1'
            redis_command = ['redis-cli', '-h', redis_url.hostname, '-p', str(redis_url.port)]
        wait_lock = f'{root.name}:horizon-probe:{{horizon}}:wait_time_monitor'
        def redis(*args):
            return subprocess.check_output(
                [*redis_command, *args],
                env=redis_auth, text=True, timeout=10,
            ).strip()
        def artisan(*args, code=0):
            result = subprocess.run([binary, 'artisan', *args], env=env, capture_output=True, text=True, timeout=30)
            assert result.returncode == code, (args, result.stdout, result.stderr)
            return result.stdout
        def dispatch(payload_uuid=False, destination=None, **data):
            dispatch_env = dict(env, HORIZON_PROBE_DISPATCH=json.dumps(data), HORIZON_PROBE_DISPATCH_UUID="1" if payload_uuid else "0")
            if destination is not None:
                dispatch_env['HORIZON_PROBE_DISPATCH_QUEUE'] = destination
            result = subprocess.run([binary], env=dispatch_env, capture_output=True, text=True, timeout=15)
            assert result.returncode == 0, result.stderr
            return result.stdout.strip()
        def status():
            result = subprocess.run([binary, 'artisan', 'horizon:status', '--json'], env=env, capture_output=True, text=True, timeout=15)
            assert result.returncode in (0, 1, 2), result.stderr
            return json.loads(result.stdout)
        def until(check, timeout=25):
            end = time.monotonic() + timeout
            while time.monotonic() < end:
                result = check()
                if result:
                    return result
                time.sleep(.2)
            raise AssertionError('acceptance condition timed out: ' + repr({'status': status(), 'stats': stats()}))
        def until_master(predicate):
            def read():
                masters = status()
                if masters and predicate(masters[0]):
                    return masters[0]
            return until(read)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        server = subprocess.Popen([binary, 'artisan', 'serve', '--host=127.0.0.1', f'--port={port}', '--tries=0'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        master = None
        child_pids = set()
        try:
            path = env.get('HORIZON_PATH', 'horizon').strip('/')
            endpoint = f'http://127.0.0.1:{port}/{path}'
            def request(path='', allowed=True):
                headers = {'x-test-operator': 'operator'} if allowed else {}
                try:
                    return urllib.request.urlopen(urllib.request.Request(endpoint + path, headers=headers), timeout=3)
                except urllib.error.HTTPError as error:
                    return error
            def available():
                try:
                    return request().status == 200
                except OSError:
                    return False
            until(available)
            assert request(allowed=False).status == 403
            assert request('/api/stats', False).status == 403
            response = request()
            assert response.headers['cache-control'] == 'no-store'
            assert "frame-ancestors 'none'" in response.headers['content-security-policy']
            def stats():
                with request('/api/stats') as response:
                    assert response.status == 200, (response.status, response.read())
                    return json.load(response)
            assert stats()['masters'] == []
            artisan('horizon:status', code=2)
            deployment_events = root / 'deployment-events.jsonl'
            def deployments():
                if not deployment_events.exists():
                    return []
                return [json.loads(line) for line in deployment_events.read_text().splitlines()]
            for environment in ('unmatched', 'empty', 'disabled'):
                environment_env = dict(env, HORIZON_PROBE_EMPTY_DEFAULTS='1') if environment == 'empty' else env
                master = subprocess.Popen(
                    [binary, 'artisan', 'horizon', f'--environment={environment}'],
                    env=environment_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                )
                state = until_master(lambda state: state['environment'] == environment)
                assert state['supervisors'] == []
                assert re.fullmatch(re.escape(state['basename']) + '-[A-Za-z0-9]{4}', state['name']), state
                expected = [] if environment == 'unmatched' else [{
                    'name': 'MasterSupervisorDeployed',
                    'master': state['name'],
                    'environment': environment,
                    'supervisors': [],
                }]
                if expected:
                    until(lambda: [event for event in deployments() if event['master'] == state['name']] == expected)
                master.send_signal(signal.SIGTERM)
                stdout, stderr = master.communicate(timeout=15)
                assert master.returncode == 0, (stdout, stderr)
                assert [event for event in deployments() if event['master'] == state['name']] == expected
                assert status() == []
            basename = os.environ.get('HORIZON_TEST_MASTER_BASENAME', '-probe.cluster_one')
            env['HORIZON_PROBE_MASTER_BASENAME'] = basename
            assert 'No processes to pause.' in artisan('horizon:pause')
            assert redis('SET', wait_lock, 'contender', 'EX', '30') == 'OK'
            master = subprocess.Popen([binary, 'artisan', 'horizon'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            until_master(lambda state: state['pid'] == master.pid and state['workers'])
            assert 'Sending USR2 signal to processes.' in artisan('horizon:pause')
            until_master(lambda state: state['status'] == 'paused')
            dispatch(password='secret-never-exported')
            time.sleep(1.2)
            assert redis('GET', wait_lock) == 'contender'
            assert redis('DEL', wait_lock) == '1'
            # A lost Redis lock does not start the local cooldown.
            until(lambda: redis('GET', wait_lock) == '1', timeout=8)
            assert redis('DEL', wait_lock) == '1'
            # After a win the same Supervisor must neither reacquire an early removal nor extend the interval.
            time.sleep(2.2)
            assert redis('EXISTS', wait_lock) == '0'
            assert stats()['queues'][0]['backlog']['pending'] == 1
            artisan('horizon:continue')
            until(lambda: stats()['queues'][0]['metrics']['completed'] == 1)
            if os.environ.get('HORIZON_TEST_BROWSER') == '1':
                subprocess.run(['node', str(Path(__file__).with_name('dashboard_browser.mjs')), endpoint], env=env, check=True, timeout=35)
            initial = until_master(lambda state: state['workers'])
            assert initial['basename'] == basename, initial
            assert re.fullmatch(re.escape(basename) + '-[A-Za-z0-9]{4}', initial['name']), initial
            child_pids.update(w['pid'] for w in initial['workers'])
            worker_pid = initial['workers'][0]['pid']
            command = subprocess.check_output(['ps', '-p', str(worker_pid), '-o', 'command='], text=True)
            for argument in ['artisan horizon:work', '--supervisor=' + initial['name'] + ':supervisor-1', '--name=horizon-probe', '--tries=2', '--backoff=1,2', '--rest=1', '--force', '--json']:
                assert argument in command, (argument, command)
            assert int(subprocess.check_output(['ps', '-p', str(worker_pid), '-o', 'ni='], text=True).strip()) >= 1
            supervisor_name = initial['name'] + ':supervisor-1'
            prefix = f'{root.name}:horizon-probe:{{horizon}}:'
            child = json.loads(redis('GET', prefix + 'supervisor:' + supervisor_name.encode().hex()))
            assert child['master'] == initial['name'] and child['basename'] == basename, child
            assert initial['targets']['supervisor-1'] == [1], initial
            assert stats()['masters'][0]['name'] == initial['name']
            with request('/api/masters') as response:
                assert response.status == 200 and json.load(response)[0]['name'] == initial['name']
            # 上游 ListCommand 的表格从第一个冒号后开始显示，Redis 仍保存完整名称。
            assert f"{initial['name']}\t{master.pid}\t{supervisor_name.split(':', 1)[1]}\t" in artisan('horizon:list')
            assert 'is running' in artisan('horizon:supervisor-status', supervisor_name)
            artisan('horizon:purge')
            assert [event for event in deployments() if event['master'] == initial['name']] == [{
                'name': 'MasterSupervisorDeployed',
                'master': initial['name'],
                'environment': 'testing',
                'supervisors': [supervisor_name],
            }]
            supervisor_pid = int(subprocess.check_output(['ps', '-p', str(worker_pid), '-o', 'ppid='], text=True))
            assert int(subprocess.check_output(['ps', '-p', str(supervisor_pid), '-o', 'ppid='], text=True)) == master.pid
            assert 'artisan horizon:supervisor ' + supervisor_name in subprocess.check_output(['ps', '-p', str(supervisor_pid), '-o', 'command='], text=True)
            artisan('horizon:pause-supervisor', supervisor_name)
            until(lambda: 'paused' in artisan('horizon:supervisor-status', supervisor_name))
            artisan('horizon:continue-supervisor', supervisor_name)
            until(lambda: 'running' in artisan('horizon:supervisor-status', supervisor_name))
            assert initial['name'] in artisan('horizon:list')
            assert supervisor_name in artisan('horizon:supervisors')
            restart_events = root / 'restart-events.jsonl'
            assert not restart_events.exists(), 'initial launch is not a restart'
            def restart_history(supervisor, pid):
                if not restart_events.exists():
                    return []
                events = [json.loads(line) for line in restart_events.read_text().splitlines()]
                return [event for event in events if event['supervisor'] == supervisor and event['pid'] == pid]
            def expected_restart(supervisor, pid, code, *, expired):
                events = ['WorkerProcessRestarting']
                if expired:
                    events.append('UnableToLaunchProcess')
                return [
                    {'name': name, 'supervisor': supervisor, 'queue': 'default', 'pid': pid, 'exit_code': code}
                    for name in events
                ]
            # Unexpected termination is detected, reaped and replaced.
            killed = initial['workers'][0]['pid']
            os.kill(killed, signal.SIGKILL)
            recovered = until_master(lambda state: (
                state['unexpected_exits'] >= 1 and state['workers']
                and all(worker['pid'] != killed for worker in state['workers'])
            ))
            child_pids.update(w['pid'] for w in recovered['workers'])
            assert restart_history(supervisor_name, killed) == expected_restart(
                supervisor_name, killed, 128 + signal.SIGKILL, expired=True,
            )
            # Backlog grows the pool within its cap; maxJobs also recycles workers.
            dispatch(sleep_ms=1500, probe_output='recovered-worker')
            for _ in range(3):
                dispatch(sleep_ms=1500)
            scaled = until_master(lambda state: len(state['workers']) == 2)
            child_pids.update(w['pid'] for w in scaled['workers'])
            until(lambda: stats()['queues'][0]['metrics']['completed'] == 5)
            assert all(len(m['workers']) <= 2 for m in status())
            # A dead supervisor is rebuilt independently; its old workers drain
            # after noticing that their parent disappeared.
            observed = status()[0]
            old_workers = [worker['pid'] for worker in observed['workers']]
            # A maxJobs worker can exit between its heartbeat and ps. The
            # supervisor PID was already verified against the real process tree.
            old_supervisor = supervisor_pid
            os.kill(old_supervisor, signal.SIGKILL)
            rebuilt = until_master(lambda state: (
                state['unexpected_exits'] >= 2 and state['workers']
                and all(worker['pid'] not in old_workers for worker in state['workers'])
            ))
            child_pids.update(worker['pid'] for worker in rebuilt['workers'])
            def gone(pid):
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    return True
                return False
            until(lambda: all(gone(pid) for pid in old_workers))
            # Native failed provider is the source for retry / forget.
            failed_id = dispatch(fail=True, token='private-token')
            until(lambda: stats()['queues'][0]['metrics']['failed'] == 1)
            assert next(j for j in stats()['recent'] if j['id'] == failed_id)['attempts'] == 2
            assert failed_id in artisan('horizon:failed')
            artisan('horizon:retry', failed_id)
            until(lambda: stats()['queues'][0]['metrics']['failed'] == 2)
            retried_id = next(j['id'] for j in stats()['recent'] if j['status'] == 'failed' and j['id'] != failed_id)
            artisan('horizon:forget', retried_id)
            artisan('horizon:forget', retried_id, code=1)
            text = json.dumps(stats())
            assert 'secret-never-exported' not in text and 'private-token' not in text
            # Live reservations stop and resume without dropping pending work.
            artisan('horizon:pause')
            until_master(lambda state: state['status'] == 'paused')
            time.sleep(1.2)
            dispatch()
            before = stats()['queues'][0]['metrics']['completed']
            time.sleep(1.5)
            assert stats()['queues'][0]['metrics']['completed'] == before
            artisan('horizon:continue')
            until(lambda: stats()['queues'][0]['metrics']['completed'] == before + 1)
            # SIGTERM drains an active job and removes the master heartbeat.
            dispatch(sleep_ms=2000)
            until(lambda: any(j['status'] == 'processing' for j in stats()['recent']))
            current = status()[0]
            child_pids.update(w['pid'] for w in current['workers'])
            master.send_signal(signal.SIGTERM)
            stdout, stderr = master.communicate(timeout=15)
            assert master.returncode == 0, (stdout, stderr)
            assert stdout.count('worker-out-東京-recovered-worker'.encode()) == 1, stdout
            assert stdout.count(b'worker-err-recovered-worker') == 1, stdout
            assert status() == []
            until(lambda: stats()['queues'][0]['metrics']['completed'] == before + 2)
            for pid in child_pids:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    continue
                raise AssertionError(f'orphan worker: {pid}')
            # A new master survives previous termination generations until a fresh request.
            # The configured --force worker must also consume during maintenance.
            artisan('down')
            forced_id = dispatch(payload_uuid=True)
            master = subprocess.Popen([binary, 'artisan', 'horizon'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            until_master(lambda state: state['workers'])
            # HTTP correctly returns 503 during maintenance. Observe native
            # completion directly, then inspect Horizon after leaving maintenance.
            until(lambda: (root / 'completed.log').exists() and forced_id in (root / 'completed.log').read_text())
            artisan('up')
            until(lambda: stats()['queues'][0]['metrics']['completed'] == before + 3)
            # A heartbeat outside this master group can even name a valid local PID.
            peers = [('remote-master-probe', 'other-' + socket.gethostname(), basename),
                     ('another-local-group', socket.gethostname(), basename + '_other')]
            peer = subprocess.Popen(['sleep', '60'])
            try:
                for name, host, basename in peers:
                    heartbeat = json.dumps({'name': name, 'pid': peer.pid, 'host': host, 'basename': basename, 'status': 'running'})
                    assert redis('SET', prefix + 'master:' + name, heartbeat, 'EX', '15') == 'OK'
                    redis('ZADD', prefix + 'masters', str(int(time.time() * 1000)), name)
                for action, expected in [('pause', 'paused'), ('continue', 'running')]:
                    response = artisan('horizon:' + action)
                    assert f'Process: {master.pid}' in response and f'Process: {peer.pid}' not in response, response
                    until(lambda: any(row['pid'] == master.pid and row['status'] == expected for row in status()))
                    assert peer.poll() is None
                    assert redis('EXISTS', prefix + 'paused') == '0'
                response = artisan('horizon:terminate')
                assert f'Process: {master.pid}' in response and f'Process: {peer.pid}' not in response, response
                stdout, stderr = master.communicate(timeout=15)
                assert master.returncode == 0, (stdout, stderr)
                assert peer.poll() is None
                assert redis('EXISTS', prefix + 'terminate') == '0'
            finally:
                redis('ZREM', prefix + 'masters', *(name for name, _, _ in peers))
                redis('DEL', *(prefix + 'master:' + name for name, _, _ in peers))
                peer.terminate()
                peer.wait(timeout=5)
            assert status() == []
            # Fast termination releases the master while jobs finish; --wait
            # explicitly restores the draining guarantee.
            env['HORIZON_PROBE_FAST_TERMINATION'] = '1'
            env['HORIZON_PROBE_WORKER_TIMEOUT'] = '22'
            for wait in [False, True]:
                long_id = dispatch(sleep_ms=12000 if wait else 3000)
                master = subprocess.Popen([binary, 'artisan', 'horizon'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                until(lambda: any(job['id'] == long_id and job['status'] == 'processing' for job in stats()['recent']))
                draining = status()[0]
                draining_workers = [worker['pid'] for worker in draining['workers']]
                owner_key = prefix + 'master-owner:' + draining['name']
                owner = redis('GET', owner_key)
                assert owner
                started = time.monotonic()
                artisan('horizon:terminate', *(['--wait'] if wait else []))
                if wait:
                    assert redis('PEXPIRE', owner_key, '5000') == '1'
                    until(lambda: int(redis('PTTL', owner_key)) > 7500, timeout=7)
                    assert redis('GET', owner_key) == owner and master.poll() is None
                stdout, stderr = master.communicate(timeout=18)
                elapsed = time.monotonic() - started
                assert master.returncode == 0, (stdout, stderr)
                assert redis('EXISTS', owner_key) == '0'
                if wait:
                    assert any(job['id'] == long_id and job['status'] == 'completed' for job in stats()['recent'])
                else:
                    assert elapsed < 2.5, elapsed
                    until(lambda: any(job['id'] == long_id and job['status'] == 'completed' for job in stats()['recent']))
                until(lambda: all(gone(pid) for pid in draining_workers))
                assert status() == []
            env.pop('HORIZON_PROBE_FAST_TERMINATION')
            env.pop('HORIZON_PROBE_WORKER_TIMEOUT')
            # Both supervisor and master memory limits stop with the upstream
            # exit code and leave no live process registrations.
            for fail_loop in [False, True]:
                if fail_loop:
                    env['HORIZON_PROBE_FAIL_LOOP_DISPATCH'] = '1'
                artisan('horizon:supervisor', 'memory-probe', 'redis', '--memory=1', code=12)
                env['HORIZON_PROBE_MEMORY_LIMIT'] = '1'
                artisan('horizon', code=12)
                env.pop('HORIZON_PROBE_MEMORY_LIMIT')
            env.pop('HORIZON_PROBE_FAIL_LOOP_DISPATCH')
            assert status() == []
            memory_events = [json.loads(line) for line in (root / 'memory-events.jsonl').read_text().splitlines()]
            assert [event['event'] for event in memory_events] == ['SupervisorOutOfMemory', 'MasterSupervisorOutOfMemory'] * 2
            assert all(event['name'] == 'memory-probe' for event in memory_events[::2])
            assert all(event['name'] and event['usage'] > event['limit'] == 1 for event in memory_events)
            assert {'SupervisorLooped', 'MasterSupervisorLooped'} <= set((root / 'loop-errors.log').read_text().splitlines())
            # File watching is live, responds to a change once, and cleans up
            # its child tree when interrupted.
            watched = root / 'routes' / 'web.rs'
            watched.parent.mkdir()
            watched.write_text('// first version\n')
            listen_log = root / 'listen.log'
            with listen_log.open('wb') as output:
                master = subprocess.Popen([binary, 'artisan', 'horizon:listen', '--poll'], env=env, stdout=output, stderr=subprocess.PIPE)
                watched_master = until_master(lambda state: state['workers'])['name']
                dispatch(probe_output='listen-before')
                until(lambda: b'worker-err-listen-before' in listen_log.read_bytes())
                watched.write_text('// changed version\n')
                until_master(lambda state: state['name'] != watched_master and state['workers'])
                dispatch(probe_output='listen-after')
                until(lambda: b'worker-err-listen-after' in listen_log.read_bytes())
                master.send_signal(signal.SIGTERM)
                _, stderr = master.communicate(timeout=20)
                assert master.returncode == 0, (listen_log.read_bytes(), stderr)
            assert status() == []
            # Exercise the remaining native management entry points against the
            # same isolated metadata and queue transport, after process assertions.
            assert '5 seconds' in artisan('horizon:timeout', 'testing')
            artisan('horizon:snapshot')
            artisan('horizon:snapshot')  # Snapshot lock makes a concurrent schedule safe.
            artisan('horizon:forget', '--all')
            assert artisan('horizon:failed').strip() == '[]'
            artisan('horizon:clear-metrics')
            assert stats()['queues'][0]['metrics']['attempts'] == 0
            dispatch()
            assert 'Cleared 1 jobs' in artisan('horizon:clear', 'redis', '--queue=default', '--force')
            assert stats()['queues'][0]['backlog']['pending'] == 0
            artisan('horizon:purge')  # No orphaned processes remain after clean shutdown.
            artisan('horizon:purge', '--signal=invalid', code=1)
            # Each logical slot retains its cooldown across successive PIDs.
            master = subprocess.Popen(
                [binary, 'artisan', 'horizon', '--environment=cooldown'],
                env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            first = until_master(lambda state: state['workers'])
            worker = first['workers'][0]
            supervisor_name, first_pid = worker['supervisor'], worker['pid']
            time.sleep(1.2)
            cooldown_started_after = time.monotonic()
            os.kill(first_pid, signal.SIGKILL)
            second = until_master(lambda state: state['workers'] and state['workers'][0]['pid'] != first_pid)
            second_pid = second['workers'][0]['pid']
            assert restart_history(supervisor_name, first_pid) == expected_restart(
                supervisor_name, first_pid, 128 + signal.SIGKILL, expired=True,
            )
            for _ in range(3):
                dispatch()
            third = until_master(lambda state: state['workers'] and state['workers'][0]['pid'] != second_pid)
            assert restart_history(supervisor_name, second_pid) == expected_restart(
                supervisor_name, second_pid, 0, expired=False,
            )
            third_pid = third['workers'][0]['pid']
            os.kill(third_pid, signal.SIGKILL)
            until_master(lambda state: state['unexpected_exits'] == 2 and not state['workers'])
            time.sleep(2.2)
            assert status()[0]['workers'] == []
            assert restart_history(supervisor_name, third_pid) == []
            assert all(gone(pid) for pid in (first_pid, second_pid, third_pid))
            def replacement():
                state = status()[0]
                return state['workers'] and state['workers'][0]['pid']
            fourth_pid = until(replacement, timeout=65)
            assert time.monotonic() - cooldown_started_after >= 60
            assert fourth_pid != third_pid
            assert restart_history(supervisor_name, third_pid) == expected_restart(
                supervisor_name, third_pid, 128 + signal.SIGKILL, expired=True,
            )
            master.send_signal(signal.SIGTERM)
            stdout, stderr = master.communicate(timeout=15)
            assert master.returncode == 0, (stdout, stderr)
            assert status() == []
            assert gone(fourth_pid)
            # The provisioning queue passes the working directory to the Supervisor itself.
            master = subprocess.Popen(
                [binary, 'artisan', 'horizon', '--environment=spawn'],
                env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            state = until_master(lambda state: state['supervisors'])
            supervisor_name = state['supervisors'][0]
            failed = until_master(lambda state: state['unexpected_exits'] > 0)
            assert not failed['workers']
            assert restart_history(supervisor_name, None) == []
            (root / 'worker-cwd').mkdir()
            restored_pid = until(replacement)
            assert 'is running' in artisan('horizon:supervisor-status', supervisor_name)
            assert status()[0]['supervisors'] == [supervisor_name]
            master.send_signal(signal.SIGTERM)
            stdout, stderr = master.communicate(timeout=15)
            assert master.returncode == 0, (stdout, stderr)
            assert status() == []
            assert gone(restored_pid)
            # The Master creates a grouped Supervisor and Worker with literal queue names.
            names = ['出库 汇总:报表', '  打包复核']
            ids = {dispatch(destination=queue_name) for queue_name in names}
            master = subprocess.Popen(
                [binary, 'artisan', 'horizon', '--environment=queue-names'],
                env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            ready = until_master(lambda state: state['workers'])
            assert ready['workers'][0]['queue'] == ','.join(names)
            until(lambda: ids <= {job['id'] for job in stats()['recent'] if job['status'] == 'completed'})
            with request('/api/workload') as response:
                assert response.status == 200
                group = json.load(response)[0]
            assert group['name'] == ','.join(names)
            assert list(group['split_queues']) == names
            master.send_signal(signal.SIGTERM)
            stdout, stderr = master.communicate(timeout=15)
            assert master.returncode == 0, (stdout, stderr)
            assert status() == []
            assert gone(ready['workers'][0]['pid'])
            # Loss of metadata terminates the master and reaps its owned workers.
            master = subprocess.Popen([binary, 'artisan', 'horizon'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            last_master = until_master(lambda state: state['workers'])
            last_workers = [worker['pid'] for worker in last_master['workers']]
            if redis_container:
                subprocess.run(['docker', 'stop', '--time', '3', redis_container], check=True, capture_output=True)
            else:
                redis('SHUTDOWN', 'NOSAVE')
            master.communicate(timeout=25)
            assert master.returncode != 0
            for pid in last_workers:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    continue
                raise AssertionError(f'orphan after metadata failure: {pid}')
            print('PASS: native processes, grouped literal queue names, worker options/backoff/nice/maintenance force, replacement and one-minute cooldown, spawn failure recovery, pause/continue, retry/forget, graceful signals, terminate, HTTP authorization and redaction')
        finally:
            for process in [master, server]:
                if process is not None and process.poll() is None:
                    process.terminate()
                    try:
                        process.communicate(timeout=15)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.communicate()


if __name__ == '__main__':
    main()
