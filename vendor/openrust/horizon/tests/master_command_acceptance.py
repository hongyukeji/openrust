"""Exercise the public command queue against a native Horizon Master and Redis."""
import json
import os
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def main():
    assert os.environ['HORIZON_REQUIRE_REDIS'] == '1'
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix='horizon-master-command-') as base:
        root = Path(base)
        (root / 'database.sqlite').touch()
        env = dict(os.environ, APP_BASE_PATH=base, APP_ENV='testing', LOG_CHANNEL='null',
                   HORIZON_PREFIX='master-command:', HORIZON_PROBE_MASTER_BASENAME='North:東京 1',
                   HORIZON_PROBE_EMPTY_DEFAULTS='1', HORIZON_PROBE_PRELOAD_MASTER_COMMAND='1',
                   HORIZON_PROBE_RECORD_SUPERVISOR_CWD='1')

        def run(*args, accepted=(0,), **variables):
            result = subprocess.run([binary, *args], env=dict(env, **variables),
                                    capture_output=True, text=True, timeout=15)
            assert result.returncode in accepted, (args, result.returncode, result.stdout, result.stderr)
            return result.stdout.strip()

        def queue(action, name, **values):
            return run(HORIZON_PROBE_COMMAND_QUEUE=json.dumps(dict(action=action, name=name, **values)))

        def statuses():
            return json.loads(run('artisan', 'horizon:status', '--json', accepted=(0, 1, 2)))

        def until(predicate):
            end = time.monotonic() + 15
            while time.monotonic() < end:
                if value := predicate():
                    return value
                time.sleep(.15)
            raise AssertionError(f'Acceptance condition not reached: {statuses()}')

        run(HORIZON_PROBE_PREPARE_DATABASE='1')
        queue('push', 'public-probe', command='One', options=[])
        queue('push', 'public-probe', command='Two', options={'nested': [False, '東京']})
        queue('push', 'other-probe', command='Other', options={})
        with ThreadPoolExecutor(max_workers=2) as readers:
            batches = list(readers.map(lambda _: json.loads(queue('pending', 'public-probe')), range(2)))
        assert sorted(batches, key=len) == [[], [
            {'command': 'One', 'options': []}, {'command': 'Two', 'options': {'nested': [False, '東京']}},
        ]]
        assert json.loads(queue('pending', 'public-probe')) == []
        queue('flush', 'other-probe')
        assert json.loads(queue('pending', 'other-probe')) == []

        master = subprocess.Popen([binary, 'artisan', 'horizon'], env=env,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            row = until(lambda: next((row for row in statuses() if row['pid'] == master.pid), None))
            name = row['name']
            child = name + ':部署 queue'
            directory = root / 'child cwd'
            assert not row['supervisors']
            run('artisan', 'horizon:pause')
            until(lambda: any(row['status'] == 'paused' for row in statuses()))
            queue('push', name, master=True, command='Unknown', options={})
            queue('push', name, master=True, options={'name': 'another-master:foreign'})
            queue('push', name, master=True, options={'name': child, 'queue': ['added'], 'maxProcesses': 1,
                                                      'directory': directory.name})
            queue('push', name, master=True, options={'name': child, 'queue': ['wrong'], 'maxProcesses': 1})

            paused = until(lambda: next((row for row in statuses() if child in row['supervisors']), None))
            assert paused['supervisors'] == [child] and not paused['workers'], paused
            run('artisan', 'horizon:continue')
            failed_start = until(lambda: next((row for row in statuses() if row['unexpected_exits']), None))
            assert not failed_start['workers']
            directory.mkdir()
            live = until(lambda: next((row for row in statuses() if row['workers']), None))
            assert live['supervisors'] == [child], live
            assert live['workers'][0]['supervisor'] == child and live['workers'][0]['queue'] == 'added', live
            assert Path(json.loads((root / 'supervisor-cwd.json').read_text())).resolve() == directory.resolve()
            assert json.loads(queue('pending', name, master=True)) == []
            job = run(HORIZON_PROBE_DISPATCH='{"probe_output":"master-native"}', HORIZON_PROBE_DISPATCH_UUID='1',
                      HORIZON_PROBE_DISPATCH_QUEUE='added')
            completed = root / 'completed.log'
            until(lambda: completed.exists() and job in completed.read_text())
            run(HORIZON_PROBE_DEPLOY_PLAN=json.dumps({'master': name, 'environment': 'public-plan'}))
            planned = name + ':planned'
            live = until(lambda: next((row for row in statuses()
                                      if any(worker['supervisor'] == planned for worker in row['workers'])), None))
            assert live['supervisors'] == [child, planned], live
            event = json.loads((root / 'deployment-events.jsonl').read_text().splitlines()[-1])
            assert event['environment'] == 'public-plan' and event['supervisors'] == [planned], event
            planned_job = run(HORIZON_PROBE_DISPATCH='{}', HORIZON_PROBE_DISPATCH_UUID='1',
                              HORIZON_PROBE_DISPATCH_QUEUE='planned')
            until(lambda: planned_job in completed.read_text())

            def parent_pid(pid):
                if sys.platform.startswith('linux'):
                    status = Path(f'/proc/{pid}/status').read_text()
                    return int(next(line.split()[1] for line in status.splitlines() if line.startswith('PPid:')))
                return int(subprocess.check_output(['ps', '-p', str(pid), '-o', 'ppid='], text=True).strip())

            def worker_tree(row):
                return {entry['supervisor']: (entry['pid'], parent_pid(entry['pid'])) for entry in row['workers']}

            before = worker_tree(live)
            assert before.keys() == {child, planned}, live
            active_job = run(HORIZON_PROBE_DISPATCH='{"record_start":true,"sleep_ms":6000}',
                             HORIZON_PROBE_DISPATCH_UUID='1', HORIZON_PROBE_DISPATCH_QUEUE='added')
            started = root / 'started.log'
            until(lambda: started.exists() and active_job in started.read_text())
            run('artisan', 'horizon:pause')
            until(lambda: statuses()[0]['status'] == 'paused')
            pending_job = run(HORIZON_PROBE_DISPATCH='{"record_start":true,"probe_output":"master-replacement"}',
                              HORIZON_PROBE_DISPATCH_UUID='1', HORIZON_PROBE_DISPATCH_QUEUE='added')
            command = 'Laravel\\Horizon\\SupervisorCommands\\Restart'
            queue('push', name, master=True, command=command, options=[])
            resumed = until(lambda: next((row for row in statuses() if row['status'] == 'running'), None))
            assert resumed['pid'] == master.pid and resumed['name'] == name, resumed
            assert active_job not in completed.read_text()
            assert pending_job not in started.read_text()

            def replaced():
                row = statuses()[0]
                workers = {item['supervisor']: item['pid'] for item in row['workers']}
                if workers.keys() == before.keys() and all(
                    pid != before[supervisor][0] for supervisor, pid in workers.items()
                ):
                    return row

            replacement = until(replaced)
            after = worker_tree(replacement)
            assert all(old[1] != after[supervisor][1] for supervisor, old in before.items()), after
            assert replacement['unexpected_exits'] == live['unexpected_exits'], replacement
            until(lambda: pending_job in completed.read_text())
            assert completed.read_text().splitlines().count(active_job) == 1
            assert completed.read_text().splitlines().index(active_job) < completed.read_text().splitlines().index(pending_job)
            print('PASS: Master restart resumes a paused deployment, drains a real job and replaces both Supervisor and Worker trees')

            run('artisan', 'horizon:terminate')
            stdout, stderr = master.communicate(timeout=15)
            assert master.returncode == 0, (stdout, stderr)
            assert stdout.count('worker-out-東京-master-native') == 1, stdout
            assert stdout.count('worker-err-master-native') == 1, stdout
            assert stdout.count('worker-out-東京-master-replacement') == 1, stdout
            assert stdout.count('worker-err-master-replacement') == 1, stdout
            assert 'worker-err-master-native' not in stderr, stderr
            assert statuses() == []
            print('PASS: public FIFO queue, startup flush, paused dynamic deployment, actual Worker and scoped shutdown')
        finally:
            if master.poll() is None:
                master.terminate()
                try:
                    master.communicate(timeout=12)
                except subprocess.TimeoutExpired:
                    master.kill()
                    master.communicate()


if __name__ == '__main__':
    main()
