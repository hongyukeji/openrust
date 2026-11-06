#!/usr/bin/env python3
"""D6 入站 URL 真实 TCP / TLS 终止反代验收；不构建、不连接数据库。

传入已验收二进制及 SHA256。必须由 --source-snapshot 提供构建时受控的路径->SHA256 JSON 快照。
TLS 在本工具拥有的反代终止，Rust server 仅通过可信 peer/proto 判断安全；
不能据此声明原生 Rust TLS adapter 已验收。结果只记录布尔断言与脱敏资源信息。
"""
import argparse
import hashlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import sys
import signal
import socket
import ssl
import subprocess
import tempfile
import threading
import time

TOOL_SHA = '02f89c887a4bd5d8c044f63f4410eadc24614a807b3e2c4c5b5171b709780a93'
PATH = '/api/example/request-url'
QUERY = '?page=1&label=public'
TIMEOUT = 5
TOTAL = 120
WORKER_LIMIT = 90
KEY = 'KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio='


class Failure(Exception):
    """仅携带静态验收标签。"""


def ensure(condition, tag):
    if not condition:
        raise Failure(tag)


def sha(path):
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def snapshot(root):
    paths = set()
    for extra in ([], ['--others', '--exclude-standard']):
        result = subprocess.run(['git', 'ls-files', '-z', *extra], cwd=root,
                                capture_output=True, check=True, timeout=10)
        paths.update(os.fsdecode(p) for p in result.stdout.split(b'\0') if p)
    return {p: sha(root / p) for p in sorted(paths) if (root / p).is_file()}


def resources(root):
    sizes = {}
    target = Path(os.environ.get('CARGO_TARGET_DIR', root / 'target'))
    if not target.is_absolute():
        target = root / target
    paths = [target, root / 'target', *sorted((root / 'tests/consumers').glob('*/target'))]
    for path in paths:
        if path.exists():
            resolved = str(path.resolve())
            if resolved in sizes:
                continue
            result = subprocess.run(['du', '-sk', resolved], capture_output=True,
                                    text=True, check=True, timeout=10)
            sizes[resolved] = int(result.stdout.split()[0]) * 1024
    stat = os.statvfs(root)
    free, total = stat.f_bavail * stat.f_frsize, stat.f_blocks * stat.f_frsize
    subtotal = sum(sizes.values())
    ensure(subtotal < 100 * 1000**3 and free / total >= .05,
           'resource_hard_limit')
    return {'target_bytes': sizes, 'disk_free_bytes': free, 'disk_free_fraction': free / total,
            'partial_target_total_bytes': subtotal,
            'coverage': '当前worktree目标、CARGO_TARGET_DIR及本worktree consumer目标；不含其他worktree',
            'soft_warning': subtotal >= 80 * 1000**3 or free / total < .08}


def listener_owned(port, process):
    result = subprocess.run(['/usr/sbin/lsof', '-nP', f'-iTCP@127.0.0.1:{port}',
                             '-sTCP:LISTEN', '-Fp'], capture_output=True, text=True, timeout=2)
    pids = [int(line[1:]) for line in result.stdout.splitlines()
            if line.startswith('p') and line[1:].isdigit()]
    if result.returncode or not pids:
        return False
    try:
        return all(os.getpgid(pid) == os.getpgrp() for pid in pids)
    except ProcessLookupError:
        return False


def group_members(pgid):
    output = subprocess.run(['/bin/ps', '-A', '-o', 'pid=,pgid=,stat='],
                            capture_output=True, text=True, check=True, timeout=2).stdout
    members = []
    for line in output.splitlines():
        row = line.split()
        ensure(len(row) == 3, 'process_scan_invalid')
        if int(row[1]) == pgid and not row[2].startswith('Z'):
            try:
                if os.getpgid(int(row[0])) == pgid:
                    members.append(int(row[0]))
            except ProcessLookupError:
                pass
    return members


def stop(process):
    # server 为本 worker 直接创建且无 fork 的二进制；只对该 Popen 发送信号。
    forced = False
    if process.poll() is None:
        ensure(os.getpgid(process.pid) == os.getpgrp(), 'server_left_supervised_group')
        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=12)
        except subprocess.TimeoutExpired:
            forced = True
            process.kill()
            process.wait(timeout=3)
    return {'pid': process.pid, 'code': process.returncode, 'forced': forced}


def validate_owned_directory(owned):
    # 写入和清理都只接受本 supervisor 在 /tmp 中新建的私有目录。
    ensure(not owned.is_symlink()
           and owned.parent.resolve() == Path('/tmp').resolve()
           and owned.name.startswith('openrust-c04d6-url-network-'), 'cleanup_directory_invalid')
    ensure(owned.is_dir() and owned.stat().st_mode & 0o777 == 0o700, 'owned_directory_mode')


def clear_sensitive(owned):
    validate_owned_directory(owned)
    for label in ('trusted', 'untrusted'):
        cwd = owned / label
        config = owned / label / 'config'
        if config.exists():
            shutil.rmtree(config)
        (owned / label / '.env').unlink(missing_ok=True)
        module = cwd / 'modules/example'
        if module.is_symlink():
            module.unlink()
        if (cwd / 'modules').is_dir():
            (cwd / 'modules').rmdir()
    for name in ('private-key.pem', 'certificate.pem'):
        (owned / name).unlink(missing_ok=True)


def prepare(root, owned, trusted):
    validate_owned_directory(owned)
    with socket.socket() as reserved:
        reserved.bind(('127.0.0.1', 0))
        port = reserved.getsockname()[1]
    cwd = owned / ('trusted' if trusted else 'untrusted')
    config = cwd / 'config'
    config.mkdir(parents=True)
    (cwd / '.env').write_text('')
    # 真实宿主使用已编译代码配置；只向本次子进程提供明确环境覆盖。
    # 默认 registered 挂载已编译示例，不制造根状态文件或依赖旧 TOML 骨架。
    profile = {
        'SERVER_PORT': str(port),
        'APP_SERVER_HOST': '127.0.0.1',
        'APP_SERVER_TRUSTED_PROXY_RANGES': '["127.0.0.1/32"]' if trusted else '[]',
        'APP_URL': f'http://localhost:{port}',
        'MODULES_ACTIVATOR': 'registered',
        'CACHE_STORE': 'default',
        'APP_CACHE_STORES_DEFAULT_DRIVER': 'memory',
        'SESSION_DRIVER': 'null',
        'APP_DATABASE_TABLE_PREFIX': 'tb_',
        'APP_SECURITY_SIGNED_LINKS_KEYS_LOCAL': KEY,
        'APP_SECURITY_ENCRYPTION_KEYS_LOCAL': KEY,
    }
    (cwd / 'modules').mkdir()
    (cwd / 'modules/example').symlink_to(root / 'modules/example', target_is_directory=True)
    ensure(not (cwd / 'modules_statuses.json').exists()
           and (cwd / 'modules/example').resolve() == root / 'modules/example',
           'module_root_not_isolated')
    ensure(profile['MODULES_ACTIVATOR'] == 'registered' and not any(config.iterdir()),
           'registered_fixture_requires_no_file_config')
    ensure(not any(key.startswith('DB_') or key.endswith('DATABASE_URL') for key in profile),
           'database_not_isolated')
    ensure(profile['SESSION_DRIVER'] == 'null', 'session_not_isolated')
    return cwd, config, port, profile


def server_environment(config, profile):
    """只把受控配置交给本次服务器，不继承调用方凭据或部署开关。"""
    inherited = {key: value for key, value in os.environ.items()
                 if key in ('PATH', 'HOME', 'TMPDIR')}
    return dict(inherited, APP_CONFIG_DIR=str(config), APP_ENV='test',
                RUST_LOG='off', **profile)


def alarm(_signal, _frame):
    raise Failure('total_deadline')


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path.cwd())
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--binary-sha256')
    parser.add_argument('--source-snapshot', type=Path)
    parser.add_argument('--worker', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--owned', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('--supervisor-self-test', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--supervisor-sleep', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not (args.supervisor_self_test or args.supervisor_sleep):
        for name in ('binary', 'binary_sha256', 'source_snapshot'):
            if getattr(args, name) is None:
                parser.error('--' + name.replace('_', '-') + ' is required')
    return args


def worker_main(args):
    root, binary = args.root.resolve(), args.binary.resolve()
    ensure(binary.is_file() and os.access(binary, os.X_OK), 'binary_missing_or_not_executable')
    ensure(re.fullmatch('[0-9a-f]{64}', args.binary_sha256) is not None, 'binary_sha_invalid')
    ensure(sha(binary) == args.binary_sha256, 'binary_sha_mismatch')
    tool = root / '.agents/project/scripts/verify-cookie-security.py'
    ensure(sha(tool) == TOOL_SHA, 'reviewed_tls_helper_changed')
    spec = importlib.util.spec_from_file_location('reviewed_cookie_tls', tool)
    tls = importlib.util.module_from_spec(spec)
    # 禁止导入生成仓库 __pycache__；只执行已经核定 SHA 的通用类与证书函数。
    exec(compile(tool.read_bytes(), str(tool), 'exec'), tls.__dict__)
    before = snapshot(root)
    snapshot_bytes = args.source_snapshot.read_bytes()
    expected = json.loads(snapshot_bytes)
    ensure(isinstance(expected, dict) and expected and all(
        isinstance(k, str) and isinstance(v, str) and re.fullmatch('[0-9a-f]{64}', v)
        for k, v in expected.items()), 'expected_source_snapshot_invalid')
    ensure(expected == before, 'expected_source_snapshot_mismatch')
    owned = args.owned.absolute()
    validate_owned_directory(owned)
    report = {'owned_directory': str(owned), 'binary': str(binary), 'binary_sha256': args.binary_sha256,
              'helper_sha256': TOOL_SHA, 'build_source_snapshot_sha256': hashlib.sha256(snapshot_bytes).hexdigest(),
              'build_source_snapshot': str(args.source_snapshot.resolve()), 'cases': {}, 'accepted': False,
              'limits': {'total_seconds': TOTAL, 'request_seconds': TIMEOUT, 'response_bytes': tls.RESPONSE_LIMIT},
              'tls_boundary': 'TLS terminates in owned Python proxy; Rust backend trusts loopback forwarded proto; not native Rust TLS adapter proof',
              'excluded': ['database parity', 'native Rust TLS adapter', 'TrustProxies host/port/prefix full parity'],
              'stdout_stderr': 'server output discarded; no raw request, body, credential or key logging'}
    processes, proxies = [], []
    (owned / 'source-before.json').write_text(json.dumps(before, sort_keys=True, indent=2) + '\n')

    def exchange(port, method='GET', json_source=False, *, host='localhost:8080',
                 forwarded=None, purpose='prefetch', context=None, path=PATH + QUERY):
        body = b'{"secret":"never-echo"}' if json_source else None
        headers = {'Host': host, 'Connection': 'close', 'Purpose': purpose,
                   'Authorization': 'Bearer never-echo', 'Cookie': 'private=never-echo'}
        if json_source:
            headers['Content-Type'] = 'application/json'
        if forwarded:
            headers['X-Forwarded-Proto'] = forwarded
        conn = http.client.HTTPConnection('127.0.0.1', port, timeout=TIMEOUT)
        conn.response_class = tls.BoundedResponse
        previous_timer = signal.getitimer(signal.ITIMER_REAL)[0]
        started = time.monotonic()
        ensure(previous_timer > 0, 'request_without_total_deadline')
        signal.setitimer(signal.ITIMER_REAL, min(TIMEOUT, previous_timer))
        try:
            if context:
                raw = socket.create_connection(('127.0.0.1', port), timeout=TIMEOUT)
                try:
                    conn.sock = context.wrap_socket(raw, server_hostname='localhost')
                except BaseException:
                    raw.close()
                    raise
                report['tls_negotiation'] = {'version': conn.sock.version(), 'cipher': conn.sock.cipher()[0],
                                             'hostname_verified': context.check_hostname,
                                             'explicit_ca': context.verify_mode == ssl.CERT_REQUIRED}
                ensure(conn.sock.version() in ('TLSv1.2', 'TLSv1.3'), 'tls_version')
            conn.request(method, path, body=body, headers=headers)
            response = conn.getresponse()
            content = response.read(tls.RESPONSE_LIMIT + 1)
            ensure(len(content) <= tls.RESPONSE_LIMIT, 'response_too_large')
            response_headers = response.getheaders()
            header_text = '\n'.join(str(k) + ': ' + str(v) for k, v in response_headers)
            ensure(b'never-echo' not in content and 'never-echo' not in header_text,
                   'sensitive_value_echo')
            return response.status, json.loads(content)
        finally:
            conn.close()
            remaining = previous_timer - (time.monotonic() - started)
            signal.setitimer(signal.ITIMER_REAL, max(.001, remaining))

    class URLProxyHandler(tls.ProxyHandler):
        def forward(self):
            connection = None
            try:
                tls.ensure(self.path == PATH + QUERY, 'proxy_fixed_public_path')
                tls.ensure(self.headers.get('Transfer-Encoding') is None, 'proxy_no_transfer_encoding')
                length = int(self.headers.get('Content-Length', '0'))
                tls.ensure(0 <= length <= tls.REQUEST_LIMIT, 'proxy_request_limit')
                body = self.rfile.read(length)
                tls.ensure(len(body) == length, 'proxy_body_complete')
                # 外部 Host 仅使用测试固定值，不信任调用方转发安全声明。
                headers = {'Host': 'localhost:443', 'X-Forwarded-Proto': 'https', 'Connection': 'close'}
                for name in ('Content-Type', 'Purpose'):
                    value = self.headers.get(name)
                    if value is not None:
                        headers[name] = value
                connection = http.client.HTTPConnection(*self.server.origin[:2], timeout=TIMEOUT)
                connection.response_class = tls.BoundedResponse
                connection.request(self.command, self.path, body=body or None, headers=headers)
                response = connection.getresponse()
                content = response.read(tls.RESPONSE_LIMIT + 1)
                tls.ensure(len(content) <= tls.RESPONSE_LIMIT, 'proxy_response_limit')
                self.send_response(response.status)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(content)))
                self.send_header('Connection', 'close')
                self.end_headers()
                self.wfile.write(content)
            except Exception:
                self.send_error(502, 'URL verification proxy failed')
            finally:
                if connection:
                    connection.close()
                self.close_connection = True

    def check(result, scheme, host, port, mutates_query, prefetch=True):
        status, doc = result
        ensure(status == 200 and doc.get('success') is True, 'url_response_success')
        data = doc['data']
        address = data['address']
        base = f'{scheme}://{host}{PATH}'
        original = base + '?label=public&page=1'
        ensure(address['root'] == f'{scheme}://{host}' and address['url'] == base, 'root_url')
        ensure(address['full_url'] == original and data['merged']['full_url'] == original
               and data['replaced_full_url'] == original, 'original_uri_stable')
        ensure(address['scheme'] == scheme and address['http_host'] == host and address['port'] == port,
               'scheme_authority_port')
        ensure(address['path'] == 'api/example/request-url' and address['decoded_path'] == address['path']
               and address['segments'] == ['api', 'example', 'request-url']
               and address['first_segment'] == 'api' and address['zero_segment'] is None
               and address['missing_segment'] is None, 'path_segments')
        ensure(address['is_example'] is True and address['full_url_is_example'] is True
               and address['prefetch'] is prefetch, 'patterns_prefetch')
        ensure(address['with_query'] == base + '?page=2&label=public'
               and address['without_query'] == base + '?label=public', 'original_query_projection')
        page = 3 if mutates_query else 1
        ensure(data['merged']['with_query'] == base + f'?page={page}&label=public&view=compact'
               and data['merged']['without_query'] == base + f'?page={page}', 'current_query_merge')
        expected = '?page=4&view=compact' if mutates_query else '?page=1&label=public&view=compact'
        ensure(data['replaced_with_query'] == base + expected, 'current_query_replace')

    signal.signal(signal.SIGALRM, alarm)
    signal.setitimer(signal.ITIMER_REAL, WORKER_LIMIT)
    try:
        report['before_resources'] = resources(root)
        origins = []
        for trusted in (True, False):
            cwd, config, port, profile = prepare(root, owned, trusted)
            process = subprocess.Popen([str(binary)], cwd=cwd,
                env=server_environment(config, profile),
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            processes.append(process)
            ensure(os.getpgid(process.pid) == os.getpgrp(), 'owned_group_mismatch')
            until = time.monotonic() + 15
            while True:
                ensure(process.poll() is None, 'origin_exited')
                ensure(time.monotonic() < until, 'origin_readiness_timeout')
                if listener_owned(port, process):
                    try:
                        check(exchange(port), 'http', 'localhost:8080', 8080, True)
                        break
                    except (OSError, http.client.HTTPException):
                        pass
                time.sleep(.1)
            origins.append(port)
        report['cases']['both_origins_business_ready'] = True
        # 独占但不 listen 的 loopback 端口用于真实连接失败，不探测他人端口。
        with socket.socket() as unavailable:
            unavailable.bind(('127.0.0.1', 0))
            unavailable_error = None
            started = time.monotonic()
            try:
                with socket.create_connection(unavailable.getsockname(), timeout=TIMEOUT):
                    pass
            except (ConnectionRefusedError, TimeoutError) as error:
                unavailable_error = {'type': type(error).__name__, 'errno': error.errno,
                                     'elapsed_seconds': round(time.monotonic() - started, 3),
                                     'timeout_seconds': TIMEOUT}
            ensure(unavailable_error is not None, 'owned_unavailable_port_connected')
        report['owned_unavailable_network'] = unavailable_error
        report['cases']['owned_unavailable_network_bounded_failure'] = True
        for index, port in enumerate(origins):
            for method, json_source in [('GET', False), ('GET', True), ('POST', True)]:
                check(exchange(port, method, json_source), 'http', 'localhost:8080', 8080,
                      method == 'GET' and not json_source)
            check(exchange(port, purpose='prefetch; next'), 'http', 'localhost:8080', 8080, True, False)
            check(exchange(port, host='[::1]:8080'), 'http', '[::1]:8080', 8080, True)
            check(exchange(port, host='localhost:80'), 'http', 'localhost', 80, True)
            status, _ = exchange(port, path=PATH + '?token=never-echo')
            ensure(status == 400, 'sensitive_query_rejected')
            report['cases'][f'tcp_origin_{index}_methods_authority_prefetch_privacy'] = True
        check(exchange(origins[1], forwarded='https'), 'http', 'localhost:8080', 8080, True)
        report['cases']['untrusted_forwarded_https_ignored'] = True
        cert, key = tls.create_certificate(owned)
        server_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        server_context.minimum_version = ssl.TLSVersion.TLSv1_2
        server_context.load_cert_chain(cert, key)
        client = ssl.create_default_context(cafile=str(cert))
        client.minimum_version = ssl.TLSVersion.TLSv1_2
        ensure(client.check_hostname and client.verify_mode == ssl.CERT_REQUIRED, 'strict_tls_client')
        for index, port in enumerate(origins):
            proxy = tls.OwnedTLSProxy(server_context, ('127.0.0.1', port, 'localhost:443'))
            proxy.RequestHandlerClass = URLProxyHandler
            thread = threading.Thread(target=proxy.serve_forever, kwargs={'poll_interval': .1}, daemon=False)
            thread.start()
            proxies.append((proxy, thread))
            for method, json_source in [('GET', False), ('GET', True), ('POST', True)]:
                check(exchange(proxy.server_port, method, json_source, context=client),
                      'https' if index == 0 else 'http', 'localhost' if index == 0 else 'localhost:443',
                      443, method == 'GET' and not json_source)
            if index == 0:
                for negative, context, hostname in [('no_ca', ssl.create_default_context(), 'localhost'),
                                                     ('wrong_hostname', client, 'wrong.invalid')]:
                    rejected = False
                    with socket.create_connection(('127.0.0.1', proxy.server_port), timeout=TIMEOUT) as raw:
                        try:
                            with context.wrap_socket(raw, server_hostname=hostname):
                                pass
                        except ssl.SSLCertVerificationError:
                            rejected = True
                    ensure(rejected, 'tls_negative_not_rejected')
                    report['cases']['tls_' + negative + '_rejected'] = True
                # 证书负例不能破坏后续业务。
                check(exchange(proxy.server_port, context=client), 'https', 'localhost', 443, True)
            report['cases'][f'tls_proxy_origin_{index}_strict_methods_port'] = True
        check(exchange(origins[0]), 'http', 'localhost:8080', 8080, True)
        check(exchange(origins[1]), 'http', 'localhost:8080', 8080, True)
        report['cases']['both_origins_business_after_tls'] = True
        report['accepted'] = True
    except Exception as error:
        report['failure_type'] = type(error).__name__
        if isinstance(error, Failure):
            report['failure_tag'] = str(error)
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        errors = []
        for proxy, thread in reversed(proxies):
            try:
                proxy.shutdown()
                proxy.server_close()
                thread.join(timeout=6)
                ensure(not thread.is_alive(), 'proxy_thread_survived')
            except Exception as error:
                errors.append(type(error).__name__)
        exits = []
        for process in reversed(processes):
            try:
                exits.append(stop(process))
            except Exception as error:
                errors.append(type(error).__name__)
        report['backend_exits'] = exits
        if len(exits) != 2 or any(p['code'] != 0 or p['forced'] for p in exits):
            errors.append('origin_shutdown_not_graceful')
        try:
            after = snapshot(root)
            report['source_changed'] = sorted(p for p in set(before) | set(after) if before.get(p) != after.get(p))
            (owned / 'source-after.json').write_text(json.dumps(after, sort_keys=True, indent=2) + '\n')
            report['binary_unchanged'] = sha(binary) == args.binary_sha256
            report['after_resources'] = resources(root)
            build_target = Path(os.environ.get('CARGO_TARGET_DIR', root / 'target'))
            if not build_target.is_absolute():
                build_target = root / build_target
            build_target = str(build_target.resolve())
            old_before = {k: v for k, v in report.get('before_resources', {}).get('target_bytes', {}).items()
                          if k != build_target}
            old_after = {k: v for k, v in report['after_resources']['target_bytes'].items()
                         if k != build_target}
            ensure(old_before == old_after, 'consumer_targets_changed')
        except Exception as error:
            errors.append(type(error).__name__)
        try:
            clear_sensitive(owned)
            report['sensitive_temporary_files_removed'] = True
        except Exception as error:
            errors.append(type(error).__name__)
        report['cleanup_errors'] = errors
        report['accepted'] = bool(report['accepted'] and not errors and not report.get('source_changed', True)
                                  and report.get('binary_unchanged'))
        output = owned / 'result.json'
        output.write_text(json.dumps(report, sort_keys=True, indent=2) + '\n')
        print(json.dumps({'result': str(output), 'accepted': report['accepted'],
                          'failure_type': report.get('failure_type'), 'failure_tag': report.get('failure_tag'),
                          'cleanup_errors': errors}, sort_keys=True), flush=True)
    return 0 if report['accepted'] else 1


def terminate_owned_group(process):
    # start_new_session 的 Popen PID 是本 supervisor 创建并保留的组身份。
    # 即使 leader 已退出，仍查该 PGID 的成员并清理遗留线程/servers。
    members = group_members(process.pid)
    if members:
        ensure(all(os.getpgid(pid) == process.pid for pid in members), 'supervised_group_identity')
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    process.wait(timeout=3)
    until = time.monotonic() + 3
    while group_members(process.pid) and time.monotonic() < until:
        time.sleep(.05)
    ensure(not group_members(process.pid), 'supervised_group_survived')


def supervised(command, timeout):
    process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                               start_new_session=True)
    timed_out = False
    try:
        ensure(os.getpgid(process.pid) == process.pid, 'supervisor_new_group_mismatch')
        try:
            code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            code = None
    finally:
        terminate_owned_group(process)
    return code, timed_out


def main():
    args = parse_args()
    if args.supervisor_sleep:
        # 同组子进程模拟 server；超时测试同时证明 worker 及其子进程被清理。
        child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
        child.wait()
        return 0
    if args.supervisor_self_test:
        started = time.monotonic()
        code, timed_out = supervised([sys.executable, str(Path(__file__).resolve()),
                                      '--supervisor-sleep'], .25)
        ensure(timed_out and code is None and time.monotonic() - started < 8,
               'supervisor_timeout_self_test_failed')
        print(json.dumps({'supervisor_timeout_self_test': True,
                          'owned_group_removed': True, 'framework_started': False}))
        return 0
    if args.worker:
        ensure(args.owned is not None, 'worker_owned_missing')
        return worker_main(args)
    supervisor_started = time.monotonic()
    owned = Path(tempfile.mkdtemp(prefix='openrust-c04d6-url-network-', dir='/tmp'))
    owned.chmod(0o700)
    command = [sys.executable, str(Path(__file__).resolve()), '--worker', '--owned', str(owned),
               '--root', str(args.root.resolve()), '--binary', str(args.binary.resolve()),
               '--binary-sha256', args.binary_sha256,
               '--source-snapshot', str(args.source_snapshot.resolve())]
    report = {'accepted': False, 'supervisor_total_deadline_seconds': TOTAL,
              'supervisor_reserved_forced_cleanup_seconds': 15,
              'build_source_snapshot_sha256': sha(args.source_snapshot.resolve())}
    try:
        code, timed_out = supervised(command, max(.001, TOTAL - 15 - (time.monotonic() - supervisor_started)))
        output = owned / 'result.json'
        if output.is_file():
            report.update(json.loads(output.read_text()))
        report.update(worker_exit_code=code, supervisor_timed_out=timed_out)
        report['accepted'] = bool(report.get('accepted') and code == 0 and not timed_out)
        if timed_out:
            report['failure_tag'] = 'supervisor_total_deadline_including_cleanup'
    except Exception as error:
        report.update(accepted=False, supervisor_failure_type=type(error).__name__)
    finally:
        try:
            clear_sensitive(owned)
            report['sensitive_temporary_files_removed'] = True
        except Exception as error:
            report.update(accepted=False, sensitive_cleanup_failure_type=type(error).__name__)
        output = owned / 'result.json'
        output.write_text(json.dumps(report, sort_keys=True, indent=2) + '\n')
        print(json.dumps({'result': str(output), 'accepted': report['accepted'],
                          'failure_tag': report.get('failure_tag'),
                          'supervisor_timed_out': report.get('supervisor_timed_out')}, sort_keys=True))
    return 0 if report['accepted'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
