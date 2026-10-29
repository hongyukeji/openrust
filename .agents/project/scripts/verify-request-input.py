#!/usr/bin/env python3
"""经真实 loopback TCP 核对示例 Request B2 公开路由；不负责启动服务。"""

import argparse
import http.client
import json
import sys
from datetime import datetime
from pathlib import Path
from urllib.parse import urlsplit


PATH = "/api/example/input"
QUERY = "?title=query&nullable=from-query&fallback=%20query-only%20&page=7&label=%20query%20"
MARKER = "INPUT-ERROR-MARKER-73f4c2"
TIMEOUT_SECONDS = 5
MAX_RESPONSE_BYTES = 256 * 1024
MAX_REQUEST_BYTES = 16 * 1024
MAX_CASES = 6


def parse_base_url(raw):
    parsed = urlsplit(raw)
    if (
        parsed.scheme != "http"
        or parsed.hostname not in {"localhost", "127.0.0.1", "::1"}
        or parsed.username is not None
        or parsed.password is not None
        or parsed.path not in {"", "/"}
        or parsed.query
        or parsed.fragment
    ):
        raise ValueError("--base-url 必须是带显式端口的 loopback HTTP origin")
    try:
        port = parsed.port
    except ValueError as error:
        raise ValueError("--base-url 端口无效") from error
    if port is None or not 1 <= port <= 65535:
        raise ValueError("--base-url 必须提供有效端口")
    # 不解析 localhost 的 DNS；连接始终指向数字 loopback 地址。
    address = "::1" if parsed.hostname == "::1" else "127.0.0.1"
    host = f"[{parsed.hostname}]" if parsed.hostname == "::1" else parsed.hostname
    return address, port, f"{host}:{port}"


def parse_output(raw):
    path = Path(raw).expanduser().resolve()
    if not path.is_relative_to(Path("/tmp").resolve()) or path == Path("/tmp").resolve():
        raise ValueError("--output 必须是 /tmp 内的文件路径")
    if not path.parent.is_dir():
        raise ValueError("--output 的父目录必须已存在")
    if path.exists():
        raise ValueError("证据文件已存在，请为本次运行指定新路径")
    return path


def post_json(address, port, host, path, payload):
    body = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    if len(body) > MAX_REQUEST_BYTES:
        raise ValueError("请求超过本工具的 16 KiB 上限")
    connection = http.client.HTTPConnection(address, port, timeout=TIMEOUT_SECONDS)
    try:
        connection.request(
            "POST",
            path,
            body=body,
            headers={"Host": host, "Content-Type": "application/json"},
        )
        response = connection.getresponse()
        raw = response.read(MAX_RESPONSE_BYTES + 1)
        if len(raw) > MAX_RESPONSE_BYTES:
            raise ValueError("响应超过本工具的 256 KiB 上限")
        if response.status in {301, 302, 303, 307, 308}:
            raise ValueError("验收路由发生重定向")
        document = json.loads(raw)
        return response.status, document, response.getheader("x-request-id"), len(body), raw
    finally:
        connection.close()


def expect_success(status, document, data):
    if status != 200:
        raise AssertionError(f"状态码 {status}，预期 200")
    if not isinstance(document, dict) or set(document) != {"success", "code", "message", "data"}:
        raise AssertionError("成功响应封装字段不匹配")
    if document["success"] is not True or document["code"] != "ok" or document["message"] != "ok":
        raise AssertionError("成功响应封装语义不匹配")
    if document["data"] != data:
        raise AssertionError("成功响应业务数据不匹配")


def expect_error(status, document, request_id, raw, expected_status, expected_code):
    if status != expected_status:
        raise AssertionError(f"状态码 {status}，预期 {expected_status}")
    if not isinstance(document, dict) or set(document) != {
        "success", "code", "message", "timestamp", "request_id"
    }:
        raise AssertionError("错误响应封装字段不匹配")
    if document["success"] is not False or document["code"] != expected_code:
        raise AssertionError("错误响应封装语义不匹配")
    if not isinstance(document["message"], str) or not document["message"]:
        raise AssertionError("错误消息缺失")
    if not isinstance(document["request_id"], str) or not document["request_id"]:
        raise AssertionError("错误请求 ID 缺失")
    if request_id != document["request_id"]:
        raise AssertionError("错误请求 ID 与响应头不一致")
    try:
        datetime.fromisoformat(document["timestamp"].replace("Z", "+00:00"))
    except (AttributeError, ValueError) as error:
        raise AssertionError("错误时间戳无效") from error
    # 同时检查解码后的值，避免 JSON Unicode escape 绕过原始字节检查。
    decoded = json.dumps(document, ensure_ascii=False)
    if MARKER.encode("ascii") in raw or MARKER in decoded:
        raise AssertionError("错误响应泄漏合成输入 marker")


def normal_case():
    payload = {
        "title": "  body title  ",
        "profile": {"name": "  Alice  "},
        "nullable": "  ",
        "password": "  private-sentinel  ",
        "user": {"password": "  nested-sentinel  "},
        "password_confirmation": "",
        "typed": {
            "title": "  demo  ",
            "quantity": "12",
            "amount": "1.25x",
            "subscribed": "YES",
            "publication": "published",
            "publications": {"2": "draft", "5": "unknown", "9": "published"},
            "items": {"2": "a", "5": "b"},
            "profile": {"display_name": " Alice "},
            "note": "0",
            "published_at": "2020-01-01T16:30:25+00:00",
            "calendar_date": "2020-01-02",
            "duration": "90 minutes",
            "delay": 75,
        },
    }
    encoded = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    data = {
        "method": "POST",
        "selected": {"title": "body title", "profile": {"name": "Alice"}},
        "typed": {
            "title": "demo", "quantity": 12, "amount": 1.25, "subscribed": True,
            "bounded_quantity": 10, "publication": "published",
            "publications": {"2": "draft", "9": "published"},
            "items": {"2": "a", "5": "b"},
            "selected": {"typed": {"title": "demo", "quantity": "12"}},
            "profile_display_name": "Alice", "note_produced_value": False,
            "published_at": "2020-01-01T16:30:25+00:00",
            "calendar_date": "2020-01-02T00:00:00+00:00",
            "duration": {
                "years": 0, "months": 0, "days": 0, "hours": 0, "minutes": 90,
                "seconds": 0, "microseconds": 0, "invert": False, "total_days": None,
            },
            "delay_minutes": 75,
        },
        "title": "body title", "profile_name": "Alice",
        "checks": {
            "has_nullable": True, "nullable_filled": False, "has_any": True,
            "missing": True, "is_not_filled": True, "any_filled": True,
            "password_has_outer_spaces": True, "nested_password_has_outer_spaces": False,
            "empty_password_is_null": True,
        },
        "query_title": "query", "query_page": 7, "query_label": "query",
        "form_title": None, "json_title": "body title",
        "excluded_title": True, "source_has_title": True,
        "merged": {"title": "  revised  ", "profile": {"role": "reader"}, "generated": "fallback"},
        "replaced_title": "replacement", "query_fallback": "query-only",
        "content_bytes": len(encoded), "content_preserved": True,
    }
    return payload, data


def default_case():
    payload = {"typed": {"title": None, "quantity": None, "amount": None, "subscribed": None}}
    expected = {
        "title": "", "quantity": 0, "amount": 0.0, "subscribed": False,
        "bounded_quantity": 1, "publication": None, "publications": [],
        "items": [], "selected": {"typed": {"title": None, "quantity": None}},
        "profile_display_name": "", "note_produced_value": False,
        "published_at": None, "calendar_date": None, "duration": None,
        "delay_minutes": None,
    }
    return payload, expected


def run(base, output):
    address, port, host = base
    normal_payload, expected_data = normal_case()
    default_payload, default_typed = default_case()
    error_payloads = [
        ("enum_container", {"typed": {"title": MARKER, "publication": ["draft"]}}, 400, "input_enum_type_invalid"),
        ("invalid_date", {"typed": {"title": MARKER, "calendar_date": "invalid-date"}}, 400, "input_date_format_invalid"),
        ("invalid_interval", {"typed": {"title": MARKER, "duration": "of 2 days"}}, 400, "input_interval_invalid"),
        ("interval_limit", {"typed": {"title": MARKER, "duration": "0d" * 1025}}, 413, "input_limits_exceeded"),
    ]
    cases = [("normal", normal_payload, QUERY), ("default_null", default_payload, "")]
    cases.extend((name, payload, "") for name, payload, _, _ in error_payloads)
    if len(cases) != MAX_CASES:
        raise AssertionError("验收用例数量漂移")
    results = []
    for name, payload, query in cases:
        status = None
        try:
            status, document, request_id, body_bytes, raw = post_json(address, port, host, PATH + query, payload)
            if name == "normal":
                expect_success(status, document, expected_data)
                if b"private-sentinel" in raw:
                    raise AssertionError("成功响应泄漏凭据合成值")
            elif name == "default_null":
                if status != 200 or not isinstance(document, dict) or set(document) != {"success", "code", "message", "data"}:
                    raise AssertionError("默认值响应封装不匹配")
                if document["success"] is not True or document["code"] != "ok" or document["message"] != "ok":
                    raise AssertionError("默认值响应语义不匹配")
                if not isinstance(document["data"], dict) or document["data"].get("typed") != default_typed:
                    raise AssertionError("默认值与 null 转换不匹配")
            else:
                _, _, expected_status, expected_code = next(item for item in error_payloads if item[0] == name)
                expect_error(status, document, request_id, raw, expected_status, expected_code)
            results.append({"case": name, "ok": True, "status": status, "request_bytes": body_bytes})
        except (AssertionError, http.client.HTTPException, OSError, ValueError, UnicodeError) as error:
            # 仅记录自有断言文案；系统与解析器错误可能包含请求上下文。
            reason = str(error) if isinstance(error, AssertionError) else type(error).__name__
            results.append({"case": name, "ok": False, "status": status, "reason": reason[:120]})
    result = {"route": PATH, "transport": "tcp", "cases": results, "passed": sum(item["ok"] for item in results), "total": len(results)}
    with output.open("x", encoding="utf-8") as evidence:
        evidence.write(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"passed": result["passed"], "total": result["total"], "output": str(output)}, ensure_ascii=False))
    return 0 if result["passed"] == result["total"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", required=True, help="显式端口的 http://localhost、127.0.0.1 或 [::1]")
    parser.add_argument("--output", required=True, help="/tmp 内已存在父目录中的 JSON 证据文件")
    arguments = parser.parse_args()
    try:
        base = parse_base_url(arguments.base_url)
        output = parse_output(arguments.output)
        return run(base, output)
    except (OSError, ValueError) as error:
        print(f"验收工具参数或输出错误：{error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
