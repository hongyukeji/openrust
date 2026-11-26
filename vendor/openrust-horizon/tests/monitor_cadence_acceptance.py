"""Check both native Artisan monitors against an isolated Redis instance."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time


def main():
    assert os.environ["HORIZON_REQUIRE_REDIS"] == "1"
    binary = str(Path(sys.argv[1]).resolve())
    for role, command in (
        ("master", ["horizon", "--environment=empty"]),
        ("supervisor", ["horizon:supervisor", "cadence:supervisor", "redis", "--queue=probe", "--paused"]),
    ):
        with tempfile.TemporaryDirectory(prefix=f"horizon-cadence-{role}-") as directory:
            root = Path(directory)
            (root / "database.sqlite").touch()
            env = dict(os.environ, APP_BASE_PATH=directory, APP_ENV="testing", LOG_CHANNEL="null",
                       HORIZON_PROBE_LOOP_CADENCE=role, HORIZON_PROBE_EMPTY_DEFAULTS="1")
            with subprocess.Popen([binary, "artisan", *command], env=env, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, text=True) as process:
                try:
                    path = root / "cadence.log"
                    deadline = time.monotonic() + 15
                    rows = []
                    while len(rows) < 4:
                        assert process.poll() is None, process.communicate()
                        assert time.monotonic() < deadline, (role, rows)
                        time.sleep(0.025)
                        rows = path.read_text().splitlines() if path.exists() else []
                    first, completed, second, next_completed = (row.split() for row in rows[:4])
                    assert (first[0], completed[0], second[0], next_completed[0]) == (
                        "start", "completed", "start", "completed",
                    ), rows
                    startup_ms = int(first[1])
                    after_work_ms = int(second[1]) - int(completed[1])
                    assert startup_ms >= 950 and after_work_ms >= 950, (role, startup_ms, after_work_ms)
                    process.send_signal(signal.SIGTERM)
                    stdout, stderr = process.communicate(timeout=0.9)
                    assert process.returncode == 0, (stdout, stderr)
                    assert path.read_text().splitlines() == rows[:4], (role, path.read_text())
                    print(json.dumps(dict(monitor=role, startup_ms=startup_ms, after_work_ms=after_work_ms)))
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.communicate(timeout=5)


if __name__ == "__main__":
    main()
