// Browser acceptance invoked by the Reverb transport test with pinned client packages.
import assert from "node:assert/strict";
import { createHash, createHmac, X509Certificate } from "node:crypto";
import { readFileSync } from "node:fs";
import { connect } from "node:net";
import { createServer } from "node:tls";
import { createRequire } from "node:module";
import { join } from "node:path";

let [http, ws, prefix, driver = "reverb"] = process.argv.slice(2);
const modules = process.env.OPENRUST_REVERB_ECHO_MODULES;
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.DOCS_PLAYWRIGHT_MODULE || "playwright");

const proxies = [];
const sockets = new Set();
const certPath = process.env.OPENRUST_REVERB_TLS_CERT;
const keyPath = process.env.OPENRUST_REVERB_TLS_KEY;
assert.equal(Boolean(certPath), Boolean(keyPath), "TLS certificate and key must be supplied together");
const launchArgs = [];
let browser;
try {
    if (certPath) {
        const cert = readFileSync(certPath);
        const key = readFileSync(keyPath);
        const spki = new X509Certificate(cert).publicKey.export({ type: "spki", format: "der" });
        // Pin only this fixture certificate; Node HTTPS continues to verify NODE_EXTRA_CA_CERTS.
        launchArgs.push(`--ignore-certificate-errors-spki-list=${createHash("sha256").update(spki).digest("base64")}`);
        const proxy = async endpoint => {
            const upstream = new URL(endpoint);
            const server = createServer({ cert, key, minVersion: "TLSv1.2" }, secure => {
                const plain = connect(Number(upstream.port), upstream.hostname);
                for (const socket of [secure, plain]) {
                    sockets.add(socket);
                    socket.on("close", () => sockets.delete(socket));
                }
                secure.on("error", () => plain.destroy());
                plain.on("error", () => secure.destroy());
                secure.pipe(plain).pipe(secure);
            });
            proxies.push(server);
            await new Promise((resolve, reject) => {
                server.once("error", reject);
                server.listen(0, "127.0.0.1", resolve);
            });
            return `https://localhost:${server.address().port}`;
        };
        http = await proxy(http);
        ws = await proxy(ws);
    }
    browser = await chromium.launch({ executablePath: process.env.DOCS_BROWSER_PATH, args: launchArgs });
    const page = await browser.newPage();
    page.setDefaultTimeout(12_000);
    const authStatuses = [];
    const userAuthStatuses = [];
    page.on("response", response => {
        if (response.url() === `${http}/broadcasting/auth`) {
            authStatuses.push(response.status());
        }
        if (response.url() === `${http}/broadcasting/user-auth`) {
            userAuthStatuses.push(response.status());
        }
    });
    await page.goto(http);
    await page.addScriptTag({ path: join(modules, "pusher-js/dist/web/pusher.js") });
    await page.addScriptTag({ path: join(modules, "laravel-echo/dist/echo.iife.js") });
    await page.evaluate(({ http, ws, prefix, driver }) => {
        const endpoint = new URL(ws);
        window.echo = new window.Echo.default({
            broadcaster: driver,
            cluster: "mt1",
            key: "test-key",
            wsHost: endpoint.hostname,
            wsPort: Number(endpoint.port),
            wssPort: Number(endpoint.port),
            wsPath: prefix,
            forceTLS: endpoint.protocol === "https:",
            enabledTransports: ["ws"],
            disableStats: true,
            channelAuthorization: {
                endpoint: `${http}/broadcasting/auth`,
                transport: "ajax",
                headers: { Accept: "application/json", "X-Identity": "0042" },
            },
            userAuthentication: {
                endpoint: `${http}/broadcasting/user-auth`,
                transport: "ajax",
                headers: { Accept: "application/json", "X-Identity": "0042" },
            },
        });
        window.probe = { subscriptions: [0, 0, 0], messages: [], roster: [], userMessages: [] };
        const pusher = echo.connector.pusher;
        pusher.user.bind("echo.probe", data => probe.userMessages.push(data.sequence));
        pusher.signin();
        const channels = [echo.channel("echo.news"), echo.private("orders.9"), echo.join("room.9")];
        for (const [index, channel] of channels.entries()) {
            channel.subscribed(() => probe.subscriptions[index]++)
                .listen(".echo.probe", data => probe.messages.push([index, data.sequence]));
        }
        channels[2].here(members => probe.roster = members);
    }, { http, ws, prefix, driver });

    const waitForSubscriptions = count => page.waitForFunction(
        count => probe.subscriptions.every(value => value === count)
            && echo.connector.pusher.user.serverToUserChannel?.subscribed,
        count,
    );
    const publish = async sequence => {
        if (driver === "pusher") {
            const response = await fetch(`${http}/fixture/publish/${sequence}`, {
                method: "POST",
                signal: AbortSignal.timeout(3_000),
            });
            assert.equal(response.status, 204);
            return;
        }
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
    const publishUser = async sequence => {
        const response = await fetch(`${http}/fixture/publish/${sequence}?user=true`, {
            method: "POST",
            signal: AbortSignal.timeout(3_000),
        });
        assert.equal(response.status, 204);
        await page.waitForFunction(sequence => probe.userMessages.includes(sequence), sequence);
    };
    const readMessages = () => page.evaluate(() => probe.messages);

    await waitForSubscriptions(1);
    assert.deepEqual(await page.evaluate(() => probe.roster), [{ name: "member", room: "9" }]);
    const firstSocket = await page.evaluate(() => echo.socketId());
    await publish(1);
    await publishUser(1);
    await page.waitForFunction(() => probe.messages.length === 3);

    await page.evaluate(() => {
        echo.connector.pusher.connection.connection.transport.socket.close(4200, "Reconnect probe");
    });
    await waitForSubscriptions(2);
    const nextSocket = await page.evaluate(() => echo.socketId());
    assert.ok(nextSocket && nextSocket !== firstSocket);
    await publish(2);
    await publishUser(2);
    await page.waitForFunction(() => probe.messages.length === 6);

    const messages = await readMessages();
    assert.deepEqual(messages.sort((a, b) => a[1] - b[1] || a[0] - b[0]), [
        [0, 1], [1, 1], [2, 1], [0, 2], [1, 2], [2, 2],
    ]);
    assert.deepEqual(authStatuses, [200, 200, 200, 200]);
    assert.deepEqual(userAuthStatuses, [200, 200]);
    assert.deepEqual(await page.evaluate(() => probe.userMessages), [1, 2]);
    assert.deepEqual(await page.evaluate(() => echo.connector.pusher.user.user_data),
        { id: "0042", user_info: { name: "signed user" } });
    if (certPath) {
        const protocol = await page.evaluate(() => echo.connector.pusher.connection.connection.transport.socket.url);
        assert.ok(protocol.startsWith("wss://"), protocol);
    }
    await page.evaluate(() => echo.disconnect());
    console.log(`Chromium Echo (${driver}, ${certPath ? "HTTPS/WSS certificate pinned" : "HTTP/WS"}) public, private and presence subscriptions, user sign-in, Artisan delivery and automatic reconnect passed`);
} finally {
    await browser?.close();
    for (const socket of sockets) socket.destroy();
    await Promise.all(proxies.map(server => new Promise(resolve => server.close(resolve))));
}
