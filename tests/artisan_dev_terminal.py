#!/usr/bin/env python3
"""Exercise Artisan's Unix terminal modes with a compiled executable or source launcher."""

import fcntl
import os
from pathlib import Path
import pty
import re
import select
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import termios
import time
import urllib.request

ENTER_SCREEN = b"\x1b[?1049h"
LEAVE_SCREEN = b"\x1b[?1049l"


class Terminal:
    def __init__(self, command, env, rows):
        self.master, self.slave = pty.openpty()
        self.resize(rows, 100)
        self.initial = termios.tcgetattr(self.slave)
        self.output = bytearray()

        def controlling_terminal():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen(
            command,
            stdin=self.slave,
            stdout=self.slave,
            stderr=self.slave,
            preexec_fn=controlling_terminal,
            env=env,
        )

    def __enter__(self):
        return self

    def __exit__(self, *_):
        if self.process.poll() is None:
            self.send(b"\x03")
            try:
                self.finish()
            except (AssertionError, OSError):
                os.killpg(self.process.pid, signal.SIGKILL)
                self.process.wait(timeout=5)
        os.close(self.slave)
        os.close(self.master)

    def resize(self, rows, columns):
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))

    def send(self, keys):
        os.write(self.master, keys)

    def read(self):
        if select.select([self.master], [], [], 0.05)[0]:
            self.output.extend(os.read(self.master, 65536))

    def frame(self):
        return bytes(self.output).rsplit(b"\x1b[H", 1)[-1]

    def expect(self, check, description):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            self.read()
            if check():
                return
            if self.process.poll() is not None:
                break
        raise AssertionError(f"{description}\n{self.output[-4000:]!r}")

    def finish(self):
        deadline = time.monotonic() + 12
        while self.process.poll() is None:
            self.read()
            assert time.monotonic() < deadline, "Artisan failed to exit"
        while select.select([self.master], [], [], 0)[0]:
            self.read()
        assert self.process.returncode == 0, self.output[-4000:]
        assert termios.tcgetattr(self.slave) == self.initial, "terminal attributes were not restored"


def available_port():
    with socket.socket() as server:
        server.bind(("127.0.0.1", 0))
        return server.getsockname()[1]


def inspect_tabs(terminal, root, port):
    terminal.expect(
        lambda: b"Tabs" in terminal.frame() and b"vite [exited]" in terminal.frame(),
        "tabbed mode did not report Vite exit",
    )
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/up", timeout=3) as response:
        assert response.status == 200
    terminal.send(b"3")
    terminal.expect(
        lambda: b"needle-2" in terminal.frame() and b"early-1" not in terminal.frame(),
        "tab log limit or selection failed",
    )
    terminal.resize(4, 40)
    terminal.expect(lambda: b"q quit" in terminal.frame(), "resize was not rendered")
    terminal.resize(16, 100)
    terminal.expect(lambda: b"needle-2" in terminal.frame(), "resize lost the selected tab")
    terminal.send(b"/needle\r")
    terminal.expect(lambda: b"\x1b[7m" in terminal.frame(), "search did not highlight a match")
    terminal.send(b"n")
    terminal.expect(
        lambda: "› needle-2".encode() in terminal.frame(),
        "next match did not move the highlight",
    )
    terminal.send(b"Nr")
    terminal.expect(
        lambda: (root / "runs").read_text().strip() == "3" and b"needle-3" in terminal.frame(),
        "manual restart did not relaunch the selected process",
    )
    terminal.send(b"c")
    terminal.expect(lambda: b"needle-3" not in terminal.frame(), "clear did not empty the selected tab")
    terminal.send(b"sf3")
    terminal.expect(
        lambda: b"Stream" in terminal.frame() and b"3:-vite" in terminal.frame()
        and b"needle-3" not in terminal.frame(),
        "stream filter did not hide Vite output",
    )
    terminal.send(b"\r3f3\rt")
    terminal.expect(lambda: b"Tabs" in terminal.frame(), "tab mode did not return")
    terminal.send(b"q")


def run(command, start_mode, rows=16):
    with tempfile.TemporaryDirectory(prefix="openrust-dev-terminal-") as directory:
        root = Path(directory)
        tools = root / "bin"
        tools.mkdir()
        npm = tools / "npm"
        npm.write_text(
            "#!/bin/sh\n"
            'last=$(cat runs 2>/dev/null || echo 0)\n'
            'if [ "$last" = 0 ]; then printf "early-1\\nneedle-1\\nneedle-2\\n"; echo 2 > runs; '
            'else next=$((last + 1)); printf "needle-%s\\n" "$next"; echo "$next" > runs; fi\n'
        )
        npm.chmod(0o755)
        (root / "package.json").write_text("{}")
        db = root / "jobs.sqlite"
        db.touch()
        port = available_port()
        env = dict(
            os.environ, APP_BASE_PATH=str(root), APP_ENV="local", SERVER_PORT=str(port),
            QUEUE_CONNECTION="database", DB_CONNECTION="sqlite", DB_DATABASE=str(db),
            TERM="xterm-256color", PATH=str(tools) + os.pathsep + os.environ["PATH"],
        )
        env.pop("NO_COLOR", None)
        subprocess.run(command + ["migrate", "--force"], env=env, check=True, capture_output=True, timeout=30)
        with Terminal(command + ["dev"] + start_mode, env, rows) as terminal:
            if rows < 8:
                terminal.expect(lambda: b"needle-2" in terminal.output, "small TTY did not fall back to inline")
                terminal.send(b"\x03")
            elif "--buffer-size=2" in start_mode:
                inspect_tabs(terminal, root, port)
            elif not start_mode or start_mode == ["--tabs"]:
                terminal.expect(lambda: b"Tabs" in terminal.frame(), "default tabs did not open")
                terminal.expect(lambda: b"vite [exited]" in terminal.frame(), "default tabs lost process state")
                terminal.send(b"q")
            else:
                terminal.expect(
                    lambda: b"Stream" in terminal.frame() and b"[vite] needle-2" in terminal.frame(),
                    "stream view did not show interleaved output",
                )
                terminal.send(b"\x03")
            terminal.finish()
            output = bytes(terminal.output)
            assert output.count(ENTER_SCREEN) == (rows >= 8)
            assert output.count(LEAVE_SCREEN) == (rows >= 8)
            if rows >= 8:
                replay = output.split(LEAVE_SCREEN, 1)[1]
                replay = re.sub(rb"\x1b\[[0-9;]*m", b"", replay)
                assert re.search(rb"\[vite\] needle-[23]", replay), output[-2000:]
        with socket.socket() as probe:
            assert probe.connect_ex(("127.0.0.1", port)) != 0, "HTTP server kept its port"


def main():
    command = sys.argv[1:] or ["./target/debug/main", "artisan"]
    run(command, ["--tabs", "--buffer-size=2", "--stream-buffer-size=64"])
    run(command, ["--stream"])
    run(command, ["--tabs"], rows=6)
    print("Artisan dev: tabs, stream, search, filter, restart, resize, quit, Ctrl+C and terminal restoration PASS")


if __name__ == "__main__":
    main()
