#!/usr/bin/env python3
"""原总控的合作式队列定时器；只投递控制检查，不唤醒模型或修改 Goal。"""
import argparse
from contextlib import contextmanager
import json
import math
import os
import re
from contextlib import closing
from pathlib import Path
import subprocess
import sqlite3
import sys
import time
import uuid

import guard_heartbeat as heartbeat
from guard_platform import background, lock_file

WAIT_TIMEOUT = 3600


def caller(thread):
    thread = str(uuid.UUID(thread))
    if os.environ.get('CODEX_THREAD_ID') != thread:
        raise ValueError('caller_mismatch')
    return thread


def runtime(thread):
    path = heartbeat.ROOT / '.agents/project/guards' / thread
    if not path.resolve().is_relative_to(heartbeat.ROOT) or not path.is_dir():
        raise ValueError('runtime_missing_or_outside_project')
    return path


@contextmanager
def locked(path, name='cooperative.lock', blocking=True):
    with (path / name).open('a') as stream:
        lock_file(stream, blocking)
        yield


def evidence(thread):
    home = Path(os.environ.get('CODEX_HOME', str(Path.home() / '.codex'))).resolve()
    binding, goal, status = heartbeat.identity(thread, home)
    ledger = heartbeat.read_json(runtime(thread) / 'native-heartbeat.json')
    if (ledger.get('binding') != binding or ledger.get('goal') != goal
            or ledger.get('version') != 1 or ledger.get('fraction') != '0.8'
            or ledger.get('allow_existing_credits') is not False
            or not isinstance(ledger.get('account_budgets'), dict)):
        raise ValueError('native_binding_or_budget_invalid')
    return binding, goal, status, ledger


def eligible(ledger, now, interval):
    check = ledger.get('last_check', {})
    stamp = check.get('at')
    return (ledger.get('usage_status') == 'usage_budget_available'
            and check.get('action') == 'continue'
            and type(stamp) in (int, float) and math.isfinite(stamp)
            and 0 <= now - stamp <= interval + 30
            and bool(ledger.get('account_budgets')))


def finish(state, reason):
    state.update(status='stopped', reason=reason,
                 cleanup_pending=bool(state.get('pending')))
    return state


def tick(state, binding, goal, status, ledger, now):
    """纯状态决策；发送 intent 必须先持久化，调用方才可执行队列命令。"""
    if state['status'] == 'stopped':
        return None
    if state['binding'] != binding or state['goal'] != goal:
        finish(state, 'binding_or_goal_changed')
        return None
    if ledger.get('terminal_action') or status in ('paused', 'complete'):
        finish(state, 'goal_' + status)
        return None
    if state.get('pending') or status != 'active' or not eligible(ledger, now, state['interval']):
        state.setdefault('wait_since', now)
        if now - state['wait_since'] >= WAIT_TIMEOUT:
            finish(state, 'wait_expired')
        return None
    state.pop('wait_since', None)
    if now < state['next_at']:
        return None
    identifier = str(uuid.uuid4())
    state['pending'] = {'heartbeat_id': identifier, 'phase': 'sending', 'at': now}
    state['wait_since'] = now
    state['next_at'] = now + state['interval']
    return identifier


def message(state, identifier):
    return (f"控制检查请求 source=codex_cli_queue run_id={state['run_id']} "
            f"heartbeat_id={identifier} thread={state['binding']['thread']} "
            f"goal_id={state['goal']['id']}。这不是 App 原生 heartbeat，也不是业务执行许可。"
            '仅原总控在安全队列边界处理：先运行项目内 '
            f"{Path(__file__).resolve()} --thread {state['binding']['thread']} ack "
            f"--heartbeat-id {identifier}；必须核验本 run/Goal/活动状态且 ack 成功，"
            '再获取新鲜正式 get_usage_limits 输出并执行 guard_heartbeat checkpoint；'
            '仅门禁允许后按原 Goal 与唯一 todo 继续。停止、旧 run 或不匹配须拒绝。'
            '不承诺自动唤醒 true idle 或精确中断；不重新初始化或扩大预算。')


def validate_cli(codex, runner=subprocess.run):
    path = Path(codex)
    if not path.is_absolute() or not path.is_file() or not os.access(path, os.X_OK):
        raise ValueError('absolute_existing_codex_required')
    result = runner([str(path), 'queue', '--help'], capture_output=True, text=True, timeout=15)
    lines = [line.strip() for line in result.stdout.splitlines()]
    help_text = ' '.join(lines).lower()
    if (result.returncode or '--resume' not in lines
            or 'after queueing, try to resume the thread on the local daemon or selected --remote server.' not in help_text
            or 'a failed resume produces a warning and the command still succeeds' not in help_text):
        raise ValueError('queue_only_contract_unverified')
    return str(path)


def receipt(result, thread):
    match = re.fullmatch(r'Queued message ([0-9a-f-]{36}) for thread ([0-9a-f-]{36})\.\s*', result.stdout)
    if result.returncode or not match:
        return None
    try:
        queue_id, target = (str(uuid.UUID(value)) for value in match.groups())
    except ValueError:
        return None
    return queue_id if target == thread else None


def queue_absent(state):
    """只核自有精确队列ID；消失不证明已执行或回复。"""
    pending = state.get('pending') or {}
    queue_id = pending.get('queue_id')
    if not queue_id:
        return None
    home = Path(os.environ.get('CODEX_HOME', str(Path.home() / '.codex'))).resolve()
    try:
        if str(uuid.UUID(queue_id)) != queue_id:
            return None
        with closing(sqlite3.connect((home / 'queue_1.sqlite').as_uri() + '?mode=ro', uri=True)) as db:
            row = db.execute('SELECT thread_id FROM queued_items WHERE id=? LIMIT 1',
                             (queue_id,)).fetchone()
        return True if row is None else (False if row[0] == state['binding']['thread'] else None)
    except (OSError, ValueError, sqlite3.Error):
        return None


def status_view(path, state):
    result = dict(state, worker_lock_held=False)
    try:
        with (path / 'cooperative-worker.lock').open('r+') as stream:
            lock_file(stream, False)
    except BlockingIOError:
        result['worker_lock_held'] = True
    except OSError:
        result['worker_lock_held'] = None
    if state.get('pending'):
        result['queue_absent'] = queue_absent(state)
    return result


def start(thread, codex, interval=1800, runner=subprocess.run, spawn=subprocess.Popen):
    thread = caller(thread)  # 错调用方在任何锁/文件写入之前拒绝。
    if type(interval) is not int or interval < 60:
        raise ValueError('interval_must_be_at_least_60')
    path = runtime(thread)
    binding, goal, status, ledger = evidence(thread)
    if status != 'active' or ledger.get('terminal_action'):
        raise ValueError('goal_not_active')
    codex = validate_cli(codex, runner)
    with locked(path):
        target = path / 'cooperative.json'
        old = heartbeat.read_json(target) if target.exists() else {}
        if old.get('status') in ('starting', 'running'):
            if old.get('binding') != binding or old.get('goal') != goal:
                raise ValueError('existing_run_binding_mismatch')
            return status_view(path, old)
        # 只有 stopped 且实际独占 worker 锁后，才核对队列消失并允许重启。
        with locked(path, 'cooperative-worker.lock', False):
            if old.get('pending'):
                if old.get('status') != 'stopped' or queue_absent(old) is not True:
                    return dict(old, worker_lock_held=False)
                old['last_queue_absent'] = dict(old['pending'], observed_at=time.time(),
                                                meaning='absent_only_not_execution')
                old.update(pending=None, cleanup_pending=False)
                heartbeat.atomic(target, old)
            state = {'version': 1, 'run_id': str(uuid.uuid4()), 'binding': binding,
                     'goal': goal, 'status': 'starting', 'interval': interval,
                     'next_at': time.time() + interval, 'codex': codex,
                     'pending': None, 'cleanup_pending': False,
                     'previous_queue_absent': old.get('last_queue_absent') if target.exists() else None}
            heartbeat.atomic(target, state)
            try:
                spawn([sys.executable, '-B', str(Path(__file__).resolve()), '--thread', thread,
                       '_worker', '--run-id', state['run_id']], stdin=subprocess.DEVNULL,
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **background())
            except OSError:
                finish(state, 'launch_failed')
                heartbeat.atomic(target, state)
                raise
            return state


def control(thread, action, identifier=None):
    thread = caller(thread)
    path = runtime(thread)
    if action == 'status':
        return status_view(path, heartbeat.read_json(path / 'cooperative.json'))
    with locked(path):
        target = path / 'cooperative.json'
        state = heartbeat.read_json(target)
        if action == 'stop':
            finish(state, 'manual_stop')
        elif action == 'ack':
            binding, goal, status, ledger = evidence(thread)
            pending = state.get('pending')
            if (state['status'] not in ('starting', 'running') or status != 'active'
                    or ledger.get('terminal_action') or state['binding'] != binding
                    or state['goal'] != goal or not pending
                    or pending['heartbeat_id'] != identifier):
                raise ValueError('ack_rejected')
            state['last_ack'] = dict(pending, run_id=state['run_id'], received_at=time.time())
            state['pending'] = None
            state.pop('wait_since', None)
        heartbeat.atomic(target, state)
        return state


def worker(thread, run_id, runner=subprocess.run, sleep=time.sleep):
    thread = caller(thread)
    path = runtime(thread)
    target = path / 'cooperative.json'
    with locked(path, 'cooperative-worker.lock'):
        while True:
            with locked(path):
                state = heartbeat.read_json(target)
                if state['run_id'] != run_id or state['status'] == 'stopped':
                    return
                try:
                    binding, goal, status, ledger = evidence(thread)
                    state.update(status='running', last_tick=time.time())
                    identifier = tick(state, binding, goal, status, ledger, time.time())
                except (OSError, ValueError, TypeError, KeyError, sqlite3.Error):
                    identifier = None
                    finish(state, 'local_evidence_unavailable')
                heartbeat.atomic(target, state)
                if state['status'] == 'stopped':
                    return
            if identifier:
                # 不持有状态锁等待CLI；stop/ack仍可立即落盘。
                try:
                    result = runner([state['codex'], 'queue', '--thread', thread,
                                     '--message', message(state, identifier)],
                                    capture_output=True, text=True, timeout=4)
                    queue_id = receipt(result, thread)
                    phase = 'queued' if queue_id and not result.stderr else 'unknown'
                except (OSError, subprocess.SubprocessError):
                    phase, queue_id = 'unknown', None
                with locked(path):
                    current = heartbeat.read_json(target)
                    if (current['run_id'] == run_id and current.get('pending')
                            and current['pending']['heartbeat_id'] == identifier):
                        current['pending'].update(phase=phase, queue_id=queue_id)
                        current['cleanup_pending'] = current['status'] == 'stopped'
                        heartbeat.atomic(target, current)
            sleep(.5)  # 队列命令≤4秒，加本地轮询≤0.5秒。


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--thread', required=True)
    commands = parser.add_subparsers(dest='action', required=True)
    command = commands.add_parser('start')
    command.add_argument('--codex', required=True)
    command.add_argument('--interval', type=int, default=1800)
    commands.add_parser('stop')
    commands.add_parser('status')
    command = commands.add_parser('ack')
    command.add_argument('--heartbeat-id', required=True)
    command = commands.add_parser('_worker')
    command.add_argument('--run-id', required=True)
    args = parser.parse_args()
    if args.action == 'start':
        result = start(args.thread, args.codex, args.interval)
    elif args.action == '_worker':
        return worker(args.thread, args.run_id)
    else:
        result = control(args.thread, args.action, getattr(args, 'heartbeat_id', None))
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
