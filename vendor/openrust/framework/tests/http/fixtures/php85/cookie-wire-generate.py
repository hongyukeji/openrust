#!/usr/bin/env python3
"""采集固定 PHP 8.5 HTTP SAPI 的 Cookie bag；仅写新的本簇 fixture。"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time


PHP = Path("/tmp/openrust-c04b2-php-runtime/source/php-8.5.0/sapi/cli/php")
HERE = Path(__file__).resolve().parent
ROUTER = HERE / "cookie-wire-router.php"
FIXTURE = HERE / "cookie-wire-golden.json"
PROVENANCE = HERE / "cookie-provenance.json"
MAX_CASES = 32
MAX_REQUEST_BYTES = 4096
MAX_RESPONSE_BYTES = 65536
TIMEOUT = 2.0

CASES: list[tuple[str, list[bytes]]] = [
    ("plain", [b"plain=value"]),
    ("duplicate_plain_first", [b"dup=first; dup=second"]),
    ("duplicate_headers", [b"duph=first", b"duph=second"]),
    ("duplicate_nested", [b"nest[key]=first; nest[key]=second"]),
    ("scalar_then_array", [b"mix=scalar; mix[key]=array"]),
    ("array_then_scalar", [b"mix[key]=array; mix=scalar"]),
    ("append", [b"list[]=one; list[]=two"]),
    ("negative_index", [b"idx[-1]=negative; idx[0]=zero"]),
    ("dot_and_space", [b"a.b=dot; a b=space"]),
    ("name_not_url_decoded", [b"a%2Eb=encoded; a.b=dot"]),
    ("value_plus", [b"plus=a+b; encoded=a%2Bb; space=a%20b"]),
    ("value_nul", [b"nul=ab%00cd"]),
    ("value_invalid_utf8", [b"binary=%FF"]),
    ("malformed_unclosed", [b"broken[key=value"]),
    ("malformed_suffix", [b"broken[key]tail=value"]),
    ("malformed_closing", [b"broken]key=value"]),
    ("host_secure_prefix", [b"__Host-id=host; __Secure-id=secure"]),
    ("bare_empty_equals", [b"bare; empty=; equals=a=b"]),
    ("leading_whitespace", [b"  spaced=value;\t tabbed=other"]),
    ("case_sensitive", [b"Case=upper; case=lower"]),
    ("nested_two_levels", [b"root[nested][item]=value"]),
    ("literal_percent_name", [b"percent%25name=value"]),
]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def php_version() -> str:
    run = subprocess.run(
        [str(PHP), "-n", "-r", "echo PHP_VERSION;"],
        check=True,
        capture_output=True,
        timeout=TIMEOUT,
        env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
    )
    return run.stdout.decode("ascii")


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def send(port: int, headers: list[bytes]) -> tuple[int, bytes]:
    request = b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n"
    for value in headers:
        if b"\r" in value or b"\n" in value:
            raise ValueError("Cookie value contains a line break")
        request += b"Cookie: " + value + b"\r\n"
    request += b"\r\n"
    if len(request) > MAX_REQUEST_BYTES:
        raise ValueError("request exceeds budget")
    with socket.create_connection(("127.0.0.1", port), timeout=TIMEOUT) as client:
        client.settimeout(TIMEOUT)
        client.sendall(request)
        chunks = []
        size = 0
        while True:
            piece = client.recv(4096)
            if not piece:
                break
            size += len(piece)
            if size > MAX_RESPONSE_BYTES:
                raise ValueError("response exceeds budget")
            chunks.append(piece)
    response = b"".join(chunks)
    head, sep, body = response.partition(b"\r\n\r\n")
    if not sep:
        raise ValueError("response lacks header boundary")
    status_line = head.split(b"\r\n", 1)[0]
    status = int(status_line.split(b" ", 2)[1])
    return status, body


def wait_ready(port: int, process: subprocess.Popen[bytes]) -> None:
    deadline = time.monotonic() + 3.0
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("PHP server exited before readiness")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                return
        except OSError:
            time.sleep(0.05)
    raise TimeoutError("PHP server did not become ready")


def main() -> None:
    if FIXTURE.exists() or PROVENANCE.exists():
        raise FileExistsError("cookie fixture already exists; refusing to overwrite")
    if len(CASES) > MAX_CASES:
        raise ValueError("case count exceeds budget")
    version = php_version()
    if version != "8.5.0":
        raise RuntimeError(f"unexpected PHP version: {version}")
    port = free_port()
    log_fd, log_name = tempfile.mkstemp(prefix="openrust-c04c-php-", suffix=".log")
    process: subprocess.Popen[bytes] | None = None
    try:
        with os.fdopen(log_fd, "wb") as log:
            process = subprocess.Popen(
                [str(PHP), "-n", "-S", f"127.0.0.1:{port}", str(ROUTER)],
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
                env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
            )
            wait_ready(port, process)
            rows = []
            for case_id, headers in CASES:
                status, body = send(port, headers)
                if status != 200 or not body:
                    raise RuntimeError(f"{case_id}: HTTP {status} or empty body")
                result = json.loads(body)
                rows.append(
                    {
                        "id": case_id,
                        "cookie_headers_latin1": [value.decode("latin-1") for value in headers],
                        "status": status,
                        "result": result,
                    }
                )
    finally:
        if process is not None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)
        os.unlink(log_name)

    fixture = {
        "evidence": "PHP 8.5.0 -n built-in HTTP SAPI, raw socket Cookie headers, actual $_COOKIE",
        "cases": rows,
    }
    binary_count = sum(row["result"].get("type") == "binary_tree" for row in rows)
    if binary_count != 1:
        raise RuntimeError(f"expected one explicit binary result, got {binary_count}")
    FIXTURE.write_text(json.dumps(fixture, ensure_ascii=False, indent=2) + "\n")
    provenance = {
        "php_version": version,
        "php_source_archive": "https://www.php.net/distributions/php-8.5.0.tar.xz",
        "php_source_archive_sha256": "39cb6e4acd679b574d3d3276f148213e935fc25f90403eb84fb1b836a806ef1e",
        "php_binary_sha256": sha256(PHP),
        "router_sha256": sha256(ROUTER),
        "generator_sha256": sha256(Path(__file__)),
        "fixture_sha256": sha256(FIXTURE),
        "transport": "localhost built-in PHP HTTP server, raw socket requests with Cookie headers; not parse_str",
        "binary_boundary": "invalid UTF-8 makes json_encode($_COOKIE) fail; router then emits typed base64 leaf bytes",
        "cases": len(rows),
        "invalid_utf8_cases": binary_count,
        "max_input_vars_note": "PHP max_input_vars=1000 may truncate; Rust cookie budgets reject rather than truncate and need separate tests",
    }
    PROVENANCE.write_text(json.dumps(provenance, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"cases": len(rows), "invalid_utf8_cases": binary_count, "fixture_sha256": sha256(FIXTURE)}))


if __name__ == "__main__":
    main()
