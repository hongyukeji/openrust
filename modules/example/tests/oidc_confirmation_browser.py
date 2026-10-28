#!/usr/bin/env python3
"""执行生产 OIDC 确认页脚本，验证浏览器对 HTTP 与业务结果的判断。"""

import asyncio
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
import threading

from playwright.async_api import async_playwright


HTML = os.environ["OPENRUST_OIDC_CONFIRMATION_HTML"]
RESULTS = {
    "success": (200, b'{"success":true,"data":{"status":"bound"}}'),
    "business_failure": (200, b'{"success":false,"data":{"status":"failed"}}'),
    "http_failure": (500, b'{"success":true,"data":{"status":"bound"}}'),
    "invalid_json": (200, b"not-json"),
}


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        body = HTML.encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", "0")))
        scenario = next(
            (part.split("=", 1)[1] for part in self.headers.get("Cookie", "").split("; ") if part.startswith("scenario=")),
            "",
        )
        status, body = RESULTS[scenario]
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format, *_args):
        return


async def run():
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    origin = f"http://127.0.0.1:{server.server_port}"
    try:
        async with async_playwright() as playwright:
            browser = await playwright.chromium.launch(headless=True)
            try:
                for scenario, expected in (
                    ("success", "绑定成功"),
                    ("business_failure", "绑定失败，请重新发起授权"),
                    ("http_failure", "绑定失败，请重新发起授权"),
                    ("invalid_json", "请求失败，请重新发起授权"),
                ):
                    context = await browser.new_context()
                    await context.add_cookies(
                        [{"name": "scenario", "value": scenario, "url": origin}]
                    )
                    page = await context.new_page()
                    await page.goto(origin)
                    await page.locator("#email").fill("owner@example.test")
                    await page.locator("#password").fill("test-password")
                    await page.locator("button").click()
                    await page.get_by_text(expected, exact=True).wait_for(timeout=3000)
                    await context.close()
            finally:
                await browser.close()
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


asyncio.run(run())
