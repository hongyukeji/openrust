"""Sightengine 短视频语音以及审核 Worker 一次请求完成的真实协议验收。"""

import argparse
from concurrent.futures import ThreadPoolExecutor
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
import xml.etree.ElementTree as XML


ROOT = Path(__file__).resolve().parents[1]
SUBMISSION = runpy.run_path(str(ROOT / "tests/moderation_submission_acceptance.py"))
VIDEO, HTTP, DATABASE, COMMAND = (SUBMISSION[key] for key in ("VIDEO", "HTTP", "DATABASE", "COMMAND"))
PRIVATE_MATCH = "fixture-only-transcribed-private-match"
PRIMARY = "sightengine-speech"


class Supplier(VIDEO["Supplier"]):
    def do_POST(self):
        assert self.path == "/1.0/video/check-sync.json"
        fields = self.form()
        self.authorization(fields)
        source = urlsplit(fields.pop("stream_url")[0])
        assert fields == {"models": ["audio-moderation"], "audio_categories": ["insult,phone_number"]}
        assert f"{source.scheme}://{source.netloc}" == VIDEO["ORIGIN"]
        assert source.path.endswith("/" + VIDEO["KEY"])
        query = parse_qs(source.query, strict_parsing=True)
        version, = query["versionId"]
        assert "X-Amz-Signature" in query and 0 < int(query["X-Amz-Expires"][0]) <= 300
        backend = self.server.storage
        connection = http.client.HTTPConnection(backend.hostname, backend.port, timeout=5)
        try:
            connection.request("GET", source.path + "?" + source.query, headers={"Host": backend.netloc})
            response = connection.getresponse()
            clip = response.read()
            assert response.status == 200
        finally:
            connection.close()
        with self.server.lock:
            self.server.posted.append((version, clip))
            number = len(self.server.posted)
            mode = self.server.mode
        if gate := self.server.gate:
            entered, release = gate
            entered.set()
            assert release.wait(12)

        data = {}
        if mode != "missing-track":
            audio = {"detected_categories": [], "detections": {}}
            data["audio"] = audio
            if mode in ("findings", "bad-time", "duplicate"):
                categories = ["insult", "phone_number"]
                audio["detected_categories"] = categories + (["insult"] if mode == "duplicate" else [])
                audio["detections"] = {
                    "insult": {
                        "severity": "high",
                        "details": [{"severity": "high", "start_ms": 150, "end_ms": 400,
                                     "match": PRIVATE_MATCH}],
                    },
                    "phone_number": {
                        "details": [{"start_ms": 650, "end_ms": 9000 if mode == "bad-time" else 980,
                                     "match": PRIVATE_MATCH, "country": "us"}],
                    },
                }
        body = {
            "status": "success", "request": {"id": f"req_speech_{number}"},
            "media": {"id": f"med_speech{number}", "uri": PRIVATE_MATCH}, "data": data,
        }
        self.respond(200, body)

    def do_GET(self):
        self.server.queried.append(self.path)
        self.respond(405, {"status": "failure"})


def probe_ms(path):
    result = subprocess.check_output(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration",
         "-of", "default=noprint_wrappers=1:nokey=1", str(path)], timeout=10,
    )
    return round(float(result) * 1000)


def audio_track_count(path):
    result = subprocess.check_output(
        ["ffprobe", "-v", "error", "-select_streams", "a", "-show_entries", "stream=index",
         "-of", "json", str(path)], timeout=10,
    )
    return len(json.loads(result)["streams"])


def upload_version(path, storage):
    body = path.read_bytes()
    storage("PUT", VIDEO["KEY"], content=body)
    versions = XML.fromstring(storage("GET", query="versions="))
    version, = [
        node.findtext("{*}VersionId") for node in versions.findall("{*}Version")
        if node.findtext("{*}Key") == VIDEO["KEY"] and node.findtext("{*}IsLatest") == "true"
    ]
    assert version not in {"", "null"}
    return version, body


def versioned_segments(root, storage):
    storage("PUT", query="versioning", content=(
        b'<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
        b'<Status>Enabled</Status></VersioningConfiguration>'))
    source = root / "long.mp4"
    common = ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-threads", "1"]
    codecs = ["-c:v", "mpeg4", "-c:a", "aac"]
    subprocess.run(common + [
        "-f", "lavfi", "-i", "color=navy:s=64x64:r=8:d=70",
        "-f", "lavfi", "-i", "sine=frequency=220:duration=70", "-shortest",
        *codecs, str(source),
    ], check=True, timeout=30)
    assert probe_ms(source) == 70_000
    frames = [source]
    for name, start, duration in (("head", 0, 40), ("tail", 33, 37)):
        path = root / f"{name}.mp4"
        subprocess.run(common + [
            "-ss", str(start), "-i", str(source), "-t", str(duration), *codecs, str(path),
        ], check=True, timeout=30)
        frames.append(path)
    lengths = [probe_ms(path) for path in frames[1:]]
    assert 33_000 < lengths[0] < 60_000 and 33_000 + lengths[1] == probe_ms(source), lengths

    tracks = [audio_track_count(path) for path in frames]
    assert tracks == [1, 1, 1]
    original_version, original = upload_version(frames[0], storage)
    first, second = (upload_version(path, storage) for path in frames[1:])
    assert len({original_version, first[0], second[0]}) == 3
    storage("GET", VIDEO["KEY"], status=403, signed=False)
    return original_version, original, (first, second), lengths, tracks


def exercise(binary, driver, url, root, storage_env, storage, *, test_segments=False):
    if test_segments:
        version, original, clips, lengths, tracks = versioned_segments(root, storage)
        duration = 70_000
        timeline = [
            {"start_ms": 0, "duration_ms": lengths[0], "version": clips[0][0], "audio_track_count": tracks[1]},
            {"start_ms": 33_000, "duration_ms": lengths[1], "version": clips[1][0], "audio_track_count": tracks[2]},
        ]
    else:
        version, silent_version, original, silent = VIDEO["versioned_clips"](root, storage)
        source = root / "video-with-audio.mp4"
        silent_source = root / "video-visual-only.mp4"
        duration = probe_ms(source)
        silent_duration = probe_ms(silent_source)
        assert audio_track_count(source) == 1 and audio_track_count(silent_source) == 0
        assert 980 <= duration < 60_000 and 0 < silent_duration < 60_000
        multi_path = root / "two-audio-tracks.mp4"
        subprocess.run(["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error",
                        "-i", str(source), "-f", "lavfi", "-i", "sine=frequency=660:duration=2",
                        "-map", "0:v", "-map", "0:a", "-map", "1:a", "-c:v", "copy", "-c:a", "aac",
                        str(multi_path)], timeout=10, check=True)
        multi_count = audio_track_count(multi_path)
        assert multi_count == 2
        multi_version, multi_body = upload_version(multi_path, storage)
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.mode = threading.Lock(), "clean"
    supplier.expected_secret, supplier.gate = VIDEO["SECRET"], None
    supplier.posted, supplier.queried = [], []
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    serving = threading.Thread(target=supplier.serve_forever, daemon=True)
    serving.start()

    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    db = root / "speech.sqlite"
    if driver == "sqlite":
        db.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(db)
    mount = root / "credentials"
    mount.mkdir(mode=0o700)
    credential = os.open(mount / "sightengine.r1.json", os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o400)
    with os.fdopen(credential, "w") as stream:
        json.dump({"api_user": "local-sightengine-account", "api_secret": VIDEO["SECRET"]}, stream)
    endpoint = f"http://127.0.0.1:{supplier.server_port}"
    outcome = root / "worker.json"
    event_log = root / "request-events.txt"
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": "database",
        "DB_DATABASE": str(db) if driver == "sqlite" else "openrust_acceptance",
        "MODERATION_CREDENTIAL_DIRECTORY": str(mount), "MODERATION_FIXTURE_PIN_ENDPOINT": endpoint,
        "MODERATION_FIXTURE_SIGHTENGINE_VIDEO": endpoint, "MODERATION_FIXTURE_SIGHTENGINE_VIDEO_TIMEOUT": "18000",
        "AWS_TEMPORARY_URL": VIDEO["ORIGIN"], "MODERATION_FIXTURE_IMAGE_ORIGIN": VIDEO["ORIGIN"],
        "MODERATION_FIXTURE_RESULT": str(outcome), "MODERATION_FIXTURE_SPEECH": "1",
        "MODERATION_EVENT_OBSERVATIONS": str(event_log),
        "MODERATION_FIXTURE_GROUP": "1", "MODERATION_FIXTURE_DURABLE_VIDEO_PROFILE": PRIMARY,
    }
    env.pop("MODERATION_FIXTURE_VIDEO_DURATION_MS", None)
    env.pop("MODERATION_FIXTURE_VIDEO_AUDIO_TRACK_COUNT", None)
    if test_segments:
        env["MODERATION_FIXTURE_VIDEO_DURATION_MS"] = str(duration)
        env["MODERATION_FIXTURE_VIDEO_AUDIO_TRACK_COUNT"] = str(tracks[0])
        env["MODERATION_FIXTURE_SEGMENT_PLAN"] = json.dumps(timeline)
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)

    def artisan(*tokens, extra=None, successful=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=env | (extra or {}),
                                capture_output=True, text=True, timeout=35)
        output = result.stdout + result.stderr
        assert PRIVATE_MATCH not in output and VIDEO["SECRET"] not in output
        assert (result.returncode == 0) == successful, output
        return result.stdout

    def stage(intent, clip=version, ms=duration, profile=PRIMARY, successful=True, tracks=1):
        extra = {"MODERATION_FIXTURE_DURABLE_VIDEO_PROFILE": profile}
        if ms is not None:
            extra["MODERATION_FIXTURE_VIDEO_DURATION_MS"] = str(ms)
        if tracks is not None:
            extra["MODERATION_FIXTURE_VIDEO_AUDIO_TRACK_COUNT"] = str(tracks)
        result = artisan(COMMAND, "stage", "app", "tenant", intent, clip, extra=extra, successful=successful)
        return json.loads(result) if successful else None

    def command(action, review):
        return json.loads(artisan(COMMAND, action, "app", "tenant", review["id"]))

    def worker(action, review):
        assert command(f"queue-{action}", review) == {"queued": True}
        payload = sql("SELECT payload FROM jobs")
        assert review["id"] in payload and all(value not in payload for value in (version, VIDEO["SECRET"], PRIVATE_MATCH))
        artisan("queue:work", "database", "--once")
        return json.loads(outcome.read_text())

    def sql(query):
        if driver == "sqlite":
            with sqlite3.connect(db) as connection:
                return "\n".join("\t".join(map(str, row)) for row in connection.execute(query))
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def submit_http(review):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=20)
        try:
            connection.request("POST", "/api/drivers/moderation/sightengine-video", headers={
                "Accept": "application/json", "X-Fixture-Submit-Review": review["id"],
            })
            response = connection.getresponse()
            body = response.read()
            assert response.status == 200 and all(value.encode() not in body for value in (VIDEO["SECRET"], PRIVATE_MATCH))
            return json.loads(body)
        finally:
            connection.close()

    def group_http(expected=200, **values):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=20)
        try:
            headers = {"Accept": "application/json"} | {
                "X-Fixture-" + name.replace("_", "-").title(): str(value) for name, value in values.items()
            }
            connection.request("POST", "/api/drivers/moderation/group", headers=headers)
            response = connection.getresponse()
            body = response.read()
            assert response.status == expected and all(value.encode() not in body for value in (VIDEO["SECRET"], PRIVATE_MATCH))
            return json.loads(body)
        finally:
            connection.close()

    def exercise_segments():
        def stage_group(intent, layout=timeline, total=duration, profile=PRIMARY, successful=True):
            result = artisan(
                COMMAND, "stage-segments", "app", "tenant", intent, version, profile,
                extra={"MODERATION_FIXTURE_SEGMENT_PLAN": json.dumps(layout),
                       "MODERATION_FIXTURE_VIDEO_DURATION_MS": str(total)},
                successful=successful,
            )
            return json.loads(result) if successful else None

        for bad in (
            [timeline[0], timeline[1] | {"start_ms": 41_000, "duration_ms": 29_000}],
            [timeline[0], timeline[1] | {"version": timeline[0]["version"]}],
            [timeline[0] | {"duration_ms": 60_000}, timeline[1]],
            [timeline[0], timeline[1] | {"duration_ms": 36_999}],
            [timeline[0], timeline[1] | {"start_ms": 0}],
        ):
            stage_group("bad-plan", bad, successful=False)
        stage_group("bad-profile", profile="sightengine-speech-visual", successful=False)
        assert sql("SELECT COUNT(*) FROM moderation_review_groups") == "0" and not supplier.posted

        stage_input = {"segment_plan": 1, "review_intent": "segmented", "version": version, "profiles": PRIMARY}
        group = group_http(**stage_input)
        assert group == stage_group("segmented")
        assert group["original_version"] == version and group["original_duration_ms"] == duration
        assert group["segment_windows_ms"] == [[0, lengths[0]], [33_000, 70_000]]
        plan_query = f"WHERE id = '{group['id']}'"
        encoded = sql("SELECT segment_plan FROM moderation_review_groups " + plan_query)
        sql("UPDATE moderation_review_groups SET segment_plan = '{}' " + plan_query)
        assert group_http(group_id=group["id"], expected=502)["error"] == "ReviewStorage"
        escaped = encoded.replace("'", "''")
        sql(f"UPDATE moderation_review_groups SET segment_plan = '{escaped}' " + plan_query)
        assert group_http(group_id=group["id"]) == group
        changed = [timeline[0] | {"duration_ms": lengths[0] + 1}, timeline[1]]
        stage_group("segmented", changed, successful=False)
        assert group_http(group_id=group["id"], tenant="Tenant", expected=404)["error"] == "ReviewNotFound"

        first, second = group["checks"]
        assert [first["object_version"], second["object_version"]] == [clips[0][0], clips[1][0]]
        assert worker("submit", first)["decision"] == "allow"
        assert group_http(group_id=group["id"])["decision"] is None
        assert not group_http(group_id=group["id"], operation="publish",
                              content_revision="content-v1", decision_revision=0, expected=403)["permitted"]
        supplier.mode = "findings"
        assert submit_http(second)["decision"] == "review"
        final = group_http(group_id=group["id"])
        assert (final["state"], final["decision"]) == ("Succeeded", "review")
        speech_end = next(item["span_ms"][1] for item in final["checks"][1]["assessment"]["labels"]
                          if item["provider"] == "phone_number")
        assert final["segment_windows_ms"][1][0] + speech_end == 33_980
        assert final == command("find-group", group)
        decision = group_http(group_id=group["id"], operation="record",
                              decision="allow", decision_revision=0, actor="moderator-test")
        assert decision["revision"] == 1
        assert group_http(group_id=group["id"], operation="publish",
                          content_revision="content-v1", decision_revision=1)["permitted"]
        # 旧分段计划即使所有子任务已有结论，也缺少原素材音轨证据。
        for track_count in (None, 2):
            plan = json.loads(encoded)
            if track_count is None:
                plan.pop("audio_track_count")
            else:
                plan["audio_track_count"] = track_count
            altered = json.dumps(plan).replace("'", "''")
            sql(f"UPDATE moderation_review_groups SET segment_plan = '{altered}' " + plan_query)
            assert group_http(group_id=group["id"])["decision"] is None
            group_http(group_id=group["id"], operation="publish",
                       content_revision="content-v1", decision_revision=1, expected=403)
        sql(f"UPDATE moderation_review_groups SET segment_plan = '{escaped}' " + plan_query)
        assert group_http(group_id=group["id"]) == final

        no_audio = stage_group("incomplete-segments")
        supplier.mode = "clean"
        assert worker("submit", no_audio["checks"][0])["decision"] == "allow"
        supplier.mode = "missing-track"
        assert worker("submit", no_audio["checks"][1])["state"] == "Failed"
        assert group_http(group_id=no_audio["id"])["decision"] is None

        many = [{"start_ms": number * 1000, "duration_ms": 1000, "version": f"stage-only-{number}"}
                for number in range(65)]
        maximum = stage_group("bounded-plan", many[:64], 64_000)
        assert len(maximum["checks"]) == 64 and maximum["segment_windows_ms"][-1] == [63_000, 64_000]
        stage_group("too-many-segments", many, 65_000, successful=False)
        assert sql("SELECT COUNT(*) FROM moderation_review_groups") == "3"
        assert sql("SELECT COUNT(*) FROM moderation_checks") == "68"
        assert supplier.posted == list(clips) * 2 and not supplier.queried
        assert event_log.read_text().splitlines() == ["POST"] * 4

    def exercise_short():
        for intent, ms in (("missing-duration", None), ("at-limit", 60_000)):
            failed = submit_http(stage(intent, ms=ms))
            assert failed["state"] == "Failed" and failed["submission"] == {"state": "Failed", "reason": "input"}
            assert failed["decision"] is None
        assert stage("zero-duration", ms=0, successful=False) is None and not supplier.posted

        clean = stage("short-clean")
        assert stage("short-clean") == clean
        stage("short-clean", ms=duration + 1, successful=False)
        assert sql("SELECT video_duration_ms FROM moderation_reviews "
                   f"WHERE id = '{clean['id']}'") == str(duration)
        inspected = submit_http(clean)
        assert inspected["state"] == "Succeeded" and inspected["decision"] == "allow"
        assert inspected["query_attempts"] == 0 and inspected["next_query_at"] is None
        assert inspected["assessment"]["coverage"] == {"Speech": "Complete", "Visual": "Unavailable", "Ocr": "Unavailable"}
        assert command("find", clean) == command("query", clean) == inspected
        assert submit_http(clean) == inspected and len(supplier.posted) == 1

        supplier.mode = "findings"
        flagged = worker("submit", stage("short-findings"))
        assert flagged["state"] == "Succeeded" and flagged["decision"] == "review"
        assert {item["provider"]: item["span_ms"] for item in flagged["assessment"]["labels"]} == {
            "insult/high": [150, 400], "phone_number": [650, 980],
        }
        assert flagged["assessment"]["labels"][0]["modality"] == "Speech"

        supplier.mode = "missing-track"
        missing = worker("submit", stage("silent-video", clip=silent_version, ms=silent_duration, tracks=0))
        assert missing["state"] == "Failed" and missing["query_failure"] == "incomplete_coverage"
        assert missing["decision"] is None and missing["assessment"]["coverage"]["Speech"] == "Unavailable"

        for bad in ("duplicate", "bad-time"):
            supplier.mode = bad
            invalid = submit_http(stage(f"short-{bad}"))
            assert invalid["state"] == "Running" and invalid["submission"] == {"state": "Unknown"}
            assert invalid["decision"] is None and invalid["assessment"] is None

        supplier.mode = "clean"
        wider = worker("submit", stage("short-also-visual", profile="sightengine-speech-visual"))
        assert wider["state"] == "Failed" and wider["query_failure"] == "incomplete_coverage"
        assert wider["decision"] is None and wider["assessment"]["coverage"]["Speech"] == "Complete"

        group = json.loads(artisan(COMMAND, "stage-group", "app", "tenant", "with-speech",
                                   version, f"group-visual,{PRIMARY}",
                                   extra={"MODERATION_FIXTURE_VIDEO_DURATION_MS": str(duration),
                                          "MODERATION_FIXTURE_VIDEO_AUDIO_TRACK_COUNT": "1"}))
        visual, speech = group["checks"]
        assert worker("submit", speech)["state"] == "Succeeded"
        assert command("find-group", group)["decision"] is None
        assert worker("submit", visual)["state"] == "Running"
        sql(f"UPDATE moderation_checks SET available_at = 1 WHERE id = '{visual['check_id']}'")
        assert worker("query", visual)["state"] == "Succeeded"
        assert command("find-group", group)["decision"] == "allow"

        late = stage("short-late")
        with ThreadPoolExecutor(max_workers=1) as pool:
            arrived, release = supplier.gate = (threading.Event(), threading.Event())
            sending = pool.submit(submit_http, late)
            try:
                assert arrived.wait(8)
                cancelled = command("cancel", late)
                assert cancelled["state"] == "Cancelled" and cancelled["provider_cancellation"] == "task_unknown"
            finally:
                release.set()
            assert sending.result(timeout=12) == cancelled
            supplier.gate = None
        assert cancelled["decision"] is None and cancelled["assessment"] is None
        assert supplier.posted == [(version, original)] * 2 + [(silent_version, silent)] + [(version, original)] * 5
        assert not supplier.queried and event_log.read_text().splitlines() == ["POST"] * 8
        assert sql("SELECT COUNT(*) FROM moderation_remote_leases") == "3"
        assert PRIVATE_MATCH not in sql("SELECT assessment FROM moderation_checks WHERE assessment IS NOT NULL")
        assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"

        supplier.mode = "clean"
        multiple = stage("short-two-tracks", clip=multi_version, ms=probe_ms(multi_path), tracks=multi_count)
        assert stage("short-two-tracks", clip=multi_version, ms=probe_ms(multi_path),
                     tracks=1, successful=False) is None
        assert sql("SELECT video_audio_track_count FROM moderation_reviews "
                   f"WHERE id = '{multiple['id']}'") == "2"
        for result in (submit_http(multiple), worker("submit", stage("short-unknown-tracks", tracks=None))):
            assert result["state"] == "Failed" and result["query_failure"] == "incomplete_coverage"
            assert result["decision"] is None and result["assessment"]["coverage"]["Speech"] == "Partial"
        assert supplier.posted[-2:] == [(multi_version, multi_body), (version, original)]
        supplier.mode = "findings"
        partial_findings = worker("submit", stage("short-two-tracks-findings", clip=multi_version,
                                                  ms=probe_ms(multi_path), tracks=multi_count))
        assert partial_findings["decision"] is None and partial_findings["assessment"]["coverage"]["Speech"] == "Partial"
        assert {item["provider"] for item in partial_findings["assessment"]["labels"]} == {"insult/high", "phone_number"}
        sql(f"UPDATE moderation_reviews SET video_audio_track_count = NULL WHERE id = '{clean['id']}'")
        old = command("find", clean)
        assert old["state"] == "Succeeded" and old["decision"] is None
        assert old["assessment"]["coverage"]["Speech"] == "Partial"

    try:
        artisan("migrate", "--force")
        artisan("config:cache")
        artisan("route:cache")
        with (root / "http.log").open("w+") as log:
            app = HTTP["serve"](binary, env, log)
            try:
                if test_segments:
                    exercise_segments()
                else:
                    exercise_short()
            finally:
                HTTP["stop"](app)
            log.seek(0)
            messages = log.read()
            assert PRIVATE_MATCH not in messages and VIDEO["SECRET"] not in messages
        migration = (
            "2026_11_26_000016_add_moderation_video_segment_plan.rs" if test_segments
            else "2026_11_26_000014_add_moderation_video_duration.rs"
        )
        artisan("migrate:rollback", "--force",
                "--path=vendor/openrust/content-moderation/database/migrations/" + migration,
                successful=False)
        artisan("migrate:rollback", "--force",
                "--path=vendor/openrust/content-moderation/database/migrations/"
                "2026_11_26_000019_add_moderation_video_audio_track_count.rs", successful=False)
    finally:
        supplier.shutdown()
        supplier.server_close()
        serving.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    test_segments = os.environ.get("MODERATION_TEST_SPEECH_SEGMENTS") == "1"
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-speech-") as root, \
            VIDEO["VIDEO"]["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, storage):
        exercise(args.binary.resolve(), driver, url, Path(root), env, storage, test_segments=test_segments)
    if test_segments:
        print(f"{driver}: Sightengine 长视频片段、私有版本、完整时间线和发布门禁通过 HTTP、Artisan 和 Worker")
    else:
        print(f"{driver}: Sightengine 短视频语音、限长、私有版本、即时终态及复合门禁通过 HTTP、Artisan 和 Worker")
