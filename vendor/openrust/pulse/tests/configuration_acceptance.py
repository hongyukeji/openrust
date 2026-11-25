"""Check the Pulse server name through the native configuration cache."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile


def verify(binary):
    with tempfile.TemporaryDirectory(prefix="openrust-pulse-server-config-") as directory:
        root = Path(directory)
        cache = root / "config.json"
        env = dict(os.environ, APP_BASE_PATH=directory, APP_CONFIG_CACHE=str(cache))
        env.pop("PULSE_SERVER_NAME", None)

        def artisan(*arguments, overrides=None):
            result = subprocess.run(
                [binary, "artisan", *arguments],
                env=env | (overrides or {}),
                capture_output=True,
                text=True,
                timeout=30,
            )
            assert result.returncode == 0, (arguments, result.stdout, result.stderr)
            return result.stdout

        def assert_cached(name):
            configuration = json.loads(cache.read_text())
            assert configuration["pulse"]["recorders"]["Servers"]["server_name"] == name
            shown = artisan(
                "config:show", "pulse.recorders.Servers.server_name",
                overrides={"PULSE_SERVER_NAME": "changed-after-cache"},
            )
            assert name in shown and "changed-after-cache" not in shown, shown

        for mode in ("provider", "published"):
            env["PULSE_PROBE_CONFIG"] = mode
            artisan("config:cache")
            assert_cached(socket.gethostname())
            artisan("config:cache", overrides={"PULSE_SERVER_NAME": "release-server"})
            assert_cached("release-server")
            artisan("config:clear")
            assert not cache.exists()
        print("PASS: Pulse provider and published config cache the default and explicit server names")


if __name__ == "__main__":
    verify(str(Path(sys.argv[1]).resolve()))
