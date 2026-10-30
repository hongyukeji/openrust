#!/usr/bin/env python3
"""D8 正式模块 Session previous 与 Precognition 的单进程真实 TCP 验证。

不构建、不连接数据库。沿 D6 supervisor 运行，必须提供已构建二进制、其
SHA256 和构建来源快照；构建关联由外部记录证明。只验证此进程内显式 array
driver 的两个浏览器会话，不作为双库、TLS 或完整字段选择能力的证据。
"""
import hashlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import time
import tomllib


def load_helper(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    exec(compile(path.read_bytes(), str(path), 'exec'), module.__dict__)
    return module


network = load_helper(Path(__file__).with_name('verify-request-url.py'), 'session_network')
BASE = '/api/example/session/previous'
COOKIE_NAME = 'example_network_session'


class Browser:
    """只保留服务端 Set-Cookie 的原始首段，模拟浏览器同源 Cookie 合并。"""

    def __init__(self):
        self.cookies = {}

    def header(self):
        return '; '.join(f'{name}={value}' for name, value in self.cookies.items())

    def receive(self, headers):
        for name, value in headers:
            if name.lower() != 'set-cookie':
                continue
            first, *attributes = value.split(';')
            key, separator, encoded = first.strip().partition('=')
            network.ensure(separator and re.fullmatch(r"[!#$%&'*+.^_`|~0-9A-Za-z-]+", key)
                           and '\r' not in value and '\n' not in value,
                           'set_cookie_invalid')
            expired = any(attribute.strip().lower().replace(' ', '') == 'max-age=0'
                          for attribute in attributes)
            if expired:
                self.cookies.pop(key, None)
            else:
                network.ensure(encoded, 'set_cookie_empty_without_expiry')
                if key == COOKIE_NAME:
                    normalized = [attribute.strip().lower() for attribute in attributes]
                    network.ensure('path=/' in normalized and 'httponly' in normalized
                                   and 'samesite=lax' in normalized and 'secure' not in normalized,
                                   'session_cookie_attributes')
                self.cookies[key] = encoded
        network.ensure(len(self.cookies) <= 16 and len(self.header()) <= 16 * 1024,
                       'cookie_jar_limit')


def headers_map(headers):
    values = {}
    for key, value in headers:
        values.setdefault(key.lower(), []).append(value)
    return values


def one_header(headers, name):
    values = headers_map(headers).get(name.lower(), [])
    network.ensure(len(values) <= 1, 'duplicate_' + name.replace('-', '_'))
    return values[0] if values else None


def assert_success(status, content, expected, tag):
    network.ensure(status == 200 and content is not None and content.get('success') is True
                   and content.get('code') == 'ok' and content.get('message') == 'ok'
                   and content.get('data') == expected, tag)


def previous(url, route, visits, attempting=False, route_exists=False):
    return {'url': url, 'route': route, 'has_uri': url is not None,
            'route_exists': route_exists, 'visits': visits,
            'attempting': attempting, 'confirmed': False}


def worker_main(args):
    root, binary, owned = args.root.resolve(), args.binary.resolve(), args.owned.resolve()
    network.ensure(binary.is_file() and os.access(binary, os.X_OK), 'binary_not_executable')
    network.ensure(re.fullmatch('[0-9a-f]{64}', args.binary_sha256) is not None,
                   'binary_sha_invalid')
    network.ensure(network.sha(binary) == args.binary_sha256, 'binary_sha_mismatch')
    source_bytes = args.source_snapshot.read_bytes()
    expected_source = json.loads(source_bytes)
    network.ensure(isinstance(expected_source, dict) and expected_source and all(
        isinstance(key, str) and isinstance(value, str)
        and re.fullmatch('[0-9a-f]{64}', value)
        for key, value in expected_source.items()), 'source_snapshot_invalid')
    before = network.snapshot(root)
    network.ensure(before == expected_source, 'source_snapshot_mismatch')
    network.ensure(owned.is_dir() and owned.stat().st_mode & 0o777 == 0o700,
                   'owned_directory_mode')
    bounded_path = root / '.agents/project/scripts/verify-cookie-security.py'
    network.ensure(network.sha(bounded_path) == network.TOOL_SHA, 'bounded_helper_changed')
    bounded = load_helper(bounded_path, 'session_bounded_response')
    report = {
        'accepted': False, 'cases': {}, 'owned_directory': str(owned),
        'binary': str(binary), 'binary_sha256': args.binary_sha256,
        'build_source_snapshot': str(args.source_snapshot.resolve()),
        'build_source_snapshot_sha256': hashlib.sha256(source_bytes).hexdigest(),
        'helper_sha256': network.sha(Path(__file__).with_name('verify-request-url.py')),
        'bounded_helper_sha256': network.TOOL_SHA,
        'limits': {'total_seconds': network.TOTAL, 'request_seconds': network.TIMEOUT,
                   'response_bytes': bounded.RESPONSE_LIMIT},
        'stdout_stderr': 'server and worker output discarded; cookies and bodies never logged',
        'excluded': ['build provenance without external build record', 'database parity',
                     'TLS', 'multi-process array persistence', 'partial field validation',
                     'complete Session or Precognition parity'],
    }
    process = None
    (owned / 'source-before.json').write_text(json.dumps(before, sort_keys=True, indent=2) + '\n')

    def exchange(port, browser, method, path, attempting=False):
        connection = http.client.HTTPConnection('127.0.0.1', port, timeout=network.TIMEOUT)
        connection.response_class = bounded.BoundedResponse
        headers = {'Host': 'localhost', 'Connection': 'close'}
        if browser.header():
            headers['Cookie'] = browser.header()
        if attempting:
            headers['Precognition'] = 'true'
        previous_timer = signal.getitimer(signal.ITIMER_REAL)[0]
        network.ensure(previous_timer > 0, 'request_without_total_deadline')
        started = time.monotonic()
        signal.setitimer(signal.ITIMER_REAL, min(previous_timer, network.TIMEOUT))
        try:
            connection.request(method, path, headers=headers)
            response = connection.getresponse()
            content = response.read(bounded.RESPONSE_LIMIT + 1)
            network.ensure(len(content) <= bounded.RESPONSE_LIMIT, 'response_too_large')
            received = response.getheaders()
            browser.receive(received)
            network.ensure(response.status in (200, 404, 405), 'unexpected_response_status')
            return response.status, received, json.loads(content) if content else None
        finally:
            connection.close()
            signal.setitimer(signal.ITIMER_REAL,
                             max(.001, previous_timer - (time.monotonic() - started)))

    def wire_empty(port, browser, method, path, attempting, expected_status):
        # HTTPResponse 会依状态/HEAD 语义隐藏收到的正文，必须读到真实 EOF。
        previous_timer = signal.getitimer(signal.ITIMER_REAL)[0]
        network.ensure(previous_timer > 0, 'wire_without_total_deadline')
        started = time.monotonic()
        signal.setitimer(signal.ITIMER_REAL, min(previous_timer, network.TIMEOUT))
        try:
            with socket.create_connection(('127.0.0.1', port), timeout=network.TIMEOUT) as connection:
                lines = [f'{method} {path} HTTP/1.1', 'Host: localhost', 'Connection: close']
                if browser.header():
                    lines.append('Cookie: ' + browser.header())
                if attempting:
                    lines.append('Precognition: true')
                connection.sendall(('\r\n'.join(lines) + '\r\n\r\n').encode('ascii'))
                chunks, total = [], 0
                while True:
                    chunk = connection.recv(min(8192, bounded.RESPONSE_LIMIT + 1 - total))
                    if not chunk:
                        break
                    chunks.append(chunk)
                    total += len(chunk)
                    network.ensure(total <= bounded.RESPONSE_LIMIT, 'wire_response_too_large')
                wire = b''.join(chunks)
            head, separator, body = wire.partition(b'\r\n\r\n')
            network.ensure(separator and len(head) <= 64 * 1024 and body == b'',
                           'wire_body_or_headers_invalid')
            lines = head.split(b'\r\n')
            status = re.fullmatch(rb'HTTP/1\.[01] ([0-9]{3}) [^\r\n]*', lines[0])
            network.ensure(status is not None and int(status.group(1)) == expected_status,
                           'wire_status')
            received = []
            for line in lines[1:]:
                key, colon, value = line.partition(b':')
                network.ensure(colon and re.fullmatch(rb"[!#$%&'*+.^_`|~0-9A-Za-z-]+", key)
                               and b'\r' not in value and b'\n' not in value,
                               'wire_header_invalid')
                received.append((key.decode('ascii'), value.strip().decode('latin-1')))
            browser.receive(received)
            return received
        finally:
            signal.setitimer(signal.ITIMER_REAL,
                             max(.001, previous_timer - (time.monotonic() - started)))

    signal.signal(signal.SIGALRM, network.alarm)
    signal.setitimer(signal.ITIMER_REAL, network.WORKER_LIMIT)
    try:
        report['before_resources'] = network.resources(root)
        cwd, config, port = network.prepare(root, owned, True)
        (config / 'session.toml').write_text(
            '[session]\ndriver="array"\ncookie="' + COOKIE_NAME + '"\n'
            'secure=false\nlottery=[1,1]\n')
        # network.prepare 故意隔离 DB/Session；此处显式选择单进程 memory store。
        session_config = tomllib.loads((config / 'session.toml').read_text())['session']
        database_config = tomllib.loads((config / 'database.toml').read_text())['database']
        network.ensure(session_config['driver'] == 'array' and session_config['secure'] is False
                       and session_config['lottery'] == [1, 1]
                       and 'url' not in database_config and not database_config.get('instances'),
                       'session_isolation_config')
        env = {key: value for key, value in os.environ.items()
               if key in ('PATH', 'HOME', 'TMPDIR')}
        process = subprocess.Popen([str(binary)], cwd=cwd,
            env=dict(env, APP_CONFIG_DIR=str(config), APP_ENV='test', RUST_LOG='off'),
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        network.ensure(os.getpgid(process.pid) == os.getpgrp(), 'owned_group_mismatch')
        until = time.monotonic() + 15
        while True:
            network.ensure(process.poll() is None, 'origin_exited')
            network.ensure(time.monotonic() < until, 'origin_readiness_timeout')
            if network.listener_owned(port, process):
                break
            time.sleep(.1)

        first, second = Browser(), Browser()
        named_url = 'http://localhost' + BASE + '/named'
        anonymous_url = 'http://localhost' + BASE + '/anonymous'
        status, headers, content = exchange(port, first, 'GET', BASE + '/named')
        assert_success(status, content, previous(None, None, 1), 'named_initial')
        network.ensure(COOKIE_NAME in first.cookies, 'session_cookie_missing')
        report['cases']['named_initial_and_cookie'] = True

        status, _, content = exchange(port, first, 'GET', BASE + '/anonymous')
        assert_success(status, content, previous(named_url, 'example.session.previous.named',
                                                 2, route_exists=True), 'anonymous_previous_named')
        status, _, content = exchange(port, first, 'POST', BASE)
        assert_success(status, content, previous(anonymous_url, None, 2, route_exists=True),
                       'post_reads_anonymous')
        report['cases']['anonymous_null_route_and_post_read'] = True

        status, headers, content = exchange(port, first, 'GET', BASE + '/named', True)
        assert_success(status, content, previous(anonymous_url, None, 3, True, True),
                       'attempting_without_opt_in')
        network.ensure(one_header(headers, 'precognition') is None
                       and all('Precognition' not in value for value in
                               headers_map(headers).get('vary', [])),
                       'attempting_was_confirmed')
        report['cases']['attempting_header_runs_ordinary_business'] = True

        headers = wire_empty(port, first, 'GET', BASE + '/preview', True, 204)
        network.ensure(one_header(headers, 'precognition') == 'true'
                       and one_header(headers, 'precognition-success') == 'true'
                       and any('Precognition' in value for value in headers_map(headers).get('vary', [])),
                       'confirmed_headers')
        status, _, content = exchange(port, first, 'POST', BASE)
        assert_success(status, content, previous(named_url, 'example.session.previous.named',
                                                 3, route_exists=True), 'confirmed_did_not_persist')
        report['cases']['confirmed_204_wire_empty_skips_business_history_save'] = True

        status, _, content = exchange(port, second, 'POST', BASE)
        assert_success(status, content, previous(None, None, None), 'second_client_initial')
        network.ensure(COOKIE_NAME in second.cookies
                       and second.cookies[COOKIE_NAME] != first.cookies[COOKIE_NAME],
                       'two_client_cookie_isolation')
        headers = wire_empty(port, second, 'HEAD', BASE + '/named', False, 200)
        network.ensure(one_header(headers, 'precognition-success') is None,
                       'head_incorrect_precognition_success')
        status, _, content = exchange(port, second, 'POST', BASE)
        assert_success(status, content, previous(None, None, 1), 'head_kept_previous_unset')
        report['cases']['head_wire_empty_and_second_client_isolation'] = True

        status, _, _ = exchange(port, second, 'PUT', BASE + '/named')
        network.ensure(status == 405, 'method_miss_status')
        status, _, _ = exchange(port, second, 'GET', BASE + '/not-mounted')
        network.ensure(status == 404, 'unmatched_status')
        status, _, content = exchange(port, second, 'POST', BASE)
        assert_success(status, content, previous(None, None, 1), 'nonroute_did_not_replace_history')
        status, _, content = exchange(port, first, 'POST', BASE)
        assert_success(status, content, previous(named_url, 'example.session.previous.named',
                                                 3, route_exists=True), 'first_client_survives_other')
        report['cases']['method_miss_and_unmatched_preserve_both_clients'] = True
        report['accepted'] = True
    except Exception as error:
        report['failure_type'] = type(error).__name__
        if isinstance(error, network.Failure):
            report['failure_tag'] = str(error)
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        errors = []
        if process is not None:
            try:
                exit_info = network.stop(process)
                report['backend_exit'] = exit_info
                network.ensure(exit_info['code'] == 0 and not exit_info['forced'],
                               'origin_shutdown_not_graceful')
            except Exception as error:
                errors.append(type(error).__name__)
        else:
            errors.append('origin_not_started')
        try:
            after = network.snapshot(root)
            report['source_changed'] = sorted(key for key in set(before) | set(after)
                                             if before.get(key) != after.get(key))
            (owned / 'source-after.json').write_text(json.dumps(after, sort_keys=True, indent=2) + '\n')
            report['binary_unchanged'] = network.sha(binary) == args.binary_sha256
            report['after_resources'] = network.resources(root)
            previous_targets = report.get('before_resources', {}).get('target_bytes', {})
            current_targets = report['after_resources']['target_bytes']
            network.ensure({key: value for key, value in previous_targets.items()
                            if key != str(root / 'target')}
                           == {key: value for key, value in current_targets.items()
                               if key != str(root / 'target')}, 'consumer_targets_changed')
        except Exception as error:
            errors.append(type(error).__name__)
        try:
            network.clear_sensitive(owned)
            report['sensitive_temporary_files_removed'] = True
        except Exception as error:
            errors.append(type(error).__name__)
        report['cleanup_errors'] = errors
        report['accepted'] = bool(report['accepted'] and not errors
                                  and not report.get('source_changed', True)
                                  and report.get('binary_unchanged'))
        (owned / 'result.json').write_text(json.dumps(report, sort_keys=True, indent=2) + '\n')
    return 0 if report['accepted'] else 1


if __name__ == '__main__':
    network.__file__ = __file__
    network.__doc__ = __doc__
    network.worker_main = worker_main
    raise SystemExit(network.main())
