"""用既有队列化 Artisan 命令，让独立 Worker 实际消费两个包的签名与配置。"""
import argparse
import hashlib
import hmac
import json
import os
from pathlib import Path
import runpy
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument("binary",type=Path);args=p.parse_args()
helpers=runpy.run_path(str(ROOT/"tests/sms_worker.py"))


def command(binary,env,*tokens):
    result=subprocess.run([str(binary),"artisan",*tokens],env=env,capture_output=True,text=True,timeout=45)
    assert result.returncode==0,result.stdout+result.stderr
    return result.stdout


def main():
    driver,url=helpers["database_settings"]()
    binary=args.binary.resolve()
    with tempfile.TemporaryDirectory(dir=ROOT/"target/tmp",prefix="webhook-worker-") as tmp:
        root=Path(tmp)
        if driver=="sqlite":os.environ["OPENRUST_SMS_SQLITE_PATH"]=str(root/"database.sqlite")
        env=os.environ|{"APP_BASE_PATH":tmp,"APP_ENV":"testing","APP_DEBUG":"false","DB_CONNECTION":driver,
            "DB_DATABASE":os.environ.get("OPENRUST_SMS_SQLITE_PATH","openrust_acceptance"),
            "CACHE_STORE":"file","SESSION_DRIVER":"array","APP_DISABLED_RUNNERS":"queue,schedule",
            "APP_CONFIG_CACHE":str(root/"configuration.json"),"QUEUE_CONNECTION":"database","DB_QUEUE":"webhook-primitives",
            "FIXTURE_SPATIE_SECRET":"worker-private-key","FIXTURE_STANDARD_SECRET":"whsec_C2FVsBQIhrscChlQIMV+b5sSYspob7oD"}
        if url:env["DB_URL"]=url
        else:env.pop("DB_URL",None)
        command(binary,env,"migrate","--force")
        command(binary,env,"config:cache")
        producer=env|{"FIXTURE_SPATIE_SECRET":"producer-private-key","FIXTURE_STANDARD_SECRET":"producer-not-worker"}
        for profile in ("spatie","standard"):
            marker=root/f"{profile}-headers.json"
            command(binary,producer,"webhooks:enqueue",profile,"worker-body")
            wire=helpers["database_rows"](helpers["database_container"](),"SELECT payload FROM jobs")
            assert "producer-private-key" not in wire and "worker-private-key" not in wire and "producer-not-worker" not in wire
            assert "FIXTURE_SPATIE_SECRET" not in wire and "whsec_" not in wire
            assert not marker.exists()
            command(binary,env|{"WEBHOOK_TEST_OUTPUT":str(marker)},"queue:work","database","--queue=webhook-primitives","--once","--tries=1")
            headers=json.loads(marker.read_text())["headers"]
            if profile=="spatie":
                assert headers["signature"]==hmac.new(b"worker-private-key",b"worker-body",hashlib.sha256).hexdigest()
            else:
                import base64
                data=f"{headers['webhook-id']}.{headers['webhook-timestamp']}.worker-body".encode()
                expected="v1,"+base64.b64encode(hmac.new(base64.b64decode(env["FIXTURE_STANDARD_SECRET"][6:]),data,hashlib.sha256).digest()).decode()
                assert headers["webhook-signature"]==expected
        rows=helpers["database_rows"](helpers["database_container"](),"SELECT COUNT(*) FROM jobs; SELECT COUNT(*) FROM failed_jobs").split()
        assert rows==["0","0"],rows
        print(f"{driver}: 独立 Artisan Worker 使用自己的凭据与包发现，缓存内仅有引用，两个协议通过")

if __name__=="__main__":main()
