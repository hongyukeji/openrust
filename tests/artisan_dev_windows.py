#!/usr/bin/env python3
"""Exercise Windows Artisan console signals and the supervised process tree."""

import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request


def checked(result):
    if not result:
        raise ctypes.WinError(ctypes.get_last_error())


def interrupt(pid):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.FreeConsole()
    checked(kernel.AttachConsole(pid))
    checked(kernel.SetConsoleCtrlHandler(None, True))
    checked(kernel.GenerateConsoleCtrlEvent(0, 0))


def descendants_exited(pids):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
    for pid in pids:
        handle = kernel.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE
        if not handle:
            assert ctypes.get_last_error() == 87, ctypes.WinError(ctypes.get_last_error())
            continue
        try:
            assert kernel.WaitForSingleObject(handle, 3000) == 0, f"process {pid} survived"
        finally:
            kernel.CloseHandle(handle)


def events_in(file):
    lines = file.read_text(encoding="utf-8").splitlines()
    return [json.loads(line) for line in lines if line.endswith("}")]


def running_console(command, env, stdout, stderr):
    return subprocess.Popen(
        command, env=env, stdout=stdout, stderr=stderr,
        creationflags=subprocess.CREATE_NEW_CONSOLE,
    )


def run(binary, fail):
    with tempfile.TemporaryDirectory(prefix="openrust-dev-windows-") as directory:
        root = Path(directory)
        tools = root / "bin"
        tools.mkdir()
        (root / ".env").write_text("APP_NAME=WindowsDevelopment\nAPP_ENV=local\n", encoding="utf-8")
        (root / "package.json").write_text("{}", encoding="utf-8")
        (tools / "npm.cmd").write_text(
            '@echo off\n"%DEV_TEST_PYTHON%" -B "%DEV_TEST_SCRIPT%" asset\nexit /b %ERRORLEVEL%\n',
            encoding="utf-8",
        )
        db = root / "jobs.sqlite"
        db.touch()
        with socket.socket() as candidate:
            candidate.bind(("127.0.0.1", 0))
            port = candidate.getsockname()[1]
        env = dict(
            os.environ, APP_BASE_PATH=str(root), APP_ENV="local", SERVER_PORT=str(port),
            QUEUE_CONNECTION="database", DB_CONNECTION="sqlite", DB_DATABASE=str(db),
            DEV_TEST_PYTHON=sys.executable, DEV_TEST_SCRIPT=str(Path(__file__).resolve()),
            PATH=str(tools) + os.pathsep + os.environ["PATH"],
        )
        subprocess.run(
            [binary, "artisan", "migrate", "--force"], env=env, check=True,
            capture_output=True, timeout=45,
        )
        output, errors = root / "stdout.txt", root / "stderr.txt"
        args = [binary, "artisan", "dev", "--json"]
        if fail:
            args.append("--no-restart")
        with output.open("wb") as out, errors.open("wb") as err:
            with running_console(args, env, out, err) as parent:
                try:
                    deadline = time.monotonic() + 45
                    observed = []
                    while time.monotonic() < deadline and parent.poll() is None:
                        observed = events_in(output)
                        asset = (root / "asset.pid").is_file()
                        queue = any("Processing jobs" in event.get("text", "") for event in observed)
                        try:
                            with urllib.request.urlopen(f"http://127.0.0.1:{port}/up", timeout=.5) as response:
                                if response.status == 200 and asset and queue:
                                    break
                        except OSError:
                            pass
                        time.sleep(.05)
                    else:
                        raise AssertionError((observed, errors.read_text(encoding="utf-8")))
                    pids = [event["pid"] for event in observed if event["type"] == "start"]
                    pids.append(int((root / "asset.pid").read_text(encoding="utf-8")))
                    if fail:
                        (root / "asset-fail").touch()
                    else:
                        subprocess.run(
                            [sys.executable, "-B", __file__, "interrupt", str(parent.pid)],
                            check=True, timeout=15, creationflags=subprocess.DETACHED_PROCESS,
                        )
                    assert parent.wait(timeout=15) == (9 if fail else 0)
                    final = events_in(output)
                    assert final[-1]["type"] == "done", final
                    assert final[-1]["code"] == (9 if fail else 0), final
                    if fail:
                        assert any(event.get("reason") == "restart-disabled" for event in final), final
                    else:
                        assert any(
                            event.get("label") == "server" and event["type"] == "exit" and event["code"] == 0
                            for event in final
                        ), final
                    descendants_exited(pids)
                finally:
                    if parent.poll() is None:
                        parent.kill()
                        parent.wait(timeout=5)
        with socket.socket() as probe:
            assert probe.connect_ex(("127.0.0.1", port)) != 0, "HTTP still listening"


def asset():
    root = Path(os.environ["APP_BASE_PATH"])
    # This descendant has its own console and cannot receive the supervisor's Ctrl+Break.
    child = subprocess.Popen(
        [sys.executable, "-c", "import time; time.sleep(120)"],
        creationflags=subprocess.CREATE_NEW_CONSOLE,
    )
    (root / "asset.pid").write_text(str(child.pid), encoding="utf-8")
    print("asset started", flush=True)
    while not (root / "asset-fail").is_file():
        time.sleep(.05)
    return 9


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "interrupt":
        interrupt(int(sys.argv[2]))
    elif len(sys.argv) > 1 and sys.argv[1] == "asset":
        return asset()
    else:
        binary = str(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/main.exe").resolve())
        run(binary, fail=False)
        run(binary, fail=True)
        print("Windows Artisan dev: real Ctrl+C, HTTP, queue, Job descendants and no-restart PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
