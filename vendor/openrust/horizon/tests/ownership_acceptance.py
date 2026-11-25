"""Exercise Horizon process ownership with a dedicated Redis fixture."""
import json
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit
import uuid


def main():
    assert os.environ["HORIZON_REQUIRE_REDIS"] == "1"
    binary = str(Path(sys.argv[1]).resolve())
    redis_url = urlsplit(os.environ["HORIZON_TEST_REDIS_URL"])
    redis_container = os.environ.get("HORIZON_ACCEPTANCE_REDIS_CONTAINER")
    if redis_container:
        redis_command = ["docker", "exec", "-e", "REDISCLI_AUTH", redis_container, "redis-cli"]
    else:
        assert os.environ["HORIZON_ACCEPTANCE_REDIS_DIRECT"] == "1"
        redis_command = ["redis-cli", "-h", redis_url.hostname, "-p", str(redis_url.port)]
    with tempfile.TemporaryDirectory(prefix="horizon-child-ownership-") as temporary:
        root = Path(temporary)
        with sqlite3.connect(root / "database.sqlite") as database:
            database.execute("CREATE TABLE failed_jobs (id INTEGER PRIMARY KEY, uuid TEXT UNIQUE, connection TEXT, queue TEXT, payload TEXT, exception TEXT, failed_at TEXT)")
        env = dict(os.environ, APP_BASE_PATH=temporary, APP_ENV="testing", LOG_CHANNEL="null",
                   HORIZON_PREFIX="child-ownership:")
        completed = root / "completed.log"
        master = subprocess.Popen([binary, "artisan", "horizon"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        stopped = []

        def redis(*arguments):
            return subprocess.check_output(
                [*redis_command, *arguments],
                env=dict(os.environ, REDISCLI_AUTH=redis_url.password), text=True, timeout=10,
            ).strip()

        def artisan(*args, environment=env):
            return subprocess.run([binary, "artisan", *args], env=environment, capture_output=True,
                                  text=True, timeout=15)

        def state():
            response = artisan("horizon:status", "--json")
            assert response.returncode in (0, 1, 2), response.stderr
            rows = json.loads(response.stdout)
            return rows[0] if rows and rows[0]["workers"] else None

        def until(check):
            end = time.monotonic() + 12
            while time.monotonic() < end:
                value = check()
                if value:
                    return value
                time.sleep(.2)
            raise AssertionError("timed out in process ownership acceptance")

        def dispatch():
            return subprocess.check_output([binary], env=dict(env, HORIZON_PROBE_DISPATCH="{}",
                                           HORIZON_PROBE_DISPATCH_UUID="1"), text=True, timeout=10).strip()

        def process_state(pid):
            return subprocess.run(["ps", "-p", str(pid), "-o", "stat="], capture_output=True,
                                  text=True, timeout=5).stdout.strip()

        def completed_job(payload):
            return completed.exists() and payload in completed.read_text()

        try:
            initial_job = dispatch()
            until(lambda: completed_job(initial_job))
            active = until(state)
            assert active["pid"] == master.pid, active
            name, supervisor = active["name"], active["supervisors"][0]
            prefix = f"{root.name}:child-ownership:{{horizon}}:"
            owner_key, master_key = prefix + "master-owner:" + name, prefix + "master:" + name
            suffix = supervisor.encode().hex()
            child_key, commands_key = prefix + "supervisor:" + suffix, prefix + "commands:" + suffix
            old_token, token = redis("GET", owner_key), str(uuid.uuid4())
            assert old_token
            child = json.loads(redis("GET", child_key))
            child_pid = child["pid"]
            assert int(subprocess.check_output(["ps", "-p", str(child_pid), "-o", "ppid="], text=True)) == master.pid

            # Keep both parents alive while the old Worker observes the Redis takeover itself.
            for pid in (master.pid, child_pid):
                os.kill(pid, signal.SIGSTOP)
                stopped.append(pid)
                until(lambda: process_state(pid).startswith("T"))
            replacement = dict(child, probe="replacement")
            assert redis(
                "EVAL",
                "redis.call('SET',KEYS[1],ARGV[1],'EX',60);redis.call('SET',KEYS[2],ARGV[2],'EX',60);redis.call('SET',KEYS[3],ARGV[3],'EX',60);redis.call('RPUSH',KEYS[4],'continue');return 1",
                "4", owner_key, master_key, child_key, commands_key, token,
                json.dumps(dict(active, environment="replacement")), json.dumps(replacement),
            ) == "1"
            job = dispatch()
            worker_pids = [worker["pid"] for worker in child["workers"]]
            assert worker_pids
            until(lambda: all(not process_state(pid) or process_state(pid).startswith("Z") for pid in worker_pids))
            assert not completed_job(job)

            stale = dict(env, OPENRUST_HORIZON_MASTER_NAME=name, OPENRUST_HORIZON_MASTER_OWNER=old_token)
            current = dict(stale, OPENRUST_HORIZON_MASTER_OWNER=token)
            args = ("horizon:work", "redis", "--once", "--force", f"--supervisor={supervisor}")
            result = artisan(*args, environment=stale)
            assert result.returncode != 0 and not completed_job(job), (result.stdout, result.stderr)
            result = artisan(*args, environment=current)
            assert result.returncode == 0, (result.stdout, result.stderr)
            assert completed_job(job)
            ordinary = dispatch()
            result = artisan("queue:work", "redis", "--once", "--force", environment=stale)
            assert result.returncode == 0 and completed_job(ordinary), (result.stdout, result.stderr)

            os.kill(child_pid, signal.SIGCONT)
            stopped.remove(child_pid)
            until(lambda: not process_state(child_pid) or process_state(child_pid).startswith("Z"))
            os.kill(master.pid, signal.SIGCONT)
            stopped.remove(master.pid)
            stdout, stderr = master.communicate(timeout=12)
            assert master.returncode != 0, (stdout, stderr)
            assert json.loads(redis("GET", master_key))["environment"] == "replacement"
            assert json.loads(redis("GET", child_key)) == replacement
            assert redis("LRANGE", commands_key, "0", "-1") == "continue"
            print("PASS: stale Supervisor and Worker cannot mutate new records or reserve jobs after takeover")
        finally:
            for pid in reversed(stopped):
                try:
                    os.kill(pid, signal.SIGCONT)
                except ProcessLookupError:
                    pass
            if master.poll() is None:
                master.terminate()
                try:
                    master.communicate(timeout=12)
                except subprocess.TimeoutExpired:
                    master.kill()
                    master.communicate()


if __name__ == "__main__":
    main()
