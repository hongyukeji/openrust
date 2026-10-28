#!/usr/bin/env python3
"""持续开发守护：原目标接续、冷却、单实例监督和按任务停止；仅使用标准库。"""

import argparse
from datetime import datetime
import json
import os
from pathlib import Path
import random
import re
import signal
import sqlite3
import subprocess
import sys
import time
import tempfile
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guard_platform as platform

ROOT = Path.cwd().resolve()
CHECK_INTERVAL = 120
TAIL_BYTES = 512 * 1024
SCRIPT = Path(__file__).resolve()
DELAYS = (60, 180, 600)
PROMPT = (
    "按原目标与项目约束，从当前证据继续；先收件避免重复执行。"
    "用户停止优先，整体验收后完成Goal并清理守护。"
)
_TIMELINE_CACHE = (None, None)



def atomic(path, value):
    # 各任务可同时更新项目看板，临时文件须唯一，避免互相覆盖/删除。
    fd, name = tempfile.mkstemp(prefix=path.name + '.', suffix='.tmp', dir=path.parent)
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as output:
            output.write(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
        os.replace(name, path)
    finally:
        Path(name).unlink(missing_ok=True)


def supervisor_alive(supervisor, runtime):
    pid = (supervisor or {}).get('pid')
    if not isinstance(pid, int) or pid <= 0:
        return False
    if platform.WINDOWS:
        identity = supervisor.get('created')
        return identity is not None and platform.process_identity(pid) == identity
    result = subprocess.run(['ps', '-p', str(pid), '-o', 'command='], capture_output=True,
                            text=True, timeout=5)
    command = result.stdout
    return result.returncode == 0 and str(SCRIPT) in command and ' _supervise ' in command and str(runtime) in command


def summary(runtime):
    state = json.loads((runtime / 'state.json').read_text(encoding='utf-8')) if (runtime / 'state.json').exists() else {'status': 'not_started'}
    config = json.loads((runtime / 'config.json').read_text(encoding='utf-8')) if (runtime / 'config.json').exists() else {}
    held = False
    with (runtime / 'lock').open('a') as lock:
        try:
            platform.lock_file(lock, blocking=False)
        except BlockingIOError:
            held = True
    supervisor = json.loads((runtime / 'supervisor.json').read_text(encoding='utf-8')) if (runtime / 'supervisor.json').exists() else None
    alive = supervisor_alive(supervisor, runtime)
    fresh = time.time() - state.get('checked_at', 0) <= CHECK_INTERVAL * 3
    terminal = state.get('status') in ('complete', 'paused', 'stopped', 'parent_lost')
    state.update(agent=config.get('agent', runtime.name), thread=config.get('thread'),
                 label=config.get('label', '持续开发任务'), goal_id=config.get('goal_id'),
                 lock_held=held, supervisor=supervisor, supervisor_alive=alive,
                 heartbeat_fresh=fresh, active=held and alive and fresh and not terminal)
    if not terminal and state.get('status') != 'not_started':
        if not held or not alive:
            state['status'] = 'orphaned'
        elif not fresh:
            state['status'] = 'stale'
    state['guard_alive'] = state['active']
    state['task_running'] = bool(state['active'] and state.get('task_running'))
    return state


def publish_registry(runtime):
    base = runtime.parent
    if base.name != 'guards' or base.parent.name != 'project' or base.parent.parent.name != '.agents':
        return
    # 串行整次快照，防止较旧快照后写覆盖另一个守护的新状态。
    with (base / '.registry.lock').open('a') as registry_lock:
        platform.lock_file(registry_lock)
        rows = []
        for item in sorted(base.iterdir()):
            if not item.is_dir() or not (item / 'config.json').exists():
                continue
            try:
                rows.append(summary(item))
            except (OSError, ValueError, subprocess.SubprocessError):
                rows.append({'agent': item.name, 'status': 'unreadable', 'active': False})
        atomic(base / 'registry.json', {'updated_at': datetime.now().astimezone().isoformat(),
                                       'guards': rows})


def run_json(command):
    process = platform.launch_tool(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE)
    try:
        output, error = process.communicate(timeout=30)
        if process.returncode:
            raise subprocess.CalledProcessError(process.returncode, command, output, error)
        return json.loads(output.decode('utf-8'))
    finally:
        try:
            terminate_child(process)
        finally:
            for stream in (process.stdin, process.stdout, process.stderr):
                if stream:
                    stream.close()


def inspect(agent):
    return run_json(['paseo', 'inspect', agent, '--json'])


def rpc(method, params):
    """使用已安装Codex的正式本机协议；子进程总会回收，不读取认证文件。"""
    process = platform.launch_tool(['codex', 'app-server', '--stdio'], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    lines = platform.Lines(process.stdout)

    def request(number, name, arguments):
        process.stdin.write(json.dumps({'id': number, 'method': name, 'params': arguments}).encode('utf-8') + b'\n')
        process.stdin.flush()
        end = time.monotonic() + 20
        while time.monotonic() < end:
            value = json.loads(lines.get(end))
            if value.get('id') == number:
                if 'error' in value:
                    raise RuntimeError('协议返回错误')
                return value['result']
        raise TimeoutError(name)

    try:
        request(1, 'initialize', {'clientInfo': {'name': 'continuous-development-guard', 'version': '2'},
                                 'capabilities': {'experimentalApi': True}})
        process.stdin.write(b'{"method":"initialized"}\n')
        process.stdin.flush()
        return request(2, method, params)
    finally:
        try:
            terminate_child(process)
        finally:
            process.stdin.close()
            process.stdout.close()
            lines.close()


def goal(config):
    with sqlite3.connect(Path(config['goals_db']).resolve().as_uri() + '?mode=ro', uri=True) as db:
        row = db.execute('SELECT goal_id,status,created_at_ms,objective,updated_at_ms FROM thread_goals WHERE thread_id=?',
                         (config['thread'],)).fetchone()
    return {'id': row[0], 'status': row[1], 'created_at_ms': row[2], 'objective': row[3], 'updated_at_ms': row[4]} if row else None


def bound_goal(config):
    """协议不暴露 goal_id 时，以本地ID前后校验与协议公开身份交叉确认。"""
    before = goal(config)
    if not before or before['id'] != config['goal_id'] or not binding_valid(config):
        raise ValueError('原Goal或线程绑定已改变，拒绝操作新任务')
    remote = rpc('thread/goal/get', {'threadId': config['thread']}).get('goal')
    after = goal(config)
    if before != after or not remote:
        raise ValueError('核对期间Goal已变化，拒绝操作')
    if remote.get('id') is not None:
        matches = remote['id'] == config['goal_id']
    else:
        matches = (remote.get('threadId') == config['thread']
                   and isinstance(before.get('created_at_ms'), int)
                   and remote.get('createdAt') == before['created_at_ms'] // 1000
                   and remote.get('objective') == before.get('objective'))
    if not matches:
        raise ValueError('协议与本地原Goal身份不一致，拒绝操作')
    return remote


def completed_at(turn):
    value = turn['payload'].get('completed_at')
    if isinstance(value, (int, float)):
        return value
    try:
        timestamp = datetime.fromisoformat(turn['key'].replace('Z', '+00:00'))
        return timestamp.timestamp() if timestamp.tzinfo is not None else None
    except (ValueError, TypeError, AttributeError):
        return None


def explicit_goal_status(payload):
    """只读显式Goal控制调用；不从普通错误文字猜测业务阻塞。"""
    name = payload.get('name', '').rsplit('.', 1)[-1]
    arguments = payload.get('arguments', payload.get('input', ''))
    if name == 'update_goal':
        try:
            value = json.loads(arguments) if isinstance(arguments, str) else arguments
            return value.get('status') if isinstance(value, dict) else None
        except ValueError:
            return None
    if name == 'exec' and isinstance(arguments, str):
        try:
            decoded = json.loads(arguments)
            if isinstance(decoded, dict):
                arguments = decoded.get('code', '')
        except ValueError:
            pass
        if not isinstance(arguments, str):
            return None
        matches = re.findall(r"\bupdate_goal\s*\(\s*\{[^{}]*?['\"]?status['\"]?\s*:\s*['\"](blocked|paused|complete|active)['\"]", arguments)
        return matches[-1] if matches else None
    return None


def timeline(path):
    """只使用原线程生命周期；新输入和取消使旧恢复依据失效。"""
    global _TIMELINE_CACHE
    path = Path(path)
    info = path.stat()
    identity = (str(path.resolve()), info.st_ino, info.st_size, info.st_mtime_ns)
    if identity == _TIMELINE_CACHE[0]:
        return _TIMELINE_CACHE[1]
    last = None
    tools = 0
    semantic_blocked = False
    goal_control = None
    with path.open('rb') as source:
        size = source.seek(0, os.SEEK_END)
        source.seek(max(0, size - TAIL_BYTES))
        if size > TAIL_BYTES:
            source.readline()
        while line := source.readline():
            offset = source.tell()
            try:
                row = json.loads(line)
            except (json.JSONDecodeError, UnicodeDecodeError):
                continue
            payload = row.get('payload', {})
            if row.get('type') == 'response_item' and payload.get('type') in ('function_call', 'custom_tool_call'):
                tools += 1
                explicit = explicit_goal_status(payload)
                if explicit is not None:
                    semantic_blocked = explicit == 'blocked'
                    goal_control = {'status': explicit, 'key': row.get('timestamp'),
                                    'source': [str(path.resolve()), info.st_ino], 'offset': offset}
            if row.get('type') != 'event_msg':
                continue
            kind = payload.get('type')
            if kind in ('task_started', 'task_complete', 'turn_aborted', 'user_message'):
                if kind == 'task_started':
                    tools = 0
                last = {'kind': kind, 'key': row['timestamp'], 'payload': payload, 'tools': tools}
    if last is not None:
        last['semantic_blocked'] = semantic_blocked
        last['goal_control'] = goal_control
    _TIMELINE_CACHE = (identity, last)
    return last


def controlled_turn(state, turn, current, config):
    """同一Goal确认为active后记录恢复水位，旧日志不能重新引入已解除的控制。"""
    if not turn:
        return turn
    control = turn.get('goal_control')
    watermark = state.get('control_watermark') or {}
    if control and control == watermark:
        control = None
    if control:
        state['goal_control'] = control
    if current and current['id'] == config['goal_id'] and current['status'] == 'active':
        resolved = state.get('goal_control')
        # active的本地版本必须晚于控制调用；刚发出的blocked尚未入库时不能误清。
        control_time = completed_at({'key': (resolved or {}).get('key'), 'payload': {}})
        version = current.get('updated_at_ms')
        if (resolved and isinstance(version, int) and control_time is not None
                and control_time * 1000 <= version):
            state.pop('goal_control', None)
            if resolved.get('source') and isinstance(resolved.get('offset'), int):
                state['control_watermark'] = dict(resolved)
    blocked = (state.get('goal_control') or {}).get('status') in ('blocked', 'paused', 'complete')
    # 无版本的既有state继续保守保护；只有确认原Goal active才解除。
    return dict(turn, semantic_blocked=blocked or (not turn.get('goal_control') and turn.get('semantic_blocked', False)))


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
    if agent.get('Status') not in ('idle', 'error', 'closed'):
        return 'working'
    if not turn or turn['kind'] == 'user_message':
        return 'needs_attention'
    if turn.get('semantic_blocked'):
        return 'needs_attention'
    error = turn['payload'].get('error')
    if error:
        message = json.dumps(error).lower()
        if any(x in message for x in ('unauthorized', 'authentication', 'permission denied', '401', '403')):
            return 'needs_attention'
        if any(x in message for x in ('insufficient_quota', 'usage_limit', 'quota exceeded', 'credit', 'billing', 'usage limit')):
            return 'quota_wait'
        return 'quota_wait' if status == 'usage_limited' else 'recover_error'
    if status == 'usage_limited':
        return 'quota_wait'
    if agent.get('Status') in ('error', 'closed'):
        return 'recover_error'
    if turn['kind'] == 'task_complete':
        finished = completed_at(turn)
        if finished is None:
            return 'needs_attention'
        if now - finished >= 120:
            return 'recover_idle'
    return 'working'


def quota_delay(response, now):
    """只确认自然用量窗口，不购买credits、不兑换reset、不更换账户。"""
    # 新协议明确要求由后台确认included usage可用，不能仅从百分比推断。
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
    if response.get('ordinaryUsageAllowed') is not True or limits.get('rateLimitReachedType'):
        return 300
    return 0


def notify(reason):
    """仅本机系统通知，失败不阻断守护；正文不含线程内容或凭据。"""
    if sys.platform != 'darwin':
        return False
    try:
        subprocess.run(['osascript', '-e', 'on run argv\ndisplay notification (item 1 of argv) with title "持续开发守护"\nend run',
                        reason], capture_output=True, timeout=5, check=True)
        return True
    except (OSError, subprocess.SubprocessError):
        return False


def sleep_until(runtime, seconds):
    end = time.monotonic() + seconds
    while time.monotonic() < end and not (runtime / 'disabled').exists():
        time.sleep(min(1, max(0, end - time.monotonic())))


def binding_valid(config):
    record = json.loads(Path(config['agent_record']).read_text(encoding='utf-8'))
    persistence = record.get('persistence') or {}
    return (record.get('id') == config['agent'] and record.get('provider') == 'codex'
            and Path(record.get('cwd', '/')).resolve() == ROOT
            and persistence.get('provider') == 'codex'
            and persistence.get('sessionId') == config['thread']
            and persistence.get('metadata', {}).get('threadId') == config['thread'])


def worker(runtime, parent_pid):
    config = json.loads((runtime / 'config.json').read_text(encoding='utf-8'))
    path = runtime / 'state.json'
    state = json.loads(path.read_text(encoding='utf-8')) if path.exists() else {}
    attempts = state.get('attempts', 0)

    def record(status, **extra):
        changed = state.get('status') != status
        state.update(status=status, attempts=attempts, checked_at=time.time(),
                         updated_at=datetime.now().astimezone().isoformat(), **extra)
        if status != 'working':
            state['task_running'] = False
        state['reason'] = extra.get('reason', {
            'working': 'task_running' if state.get('task_running') else 'waiting_for_turn',
            'cooldown': 'recovery_cooldown', 'backoff': 'recovery_backoff',
            'quota_wait': 'quota_check_pending', 'needs_attention': 'hard_boundary',
        }.get(status, status))
        state['next_recovery_at'] = (state.get('quota_check_at') if status == 'quota_wait' else
                                     state.get('retry_at') if status in ('backoff', 'cooldown') else None)
        state['next_check_at'] = None if status in ('complete', 'paused', 'stopped', 'parent_lost') else time.time() + CHECK_INTERVAL
        atomic(path, state)
        publish_registry(runtime)
        if changed:
            print(json.dumps({'status': status, 'attempts': attempts}, ensure_ascii=False), flush=True)
        if changed and status in ('cooldown', 'needs_attention', 'quota_wait', 'complete'):
            state['notification_delivered'] = notify({
                'cooldown': '恢复暂未成功，已进入冷却监测；无需反复发送继续。',
                'needs_attention': '任务需要检查权限、认证或目标状态；守护仍在监测。',
                'quota_wait': '正在等待自然额度恢复，恢复后继续原任务。',
                'complete': '整个目标已标记完成，正在清理本任务守护进程。',
            }[status])
            state['notification_status'] = ('delivered' if state['notification_delivered'] else
                                            'unavailable' if sys.platform != 'darwin' else 'failed')
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
            if not binding_valid(config):
                record('needs_attention', reason='task_binding_changed')
                sleep_until(runtime, CHECK_INTERVAL)
                continue
            agent = inspect(config['agent'])
            state['task_running'] = agent.get('Status') == 'running'
            current = goal(config)
            turn = timeline(config['rollout'])
            turn = controlled_turn(state, turn, current, config)
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
                sleep_until(runtime, CHECK_INTERVAL)
                continue
            if action == 'needs_attention':
                reason = ('goal_changed' if not current or current['id'] != config['goal_id'] else
                          'permissions_pending' if agent.get('PendingPermissions') else
                          'budget_limited' if current['status'] == 'budget_limited' else
                          'explicit_goal_control' if turn and turn.get('semantic_blocked') else
                          'input_or_authentication_required')
                record(action, reason=reason)
                sleep_until(runtime, CHECK_INTERVAL)
                continue
            if time.time() < state.get('check_after', 0):
                record('cooldown', reason=state.get('reason', 'check_failure'))
                sleep_until(runtime, CHECK_INTERVAL)
                continue
            # 同一发送意图尚无新轮次确认时不重发，避免队列延迟造成重复开发。
            if state.get('dispatch_key') == key:
                record('needs_attention', reason='awaiting_new_turn')
                sleep_until(runtime, CHECK_INTERVAL)
                continue
            if state.get('pending_key') != key:
                delay = DELAYS[min(attempts, 2)] if attempts < 3 else 1800
                record('cooldown' if attempts >= 3 else 'backoff', pending_key=key,
                       retry_at=time.time() + delay + random.uniform(0, 15))
            if time.time() < state.get('retry_at', 0):
                record(state.get('status', 'backoff'))
                sleep_until(runtime, CHECK_INTERVAL)
                continue
            if time.time() < state.get('quota_check_at', 0):
                record('quota_wait')
                sleep_until(runtime, CHECK_INTERVAL)
                continue
            delay = quota_delay(rpc('account/rateLimits/read', {}), time.time())
            if delay:
                state['check_failures'] = 0
                record('quota_wait', quota_check_at=time.time() + delay)
                sleep_until(runtime, CHECK_INTERVAL)
                continue
            state.pop('quota_check_at', None)
            # 与stop串行；停止标记会先于等待锁写入，从而阻断新的发送。
            with (runtime / 'control.lock').open('a') as control:
                platform.lock_file(control)
                if (runtime / 'disabled').exists() or not binding_valid(config):
                    continue
                current = goal(config)
                current_turn = controlled_turn(state, timeline(config['rollout']), current, config)
                fresh = decision(inspect(config['agent']), current, current_turn, config, time.time())
                if fresh != action or not current_turn or current_turn['key'] != key:
                    continue
                latest_goal = bound_goal(config)
                if latest_goal.get('status') in ('paused', 'complete', 'budgetLimited'):
                    continue
                if latest_goal['status'] in ('usageLimited', 'blocked'):
                    if latest_goal['status'] == 'blocked' and current_turn.get('semantic_blocked'):
                        continue
                    rpc('thread/goal/set', {'threadId': config['thread'], 'status': 'active'})
                    # 协议往返期间的新暂停/输入/权限和Goal替换仍优先于发送。
                    if bound_goal(config).get('status') != 'active':
                        continue
                    current = goal(config)
                    current_turn = controlled_turn(state, timeline(config['rollout']), current, config)
                    if (not current_turn or current_turn['key'] != key
                            or decision(inspect(config['agent']), current, current_turn, config, time.time()) != action):
                        continue
                if (runtime / 'disabled').exists():
                    continue
                attempts += 1
                record('dispatching', dispatch_key=key, dispatch_at=time.time())
                # 先落盘意图；发送异常也不清除，必须看到新的实际轮次才可再发送。
                try:
                    run_json(['paseo', 'send', config['agent'], PROMPT, '--no-wait', '--json'])
                except FileNotFoundError:
                    # 工具尚未启动，确定没有入队；网络/退出错误仍保留意图避免重发。
                    state.pop('dispatch_key', None)
                    raise
                state['check_failures'] = 0
                record('sent')
        except Exception as error:
            failures = state.get('check_failures', 0) + 1
            record('cooldown', check_failures=failures,
                   check_after=time.time() + (1800 if failures >= 3 else 60),
                   retry_at=time.time() + (1800 if failures >= 3 else 60), reason=type(error).__name__)
        sleep_until(runtime, CHECK_INTERVAL)
    record('stopped')


def terminate_child(process):
    owner_fd = getattr(process, '_guard_owner_fd', None)
    if hasattr(process, '_guard_owner_fd'):
        if owner_fd is not None:
            os.close(owner_fd)
            process._guard_owner_fd = None
        # EOF由独立bootstrap处理，worker被SIGKILL也触发同一回收路径。
        process.wait(timeout=5)
        return
    if platform.WINDOWS:
        job = getattr(process, '_guard_job', None)
        if job:
            job.close()
        elif process.poll() is None:
            process.kill()  # 仅尚未放行、不能派生子进程的 bootstrap
        process.wait(timeout=10)
        return
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
    owned_lock = None
    if platform.WINDOWS:
        owned_lock = (runtime / 'lock').open('a')
        platform.lock_file(owned_lock, blocking=False)
    stopped = False

    def stop_signal(_signal, _frame):
        nonlocal stopped
        stopped = True

    signal.signal(signal.SIGTERM, stop_signal)
    signal.signal(signal.SIGINT, stop_signal)
    process = None
    try:
        while not stopped and not (runtime / 'disabled').exists():
            command = [sys.executable, '-B', str(SCRIPT), '_worker', '--runtime', str(runtime),
                       '--parent-pid', str(os.getpid())]
            if platform.WINDOWS:
                # worker 在 stdin barrier 前不得运行任何任务逻辑；先入 job 再放行。
                job = platform.Job()
                try:
                    process = subprocess.Popen(command, stdin=subprocess.PIPE, cwd=ROOT, **platform.background())
                    process._guard_job = job
                    job.assign(process)
                    process.stdin.write(b'1')
                    process.stdin.flush()
                    process.stdin.close()
                except BaseException:
                    job.close()
                    if process is not None:
                        if process.poll() is None:
                            process.kill()
                        process.wait(timeout=10)
                    raise
            else:
                process = subprocess.Popen(command, pass_fds=(lock_fd,), start_new_session=True,
                                           stdin=subprocess.DEVNULL, cwd=ROOT)
            atomic(runtime / 'supervisor.json', {'pid': os.getpid(), 'created': platform.process_identity(os.getpid())})
            atomic(runtime / 'worker.json', {'pid': process.pid})
            while process.poll() is None and not stopped and not (runtime / 'disabled').exists():
                if (runtime / 'watch.log').exists() and (runtime / 'watch.log').stat().st_size > TAIL_BYTES * 2:
                    with (runtime / 'watch.log').open('r+b') as log:
                        log.seek(-TAIL_BYTES, os.SEEK_END)
                        tail = log.read()
                        log.seek(0)
                        log.write(tail)
                        log.truncate()
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
        if owned_lock:
            owned_lock.close()
        else:
            os.close(lock_fd)
        publish_registry(runtime)
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
    with Path(row[0]).open(encoding='utf-8') as source:
        metadata = json.loads(source.readline())['payload']
    if metadata.get('id') != thread or isinstance(metadata.get('source'), dict):
        raise ValueError('只能登记已确认的主线程，不能为子代理另建守护')
    inspected = inspect(agent)
    if Path(inspected.get('Cwd', '/')).resolve() != ROOT or inspected.get('Provider') != 'codex':
        raise ValueError('Paseo目标不是本仓Codex任务')
    # Paseo 在不同系统有不同路径编码；只在已知 agents 层查同 UUID，再验证完整绑定。
    agents = Path(os.environ.get('PASEO_HOME', str(Path.home() / '.paseo'))) / 'agents'
    records = list(agents.glob('*/' + agent + '.json'))
    matching = []
    for candidate in records:
        try:
            value = json.loads(candidate.read_text(encoding='utf-8'))
            if value.get('id') == agent and Path(value.get('cwd', '/')).resolve() == ROOT:
                matching.append(candidate)
        except (ValueError, OSError):
            continue
    if len(matching) != 1:
        raise ValueError('找不到唯一匹配当前项目的Paseo任务记录')
    record = matching[0]
    config = {'agent': agent, 'thread': thread, 'rollout': row[0], 'goals_db': str(home / 'goals_1.sqlite'),
              'agent_record': str(record), 'project': str(ROOT), 'label': args.label or '持续开发任务'}
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
    parser.add_argument('command', choices=['start', 'status', 'list', 'stop', 'finish', '_supervise', '_worker'])
    parser.add_argument('--agent')
    parser.add_argument('--thread')
    parser.add_argument('--label', help='供本地看板显示的简短任务名，不写凭据')
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--lock-fd', type=int)
    parser.add_argument('--parent-pid', type=int)
    parser.add_argument('--stop-task', action='store_true', help='同时暂停Goal并中断主控，须用户明确停止')
    args = parser.parse_args()
    requested_at = time.time()
    if args.command.startswith('_'):
        if not args.runtime or not args.runtime.resolve().is_relative_to(ROOT / '.agents/project/guards'):
            raise SystemExit('无效守护运行目录')
        if args.command == '_worker':
            if platform.WINDOWS and os.read(sys.stdin.fileno(), 1) != b'1':
                raise SystemExit('worker未获监督进程放行')
            worker(args.runtime, args.parent_pid)
        else:
            supervise(args.runtime, args.lock_fd)
        return
    if args.command == 'list':
        base = ROOT / '.agents/project/guards'
        if base.exists():
            publish_registry(base / '_overview')
        print((base / 'registry.json').read_text(encoding='utf-8') if (base / 'registry.json').exists() else '{"guards":[]}')
        return
    agent = args.agent or os.environ.get('PASEO_AGENT_ID')
    if not agent:
        raise SystemExit('缺少Paseo任务身份')
    uuid.UUID(agent)
    runtime = ROOT / '.agents/project/guards' / agent
    if args.command == 'status':
        print(json.dumps(summary(runtime) if runtime.exists() else {'status': 'not_started'}, ensure_ascii=False))
        return
    if args.command == 'start':
        if sys.version_info < (3, 10):
            raise SystemExit('需要已有 Python 3.10+，不会自动安装')
        for executable in ('paseo', 'codex'):
            platform.command([executable])
        # 在任何项目运行状态写入之前，确认现有客户端可用。
        preflight = config_for(args)
        bound_goal(preflight)
    runtime.mkdir(parents=True, exist_ok=True)
    ignore = runtime.parent / '.gitignore'
    if not ignore.exists():
        ignore.write_text('*\n!.gitignore\n', encoding='utf-8')
    if args.command == 'finish' and args.stop_task:
        raise SystemExit('finish只清理完成目标的守护，不能暂停开发任务')
    finish_goal_id = None
    if args.command == 'finish':
        config = json.loads((runtime / 'config.json').read_text(encoding='utf-8'))
        current = goal(config)
        if not current or current['id'] != config['goal_id'] or current['status'] != 'complete':
            raise SystemExit('整体Goal尚未完成，拒绝提前清理守护')
        finish_goal_id = config['goal_id']
    if args.command == 'stop':
        (runtime / 'disabled').touch()
    with (runtime / 'control.lock').open('a') as control:
        platform.lock_file(control)
        if args.command in ('stop', 'finish'):
            if args.command == 'finish':
                config = json.loads((runtime / 'config.json').read_text(encoding='utf-8'))
                current = goal(config)
                if config['goal_id'] != finish_goal_id or not current or current['id'] != finish_goal_id or current['status'] != 'complete':
                    raise SystemExit('目标已变化或未完成，保留当前守护')
            # start可能在本次stop等待控制锁时清过标记；获锁后重新确立停止优先。
            (runtime / 'disabled').touch()
            if args.stop_task:
                config = json.loads((runtime / 'config.json').read_text(encoding='utf-8'))
                if not binding_valid(config):
                    raise SystemExit('任务绑定已改变，已禁用守护但不操作其他线程')
                bound_goal(config)
                rpc('thread/goal/set', {'threadId': config['thread'], 'status': 'paused'})
                bound_goal(config)
                run_json(['paseo', 'stop', agent, '--json'])
                # Paseo发送会等两个安静秒；让已入队操作落定后再确认中断。
                time.sleep(3)
                if inspect(agent).get('Status') == 'running':
                    bound_goal(config)
                    run_json(['paseo', 'stop', agent, '--json'])
                    time.sleep(2)
                    if inspect(agent).get('Status') == 'running':
                        raise SystemExit('守护已禁用，但主控尚未确认停止，需检查客户端状态')
            end = time.monotonic() + 45
            while (runtime / 'supervisor.json').exists() and time.monotonic() < end:
                time.sleep(1)
            if (runtime / 'supervisor.json').exists():
                raise SystemExit('停止请求已记录，但监督进程尚未确认退出')
            state_path = runtime / 'state.json'
            state = json.loads(state_path.read_text(encoding='utf-8')) if state_path.exists() else {}
            state.update(status='complete' if args.command == 'finish' else 'stopped',
                         task_running=False, next_recovery_at=None,
                         reason='complete' if args.command == 'finish' else 'stopped', next_check_at=None, updated_at=datetime.now().astimezone().isoformat())
            atomic(state_path, state)
            publish_registry(runtime)
            print(json.dumps({'status': state['status']}))
            return
        if (runtime / 'disabled').exists() and (runtime / 'disabled').stat().st_mtime >= requested_at:
            raise SystemExit('并发停止优先，未启动守护')
        with (runtime / 'lock').open('a') as lock:
            try:
                platform.lock_file(lock, blocking=False)
            except BlockingIOError:
                print('{"status":"already_running"}')
                return
            config = config_for(args)
            prior_config = json.loads((runtime / 'config.json').read_text(encoding='utf-8')) if (runtime / 'config.json').exists() else {}
            atomic(runtime / 'config.json', config)
            # 用户明确重新授权start，才可清除之前停止标记并恢复暂停的原目标。
            (runtime / 'disabled').unlink(missing_ok=True)
            current_goal = goal(config)
            if not current_goal or current_goal['id'] != config['goal_id']:
                raise SystemExit('原Goal已变化，未启动守护')
            if current_goal['status'] == 'paused':
                remote = bound_goal(config)
                if remote.get('status') != 'paused':
                    raise SystemExit('暂停Goal状态不一致，未恢复任务')
                rpc('thread/goal/set', {'threadId': config['thread'], 'status': 'active'})
            # 同一Goal重启保留退避、语义阻塞及不明确发送记录，只有新目标才重置。
            if prior_config.get('goal_id') != config['goal_id']:
                atomic(runtime / 'state.json', {})
            pending_state = json.loads((runtime / 'state.json').read_text(encoding='utf-8')) if (runtime / 'state.json').exists() else {}
            pending_state.update(status='starting', checked_at=time.time(), updated_at=datetime.now().astimezone().isoformat())
            atomic(runtime / 'state.json', pending_state)
            for name in ('supervisor.json', 'worker.json', 'pid'):
                (runtime / name).unlink(missing_ok=True)
            log_path = runtime / 'watch.log'
            if log_path.exists() and log_path.stat().st_size > TAIL_BYTES:
                with log_path.open('rb') as source:
                    source.seek(-TAIL_BYTES, os.SEEK_END)
                    tail = source.read()
                log_path.write_bytes(tail)
            # Windows 不继承进程字节锁。控制锁保证交接期间无另一 start；
            # supervisor 自行获锁并发布身份，父进程在确认之前不释放控制锁。
            if platform.WINDOWS:
                lock.close()
            with log_path.open('ab') as log:
                kwargs = platform.background()
                if not platform.WINDOWS:
                    kwargs['pass_fds'] = (lock.fileno(),)
                process = subprocess.Popen([sys.executable, '-B', str(SCRIPT), '_supervise', '--runtime', str(runtime),
                                            '--lock-fd', str(lock.fileno() if not platform.WINDOWS else -1)],
                                           stdin=subprocess.DEVNULL, stdout=log, stderr=log, cwd=ROOT, **kwargs)
            lock.close()
            deadline = time.monotonic() + 5
            while not (runtime / 'supervisor.json').exists() and process.poll() is None and time.monotonic() < deadline:
                time.sleep(.1)
            if process.poll() is not None or not (runtime / 'supervisor.json').exists():
                (runtime / 'disabled').touch()
                terminate_child(process)
                raise SystemExit('监督进程未确认启动，已禁用并回收；见watch.log')
            print(json.dumps({'status': 'started', 'pid': process.pid, 'agent': agent, 'thread': config['thread']}))

if __name__ == '__main__':
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8')
    main()
