"""核对独立包发现、发布、配置缓存与真实 HTTP／Artisan 签名消费者。"""
import argparse
import base64
import hashlib
import hmac
import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

ROOT=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument("binary",type=Path);args=p.parse_args()
SPATIE="fixture-private-key-should-not-appear"
STANDARD="whsec_C2FVsBQIhrscChlQIMV+b5sSYspob7oD"


def artisan(binary,env,*tokens):
    process=subprocess.run([str(binary),"artisan",*tokens],env=env,capture_output=True,text=True,timeout=30)
    assert process.returncode==0,process.stdout+process.stderr
    return process.stdout


def exchange(port,path,body,headers,expected):
    req=Request(f"http://127.0.0.1:{port}/api/webhooks/{path}",data=body,headers={"Content-Type":"application/octet-stream","Accept":"application/json"}|headers,method="POST")
    try:
        with urlopen(req,timeout=5) as response:
            code=response.status;value=response.read()
    except HTTPError as error:
        code=error.code;value=error.read()
    assert code==expected,(code,expected,len(body),body[:24],value[:250])
    if expected==200: assert json.loads(value)=={"message":"ok"},value


def duplicate_header(port,profile,body,headers,name):
    connection=http.client.HTTPConnection("127.0.0.1",port,timeout=5)
    try:
        connection.putrequest("POST",f"/api/webhooks/{profile}")
        connection.putheader("Content-Type","application/octet-stream")
        connection.putheader("Accept","application/json")
        connection.putheader("Content-Length",str(len(body)))
        for key,value in headers.items():connection.putheader(key,value)
        connection.putheader(name,headers[name])
        connection.endheaders(body)
        response=connection.getresponse();data=response.read()
        assert response.status==403,(profile,name,response.status,data)
    finally:connection.close()


def serve(binary,env,log):
    process=subprocess.Popen([str(binary)],env=env|{"WEBHOOK_HTTP":"1"},stdout=log,stderr=log)
    deadline=time.monotonic()+20
    try:
        while True:
            assert process.poll() is None,"HTTP 进程提前退出"
            try:
                # 可连接后不依赖业务端点成功作为启动探针。
                with socket.create_connection(("127.0.0.1",int(env["APP_PORT"])),timeout=.3):
                    return process
            except OSError:
                assert time.monotonic()<deadline,"等待 HTTP 就绪超时"
                time.sleep(.05)
    except BaseException:
        process.terminate();process.wait(timeout=5)
        raise


def stop(process):
    process.terminate()
    try:process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill();process.wait(timeout=5)


def exercise(binary,env,log):
    port=int(env["APP_PORT"])
    process=serve(binary,env,log)
    try:
        for body in [b"",b'{"id":1}',b'{ "id": 1 }\n',"中文繁體🙂".encode(),b"a=1&a=2",b"\x00\xff"]:
            signature=hmac.new(SPATIE.encode(),body,hashlib.sha256).hexdigest()
            exchange(port,"spatie",body,{"Signature":signature},200)
            exchange(port,"spatie",body+b" ",{"Signature":signature},403)
        for body, content_type in [
            (b'{ "text": " keep whitespace " }\n', "application/json"),
            (b'a=1&a=2&message=hello%20world', "application/x-www-form-urlencoded"),
            (b'--sample\r\nContent-Disposition: form-data; name="message"\r\n\r\nvalue\r\n--sample--\r\n', "multipart/form-data; boundary=sample"),
        ]:
            signature=hmac.new(SPATIE.encode(),body,hashlib.sha256).hexdigest()
            exchange(port,"spatie",body,{"Signature":signature,"Content-Type":content_type},200)
        body=b'{"event":"fixture"}'
        for profile in ("spatie","standard"):
            headers=json.loads(artisan(binary,env,"webhooks:sign",profile,body.decode()))["headers"]
            if profile=="spatie":
                assert headers["signature"]==hmac.new(SPATIE.encode(),body,hashlib.sha256).hexdigest()
            else:
                data=f"{headers['webhook-id']}.{headers['webhook-timestamp']}.".encode()+body
                expected="v1,"+base64.b64encode(hmac.new(base64.b64decode(STANDARD[6:]),data,hashlib.sha256).digest()).decode()
                assert headers["webhook-signature"]==expected
                expired=headers|{"webhook-timestamp":str(int(headers["webhook-timestamp"])-301)}
                exchange(port,profile,body,expired,403)
            exchange(port,profile,body,headers,200)
            for name in (["signature"] if profile=="spatie" else ["webhook-id","webhook-signature","webhook-timestamp"]):
                duplicate_header(port,profile,body,headers,name)
        exchange(port,"spatie",b"unsigned",{},403)
        exchange(port,"unknown",b"no",{},404)
        exchange(port,"spatie",b"x"*(1024*1024+1),{},413)
    finally:
        stop(process)


def check_sender_builder(binary, env):
    def prepare(mode):
        return json.loads(artisan(binary, env, "webhooks:prepare-send", mode))

    secret = b"outgoing-fixture-credential"
    rendered = prepare("json")
    expected_json = br'{"z":"\u4e2d\/\ud83d\ude42","a":2}'
    assert bytes(rendered["body"]) == bytes(rendered["signed"]) == expected_json
    assert rendered["headers"]["signature"] == hmac.new(secret, expected_json, hashlib.sha256).hexdigest()
    assert rendered["headers"]["timestamp"] == "1700000000"
    assert rendered["headers"]["webhook-id"] == rendered["id"] == "outgoing-fixture"
    assert (rendered["tries"], rendered["timeout"], rendered["queue"], rendered["backoff"]) == (3, 3, "default", 100)
    assert rendered["method"] == "POST"
    get = prepare("get")
    assert get["method"] == "GET" and get["body"] == []
    assert get["url"] == "https://example.test/callback?label=a%20b&settings%5Benabled%5D=1"
    assert get["headers"]["signature"] == hmac.new(secret, bytes(get["signed"]), hashlib.sha256).hexdigest()
    raw = prepare("raw")
    assert bytes(raw["body"]) == b"\x00\xffraw\n"
    assert "webhook-id" not in raw["headers"] and "signature" not in raw["headers"]
    custom = prepare("custom")
    assert custom["headers"]["x-fixture-signature"] == "outgoing-fixture:1700000000:4"
    assert custom["backoff"] == 14
    blocked = subprocess.run([str(binary), "artisan", "webhooks:prepare-send", "blocked"],
                             env=env, capture_output=True, text=True, timeout=15)
    assert blocked.returncode != 0 and secret.decode() not in blocked.stdout + blocked.stderr
    for profile, field in (
        ("missing-url", "url"), ("missing-secret", "signing_secret"),
        ("zero-secret", "signing_secret"), ("bad-signer", "signer"),
        ("bad-backoff", "backoff_strategy"),
    ):
        result = subprocess.run([str(binary), "artisan", "webhooks:prepare-send", profile],
                                env=env, capture_output=True, text=True, timeout=15)
        output = result.stdout + result.stderr
        assert result.returncode != 0 and f"Webhook 发送配置无效：{field}" in output, output
        assert secret.decode() not in output and "private-body-sentinel" not in output


def main():
    binary=args.binary.resolve()
    with tempfile.TemporaryDirectory(dir=ROOT/"target/tmp",prefix="webhook-app-") as tmp:
        root=Path(tmp)
        with socket.socket() as listener:
            listener.bind(("127.0.0.1",0));port=listener.getsockname()[1]
        env=os.environ|{"APP_BASE_PATH":tmp,"APP_ENV":"testing","APP_HOST":"127.0.0.1","APP_PORT":str(port),
            "APP_DEBUG":"false","APP_DISABLED_RUNNERS":"queue,schedule","CACHE_STORE":"file","SESSION_DRIVER":"array",
            "APP_CONFIG_CACHE":str(root/"cached-config.json"),
            "APP_KEY":"base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","APP_SUPPORTED_LOCALES":"",
            "FIXTURE_SPATIE_SECRET":SPATIE,"FIXTURE_STANDARD_SECRET":STANDARD}
        with (root/"http.log").open("w+") as log:
            try:
                artisan(binary,env,"package:discover")
                artisan(binary,env,"vendor:publish","--tag=webhook-client-config","--tag=webhook-server-config")
                for name in ("webhook_client","webhook_server"):
                    path=root/"config"/(name+".rs")
                    assert path.exists(),path
                    path.write_text(path.read_text()+"\n// preserved customization\n")
                artisan(binary,env,"vendor:publish","--tag=webhook-client-config","--tag=webhook-server-config")
                for name in ("webhook_client","webhook_server"):
                    assert (root/"config"/(name+".rs")).read_text().endswith("// preserved customization\n")
                exercise(binary,env,log)
                check_sender_builder(binary,env)
                artisan(binary,env,"config:cache")
                wire=(root/"cached-config.json").read_text()
                assert SPATIE not in wire and STANDARD not in wire
                assert "FIXTURE_SPATIE_SECRET" in wire
                shown=artisan(binary,env,"config:show","webhook-client")
                assert SPATIE not in shown and STANDARD not in shown
                exercise(binary,env,log)
                check_sender_builder(binary,env)
                # 凭据是受控引用，不是固化到 config cache 中的明文；每个进程取自己的环境快照。
                changed=env|{"FIXTURE_SPATIE_SECRET":"rotated-private-key"}
                headers=json.loads(artisan(binary,changed,"webhooks:sign","spatie","rotation"))["headers"]
                assert headers["signature"]==hmac.new(b"rotated-private-key",b"rotation",hashlib.sha256).hexdigest()
                artisan(binary,env,"config:clear")
                print("包发现／重复发布保留定制／缓存无密钥／真实 HTTP＋Artisan 原始正文、双协议与发送快照通过")
            except BaseException:
                log.flush();print((root/"http.log").read_text());raise

if __name__=="__main__":main()
