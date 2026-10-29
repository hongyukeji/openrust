#!/usr/bin/env python3
"""通过本次拥有的 TLS 终止代理验收 Cookie Secure；不启动或配置后端应用。

后端必须分别配置可信代理与不可信代理，Cookie factory secure 必须为 None。
客户端显式信任本次证书并校验 localhost；证据不包含 Cookie、密钥或原始头。
父进程强制 40 秒 worker 总期限，超时终止本次独立进程组；可另设 45 秒外层期限。
--offline-check 不启动网络或 openssl。
"""

import argparse
import hashlib
import http.client
import http.server
import io
import json
import os
from pathlib import Path
import socket
import signal
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from urllib.parse import urlsplit

PATH = "/api/example/cookie"
NAME = "example_preference"
VALUE = "tls-contract;plus+value"
TIMEOUT = 5
REQUEST_LIMIT = 16 * 1024
RESPONSE_LIMIT = 256 * 1024
TOTAL_SECONDS = 40
WORKER_ENV = "OPENRUST_COOKIE_TLS_OWNED_WORKER"


class VerificationError(Exception):
    """仅携带静态、脱敏的验收错误。"""


def ensure(condition, message):
    if not condition:
        raise VerificationError(message)


def parse_origin(raw):
    try:
        parsed = urlsplit(raw)
        port = parsed.port
        valid = (
            parsed.scheme == "http"
            and parsed.hostname in {"127.0.0.1", "::1"}
            and parsed.username is None
            and parsed.password is None
            and not parsed.path
            and not parsed.query
            and not parsed.fragment
            and port is not None
            and 1 <= port <= 65535
        )
    except ValueError as error:
        raise VerificationError("后端必须为带显式端口的数字 loopback HTTP origin") from error
    ensure(valid, "后端必须为无路径、凭据、查询或片段的数字 loopback HTTP origin")
    host = f"[{parsed.hostname}]" if parsed.hostname == "::1" else parsed.hostname
    ensure(raw == f"http://{host}:{port}", "后端 origin 格式必须规范且无附加字符")
    return parsed.hostname, port, f"{host}:{port}"


def parse_output(raw):
    path = Path(raw)
    ensure(path.is_absolute(), "证据路径必须为绝对路径")
    ensure(path.parent.resolve() == Path("/tmp").resolve(), "证据文件必须直接位于 /tmp")
    ensure(path.name.startswith("openrust-c04c-tls-") and path.name.endswith(".json"),
           "证据文件必须使用 openrust-c04c-tls- 前缀及 .json 后缀")
    return path


def encode_document(document):
    return json.dumps(document, ensure_ascii=False, sort_keys=True, indent=2).encode("utf-8") + b"\n"


class BudgetReader:
    """在读取请求或响应头之前即限制总字节数。"""

    def __init__(self, stream, limit):
        self.stream = stream
        self.remaining = limit

    def _size(self, size):
        return min(size, self.remaining + 1) if size >= 0 else self.remaining + 1

    def _account(self, data):
        self.remaining -= len(data)
        if self.remaining < 0:
            raise VerificationError("HTTP 消息超过本工具字节上限")
        return data

    def read(self, size=-1):
        return self._account(self.stream.read(self._size(size)))

    def readline(self, size=-1):
        return self._account(self.stream.readline(self._size(size)))

    def read1(self, size=-1):
        return self.read(size)

    def readinto(self, buffer):
        data = self.read(len(buffer))
        buffer[:len(data)] = data
        return len(data)

    def __getattr__(self, name):
        return getattr(self.stream, name)


class BoundedResponse(http.client.HTTPResponse):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.fp = BudgetReader(self.fp, RESPONSE_LIMIT)


def exchange(connection, method, payload=None, cookies=None, forwarded=None):
    connection.response_class = BoundedResponse
    body = None if payload is None else json.dumps(payload, separators=(",", ":")).encode("utf-8")
    headers = {"Connection": "close"}
    if body is not None:
        headers["Content-Type"] = "application/json"
    if cookies is not None:
        headers["Cookie"] = cookies
    if forwarded is not None:
        headers["X-Forwarded-Proto"] = forwarded
    request_size = len(body or b"") + len(PATH) + sum(len(k) + len(v) + 4 for k, v in headers.items()) + 128
    ensure(request_size <= REQUEST_LIMIT, "请求超过 16 KiB 上限")
    try:
        connection.request(method, PATH, body=body, headers=headers)
        response = connection.getresponse()
        ensure(not 300 <= response.status < 400, "后端发生重定向")
        document = json.loads(response.read(RESPONSE_LIMIT + 1))
        return response.status, document, response.getheaders()
    finally:
        connection.close()


def backend_exchange(origin, method, payload=None, cookies=None, forwarded=None):
    address, port, _ = origin
    return exchange(http.client.HTTPConnection(address, port, timeout=TIMEOUT), method,
                    payload, cookies, forwarded)


class OwnedTLSProxy(http.server.ThreadingHTTPServer):
    daemon_threads = False
    block_on_close = True
    request_queue_size = 4

    def __init__(self, context, origin):
        self.tls_context = context
        self.origin = origin
        self.slots = threading.BoundedSemaphore(4)
        super().__init__(("127.0.0.1", 0), ProxyHandler)

    def get_request(self):
        raw, address = self.socket.accept()
        raw.settimeout(TIMEOUT)
        try:
            return self.tls_context.wrap_socket(raw, server_side=True), address
        except BaseException:
            raw.close()
            raise

    def process_request(self, request, client_address):
        if not self.slots.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.slots.release()

    def handle_error(self, request, client_address):
        # 禁止标准 server traceback 回显请求或原始头。
        pass


class ProxyHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def setup(self):
        super().setup()
        self.connection.settimeout(TIMEOUT)
        self.rfile = BudgetReader(self.rfile, REQUEST_LIMIT)

    def log_message(self, format_string, *args):
        pass

    def do_GET(self):
        self.forward()

    def do_POST(self):
        self.forward()

    def forward(self):
        try:
            ensure(self.path == PATH, "代理仅允许固定示例路径")
            ensure(self.headers.get("Transfer-Encoding") is None, "代理不接受传输编码")
            length = int(self.headers.get("Content-Length", "0"))
            ensure(0 <= length <= REQUEST_LIMIT, "请求体超过上限")
            body = self.rfile.read(length)
            ensure(len(body) == length, "请求体提前结束")
            cookies = self.headers.get("Cookie")
            ensure(cookies is None or not any(ord(c) < 32 or ord(c) == 127 for c in cookies),
                   "Cookie 请求头无效")
            connection = http.client.HTTPConnection(*self.server.origin[:2], timeout=TIMEOUT)
            connection.response_class = BoundedResponse
            try:
                headers = {"Host": self.server.origin[2], "X-Forwarded-Proto": "https", "Connection": "close"}
                if body:
                    headers["Content-Type"] = "application/json"
                if cookies is not None:
                    headers["Cookie"] = cookies
                connection.request(self.command, PATH, body=body or None, headers=headers)
                response = connection.getresponse()
                ensure(not 300 <= response.status < 400, "后端发生重定向")
                response_body = response.read(RESPONSE_LIMIT + 1)
                response_headers = response.getheaders()
                self.send_response(response.status)
                for key, value in response_headers:
                    if key.lower() in {"set-cookie", "content-type"}:
                        self.send_header(key, value)
                self.send_header("Content-Length", str(len(response_body)))
                self.send_header("Connection", "close")
                self.end_headers()
                self.wfile.write(response_body)
            finally:
                connection.close()
        except Exception:
            self.send_error(502, "TLS verification proxy failed")
        finally:
            self.close_connection = True


def tls_exchange(port, context, method, payload=None, cookies=None):
    # 数字地址连接且显式以 localhost 校验 SAN，不依赖 localhost DNS。
    raw = socket.create_connection(("127.0.0.1", port), timeout=TIMEOUT)
    try:
        secure = context.wrap_socket(raw, server_hostname="localhost")
    except BaseException:
        raw.close()
        raise
    version = secure.version()
    cipher = secure.cipher()
    connection = http.client.HTTPConnection("localhost", port, timeout=TIMEOUT)
    connection.sock = secure
    result = exchange(connection, method, payload, cookies)
    ensure(version in {"TLSv1.2", "TLSv1.3"}, "实际协商协议低于 TLS 1.2")
    return result, {"version": version, "cipher": cipher[0], "hostname_verified": True,
                    "explicit_certificate_trust": True}


def expect_success(result, expected):
    status, document, _ = result
    ensure(status == 200, "响应状态不是 200")
    ensure(document == {"success": True, "code": "ok", "message": "ok", "data": expected},
           "完整成功响应封装不匹配")


def cookie_pair(result, secure_expected):
    matches = []
    for key, header in result[2]:
        if key.lower() != "set-cookie":
            continue
        parts = header.split(";")
        if parts[0].split("=", 1)[0] == NAME:
            matches.append(parts)
    ensure(len(matches) == 1, "示例 Cookie 缺失或重复")
    parts = matches[0]
    ensure("=" in parts[0] and bool(parts[0].split("=", 1)[1]), "示例 Cookie 值为空")
    attrs = {}
    for item in parts[1:]:
        key, separator, value = item.strip().partition("=")
        key = key.lower()
        ensure(key not in attrs, "Cookie 属性重复")
        attrs[key] = value if separator else None
    ensure(("secure" in attrs) == secure_expected, "Cookie Secure 与可信代理语义不匹配")
    ensure("secure" not in attrs or attrs["secure"] is None, "Secure 必须为标志属性")
    ensure(attrs.get("path") == "/api/example", "Cookie Path 不匹配")
    ensure("httponly" in attrs and attrs["httponly"] is None, "Cookie HttpOnly 不匹配")
    ensure(attrs.get("samesite", "").lower() == "lax", "Cookie SameSite 不匹配")
    ensure(0 < int(attrs.get("max-age", "0")) <= 900, "Cookie Max-Age 不匹配")
    expires = parsedate_to_datetime(attrs.get("expires", ""))
    ensure(expires.tzinfo is not None and expires > datetime.now(timezone.utc), "Cookie Expires 不匹配")
    return parts[0]


def create_certificate(directory):
    cert = Path(directory) / "certificate.pem"
    key = Path(directory) / "private-key.pem"
    command = ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-sha256", "-nodes",
               "-days", "1", "-subj", "/CN=localhost", "-addext",
               "subjectAltName=DNS:localhost,IP:127.0.0.1", "-keyout", str(key), "-out", str(cert)]
    process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        ensure(process.wait(timeout=10) == 0, "openssl 证书生成失败")
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
        raise VerificationError("openssl 超过 10 秒期限") from None
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
    return cert, key


def run(trusted, untrusted, report, directory):
    report["stage"] = "certificate_generation"
    cert, key = create_certificate(directory)
    server_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    server_context.minimum_version = ssl.TLSVersion.TLSv1_2
    server_context.load_cert_chain(cert, key)
    client_context = ssl.create_default_context(cafile=str(cert))
    client_context.minimum_version = ssl.TLSVersion.TLSv1_2
    ensure(client_context.check_hostname and client_context.verify_mode == ssl.CERT_REQUIRED,
           "TLS 客户端必须校验证书和主机名")
    server = OwnedTLSProxy(server_context, trusted)
    worker = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.1},
                              name="openrust-c04c-tls-proxy", daemon=False)
    started = False
    try:
        worker.start()
        started = True
        report["stage"] = "tls_trusted_https_post"
        result, negotiation = tls_exchange(server.server_port, client_context, "POST", {"action": "queue", "value": VALUE})
        report["tls"] = negotiation
        expect_success(result, {"action": "queue", "queued": True})
        pair = cookie_pair(result, True)
        report["stage"] = "tls_opaque_get_roundtrip"
        readback, _ = tls_exchange(server.server_port, client_context, "GET", cookies=pair)
        expect_success(readback, {"preference": VALUE, "present": True})
        report["stage"] = "untrusted_spoofed_https_post"
        spoofed = backend_exchange(untrusted, "POST", {"action": "direct", "value": VALUE}, forwarded="https")
        expect_success(spoofed, {"action": "direct", "queued": False})
        cookie_pair(spoofed, False)
        report["stage"] = "trusted_forwarded_http_post"
        plain = backend_exchange(trusted, "POST", {"action": "direct", "value": VALUE}, forwarded="http")
        expect_success(plain, {"action": "direct", "queued": False})
        cookie_pair(plain, False)
        return {"stage": "completed", "tls": negotiation, "certificate_sha256": hashlib.sha256(cert.read_bytes()).hexdigest(),
                "checks": {"tls_trusted_https_secure": True, "opaque_roundtrip": True,
                           "untrusted_spoofed_https_not_secure": True, "trusted_http_not_secure": True,
                           "full_envelopes": True, "named_cookie_attributes": True}}
    finally:
        if started:
            server.shutdown()
        server.server_close()
        if started:
            worker.join(timeout=TIMEOUT + 1)
            ensure(not worker.is_alive(), "TLS 代理线程未能终止")



def check_owned_deadline():
    """离线回归：非 daemon 线程和后代进程也不能续命超过监督期限。"""
    with tempfile.TemporaryDirectory(prefix="openrust-c04c-tls-deadline-", dir="/tmp") as directory:
        pidfile = Path(directory) / "pids.json"
        program = (
            "import json,os,pathlib,subprocess,sys,threading,time; "
            "child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)']); "
            "stream=pathlib.Path(sys.argv[1]).open('x'); "
            "json.dump([os.getpid(),child.pid],stream); stream.close(); "
            "threading.Thread(target=lambda:time.sleep(30),daemon=False).start(); time.sleep(30)"
        )
        started = time.monotonic()
        try:
            supervise([sys.executable, "-c", program, str(pidfile)], 0.5)
        except subprocess.TimeoutExpired:
            pass
        else:
            raise VerificationError("受控进程未触发总期限")
        ensure(time.monotonic() - started < 3, "受控进程超时清理未能及时结束")
        pids = json.loads(pidfile.read_text())
        # 只检查本次合成进程，不列出或终止其它进程。
        for _ in range(20):
            alive = []
            for pid in pids:
                try:
                    os.kill(pid, 0)
                    alive.append(pid)
                except ProcessLookupError:
                    pass
            if not alive:
                break
            time.sleep(0.05)
        ensure(not alive, "受控进程组仍存在未退出成员")
        try:
            os.waitpid(pids[0], os.WNOHANG)
        except ChildProcessError:
            pass
        else:
            raise VerificationError("监督器未回收直接子进程")


def offline_check():
    for origin in ["http://127.0.0.1:1234", "http://[::1]:1234"]:
        parse_origin(origin)
    for origin in ["https://127.0.0.1:1234", "http://localhost:1234", "http://127.0.0.1",
                   "http://127.0.0.1:1234/", "http://user@127.0.0.1:1234", "http://127.0.0.1:1234?q=1",
                   "http://127.0.0.1:1234#x", "http://192.0.2.1:1234", "http://127.0.0.1:0", "http://127.0.0.1:1234?",
                   "http://127.0.0.1:1234#", "http://127.0.0.1:1234\n"]:
        try:
            parse_origin(origin)
        except VerificationError:
            continue
        raise VerificationError("非法 origin 离线验证未拒绝")
    ensure(json.loads(encode_document({"checks": {"offline": True}})) == {"checks": {"offline": True}},
           "证据序列化离线验证失败")
    ensure(parse_output("/tmp/openrust-c04c-tls-offline.json").name.endswith(".json"), "输出路径检查失败")
    reader = BudgetReader(io.BytesIO(b"12345"), 4)
    try:
        reader.read()
    except VerificationError:
        pass
    else:
        raise VerificationError("字节预算离线验证失败")
    # 仅核对属性读取器；此合成消息不是 TLS 或真实后端证据。
    future = "Wed, 01 Jan 2042 00:00:00 GMT"
    header = f"{NAME}=opaque; path=/api/example; secure; httponly; samesite=lax; Max-Age=900; expires={future}"
    ensure(cookie_pair((200, {}, [("Set-Cookie", header)]), True) == f"{NAME}=opaque", "属性检查离线验证失败")
    check_owned_deadline()


class DeadlineExceeded(VerificationError):
    """本次 worker 的整个进程组已因绝对期限被终止并回收。"""


def supervise(command, timeout, environment=None):
    """监督本次创建的独立进程组；期限包含线程关闭和 openssl 等待。"""
    process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.DEVNULL, env=environment, start_new_session=True)
    try:
        output, _ = process.communicate(timeout=timeout)
    except BaseException:
        # start_new_session 保证 PGID 等于本次直接子进程 PID；不终止调用方或后端组。
        # 超时路径尚未 wait/reap，PID 仍归本次 Popen 所有，不存在复用后的 PGID 猜测。
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.communicate()
        raise
    finally:
        if process.returncode is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
    ensure(process.returncode == 0, "受管 worker 未正常完成")
    return output


def execute_worker(args):
    ensure(os.environ.get(WORKER_ENV) == "1" and not args.output and not args.offline_check,
           "内部 worker 不接受外部输出或递归启动")
    trusted, untrusted = parse_origin(args.trusted_origin), parse_origin(args.untrusted_origin)
    directory = Path(args.owned_directory)
    ensure(directory.parent.resolve() == Path("/tmp").resolve()
           and directory.name.startswith("openrust-c04c-tls-") and directory.is_dir(),
           "内部 worker 临时目录无效")
    report = {"status": "failed"}
    try:
        report.update(run(trusted, untrusted, report, directory))
        report["status"] = "passed"
    except Exception as error:
        report["failure_class"] = type(error).__name__
    sys.stdout.buffer.write(encode_document(report))
    sys.stdout.buffer.flush()
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trusted-origin")
    parser.add_argument("--untrusted-origin")
    parser.add_argument("--output")
    parser.add_argument("--offline-check", action="store_true")
    parser.add_argument("--owned-worker", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--owned-directory", help=argparse.SUPPRESS)
    args = parser.parse_args()
    try:
        if args.owned_worker:
            return execute_worker(args)
        ensure(os.environ.get(WORKER_ENV) != "1" and args.owned_directory is None,
               "禁止内部 worker 递归启动监督器")
        if args.offline_check:
            ensure(not any([args.trusted_origin, args.untrusted_origin, args.output]), "离线检查不接受网络或输出参数")
            offline_check()
            print("offline checks passed; no network started")
            return 0
        ensure(all([args.trusted_origin, args.untrusted_origin, args.output]), "必须提供两个后端 origin 和新证据路径")
        trusted, untrusted = parse_origin(args.trusted_origin), parse_origin(args.untrusted_origin)
        ensure(trusted != untrusted, "可信与不可信后端必须分别提供")
        output = parse_output(args.output)
        # O_EXCL 先于 worker、证书生成与网络，父进程独占证据和临时目录。
        descriptor = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as evidence:
            report = {"tool": "openrust-cookie-security", "status": "failed", "timestamp": int(time.time()),
                      "stage": "supervised_worker", "worker_deadline_seconds": TOTAL_SECONDS,
                      "scope": "owned loopback TLS proxy; not cluster acceptance",
                      "precondition": "backend factory secure=None; trusted proxy configurations differ"}
            try:
                with tempfile.TemporaryDirectory(prefix="openrust-c04c-tls-", dir="/tmp") as directory:
                    environment = dict(os.environ)
                    environment[WORKER_ENV] = "1"
                    command = [sys.executable, str(Path(__file__).resolve()), "--owned-worker",
                               "--owned-directory", directory, "--trusted-origin", args.trusted_origin,
                               "--untrusted-origin", args.untrusted_origin]
                    try:
                        result = supervise(command, TOTAL_SECONDS, environment)
                    except subprocess.TimeoutExpired:
                        raise DeadlineExceeded("本次 worker 进程组超过绝对期限并已终止") from None
                    report.update(json.loads(result))
            except Exception as error:
                report["status"] = "failed"
                report["failure_class"] = type(error).__name__
                if isinstance(error, DeadlineExceeded):
                    report["stage"] = "whole_process_deadline"
                    report["owned_process_group_terminated"] = True
                    report["worker_reaped"] = True
            evidence.write(encode_document(report))
            evidence.flush()
        passed = report["status"] == "passed"
        print("cookie TLS verification passed" if passed else "cookie TLS verification failed; see safe evidence")
        return 0 if passed else 1
    except Exception:
        print("cookie TLS verification rejected parameters or output; no sensitive details emitted", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
