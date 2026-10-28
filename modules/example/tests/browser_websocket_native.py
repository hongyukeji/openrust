#!/usr/bin/env python3
"""真实浏览器 Cookie/Origin WebSocket 验收；fixture 与服务由集成人提供。

仅接受权限 0600 的临时 JSON；不记录凭据、Cookie、响应正文、HAR 或 trace。
协议回归单独证明 HTTP 状态，浏览器仅证明原生事件与 Cookie/Origin 行为。
本脚本只操作本次创建的浏览器，不通过 CDP 状态码推断原生 API 暴露状态。
"""

import argparse
import asyncio
import contextlib
import json
import math
import os
from pathlib import Path
import stat
import tempfile
import time
from urllib.parse import urlsplit

from playwright.async_api import async_playwright

STEP_MS = 8000


def origin(value):
    parsed = urlsplit(value)
    if (
        parsed.scheme not in ("http", "https")
        or parsed.hostname not in ("127.0.0.1", "localhost", "::1")
        or parsed.username
        or parsed.password
        or parsed.path not in ("", "/")
        or parsed.query
        or parsed.fragment
    ):
        raise ValueError("invalid local fixture origin")
    return value.rstrip("/")


def read_fixture(path):
    path = Path(path)
    if not path.resolve().is_relative_to(Path("/tmp").resolve()):
        raise ValueError("fixture must be under /tmp")
    info = path.lstat()
    if (
        not stat.S_ISREG(info.st_mode)
        or stat.S_IMODE(info.st_mode) != 0o600
        or info.st_uid != os.geteuid()
        or path.stat().st_size > 16384
    ):
        raise ValueError("fixture must be a small owner-only regular file")
    config = json.loads(path.read_text())
    config["origin"] = origin(config["origin"])
    if type(config["article_id"]) is not int or config["article_id"] <= 0:
        raise ValueError("invalid article id")
    for role in ("owner", "other"):
        if not all(
            isinstance(config[role][field], str) and config[role][field]
            for field in ("email", "password")
        ):
            raise ValueError("missing temporary role credentials")
    if not isinstance(config.get("cookie_name"), str) or not config["cookie_name"]:
        raise ValueError("fixture cookie_name is required")
    ttl = config["session_ttl_secs"]
    if type(ttl) is not int or not 5 <= ttl <= 30:
        raise ValueError("fixture session TTL must be between 5 and 30 seconds")
    if config.get("foreign_origin"):
        config["foreign_origin"] = origin(config["foreign_origin"])
        a, b = urlsplit(config["origin"]), urlsplit(config["foreign_origin"])
        if a.hostname != b.hostname or a.scheme != b.scheme or a.port == b.port:
            raise ValueError("foreign page must be same-host and different-port")
    if config.get("shutdown_ready_path"):
        marker = Path(config["shutdown_ready_path"])
        if not marker.is_absolute() or not str(marker).startswith("/tmp/"):
            raise ValueError("shutdown marker must be under /tmp")
        if marker.exists() or marker.is_symlink():
            raise ValueError("shutdown marker must not already exist")
    return config


async def login(page, config, role):
    result = await page.evaluate(
        """async ({email, password}) => {
            const started = Date.now() / 1000;
            const response = await fetch('/api/example/account/session/login', {
                method: 'POST', credentials: 'same-origin',
                headers: {'Content-Type': 'application/json',
                          'X-Example-Request': 'same-origin'},
                body: JSON.stringify({email, password}),
                signal: AbortSignal.timeout(8000)
            });
            return {ok: response.ok, status: response.status,
                    started, completed: Date.now() / 1000};
        }""",
        config[role],
    )
    if result["ok"] is not True or result["status"] != 200:
        raise AssertionError("temporary Session login failed")
    cookies = await page.context.cookies(config["origin"])
    selected = [cookie for cookie in cookies if cookie["name"] == config["cookie_name"]]
    if len(selected) != 1 or not selected[0]["httpOnly"]:
        raise AssertionError("browser did not retain the HttpOnly Session cookie")
    return {"started": result["started"], "completed": result["completed"]}


def expiry_window(login_started, login_completed, ttl):
    if not all(math.isfinite(value) for value in (login_started, login_completed, ttl)):
        raise AssertionError("invalid timing values")
    # Session.save 持久化在真实登录 fetch 的开始和响应完成之间；同一主机时钟。
    # Realtime 截止会截断到整数秒，另容许 150ms 时钟/事件调度误差。
    if not 0 <= login_completed - login_started <= 2:
        raise AssertionError("login duration cannot establish a narrow expiry bracket")
    return (math.floor(login_started + ttl) - 0.15, login_completed + ttl + 1)


def check_expiry_observation(window, opened_at, alive_at, closed_at):
    lower, upper = window
    if not all(math.isfinite(value) for value in (opened_at, alive_at, closed_at)):
        raise AssertionError("expiry observation missing")
    if not opened_at <= alive_at < lower <= closed_at <= upper:
        raise AssertionError("connection closed outside the persisted-expiry bracket")
    # 到达下界前 350ms 内必须实际观测 open，单次握手成功不能代替存活证明。
    if alive_at < lower - 0.35:
        raise AssertionError("connection survival was not observed near expiry")


async def observe_expiry(page, name, window):
    lower, upper = window
    delay = lower - 0.25 - time.time()
    if delay <= 0:
        raise AssertionError("expiry observation window was missed")
    await asyncio.sleep(delay)
    alive = await page.evaluate(
        """name => { const state = window.__openrustSockets[name];
            return {open: state.socket.readyState === WebSocket.OPEN && !state.closed,
                    observedAt: Date.now() / 1000, openedAt: state.openedAt}; }""", name
    )
    if not alive["open"]:
        raise AssertionError("connection closed before the earliest justified expiry")
    remaining = upper - time.time()
    if remaining <= 0:
        raise AssertionError("expiry upper bound already passed")
    code = await wait_closed(page, name, max(1, int(remaining * 1000)))
    closed_at = await page.evaluate(
        "name => window.__openrustSockets[name].closedAt", name
    )
    check_expiry_observation(window, alive["openedAt"], alive["observedAt"], closed_at)
    return code, closed_at


def self_test_expiry():
    window = expiry_window(1000.2, 1000.6, 10)
    check_expiry_observation(window, 1002, 1009.6, 1010.1)
    for opened, alive, closed in ((1002, 1009.6, 1009.75), (1002, 1009.6, 1012), (1002, 1005, 1010.1)):
        try:
            check_expiry_observation(window, opened, alive, closed)
        except AssertionError:
            continue
        raise AssertionError("invalid expiry observation accepted")
    try:
        expiry_window(1000, 1003, 10)
    except AssertionError:
        pass
    else:
        raise AssertionError("slow login timing accepted")
    for accepted in ({"opened": False, "event": "error"}, {"opened": False, "event": "close"}):
        if not native_rejection(accepted):
            raise AssertionError("native rejection classifier lost an explicit rejection")
    for rejected in ({"opened": False, "event": "timeout"}, {"opened": True, "event": "close"},
                     {"opened": 0, "event": "error"}, {"event": "error"},
                     {"opened": False}, {"opened": False, "event": "unexpected"}, None):
        if native_rejection(rejected):
            raise AssertionError("native rejection classifier accepted non-rejection evidence")
    print(json.dumps({"time_window_self_test": True, "early_close_rejected": True,
                      "late_close_rejected": True, "missing_survival_rejected": True,
                      "slow_login_rejected": True, "native_rejection_self_test": True,
                      "timeout_is_not_rejection": True}), flush=True)
    return 0


async def open_socket(page, url, name):
    return await page.evaluate(
        """({url, name}) => new Promise(resolve => {
            window.__openrustSockets ??= {};
            const socket = new WebSocket(url);
            const state = {socket, opened: false, closed: false, closeCode: null,
                           openedAt: null, closedAt: null};
            window.__openrustSockets[name] = state;
            let settled = false;
            const finish = event => {
                if (!settled) { settled = true; clearTimeout(timer);
                    resolve({opened: state.opened, event}); }
            };
            const timer = setTimeout(() => { socket.close(); finish('timeout'); }, 8000);
            socket.addEventListener('open', () => {
                state.opened = true; state.openedAt = Date.now() / 1000; finish('open');
            });
            socket.addEventListener('error', () => finish('error'));
            socket.addEventListener('close', event => {
                state.closed = true; state.closedAt = Date.now() / 1000;
                state.closeCode = event.code; finish('close');
            });
        })""",
        {"url": url, "name": name},
    )


async def wait_closed(page, name, timeout_ms=STEP_MS):
    await page.wait_for_function(
        "name => window.__openrustSockets?.[name]?.closed === true",
        arg=name,
        timeout=timeout_ms,
    )
    # close code is protocol metadata, never a close reason or frame payload.
    return await page.evaluate(
        "name => window.__openrustSockets[name].closeCode", name
    )


async def expect_open(page, url, name):
    result = await open_socket(page, url, name)
    if not result["opened"] or result["event"] != "open":
        raise AssertionError("native WebSocket did not open")


def native_rejection(result):
    # WebSocket 标准 API 不公开 HTTP 状态；超时和成功后关闭不能算握手拒绝。
    return (isinstance(result, dict) and result.get("opened") is False
            and result.get("event") in ("error", "close"))


async def expect_rejected(page, url, name):
    result = await open_socket(page, url, name)
    if not native_rejection(result):
        raise AssertionError("native error/close handshake rejection was not observed")
    await wait_closed(page, name)


async def rejection_with_connectivity(owner, page, authorized_url, rejected_url, name):
    # 前后均真正重新建立授权连接，避免把服务不可用误认为权限/Origin 拒绝。
    await reopen(owner, authorized_url, name + "_before")
    await close_socket(owner, name + "_before")
    await expect_rejected(page, rejected_url, name)
    await reopen(owner, authorized_url, name + "_after")
    await close_socket(owner, name + "_after")


async def assert_socket_open(page, name):
    opened = await page.evaluate(
        "name => window.__openrustSockets[name].socket.readyState === WebSocket.OPEN && !window.__openrustSockets[name].closed", name
    )
    if opened is not True:
        raise AssertionError("authorized connection did not survive the rejection probe")


async def close_socket(page, name):
    await page.evaluate("name => window.__openrustSockets[name].socket.close()", name)
    await wait_closed(page, name)


async def reopen(page, url, name):
    # 服务器释放名额可能晚于浏览器 close 事件，重试仍有明确总界限。
    async with asyncio.timeout(8):
        while True:
            result = await open_socket(page, url, name)
            if result["opened"]:
                return
            await asyncio.sleep(0.05)


async def scenario(config, report):
    async with async_playwright() as playwright:
        # 所有 profile 都在自有临时目录；从不读取用户浏览器 profile。
        with tempfile.TemporaryDirectory(prefix="openrust-native-ws-", dir="/tmp") as temp:
            contexts = []
            try:
                pages = {}
                for role in ("owner", "other", "anonymous"):
                    context = await playwright.chromium.launch_persistent_context(
                        str(Path(temp) / role),
                        headless=True,
                        args=["--no-proxy-server"],
                        viewport={"width": 1024, "height": 768},
                        accept_downloads=False,
                    )
                    contexts.append(context)
                    context.set_default_timeout(STEP_MS)
                    context.set_default_navigation_timeout(STEP_MS)
                    page = context.pages[0] if context.pages else await context.new_page()
                    await page.goto(config["origin"] + "/", wait_until="domcontentloaded")
                    pages[role] = page
                browser = contexts[0].browser
                report["browser"] = "chromium"
                report["version"] = browser.version if browser else "unavailable"
                report["phase"] = "login"
                await login(pages["other"], config, "other")
                await login(pages["owner"], config, "owner")
                url = config["origin"].replace("http", "ws", 1)
                url += f'/api/example/articles/{config["article_id"]}/browser-socket'
                owner = pages["owner"]
                report["phase"] = "owner_open"
                await expect_open(owner, url, "primary")
                report["owner_native_cookie_origin"] = True
                report["session_cookie_httponly"] = True
                await close_socket(owner, "primary")
                report["phase"] = "cross_owner"
                await rejection_with_connectivity(owner, pages["other"], url, url, "cross_owner")
                report["cross_owner_rejected"] = True
                report["phase"] = "anonymous"
                await rejection_with_connectivity(owner, pages["anonymous"], url, url, "anonymous")
                report["anonymous_rejected"] = True
                report["phase"] = "query_credential"
                await rejection_with_connectivity(owner, owner, url, url + "?access_token=forbidden", "url_credential")
                report["url_credential_rejected"] = True
                # 既有真实 TCP 协议测试单独断言 HTTP 503；浏览器只证拒绝与连接存活。
                report["phase"] = "connection_limit"
                await reopen(owner, url, "primary")
                await assert_socket_open(owner, "primary")
                await expect_rejected(owner, url, "capacity")
                await assert_socket_open(owner, "primary")
                report["connection_limit_rejected"] = True
                await owner.evaluate(
                    "() => window.__openrustSockets.primary.socket.send('read-only-check')"
                )
                await wait_closed(owner, "primary")
                report["client_publication_closed"] = True
                report["phase"] = "slot_reopen"
                await reopen(owner, url, "reopened")
                await close_socket(owner, "reopened")
                report["slot_released"] = True
                if config.get("foreign_origin"):
                    # 同 host 异 port 真实页面共享 Cookie 选择；浏览器自动产生外来 Origin。
                    foreign = await contexts[0].new_page()
                    try:
                        await foreign.goto(config["foreign_origin"] + "/", wait_until="domcontentloaded")
                        report["phase"] = "foreign_origin"
                        await rejection_with_connectivity(owner, foreign, url, url, "foreign")
                        report["foreign_origin_rejected"] = True
                    finally:
                        await asyncio.wait_for(foreign.close(), timeout=5)
                else:
                    report["foreign_origin_rejected"] = "not_run"
                report["phase"] = "expiry"
                logged_in = await login(owner, config, "owner")
                window = expiry_window(logged_in["started"], logged_in["completed"], config["session_ttl_secs"])
                await asyncio.sleep(2)
                await reopen(owner, url, "expiry")
                code, closed_at = await observe_expiry(owner, "expiry", window)
                report["expiry_survived_until_lower_bound"] = True
                report["expiry_closed"] = True
                report["expiry_close_code"] = code
                report["expiry_close_after_login_secs"] = round(closed_at - logged_in["started"], 3)
                await asyncio.sleep(max(0, logged_in["completed"] + config["session_ttl_secs"] + 0.1 - time.time()))
                report["phase"] = "expired_session"
                await expect_rejected(owner, url, "expired_cookie")
                # 到期前的 near-OPEN 已证明前置连通；新登录后必须再次打开，排除服务故障。
                await login(owner, config, "owner")
                await reopen(owner, url, "renewed_session")
                await close_socket(owner, "renewed_session")
                report["expired_session_rejected"] = True
                if config.get("shutdown_ready_path"):
                    report["phase"] = "shutdown"
                    await reopen(owner, url, "shutdown")
                    marker = Path(config["shutdown_ready_path"])
                    descriptor = os.open(marker, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
                    with os.fdopen(descriptor, "w") as output:
                        output.write("ready\n")
                    print(json.dumps({"phase": "awaiting_fixture_owner_shutdown"}), flush=True)
                    await wait_closed(owner, "shutdown")
                    report["owner_controlled_shutdown_closed"] = True
                else:
                    report["owner_controlled_shutdown_closed"] = "not_run"
            finally:
                # 外层取消也会进入此处，每个 context 关闭各有界限。
                for context in reversed(contexts):
                    browser = context.browser
                    try:
                        await asyncio.wait_for(context.close(), timeout=5)
                    except Exception:
                        if browser:
                            with contextlib.suppress(Exception):
                                await asyncio.wait_for(browser.close(), timeout=5)


async def run(config):
    report = {"runtime": "native_browser_websocket", "passed": False}
    try:
        # 120s 限制异步场景预算；finally 逐个回收自有浏览器另有有限清理时间，
        # 不是整个 Python 进程的严格硬截止。
        async with asyncio.timeout(120):
            await scenario(config, report)
        report["passed"] = True
        report["phase"] = "finished"
        report["complete"] = all(
            report.get(key) is True
            for key in ("foreign_origin_rejected", "owner_controlled_shutdown_closed")
        )
    except Exception as error:
        # 不使用异常文本或 traceback：第三方错误可能包含网络/凭据上下文。
        report["failure_class"] = type(error).__name__
        report["complete"] = False
    print(json.dumps(report, sort_keys=True), flush=True)
    return 0 if report["passed"] else 1


def main():
    parser = argparse.ArgumentParser(description="Isolated native browser WebSocket acceptance")
    inputs = parser.add_mutually_exclusive_group(required=True)
    inputs.add_argument("--fixture", help="owner-only temporary fixture JSON path")
    inputs.add_argument("--self-test", action="store_true", help="pure timing checks; no browser or service")
    args = parser.parse_args()
    try:
        if args.self_test:
            return self_test_expiry()
        config = read_fixture(args.fixture)
        return asyncio.run(run(config))
    except Exception as error:
        print(json.dumps({"passed": False, "complete": False, "failure_class": type(error).__name__}), flush=True)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
