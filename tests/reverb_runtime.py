from pathlib import Path
from contextlib import AsyncExitStack
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
import uuid

from websockets.asyncio.client import connect

binary = Path(os.environ.get("REVERB_TEST_BINARY", "target/debug/main")).resolve()


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
               APP_DISABLED_RUNNERS="queue",
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


async def frame(peer, event, timeout=3):
    message = json.loads(await asyncio.wait_for(peer.recv(), timeout))
    assert message["event"] == event, message
    return message, json.loads(message["data"])


async def quiet(*peers):
    async def check(peer):
        try:
            message = await asyncio.wait_for(peer.recv(), 0.2)
        except TimeoutError:
            return
        raise AssertionError(f"Unexpected frame: {message}")

    await asyncio.gather(*(check(peer) for peer in peers))


async def open_peer(stack, port, key):
    peer = await stack.enter_async_context(connect(f"ws://127.0.0.1:{port}/app/{key}"))
    _, greeting = await frame(peer, "pusher:connection_established")
    return peer, greeting["socket_id"]


async def subscribe(peer, socket_id, channel, key, user=None):
    payload = {"channel": channel}
    signature = f"{socket_id}:{channel}"
    if user is not None:
        # Include JSON shapes and a number that must not pass through Lua doubles.
        details = {"user_id": user, "user_info": {"name": user, "nested": [], "serial": 9007199254740993}}
        payload["channel_data"] = json.dumps(details, separators=(",", ":"))
        signature += f":{payload['channel_data']}"
    token = hmac.new(b"probe-secret", signature.encode(), hashlib.sha256).hexdigest()
    payload["auth"] = f"{key}:{token}"
    await peer.send(json.dumps({"event": "pusher:subscribe", "data": payload}))
    _, data = await frame(peer, "pusher_internal:subscription_succeeded")
    return data


async def client_event(peer, channel, event):
    await peer.send(json.dumps({
        "event": event, "channel": channel, "user_id": "forged", "data": {"message": event}
    }))


def restart_managed_redis(container):
    run = os.environ["OPENRUST_ACCEPTANCE_RUN"]
    container_id, name, label = subprocess.check_output(
        ["docker", "inspect", "--type", "container", "--format",
         '{{.Id}}|{{.Name}}|{{index .Config.Labels "io.openrust.acceptance.run"}}', container],
        text=True, timeout=15,
    ).rstrip("\n").split("|", 2)
    if name != f"/{run}-redis" or label != run:
        raise RuntimeError("Redis restart requires the current acceptance run's fixture")
    subprocess.run(["docker", "restart", container_id], check=True, capture_output=True, timeout=15)


async def verify_cluster(base, redis_url, mysql_url, redis_container=None):
    prefix = uuid.uuid4().hex
    common = dict(os.environ, APP_HOST="127.0.0.1", CACHE_STORE="file",
                  DB_CONNECTION="mysql", DB_URL=mysql_url,
                  REVERB_ENABLED="true", REVERB_MODE="embedded",
                  APP_DISABLED_RUNNERS="queue,schedule",
                  REVERB_SERVER_HOST="127.0.0.1", REVERB_APP_SECRET="probe-secret",
                  REVERB_SCALING_ENABLED="true", REVERB_SCALING_CHANNEL=f"accept-{prefix}",
                  REDIS_URL=redis_url, REDIS_TIMEOUT="2")
    environments, ports = [], []
    for index in range(3):
        root = base / f"node-{index}"
        root.mkdir()
        http, reverb = socket_port(), socket_port()
        key, app_id = ("probe", "42") if index < 2 else ("other", "43")
        environments.append(dict(common, APP_BASE_PATH=str(root), APP_PORT=str(http),
                                 REVERB_SERVER_PORT=str(reverb), REVERB_APP_KEY=key,
                                 REVERB_APP_ID=app_id))
        ports.append((http, reverb))
    migrate = command(environments[0], "migrate", "--force")
    assert migrate.returncode == 0, (migrate.stdout, migrate.stderr)
    status = command(environments[1], "migrate:status")
    assert status.returncode == 0 and "create_users_table" in status.stdout, (status.stdout, status.stderr)

    processes = []
    async def start(index):
        process = subprocess.Popen([binary], env=environments[index], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        processes.append(process)
        http, reverb = ports[index]
        await wait_port(http, process)
        await wait_port(reverb, process)
        with urllib.request.urlopen(f"http://127.0.0.1:{http}/up", timeout=3) as health:
            assert health.status == 200
        return process

    room = "presence-room"
    private = "private-room"
    try:
        first_node = await start(0)
        await start(1)
        await start(2)
        first_port, second_port, other_port = [port for _, port in ports]
        async with AsyncExitStack() as clients:
            first, first_id = await open_peer(clients, first_port, "probe")
            duplicate, duplicate_id = await open_peer(clients, second_port, "probe")
            second, second_id = await open_peer(clients, second_port, "probe")
            outsider, outsider_id = await open_peer(clients, first_port, "probe")
            other, other_id = await open_peer(clients, other_port, "other")
            presence = (await subscribe(first, first_id, room, "probe", "alice"))["presence"]
            assert presence["ids"] == ["alice"] and presence["count"] == 1
            assert presence["hash"]["alice"]["nested"] == []
            assert presence["hash"]["alice"]["serial"] == 9007199254740993
            presence = (await subscribe(duplicate, duplicate_id, room, "probe", "alice"))["presence"]
            assert presence["ids"] == ["alice"] and presence["count"] == 1
            await quiet(first)
            presence = (await subscribe(second, second_id, room, "probe", "bob"))["presence"]
            assert presence["ids"] == ["alice", "bob"] and presence["count"] == 2
            for peer in (first, duplicate):
                _, added = await frame(peer, "pusher_internal:member_added")
                assert added["user_id"] == "bob"
            presence = (await subscribe(other, other_id, room, "other", "eve"))["presence"]
            assert presence["ids"] == ["eve"] and presence["count"] == 1
            await quiet(first, duplicate, second)

            for peer, peer_id, key in ((first, first_id, "probe"), (second, second_id, "probe"), (other, other_id, "other")):
                await subscribe(peer, peer_id, private, key)
            await subscribe(outsider, outsider_id, "private-other-room", "probe")
            await outsider.send(json.dumps({
                "event": "pusher:subscribe", "data": {"channel": private, "auth": "probe:invalid"}
            }))
            assert (await frame(outsider, "pusher:error"))[1]["code"] == 4009
            await client_event(outsider, private, "client-denied")
            _, denied = await frame(outsider, "pusher:error")
            assert denied["code"] == 4009
            await client_event(first, private, "client-private")
            result, payload = await frame(second, "client-private")
            assert "user_id" not in result and payload["message"] == "client-private"
            await quiet(first, second, duplicate, outsider, other)

            await client_event(first, room, "client-presence")
            for peer in (duplicate, second):
                result, payload = await frame(peer, "client-presence")
                assert result["user_id"] == "alice" and payload["message"] == "client-presence"
            await quiet(first, duplicate, second, outsider, other)

            await duplicate.close()
            await quiet(first, second)
            await first.close()
            _, removed = await frame(second, "pusher_internal:member_removed")
            assert removed["user_id"] == "alice"
            reconnected, socket_id = await open_peer(clients, first_port, "probe")
            presence = (await subscribe(reconnected, socket_id, room, "probe", "alice"))["presence"]
            assert presence["count"] == 2
            assert (await frame(second, "pusher_internal:member_added"))[1]["user_id"] == "alice"

            first_node.kill()
            assert first_node.wait(timeout=5) != 0
            _, expired = await frame(second, "pusher_internal:member_removed", timeout=16)
            assert expired["user_id"] == "alice"
            restarted_node = await start(0)
            fresh, socket_id = await open_peer(clients, first_port, "probe")
            presence = (await subscribe(fresh, socket_id, room, "probe", "carol"))["presence"]
            assert presence["ids"] == ["bob", "carol"] and presence["count"] == 2
            assert (await frame(second, "pusher_internal:member_added"))[1]["user_id"] == "carol"
            await quiet(second, other)

            if redis_container:
                restart_managed_redis(redis_container)
                await asyncio.gather(*(asyncio.wait_for(peer.wait_closed(), 10) for peer in (fresh, second, other)))
                loop = asyncio.get_running_loop()
                deadline = loop.time() + 16
                last_error = None
                while loop.time() < deadline:
                    recovered, socket_id = await open_peer(clients, first_port, "probe")
                    try:
                        presence = (await subscribe(recovered, socket_id, room, "probe", "recovered"))["presence"]
                    except (AssertionError, TimeoutError) as error:
                        last_error = error
                    else:
                        assert "recovered" in presence["ids"]
                        if presence["ids"] == ["recovered"]:
                            break
                    await recovered.close()
                    await asyncio.sleep(0.25)
                else:
                    raise AssertionError("Redis subscription did not recover with a clean roster") from last_error
                print("Redis restart closed presence sockets, then fresh subscriptions recovered without stale users")
                remaining, socket_id = await open_peer(clients, second_port, "probe")
                assert (await subscribe(remaining, socket_id, room, "probe", "dana"))["presence"]["count"] == 2
                assert (await frame(recovered, "pusher_internal:member_added"))[1]["user_id"] == "dana"
            else:
                remaining = second
            restarted_node.terminate()
            assert await asyncio.wait_for(asyncio.to_thread(restarted_node.wait), 5) == 0
            expected = "recovered" if redis_container else "carol"
            assert (await frame(remaining, "pusher_internal:member_removed"))[1]["user_id"] == expected
            await quiet(remaining)
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate()
                assert process.wait(timeout=8) == 0, process.communicate()
    print("Three applications shared isolated MySQL and Redis; global presence, client events, app isolation and lease expiry passed")


if __name__ == "__main__":
    if os.environ.get("REVERB_TEST_RESTART_REDIS_CONTAINER"):
        raise SystemExit("For the Redis restart test, unset REVERB_TEST_RESTART_REDIS_CONTAINER "
                         "and run docker/accept.py --mysql --redis -- python3 tests/reverb_runtime.py")
    managed_redis = os.environ.get("OPENRUST_ACCEPTANCE_REDIS_URL")
    redis_url = managed_redis or os.environ.get("REVERB_TEST_REDIS_URL")
    mysql_url = os.environ.get("OPENRUST_ACCEPTANCE_MYSQL_URL") or os.environ.get("REVERB_TEST_MYSQL_URL")
    with tempfile.TemporaryDirectory(prefix="openrust-reverb-") as directory:
        asyncio.run(verify(Path(directory)))
        if redis_url:
            asyncio.run(verify_redis(Path(directory), redis_url))
            if mysql_url:
                redis_container = os.environ["OPENRUST_ACCEPTANCE_REDIS_CONTAINER"] if managed_redis else None
                asyncio.run(verify_cluster(Path(directory), redis_url, mysql_url, redis_container))
