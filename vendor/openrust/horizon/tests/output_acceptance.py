"""Exercise native output forwarding through Artisan and the two public callbacks."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def run_case(binary, mode):
    with tempfile.TemporaryDirectory(prefix='horizon-output-') as base:
        root = Path(base)
        (root / 'database.sqlite').touch()
        env = dict(os.environ, APP_BASE_PATH=base, APP_ENV='testing', LOG_CHANNEL='null')

        def run(*args, **variables):
            return subprocess.run([binary, *args], env=dict(env, **variables), capture_output=True,
                                  check=True, timeout=15)

        def read(file):
            return file.read_bytes() if file.exists() else b''

        def until(condition):
            end = time.monotonic() + 15
            while time.monotonic() < end:
                assert process.poll() is None, (mode, process.returncode, read(stdout), read(stderr))
                if condition():
                    return
                time.sleep(.15)
            raise AssertionError((mode, read(stdout), read(stderr)))

        run(HORIZON_PROBE_PREPARE_DATABASE='1')
        public = mode in ('master', 'supervisor')
        process_env = env
        if public:
            args = []
            process_env = dict(env, HORIZON_PROBE_OUTPUT_CALLBACK=mode)
        else:
            args = ['artisan', 'horizon']
            if mode == 'silent':
                args.append('--silent')
        stdout, stderr = root / 'artisan.stdout', root / 'artisan.stderr'
        with stdout.open('wb') as out, stderr.open('wb') as err:
            process = subprocess.Popen([binary, *args], env=process_env, stdout=out, stderr=err)
        try:
            run(HORIZON_PROBE_DISPATCH=json.dumps({'probe_output': mode}))
            marker_out = f'worker-out-東京-{mode}'.encode()
            marker_err = f'worker-err-{mode}'.encode()
            if public:
                callback_out, callback_err = root / 'callback.stdout', root / 'callback.stderr'
                expected = callback_err if mode == 'supervisor' else callback_out
                until(lambda: marker_out in read(callback_out) and marker_err in read(expected))
                assert f'direct-{mode}'.encode() in read(callback_err)
                assert marker_err not in read(stdout) and marker_err not in read(stderr)
                assert (marker_err in read(callback_err)) == (mode == 'supervisor')
            elif mode == 'silent':
                until(lambda: (root / 'completed.log').exists())
            else:
                until(lambda: marker_out in read(stdout) and marker_err in read(stdout))
                assert marker_err not in read(stderr)

            if mode == 'supervisor':
                run(HORIZON_PROBE_COMMAND_QUEUE=json.dumps({
                    'action': 'push', 'name': 'callback',
                    'command': 'Laravel\\Horizon\\SupervisorCommands\\Terminate', 'options': [],
                }))
            else:
                run('artisan', 'horizon:terminate')
            assert process.wait(timeout=15) == 0, (mode, read(stdout), read(stderr))
            if mode == 'silent':
                assert not read(stdout) and not read(stderr), (read(stdout), read(stderr))
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == '__main__':
    assert os.environ['HORIZON_REQUIRE_REDIS'] == '1'
    for mode in ('artisan', 'silent', 'master', 'supervisor'):
        run_case(str(Path(sys.argv[1]).resolve()), mode)
    print('PASS: live Artisan stdout, silent output, public Master callback and public Supervisor callback')
