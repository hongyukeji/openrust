"""Native horizon:work contract checks; run through redis_acceptance.py."""
import json
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time


def main():
    assert os.environ['HORIZON_REQUIRE_REDIS'] == '1'
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix='openrust-horizon-work-') as base:
        root = Path(base)
        with sqlite3.connect(root / 'database.sqlite') as db:
            db.execute('CREATE TABLE failed_jobs (id INTEGER PRIMARY KEY AUTOINCREMENT, uuid TEXT UNIQUE, connection TEXT, queue TEXT, payload TEXT, exception TEXT, failed_at TEXT)')
        env = dict(os.environ, APP_BASE_PATH=base, APP_ENV='testing', LOG_CHANNEL='null')

        def artisan(*args, code=0):
            result = subprocess.run([binary, 'artisan', *args], env=env, capture_output=True, text=True, timeout=20)
            assert result.returncode == code, (args, result.stdout, result.stderr)
            return result.stdout

        def dispatch(**data):
            result = subprocess.run([binary], env=dict(env, HORIZON_PROBE_DISPATCH=json.dumps(data), HORIZON_PROBE_DISPATCH_UUID="1"), capture_output=True, text=True, timeout=15)
            assert result.returncode == 0, result.stderr
            return result.stdout.strip()

        def failed():
            with sqlite3.connect(root / 'database.sqlite') as db:
                return db.execute('SELECT COUNT(*) FROM failed_jobs').fetchone()[0]

        help_text = artisan('help', 'horizon:work')
        for option in ['--supervisor', '--delay', '--daemon', '--stop-when-empty-for', '--tries', '--backoff']:
            assert option in help_text, (option, help_text)
        assert 'horizon:work' not in artisan('list', '--raw')
        artisan('horizon:work', '--backoff=bad', code=1)
        # Hidden entry point still supports direct, unsupervised once/daemon operation.
        first = dispatch()
        artisan('horizon:work', 'redis', '--once', '--force', '--json')
        assert first in (root / 'completed.log').read_text()
        artisan('horizon:work', '--stop-when-empty', '--force', '--daemon')

        # The default is unlimited attempts (Horizon tries=0, native queue:work tries=1).
        # Deprecated delay must participate in PHP max(backoff,delay), including schedules.
        for backoff, delay, expected in [('0', '2', 2), ('1,3', '2', 2), ('3,1', '2', 3)]:
            dispatch(fail=True)
            started = time.monotonic()
            artisan('horizon:work', '--max-jobs=2', '--force', '--sleep=0', f'--backoff={backoff}', f'--delay={delay}')
            assert time.monotonic() - started >= expected, (backoff, delay)
            assert failed() == 0, 'Horizon default tries must be unlimited'
            artisan('queue:clear', 'redis', '--force')

        artisan('horizon:pause')
        pending = dispatch()
        worker = subprocess.Popen([binary, 'artisan', 'horizon:work', '--supervisor=worker scope $(literal)', '--force', '--sleep=0', '--json'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            time.sleep(1.3)
            assert pending not in (root / 'completed.log').read_text()
            # An ordinary worker must not inherit Horizon pause from a process environment marker.
            normal_env = dict(env, OPENRUST_HORIZON_SUPERVISOR='stale-marker')
            result = subprocess.run([binary, 'artisan', 'queue:work', '--once', '--force'], env=normal_env, capture_output=True, text=True, timeout=10)
            assert result.returncode == 0, result.stderr
            assert pending in (root / 'completed.log').read_text()
            worker.send_signal(signal.SIGTERM)
            stdout, stderr = worker.communicate(timeout=10)
            assert worker.returncode == 0, (stdout, stderr)
        finally:
            if worker.poll() is None:
                worker.kill()
                worker.communicate()
        print('PASS: hidden Horizon worker, native execution, unlimited tries, PHP delay/backoff selection, pause isolation, SIGTERM')


if __name__ == '__main__':
    main()
