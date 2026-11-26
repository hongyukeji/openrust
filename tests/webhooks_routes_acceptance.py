"""公开包路由／控制器、具名工厂及真实 HTTP／Artisan Worker 的隔离三库消费者。"""
import argparse,base64,hashlib,hmac,http.client,json,os,re,runpy,socket,subprocess,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument("binary",type=Path);args=p.parse_args()
helpers=runpy.run_path(str(ROOT/"tests/sms_worker.py"))
http_helpers=runpy.run_path(str(ROOT/"tests/webhooks_acceptance.py"),run_name="webhook_http_helpers")
SECRET="route-private-key"
STANDARD="whsec_C2FVsBQIhrscChlQIMV+b5sSYspob7oD"

def command(binary,env,*tokens):
 result=subprocess.run([str(binary),"artisan",*tokens],env=env,capture_output=True,text=True,timeout=45)
 assert result.returncode==0,result.stdout+result.stderr
 return result.stdout

def write_database(driver, sql, statement):
    if driver == "sqlite":
        import sqlite3
        from contextlib import closing
        with closing(sqlite3.connect(helpers["database_container"]())) as connection:
            connection.execute("PRAGMA foreign_keys = ON")
            connection.execute(statement)
            connection.commit()
    else:
        sql(statement)


def check_processing_lease(binary, env, root, port, driver, sql):
    current = env | {"DB_QUEUE_RETRY_AFTER": "8"}
    command(binary, current, "route:clear")
    work = [str(binary), "artisan", "queue:work", "database",
            "--queue=webhook-receipts", "--once", "--tries=1"]

    def worker(marker, **extra):
        return current | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(root / marker),
                          "FIXTURE_WEBHOOK_FAILED_HOOK": str(root / f"{marker}-failure.json"), **extra}

    def run(marker, **extra):
        result = subprocess.run(work, env=worker(marker, **extra),
                                capture_output=True, text=True, timeout=20)
        expected = 79 if "FIXTURE_LEASE_CRASH" in extra else 0
        assert result.returncode == expected, result.stdout + result.stderr

    def dispatch(body):
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            signature = hmac.new(SECRET.encode(), body, hashlib.sha256).hexdigest()
            conn.request("POST", "/api/package/leased", body, {
                "Signature": signature, "Content-Type": "application/octet-stream",
            })
            response = conn.getresponse()
            assert response.status == 200, response.read()
        finally:
            conn.close()
        return json.loads(sql("SELECT payload FROM jobs"))["data"]["id"]

    def state(receipt):
        assert re.fullmatch(r"[0-9a-f-]{36}", receipt)
        return sql(
            "SELECT processing_generation,"
            "CASE WHEN processing_token IS NULL THEN 0 ELSE 1 END,"
            "CASE WHEN processed_at IS NULL THEN 0 ELSE 1 END "
            f"FROM webhook_calls WHERE id='{receipt}'"
        )

    def attempts(receipt):
        assert re.fullmatch(r"[0-9a-f-]{36}", receipt)
        return sql(
            "SELECT generation,outcome,CASE WHEN finished_at IS NULL THEN 0 ELSE 1 END "
            f"FROM webhook_processing_attempts WHERE call_id='{receipt}' ORDER BY generation"
        )

    def inspect(receipt, *options, scope="webhook-acceptance"):
        return json.loads(command(binary, current, "webhook-client:inspect", receipt,
                                  "--scope", scope, *options))

    def inspect_http(receipt, kind, expected=200, scope="webhook-acceptance", authorized=True):
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            headers = {"x-fixture-inspector": "fixture-operator"} if authorized else {}
            conn.request("GET", f"/api/inbound-inspect/{scope}/{kind}/{receipt}", headers=headers)
            response = conn.getresponse()
            body = json.loads(response.read())
            assert response.status == expected, (response.status, body)
            return body
        finally:
            conn.close()

    log_path = root / "processing-lease.log"
    with log_path.open("w+") as log:
        server = http_helpers["serve"](binary, current, log)
        try:
            released = dispatch(b"leased-release")
            fresh = inspect(released)
            assert fresh["id"] == released and fresh["profile"] == "leased" and fresh["revision"]
            assert fresh["state"] == "received" and fresh["generation"] == fresh["replay_generation"] == 0
            assert fresh["received_at"] > 0 and not fresh["has_exception"]
            assert fresh["processed_at"] is None and fresh["processing_until"] is None
            assert inspect(released, "--database=fixture-foreign") == fresh
            assert inspect_http(released, "receipt") == fresh
            assert inspect_http(released, "receipt", 403, authorized=False) == {}
            assert inspect_http(released, "receipt", 403, scope="outside") == {}
            assert inspect_http("00000000-0000-0000-0000-000000000000", "receipt", 404) == {}
            assert inspect(released, scope="outside") is None
            run("release")
            assert state(released) == "1\t0\t0" and not (root / "release").exists()
            run("release")
            assert state(released) == "2\t0\t1" and (root / "release").exists()
            assert attempts(released) == "1\treleased\t1\n2\tcompleted\t1"
            settled = inspect(released)
            assert settled["state"] == "completed" and settled["processed_at"] > 0
            page = inspect(released, "--attempts", "--limit=1")
            assert page["next_after"] == 1 and len(page["data"]) == 1
            assert page["data"][0]["generation"] == 1 and page["data"][0]["outcome"] == "released"
            assert page["data"][0]["finished_at"] >= page["data"][0]["started_at"]
            assert inspect_http(released, "attempts") == page
            final_page = inspect(released, "--attempts", "--after=1", "--limit=1")
            assert final_page["next_after"] is None and len(final_page["data"]) == 1
            assert final_page["data"][0]["generation"] == 2 and final_page["data"][0]["outcome"] == "completed"
            empty_page = {"data": [], "next_after": None}
            assert inspect(released, "--attempts", scope="outside") == empty_page
            assert inspect(released, "--attempts", "--after=18446744073709551615") == empty_page
            for options in (("--attempts", "--limit=101"), ("--after=3",)):
                invalid = subprocess.run([str(binary), "artisan", "webhook-client:inspect", released,
                                          "--scope=webhook-acceptance", *options], env=current,
                                          capture_output=True, text=True, timeout=20)
                assert invalid.returncode != 0

            duplicate = (
                "INSERT INTO jobs (queue,payload,attempts,reserved_at,available_at,created_at) "
                "SELECT queue,payload,0,NULL,available_at,created_at FROM jobs"
            )
            halted = dispatch(b"leased-stop-on-error")
            failed_count = int(sql("SELECT COUNT(*) FROM failed_jobs"))
            write_database(driver, sql, duplicate)
            run("halted")
            terminal_state = (state(halted), attempts(halted))
            assert terminal_state == ("1\t0\t0", "1\tstopped\t1"), terminal_state
            assert sql(f"SELECT CASE WHEN processing_stopped_at IS NULL THEN 0 ELSE 1 END "
                       f"FROM webhook_calls WHERE id='{halted}'") == "1"
            assert sql("SELECT COUNT(*) FROM jobs") == "1"
            assert int(sql("SELECT COUNT(*) FROM failed_jobs")) == failed_count + 1
            assert "private-terminal-sentinel" not in sql("SELECT exception FROM failed_jobs")
            first_hook = json.loads((root / "halted-failure.json").read_text())
            assert first_hook == {
                "id": halted, "scope": "webhook-acceptance", "profile": "leased",
                "queue": "webhook-receipts", "connection": "database", "attempts": 1, "failed_jobs": 1,
                "exception": {"code": 0, "message": "Webhook 处理已终止"},
                "redacted_error": True, "expected_body": True,
            }, first_hook
            run("halted-duplicate")
            assert state(halted) == "1\t0\t0" and attempts(halted) == "1\tstopped\t1"
            assert sql("SELECT COUNT(*) FROM jobs") == "0"
            assert int(sql("SELECT COUNT(*) FROM failed_jobs")) == failed_count + 1
            assert not (root / "halted").exists() and not (root / "halted-duplicate").exists()
            assert not (root / "halted-duplicate-failure.json").exists()
            record = json.loads(command(binary, current, "webhooks:record", halted, "webhook-acceptance"))
            assert record["exception"] == {"code":0,"message":"Webhook 处理已终止"}
            stopped = inspect(halted)
            assert stopped["state"] == "stopped" and stopped["stopped_at"] > 0 and stopped["has_exception"]
            assert not {"exception", "body", "url", "headers", "payload", "scope"}.intersection(stopped)
            assert inspect_http(halted, "receipt") == stopped
            write_database(driver, sql, f"DELETE FROM failed_jobs WHERE payload LIKE '%{halted}%'")

            completed = dispatch(b"leased-stop-on-success")
            run("completed-with-control")
            assert state(completed) == "1\t0\t1" and attempts(completed) == "1\tcompleted\t1"
            assert (root / "completed-with-control").exists()
            assert not (root / "completed-with-control-failure.json").exists()
            assert int(sql("SELECT COUNT(*) FROM failed_jobs")) == failed_count
            command(binary, current, "webhooks:record", completed, "webhook-acceptance",
                    '--save-exception={"message":"private-receipt-diagnostic-sentinel"}')
            observed = inspect(completed)
            assert observed["has_exception"] and observed["state"] == "completed"
            assert "private-receipt-diagnostic-sentinel" not in json.dumps(observed)

            mismatched = dispatch(b"leased-untrusted-queue-revision")
            job = json.loads(sql("SELECT payload FROM jobs"))
            original = job["data"]["revision"]
            last_attempt = job["options"]["max_tries"]
            assert re.fullmatch(r"[a-zA-Z0-9_.:-]+", original)
            assert isinstance(last_attempt, int) and last_attempt > 0
            write_database(driver, sql,
                           f"UPDATE jobs SET payload=replace(payload, '\"revision\":\"{original}\"', "
                           f"'\"revision\":\"not-the-current-revision\"'), attempts={last_attempt}")
            run("wrong-revision")
            assert not (root / "wrong-revision-failure.json").exists()
            assert state(mismatched) == "0\t0\t0" and not attempts(mismatched)
            assert int(sql("SELECT COUNT(*) FROM failed_jobs")) == failed_count + 1
            write_database(driver, sql, f"DELETE FROM failed_jobs WHERE payload LIKE '%{mismatched}%'")

            concurrent = dispatch(b"leased-concurrent")
            write_database(driver, sql, duplicate)
            entered = root / "first-entered"
            first = subprocess.Popen(
                work, env=worker("first", FIXTURE_LEASE_ENTERED=str(entered)), stdout=log, stderr=log,
            )
            try:
                deadline = time.monotonic() + 10
                while not entered.exists() and first.poll() is None and time.monotonic() < deadline:
                    time.sleep(.025)
                assert entered.exists()
                assert state(concurrent) == "1\t1\t0"
                second = root / "second-entered"
                run("second", FIXTURE_LEASE_ENTERED=str(second))
                assert not second.exists()
                assert first.wait(timeout=10) == 0
                assert (root / "first").exists() and state(concurrent) == "1\t0\t1"
                assert sql("SELECT COUNT(*) FROM jobs") == "1"
            finally:
                if first.poll() is None:
                    first.kill()
                    first.wait(timeout=10)
            time.sleep(6.2)
            run("second", FIXTURE_LEASE_ENTERED=str(second))
            assert sql("SELECT COUNT(*) FROM jobs") == "0"
            assert not second.exists() and state(concurrent) == "1\t0\t1"

            crashed = dispatch(b"leased-crash")
            run("crash", FIXTURE_LEASE_CRASH="1")
            assert state(crashed) == "1\t1\t0" and not (root / "crash").exists()
            assert attempts(crashed) == "1\trunning\t0"
            assert inspect(crashed)["state"] == "processing"
            run("recovered")
            assert state(crashed) == "1\t1\t0" and not (root / "recovered").exists()
            time.sleep(8.2)
            assert inspect(crashed)["state"] == "lease_expired"
            run("recovered")
            assert state(crashed) == "2\t0\t1" and (root / "recovered").exists()
            assert attempts(crashed) == "1\texpired\t1\n2\tcompleted\t1"
            assert sql("SELECT COUNT(*) FROM jobs") == "0"
            assert "private-failed-hook-sentinel" not in log_path.read_text()

            stale = dispatch(b"leased-stale")
            write_database(driver, sql, duplicate)
            gate = root / "stale-gate"
            ready = root / "stale-ready"
            old = subprocess.Popen(
                work, env=worker("old", FIXTURE_OLD_ENTERED=str(ready), FIXTURE_OLD_GATE=str(gate)),
                stdout=log, stderr=log,
            )
            try:
                deadline = time.monotonic() + 7
                while not ready.exists() and old.poll() is None and time.monotonic() < deadline:
                    time.sleep(.025)
                assert ready.exists() and attempts(stale) == "1\trunning\t0"
                write_database(driver, sql, f"UPDATE webhook_calls SET processing_until=0 WHERE id='{stale}'")
                run("replacement")
                assert (root / "replacement").exists()
                assert state(stale) == "2\t0\t1"
                assert attempts(stale) == "1\texpired\t1\n2\tcompleted\t1"
                assert old.poll() is None
                gate.write_text("ready")
                assert old.wait(timeout=10) == 0
            finally:
                if old.poll() is None:
                    old.kill()
                    old.wait(timeout=10)
            assert state(stale) == "2\t0\t1"
            assert attempts(stale) == "1\texpired\t1\n2\tcompleted\t1"
            record = json.loads(command(binary, current, "webhooks:record", stale, "webhook-acceptance"))
            assert record["exception"] is None and not (root / "old").exists()
            run("stale-ack")
            assert sql("SELECT COUNT(*) FROM jobs") == "0"
        except BaseException:
            log.flush()
            print(log_path.read_text())
            raise
        finally:
            http_helpers["stop"](server)
    print(f"{driver}: 接收处理租约 13 个真实 HTTP、15 个独立 Worker；主动释放、终态回调、脱敏状态和分页、失配拒绝、崩溃接管及旧令牌隔离通过")


def check_verified_event_identity(binary, env, root, port, driver, sql):
    from concurrent.futures import ThreadPoolExecutor

    secret = base64.b64decode(STANDARD.removeprefix("whsec_"))
    scope = "webhook-acceptance"
    body = b'{"source":"acceptance"}'
    custom_body = b'{"event":"signed-body-id"}'
    path = "/api/package/event-dedup"
    now = int(time.time())
    original = int(sql("SELECT COUNT(*) FROM webhook_calls"))

    def send(target, event, value=body, timestamp=now, expected=200, listen=port, unsigned=False):
        signature = hmac.new(secret, f"{event}.{timestamp}.".encode() + value, hashlib.sha256)
        headers = {
            "Accept": "application/json", "Content-Type": "application/json",
            "webhook-id": event, "webhook-timestamp": str(timestamp),
            "webhook-signature": "v1," + base64.b64encode(signature.digest()).decode(),
        }
        if unsigned:
            headers["Signature"] = hmac.new(SECRET.encode(), value, hashlib.sha256).hexdigest()
        connection = http.client.HTTPConnection("127.0.0.1", listen, timeout=15)
        try:
            connection.request("POST", target, value, headers)
            response = connection.getresponse()
            result = response.read()
            assert response.status == expected, (target, expected, response.status, result)
            if expected == 200:
                assert json.loads(result) == {"message": "ok"}
            assert event.encode() not in result and STANDARD.encode() not in result
        finally:
            connection.close()

    def counts(receipts, jobs):
        assert int(sql("SELECT COUNT(*) FROM webhook_calls")) == original + receipts
        assert int(sql("SELECT COUNT(*) FROM webhook_event_receipts")) == receipts
        assert int(sql("SELECT COUNT(*) FROM jobs")) == jobs

    with (root / "identity-refuse-enqueue.log").open("w+") as log:
        missing = http_helpers["serve"](binary, env | {"DB_QUEUE_TABLE": "identity_missing_jobs"}, log)
        try:
            send(path, "stable", expected=500)
        finally:
            http_helpers["stop"](missing)
    counts(0, 0)

    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        other_port = sock.getsockname()[1]
    held = root / "identity-held"
    with (root / "identity-first.log").open("w+") as first_log, (root / "identity-peer.log").open("w+") as peer_log:
        first = http_helpers["serve"](binary, env | {
            "FIXTURE_ATOMIC_CRASH": "hold-after-enqueue", "FIXTURE_ATOMIC_HELD": str(held),
        }, first_log)
        try:
            peer = http_helpers["serve"](binary, env | {
                "APP_PORT": str(other_port), "APP_URL": f"http://127.0.0.1:{other_port}",
            }, peer_log)
            try:
                with ThreadPoolExecutor(max_workers=2) as clients:
                    a = clients.submit(send, path, "stable")
                    deadline = time.monotonic() + 5
                    while not held.exists() and not a.done() and time.monotonic() < deadline:
                        time.sleep(.025)
                    assert held.exists() and not a.done()
                    b = clients.submit(send, path, "stable", timestamp=now + 1, listen=other_port)
                    a.result()
                    b.result()
                counts(1, 1)
                send(path, "stable", listen=other_port)
                send(path, "stable", value=b'{"source":"different"}', expected=409, listen=other_port)
                send(path + "?changed=1", "stable", expected=409, listen=other_port)
                send(path, "disallowed.id", expected=500, listen=other_port)
                send("/api/package/event-unsigned", "unsigned", unsigned=True, expected=500, listen=other_port)
                counts(1, 1)

                send(path, "STABLE", listen=other_port)
                send("/api/package/event-other", "stable", listen=other_port)
                custom_path = "/api/package/event-custom"
                send(custom_path, "first-unsigned-header", value=custom_body, unsigned=True, listen=other_port)
                send(custom_path, "changed-unsigned-header", value=custom_body, unsigned=True, listen=other_port)
                counts(4, 4)
                saved_ids = sql("SELECT call_id FROM webhook_event_receipts").split()
                for receipt in saved_ids:
                    record = json.loads(command(binary, env, "webhooks:record", receipt, scope))
                    assert record["name"] in ("event-dedup", "event-other", "event-custom")
                    assert bytes(record["body"]) == (custom_body if record["name"] == "event-custom" else body)
                for index in range(4):
                    marker = root / f"identity-worker-{index}.json"
                    command(binary, env | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(marker)},
                            "queue:work", "database", "--queue=webhook-receipts", "--once")
                    assert bytes(json.loads(marker.read_text())["body"]) in (body, custom_body)
                send(path, "stable", listen=other_port)
                counts(4, 0)
                for receipt in saved_ids:
                    assert re.fullmatch(r"[0-9a-f-]{36}", receipt)
                    write_database(driver, sql, f"DELETE FROM webhook_calls WHERE id='{receipt}'")
                counts(0, 0)
            finally:
                http_helpers["stop"](peer)
        except BaseException:
            first_log.flush()
            peer_log.flush()
            print((root / "identity-first.log").read_text())
            print((root / "identity-peer.log").read_text())
            raise
        finally:
            http_helpers["stop"](first)
    print("已签名事件去重 13 个真实 HTTP、4 个独立 Worker；两进程并发、原子回滚、冲突、大小写、配置隔离和自定义验证器通过")


def check_deferred_processing(binary, env, root, port, driver, sql):
    current = env | {"DB_QUEUE_RETRY_AFTER": "8"}
    failed_before = int(sql("SELECT COUNT(*) FROM failed_jobs"))

    def work(marker, **extra):
        command(binary, current | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(root / marker), **extra},
                "queue:work", "database", "--queue=webhook-receipts", "--once")

    def dispatch(body):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        signature = hmac.new(SECRET.encode(), body, hashlib.sha256).hexdigest()
        try:
            connection.request("POST", "/api/package/deferred", body, {
                "Signature": signature, "Content-Type": "application/octet-stream",
            })
            response = connection.getresponse()
            assert response.status == 200, response.read()
        finally:
            connection.close()
        payload = json.loads(sql("SELECT payload FROM jobs"))
        assert payload["options"]["max_tries"] == 1
        receipt = payload["data"]["id"]
        assert re.fullmatch(r"[0-9a-f-]{36}", receipt)
        return receipt, payload["uuid"]

    def state(receipt):
        return sql(
            "SELECT processing_generation,processing_deferrals,"
            "CASE WHEN processing_next_at IS NULL THEN 0 ELSE 1 END,"
            "CASE WHEN processed_at IS NULL THEN 0 ELSE 1 END,"
            "CASE WHEN processing_stopped_at IS NULL THEN 0 ELSE 1 END "
            f"FROM webhook_calls WHERE id='{receipt}'"
        )

    log_path = root / "deferred-http.log"
    with log_path.open("w+") as log:
        server = http_helpers["serve"](binary, current, log)
        try:
            waiting, first = dispatch(b"deferred-wait")
            work("wait")
            assert state(waiting) == "1\t1\t1\t0\t0" and not (root / "wait").exists()
            inspection = json.loads(command(binary, current, "webhook-client:inspect", waiting,
                                             "--scope=webhook-acceptance"))
            assert inspection["state"] == "deferred" and inspection["next_processing_at"] > 0
            next_task = json.loads(sql("SELECT payload FROM jobs"))
            assert next_task["uuid"] != first and next_task["data"]["id"] == waiting
            assert sql("SELECT attempts FROM jobs") == "0"
            # 固定「尚未到期」状态，避免启动独立 Worker 的耗时用完两秒窗口。
            write_database(driver, sql, "UPDATE webhook_calls SET processing_next_at=processing_next_at+300 "
                           f"WHERE id='{waiting}'")
            write_database(driver, sql, "UPDATE jobs SET available_at=available_at+300")
            write_database(driver, sql,
                           "INSERT INTO jobs (queue,payload,attempts,reserved_at,available_at,created_at) "
                           "SELECT queue,payload,0,NULL,0,created_at FROM jobs")
            work("early", FIXTURE_DEFERRED_READY="1")
            early_state = state(waiting)
            assert not (root / "early").exists() and early_state == "1\t1\t1\t0\t0", early_state
            assert sql("SELECT COUNT(*) FROM jobs") == "1"
            write_database(driver, sql, f"UPDATE webhook_calls SET processing_next_at=0 WHERE id='{waiting}'")
            write_database(driver, sql, "UPDATE jobs SET available_at=0")
            work("ready", FIXTURE_DEFERRED_READY="1")
            assert (root / "ready").exists() and state(waiting) == "2\t1\t0\t1\t0"
            assert sql("SELECT COUNT(*) FROM jobs") == "0"

            waking, _ = dispatch(b"deferred-wake")
            work("wake-pending")
            assert state(waking) == "1\t1\t1\t0\t0"
            wake_command = "webhooks:wake-fixture"
            scope = "webhook-acceptance"
            wake_at = sql(f"SELECT processing_next_at FROM webhook_calls WHERE id='{waking}'")
            result = subprocess.run(
                [str(binary), "artisan", wake_command, waking, scope],
                env=current | {"DB_QUEUE_TABLE": "missing_wake_queue"},
                capture_output=True, text=True, timeout=15,
            )
            assert result.returncode != 0
            assert sql(f"SELECT processing_next_at FROM webhook_calls WHERE id='{waking}'") == wake_at
            assert sql("SELECT COUNT(*) FROM jobs") == "1"
            assert command(binary, current, wake_command, waking, "webhook-custom").strip() == "false"

            from concurrent.futures import ThreadPoolExecutor
            with ThreadPoolExecutor(max_workers=2) as callers:
                requests = [callers.submit(command, binary, current, wake_command, waking, scope)
                            for _ in range(2)]
                assert sorted(request.result().strip() for request in requests) == ["false", "true"]
            assert sql("SELECT COUNT(*) FROM jobs") == "2"
            work("woken", FIXTURE_DEFERRED_READY="1")
            assert state(waking) == "2\t1\t0\t1\t0" and (root / "woken").exists()
            assert sql(f"SELECT COUNT(processing_deferred_route) FROM webhook_calls WHERE id='{waking}'") == "0"
            assert command(binary, current, wake_command, waking, scope).strip() == "false"
            time.sleep(6.2)
            work("wake-duplicate", FIXTURE_DEFERRED_READY="1")
            assert state(waking) == "2\t1\t0\t1\t0" and not (root / "wake-duplicate").exists()
            assert sql("SELECT COUNT(*) FROM jobs") == "0"

            exhausted, _ = dispatch(b"deferred-exhaust")
            work("exhaust")
            assert state(exhausted) == "1\t1\t1\t0\t0"
            time.sleep(2.2)
            work("exhaust")
            assert state(exhausted) == "2\t1\t0\t0\t1" and not (root / "exhaust").exists()
            assert int(sql("SELECT COUNT(*) FROM failed_jobs")) == failed_before + 1
            assert sql("SELECT COUNT(*) FROM jobs") == "0"

            permanent, _ = dispatch(b"deferred-permanent")
            work("permanent")
            assert state(permanent) == "1\t0\t0\t0\t1"
            assert int(sql("SELECT COUNT(*) FROM failed_jobs")) == failed_before + 2

            retryable, _ = dispatch(b"deferred-retryable")
            work("retryable")
            assert state(retryable) == "1\t0\t0\t0\t0"
            assert int(sql("SELECT COUNT(*) FROM failed_jobs")) == failed_before + 3

            expired, _ = dispatch(b"deferred-expire")
            work("expired")
            assert state(expired) == "1\t1\t1\t0\t0"
            time.sleep(8.2)
            work("expired")
            assert state(expired) == "1\t1\t0\t0\t1" and not (root / "expired").exists()
            assert sql("SELECT COUNT(*) FROM jobs") == "0"
            for receipt, expected in (
                (waiting, "deferred\ncompleted"), (waking, "deferred\ncompleted"), (exhausted, "deferred\nstopped"),
                (permanent, "stopped"), (retryable, "failed"), (expired, "deferred"),
            ):
                assert sql("SELECT outcome FROM webhook_processing_attempts "
                           f"WHERE call_id='{receipt}' ORDER BY generation") == expected
        except BaseException:
            log.flush()
            print(log_path.read_text())
            raise
        finally:
            http_helpers["stop"](server)
    print(f"{driver}: 待关联延期和唤醒 6 个真实 HTTP、12 次 Worker、5 次 Artisan 唤醒；预算、期限、竞争与回滚通过")


def check_body_deduplication(binary, env, root, port, driver, sql):
    from concurrent.futures import ThreadPoolExecutor

    path = "/api/package/digest-window"
    body = b'{"value":1}'
    receipts_before = int(sql("SELECT COUNT(*) FROM webhook_calls"))

    def send(target, value=body, method="POST", media="application/json", event="label",
             signature_valid=True, expected=200):
        signature = hmac.new(SECRET.encode(), value, hashlib.sha256).hexdigest()
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            conn.request(method, target, value, {
                "Signature": signature if signature_valid else "0" * 64,
                "Content-Type": media, "Accept": "application/json", "Webhook-Id": event,
            })
            response = conn.getresponse()
            data = response.read()
            assert response.status == expected, (method, target, response.status, data[:180])
            if expected == 200:
                assert json.loads(data) == {"message": "ok"}
        finally:
            conn.close()

    def counts(receipts, keys, jobs):
        assert int(sql("SELECT COUNT(*) FROM webhook_calls")) == receipts_before + receipts
        assert int(sql("SELECT COUNT(*) FROM webhook_event_receipts")) == keys
        assert int(sql("SELECT COUNT(*) FROM jobs")) == jobs

    log_path = root / "digest-window.log"
    with log_path.open("w+") as log:
        server = http_helpers["serve"](binary, env, log)
        try:
            send("/api/package/digest-invalid", expected=500)
            send(path + "?q=one", signature_valid=False, expected=500)
            counts(0, 0, 0)
            with ThreadPoolExecutor(max_workers=2) as clients:
                first = clients.submit(send, path + "?q=one")
                second = clients.submit(send, path + "?q=one", event="different-unsigned-header")
                first.result()
                second.result()
            original_call = sql("SELECT call_id FROM webhook_event_receipts")
            assert sql("SELECT COUNT(expires_at) FROM webhook_event_receipts") == "1"
            counts(1, 1, 1)
            send(path + "?q=two")
            send(path + "?q=one", media="application/octet-stream")
            send(path, value=b"", method="GET", media="application/octet-stream")
            send(path, value=b"", method="POST", media="application/octet-stream")
            counts(5, 5, 5)
            time.sleep(5.2)
            with ThreadPoolExecutor(max_workers=2) as clients:
                results = [clients.submit(send, path + "?q=one") for _ in range(2)]
                for result in results:
                    result.result()
            counts(6, 5, 6)
            assert original_call not in sql("SELECT call_id FROM webhook_event_receipts").split()
            assert original_call in sql("SELECT id FROM webhook_calls").split()

            for index in range(6):
                marker = root / f"digest-worker-{index}.json"
                command(binary, env | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(marker)},
                        "queue:work", "database", "--queue=webhook-receipts", "--once")
                assert json.loads(marker.read_text())["name"] == "digest-window"
            counts(6, 5, 0)
        except BaseException:
            log.flush()
            print(log_path.read_text())
            raise
        finally:
            http_helpers["stop"](server)
    print(f"{driver}: 摘要去重 10 个真实 HTTP、6 次 Worker；窗口、并发换代、方法／查询／媒体类型与拒绝边界通过")


def check_mass_pruning(binary, env, root, driver, sql):
    definition = ("CREATE TABLE fixture_mass_entries "
                  "(id INTEGER PRIMARY KEY, eligible INTEGER NOT NULL, kind INTEGER NOT NULL, "
                  "visible INTEGER NOT NULL, deleted_at VARCHAR(40))")
    if driver == "pgsql":
        write_database(driver, sql, definition + " PARTITION BY RANGE (id)")
        write_database(driver, sql, "CREATE TABLE fixture_mass_first PARTITION OF fixture_mass_entries "
                       "FOR VALUES FROM (MINVALUE) TO (6)")
        write_database(driver, sql, "CREATE TABLE fixture_mass_second PARTITION OF fixture_mass_entries "
                       "FOR VALUES FROM (6) TO (MAXVALUE)")
    else:
        write_database(driver, sql, definition)
    write_database(driver, sql, "INSERT INTO fixture_mass_entries (id,eligible,kind,visible,deleted_at) VALUES "
                   "(1,1,0,1,NULL), (2,1,0,1,NULL), (3,1,0,1,NULL), (4,1,0,0,NULL), "
                   "(5,0,0,1,NULL), (6,1,1,1,'2025-01-01 00:00:00'), (7,1,1,0,NULL), (8,0,1,1,NULL)")
    if driver == "pgsql":
        assert sql("SELECT COUNT(DISTINCT ctid), COUNT(DISTINCT tableoid) "
                   "FROM fixture_mass_entries WHERE id IN (1,6)") == "1\t2"
    event_log = root / "mass-pruning-events.jsonl"
    current = env | {"FIXTURE_MODEL_PRUNING_EVENTS": str(event_log), "FIXTURE_MASS_PRUNE": "1"}
    hard = "fixture::MassEntry"
    soft = "fixture::SoftMassEntry"
    model = ("model:prune", "--model=" + hard, "--model=" + soft, "--chunk=1")
    preview = command(binary, current, *model, "--pretend")
    assert f"[{hard}] 可清理 3 条" in preview and f"[{soft}] 可清理 1 条" in preview, preview
    assert not event_log.exists()
    assert sql("SELECT COUNT(*) FROM fixture_mass_entries") == "8"

    failed = subprocess.run([str(binary), "artisan", *model],
                            env=current | {"FIXTURE_MASS_PRUNE_ZERO_CALLBACK": "1"},
                            capture_output=True, text=True, timeout=15)
    assert failed.returncode != 0 and sql("SELECT COUNT(*) FROM fixture_mass_entries") == "8"
    assert [json.loads(line) for line in event_log.read_text().splitlines()] == [["starting", [hard, soft]]]
    event_log.unlink()

    output = command(binary, current, *model)
    assert f"[{hard}] 已清理 3 条" in output and f"[{soft}] 已清理 2 条" in output, output
    assert [json.loads(line) for line in event_log.read_text().splitlines()] == [
        ["starting", [hard, soft]], ["pruned", hard, 2], ["pruned", hard, 3],
        ["pruned", soft, 1], ["pruned", soft, 2], ["finished", [hard, soft]],
    ]
    assert sql("SELECT id FROM fixture_mass_entries ORDER BY id").split() == ["4", "5", "8"]
    event_log.unlink()
    assert "无记录可清理" in command(binary, current, *model)
    assert [json.loads(line) for line in event_log.read_text().splitlines()] == [
        ["starting", [hard, soft]], ["finished", [hard, soft]],
    ]
    write_database(driver, sql, "INSERT INTO fixture_mass_entries (id,eligible,kind,visible) VALUES (9,1,1,1)")
    event_log.unlink()
    assert "批量清理 1 条" in command(binary, current | {"FIXTURE_MASS_PRUNE_ZERO_LIMIT": "1"}, "webhooks:mass-prune")
    [[kind, source, count]] = [json.loads(line) for line in event_log.read_text().splitlines()]
    assert kind == "pruned" and "::mass_pruning::MassEntry" in source and count == 1
    assert sql("SELECT id FROM fixture_mass_entries ORDER BY id").split() == ["4", "5", "8"]
    print(f"{driver}: 批量清理通过预演、显式上限、默认分批、作用域、无逐实例事件及直接调用", flush=True)


def check_instance_pruning(binary, env, root, driver, sql):
    write_database(driver, sql, "CREATE TABLE fixture_cleanup_entries "
                   "(id INTEGER PRIMARY KEY, eligible INTEGER NOT NULL, deleted_at VARCHAR(40))")
    write_database(driver, sql, "INSERT INTO fixture_cleanup_entries (id,eligible,deleted_at) VALUES "
                   "(1,1,NULL), (2,1,'2025-01-01 00:00:00'), (3,1,NULL), (4,0,NULL), (5,0,NULL), (6,1,NULL)")
    model = "fixture::CleanupEntry"
    event_log = root / "instance-pruning-events.jsonl"
    hook_log = root / "instance-pruning-hook.log"
    current = env | {"FIXTURE_MODEL_PRUNING_EVENTS": str(event_log),
                     "FIXTURE_INSTANCE_PRUNE_LOG": str(hook_log)}
    assert "可清理 4 条" in command(binary, current, "model:prune", "--pretend")
    assert not hook_log.exists() and not event_log.exists()
    assert "已处理 3 条" in command(binary, current, "model:prune", "--model=" + model, "--chunk=9")
    assert hook_log.read_text().splitlines() == ["hook 1", "reported 1", "hook 2", "deleted 2",
                                                "hook 3", "deleted 3", "hook 6", "veto 6"]
    assert [json.loads(line) for line in event_log.read_text().splitlines()] == [
        ["starting", [model]], ["pruned", model, 0], ["pruned", model, 1], ["pruned", model, 2],
        ["pruned", model, 3], ["finished", [model]],
    ]
    assert sql("SELECT id FROM fixture_cleanup_entries ORDER BY id").split() == ["1", "4", "5", "6"]
    command(binary, current, "webhooks:prune-instance")
    assert hook_log.read_text().splitlines()[-2:] == ["hook 4", "deleted 4"]
    command(binary, current, "webhooks:prune-instance", "--plain")
    assert hook_log.read_text().splitlines()[-1] == "hook 5"
    assert sql("SELECT id FROM fixture_cleanup_entries ORDER BY id").split() == ["1", "6"]
    event_log.unlink()
    assert "清理计数 1" in command(binary, current, "webhooks:prune-instances")
    events = [json.loads(line) for line in event_log.read_text().splitlines()]
    source = events[0][1]
    assert "::model_pruning::CleanupEntry" in source
    assert events == [["pruned", source, 0], ["pruned", source, 1]]
    print(f"{driver}: 逐实例模型清理通过预演、失败报告、按主键继续、逐批事件与永久删除", flush=True)


def check_retention(binary, env, root, port, driver, sql):
    current = env | {"DB_QUEUE_RETRY_AFTER": "90"}
    scope = "webhook-acceptance"

    def intake(body):
        before = set(sql("SELECT id FROM webhook_calls").split())
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            signature = hmac.new(SECRET.encode(), body, hashlib.sha256).hexdigest()
            conn.request("POST", "/api/package/retention-digest", body, {
                "Signature": signature, "Content-Type": "application/octet-stream",
            })
            response = conn.getresponse()
            assert response.status == 200, response.read()
        finally:
            conn.close()
        rows = set(sql("SELECT id FROM webhook_calls").split()) - before
        assert len(rows) == 1
        return rows.pop()

    def worker(receipt, crash=False):
        run_env = current | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(root / f"prune-{receipt}.json")}
        if crash:
            run_env["FIXTURE_LEASE_CRASH"] = "1"
        result = subprocess.run(
            [str(binary), "artisan", "queue:work", "database", "--queue=webhook-receipts", "--once"],
            env=run_env, capture_output=True, text=True, timeout=20,
        )
        assert result.returncode == (79 if crash else 0), result.stdout + result.stderr

    def edit(receipt, assignments):
        assert re.fullmatch(r"[0-9a-f-]{36}", receipt)
        write_database(driver, sql, f"UPDATE webhook_calls SET {assignments} WHERE id='{receipt}'")

    def bad_prune(*args, **extra):
        result = subprocess.run(
            [str(binary), "artisan", "webhook-client:prune", *args],
            env=current | extra, capture_output=True, text=True, timeout=15,
        )
        assert result.returncode != 0, result.stdout + result.stderr

    log_path = root / "retention-http.log"
    with log_path.open("w+") as log:
        server = http_helpers["serve"](binary, current, log)
        try:
            oldest = intake(b"retention-first")
            worker(oldest)
            edit(oldest, "created_at=1, processed_at=1")
            older = intake(b"retention-second")
            worker(older)
            edit(older, "created_at=2, processed_at=2")
            recently_completed = intake(b"retention-recent")
            worker(recently_completed)
            edit(recently_completed, "created_at=1")
            active = intake(b"leased-crash")
            worker(active, crash=True)
            edit(active, "created_at=1")
            pending = intake(b"retention-pending")
            edit(pending, "created_at=1")
            stopped = sql("SELECT id FROM webhook_calls WHERE name='deferred' AND processing_stopped_at IS NOT NULL ORDER BY id LIMIT 1")
            edit(stopped, "created_at=3, processing_stopped_at=3")
            legacy = sql("SELECT id FROM webhook_calls WHERE name='spatie' ORDER BY id LIMIT 1")
            edit(legacy, "created_at=1")
            peer = sql("SELECT id FROM webhook_calls WHERE scope='webhook-custom' ORDER BY id LIMIT 1")
            edit(peer, "created_at=1, processed_at=1")

            preserved = {recently_completed, active, pending, legacy, peer}
            eligible = {oldest, older, stopped}
            ids = lambda: set(sql("SELECT id FROM webhook_calls").split())
            assert (preserved | eligible) <= ids()
            bad_prune()
            bad_prune("--scope=" + scope, "--limit=501")
            bad_prune("--scope=" + scope, FIXTURE_PRUNE_POLICY='"30"')
            assert "已停用" in command(binary, current | {"FIXTURE_PRUNE_POLICY": "null"},
                                       "webhook-client:prune", "--scope=" + scope)
            assert "可清理 1 条" in command(binary, current, "webhook-client:prune",
                                          "--scope=" + scope, "--limit=1", "--pretend")
            assert eligible <= ids()
            assert "已清理 1 条" in command(binary, current, "webhook-client:prune",
                                          "--scope=" + scope, "--limit=1")
            assert oldest not in ids() and (preserved | {older, stopped}) <= ids()
            model = "openrust_webhook_client::models::WebhookCall"
            event_log = root / "pruning-events.jsonl"
            model_env = current | {"FIXTURE_MODEL_PRUNING_EVENTS": str(event_log)}
            model_env.pop("FIXTURE_SPATIE_SECRET")
            model_env.pop("FIXTURE_STANDARD_SECRET")
            assert "没有找到可清理模型" in command(binary, model_env, "model:prune", "--pretend")
            model_path = "vendor/openrust/webhook-client/src"
            assert "没有找到可清理模型" in command(binary, model_env, "model:prune",
                                                       "--path=" + model_path, "--except=" + model)
            invalid = subprocess.run([str(binary), "artisan", "model:prune",
                                      "--model=" + model, "--except=" + model],
                                     env=model_env, capture_output=True, text=True, timeout=15)
            assert invalid.returncode != 0
            assert "可清理 2 条" in command(binary, model_env, "model:prune", "--path=" + model_path,
                                          "--pretend", "--chunk=1")
            assert "可清理 0 条" in command(binary, model_env | {"FIXTURE_OVERRIDE_DATABASE_STORE": "1"},
                                          "model:prune", "--model=" + model, "--pretend")
            assert (preserved | {older, stopped}) <= ids() and not event_log.exists()
            command(binary, model_env, "config:cache")
            assert "已清理 2 条" in command(binary, model_env, "model:prune", "--model=" + model, "--chunk=1")
            command(binary, model_env, "config:clear")
            assert [json.loads(line) for line in event_log.read_text().splitlines()] == [
                ["starting", [model]], ["pruned", model, 1], ["pruned", model, 2], ["finished", [model]],
            ]
            assert not (eligible & ids()) and preserved <= ids()
            assert not (eligible & set(sql("SELECT call_id FROM webhook_event_receipts").split()))
            assert not (eligible & set(sql("SELECT call_id FROM webhook_processing_attempts").split()))
            assert sql("SELECT COUNT(*) FROM jobs") == "2"
        except BaseException:
            log.flush()
            print(log_path.read_text())
            raise
        finally:
            http_helpers["stop"](server)
    print(f"{driver}: 保留期清理 5 个真实 HTTP、4 次 Worker；包命令与模型命令的范围、终态、预演和分批事件通过")


def check_unverified_hints(binary, env, root, port, driver, sql):
    target = "/api/package/hint"
    before = sql("SELECT COUNT(*) FROM webhook_calls")
    log_path = root / "hints.log"

    def send(path, expected, body=b"known", headers=None):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            connection.request("POST", path, body, {
                "Accept": "application/json", "Content-Type": "application/octet-stream",
                **(headers or {}),
            })
            response = connection.getresponse()
            data = response.read()
            assert response.status == expected, (path, expected, response.status, data)
            if expected == 202:
                assert json.loads(data) == {"message": "accepted"}
            assert b"local-task" not in data
        finally:
            connection.close()

    def worker(marker, **extra):
        command(binary, env | {"FIXTURE_HINT_MARKER": str(root / marker), **extra},
                "queue:work", "database", "--queue=webhook-hints", "--once")

    with (root / "hint-missing-queue.log").open("w+") as log:
        broken = http_helpers["serve"](binary, env | {"DB_QUEUE_TABLE": "missing_hint_jobs"}, log)
        try:
            send("/api/package/hint-standard", 500)
        finally:
            http_helpers["stop"](broken)
    assert sql("SELECT COUNT(*) FROM webhook_hints") == "0"

    with log_path.open("w+") as log:
        server = http_helpers["serve"](binary, env, log)
        try:
            send("/api/package/post", 500)
            send(target, 500, headers={"Signature": ""})
            send(target, 500, headers={"Signature": "0" * 64})
            send("/api/package/hint-standard", 500, headers={"Webhook-Id": "partial"})
            send("/api/package/hint-wrong-scope", 500)
            send(target, 202, b"foreign-task")
            assert sql("SELECT COUNT(*) FROM webhook_hints") == "0"
            assert sql("SELECT COUNT(*) FROM jobs") == "0"

            send(target, 202)
            stored_id = sql("SELECT id FROM webhook_hints")
            assert re.fullmatch(r"[0-9a-f-]{36}", stored_id)
            wire = json.loads(sql("SELECT payload FROM jobs"))
            assert wire["name"] == "openrust_webhook_client::hints::hint"
            assert wire["data"] == {"id": stored_id, "revision": "hint-one"}
            assert wire["options"]["max_tries"] == 1
            write_database(driver, sql,
                           "INSERT INTO jobs (queue,payload,attempts,reserved_at,available_at,created_at) "
                           "SELECT queue,payload,0,NULL,available_at,created_at FROM jobs")
            worker("hint-handled")
            worker("hint-duplicate")
            assert (root / "hint-handled").read_text() == "local-task"
            assert not (root / "hint-duplicate").exists()

            send(target, 202)
            write_database(driver, sql, "UPDATE webhook_hints SET expires_at=0 WHERE status='pending'")
            worker("hint-expired")
            assert not (root / "hint-expired").exists()
            send(target, 202)
            worker("hint-revoked", FIXTURE_HINT_REVOKED="1")
            assert not (root / "hint-revoked").exists()
            assert sql("SELECT status FROM webhook_hints ORDER BY status").splitlines() == [
                "completed", "expired", "ignored",
            ]
            assert sql("SELECT COUNT(*) FROM webhook_calls") == before
            assert sql("SELECT COUNT(*) FROM jobs") == "0"
            http_helpers["stop"](server)
            server = http_helpers["serve"](binary, env, log)
            send(target, 429)
            signature = hmac.new(SECRET.encode(), b"known", hashlib.sha256).hexdigest()
            send(target, 200, headers={"Signature": signature})
            assert int(sql("SELECT COUNT(*) FROM webhook_calls")) == int(before) + 1
            assert sql("SELECT COUNT(*) FROM webhook_hints") == "3"
            marker = root / "hint-signed-receipt.json"
            command(binary, env | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(marker)},
                    "queue:work", "database", "--queue=webhook-receipts", "--once")
            assert json.loads(marker.read_text())["name"] == "hint"
        except BaseException:
            log.flush()
            print(log_path.read_text())
            raise
        finally:
            http_helpers["stop"](server)
    print(f"{driver}: 无签名提示 12 个真实 HTTP、5 次 Worker；限额、签名和范围隔离、入队回滚、重复、过期及撤销通过")


def check_hint_retention(binary, env, driver, sql):
    scope = "webhook-acceptance"
    command_args = ("webhook-client:prune", "--lookup-hints")
    before = sql("SELECT COUNT(*) FROM webhook_calls")
    now = int(time.time())
    write_database(driver, sql, "UPDATE webhook_hints SET expires_at=1 WHERE status='completed'")

    def insert_hint(suffix, status, expires, owner=scope):
        key = hashlib.sha256(owner.encode()).hexdigest()
        write_database(driver, sql,
                       "INSERT INTO webhook_hints (id,scope_key,scope,profile,revision,task_id,status,expires_at) "
                       f"VALUES ('11111111-1111-4111-8111-{suffix:012d}', '{key}', '{owner}', "
                       f"'hint', 'hint-one', 'local-task', '{status}', {expires})")

    insert_hint(1, "running", now - 10)
    insert_hint(2, "running", now - 180)
    insert_hint(3, "pending", now + 300)
    insert_hint(4, "failed", 1, "unrelated-scope")
    result = command(binary, env, *command_args, f"--scope={scope}", "--pretend", "--limit=2")
    assert "可清理 2 条" in result and sql("SELECT COUNT(*) FROM webhook_hints") == "7"
    result = command(binary, env, *command_args, f"--scope={scope}", "--limit=1")
    assert "已清理 1 条" in result and sql("SELECT COUNT(*) FROM webhook_hints") == "6"
    result = command(binary, env, *command_args, f"--scope={scope}")
    assert "已清理 2 条" in result
    assert sql("SELECT status FROM webhook_hints ORDER BY status").splitlines() == [
        "failed", "ignored", "pending", "running",
    ]
    assert sql("SELECT COUNT(*) FROM jobs") == "0"
    assert sql("SELECT COUNT(*) FROM webhook_calls") == before
    assert "已清理 0 条" in command(binary, env, *command_args, f"--scope={scope}")
    denied = subprocess.run([str(binary), "artisan", *command_args, f"--scope={scope}",
                             "--verification-audit"], env=env, capture_output=True, timeout=15)
    assert denied.returncode != 0
    print(f"{driver}: Artisan 查单提示预演、过期清理、批次上限、运行宽限和范围隔离通过")


def check_verification_audit(binary, env, root, port, driver, sql):
    body = b"private-body-for-audit"
    listener = root / "audit-listener-ran"
    current = env | {"FIXTURE_EMPTY_SECRET": "", "FIXTURE_AUDIT_LISTENER_FAIL": str(listener)}
    signature = hmac.new(SECRET.encode(), body, hashlib.sha256).hexdigest()
    starting_count = int(sql("SELECT COUNT(*) FROM webhook_calls"))
    command(binary, current, "config:cache")
    with (root / "verification-audit.log").open("w+") as log:
        server = http_helpers["serve"](binary, current, log)
        try:
            def send(profile, expected, signed=None):
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
                try:
                    headers = {"Accept": "application/json", "Content-Type": "application/octet-stream",
                               "X-Private": "private-header-for-audit"}
                    if signed is not None:
                        headers["Signature"] = signed
                    connection.request("POST", f"/api/package/{profile}?query=private-query", body, headers)
                    response = connection.getresponse()
                    data = response.read()
                    assert response.status == expected, (profile, expected, response.status, data)
                finally:
                    connection.close()

            send("audit-required", 500)
            for _ in range(3):
                send("audit-required", 500, "0" * 64)
            assert listener.read_bytes() == b"ran"
            send("audit-error", 500, "0" * 64)
            send("audit-required", 200, signature)
            assert int(sql("SELECT COUNT(*) FROM webhook_calls")) == starting_count + 1
            assert sql("SELECT COUNT(*) FROM jobs") == "1"
            marker = root / "audited-worker.json"
            command(binary, current | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(marker)}, "queue:work",
                    "database", "--queue=webhook-receipts", "--once")
            assert bytes(json.loads(marker.read_text())["body"]) == body
            assert sql("SELECT COUNT(*) FROM jobs") == "0"

            entries = sql(
                "SELECT profile,revision,validator,policy,classification,evidence "
                "FROM webhook_verification_audits"
            ).splitlines()
            expected = {
                "audit-required\taudit-v1\tdefault\tsignature_required\trejected\tmissing_signature",
                "audit-required\taudit-v1\tdefault\tsignature_required\trejected\tinvalid_signature",
                "audit-required\taudit-v1\tdefault\tsignature_required\tverified\tsignature_accepted",
                "audit-error\taudit-error-v1\tdefault\tsignature_required\trejected\tvalidator_error",
                "hint\thint-one\tdefault\tfixture-hint\tunverified_hint\tmissing_signature",
                "hint-standard\t1\tstandard\tsignature_required\trejected\tinvalid_signature",
            }
            assert expected <= set(entries), entries
            assert not any("private-" in entry or SECRET in entry for entry in entries)
            assert sql("SELECT COUNT(*) FROM webhook_verification_audits WHERE profile='post'") == "0"
            assert 1 <= int(sql(
                "SELECT COUNT(*) FROM webhook_verification_audits "
                "WHERE profile='audit-required' AND evidence='invalid_signature'"
            )) <= 2
            if driver == "sqlite":
                columns = {line.split("\t")[1] for line in sql("PRAGMA table_info(webhook_verification_audits)").splitlines()}
            else:
                columns = set(sql(
                    "SELECT column_name FROM information_schema.columns "
                    "WHERE table_name='webhook_verification_audits'"
                ).splitlines())
            assert columns == {"audit_key", "scope_key", "scope", "profile", "revision", "validator",
                               "policy", "classification", "evidence", "observed_minute"}

            write_database(driver, sql,
                           "UPDATE webhook_verification_audits SET observed_minute=0 WHERE profile='audit-error'")
            prune = ("webhook-client:prune", "--verification-audit", "--limit=1")
            assert "已清理 0 条" in command(binary, current, *prune, "--scope=another-account")
            assert "可清理 1 条" in command(binary, current, *prune, "--scope=webhook-acceptance", "--pretend")
            assert "已清理 1 条" in command(binary, current, *prune, "--scope=webhook-acceptance")
            assert sql("SELECT COUNT(*) FROM webhook_verification_audits WHERE profile='audit-error'") == "0"
            assert int(sql("SELECT COUNT(*) FROM webhook_verification_audits")) == len(entries) - 1
            assert int(sql("SELECT COUNT(*) FROM webhook_calls")) == starting_count + 1
        finally:
            http_helpers["stop"](server)
            command(binary, current, "config:clear")
    print(f"{driver}: 验签证据 6 个真实 HTTP、1 次 Worker、3 次清理命令；三类判定、合并、脱敏与配置缓存通过")


def check_attachment_storage(binary, env, root, port, driver, sql):
    from concurrent.futures import ThreadPoolExecutor

    scope = "webhook-acceptance"
    disk_root = root / "attachment-disk"
    current = env | {"FILESYSTEM_LOCAL_ROOT": disk_root.name}
    atomic = "attachment-disk"
    body = (
        b'--disk\r\nContent-Disposition: form-data; name="message"\r\n\r\nstored\r\n'
        b'--disk\r\nContent-Disposition: form-data; name="files[]"; filename="../sample.bin"\r\n'
        b'Content-Type: application/octet-stream\r\n\r\n\x00\xffsample\r\n--disk--\r\n'
    )
    initial = sql("SELECT COUNT(*) FROM webhook_calls")

    def send(profile, expected=200, value=body):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            signature = hmac.new(SECRET.encode(), value, hashlib.sha256).hexdigest()
            connection.request("POST", f"/api/package/{profile}", value, {
                "Signature": signature, "Accept": "application/json",
                "Content-Type": "multipart/form-data; boundary=disk",
            })
            response = connection.getresponse()
            assert response.status == expected, (expected, response.status, response.read())
        finally:
            connection.close()

    def reject_record(receipt, selected_scope):
        result = subprocess.run([str(binary), "artisan", "webhooks:record", receipt, selected_scope],
                                env=current, capture_output=True, timeout=15)
        assert result.returncode != 0 and b"sample.bin" not in result.stdout

    def id_for(profile):
        receipt = sql(f"SELECT id FROM webhook_calls WHERE name='{profile}'")
        assert re.fullmatch(r"[0-9a-f-]{36}", receipt)
        return receipt

    def work(profile, attachment=True):
        marker = root / f"{profile}.json"
        command(binary, current | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(marker)},
                "queue:work", "database", "--queue=webhook-receipts", "--once")
        record = json.loads(marker.read_text())
        assert record["name"] == profile and bytes(record["body"]) == body
        assert record["payload"] == {"message": "stored"}
        expected = {"files": {"0": {"name": "sample.bin", "mime": "application/octet-stream",
                                   "bytes": list(b"\x00\xffsample")}}}
        assert record["attachments"] == (expected if attachment else None)

    for checkpoint, configuration in (
        ("missing-queue", {"DB_QUEUE_TABLE": "missing_attachment_jobs"}),
        ("broken-disk", {"FILESYSTEM_LOCAL_ROOT": "not-a-directory"}),
    ):
        (root / "not-a-directory").write_text("sentinel")
        with (root / f"attachment-{checkpoint}.log").open("w+") as log:
            server = http_helpers["serve"](binary, current | configuration, log)
            try:
                send(atomic, 500)
            finally:
                http_helpers["stop"](server)
        assert sql("SELECT COUNT(*) FROM webhook_calls") == initial
        assert sql("SELECT COUNT(*) FROM jobs") == "0"
        assert not list(disk_root.rglob("*.multipart"))
        if checkpoint == "missing-queue":
            assert sql("SELECT COUNT(*) FROM webhook_attachment_staging") == "0"

    def expect_guarded_rollback():
        result = subprocess.run([str(binary), "artisan", "migrate:rollback", "--path=vendor/openrust/webhook-client/database/migrations/2026_11_25_000012_add_webhook_attachment_cleanup_lease.rs", "--force"],
                                env=current, capture_output=True, text=True, timeout=20)
        assert result.returncode != 0, result.stdout + result.stderr

    staged_before_crash = int(sql("SELECT COUNT(*) FROM webhook_attachment_staging"))
    with (root / "attachment-crash.log").open("w+") as log:
        server = http_helpers["serve"](binary, current | {"FIXTURE_ATOMIC_CRASH": "after-enqueue"}, log)
        try:
            try:
                send(atomic)
            except http.client.RemoteDisconnected:
                pass
            else:
                raise AssertionError("预期派发事务提交前退出")
            assert server.wait(timeout=10) == 78
        finally:
            if server.poll() is None:
                http_helpers["stop"](server)
    assert sql("SELECT COUNT(*) FROM webhook_calls") == initial
    assert sql("SELECT COUNT(*) FROM jobs") == "0"
    assert int(sql("SELECT COUNT(*) FROM webhook_attachment_staging")) == staged_before_crash + 1
    expect_guarded_rollback()
    orphan, = disk_root.rglob("*.multipart")
    sweep = ("webhook-client:prune", "--attachment-staging", "--scope=" + scope)
    assert "可清理 0 个" in command(binary, current, *sweep, "--pretend")
    assert orphan.exists()

    with (root / "attachment-live.log").open("w+") as log:
        server = http_helpers["serve"](binary, current, log)
        try:
            with ThreadPoolExecutor(max_workers=2) as callers:
                requests = [callers.submit(send, atomic) for _ in range(2)]
                for request in requests:
                    request.result()
            receipt = id_for(atomic)
            assert sql("SELECT COUNT(*) FROM jobs") == "1"
            assert int(sql("SELECT COUNT(*) FROM webhook_attachment_staging")) == staged_before_crash + 1
            scope_hash = hashlib.sha256(scope.encode()).hexdigest()
            obj = disk_root / "webhook-client" / scope_hash / f"{receipt}.multipart"
            assert set(disk_root.rglob("*.multipart")) == {orphan, obj}
            assert obj.read_bytes() == body and obj.stat().st_mode & 0o077 == 0
            assert sql(f"SELECT length(raw_body) FROM webhook_calls WHERE id='{receipt}'") == "0"
            assert json.loads(sql(f"SELECT attachment_reference FROM webhook_calls WHERE id='{receipt}'")) == {
                "disk": "local", "digest": hashlib.sha256(body).hexdigest(), "length": len(body),
            }
            reject_record(receipt, "webhook-custom")
            obj.write_bytes(b"!" + body[1:])
            reject_record(receipt, scope)
            obj.write_bytes(body)
            moved = obj.with_suffix(".held")
            obj.rename(moved)
            try:
                reject_record(receipt, scope)
            finally:
                moved.rename(obj)
            work(atomic)
            write_database(driver, sql,
                           f"UPDATE webhook_calls SET created_at=1,processed_at=1 WHERE id='{receipt}'")
            prune = ("webhook-client:prune", "--scope=" + scope)
            assert "可清理 1 条" in command(binary, current, *prune, "--pretend")
            assert obj.exists()

            send("attachment-plain")
            ordinary = id_for("attachment-plain")
            assert (obj.parent / f"{ordinary}.multipart").read_bytes() == body
            # 即使创建日期已过保留期，尚未完成的接收记录仍保留。
            write_database(driver, sql,
                           f"UPDATE webhook_calls SET created_at=1 WHERE id='{ordinary}'")
            send("attachment-disabled")
            disabled = id_for("attachment-disabled")
            assert sql(f"SELECT COUNT(attachment_reference) FROM webhook_calls WHERE id='{disabled}'") == "0"
            assert not (obj.parent / f"{disabled}.multipart").exists()
            assert "可清理 1 条" in command(binary, current, *prune, "--pretend")
            work("attachment-plain")
            work("attachment-disabled", attachment=False)
            assert sql("SELECT COUNT(*) FROM jobs") == "0"
            write_database(driver, sql, "UPDATE webhook_attachment_staging SET created_at=1")
            assert "可清理 0 个" in command(
                binary, current, "webhook-client:prune", "--attachment-staging",
                "--scope=webhook-custom", "--pretend",
            )
            assert f"可清理 {staged_before_crash + 1} 个" in command(
                binary, current, *sweep, "--pretend",
            )

            # 用真实文件删除失败验证租约、失败退出状态及补偿重试。
            orphan_id = orphan.stem
            held = orphan.with_suffix(".held")
            orphan.rename(held)
            orphan.mkdir()
            result = subprocess.run([str(binary), "artisan", *sweep], env=current,
                                    capture_output=True, text=True, timeout=20)
            assert result.returncode != 0 and "1 个待重试" in result.stdout, result
            assert sql(f"SELECT COUNT(cleanup_token) FROM webhook_attachment_staging WHERE id='{orphan_id}'") == "1"
            assert "可清理 0 个" in command(binary, current, *sweep, "--pretend")
            orphan.rmdir()
            held.rename(orphan)
            write_database(driver, sql, f"UPDATE webhook_attachment_staging SET cleanup_until=0 WHERE id='{orphan_id}'")
            with ThreadPoolExecutor(max_workers=2) as callers:
                cleaners = [callers.submit(command, binary, current, *sweep) for _ in range(2)]
                outcomes = sorted(int(re.search(r"已清理 (\d+) 个", cleaner.result())[1]) for cleaner in cleaners)
            assert outcomes == [0, 1] and not orphan.exists()
            assert sql("SELECT COUNT(*) FROM webhook_attachment_staging") == "0"

            expect_guarded_rollback()
            preview = command(binary, current, "migrate:rollback", "--path=vendor/openrust/webhook-client/database/migrations/2026_11_25_000012_add_webhook_attachment_cleanup_lease.rs", "--pretend", "--force")
            assert "webhook_attachment_staging" in preview
            record = json.loads(command(binary, current, "webhooks:record", receipt, scope))
            assert bytes(record["body"]) == body

            # 加入冲突登记，使删除后的登记插入失败，接收记录应随事务回滚。
            write_database(driver, sql,
                           "INSERT INTO webhook_attachment_staging (id,scope_key,disk,created_at) "
                           f"VALUES ('{receipt}','{scope_hash}','local',1)")
            model_prune = ("model:prune", "--model=openrust_webhook_client::models::WebhookCall", "--chunk=1")
            result = subprocess.run([str(binary), "artisan", *model_prune], env=current,
                                    capture_output=True, text=True, timeout=20)
            assert result.returncode != 0 and obj.exists(), result
            assert sql(f"SELECT COUNT(*) FROM webhook_calls WHERE id='{receipt}'") == "1"
            write_database(driver, sql, f"DELETE FROM webhook_attachment_staging WHERE id='{receipt}'")

            assert "已清理 1 条" in command(binary, current, *model_prune)
            assert sql(f"SELECT COUNT(*) FROM webhook_calls WHERE id='{receipt}'") == "0"
            assert sql(f"SELECT COUNT(*) FROM webhook_attachment_staging WHERE id='{receipt}'") == "1"
            assert obj.exists()
            no_retention = current | {"FIXTURE_PRUNE_POLICY": "null"}
            assert "保留期清理已停用" in command(binary, no_retention, *prune)
            assert "已清理 1 个 Webhook 附件对象，0 个待重试" in command(binary, no_retention, *sweep)
            assert not obj.exists() and (obj.parent / f"{ordinary}.multipart").exists()

            write_database(driver, sql, f"UPDATE webhook_calls SET processed_at=1 WHERE id='{ordinary}'")
            assert "已清理 1 条" in command(binary, current, *prune)
            assert "已清理 1 个" in command(binary, current, *sweep)
            assert sql("SELECT COUNT(*) FROM webhook_attachment_staging") == "0"
            assert not list(disk_root.rglob("*.multipart"))
        except BaseException:
            log.flush()
            print((root / "attachment-live.log").read_text())
            raise
        finally:
            http_helpers["stop"](server)
    print(f"{driver}: 附件 Storage 7 个真实 HTTP、3 次 Worker；私有对象、并发去重、篡改拒绝、事务解绑、崩溃补偿、并发清理与回滚保护通过")


def check_dispatch_recovery(binary, env, root, port, driver, sql):
    from concurrent.futures import ThreadPoolExecutor
    from uuid import uuid4

    scope = "webhook-recovery-scope"
    redis = os.environ.get("OPENRUST_TEST_WEBHOOK_REDIS_URL")
    redis_container = os.environ.get("OPENRUST_TEST_WEBHOOK_REDIS_CONTAINER")
    assert not redis or redis_container, "Redis 验收需要隔离容器名称"
    connection = "redis" if redis else "foreign-database"
    current = env | {"FIXTURE_RECOVERY_QUEUE": connection}
    if redis:
        current |= {"REDIS_URL": redis, "REDIS_PREFIX": root.name + ":",
                    "REDIS_QUEUE_RETRY_AFTER": "8"}
        current.pop("REDIS_USERNAME", None)
        current.pop("REDIS_PASSWORD", None)
    sweep = ("webhook-client:recover", "--scope=" + scope, "--limit=1")
    body = b"durable exact bytes\x00\xff"
    initial = sql(f"SELECT COUNT(*) FROM webhook_calls WHERE scope='{scope}'")
    unrelated_queues = sql("SELECT queue,COUNT(*) FROM jobs WHERE queue <> 'webhook-recovery' GROUP BY queue ORDER BY queue")

    def pending_count():
        if not redis:
            return int(sql("SELECT COUNT(*) FROM jobs WHERE queue = 'webhook-recovery'"))
        count = subprocess.check_output(
            ["docker", "exec", redis_container, "redis-cli", "LLEN",
             current["REDIS_PREFIX"] + "queues:webhook-recovery"], text=True, timeout=15,
        )
        return int(count)

    def send(profile, status):
        client = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            client.request("POST", f"/api/package/{profile}", body, {
                "Content-Type": "application/octet-stream",
                "Signature": hmac.new(SECRET.encode(), body, hashlib.sha256).hexdigest(),
            })
            response = client.getresponse()
            assert response.status == status, (response.status, response.read())
        finally:
            client.close()

    with (root / "dispatch-recovery-crash.log").open("w+") as log:
        server = http_helpers["serve"](binary, current | {"FIXTURE_CRASH_AFTER_DURABLE_COMMIT": "1"}, log)
        try:
            try:
                send("recovery", 200)
            except http.client.RemoteDisconnected:
                pass
            else:
                raise AssertionError("预期在提交接收记录和待派发意图后退出")
            assert server.wait(timeout=10) == 80
        finally:
            if server.poll() is None:
                http_helpers["stop"](server)
    assert sql("SELECT COUNT(*) FROM webhook_dispatch_intents") == "1"
    assert pending_count() == 0

    with (root / "dispatch-recovery-http.log").open("w+") as log:
        server = http_helpers["serve"](binary, current, log)
        try:
            send("recovery-null", 500)
            send("recovery-failover", 500)
            assert sql(f"SELECT COUNT(*) FROM webhook_calls WHERE scope='{scope}'") == str(int(initial) + 1)
            assert sql("SELECT COUNT(*) FROM webhook_dispatch_intents") == "1"
            send("recovery", 200)
            send("recovery", 200)
        finally:
            http_helpers["stop"](server)

    receipt = sql(f"SELECT id FROM webhook_calls WHERE scope='{scope}' AND name='recovery'")
    assert re.fullmatch(r"[0-9a-f-]{36}", receipt)
    route = json.loads(sql(f"SELECT route FROM webhook_dispatch_intents WHERE call_id='{receipt}'"))
    assert set(route) == {"connection", "queue", "job", "options"}
    assert (route["connection"], route["queue"], route["job"]) == (connection, "webhook-recovery", "accepted")
    assert route["options"]["timeout"]["secs"] == 4 and SECRET not in json.dumps(route)
    assert pending_count() == 0
    assert "可派发 0 条" in command(binary, current, "webhook-client:recover",
                                      "--scope=wrong-scope", "--pretend")
    assert "可派发 1 条" in command(binary, current, *sweep, "--pretend")
    assert sql("SELECT COUNT(claim_token) FROM webhook_dispatch_intents") == "0"

    def verify_rollback_guard(file, reason):
        migration = "--path=vendor/openrust/webhook-client/database/migrations/" + file
        rollback = subprocess.run([str(binary), "artisan", "migrate:rollback", migration, "--force"],
                                  env=current, capture_output=True, text=True, timeout=20)
        assert rollback.returncode != 0 and reason in rollback.stdout + rollback.stderr, rollback

    verify_rollback_guard("2026_11_25_000013_create_webhook_dispatch_intents_table.rs", "仍有待派发")

    def expire_claim():
        write_database(driver, sql,
                       f"UPDATE webhook_dispatch_intents SET available_at=0 WHERE call_id='{receipt}'")

    # 队列不可用时仍保留已确认的接收记录，以及可恢复的队列快照。
    broken = {"FIXTURE_FOREIGN_QUEUE_TABLE": "missing_webhook_recovery_jobs"}
    if redis:
        with socket.socket() as unused:
            unused.bind(("127.0.0.1", 0))
            broken = {"REDIS_URL": f"redis://127.0.0.1:{unused.getsockname()[1]}"}
            failure = subprocess.run([str(binary), "artisan", *sweep], env=current | broken,
                                     capture_output=True, text=True, timeout=25)
    else:
        failure = subprocess.run([str(binary), "artisan", *sweep], env=current | broken,
                                 capture_output=True, text=True, timeout=25)
    assert failure.returncode != 0 and "1 条待重试" in failure.stdout, failure
    assert sql("SELECT COUNT(claim_token) FROM webhook_dispatch_intents") == "1"
    assert "可派发 0 条" in command(binary, current, *sweep, "--pretend")
    expire_claim()

    if redis:
        # Redis 已保存任务，但同步队列事件使本进程在清除派发意图前退出。
        crash = subprocess.run([str(binary), "artisan", *sweep],
                               env=current | {"FIXTURE_CRASH_AFTER_REDIS_ENQUEUE": "1"},
                               capture_output=True, text=True, timeout=25)
        assert crash.returncode == 79, crash
        assert pending_count() == 1
        assert sql("SELECT COUNT(claim_token) FROM webhook_dispatch_intents") == "1"
        assert "可派发 0 条" in command(binary, current, *sweep, "--pretend")
        expire_claim()

    def dispatch():
        # 进程配置可变化，已接收的记录仍使用持久路由。
        result = command(binary, current | {"FIXTURE_RECOVERY_QUEUE": "null"}, *sweep)
        return int(re.search(r"已派发 (\d+) 条", result)[1])

    with ThreadPoolExecutor(max_workers=2) as runners:
        outcomes = sorted(run.result() for run in (runners.submit(dispatch), runners.submit(dispatch)))
    assert outcomes == [0, 1]
    assert pending_count() == (2 if redis else 1)
    assert sql("SELECT COUNT(*) FROM webhook_dispatch_intents") == "0"
    marker = root / "dispatch-recovered-worker.json"
    worker = ("queue:work", connection, "--queue=webhook-recovery", "--once")
    worker_env = current | {"FIXTURE_SPATIE_SECRET": "independent-worker-key"}
    command(binary, worker_env | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(marker)}, *worker)
    record = json.loads(marker.read_text())
    assert bytes(record["body"]) == body and record["revision"] == "recover-v1"
    assert int(sql(f"SELECT processed_at FROM webhook_calls WHERE id='{receipt}'")) > 0
    if redis:
        duplicate = root / "dispatch-duplicate-worker.json"
        command(binary, worker_env | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(duplicate)}, *worker)
        assert not duplicate.exists()
    assert pending_count() == 0
    print(f"{driver}: 恢复派发经 {connection} 实测 5 个 HTTP、"
          f"{2 if redis else 1} 次 Worker；队列故障、提交后重启、独占领取与迁移保护通过",
          flush=True)

    body = b"leased-crash"
    with (root / "processing-recovery-http.log").open("w+") as log:
        server = http_helpers["serve"](binary, current, log)
        try:
            send("processing-recovery", 200)
        finally:
            http_helpers["stop"](server)
    receipt = sql(f"SELECT id FROM webhook_calls WHERE scope='{scope}' AND name='processing-recovery'")
    assert re.fullmatch(r"[0-9a-f-]{36}", receipt)
    verify_rollback_guard("2026_11_25_000014_add_processing_recovery_to_webhook_dispatch_intents.rs",
                          "仍有启用处理恢复")
    assert "已派发 1 条" in command(binary, current, *sweep)
    assert "可派发 0 条" in command(binary, current, *sweep, "--pretend")
    assert int(sql(f"SELECT enqueued_at FROM webhook_dispatch_intents WHERE call_id='{receipt}'")) > 0
    assert sql(f"SELECT processing_requeues_left FROM webhook_dispatch_intents WHERE call_id='{receipt}'") == "2"

    # 清除已确认入列的任务，模拟队列丢失。预算仅在首次成功入列后的补派领取时扣减。
    clear = ("queue:clear", connection, "--queue=webhook-recovery", "--force")
    assert pending_count() == 1
    command(binary, current, *clear)
    assert pending_count() == 0
    expire_claim()
    if redis:
        # 补派尝试的队列确认丢失时，领取已经消耗预算。
        ambiguous = subprocess.run([str(binary), "artisan", *sweep],
                                   env=current | {"FIXTURE_CRASH_AFTER_REDIS_ENQUEUE": "1"},
                                   capture_output=True, text=True, timeout=25)
        assert ambiguous.returncode == 79 and pending_count() == 1, ambiguous
    else:
        assert "已派发 1 条" in command(binary, current, *sweep)
    assert sql(f"SELECT processing_requeues_left FROM webhook_dispatch_intents WHERE call_id='{receipt}'") == "1"

    # Worker 已领取业务租约后真实退出。租约仍活跃时禁止补派，过期后才允许另一 Worker 领取。
    crash = subprocess.run([str(binary), "artisan", *worker],
                           env=worker_env | {"FIXTURE_LEASE_CRASH": "1"},
                           capture_output=True, text=True, timeout=20)
    assert crash.returncode == 79, crash
    assert re.fullmatch(r"[0-9a-f-]{36}", sql(
        f"SELECT processing_token FROM webhook_calls WHERE id='{receipt}'"))
    active_until = int(time.time()) + 120
    write_database(driver, sql, f"UPDATE webhook_calls SET processing_until={active_until} WHERE id='{receipt}'")
    expire_claim()
    assert "可派发 0 条" in command(binary, current, *sweep, "--pretend")
    assert "已派发 0 条" in command(binary, current, *sweep)
    command(binary, current, *clear)
    write_database(driver, sql, f"UPDATE webhook_calls SET processing_until=0 WHERE id='{receipt}'")
    with ThreadPoolExecutor(max_workers=2) as runners:
        outcomes = sorted(run.result() for run in (runners.submit(dispatch), runners.submit(dispatch)))
    assert outcomes == [0, 1] and pending_count() == 1
    assert sql(f"SELECT processing_requeues_left FROM webhook_dispatch_intents WHERE call_id='{receipt}'") == "0"
    expire_claim()
    assert "可派发 0 条" in command(binary, current, *sweep, "--pretend")
    assert "已派发 0 条" in command(binary, current, *sweep)

    replayed_receipt = receipt
    exhausted = ("webhook-client:exhausted", "--scope=" + scope)
    replay = ("webhook-client:replay", receipt, "--scope=" + scope,
              "--actor=fixture-operator", "--reason=checked original receipt")
    assert json.loads(command(binary, current, *exhausted)) == [{
        "id": receipt, "scope": scope, "profile": "processing-recovery",
        "revision": "1", "replay_generation": 0,
    }]
    assert json.loads(command(binary, current, "webhook-client:exhausted", "--scope=outside")) == []

    def replay_result(env, *tokens):
        return subprocess.run([str(binary), "artisan", *tokens], env=env,
                              capture_output=True, text=True, timeout=20)

    authorized = current | {"FIXTURE_REPLAY_ALLOW": "1"}
    assert replay_result(current, *replay).returncode != 0
    wrong_actor = tuple("--actor=unknown" if part.startswith("--actor=") else part for part in replay)
    assert replay_result(authorized, *wrong_actor).returncode != 0
    wrong_scope = tuple("--scope=outside" if part.startswith("--scope=") else part for part in replay)
    assert replay_result(authorized, *wrong_scope).returncode != 0
    assert sql("SELECT COUNT(*) FROM webhook_replays") == "0"
    with ThreadPoolExecutor(max_workers=2) as runners:
        requests = [runners.submit(replay_result, authorized, *replay) for _ in range(2)]
        results = [result.result() for result in requests]
    assert sorted(result.returncode == 0 for result in results) == [False, True], results
    assert "重放代次 1" in next(result.stdout for result in results if result.returncode == 0)
    assert sql(f"SELECT actor FROM webhook_replays WHERE call_id='{receipt}' AND generation=1") == "fixture-operator"
    assert sql(f"SELECT reason FROM webhook_replays WHERE call_id='{receipt}' AND generation=1") == "checked original receipt"
    assert sql(f"SELECT expires_at - created_at FROM webhook_replays WHERE call_id='{receipt}'") == "120"
    assert json.loads(command(binary, current, *exhausted)) == []
    verify_rollback_guard("2026_11_25_000015_create_webhook_replays_table.rs", "重放审计仍在保留期")

    # 原始队列正在等待的旧任务被新重放代次隔离，随后再撤销派发端许可。
    prior = root / "prior-generation-worker.json"
    prior_hook = root / "prior-generation-failure.json"
    if not redis:
        write_database(driver, sql, "UPDATE jobs SET attempts=2 WHERE queue='webhook-recovery'")
    command(binary, worker_env | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(prior),
                                  "FIXTURE_WEBHOOK_FAILED_HOOK": str(prior_hook)}, *worker)
    assert not prior.exists() and not prior_hook.exists() and pending_count() == 0
    denied = replay_result(current, *sweep)
    assert denied.returncode != 0 and "1 条待重试" in denied.stdout, denied
    assert pending_count() == 0
    expire_claim()
    deadline = int(sql(f"SELECT expires_at FROM webhook_replays WHERE call_id='{receipt}'"))
    write_database(driver, sql, f"UPDATE webhook_replays SET expires_at=0 WHERE call_id='{receipt}'")
    expired_replay = replay_result(authorized, *sweep)
    assert expired_replay.returncode != 0 and pending_count() == 0, expired_replay
    write_database(driver, sql, f"UPDATE webhook_replays SET expires_at={deadline} WHERE call_id='{receipt}'")
    expire_claim()
    assert "已派发 1 条" in command(binary, authorized, *sweep)
    assert pending_count() == 1
    expire_claim()
    assert replay_result(authorized, *replay).returncode != 0
    assert sql("SELECT COUNT(*) FROM webhook_replays") == "1"

    if not redis:
        job_id, available_at, payload = sql(
            "SELECT id,available_at,payload FROM jobs WHERE queue='webhook-recovery'"
        ).split("\t", 2)
        job_id, available_at = int(job_id), int(available_at)
        clone = json.loads(payload)
        # 隔离复制任务使用尝试上限，权威重放期限继续由接收数据库检查。
        assert clone["options"].pop("retry_until") is not None
        write_database(driver, sql, f"UPDATE jobs SET available_at={int(time.time()) + 300} WHERE id={job_id}")
        try:
            for allowed in (False, True):
                clone["uuid"] = str(uuid4())
                wire = json.dumps(clone, separators=(",", ":")).replace("'", "''")
                write_database(driver, sql,
                    "INSERT INTO jobs (queue,payload,attempts,reserved_at,available_at,created_at) "
                    f"SELECT queue,'{wire}',2,NULL,0,created_at FROM jobs WHERE id={job_id}")
                failure = root / f"replay-terminal-{allowed}.json"
                failure_env = worker_env | {"FIXTURE_WEBHOOK_FAILED_HOOK": str(failure)}
                if allowed:
                    failure_env["FIXTURE_REPLAY_ALLOW"] = "1"
                command(binary, failure_env, *worker)
                assert failure.exists() == allowed
                if allowed:
                    assert json.loads(failure.read_text())["id"] == receipt
                assert pending_count() == 1, allowed
        finally:
            write_database(driver, sql, f"UPDATE jobs SET available_at={available_at} WHERE id={job_id}")

    denied_worker = root / "denied-replay-worker.json"
    command(binary, worker_env | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(denied_worker)}, *worker)
    assert not denied_worker.exists()
    assert sql(f"SELECT COUNT(processed_at) FROM webhook_calls WHERE id='{receipt}'") == "0"
    worker_allowed = worker_env | {"FIXTURE_REPLAY_ALLOW": "1"}
    marker = root / "processing-recovered-worker.json"
    command(binary, worker_allowed | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(marker)}, *worker)
    record = json.loads(marker.read_text())
    assert bytes(record["body"]) == body and record["name"] == "processing-recovery"
    assert int(sql(f"SELECT processed_at FROM webhook_calls WHERE id='{receipt}'")) > 0
    assert sql(f"SELECT COUNT(*) FROM webhook_dispatch_intents WHERE call_id='{receipt}'") == "1"
    assert "可派发 0 条" in command(binary, current, *sweep, "--pretend")
    assert "已派发 0 条" in command(binary, current, *sweep)
    assert sql(f"SELECT COUNT(*) FROM webhook_dispatch_intents WHERE call_id='{receipt}'") == "0"
    assert pending_count() == 0
    if not redis:
        write_database(driver, sql, f"DELETE FROM failed_jobs WHERE payload LIKE '%{receipt}%'")
    print(f"{driver}: 入列后丢失恢复经 {connection} 实测 1 个 HTTP、2 次 Worker；等待窗口、"
          "清除任务、崩溃租约、并发预算扣减、终态清理及迁移保护通过", flush=True)

    def accept_deferred(profile, payload, tracked):
        nonlocal body
        body = payload
        with (root / "durable-deferred-http.log").open("w+") as log:
            server = http_helpers["serve"](binary, current, log)
            try:
                send(profile, 200)
            finally:
                http_helpers["stop"](server)
        key = hashlib.sha256(body).hexdigest()
        stored_id = sql(
            f"SELECT id FROM webhook_calls WHERE scope='{scope}' AND name='{profile}' "
            f"AND raw_body='{base64.b64encode(body).decode()}'"
        )
        assert re.fullmatch(r"[0-9a-f-]{36}", stored_id), key
        if redis and not tracked and payload == b"recovery-deferred-wake":
            # Redis 已入列，Worker 在原扫描器确认前完成延期。旧令牌不能删掉新意图。
            barrier = root / "defer-before-old-dispatch-ack"
            barrier.mkdir()
            delayed_ack = current | {"FIXTURE_HOLD_AFTER_REDIS_ENQUEUE": str(barrier)}
            with subprocess.Popen([str(binary), "artisan", *sweep], env=delayed_ack,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as dispatcher:
                try:
                    deadline = time.monotonic() + 10
                    while not (barrier / "entered").exists():
                        assert dispatcher.poll() is None and time.monotonic() < deadline
                        time.sleep(.02)
                    assert pending_count() == 1
                    command(binary, worker_env, *worker)
                finally:
                    (barrier / "release").touch()
                    stdout, stderr = dispatcher.communicate(timeout=20)
                assert dispatcher.returncode == 0 and "已派发 1 条" in stdout, stdout + stderr
        else:
            assert "已派发 1 条" in command(binary, current, *sweep)
            retained = sql(f"SELECT COUNT(*) FROM webhook_dispatch_intents WHERE call_id='{stored_id}'")
            assert int(retained) == int(tracked)
            command(binary, worker_env, *worker)
        assert pending_count() == 0
        assert sql(f"SELECT processing_deferrals FROM webhook_calls WHERE id='{stored_id}'") == "1"
        assert sql(f"SELECT COUNT(processed_at) FROM webhook_calls WHERE id='{stored_id}'") == "0"
        next_at = sql(f"SELECT processing_next_at FROM webhook_calls WHERE id='{stored_id}'")
        assert int(next_at) > 0
        assert sql(f"SELECT available_at FROM webhook_dispatch_intents WHERE call_id='{stored_id}'") == next_at
        route = json.loads(sql(f"SELECT route FROM webhook_dispatch_intents WHERE call_id='{stored_id}'"))
        deferred_route = json.loads(sql(
            f"SELECT processing_deferred_route FROM webhook_calls WHERE id='{stored_id}'"))
        assert deferred_route.pop("recover_deferred") is True
        assert route == deferred_route and route["options"]["max_tries"] == 1
        assert "可派发 0 条" in command(binary, current, *sweep, "--pretend")
        return stored_id

    def without_broker(*tokens):
        if redis:
            with socket.socket() as unused:
                unused.bind(("127.0.0.1", 0))
                offline = current | {"REDIS_URL": f"redis://127.0.0.1:{unused.getsockname()[1]}"}
                return subprocess.run([str(binary), "artisan", *tokens], env=offline,
                                      capture_output=True, text=True, timeout=25)
        offline = current | {"FIXTURE_FOREIGN_QUEUE_TABLE": "missing_webhook_recovery_jobs"}
        return subprocess.run([str(binary), "artisan", *tokens], env=offline,
                              capture_output=True, text=True, timeout=25)

    def complete_deferred(id, expected):
        marker = root / f"worker-resume-{id}.json"
        command(binary, worker_env | {"FIXTURE_DEFERRED_READY": "1",
               "WEBHOOK_TEST_RECEIPT_OUTPUT": str(marker)}, *worker)
        assert bytes(json.loads(marker.read_text())["body"]) == expected
        assert sql(f"SELECT COUNT(processed_at) FROM webhook_calls WHERE id='{id}'") == "1"
        assert sql(f"SELECT processing_deferrals FROM webhook_calls WHERE id='{id}'") == "1"

    receipt = accept_deferred("deferred-recovery", b"recovery-deferred-wake", False)
    def wake():
        result = without_broker("webhooks:wake-fixture", receipt, scope)
        assert result.returncode == 0, result
        return result.stdout.strip()
    with ThreadPoolExecutor(max_workers=2) as runners:
        awakenings = sorted(run.result() for run in (runners.submit(wake), runners.submit(wake)))
    assert awakenings == ["false", "true"] and pending_count() == 0
    failed = without_broker(*sweep)
    assert failed.returncode != 0 and "1 条待重试" in failed.stdout, failed
    assert sql(f"SELECT COUNT(processed_at) FROM webhook_calls WHERE id='{receipt}'") == "0"
    expire_claim()
    assert "已派发 1 条" in command(binary, current, *sweep)
    complete_deferred(receipt, b"recovery-deferred-wake")

    scheduled = accept_deferred("deferred-recovery-tracked", b"recovery-deferred-scheduled", True)
    write_database(driver, sql, f"UPDATE webhook_calls SET processing_next_at=0 WHERE id='{scheduled}'")
    write_database(driver, sql, f"UPDATE webhook_dispatch_intents SET available_at=0 WHERE call_id='{scheduled}'")
    assert "已派发 1 条" in command(binary, current, *sweep)
    assert sql(f"SELECT processing_requeues_left FROM webhook_dispatch_intents WHERE call_id='{scheduled}'") == "2"
    complete_deferred(scheduled, b"recovery-deferred-scheduled")
    command(binary, current, *sweep)

    expired = accept_deferred("deferred-recovery", b"recovery-deferred-expire", False)
    write_database(driver, sql, f"UPDATE webhook_calls SET processing_next_at=0 WHERE id='{expired}'")
    write_database(driver, sql, f"UPDATE webhook_dispatch_intents SET available_at=0 WHERE call_id='{expired}'")
    assert "已派发 1 条" in command(binary, current, *sweep)
    assert pending_count() == 1
    command(binary, current, *clear)
    assert pending_count() == 0
    assert sql(f"SELECT COUNT(enqueued_at) FROM webhook_dispatch_intents WHERE call_id='{expired}'") == "1"
    write_database(driver, sql, f"UPDATE webhook_calls SET processing_deadline=0 WHERE id='{expired}'")
    assert "可派发 0 条" in command(binary, current, *sweep, "--pretend")
    assert sql(f"SELECT COUNT(processing_stopped_at) FROM webhook_calls WHERE id='{expired}'") == "0"
    stopped = without_broker(*sweep)
    assert stopped.returncode == 0 and "已派发 0 条" in stopped.stdout, stopped
    assert sql(f"SELECT COUNT(processing_stopped_at) FROM webhook_calls WHERE id='{expired}'") == "1"
    assert sql(f"SELECT COUNT(processed_at) FROM webhook_calls WHERE id='{expired}'") == "0"
    assert sql("SELECT COUNT(*) FROM webhook_dispatch_intents") == "0" and pending_count() == 0
    assert sql("SELECT queue,COUNT(*) FROM jobs WHERE queue <> 'webhook-recovery' GROUP BY queue ORDER BY queue") == unrelated_queues
    print(f"{driver}: 跨后端业务延期经 {connection} 实测 3 个 HTTP、5 次 Worker；定时恢复、"
          "离线唤醒、失败补偿、到期终止与独立预算通过", flush=True)

    write_database(driver, sql,
                   f"UPDATE webhook_calls SET created_at=1,processed_at=1 WHERE id='{replayed_receipt}'")
    assert "已清理 1 条" in command(binary, current, "webhook-client:prune", "--scope=" + scope)
    assert replay_result(authorized, *replay).returncode != 0
    assert sql("SELECT COUNT(*) FROM webhook_replays") == "0"
    print(f"{driver}: 耗尽诊断与受控重放经 {connection} 实测原 HTTP 记录、3 次独立 Worker；"
          "授权拒绝、跨范围、并发代次、撤销、旧队列隔离、预算及保留期通过", flush=True)

def main():
 driver,url=helpers["database_settings"]();binary=args.binary.resolve();requests=workers=0
 with tempfile.TemporaryDirectory(dir=ROOT/"target/tmp",prefix="webhook-routes-") as tmp:
  root=Path(tmp)
  if driver=="sqlite":os.environ["OPENRUST_SMS_SQLITE_PATH"]=str(root/"database.sqlite")
  with socket.socket() as sock:sock.bind(("127.0.0.1",0));port=sock.getsockname()[1]
  env=os.environ|{"APP_BASE_PATH":tmp,"APP_HOST":"127.0.0.1","APP_PORT":str(port),"APP_URL":f"http://127.0.0.1:{port}","APP_ENV":"testing","APP_DEBUG":"false","DB_CONNECTION":driver,
   "DB_DATABASE":os.environ.get("OPENRUST_SMS_SQLITE_PATH","openrust_acceptance"),"CACHE_STORE":"file","SESSION_DRIVER":"array","APP_DISABLED_RUNNERS":"queue,schedule",
   "APP_CONFIG_CACHE":str(root/"configuration.json"),"APP_ROUTES_CACHE":str(root/"routes.json"),"QUEUE_CONNECTION":"database","APP_KEY":"base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","APP_SUPPORTED_LOCALES":"",
   "FIXTURE_SPATIE_SECRET":SECRET,"FIXTURE_STANDARD_SECRET":STANDARD}
  if url:env["DB_URL"]=url
  else:env.pop("DB_URL",None)
  sql=lambda query:helpers["database_rows"](helpers["database_container"](),query).strip()
  command(binary,env,"migrate","--force")
  check_instance_pruning(binary,env,root,driver,sql)
  check_mass_pruning(binary,env,root,driver,sql)
  for phase in ("plain","cached","unique"):
   current=env|{"FIXTURE_UNIQUE_ROUTES":"true" if phase=="unique" else "false"}
   if phase=="unique":command(binary,current,"config:clear");command(binary,current,"route:clear")
   if phase!="plain":
    command(binary,current,"config:cache");command(binary,current,"route:cache")
    cache=json.loads((root/"routes.json").read_text());assert cache["version"]==1 and set(cache)=={"version","methods"}
   routes=json.loads(command(binary,current,"route:list","--json"))
   package=[r for r in routes if "WebhookController::invoke" in r["action"]]
   assert package and not any("must-not-register" in r["uri"] for r in routes)
   for route in package:
    if phase=="unique":assert re.fullmatch(r"(?:(?:team|domain)\.)?webhook-client-[a-z-]+\.[A-Za-z0-9]{8}",route["name"]),route
    else:assert route["name"].startswith(("webhook-client-","team.webhook-client-","domain.webhook-client-")),route
   with (root/f"http-{phase}.log").open("w+") as log:
    server=http_helpers["serve"](binary,current,log)
    try:
     def send(path,body=b"",method="POST",status=200,valid=True,standard=False,custom=False,expected_name=None,scope="webhook-acceptance",stored=True,queued=True,header=None,content_type="application/octet-stream",host=None,attempt_failure=False,terminal=False):
      nonlocal requests,workers
      before=set(sql("SELECT id FROM webhook_calls").split())
      signature=hmac.new(SECRET.encode(),body,hashlib.sha256).hexdigest()
      headers={"Content-Type":content_type,"Accept":"application/json","X-Webhook" if custom else "Signature":signature if valid else "0"*64}
      if standard:
       ts=int(time.time());event=f"event-{phase}-{requests}";signature=base64.b64encode(hmac.new(base64.b64decode(STANDARD[6:]),f"{event}.{ts}.".encode()+body,hashlib.sha256).digest()).decode()
       headers={"Content-Type":content_type,"Accept":"application/json","webhook-id":event,"webhook-timestamp":str(ts),"webhook-signature":f"v1,{signature}"}
      if host:headers["Host"]=host
      connection=http.client.HTTPConnection("127.0.0.1",port,timeout=10)
      try:
       if status==413 and content_type!="application/json":
        # 以 Expect 预检大正文；读取真实 413，不把早期拒绝造成的写端 BrokenPipe 当验收。
        connection.putrequest(method,path)
        for key,value in headers.items():connection.putheader(key,value)
        connection.putheader("Content-Length",str(len(body)));connection.putheader("Expect","100-continue");connection.endheaders()
       else:connection.request(method,path,body,headers)
       response=connection.getresponse();data=response.read()
       assert response.status==status,(phase,method,path,response.status,status,data[:180])
       assert SECRET.encode() not in data and STANDARD.encode() not in data
       if header:assert response.getheader(header[0])==header[1],(path,response.getheaders())
       if status in (200,202) and method!="HEAD" and not(method=="OPTIONS"):
        assert json.loads(data)==({"profile":"custom"} if custom else {"message":"ok"}),(path,data)
       requests+=1
      finally:connection.close()
      after=set(sql("SELECT id FROM webhook_calls").split());created=after-before
      if not stored:
       assert not created,(path,created);assert sql("SELECT COUNT(*) FROM jobs")=="0";return
      assert len(created)==1,(path,created);receipt=created.pop()
      record=json.loads(command(binary,current,"webhooks:record",receipt,scope))
      assert record["name"]==expected_name and bytes(record["body"])==body and record["method"]==method,record
      if host:
       from urllib.parse import urlsplit
       uri=urlsplit(record["url"]);assert uri.scheme=="http" and uri.netloc.lower()==host.lower() and uri.path==path
      else:assert record["url"]==f"http://127.0.0.1:{port}{path}",record
      if queued:
       wire=sql("SELECT payload FROM jobs")
       assert SECRET not in wire and STANDARD not in wire
       payload=json.loads(wire)
       assert payload["name"].startswith("openrust_webhook_client::jobs::fixture")
       assert payload["data"]=={"id":receipt,"scope":scope,"profile":expected_name,"revision":record["revision"]}
       assert payload["options"]["max_tries"]==2 and payload["options"]["timeout"]["secs"]==12
       assert record["exception"] is None
       marker=root/f"worker-{receipt}.json"
       failure=root/f"worker-{receipt}-failure.json"
       worker_env=current|{"WEBHOOK_TEST_RECEIPT_OUTPUT":str(marker),"FIXTURE_WEBHOOK_FAILED_HOOK":str(failure),"FIXTURE_SPATIE_SECRET":"other-worker-secret","FIXTURE_STANDARD_SECRET":"unused"}
       def work():
        nonlocal workers
        command(binary,worker_env,"queue:work","database","--queue=webhook-receipts","--once","--tries=1")
        workers+=1
       if attempt_failure or terminal:
        work()
        assert not marker.exists() and sql("SELECT COUNT(*) FROM jobs")=="1"
        assert not failure.exists()
        failed=json.loads(command(binary,current,"webhooks:record",receipt,scope))
        assert failed["exception"]=={"code":0,"message":"Webhook 处理任务执行失败"}
        diagnostic=json.loads(command(binary,current,"webhook-client:inspect",receipt,"--scope",scope))
        assert diagnostic["state"]=="failed" and diagnostic["has_exception"] and "exception" not in diagnostic
       work()
       if terminal:
        assert not marker.exists()
        failed_hook=json.loads(failure.read_text())
        assert failed_hook=={"id":receipt,"scope":scope,"profile":expected_name,
                             "queue":"webhook-receipts","connection":"database","attempts":2,"failed_jobs":1,
                             "exception":{"code":0,"message":"Webhook 处理任务执行失败"},
                             "redacted_error":True,"expected_body":True},failed_hook
        failed=json.loads(command(binary,current,"webhooks:record",receipt,scope))
        assert failed["exception"]=={"code":0,"message":"Webhook 处理任务执行失败"}
        assert "private-business-error-sentinel" not in sql("SELECT exception FROM failed_jobs")
        assert sql("SELECT COUNT(*) FROM failed_jobs")=="1"
       else:
        assert json.loads(marker.read_text())==record
        assert not failure.exists()
      else:assert record["exception"]=={"code":0,"message":"Webhook 处理任务派发失败"}
      assert sql("SELECT COUNT(*) FROM jobs")=="0"
      return record
     send("/api/package/default",b"default",expected_name="default")
     send("/api/package/atomic",b"atomic-receipt",expected_name="atomic")
     send("/api/package/atomic-null",b"atomic-null",status=500,stored=False)
     send("/api/package/atomic-foreign",b"atomic-foreign",status=500,stored=False)
     send("/api/package/post",b"retry-binary-receipt",expected_name="spatie",attempt_failure=True)
     if phase=="unique":
      send("/api/package/post",b"terminal-binary-receipt",expected_name="spatie",terminal=True)
     send("/api/package/post",b" post\0\xff\n",expected_name="spatie")
     record=send("/api/package/post?text=query&source=external",b' { "text": "  keep  ", "empty": "" }\n',content_type="application/json",expected_name="spatie")
     assert record["payload"]=={"text":"  keep  ","empty":"","source":"external"} and record["attachments"] is None
     old_file={"originalName":"旧版.bin","mimeType":"application/octet-stream","size":3,
               "path":"/fixture/opaque/path/to/old-upload","error":0,"content":"AP9h"}
     exported={"payload":{"attachments":[old_file]}}
     record=send("/api/package/post",json.dumps(exported["payload"]).encode(),
                 content_type="application/json",expected_name="spatie")
     assert record["payload"]==exported["payload"] and record["attachments"] is None
     if phase == "unique":
      file=root/"legacy-attachments.json"
      for archived, expected in (
          (exported,{"0":{"name":"旧版.bin","mime":"application/octet-stream","bytes":[0,255,97]}}),
          ({"attachments":None}|exported,{"0":{"name":"旧版.bin","mime":"application/octet-stream","bytes":[0,255,97]}}),
          ({"attachments":[]}|exported,{}),
          ({"attachments":{"file":old_file}}|exported,{"file":{"name":"旧版.bin","mime":"application/octet-stream","bytes":[0,255,97]}}),
      ):
       file.write_text(json.dumps(archived))
       assert json.loads(command(binary,current,"webhooks:fixture-restore-legacy-attachments",str(file))) == expected
      file.write_text(json.dumps({"attachments":[old_file|{"content":"@bad!"}]}|exported))
      invalid=subprocess.run([str(binary),"artisan","webhooks:fixture-restore-legacy-attachments",str(file)],
                             env=current,capture_output=True,text=True,timeout=20)
      assert invalid.returncode != 0 and "@bad!" not in invalid.stdout + invalid.stderr
      assert "Webhook 存储记录无效" in invalid.stdout + invalid.stderr
     record=send("/api/package/post",b'a=1&a=2&message=%20keep%20',content_type="application/x-www-form-urlencoded",expected_name="spatie")
     assert record["payload"]=={"a":"2","message":" keep "} and record["attachments"] is None
     multipart=(
      b'--sample\r\nContent-Disposition: form-data; name="message"\r\n\r\nvalue\r\n'
      b'--sample\r\nContent-Disposition: form-data; name="files[]"; filename="../sample.bin"\r\n'
      b'Content-Type: application/octet-stream\r\n\r\n\x00\xfffile\r\n--sample--\r\n'
     )
     media="multipart/form-data; boundary=sample"
     record=send("/api/package/post",multipart,content_type=media,expected_name="spatie")
     assert record["payload"]=={"message":"value"} and record["attachments"]=={
      "files":{"0":{"name":"sample.bin","mime":"application/octet-stream","bytes":list(b"\x00\xfffile")}}
     },record
     record=send("/api/package/no-attachments",multipart,content_type=media,expected_name="no-attachments")
     assert record["payload"]=={"message":"value"} and record["attachments"] is None
     send("/api/package/post",b"wrong-signature",valid=False,status=500,stored=False)
     send("/api/package/unknown",status=500,stored=False)
     send("/api/package/ignored",b"ignored-valid",stored=False)
     send("/api/package/discarded",b"null-connection",status=500,expected_name="discarded",queued=False)
     send("/api/package/unregistered",b"missing-handler",status=500,expected_name="unregistered",queued=False)
     send("/api/package/factory-fails",b"constructor-failure",status=500,expected_name="factory-fails",queued=False)
     send("/api/package/custom",b"custom",status=202,custom=True,scope="webhook-custom",expected_name="custom")
     send("/api/package/standard","中文🙂".encode(),standard=True,expected_name="standard")
     for method in ("GET","HEAD","POST","PUT","PATCH","DELETE","QUERY"):
      send("/api/package/methods",b"" if method in ("GET","HEAD") else method.encode(),method,expected_name="spatie")
     send("/api/package/post",method="GET",status=405,stored=False,header=("Allow","POST"))
     send("/api/package/methods",method="OPTIONS",stored=False,header=("Allow","GET,HEAD,POST,PUT,PATCH,DELETE,QUERY"))
     send("/api/package/methods",method="TRACE",status=405,stored=False,header=("Allow","GET, HEAD, POST, PUT, PATCH, DELETE, QUERY"))
     send("/api/package/resolved",b"router-current-name",expected_name="spatie")
     send("/api/package/overlap",b"last-wins",expected_name="spatie")
     send("/api/package/provider/standard?config=custom",b'{"name":"standard"}',expected_name="spatie")
     send("/api/package/provider/%E4%B8%AD%E6%96%87?path=%2F",b"encoded-path",expected_name="spatie")
     send("/api/package/domain/callback",b"domain",host="Acme.WEBHOOK.test",expected_name="spatie")
     send("/api/package/domain/callback",b"wrong-domain",host="wrong.invalid",status=404,stored=False)
     send("/api/package/group/direct",b"group",expected_name="spatie",header=("X-Webhook-Group","preserved"))
     send("/api/package/optional",b"default-parameter",expected_name="spatie")
     send("/api/package/optional/123",b"constraint-valid",expected_name="spatie")
     send("/api/package/optional/not-number",status=404,stored=False)
     send("/api/package/must-not-register",status=404,stored=False)
     send("/api/package/post",b"x"*(1024*1024+1),status=413,stored=False)
     send("/api/package/post",b'{"text":"'+b"x"*(1024*1024)+b'"}',status=413,stored=False,content_type="application/json")
     send("/webhook-web/protected",b"signed-but-no-csrf",status=419,stored=False)
     send("/webhook-web/ordinary",b"no-global-bypass",status=419,stored=False)
     send("/webhook-web/except",b"explicit-csrf-exception",expected_name="spatie")
    except BaseException:
     log.flush();print((root/f"http-{phase}.log").read_text());raise
    finally:http_helpers["stop"](server)
  assert sql("SELECT COUNT(*) FROM failed_jobs")=="1"
  original=set(sql("SELECT id FROM webhook_calls").split())
  for checkpoint,exit_code in (("before-enqueue",77),("after-enqueue",78)):
   with (root/f"crash-{checkpoint}.log").open("w+") as log:
    server=http_helpers["serve"](binary,current|{"FIXTURE_ATOMIC_CRASH":checkpoint},log)
    connection=http.client.HTTPConnection("127.0.0.1",port,timeout=10)
    try:
     body=b"atomic-crash-probe"
     signature=hmac.new(SECRET.encode(),body,hashlib.sha256).hexdigest()
     connection.request("POST","/api/package/atomic",body,{"Content-Type":"application/octet-stream","Signature":signature})
     try:
      response=connection.getresponse()
     except http.client.RemoteDisconnected:
      pass
     else:
      raise AssertionError((checkpoint,response.status,response.read()))
     assert server.wait(timeout=10)==exit_code
     requests+=1
    finally:
     connection.close()
     if server.poll() is None:http_helpers["stop"](server)
   assert set(sql("SELECT id FROM webhook_calls").split())==original
   assert sql("SELECT COUNT(*) FROM jobs")=="0"
  command(binary,env,"config:clear")
  with (root/"missing-queue-table.log").open("w+") as log:
   server=http_helpers["serve"](binary,env|{"DB_QUEUE_TABLE":"fixture_absent_jobs"},log)
   connection=http.client.HTTPConnection("127.0.0.1",port,timeout=10)
   try:
    body=b"atomic-restart-probe"
    signature=hmac.new(SECRET.encode(),body,hashlib.sha256).hexdigest()
    connection.request("POST","/api/package/atomic",body,{"Content-Type":"application/octet-stream","Signature":signature})
    response=connection.getresponse()
    assert response.status==500,response.read()
    requests+=1
   finally:
    connection.close()
    http_helpers["stop"](server)
  assert set(sql("SELECT id FROM webhook_calls").split())==original
  assert sql("SELECT COUNT(*) FROM jobs")=="0"
  check_processing_lease(binary,env,root,port,driver,sql)
  check_verified_event_identity(binary,env,root,port,driver,sql)
  check_deferred_processing(binary,env,root,port,driver,sql)
  check_body_deduplication(binary,env,root,port,driver,sql)
  check_unverified_hints(binary,env,root,port,driver,sql)
  check_hint_retention(binary,env,driver,sql)
  check_verification_audit(binary,env,root,port,driver,sql)
  check_attachment_storage(binary,env,root,port,driver,sql)
  check_retention(binary,env,root,port,driver,sql)
  check_dispatch_recovery(binary,env,root,port,driver,sql)
  legacy_id=sql("SELECT id FROM webhook_calls WHERE name='default' ORDER BY id LIMIT 1")
  assert "add_payload_to_webhook_calls_table" in command(binary,env,"migrate:rollback","--step=14","--force")
  assert "add_payload_to_webhook_calls_table" in command(binary,env,"migrate","--force")
  legacy=json.loads(command(binary,env,"webhooks:record",legacy_id,"webhook-acceptance"))
  assert bytes(legacy["body"])==b"default" and legacy["payload"] is None and legacy["attachments"] is None

  print(f"{driver}: 公开路由／控制器 {requests} 个真实 HTTP、{workers} 个独立 Worker；方法／HEAD／Allow／组／覆盖／参数／约束／局部 CSRF／工厂／缓存与唯一名称通过")
if __name__=="__main__":main()
