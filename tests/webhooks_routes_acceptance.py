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
        return current | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(root / marker), **extra}

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

    log_path = root / "processing-lease.log"
    with log_path.open("w+") as log:
        server = http_helpers["serve"](binary, current, log)
        try:
            released = dispatch(b"leased-release")
            run("release")
            assert state(released) == "1\t0\t0" and not (root / "release").exists()
            run("release")
            assert state(released) == "2\t0\t1" and (root / "release").exists()

            concurrent = dispatch(b"leased-concurrent")
            duplicate = (
                "INSERT INTO jobs (queue,payload,attempts,reserved_at,available_at,created_at) "
                "SELECT queue,payload,0,NULL,available_at,created_at FROM jobs"
            )
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
            run("recovered")
            assert state(crashed) == "1\t1\t0" and not (root / "recovered").exists()
            time.sleep(8.2)
            run("recovered")
            assert state(crashed) == "2\t0\t1" and (root / "recovered").exists()
            assert sql("SELECT COUNT(*) FROM jobs") == "0"
        except BaseException:
            log.flush()
            print(log_path.read_text())
            raise
        finally:
            http_helpers["stop"](server)
    print(f"{driver}: 接收处理租约 3 个真实 HTTP、8 个独立 Worker；主动释放、并发重复投递、终态去重、进程崩溃重领通过")


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
       worker_env=current|{"WEBHOOK_TEST_RECEIPT_OUTPUT":str(marker),"FIXTURE_SPATIE_SECRET":"other-worker-secret","FIXTURE_STANDARD_SECRET":"unused"}
       def work():
        nonlocal workers
        command(binary,worker_env,"queue:work","database","--queue=webhook-receipts","--once","--tries=1")
        workers+=1
       if attempt_failure or terminal:
        work()
        assert not marker.exists() and sql("SELECT COUNT(*) FROM jobs")=="1"
        failed=json.loads(command(binary,current,"webhooks:record",receipt,scope))
        assert failed["exception"]=={"code":0,"message":"Webhook 处理任务执行失败"}
       work()
       if terminal:
        assert not marker.exists()
        failed=json.loads(command(binary,current,"webhooks:record",receipt,scope))
        assert failed["exception"]=={"code":0,"message":"Webhook 处理任务执行失败"}
        assert "private-business-error-sentinel" not in sql("SELECT exception FROM failed_jobs")
        assert sql("SELECT COUNT(*) FROM failed_jobs")=="1"
       else:
        assert json.loads(marker.read_text())==record
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
  legacy_id=sql("SELECT id FROM webhook_calls WHERE name='default' ORDER BY id LIMIT 1")
  assert "add_payload_to_webhook_calls_table" in command(binary,env,"migrate:rollback","--step=3","--force")
  assert "add_payload_to_webhook_calls_table" in command(binary,env,"migrate","--force")
  legacy=json.loads(command(binary,env,"webhooks:record",legacy_id,"webhook-acceptance"))
  assert bytes(legacy["body"])==b"default" and legacy["payload"] is None and legacy["attachments"] is None

  print(f"{driver}: 公开路由／控制器 {requests} 个真实 HTTP、{workers} 个独立 Worker；方法／HEAD／Allow／组／覆盖／参数／约束／局部 CSRF／工厂／缓存与唯一名称通过")
if __name__=="__main__":main()
