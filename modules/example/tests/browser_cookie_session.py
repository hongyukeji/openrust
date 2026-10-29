#!/usr/bin/env python3
"""唯一示例模块的真实 Chromium HTTP Cookie 回归；不安装浏览器、不保存凭据。"""
import argparse
import asyncio
import ipaddress
import json
import os
from pathlib import Path
import re
import tempfile
import time
from urllib.parse import urlsplit

from playwright.async_api import async_playwright


class CheckFailed(Exception):
    """只通过固定阶段报告失败，不输出底层异常或 Cookie。"""


def require(condition):
    if not condition:
        raise CheckFailed()


def options():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", required=True)
    parser.add_argument("--result-directory", required=True)
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("--cookie-name", default="d4_cookie_session")
    args = parser.parse_args()
    parsed = urlsplit(args.url)
    try:
        loopback = parsed.hostname == "localhost" or ipaddress.ip_address(parsed.hostname).is_loopback
    except ValueError:
        loopback = False
    if not (parsed.scheme == "http" and loopback and parsed.path == "/api/example/session"
            and parsed.username is None and parsed.password is None
            and not parsed.query and not parsed.fragment):
        parser.error("--url 必须为无凭据/查询的 loopback HTTP /api/example/session")
    if not 0 < args.timeout <= 90:
        parser.error("--timeout 必须大于0且不超过90秒")
    if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_-]{0,127}", args.cookie_name):
        parser.error("--cookie-name 必须是有限的配置 Cookie 名")
    directory = Path(args.result_directory).resolve()
    if directory == Path("/tmp").resolve() or Path("/tmp").resolve() not in directory.parents:
        parser.error("--result-directory 必须是 /tmp 内的专用目录")
    directory.mkdir(parents=True, exist_ok=True)
    if (directory / "result.json").exists():
        parser.error("结果文件已存在，请选择新的专用目录以保留旧证据")
    args.result_directory = directory
    return args


async def fetch(page, url, value=None):
    reply = await page.evaluate("""async ({url,value}) => {
      const response = await fetch(url, {cache:'no-store', credentials:'same-origin',
        ...(value === null ? {} : {method:'POST',headers:{'content-type':'application/json'},
          body:JSON.stringify({action:'put',value})})});
      const body = await response.json();
      return {status:response.status, hidden:response.headers.get('set-cookie') === null,
        success:body.success === true, code:body.code === 'ok', preference:body.data?.preference};
    }""", {"url": url, "value": value})
    require(reply["status"] == 200 and reply["hidden"] and reply["success"] and reply["code"])
    return reply["preference"]


async def inspect_cookies(context, page, args, expected):
    cookies = await context.cookies(args.url)
    require(len(cookies) == 2)
    ids = [cookie for cookie in cookies if cookie["name"] == args.cookie_name]
    data = [cookie for cookie in cookies if cookie["name"] != args.cookie_name
            and re.fullmatch(r"[A-Za-z0-9]{40}", cookie["name"])]
    require(len(ids) == 1 and len(data) == 1 and ids[0] is not data[0])
    require(ids[0]["value"] != data[0]["name"])
    for cookie in cookies:
        require(cookie["httpOnly"] and cookie["path"] == "/" and cookie["sameSite"] == "Lax")
        require(not cookie["secure"] and len(cookie["value"]) > 40)
        require(len(cookie["name"]) + len(cookie["value"]) + 256 < 4096)
        require(expected not in cookie["value"] and "browser-first" not in cookie["value"]
                and "browser-second" not in cookie["value"])
    require(await page.evaluate("document.cookie") == "")


async def exercise(args, result):
    contexts = []
    browser = None
    playwright = None
    try:
        result["phase"] = "browser_launch"
        playwright = await async_playwright().start()
        browser = await playwright.chromium.launch(headless=True)
        result["browser_version"] = browser.version
        for _ in range(2):
            contexts.append(await browser.new_context())
        pages = [await context.new_page() for context in contexts]
        result["phase"] = "initial_navigation"
        for page in pages:
            response = await page.goto(args.url, wait_until="domcontentloaded")
            require(response is not None and response.status == 200)
            require(await fetch(page, args.url) is None)
        result["phase"] = "isolated_writes"
        values = ["browser-first", "browser-second"]
        for page, value in zip(pages, values):
            require(await fetch(page, args.url, value) == value)
        result["phase"] = "three_rounds"
        for _ in range(3):
            for page, value in zip(pages, values):
                require(await fetch(page, args.url) == value)
        result["checks"]["two_contexts_three_rounds_isolated"] = True
        result["phase"] = "new_page"
        extra = await contexts[0].new_page()
        response = await extra.goto(args.url, wait_until="domcontentloaded")
        require(response is not None and response.status == 200)
        require(await fetch(extra, args.url) == values[0])
        result["checks"]["automatic_cookie_roundtrip_nonempty_and_new_page"] = True
        result["phase"] = "cookie_boundaries"
        for context, page, value in zip(contexts, pages, values):
            await inspect_cookies(context, page, args, value)
        result["checks"]["two_roles_opaque_small_http_only_lax_host_cookies"] = True
        result["checks"]["document_cookie_and_set_cookie_header_not_visible"] = True
    finally:
        # 关闭有界；关闭失败同样不能给出 accepted=true。
        failed = False
        closers = [context.close for context in contexts]
        if browser is not None:
            closers.append(browser.close)
        if playwright is not None:
            closers.append(playwright.stop)
        for close in closers:
            try:
                await asyncio.wait_for(close(), min(args.timeout * 0.04, 3))
            except (Exception, asyncio.CancelledError):
                failed = True
        if failed:
            raise CheckFailed()



async def main(args, result):
    try:
        await asyncio.wait_for(exercise(args, result), args.timeout * 0.8)
        result["accepted"] = len(result["checks"]) == 4 and all(result["checks"].values())
        result["phase"] = "complete"
    except (Exception, asyncio.CancelledError):
        # 底层异常可能携带 URL、响应或 Cookie；只保留固定阶段。
        result["accepted"] = False


if __name__ == "__main__":
    args = options()
    result = {"accepted": False, "checks": {}, "browser_version": None, "url": args.url,
              "transport": "real Chromium HTTP browser; no TLS assertion",
              "manual_cookie_injection": False, "phase": "setup", "started": time.time()}
    with tempfile.TemporaryDirectory(prefix="openrust-c04d4-browser-", dir="/tmp") as temporary:
        os.environ["TMPDIR"] = temporary
        os.environ["TEMP"] = temporary
        os.environ["TMP"] = temporary
        asyncio.run(main(args, result))
    result["finished"] = time.time()
    with (args.result_directory / "result.json").open("x") as output:
        output.write(json.dumps(result, indent=2) + "\n")
    raise SystemExit(0 if result["accepted"] else 1)
