#!/usr/bin/env python3
"""检查整计划 Goal 与 Paseo 总控，恢复异常或遗漏的接续，不参与框架运行。"""

import argparse
from collections import deque
from datetime import datetime
import fcntl
import json
import os
from pathlib import Path
import random
import select
import sqlite3
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
DELAYS = (60, 180, 600)
PROMPT = (
    "用户授权的整计划守护发现异常或空闲后未接续。先核对AGENTS.md、Git/WIP、"
    "todo/plan、原Goal及在途子代理，收取结果后沿用原目标按依赖继续。"
    "不得重复派发、重复已通过门禁或把单簇完成当作整计划完成。"
    "用户暂停优先；真实外部阻塞保留缺口并继续独立项；不擅改模型/账户。"
)


def latest_turn(path):
    """用户输入/取消后等待运行器处理，不凭旧错误恢复。"""
    last = None
    with path.open() as source:
        for line in deque(source, maxlen=2000):
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            event = row.get("payload", {})
            if row.get("type") != "event_msg":
                continue
            if event.get("type") in ("task_started", "turn_aborted", "user_message"):
                last = None
            elif event.get("type") == "task_complete":
                last = row
    return last


def decision(agent, goal, turn, now):
    """仅在整计划仍待执行且总控确已停止时恢复；不打断正常工作。"""
    if agent.get("Archived") or agent.get("PendingPermissions"):
        return "hold"
    if Path(agent.get("Cwd", "/")).resolve() != ROOT:
        return "hold"
    status = (goal or {}).get("status")
    if status == "complete":
        return "complete"
    if status in ("usage_limited", "budget_limited"):
        return "action_required"
    if status not in ("active", "blocked") or not turn:
        return "hold"
    if agent.get("Status") not in ("idle", "error"):
        return "hold"
    error = turn["payload"].get("error")
    if error:
        message = json.dumps(error).lower()
        if any(x in message for x in (
            "insufficient_quota", "usage_limit", "quota exceeded", "credit",
            "spend limit", "billing", "usage limit", "401", "403",
            "unauthorized", "authentication", "permission denied",
        )):
            return "action_required"
        return "recover_error"
    # 真正的blocked不能由守护随意解除；只有运行器请求失败可按授权恢复。
    if status == "blocked":
        return "hold"
    completed = turn["payload"].get("completed_at")
    if completed is not None and now - completed >= 120:
        return "recover_idle"
    return "hold"


def inspect(agent):
    result = subprocess.run(
        ["paseo", "inspect", agent, "--json"], check=True,
        capture_output=True, text=True, timeout=30,
    )
    return json.loads(result.stdout)


def read_goal(database, thread):
    # 只读现有运行器状态，不直接写SQLite，也不依赖私有表进行状态修改。
    with sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True) as db:
        row = db.execute(
            "SELECT status FROM thread_goals WHERE thread_id=?", (thread,)
        ).fetchone()
    return {"status": row[0]} if row else None


def activate_goal(thread):
    """通过本机正式协议恢复运行器错误造成的blocked，保留目标与预算。"""
    process = subprocess.Popen(
        ["codex", "app-server", "--stdio"], stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    )
    buffer = b""

    def rpc(number, method, params):
        nonlocal buffer
        process.stdin.write(json.dumps({"id": number, "method": method, "params": params}).encode() + b"\n")
        process.stdin.flush()
        end = time.monotonic() + 20
        while time.monotonic() < end:
            if b"\n" not in buffer:
                if not select.select([process.stdout], [], [], 1)[0]:
                    continue
                data = os.read(process.stdout.fileno(), 65536)
                if not data:
                    raise RuntimeError("Codex app-server退出")
                buffer += data
                continue
            line, buffer = buffer.split(b"\n", 1)
            result = json.loads(line)
            if result.get("id") == number:
                if "error" in result:
                    raise RuntimeError("Codex Goal协议返回错误")
                return result["result"]
        raise TimeoutError(method)

    try:
        rpc(1, "initialize", {"clientInfo": {"name": "openrust-recovery", "version": "1"},
                              "capabilities": {"experimentalApi": True}})
        process.stdin.write(b'{"method":"initialized"}\n')
        process.stdin.flush()
        goal = rpc(2, "thread/goal/get", {"threadId": thread}).get("goal")
        if not goal or goal.get("status") not in ("active", "blocked"):
            return False
        if goal["status"] == "blocked":
            rpc(3, "thread/goal/set", {"threadId": thread, "status": "active"})
        return True
    finally:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agent", required=True)
    parser.add_argument("--thread", required=True)
    parser.add_argument("--rollout", type=Path, required=True)
    parser.add_argument("--goals-db", type=Path, default=Path.home() / ".codex/goals_1.sqlite")
    parser.add_argument("--watch", action="store_true", help="启用守护；默认只检查")
    args = parser.parse_args()
    # 防止配置错误导致向一个线程发送另一个线程的恢复指令。
    if args.thread not in args.rollout.name:
        raise SystemExit("rollout文件名与线程ID不一致")

    def snapshot():
        agent = inspect(args.agent)
        goal = read_goal(args.goals_db, args.thread)
        turn = latest_turn(args.rollout)
        return decision(agent, goal, turn, time.time()), goal, turn

    if not args.watch:
        action, goal, _ = snapshot()
        print(json.dumps({"action": action, "goal": goal}))
        return

    runtime = ROOT / "storage" / "controller-recovery" / args.agent
    runtime.mkdir(parents=True, exist_ok=True)
    with (runtime / "lock").open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SystemExit("已有守护运行，拒绝重复启动")
        (runtime / "pid").write_text(str(os.getpid()) + "\n")
        state_path = runtime / "state.json"
        state = json.loads(state_path.read_text()) if state_path.exists() else {}
        attempts = state.get("attempts", 0)
        seen = state.get("last_turn")
        pending = None
        due = 0

        def record(status, **extra):
            state.update(status=status, attempts=attempts, last_turn=seen,
                         updated_at=datetime.now().astimezone().isoformat(), **extra)
            temp = runtime / "state.tmp"
            temp.write_text(json.dumps(state, ensure_ascii=False, indent=2) + "\n")
            temp.replace(state_path)
            print(json.dumps(state, ensure_ascii=False), flush=True)

        record("watching")
        try:
            while True:
                if (runtime / "disabled").exists():
                    record("disabled")
                    return
                action, goal, turn = snapshot()
                key = (turn or {}).get("timestamp")
                if turn and not turn["payload"].get("error") and key != state.get("last_success"):
                    attempts = 0
                    state["last_success"] = key
                if action in ("complete", "action_required"):
                    record(action)
                    return
                unacknowledged = time.time() - state.get("sent_at", time.time()) >= 120
                if action.startswith("recover_") and (key != seen or unacknowledged):
                    if attempts >= len(DELAYS):
                        record("action_required", reason="连续三次恢复仍失败，停止发送并保留证据")
                        return
                    if pending != key:
                        pending = key
                        due = time.monotonic() + DELAYS[attempts] + random.uniform(0, 15)
                        record("backoff", reason=action, delay_seconds=DELAYS[attempts])
                    if time.monotonic() >= due:
                        current, _, current_turn = snapshot()
                        if current == action and current_turn.get("timestamp") == key:
                            if (runtime / "disabled").exists() or not activate_goal(args.thread):
                                record("hold")
                                return
                            attempts += 1
                            seen = key
                            record("dispatching")
                            subprocess.run(
                                ["paseo", "send", args.agent, PROMPT, "--no-wait", "--json"],
                                check=True, timeout=30, capture_output=True, text=True,
                            )
                            record("sent", sent_at=time.time())
                        pending = None
                else:
                    pending = None
                time.sleep(15)
        except Exception as error:
            # 状态不明时不重发；不把可能含凭据的异常正文写入日志。
            record("action_required", reason=type(error).__name__)
            raise SystemExit(2) from None


if __name__ == "__main__":
    main()
