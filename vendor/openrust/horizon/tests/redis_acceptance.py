"""Run Horizon tests against a dedicated Redis 8 instance without touching development data."""
import os
from pathlib import Path
import secrets
import signal
import subprocess
import sys
import time


def docker(*arguments, check=True):
    return subprocess.run(["docker", *arguments], check=check, capture_output=True, text=True)


def main():
    command = sys.argv[1:]
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        raise SystemExit("an acceptance command is required")
    batch = secrets.token_hex(12)
    name = "openrust-horizon-" + batch
    password = secrets.token_hex(24)
    image = docker("image", "inspect", "redis:8", "--format", "{{.Id}}").stdout.strip()
    child = None
    def interrupted(signum, _):
        if child is not None:
            child.send_signal(signum)
        raise KeyboardInterrupt
    for sig in [signal.SIGINT, signal.SIGTERM, signal.SIGHUP]:
        signal.signal(sig, interrupted)
    try:
        docker("run", "-d", "--name", name, "--label", "org.openrust.horizon.acceptance.run=" + batch,
               "--tmpfs", "/data:rw,size=64m", "-p", "127.0.0.1::6379", image,
               "redis-server", "--save", "", "--appendonly", "no", "--requirepass", password)
        for _ in range(100):
            reply = docker("exec", "-e", "REDISCLI_AUTH=" + password, name, "redis-cli", "PING", check=False)
            if reply.stdout.strip() == "PONG":
                break
            time.sleep(.1)
        else:
            raise RuntimeError("Redis fixture did not become ready")
        info = docker("exec", "-e", "REDISCLI_AUTH=" + password, name, "redis-cli", "INFO", "server").stdout
        version = next(line for line in info.splitlines() if line.startswith("redis_version:"))
        assert version.startswith("redis_version:8."), version
        address = docker("port", name, "6379/tcp").stdout.strip()
        print(name, version, flush=True)
        temporary = Path(__file__).resolve().parents[4] / "target" / "tmp" / "horizon-runtime"
        temporary.mkdir(parents=True, exist_ok=True)
        env = dict(os.environ, TMPDIR=str(temporary), HORIZON_TEST_REDIS_URL="redis://:" + password + "@" + address + "/0",
                   HORIZON_REQUIRE_REDIS="1", HORIZON_ACCEPTANCE_REDIS_CONTAINER=name)
        child = subprocess.Popen(command, env=env)
        return child.wait()
    finally:
        if child is not None and child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
        docker("stop", "--time", "3", name, check=False)
        docker("rm", name, check=False)
        assert docker("inspect", name, check=False).returncode != 0


if __name__ == "__main__":
    raise SystemExit(main())
