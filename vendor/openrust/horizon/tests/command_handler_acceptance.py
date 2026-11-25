"""Check named Horizon handlers against real Artisan processes and Redis."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

PAUSE = r'Laravel\Horizon\SupervisorCommands\Pause'


def main(binary):
    with tempfile.TemporaryDirectory(prefix='horizon-handler-') as base:
        root = Path(base)
        (root / 'database.sqlite').touch()
        env = dict(os.environ, APP_BASE_PATH=base, APP_ENV='testing', LOG_CHANNEL='null',
                   HORIZON_PROBE_EMPTY_DEFAULTS='1', HORIZON_PROBE_COMMAND_HANDLERS='1')
        processes = []

        def run(*args, accepted=(0,), **variables):
            result = subprocess.run([binary, *args], env=dict(env, **variables), capture_output=True,
                                    text=True, timeout=15)
            assert result.returncode in accepted, (args, result)
            return result.stdout

        def start(*args, **variables):
            process = subprocess.Popen([binary, 'artisan', *args], env=dict(env, **variables),
                                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            processes.append(process)
            return process

        def queue(name, command, options, master=False):
            run(HORIZON_PROBE_COMMAND_QUEUE=json.dumps(
                dict(action='push', name=name, command=command, options=options, master=master)))

        def status():
            return json.loads(run('artisan', 'horizon:status', '--json', accepted=(0, 1, 2)))

        def receipts():
            journal = root / 'command-handlers.jsonl'
            return [json.loads(line) for line in journal.read_text().splitlines()] if journal.exists() else []

        def until(predicate):
            end = time.monotonic() + 15
            while time.monotonic() < end:
                if result := predicate():
                    return result
                time.sleep(.15)
            raise AssertionError((status(), receipts()))

        run(HORIZON_PROBE_PREPARE_DATABASE='1')
        try:
            master = start('horizon')
            name = until(status)[0]['name']
            child = name + ':custom'
            deployment = {'name': child, 'queue': ['default'], 'maxProcesses': 1}
            queue(name, r'Probe\Wrong', {}, master=True)
            queue(name, r'Probe\Deploy', deployment, master=True)
            until(lambda: any(row['workers'] for row in status()))

            options = {'mode': 'override', 'nested': [False, '東京']}
            queue(child, PAUSE, options)
            until(lambda: len(receipts()) == 2)
            assert 'is running' in run('artisan', 'horizon:supervisor-status', child)
            job = run(HORIZON_PROBE_DISPATCH='{}', HORIZON_PROBE_DISPATCH_UUID='1').strip()
            completed = root / 'completed.log'
            until(lambda: completed.exists() and job in completed.read_text())
            run('artisan', 'horizon:terminate')
            assert master.wait(timeout=15) == 0, master.stderr.read()

            standalone = start('horizon:supervisor', 'standalone', 'redis', '--timeout=5')
            until(lambda: 'is running' in run('artisan', 'horizon:supervisor-status', 'standalone',
                                              accepted=(0, 1, 2)))
            queue('standalone', r'Probe\Stop', ['last'])
            assert standalone.wait(timeout=15) == 23, standalone.stderr.read()
            assert receipts() == [
                {'command': r'Probe\Deploy', 'name': name, 'options': deployment},
                {'command': PAUSE, 'name': child, 'options': options},
                {'command': r'Probe\Stop', 'name': 'standalone', 'options': ['last']},
            ], receipts()

            ordinary = start('horizon:supervisor', 'ordinary', 'redis', '--timeout=5',
                             HORIZON_PROBE_COMMAND_HANDLERS='0')
            until(lambda: 'is running' in run('artisan', 'horizon:supervisor-status', 'ordinary',
                                              accepted=(0, 1, 2)))
            queue('ordinary', PAUSE, options)
            until(lambda: 'is paused' in run('artisan', 'horizon:supervisor-status', 'ordinary',
                                             accepted=(0, 1, 2)))
            queue('ordinary', r'Laravel\Horizon\SupervisorCommands\Terminate', [])
            assert ordinary.wait(timeout=15) == 0, ordinary.stderr.read()
            print('PASS: named handler deployment, same-name override, raw options, exit status and app isolation')
        finally:
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
    main(str(Path(sys.argv[1]).resolve()))
