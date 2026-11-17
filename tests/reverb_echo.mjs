// Browser acceptance invoked by the Reverb transport test with pinned client packages.
import assert from "node:assert/strict";
import { createHash, createHmac } from "node:crypto";
import { createRequire } from "node:module";
import { join } from "node:path";

const [http, ws, prefix] = process.argv.slice(2);
const modules = process.env.OPENRUST_REVERB_ECHO_MODULES;
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.DOCS_PLAYWRIGHT_MODULE || "playwright");

const browser = await chromium.launch({ executablePath: process.env.DOCS_BROWSER_PATH });
try {
    const page = await browser.newPage();
    page.setDefaultTimeout(12_000);
    const authStatuses = [];
    page.on("response", response => {
        if (response.url() === `${http}/broadcasting/auth`) {
            authStatuses.push(response.status());
        }
    });
    await page.goto(http);
    await page.addScriptTag({ path: join(modules, "pusher-js/dist/web/pusher.js") });
    await page.addScriptTag({ path: join(modules, "laravel-echo/dist/echo.iife.js") });
    await page.evaluate(({ http, ws, prefix }) => {
        const endpoint = new URL(ws);
        window.echo = new window.Echo.default({
            broadcaster: "reverb",
            key: "test-key",
            wsHost: endpoint.hostname,
            wsPort: Number(endpoint.port),
            wsPath: prefix,
            forceTLS: false,
            enabledTransports: ["ws"],
            disableStats: true,
            channelAuthorization: {
                endpoint: `${http}/broadcasting/auth`,
                transport: "ajax",
                headers: { Accept: "application/json", "X-Identity": "0042" },
            },
        });
        window.probe = { subscriptions: [0, 0, 0], messages: [], roster: [] };
        const channels = [echo.channel("echo.news"), echo.private("orders.9"), echo.join("room.9")];
        for (const [index, channel] of channels.entries()) {
            channel.subscribed(() => probe.subscriptions[index]++)
                .listen(".echo.probe", data => probe.messages.push([index, data.sequence]));
        }
        channels[2].here(members => probe.roster = members);
    }, { http, ws, prefix });

    const waitForSubscriptions = count => page.waitForFunction(
        count => probe.subscriptions.every(value => value === count),
        count,
    );
    const publish = async sequence => {
        const path = "/apps/test/events";
        const body = JSON.stringify({
            name: "echo.probe",
            channels: ["echo.news", "private-orders.9", "presence-room.9"],
            data: JSON.stringify({ sequence }),
        });
        const query = new URLSearchParams({
            auth_key: "test-key",
            auth_timestamp: String(Math.floor(Date.now() / 1000)),
            auth_version: "1.0",
            body_md5: createHash("md5").update(body).digest("hex"),
        });
        query.sort();
        const signature = createHmac("sha256", "probe-secret")
            .update(`POST\n${path}\n${query}`).digest("hex");
        query.set("auth_signature", signature);
        const response = await fetch(`${ws}${prefix}${path}?${query}`, {
            method: "POST",
            body,
            signal: AbortSignal.timeout(3_000),
        });
        assert.equal(response.status, 200);
        assert.deepEqual(await response.json(), {});
    };
    const readMessages = () => page.evaluate(() => probe.messages);

    await waitForSubscriptions(1);
    assert.deepEqual(await page.evaluate(() => probe.roster), [{ name: "member", room: "9" }]);
    const firstSocket = await page.evaluate(() => echo.socketId());
    await publish(1);
    await page.waitForFunction(() => probe.messages.length === 3);

    await page.evaluate(() => {
        echo.connector.pusher.connection.connection.transport.socket.close(4200, "Reconnect probe");
    });
    await waitForSubscriptions(2);
    const nextSocket = await page.evaluate(() => echo.socketId());
    assert.ok(nextSocket && nextSocket !== firstSocket);
    await publish(2);
    await page.waitForFunction(() => probe.messages.length === 6);

    const messages = await readMessages();
    assert.deepEqual(messages.sort((a, b) => a[1] - b[1] || a[0] - b[0]), [
        [0, 1], [1, 1], [2, 1], [0, 2], [1, 2], [2, 2],
    ]);
    assert.deepEqual(authStatuses, [200, 200, 200, 200]);
    await page.evaluate(() => echo.disconnect());
    console.log("Chromium Echo public, private and presence subscriptions, signed delivery and automatic reconnect passed");
} finally {
    await browser.close();
}
