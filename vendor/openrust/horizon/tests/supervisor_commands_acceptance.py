"""Drive all six structured commands through a native standalone Supervisor."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit


def main():
    assert os.environ['HORIZON_REQUIRE_REDIS'] == '1'
    binary = str(Path(sys.argv[1]).resolve())
    redis_url = urlsplit(os.environ['HORIZON_TEST_REDIS_URL'])
    container = os.environ.get('HORIZON_ACCEPTANCE_REDIS_CONTAINER')
    if container:
        redis_cli = ['docker', 'exec', '-e', 'REDISCLI_AUTH', container, 'redis-cli']
    else:
        assert os.environ['HORIZON_ACCEPTANCE_REDIS_DIRECT'] == '1'
        redis_cli = ['redis-cli', '-h', redis_url.hostname, '-p', str(redis_url.port)]

    with tempfile.TemporaryDirectory(prefix='horizon-supervisor-commands-') as base:
        root = Path(base)
        (root / 'database.sqlite').touch()
        env = dict(os.environ, APP_BASE_PATH=base, APP_ENV='testing', LOG_CHANNEL='null',
                   HORIZON_PREFIX='structured-supervisor:')
        name = 'probe:東京 commands'
        prefix = f'{root.name}:structured-supervisor:{{horizon}}:'
        key = prefix + 'supervisor:' + name.encode().hex()
        started, completed = root / 'started.log', root / 'completed.log'

        def invoke(**variables):
            result = subprocess.run([binary], env=dict(env, **variables), capture_output=True,
                                    text=True, timeout=15)
            assert result.returncode == 0, (result.stdout, result.stderr)
            return result.stdout.strip()

        def push(command, options):
            body = dict(action='push', name=name, command='Laravel\\Horizon\\SupervisorCommands\\' + command,
                        options=options)
            invoke(HORIZON_PROBE_COMMAND_QUEUE=json.dumps(body))

        def dispatch(duration):
            return invoke(HORIZON_PROBE_DISPATCH=json.dumps({'record_start': True, 'sleep_ms': duration}),
                          HORIZON_PROBE_DISPATCH_UUID='1', HORIZON_PROBE_DISPATCH_QUEUE='first')

        def state():
            raw = subprocess.check_output([*redis_cli, '-e', 'GET', key],
                                          env=dict(os.environ, REDISCLI_AUTH=redis_url.password),
                                          text=True, timeout=5).strip()
            return json.loads(raw) if raw else {}

        def until(check):
            end = time.monotonic() + 18
            while time.monotonic() < end:
                if result := check():
                    return result
                time.sleep(.15)
            raise AssertionError(f'Expected Supervisor state not reached: {state()}')

        def counts():
            return state().get('processes')

        def pids():
            return {worker['pid'] for worker in state().get('workers', []) if not worker['terminating']}

        def recorded(path, job):
            return path.exists() and job in path.read_text().splitlines()

        arguments = [binary, 'artisan', 'horizon:supervisor', name, 'redis', '--queue=first,second',
                     '--balance=simple', '--max-processes=2', '--balance-cooldown=120',
                     '--sleep=1', '--timeout=10', '--force']
        invoke(HORIZON_PROBE_PREPARE_DATABASE='1')
        push('Terminate', {'status': 99})
        for expected_exit in (23, 0):
            with subprocess.Popen(arguments, env=env, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, text=True) as process:
                try:
                    until(lambda: counts() == {'redis:first': 1, 'redis:second': 1})
                    if expected_exit == 0:
                        push('Terminate', [])
                    else:
                        push('Scale', {'invalid': 4})
                        push('Pause', [])
                        until(lambda: state().get('status') == 'paused')
                        job = dispatch(20)
                        time.sleep(1.25)
                        assert not recorded(started, job), 'paused Worker reserved the job'
                        push('ContinueWorking', [])
                        until(lambda: recorded(completed, job))

                        push('Pause', [])
                        until(lambda: state().get('status') == 'paused')
                        before = pids()
                        push('Restart', [])
                        until(lambda: state().get('status') == 'running'
                              and len(pids()) == 2 and before.isdisjoint(pids()))

                        push('Scale', {'scale': 5})
                        until(lambda: counts() == {'redis:first': 2, 'redis:second': 2})
                        assert state()['options']['maxProcesses'] == 5
                        push('Balance', {'first': '1', 'second': -1, 'unknown': 4})
                        until(lambda: counts() == {'redis:first': 1, 'redis:second': 0})

                        job = dispatch(2400)
                        until(lambda: recorded(started, job))
                        invoke(HORIZON_PROBE_WRAPPER_TERMINATE=json.dumps(
                            {'name': name, 'status': expected_exit}
                        ))
                    stdout, stderr = process.communicate(timeout=15)
                    assert process.returncode == expected_exit, (process.returncode, stdout, stderr)
                    if expected_exit:
                        assert recorded(completed, job), 'Terminate did not drain the running job'
                    assert not state()
                finally:
                    if process.poll() is None:
                        process.terminate()
                        try:
                            process.communicate(timeout=12)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.communicate()
        print('PASS: structured pause, continue, restart, scale, balance and draining termination with exit status')

        loop_events = root / 'loop-errors.log'
        restart_events = root / 'restart-events.jsonl'
        process_reports = root / 'process-reports.log'
        first_event = len(restart_events.read_text().splitlines()) if restart_events.exists() else 0
        failed_directory = root / 'missing-worker-directory'
        failure_arguments = arguments + ['--min-processes=2', f'--directory={failed_directory}']
        failure_env = dict(env, HORIZON_PROBE_FAIL_LOOP_DISPATCH='1', HORIZON_PROBE_RECORD_PROCESS_REPORTS='1')
        with subprocess.Popen(failure_arguments, env=failure_env, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, text=True) as process:
            try:
                initial = until(state)
                assert initial['status'] == 'running' and initial['workers'] == [], initial
                until(lambda: recorded(loop_events, 'SupervisorLooped'))
                events = [json.loads(row) for row in restart_events.read_text().splitlines()[first_event:]]
                assert [(event['name'], event['queue'], event['pid'], event['exit_code']) for event in events] == [
                    ('UnableToLaunchProcess', 'first', None, None),
                    ('UnableToLaunchProcess', 'second', None, None),
                ], events
                assert process_reports.read_text().splitlines() == ['NotFound'] * 4
                push('Terminate', [])
                stdout, stderr = process.communicate(timeout=15)
                assert process.returncode == 0, (process.returncode, stdout, stderr)
                assert not state()
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.communicate(timeout=12)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.communicate()
        print('PASS: initial Supervisor state, separate failed Worker start reports and next-loop continuation')


if __name__ == '__main__':
    main()
