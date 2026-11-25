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
  legacy_id=sql("SELECT id FROM webhook_calls WHERE name='default' ORDER BY id LIMIT 1")
  assert "add_payload_to_webhook_calls_table" in command(binary,env,"migrate:rollback","--step=1","--force")
  assert "add_payload_to_webhook_calls_table" in command(binary,env,"migrate","--force")
  legacy=json.loads(command(binary,env,"webhooks:record",legacy_id,"webhook-acceptance"))
  assert bytes(legacy["body"])==b"default" and legacy["payload"] is None and legacy["attachments"] is None

  print(f"{driver}: 公开路由／控制器 {requests} 个真实 HTTP、{workers} 个独立 Worker；方法／HEAD／Allow／组／覆盖／参数／约束／局部 CSRF／工厂／缓存与唯一名称通过")
if __name__=="__main__":main()
