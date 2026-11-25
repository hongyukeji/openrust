"""真实 HTTP 保存原文，由独立 Artisan Worker 按 ID／范围重新读取；每次使用隔离数据库。"""
import argparse
import base64
import hashlib
import hmac
import http.client
import json
import os
from pathlib import Path
import runpy
import socket
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument("binary",type=Path);args=p.parse_args()
helpers=runpy.run_path(str(ROOT/"tests/sms_worker.py"))
http_helpers=runpy.run_path(str(ROOT/"tests/webhooks_acceptance.py"),run_name="webhook_http_helpers")
SECRET="receiver-private-key"
STANDARD="whsec_C2FVsBQIhrscChlQIMV+b5sSYspob7oD"


def command(binary,env,*tokens,success=True):
    result=subprocess.run([str(binary),"artisan",*tokens],env=env,capture_output=True,text=True,timeout=45)
    assert (result.returncode==0)==success,result.stdout+result.stderr
    return result.stdout


def main():
    driver,url=helpers["database_settings"]();binary=args.binary.resolve()
    with tempfile.TemporaryDirectory(dir=ROOT/"target/tmp",prefix="webhook-receiver-") as tmp:
        root=Path(tmp)
        if driver=="sqlite":os.environ["OPENRUST_SMS_SQLITE_PATH"]=str(root/"database.sqlite")
        with socket.socket() as sock:sock.bind(("127.0.0.1",0));port=sock.getsockname()[1]
        env=os.environ|{"APP_BASE_PATH":tmp,"APP_HOST":"127.0.0.1","APP_PORT":str(port),"APP_URL":f"http://127.0.0.1:{port}","APP_ENV":"testing","APP_DEBUG":"false","DB_CONNECTION":driver,
            "DB_DATABASE":os.environ.get("OPENRUST_SMS_SQLITE_PATH","openrust_acceptance"),"CACHE_STORE":"file","SESSION_DRIVER":"array","APP_DISABLED_RUNNERS":"queue,schedule",
            "APP_CONFIG_CACHE":str(root/"configuration.json"),"QUEUE_CONNECTION":"database","FIXTURE_SPATIE_SECRET":SECRET,"FIXTURE_STANDARD_SECRET":STANDARD}
        if url:env["DB_URL"]=url
        else:env.pop("DB_URL",None)
        container=helpers["database_container"]()
        sql=lambda query:helpers["database_rows"](container,query).strip()
        command(binary,env,"migrate","--force")
        assert "webhook_calls" in command(binary,env,"migrate:status")
        def write_sql(query):
            if driver=="sqlite":
                import sqlite3
                from contextlib import closing
                with closing(sqlite3.connect(container)) as connection:
                    connection.execute(query);connection.commit()
            else:sql(query)
        accepted=[];request_count=0
        for cached in (False,True):
            if cached:command(binary,env,"config:cache")
            with (root/f"http-{cached}.log").open("w") as log:
                server=http_helpers["serve"](binary,env,log)
                try:
                    def send(profile,body,status,valid=True,opaque=False):
                        nonlocal request_count
                        signature=hmac.new(SECRET.encode(),body,hashlib.sha256).hexdigest()
                        headers={"Signature":signature if valid else "0"*64}
                        if profile=="standard":
                            import time
                            timestamp=int(time.time());event_id=f"message-{cached}-{request_count}"
                            sig=base64.b64encode(hmac.new(base64.b64decode(STANDARD[6:]),f"{event_id}.{timestamp}.".encode()+body,hashlib.sha256).digest()).decode()
                            headers={"webhook-id":event_id,"webhook-timestamp":str(timestamp),"webhook-signature":f"v1,{sig}"}
                        connection=http.client.HTTPConnection("127.0.0.1",port,timeout=10)
                        try:
                            connection.putrequest("POST",f"/api/received/{profile}?x=%2F")
                            connection.putheader("Content-Type","application/octet-stream");connection.putheader("Content-Length",str(len(body)))
                            connection.putheader("Accept","application/json");connection.putheader("X-Source","test-supplier")
                            connection.putheader("X-Duplicate","one");connection.putheader("X-Duplicate","two")
                            if opaque:connection.putheader("X-Opaque",b"\xff")
                            connection.putheader("Authorization","must-not-be-stored")
                            for key,value in headers.items():connection.putheader(key,value)
                            connection.endheaders(body);response=connection.getresponse();value=response.read()
                            assert response.status==status,(profile,response.status,status,value[:160])
                            assert SECRET.encode() not in value and STANDARD.encode() not in value
                            if status==200:assert json.loads(value)=={"message":"ok"}
                            request_count+=1
                        finally:connection.close()
                    previous=int(sql("SELECT COUNT(*) FROM webhook_calls"))
                    send("spatie",b"invalid",403,False)
                    send("ignored",b"ignored-valid-event",200)
                    send("unknown",b"",404)
                    send("spatie",b"x"*(1024*1024+1),413)
                    assert int(sql("SELECT COUNT(*) FROM webhook_calls"))==previous
                    assert "0"==sql("SELECT COUNT(*) FROM jobs")
                    write_sql("ALTER TABLE webhook_calls RENAME TO webhook_calls_unavailable")
                    try:send("spatie",b"storage-unavailable",500)
                    finally:write_sql("ALTER TABLE webhook_calls_unavailable RENAME TO webhook_calls")
                    assert int(sql("SELECT COUNT(*) FROM webhook_calls"))==previous
                    assert "0"==sql("SELECT COUNT(*) FROM jobs")
                    for profile,body in [("spatie",b"\x00\xff raw\n"),("spatie",b'{ "event": "preserve spaces" }\n'),("standard","中文繁體🙂".encode())]:
                        send(profile,body,200,opaque=profile=="spatie")
                        ids=sql("SELECT id FROM webhook_calls WHERE name IN ('spatie','standard')").split()
                        seen={entry[0] for entry in accepted}
                        receipt=next(value for value in ids if value not in seen)
                        accepted.append((receipt,profile,body))
                        data=json.loads(command(binary,env,"webhooks:record",receipt,"webhook-acceptance"))
                        assert bytes(data["body"])==body and data["revision"]=="one" and data["method"]=="POST"
                        assert data["url"]==f"http://127.0.0.1:{port}/api/received/{profile}?x=%2F",data
                        stored=data["headers"]
                        assert [bytes(value) for name,value in stored if name=="x-duplicate"]==[b"one",b"two"]
                        assert not any(name in ("authorization","signature","webhook-signature") for name,_ in stored)
                        if profile=="spatie":assert any(name=="x-opaque" and value==[255] for name,value in stored)
                        command(binary,env,"webhooks:record",receipt,"other-tenant",success=False)
                        command(binary,env,"webhooks:record",receipt,"WEBHOOK-ACCEPTANCE",success=False)
                        wire=sql("SELECT payload FROM jobs")
                        assert receipt in wire and SECRET not in wire and STANDARD not in wire and "preserve spaces" not in wire
                        marker=root/f"{receipt}.json"
                        # Worker 不重新校验旧签名、不需要接收端当前密钥；仅按已保存记录工作。
                        command(binary,env|{"WEBHOOK_TEST_RECEIPT_OUTPUT":str(marker),"FIXTURE_SPATIE_SECRET":"worker-different-key","FIXTURE_STANDARD_SECRET":"not-used"},"queue:work","database","--queue=webhook-receipts","--once","--tries=1")
                        assert json.loads(marker.read_text())==data
                        assert "0"==sql("SELECT COUNT(*) FROM jobs")
                    before_failures=set(sql("SELECT id FROM webhook_calls WHERE name='dispatch-fails'").split())
                    send("dispatch-fails",b"dispatch-failure-body",500)
                    failure_id=next(iter(set(sql("SELECT id FROM webhook_calls WHERE name='dispatch-fails'").split())-before_failures))
                    failure=json.loads(command(binary,env,"webhooks:record",failure_id,"webhook-acceptance"))
                    assert failure["exception"]=={"code":0,"message":"Webhook 处理任务派发失败"}
                    assert "0"==sql("SELECT COUNT(*) FROM jobs")
                    manual=json.loads(command(binary,env,"webhooks:record",failure_id,"webhook-acceptance","--save-exception",'{"code":"manual"}'))
                    assert manual["exception"]=={"code":"manual"}
                    command(binary,env,"webhooks:record",failure_id,"webhook-acceptance","--save-exception",json.dumps({"oversize":"x"*8192}),success=False)
                    assert json.loads(command(binary,env,"webhooks:record",failure_id,"webhook-acceptance"))["exception"]=={"code":"manual"}
                    for _ in range(2):
                        assert json.loads(command(binary,env,"webhooks:record",failure_id,"webhook-acceptance","--clear-exception"))["exception"] is None
                finally:http_helpers["stop"](server)
        assert sql("SELECT COUNT(*) FROM failed_jobs")=="0"
        assert int(sql("SELECT COUNT(*) FROM webhook_calls"))==8
        assert SECRET not in sql("SELECT raw_body,headers,exception FROM webhook_calls")
        print(f"{driver}: {request_count} 个真实 HTTP 请求；缓存前后原文／重复与非 UTF-8 头／筛选／拒绝／派发失败／跨范围隔离，6 个独立 Artisan Worker 读取持久记录通过")

if __name__=="__main__":main()
