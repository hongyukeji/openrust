"""腾讯 VM 标准点播：真实 HTTP、Artisan、Worker 与独立 TC3。"""

import argparse
import http.client
import http.server
import json
import os
from pathlib import Path
import runpy
import socket
import sqlite3
import subprocess
import tempfile
import threading
from urllib.parse import parse_qs, urlsplit
import uuid
import xml.etree.ElementTree as XML

ROOT = Path(__file__).resolve().parents[1]
TC3 = runpy.run_path(str(ROOT / "tests/moderation_tencent_text_acceptance.py"))
ALIYUN = runpy.run_path(str(ROOT / "tests/moderation_aliyun_video_acceptance.py"))
ORIGIN, KEY = ALIYUN["ORIGIN"], ALIYUN["KEY"]
PRIVATE = TC3["PRIVATE_MARKER"]
COMMAND = "drivers:moderation-tencent-video"
BIZ, TASK = "video_profile", "vm-review-result"


class Supplier(TC3["Supplier"]):
    def do_POST(self):
        action = self.headers["X-TC-Action"]
        assert action in {"CreateVideoModerationTask", "DescribeTaskDetail", "CancelTask"}
        body, identity = self.verify(action, "vm", "2021-09-22")
        assert identity == "fixture-tencent-id"
        if action == "CreateVideoModerationTask":
            if expected := getattr(self.server, "expected_callback", None):
                assert (body.pop("CallbackUrl"), body.pop("Seed")) == expected
            assert set(body) == {"BizType", "Type", "Tasks"} and body["BizType"] == BIZ and body["Type"] == "VIDEO"
            task, = body["Tasks"]
            assert set(task) == {"DataId", "Input"} and uuid.UUID(task["DataId"]).version == 4
            assert set(task["Input"]) == {"Type", "Url"} and task["Input"]["Type"] == "URL"
            source = urlsplit(task["Input"]["Url"])
            assert f"{source.scheme}://{source.netloc}" == ORIGIN and source.path.endswith("/" + KEY)
            query = parse_qs(source.query, strict_parsing=True)
            version, = query["versionId"]
            assert "X-Amz-Signature" in query and 0 < int(query["X-Amz-Expires"][0]) <= 300
            backend = self.server.storage
            connection = http.client.HTTPConnection(backend.hostname, backend.port, timeout=5)
            try:
                connection.request("GET", source.path + "?" + source.query, headers={"Host": backend.netloc})
                response = connection.getresponse()
                payload = response.read()
                assert response.status == 200
            finally:
                connection.close()
            with self.server.lock:
                data_id = task["DataId"]
                assert data_id not in self.server.references
                self.server.references.add(data_id)
                self.server.submissions.append((version, payload))
                mode = self.server.submit_mode
            if gate := getattr(self.server, "gate", None):
                reached, release = gate
                reached.set()
                assert release.wait(12), "回调夹具未释放受理凭据"
            result = {"DataId": data_id, "TaskId": TASK, "Code": "OK", "Message": PRIVATE}
            if mode == "wrong-reference":
                result["DataId"] = "different"
            elif mode == "quota":
                result["Code"] = "LimitExceeded"
            elif mode == "missing-id":
                result.pop("TaskId")
            value = {"Results": [result], "RequestId": "vm-submit"}
        elif action == "CancelTask":
            assert set(body) == {"TaskId"}
            task = body["TaskId"]
            assert task in {"pending", "pending-race"}
            with self.server.lock:
                self.server.cancellations.append(task)
                if task == "pending":
                    self.server.cancelled.add(task)
            if task == "pending-race":
                value = {"Error": {"Code": "FailedOperation.TaskHasBeenRunning", "Message": PRIVATE}}
            else:
                value = {"RequestId": "vm-cancel"}
        else:
            assert set(body) == {"TaskId", "ShowAllSegments"}
            task, show = body["TaskId"], body["ShowAllSegments"]
            assert isinstance(show, bool)
            with self.server.lock:
                self.server.queries.append((task, show))
                cancelled = task in self.server.cancelled
            status = {"pending": "PENDING", "running": "RUNNING", "pending-race": "PENDING",
                      "foreigner": "PENDING", "cancelled": "CANCELLED", "failed": "ERROR",
                      "unknown-state": "FUTURE"}.get(task, "FINISH")
            value = {"TaskId": task, "Type": "VIDEO", "BizType": BIZ,
                     "Status": "CANCELLED" if cancelled else status, "RequestId": "vm-details",
                     "Suggestion": "Pass", "Label": "Normal", "Labels": None}
            if task == TASK:
                value["ImageSegments"] = [{
                    "OffsetTime": "1.125", "Result": {
                        "Label": "Terror", "SubLabel": "Knife", "Suggestion": "Review", "Score": 88,
                        "Url": PRIVATE, "Results": [{
                            "Label": "Ad", "SubLabel": "Promotion", "Suggestion": "Review", "Score": 95,
                            "HitType": "text_rule", "Text": PRIVATE,
                            "Details": [{"Label": "Ad", "SubLabel": "Qr", "Suggestion": "Review", "Score": 98,
                                         "Text": PRIVATE, "OcrHitInfos": [{"Keyword": PRIVATE}]}],
                        }],
                    },
                }]
                value["AudioSegments"] = [{
                    "OffsetTime": "0.1", "Result": {
                        "Label": "Abuse", "SubLabel": "Insult", "Score": 93, "Suggestion": "Block", "Duration": "700",
                        "Text": PRIVATE, "Url": PRIVATE, "TextResults": [{"Label": "Ad", "Score": 89, "Suggestion": "Review"}],
                        "MoanResults": [{"Label": "Moan", "Score": 92, "Suggestion": "Review", "StartTime": 25, "EndTime": 500}],
                    },
                }]
                value["SegmentCosUrlList"] = {"ImageAllUrl": f"http://127.0.0.1:{self.server.server_port}/should-not-fetch"}
            elif task == "foreigner":
                value["BizType"] = "another_profile"
            elif task == "live":
                value["Type"] = "LIVE_VIDEO"
            elif task == "wrong-id":
                value["TaskId"] = "another-task"
            elif task == "bad-time":
                value["ImageSegments"] = [{"OffsetTime": "-1", "Result": {"Label": "Normal", "Suggestion": "Pass"}}]
            elif task == "quota":
                value = {"Error": {"Code": "RequestLimitExceeded", "Message": PRIVATE}}
        response = json.dumps({"Response": value}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(response)))
        self.end_headers()
        self.wfile.write(response)


def exercise(binary, directory, storage_env, storage):
    storage("PUT", query="versioning", content=(
        b'<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
        b'<Status>Enabled</Status></VersioningConfiguration>'))
    original = ALIYUN["generate"](directory, "original.mp4", "green", True)
    changed = ALIYUN["generate"](directory, "changed.mp4", "yellow", False)
    storage("PUT", KEY, content=original)
    storage("PUT", KEY, content=changed)
    result = XML.fromstring(storage("GET", query="versions&prefix=versions%2Fclip.mp4"))
    versions = {node.findtext("{*}IsLatest"): node.findtext("{*}VersionId")
                for node in result.findall("{*}Version") if node.findtext("{*}Key") == KEY}
    first, last = versions["false"], versions["true"]
    assert first != last and "null" not in {first, last}
    storage("GET", KEY, status=403, signed=False)

    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock = threading.Lock()
    supplier.references, supplier.cancelled = set(), set()
    supplier.submissions, supplier.queries, supplier.cancellations, supplier.unexpected = [], [], [], []
    supplier.submit_mode = "accepted"
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()

    identity = "fixture-tencent-id"
    secret, token = TC3["CREDENTIALS"][identity]
    database = directory / "queue.sqlite"
    database.touch()
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(directory), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(directory / "config.json"), "APP_ROUTES_CACHE": str(directory / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DB_CONNECTION": "sqlite", "DB_DATABASE": str(database), "QUEUE_CONNECTION": "database",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_TENCENT_VIDEO": f"http://127.0.0.1:{supplier.server_port}",
        "AWS_TEMPORARY_URL": ORIGIN, "MODERATION_FIXTURE_IMAGE_ORIGIN": ORIGIN,
        "FIXTURE_TENCENT_ID": identity, "FIXTURE_TENCENT_SECRET": secret, "FIXTURE_TENCENT_TOKEN": token,
    }
    env.pop("DB_URL", None)

    def artisan(current, *tokens, successful=True):
        process = subprocess.run([str(binary), "artisan", *tokens], env=current, capture_output=True,
                                 text=True, timeout=25)
        assert (process.returncode == 0) == successful, process.stdout + process.stderr
        assert PRIVATE not in process.stdout + process.stderr
        return process.stdout

    def post(expected=200, *, version=None, task=None, cancel=False, disk="s3"):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            headers = {"Accept": "application/json"}
            if version:
                headers.update({"X-Fixture-Version": version, "X-Fixture-Disk": disk})
            if task:
                headers["X-Fixture-Task"] = task
            if cancel:
                headers["X-Fixture-Cancel"] = "1"
            connection.request("POST", "/api/drivers/moderation/tencent-video", headers=headers)
            response = connection.getresponse()
            data = response.read()
            assert response.status == expected, (response.status, expected, data[:250])
            assert PRIVATE.encode() not in data
            return json.loads(data)
        finally:
            connection.close()

    success = {"task": TASK, "cancel": True}
    try:
        assert json.loads(artisan(env, COMMAND, "submit", first)) == success
        artisan(env | {"MODERATION_FIXTURE_TENCENT_BIZ": "bad-namespace"}, COMMAND, "submit", first, successful=False)
        artisan(env | {"AWS_TEMPORARY_URL": "https://wrong.fixture.invalid"}, COMMAND, "submit", last, successful=False)
        artisan(env, "migrate", "--force")
        artisan(env, "config:cache")
        artisan(env, "route:cache")
        cache = (directory / "config.json").read_text()
        assert secret not in cache and token not in cache and "FIXTURE_TENCENT_SECRET" in cache
        cached = env | {"MODERATION_FIXTURE_TENCENT_BIZ": "ignored_after_cache"}
        with (directory / "http.log").open("w+") as log:
            application = TC3["HTTP"]["serve"](binary, cached, log)
            try:
                assert post(version=last) == success
                assert post(400, version=first, disk="local")["error"].startswith("InvalidInput")
                assert post(400, version="null")["error"].startswith("InvalidInput")
                for mode, status, error in (("quota", 429, "Throttled"), ("wrong-reference", 502, "SubmissionUnknown"),
                                            ("missing-id", 502, "SubmissionUnknown")):
                    supplier.submit_mode = mode
                    assert post(status, version=last) == {"error": error}
                for task, state in (("pending", "Pending"), ("running", "Running"),
                                    ("cancelled", "Cancelled"), ("failed", "Failed")):
                    assert post(task=task) == {"status": state}
                for task in ("foreigner", "live", "wrong-id", "unknown-state", "bad-time"):
                    assert post(502, task=task) == {"error": "Protocol"}
                assert post(429, task="quota") == {"error": "Throttled"}
                assert post(task="clean")["assessment"]["decision"] is None
                assert post(400, task="running", cancel=True) == {"error": "Rejected"}
                assert post(502, task="foreigner", cancel=True) == {"error": "Protocol"}
                assert post(400, task="pending-race", cancel=True) == {"error": "Rejected"}
                assert post(task="pending", cancel=True) == {"cancelled": True}
                assert post(task="pending", cancel=True) == {"cancelled": True}
                assert post(task="pending") == {"status": "Cancelled"}
                checked = post(task=TASK)
                assessment = checked["assessment"]
                assert assessment["decision"] == "Block" and assessment["coverage"] == dict.fromkeys(
                    ["Visual", "Speech", "Ocr"], "Partial")
                evidence = {item["provider"]: item for item in assessment["labels"]}
                assert evidence["Terror/Knife"]["span_ms"] == [1125, 1125]
                assert evidence["Ad/Promotion"]["modality"] == "Ocr"
                assert evidence["Ad/Qr"]["confidence"] == {"value": 98.0, "maximum": 100.0}
                assert evidence["Abuse/Insult"]["span_ms"] == [100, 800]
                assert evidence["Moan"]["span_ms"] == [125, 600]
            finally:
                TC3["HTTP"]["stop"](application)
            log.seek(0)
            traces = log.read()
            assert all(value not in traces for value in (PRIVATE, secret, token, "X-Amz-Signature"))
        assert json.loads(artisan(cached, COMMAND, "query", TASK)) == checked
        assert json.loads(artisan(cached, COMMAND, "cancel", "pending")) == {"cancelled": True}
        assert json.loads(artisan(cached, COMMAND, "queue", TASK)) == {"queued": True}
        with sqlite3.connect(database) as conn:
            (payload,), = conn.execute("SELECT payload FROM jobs").fetchall()
            assert TASK in payload and KEY not in payload and secret not in payload
        path = directory / "worker-response.json"
        artisan(cached | {"MODERATION_FIXTURE_RESULT": str(path)}, "queue:work", "database", "--once")
        assert json.loads(path.read_text()) == checked
        with sqlite3.connect(database) as conn:
            assert conn.execute("SELECT COUNT(*) FROM jobs").fetchone() == (0,)
            assert conn.execute("SELECT COUNT(*) FROM failed_jobs").fetchone() == (0,)
        assert supplier.submissions == [(first, original)] + [(last, changed)] * 4
        assert supplier.cancellations == ["pending-race", "pending"] and not supplier.unexpected
        assert sum(task == TASK and all_segments for task, all_segments in supplier.queries) == 3
        assert sum(not all_segments for _, all_segments in supplier.queries) == 6
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-tencent-video-") as work, \
            ALIYUN["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, request):
        exercise(args.binary.resolve(), Path(work), env, request)
    print("腾讯云 VM 点播：HTTP／Artisan／Worker、待处理取消、TC3、视频时间和覆盖通过")
