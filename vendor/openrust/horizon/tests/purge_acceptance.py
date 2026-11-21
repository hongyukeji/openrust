"""Exercise Unix orphan inspection and signalling through the native purge command."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time


def main():
    assert os.environ['HORIZON_REQUIRE_REDIS'] == '1'
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix='openrust-horizon-purge-') as base:
        env = dict(os.environ, APP_BASE_PATH=base, APP_ENV='testing', LOG_CHANNEL='null')
        def artisan(*args):
            result = subprocess.run([binary, 'artisan', *args], env=env, capture_output=True, text=True, timeout=15)
            assert result.returncode == 0, (args, result.stdout, result.stderr)
            return result.stdout
        def until(check):
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if value := check():
                    return value
                time.sleep(.1)
            raise AssertionError('orphan purge acceptance timed out')
        def state():
            result = subprocess.run([binary, 'artisan', 'horizon:status', '--json'], env=env, capture_output=True, text=True, timeout=15)
            assert result.returncode in (0, 2), result.stderr
            return json.loads(result.stdout)
        def command(pid):
            return subprocess.run(['ps', '-p', str(pid), '-o', 'command='], capture_output=True, text=True).stdout.strip()
        master = subprocess.Popen([binary, 'artisan', 'horizon'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        orphan = None
        marker = None
        try:
            active = until(lambda: state() and state()[0])
            marker = '--supervisor=' + active['name'] + ':purge-probe'
            launcher = 'import subprocess,sys; child=subprocess.Popen(sys.argv[1:],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True); print(child.pid)'
            orphan = int(subprocess.check_output([sys.executable, '-B', '-c', launcher, binary, 'artisan', 'horizon:work', marker], env=dict(env, HORIZON_PROBE_HOLD='1'), text=True))
            until(lambda: subprocess.run(['ps', '-p', str(orphan), '-o', 'ppid='], capture_output=True, text=True).stdout.strip() == '1')
            assert marker in command(orphan)
            assert str(orphan) in artisan('horizon:purge')
            until(lambda: not command(orphan))
            assert state()[0]['pid'] == master.pid, 'purge must preserve the live master'
            master.send_signal(signal.SIGTERM)
            _, stderr = master.communicate(timeout=15)
            assert master.returncode == 0, stderr
            assert state() == []
            print('PASS: native purge discovers a real orphan, signals it, and preserves the live master')
        finally:
            if orphan and marker and marker in command(orphan):
                os.kill(orphan, signal.SIGTERM)
                until(lambda: not command(orphan))
            if master.poll() is None:
                master.terminate()
                try:
                    master.communicate(timeout=15)
                except subprocess.TimeoutExpired:
                    master.kill()
                    master.communicate()


if __name__ == '__main__':
    main()
