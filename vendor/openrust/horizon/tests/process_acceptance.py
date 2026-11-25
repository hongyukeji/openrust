"""Run through redis_acceptance.py; exercises native supervisor processes and HTTP."""
import json
import os
from pathlib import Path
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
        env = dict(os.environ, APP_BASE_PATH=base, APP_ENV='testing', LOG_CHANNEL='null', HORIZON_PREFIX='horizon-probe:')
        redis_auth = dict(os.environ, REDISCLI_AUTH=urlsplit(env['HORIZON_TEST_REDIS_URL']).password)
        wait_lock = f'{root.name}:horizon-probe:{{horizon}}:wait_time_monitor'
        def redis(*args):
            return subprocess.check_output(
                ['docker', 'exec', '-e', 'REDISCLI_AUTH', env['HORIZON_ACCEPTANCE_REDIS_CONTAINER'], 'redis-cli', *args],
                env=redis_auth, text=True, timeout=10,
            ).strip()
        def artisan(*args, code=0):
            result = subprocess.run([binary, 'artisan', *args], env=env, capture_output=True, text=True, timeout=30)
            assert result.returncode == code, (args, result.stdout, result.stderr)
            return result.stdout
        def dispatch(payload_uuid=False, **data):
            result = subprocess.run([binary], env=dict(env, HORIZON_PROBE_DISPATCH=json.dumps(data), HORIZON_PROBE_DISPATCH_UUID="1" if payload_uuid else "0"), capture_output=True, text=True, timeout=15)
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
                    assert response.status == 200
                    return json.load(response)
            assert stats()['masters'] == []
            artisan('horizon:status', code=2)
            # Pause before startup: no child may reserve queued work.
            artisan('horizon:pause')
            dispatch(password='secret-never-exported')
            assert redis('SET', wait_lock, 'contender', 'EX', '30') == 'OK'
            master = subprocess.Popen([binary, 'artisan', 'horizon'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            until(lambda: status() and status()[0]['status'] == 'paused')
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
            initial = until(lambda: status() and status()[0]['workers'] and status()[0])
            child_pids.update(w['pid'] for w in initial['workers'])
            worker_pid = initial['workers'][0]['pid']
            command = subprocess.check_output(['ps', '-p', str(worker_pid), '-o', 'command='], text=True)
            for argument in ['artisan horizon:work', '--supervisor=' + initial['name'] + ':supervisor-1', '--name=horizon-probe', '--tries=2', '--backoff=1,2', '--rest=1', '--force', '--json']:
                assert argument in command, (argument, command)
            assert int(subprocess.check_output(['ps', '-p', str(worker_pid), '-o', 'ni='], text=True).strip()) >= 1
            supervisor_name = initial['name'] + ':supervisor-1'
            supervisor_pid = int(subprocess.check_output(['ps', '-p', str(worker_pid), '-o', 'ppid='], text=True))
            assert int(subprocess.check_output(['ps', '-p', str(supervisor_pid), '-o', 'ppid='], text=True)) == master.pid
            assert 'artisan horizon:supervisor ' + supervisor_name in subprocess.check_output(['ps', '-p', str(supervisor_pid), '-o', 'command='], text=True)
            artisan('horizon:pause-supervisor', supervisor_name)
            until(lambda: 'paused' in artisan('horizon:supervisor-status', supervisor_name))
            artisan('horizon:continue-supervisor', supervisor_name)
            until(lambda: 'running' in artisan('horizon:supervisor-status', supervisor_name))
            assert initial['name'] in artisan('horizon:list')
            assert supervisor_name in artisan('horizon:supervisors')
            # Unexpected termination is detected, reaped and replaced.
            killed = initial['workers'][0]['pid']
            os.kill(killed, signal.SIGKILL)
            recovered = until(lambda: status() and status()[0]['unexpected_exits'] >= 1 and status()[0]['workers'] and all(w['pid'] != killed for w in status()[0]['workers']) and status()[0])
            child_pids.update(w['pid'] for w in recovered['workers'])
            # Backlog grows the pool within its cap; maxJobs also recycles workers.
            for _ in range(4):
                dispatch(sleep_ms=1500)
            scaled = until(lambda: status() and len(status()[0]['workers']) == 2 and status()[0])
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
            rebuilt = until(lambda: status() and status()[0]['unexpected_exits'] >= 2 and status()[0]['workers'] and all(worker['pid'] not in old_workers for worker in status()[0]['workers']) and status()[0])
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
            until(lambda: status() and status()[0]['status'] == 'paused')
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
            until(lambda: status() and status()[0]['workers'])
            # HTTP correctly returns 503 during maintenance. Observe native
            # completion directly, then inspect Horizon after leaving maintenance.
            until(lambda: (root / 'completed.log').exists() and forced_id in (root / 'completed.log').read_text())
            artisan('up')
            until(lambda: stats()['queues'][0]['metrics']['completed'] == before + 3)
            artisan('horizon:terminate')
            stdout, stderr = master.communicate(timeout=15)
            assert master.returncode == 0, (stdout, stderr)
            assert status() == []
            # Fast termination releases the master while jobs finish; --wait
            # explicitly restores the draining guarantee.
            env['HORIZON_PROBE_FAST_TERMINATION'] = '1'
            for wait in [False, True]:
                long_id = dispatch(sleep_ms=3000)
                master = subprocess.Popen([binary, 'artisan', 'horizon'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                until(lambda: any(job['id'] == long_id and job['status'] == 'processing' for job in stats()['recent']))
                draining_workers = [worker['pid'] for worker in status()[0]['workers']]
                started = time.monotonic()
                artisan('horizon:terminate', *(['--wait'] if wait else []))
                stdout, stderr = master.communicate(timeout=15)
                elapsed = time.monotonic() - started
                assert master.returncode == 0, (stdout, stderr)
                if wait:
                    assert any(job['id'] == long_id and job['status'] == 'completed' for job in stats()['recent'])
                else:
                    assert elapsed < 2.5, elapsed
                    until(lambda: any(job['id'] == long_id and job['status'] == 'completed' for job in stats()['recent']))
                until(lambda: all(gone(pid) for pid in draining_workers))
                assert status() == []
            env.pop('HORIZON_PROBE_FAST_TERMINATION')
            # Both supervisor and master memory limits stop with the upstream
            # exit code and leave no live process registrations.
            artisan('horizon:supervisor', 'memory-probe', 'redis', '--memory=1', code=12)
            env['HORIZON_PROBE_MEMORY_LIMIT'] = '1'
            artisan('horizon', code=12)
            env.pop('HORIZON_PROBE_MEMORY_LIMIT')
            assert status() == []
            # File watching is live, responds to a change once, and cleans up
            # its child tree when interrupted.
            watched = root / 'routes' / 'web.rs'
            watched.parent.mkdir()
            watched.write_text('// first version\n')
            master = subprocess.Popen([binary, 'artisan', 'horizon:listen', '--poll'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            watched_master = until(lambda: status() and status()[0]['workers'] and status()[0]['name'])
            watched.write_text('// changed version\n')
            until(lambda: status() and status()[0]['name'] != watched_master and status()[0]['workers'])
            master.send_signal(signal.SIGTERM)
            stdout, stderr = master.communicate(timeout=20)
            assert master.returncode == 0, (stdout, stderr)
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
            # Loss of metadata terminates the master and reaps its owned workers.
            master = subprocess.Popen([binary, 'artisan', 'horizon'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            until(lambda: status() and status()[0]['workers'])
            last_workers = [w['pid'] for w in status()[0]['workers']]
            subprocess.run(['docker', 'stop', '--time', '3', os.environ['HORIZON_ACCEPTANCE_REDIS_CONTAINER']], check=True, capture_output=True)
            master.communicate(timeout=25)
            assert master.returncode != 0
            for pid in last_workers:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    continue
                raise AssertionError(f'orphan after metadata failure: {pid}')
            print('PASS: native processes, worker options/backoff/nice/maintenance force, replacement, pause/continue, retry/forget, graceful signals, terminate, HTTP authorization and redaction')
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
