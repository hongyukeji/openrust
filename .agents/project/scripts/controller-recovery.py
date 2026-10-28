#!/usr/bin/env python3
"""持续开发守护：原目标接续、冷却、单实例监督和按任务停止；仅使用标准库。"""

import argparse
from collections import deque
from datetime import datetime
import fcntl
import json
import os
from pathlib import Path
import random
import select
import signal
import sqlite3
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[3]
SCRIPT = Path(__file__).resolve()
DELAYS = (60, 180, 600)
PROMPT = (
    "用户已授权持续开发至整个计划完成；守护检测到异常或遗漏接续。"
    "核对AGENTS.md、Git/WIP、todo/plan、原Goal及在途子代理，沿用原目标继续。"
    "先收取既有任务结果，不重复派发、构建或创建第二个写入者。"
    "单簇完成不等于整体完成；通过计划验收后才标记Goal完成。"
    "用户暂停优先，外部缺口保留并继续独立项，不改账户、模型或付款设置。"
)


def atomic(path, value):
    temp = path.with_suffix('.tmp')
    temp.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    temp.replace(path)


def run_json(command):
    return json.loads(subprocess.run(command, check=True, capture_output=True,
                                     text=True, timeout=30).stdout)


def inspect(agent):
    return run_json(['paseo', 'inspect', agent, '--json'])


def rpc(method, params):
    """使用已安装Codex的正式本机协议；子进程总会回收，不读取认证文件。"""
    process = subprocess.Popen(['codex', 'app-server', '--stdio'], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    buffer = b''

    def request(number, name, arguments):
        nonlocal buffer
        process.stdin.write(json.dumps({'id': number, 'method': name, 'params': arguments}).encode() + b'\n')
        process.stdin.flush()
        end = time.monotonic() + 20
        while time.monotonic() < end:
            if b'\n' not in buffer:
                if not select.select([process.stdout], [], [], 1)[0]:
                    continue
                data = os.read(process.stdout.fileno(), 65536)
                if not data:
                    raise RuntimeError('协议进程退出')
                buffer += data
                continue
            line, buffer = buffer.split(b'\n', 1)
            value = json.loads(line)
            if value.get('id') == number:
                if 'error' in value:
                    raise RuntimeError('协议返回错误')
                return value['result']
        raise TimeoutError(name)

    try:
        request(1, 'initialize', {'clientInfo': {'name': 'openrust-recovery', 'version': '2'},
                                 'capabilities': {'experimentalApi': True}})
        process.stdin.write(b'{"method":"initialized"}\n')
        process.stdin.flush()
        return request(2, method, params)
    finally:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        process.stdin.close()
        process.stdout.close()


def goal(config):
    with sqlite3.connect(Path(config['goals_db']).resolve().as_uri() + '?mode=ro', uri=True) as db:
        row = db.execute('SELECT goal_id,status FROM thread_goals WHERE thread_id=?',
                         (config['thread'],)).fetchone()
    return {'id': row[0], 'status': row[1]} if row else None


def timeline(path):
    """只使用原线程生命周期；新输入和取消使旧恢复依据失效。"""
    last = None
    tools = 0
    with Path(path).open() as source:
        for line in deque(source, maxlen=2000):
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            payload = row.get('payload', {})
            if row.get('type') == 'response_item' and payload.get('type') in ('function_call', 'custom_tool_call'):
                tools += 1
            if row.get('type') != 'event_msg':
                continue
            kind = payload.get('type')
            if kind in ('task_started', 'task_complete', 'turn_aborted', 'user_message'):
                if kind == 'task_started':
                    tools = 0
                last = {'kind': kind, 'key': row['timestamp'], 'payload': payload, 'tools': tools}
    return last


def decision(agent, current_goal, turn, config, now):
    if not current_goal or current_goal['id'] != config['goal_id']:
        return 'needs_attention'
    if agent.get('Archived'):
        return 'stopped'
    if Path(agent.get('Cwd', '/')).resolve() != ROOT:
        return 'needs_attention'
    status = current_goal['status']
    if status in ('complete', 'paused'):
        return status
    if turn and turn['kind'] == 'turn_aborted':
        return 'stopped'
    if agent.get('PendingPermissions') or status == 'budget_limited':
        return 'needs_attention'
    if status == 'usage_limited':
        return 'quota_wait'
    if agent.get('Status') not in ('idle', 'error', 'closed'):
        return 'working'
    if not turn or turn['kind'] == 'user_message':
        return 'needs_attention'
    error = turn['payload'].get('error')
    if error:
        message = json.dumps(error).lower()
        if any(x in message for x in ('insufficient_quota', 'usage_limit', 'quota exceeded', 'credit', 'billing', 'usage limit')):
            return 'quota_wait'
        if any(x in message for x in ('unauthorized', 'authentication', 'permission denied', '401', '403')):
            return 'needs_attention'
        return 'recover_error'
    if status == 'blocked':
        return 'needs_attention'
    if agent.get('Status') in ('error', 'closed'):
        return 'recover_error'
    if turn['kind'] == 'task_complete' and now - turn['payload'].get('completed_at', now) >= 120:
        return 'recover_idle'
    return 'working'


def quota_delay(response, now):
    """只确认自然用量窗口，不购买credits、不兑换reset、不更换账户。"""
    # 新协议明确要求由后台确认included usage可用，不能仅从百分比推断。
    if response.get('ordinaryUsageAllowed') is not True:
        return 300
    limits = response.get('rateLimits') or {}
    windows = [limits[k] for k in ('primary', 'secondary') if limits.get(k)]
    if not windows or any(not isinstance(w.get('usedPercent'), (int, float)) for w in windows):
        return 300
    exhausted = [w for w in windows if w['usedPercent'] >= 100]
    if exhausted:
        resets = [w.get('resetsAt') for w in exhausted]
        if any(not isinstance(t, (int, float)) for t in resets):
            return 300
        return max(60, max(resets) + 30 - now)
    if limits.get('rateLimitReachedType'):
        return 300
    return 0


def notify(reason):
    """仅本机系统通知，失败不阻断守护；正文不含线程内容或凭据。"""
    try:
        subprocess.run(['osascript', '-e', 'on run argv\ndisplay notification (item 1 of argv) with title "OpenRust 持续开发"\nend run',
                        reason], capture_output=True, timeout=5, check=True)
        return True
    except (OSError, subprocess.SubprocessError):
        return False


def sleep_until(runtime, seconds):
    end = time.monotonic() + seconds
    while time.monotonic() < end and not (runtime / 'disabled').exists():
        time.sleep(min(1, max(0, end - time.monotonic())))


def binding_valid(config):
    record = json.loads(Path(config['agent_record']).read_text())
    persistence = record.get('persistence') or {}
    return (record.get('id') == config['agent'] and record.get('provider') == 'codex'
            and Path(record.get('cwd', '/')).resolve() == ROOT
            and persistence.get('provider') == 'codex'
            and persistence.get('sessionId') == config['thread']
            and persistence.get('metadata', {}).get('threadId') == config['thread'])


def worker(runtime, parent_pid):
    config = json.loads((runtime / 'config.json').read_text())
    path = runtime / 'state.json'
    state = json.loads(path.read_text()) if path.exists() else {}
    attempts = state.get('attempts', 0)

    def record(status, **extra):
        changed = state.get('status') != status
        state.update(status=status, attempts=attempts, checked_at=time.time(),
                         updated_at=datetime.now().astimezone().isoformat(), **extra)
        atomic(path, state)
        if changed:
            print(json.dumps({'status': status, 'attempts': attempts}, ensure_ascii=False), flush=True)
        if changed and status in ('cooldown', 'needs_attention', 'quota_wait', 'complete'):
            state['notification_delivered'] = notify({
                'cooldown': '恢复暂未成功，已进入冷却监测；无需反复发送继续。',
                'needs_attention': '任务需要检查权限、认证或目标状态；守护仍在监测。',
                'quota_wait': '正在等待自然额度恢复，恢复后继续原任务。',
                'complete': '整个目标已标记完成，正在清理本任务守护进程。',
            }[status])
            atomic(path, state)

    while not (runtime / 'disabled').exists():
        if os.getppid() != parent_pid:
            record('parent_lost')
            return
        try:
            terminal = goal(config)
            if terminal and terminal['id'] == config['goal_id'] and terminal['status'] in ('complete', 'paused'):
                record(terminal['status'])
                return
            if time.time() < state.get('check_after', 0):
                sleep_until(runtime, 15)
                continue
            if not binding_valid(config):
                record('needs_attention')
                sleep_until(runtime, 60)
                continue
            agent = inspect(config['agent'])
            current = goal(config)
            turn = timeline(config['rollout'])
            action = decision(agent, current, turn, config, time.time())
            if action in ('complete', 'paused', 'stopped'):
                record(action)
                return
            key = (turn or {}).get('key')
            if turn and turn['kind'] == 'task_complete' and not turn['payload'].get('error') and turn['tools'] and key != state.get('last_progress'):
                attempts = 0
                state['last_progress'] = key
                state.pop('retry_at', None)
            if action == 'working':
                state['check_failures'] = 0
                record(action)
                sleep_until(runtime, 15)
                continue
            if action == 'needs_attention':
                record(action)
                sleep_until(runtime, 60)
                continue
            # 同一发送意图尚无新轮次确认时不重发，避免队列延迟造成重复开发。
            if state.get('dispatch_key') == key:
                record('needs_attention', reason='awaiting_new_turn')
                sleep_until(runtime, 60)
                continue
            if state.get('pending_key') != key:
                delay = DELAYS[min(attempts, 2)] if attempts < 3 else 1800
                record('cooldown' if attempts >= 3 else 'backoff', pending_key=key,
                       retry_at=time.time() + delay + random.uniform(0, 15))
            if time.time() < state.get('retry_at', 0):
                sleep_until(runtime, 15)
                continue
            delay = quota_delay(rpc('account/rateLimits/read', {}), time.time())
            if delay:
                state['check_failures'] = 0
                record('quota_wait', retry_at=time.time() + min(delay, 300))
                sleep_until(runtime, 15)
                continue
            # 与stop串行；停止标记会先于等待锁写入，从而阻断新的发送。
            with (runtime / 'control.lock').open('a') as control:
                fcntl.flock(control, fcntl.LOCK_EX)
                if (runtime / 'disabled').exists() or not binding_valid(config):
                    continue
                current_turn = timeline(config['rollout'])
                fresh = decision(inspect(config['agent']), goal(config), current_turn, config, time.time())
                if fresh != action or not current_turn or current_turn['key'] != key:
                    continue
                latest_goal = rpc('thread/goal/get', {'threadId': config['thread']}).get('goal')
                if not latest_goal or latest_goal.get('status') in ('paused', 'complete', 'budgetLimited'):
                    continue
                if latest_goal['status'] in ('blocked', 'usageLimited'):
                    rpc('thread/goal/set', {'threadId': config['thread'], 'status': 'active'})
                if (runtime / 'disabled').exists():
                    continue
                attempts += 1
                record('dispatching', dispatch_key=key, dispatch_at=time.time())
                # 先落盘意图；发送异常也不清除，必须看到新的实际轮次才可再发送。
                run_json(['paseo', 'send', config['agent'], PROMPT, '--no-wait', '--json'])
                state['check_failures'] = 0
                record('sent')
        except Exception as error:
            failures = state.get('check_failures', 0) + 1
            record('cooldown', check_failures=failures,
                   check_after=time.time() + (1800 if failures >= 3 else 60),
                   retry_at=time.time() + (1800 if failures >= 3 else 60), reason=type(error).__name__)
        sleep_until(runtime, 15)
    record('stopped')


def terminate_child(process):
    # 即使worker先退出，也限时回收本次创建的专属进程组中的CLI/协议后代。
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        process.wait()
        return
    end = time.monotonic() + 8
    while time.monotonic() < end:
        process.poll()
        try:
            os.killpg(process.pid, 0)
        except ProcessLookupError:
            process.wait()
            return
        time.sleep(.1)
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def supervise(runtime, lock_fd):
    stopped = False

    def stop_signal(_signal, _frame):
        nonlocal stopped
        stopped = True

    signal.signal(signal.SIGTERM, stop_signal)
    signal.signal(signal.SIGINT, stop_signal)
    process = None
    try:
        atomic(runtime / 'supervisor.json', {'pid': os.getpid()})
        while not stopped and not (runtime / 'disabled').exists():
            process = subprocess.Popen([sys.executable, '-B', str(SCRIPT), '_worker', '--runtime', str(runtime),
                                        '--parent-pid', str(os.getpid())], pass_fds=(lock_fd,),
                                       start_new_session=True, stdin=subprocess.DEVNULL)
            atomic(runtime / 'worker.json', {'pid': process.pid})
            while process.poll() is None and not stopped and not (runtime / 'disabled').exists():
                time.sleep(1)
            terminate_child(process)
            code = process.returncode
            process = None
            if code == 0 or stopped or (runtime / 'disabled').exists():
                break
            notify('守护工作进程异常退出，监督进程将在30秒后恢复它。')
            sleep_until(runtime, 30)
    finally:
        if process:
            terminate_child(process)
        for name in ('supervisor.json', 'worker.json', 'pid'):
            (runtime / name).unlink(missing_ok=True)
        os.close(lock_fd)
        # 保留空锁文件的inode，避免并发start在不同inode上各自获得锁。


def config_for(args):
    agent = args.agent or os.environ.get('PASEO_AGENT_ID')
    thread = args.thread or os.environ.get('CODEX_THREAD_ID')
    if not agent or not thread:
        raise ValueError('缺少可信Paseo/Codex任务身份，不能猜测最新任务')
    uuid.UUID(agent)
    uuid.UUID(thread)
    home = Path(os.environ.get('CODEX_HOME', str(Path.home() / '.codex')))
    with sqlite3.connect((home / 'state_5.sqlite').resolve().as_uri() + '?mode=ro', uri=True) as db:
        row = db.execute('SELECT rollout_path,cwd FROM threads WHERE id=?', (thread,)).fetchone()
    if not row or Path(row[1]).resolve() != ROOT:
        raise ValueError('线程不属于本仓库')
    metadata = json.loads(Path(row[0]).open().readline())['payload']
    if metadata.get('id') != thread or isinstance(metadata.get('source'), dict):
        raise ValueError('只能登记已确认的主线程，不能为子代理另建守护')
    inspected = inspect(agent)
    if Path(inspected.get('Cwd', '/')).resolve() != ROOT or inspected.get('Provider') != 'codex':
        raise ValueError('Paseo目标不是本仓Codex任务')
    record = Path(os.environ.get('PASEO_HOME', str(Path.home() / '.paseo'))) / 'agents' / str(ROOT).lstrip('/').replace('/', '-') / (agent + '.json')
    config = {'agent': agent, 'thread': thread, 'rollout': row[0], 'goals_db': str(home / 'goals_1.sqlite'),
              'agent_record': str(record)}
    if not binding_valid(config):
        raise ValueError('Paseo agent与Codex线程不匹配')
    current = goal(config)
    if not current:
        raise ValueError('请先为当前完整任务创建原生Goal')
    if current['status'] == 'complete':
        raise ValueError('旧目标已完成，请先按用户新任务创建目标')
    config['goal_id'] = current['id']
    return config


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['start', 'status', 'stop', '_supervise', '_worker'])
    parser.add_argument('--agent')
    parser.add_argument('--thread')
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--lock-fd', type=int)
    parser.add_argument('--parent-pid', type=int)
    parser.add_argument('--stop-task', action='store_true', help='同时暂停Goal并中断主控，须用户明确停止')
    args = parser.parse_args()
    requested_at = time.time()
    if args.command.startswith('_'):
        if not args.runtime or not args.runtime.resolve().is_relative_to(ROOT / 'storage/controller-recovery'):
            raise SystemExit('无效守护运行目录')
        if args.command == '_worker':
            worker(args.runtime, args.parent_pid)
        else:
            supervise(args.runtime, args.lock_fd)
        return
    agent = args.agent or os.environ.get('PASEO_AGENT_ID')
    if not agent:
        raise SystemExit('缺少Paseo任务身份')
    uuid.UUID(agent)
    runtime = ROOT / 'storage/controller-recovery' / agent
    if args.command == 'status':
        state = json.loads((runtime / 'state.json').read_text()) if (runtime / 'state.json').exists() else {'status': 'not_started'}
        with (runtime / 'lock').open('a') if runtime.exists() else open(os.devnull) as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                state['lock_held'] = False
            except BlockingIOError:
                state['lock_held'] = True
        state['supervisor'] = json.loads((runtime / 'supervisor.json').read_text()) if (runtime / 'supervisor.json').exists() else None
        print(json.dumps(state, ensure_ascii=False))
        return
    runtime.mkdir(parents=True, exist_ok=True)
    if args.command == 'stop':
        (runtime / 'disabled').touch()
    with (runtime / 'control.lock').open('a') as control:
        fcntl.flock(control, fcntl.LOCK_EX)
        if args.command == 'stop':
            # start可能在本次stop等待控制锁时清过标记；获锁后重新确立停止优先。
            (runtime / 'disabled').touch()
            if args.stop_task:
                config = json.loads((runtime / 'config.json').read_text())
                if not binding_valid(config):
                    raise SystemExit('任务绑定已改变，已禁用守护但不操作其他线程')
                rpc('thread/goal/set', {'threadId': config['thread'], 'status': 'paused'})
                run_json(['paseo', 'stop', agent, '--json'])
                # Paseo发送会等两个安静秒；让已入队操作落定后再确认中断。
                time.sleep(3)
                if inspect(agent).get('Status') == 'running':
                    run_json(['paseo', 'stop', agent, '--json'])
                    time.sleep(2)
                    if inspect(agent).get('Status') == 'running':
                        raise SystemExit('守护已禁用，但主控尚未确认停止，需检查客户端状态')
            end = time.monotonic() + 45
            while (runtime / 'supervisor.json').exists() and time.monotonic() < end:
                time.sleep(1)
            if (runtime / 'supervisor.json').exists():
                raise SystemExit('停止请求已记录，但监督进程尚未确认退出')
            print('{"status":"stopped"}')
            return
        if (runtime / 'disabled').exists() and (runtime / 'disabled').stat().st_mtime >= requested_at:
            raise SystemExit('并发停止优先，未启动守护')
        lock = (runtime / 'lock').open('a')
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print('{"status":"already_running"}')
            return
        config = config_for(args)
        atomic(runtime / 'config.json', config)
        # 用户明确重新授权start，才可清除之前停止标记并恢复暂停的原目标。
        (runtime / 'disabled').unlink(missing_ok=True)
        if goal(config)['status'] == 'paused':
            rpc('thread/goal/set', {'threadId': config['thread'], 'status': 'active'})
        if (runtime / 'state.json').exists():
            prior = json.loads((runtime / 'state.json').read_text())
            if prior.get('status') in ('complete', 'paused', 'stopped', 'parent_lost', 'disabled'):
                atomic(runtime / 'state.json', {})
        pending_state = json.loads((runtime / 'state.json').read_text()) if (runtime / 'state.json').exists() else {}
        pending_state.update(status='starting', updated_at=datetime.now().astimezone().isoformat())
        atomic(runtime / 'state.json', pending_state)
        for name in ('supervisor.json', 'worker.json', 'pid'):
            (runtime / name).unlink(missing_ok=True)
        with (runtime / 'watch.log').open('ab') as log:
            process = subprocess.Popen([sys.executable, '-B', str(SCRIPT), '_supervise', '--runtime', str(runtime),
                                        '--lock-fd', str(lock.fileno())], pass_fds=(lock.fileno(),),
                                       stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
        lock.close()
        deadline = time.monotonic() + 5
        while not (runtime / 'supervisor.json').exists() and process.poll() is None and time.monotonic() < deadline:
            time.sleep(.1)
        if process.poll() is not None:
            raise SystemExit('监督进程启动失败，见watch.log')
        print(json.dumps({'status': 'started', 'pid': process.pid, 'agent': agent, 'thread': config['thread']}))


if __name__ == '__main__':
    main()
