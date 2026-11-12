from pathlib import Path
import asyncio
import hashlib
import hmac
import json
import os
import socket
import subprocess
import tempfile
import urllib.parse
import urllib.request

from websockets.asyncio.client import connect

binary = Path("target/debug/main").resolve()


def socket_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


async def wait_port(port, server):
    for _ in range(200):
        if server.poll() is not None:
            raise RuntimeError(server.communicate())
        try:
            _, writer = await asyncio.open_connection("127.0.0.1", port)
        except OSError:
            await asyncio.sleep(0.025)
        else:
            writer.close()
            await writer.wait_closed()
            return
    raise RuntimeError("listener did not start")


def command(env, *arguments):
    return subprocess.run([binary, "artisan", *arguments], env=env, capture_output=True, text=True, timeout=10)


async def verify(base):
    http, reverb, standalone = [socket_port() for _ in range(3)]
    env = dict(os.environ, APP_BASE_PATH=str(base), APP_HOST="127.0.0.1",
               APP_PORT=str(http), CACHE_STORE="file", REVERB_ENABLED="true",
               REVERB_SERVER_HOST="127.0.0.1", REVERB_SERVER_PORT=str(reverb),
               REVERB_APP_KEY="probe", REVERB_APP_SECRET="probe-secret", REVERB_APP_ID="42")
    with subprocess.Popen([binary], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as server:
        try:
            await wait_port(http, server)
            await wait_port(reverb, server)
            with urllib.request.urlopen(f"http://127.0.0.1:{http}/up", timeout=3) as health:
                assert health.status == 200
            async with connect(f"ws://127.0.0.1:{reverb}/app/probe") as ws:
                assert json.loads(await ws.recv())["event"] == "pusher:connection_established"
                assert command(env, "inspire").returncode == 0
                conflict = command(dict(env, APP_PORT=str(socket_port())), "reverb:start")
                assert conflict.returncode != 0 and "Cannot bind Reverb listener" in conflict.stderr
                embedded_conflict = subprocess.run([binary], env=dict(env, APP_PORT=str(socket_port())), capture_output=True, text=True, timeout=5)
                assert embedded_conflict.returncode != 0 and "Cannot bind Reverb listener" in embedded_conflict.stderr
                server.terminate()
                await asyncio.wait_for(ws.wait_closed(), 5)
        finally:
            if server.poll() is None:
                server.terminate()
            assert server.wait(timeout=5) == 0, server.communicate()
    env["REVERB_MODE"] = "standalone"
    with subprocess.Popen([binary, "artisan", "reverb:start", f"--port={standalone}", "--path=/socket"], env=env,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE) as server:
        try:
            await wait_port(standalone, server)
            async with connect(f"ws://127.0.0.1:{standalone}/socket/app/probe") as ws:
                assert json.loads(await ws.recv())["event"] == "pusher:connection_established"
                result = command(env, "reverb:restart")
                assert result.returncode == 0, result.stderr
                await asyncio.wait_for(ws.wait_closed(), 5)
            assert server.wait(timeout=5) == 0, server.communicate()
        finally:
            if server.poll() is None:
                server.terminate()
                server.wait(timeout=5)
    print("Embedded HTTP and WebSocket, bind failure, Artisan isolation, standalone overrides and restart passed")


async def verify_redis(base, redis_url):
    ports = [socket_port() for _ in range(2)]
    env = dict(os.environ, APP_BASE_PATH=str(base), CACHE_STORE="file",
               REVERB_SERVER_HOST="127.0.0.1", REVERB_APP_KEY="probe",
               REVERB_APP_SECRET="probe-secret", REVERB_APP_ID="42",
               REVERB_SCALING_ENABLED="true", REVERB_SCALING_CHANNEL="reverb-acceptance",
               REDIS_URL=redis_url, REDIS_TIMEOUT="3")
    processes = [
        subprocess.Popen([binary, "artisan", "reverb:start", f"--port={port}"], env=env,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        for port in ports
    ]
    try:
        for port, process in zip(ports, processes, strict=True):
            await wait_port(port, process)
        async with (
            connect(f"ws://127.0.0.1:{ports[0]}/app/probe") as first,
            connect(f"ws://127.0.0.1:{ports[1]}/app/probe") as second,
        ):
            socket_ids = []
            for peer in [first, second]:
                hello = json.loads(await asyncio.wait_for(peer.recv(), 3))
                socket_ids.append(json.loads(hello["data"])["socket_id"])
                await peer.send(json.dumps({"event": "pusher:subscribe", "data": {"channel": "probe"}}))
                response = json.loads(await asyncio.wait_for(peer.recv(), 3))
                assert response["event"] == "pusher_internal:subscription_succeeded"
            path = "/apps/42/events"
            body = json.dumps({"name": "distributed", "data": "{}", "channels": ["probe"],
                               "socket_id": socket_ids[0]}, separators=(",", ":")).encode()
            query = {"auth_key": "probe", "auth_timestamp": "1700000000", "auth_version": "1.0",
                     "body_md5": hashlib.md5(body).hexdigest()}
            signing = "POST\n" + path + "\n" + "&".join(f"{key}={value}" for key, value in sorted(query.items()))
            query["auth_signature"] = hmac.new(b"probe-secret", signing.encode(), hashlib.sha256).hexdigest()
            url = f"http://127.0.0.1:{ports[0]}{path}?{urllib.parse.urlencode(query)}"
            with urllib.request.urlopen(urllib.request.Request(url, data=body), timeout=3) as response:
                assert response.status == 200
            assert json.loads(await asyncio.wait_for(second.recv(), 3))["event"] == "distributed"
            for peer in [first, second]:
                try:
                    await asyncio.wait_for(peer.recv(), .2)
                except TimeoutError:
                    pass
                else:
                    raise AssertionError("Redis delivery was duplicated or sent to the origin")
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate()
            assert process.wait(timeout=5) == 0, process.communicate()
    print("Redis signed publication reached a different process once and excluded its source socket")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="openrust-reverb-") as directory:
        asyncio.run(verify(Path(directory)))
        if redis_url := os.environ.get("REVERB_TEST_REDIS_URL"):
            asyncio.run(verify_redis(Path(directory), redis_url))
