"""用真实 Docker 验证独立 PostgreSQL 夹具的回收与并发隔离。"""

import json
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

from postgres_acceptance import DATA_DIR, LABEL, docker, owned_containers


RUNNER = Path(__file__).with_name("postgres_acceptance.py")
PROBE = '''
import os
from pathlib import Path
import sys
import time

Path(sys.argv[1]).write_text(os.environ["OPENRUST_ACCEPTANCE_POSTGRES_CONTAINER"])
while not Path(sys.argv[1] + ".release").exists():
    time.sleep(0.05)
sys.exit(int(sys.argv[2]))
'''


class PostgresAcceptanceLifecycle(unittest.TestCase):
    def wait_for_fixture(self, process, marker):
        deadline = time.monotonic() + 110
        while time.monotonic() < deadline and process.poll() is None:
            if marker.exists():
                container = marker.read_text()
                if len(container) == 64:
                    return container
            time.sleep(0.1)
        self.fail("验收子进程未报告夹具容器")

    def test_success_failure_and_cancel_preserve_the_other_instance(self):
        with tempfile.TemporaryDirectory(prefix="openrust-test-postgres-") as directory:
            marker = Path(directory) / "running"
            command = [sys.executable, "-B", str(RUNNER), "--", sys.executable, "-c", PROBE]
            with subprocess.Popen([*command, str(marker), "hold"]) as other:
                try:
                    survivor = self.wait_for_fixture(other, marker)
                    for outcome in ("0", "37", "hold"):
                        with self.subTest(outcome=outcome):
                            self.verify_outcome(command, Path(directory) / outcome, outcome)
                            self.assertEqual(
                                docker("exec", survivor, "sh", "-ec",
                                       'PGPASSWORD="$POSTGRES_PASSWORD" psql -h 127.0.0.1 '
                                       '-U "$POSTGRES_USER" -d "$POSTGRES_DB" -Atc "SELECT 42"'),
                                "42",
                            )
                finally:
                    if other.poll() is None:
                        other.terminate()
                        other.wait(timeout=120)
            self.assertNotIn(survivor, docker("ps", "-aq", "--no-trunc").splitlines())

    def verify_outcome(self, command, marker, outcome):
        with subprocess.Popen([*command, str(marker), outcome]) as process:
            try:
                container = self.wait_for_fixture(process, marker)
                info, = json.loads(docker("inspect", container))
                run_id = info["Config"]["Labels"][LABEL]
                self.assertEqual(set(info["HostConfig"]["Tmpfs"]), {DATA_DIR})
                self.assertTrue(all(mount["Type"] == "tmpfs" for mount in info["Mounts"]))
                if outcome == "hold":
                    process.send_signal(signal.SIGTERM)
                    expected = 128 + signal.SIGTERM
                else:
                    marker.with_suffix(".release").touch()
                    expected = int(outcome)
                self.assertEqual(process.wait(timeout=30), expected)
                self.assertEqual(owned_containers(run_id), [])
                self.assertEqual(docker("volume", "ls", "-q", "--filter", f"label={LABEL}={run_id}"), "")
            finally:
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=120)


if __name__ == "__main__":
    unittest.main()
