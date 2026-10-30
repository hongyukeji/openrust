#!/usr/bin/env python3
"""持续开发守护：原目标接续、冷却、单实例监督和按任务停止；仅使用标准库。"""

import argparse
import copy
import hashlib
from datetime import datetime
from decimal import Decimal, InvalidOperation
import math
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
from types import SimpleNamespace
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guard_platform as platform
import guard_questions as questions

ROOT = Path.cwd().resolve()
CHECK_INTERVAL = 120
QUOTA_WAIT_TIMEOUT = 3600
TAIL_BYTES = 512 * 1024
SCRIPT = Path(__file__).resolve()
DELAYS = (60, 180, 600)
PROMPT = (
    "按原目标与项目约束，从当前证据继续；先收件避免重复执行。"
    "已授权且信息充分的普通开发决策直接按推荐方案继续，不弹阻断问答；"
    "缺必要信息不猜填，继续独立工作。沉默不批准权限、认证、凭据、付费、"
    "发布、对外消息或删除；普通问卷由守护按已验证的边界处理。"
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
    terminal = state.get('status') in ('complete', 'paused', 'stopped', 'parent_lost', 'quota_wait_expired')
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


def run_json_command_text(command, **kwargs):
    process = platform.launch_tool(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, **kwargs)
    try:
        output, error = process.communicate(timeout=30)
        if process.returncode:
            raise subprocess.CalledProcessError(process.returncode, command, output, error)
        return output.decode('utf-8')
    finally:
        try:
            terminate_child(process)
        finally:
            for stream in (process.stdin, process.stdout, process.stderr):
                if stream:
                    stream.close()


def run_json(command):
    return json.loads(run_json_command_text(command))


def inspect(agent):
    result = run_json(['paseo', 'inspect', agent, '--json'])
    if result.get('PendingPermissions'):
        questions.enrich_snapshot(result, globals())
    return result


def answer_question(config, request, answer, binding, pending):
    questions.submit(config, request, answer, binding, pending, SimpleNamespace(**globals()))


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
    # 活跃任务的计量会更新时间戳；身份、目标与状态仍须前后完全一致。
    if not after or any(before.get(key) != after.get(key)
                        for key in ('id', 'status', 'created_at_ms', 'objective')):
        raise ValueError('核对期间Goal身份、状态或目标已变化，拒绝操作')
    if not remote:
        raise ValueError('协议未返回原Goal，拒绝操作')
    if remote.get('id') is not None:
        matches = remote['id'] == config['goal_id']
    else:
        matches = (remote.get('threadId') == config['thread']
                   and isinstance(before.get('created_at_ms'), int)
                   and remote.get('createdAt') == before['created_at_ms'] // 1000
                   and remote.get('objective') == before.get('objective'))
    remote_status = {'usageLimited': 'usage_limited', 'budgetLimited': 'budget_limited'}.get(
        remote.get('status'), remote.get('status'))
    if not matches or remote_status != before.get('status'):
        raise ValueError('协议与本地原Goal身份或状态不一致，拒绝操作')
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


def user_text_context(key, message):
    """只缓存有界正文，完整摘要与长度防止裁剪后误认人工消息。"""
    return {'key': key, 'message': message[-2000:] if isinstance(message, str) else None,
            'text_fingerprint': {'digest': questions.digest(message), 'length': len(message)}
            if isinstance(message, str) else None}


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
    user_context = None
    post_user_activity = False
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
                if user_context is not None:
                    post_user_activity = True
                explicit = explicit_goal_status(payload)
                if explicit is not None:
                    semantic_blocked = explicit == 'blocked'
                    goal_control = {'status': explicit, 'key': row.get('timestamp'),
                                    'source': [str(path.resolve()), info.st_ino], 'offset': offset}
            if (row.get('type') == 'response_item' and payload.get('type') == 'message'
                    and payload.get('role') == 'user'):
                # 正式问答steer也可能仅写response_item；精确保留答复，非文本输入不得冒充它。
                parts = payload.get('content')
                plain = (isinstance(parts, list) and bool(parts)
                         and all(isinstance(p, dict) and p.get('type') == 'input_text'
                                 and isinstance(p.get('text'), str) for p in parts))
                message = ''.join(p['text'] for p in parts) if plain else None
                user_context = user_text_context(row.get('timestamp'), message)
                post_user_activity = False
                last = {'kind': 'user_message', 'key': row.get('timestamp'),
                        'payload': {'type': 'user_message', 'message': message}, 'tools': tools}
            if row.get('type') != 'event_msg':
                continue
            kind = payload.get('type')
            if kind == 'user_message':
                attachments = payload.get('images') or payload.get('local_images') or payload.get('attachments')
                user_context = user_text_context(row['timestamp'], None if attachments else payload.get('message'))
                post_user_activity = False
            if kind in ('task_started', 'task_complete', 'turn_aborted', 'user_message'):
                if kind == 'task_started':
                    tools = 0
                last = {'kind': kind, 'key': row['timestamp'], 'payload': payload, 'tools': tools}
    if last is not None:
        last['semantic_blocked'] = semantic_blocked
        last['goal_control'] = goal_control
        last['user_context'] = user_context
        last['post_user_activity'] = post_user_activity
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


def budget_decimal(value):
    """正式额度数值必须有限且非负，不能将未知或NaN当余额。"""
    try:
        result = Decimal(str(value))
    except (InvalidOperation, ValueError) as error:
        raise ValueError('未知额度数值') from error
    if not result.is_finite() or result < 0:
        raise ValueError('额度数值必须有限且非负')
    return result


def initialize_usage_budget(response, now):
    """首次正式启用取真实基准；已耗尽窗口记录但不参与本任务预算。"""
    try:
        limits = response['rateLimits']
        account = response['accountId']
        limit_id = limits['limitId']
        if not isinstance(account, str) or not account or not isinstance(limit_id, str) or not limit_id:
            raise ValueError('额度身份未知')
        if not isinstance(response.get('ordinaryUsageAllowed'), bool):
            raise ValueError('普通额度状态未知')
        windows = {}
        for name in ('primary', 'secondary'):
            window = limits.get(name)
            if window is None:
                continue
            used = budget_decimal(window['usedPercent'])
            if used > 100:
                raise ValueError('窗口百分比不在0至100')
            remaining = Decimal(100) - used
            reset = budget_decimal(window['resetsAt'])
            duration = budget_decimal(window['windowDurationMins'])
            if duration <= 0:
                raise ValueError('窗口时长未知')
            windows[name] = {'baseline_remaining': str(remaining), 'reserve': str(remaining * Decimal('0.2')),
                             'participating': remaining > 0, 'resets_at': str(reset), 'duration_mins': str(duration),
                             'cycle_observed_at': now}
        if not windows:
            raise ValueError('正式窗口未知')
        credits = limits['credits']
        if not isinstance(credits.get('hasCredits'), bool) or not isinstance(credits.get('unlimited'), bool):
            raise ValueError('credits状态未知')
        balance = budget_decimal(credits['balance'])
        return {'version': 1, 'baseline_at': now, 'account_id': account, 'limit_id': limit_id,
                'fraction': '0.8', 'windows': windows,
                'credits': {'baseline_remaining': str(balance), 'reserve': str(balance * Decimal('0.2')),
                            'participating': balance > 0 and credits['hasCredits'],
                            'has_credits': credits['hasCredits'], 'unlimited': credits['unlimited']}}
    except (KeyError, TypeError, InvalidOperation) as error:
        raise ValueError('正式额度基准不完整') from error


def refresh_usage_cycles(runtime, config, response, now):
    """只按正式reset证据推进各窗口周期；守护重启和数值上涨不是重置。"""
    original = config.get('usage_budget')
    if original is None:
        return
    proposed = json.loads(json.dumps(original))
    try:
        limits = response['rateLimits']
        if response['accountId'] != original['account_id'] or limits['limitId'] != original['limit_id']:
            return
        changed = False
        for name, item in proposed['windows'].items():
            window = limits[name]
            new_reset = window['resetsAt']
            duration = window['windowDurationMins']
            # schema明确允许null；只有完整、同类型官方周期身份才可推进。
            if type(new_reset) is not int or type(duration) is not int:
                return
            old_reset = budget_decimal(item['resets_at'])
            if budget_decimal(duration) != budget_decimal(item['duration_mins']):
                return
            used = budget_decimal(window['usedPercent'])
            if used > 100:
                return
            if budget_decimal(new_reset) == old_reset:
                if item.get('awaiting_available') and item.get('previous_resets_at') and used < 100:
                    remaining = Decimal(100) - used
                    item.update(baseline_remaining=str(remaining), reserve=str(remaining * Decimal('0.2')),
                                participating=True, cycle_observed_at=now, awaiting_available=False)
                    changed = True
                continue
            if not (old_reset <= Decimal(str(now)) < new_reset and new_reset > old_reset):
                return
            remaining = Decimal(100) - used
            item.update(baseline_remaining=str(remaining), reserve=str(remaining * Decimal('0.2')),
                        participating=remaining > 0, previous_resets_at=str(old_reset),
                        resets_at=str(new_reset), cycle_observed_at=now, awaiting_available=remaining == 0)
            changed = True
        # 余额或状态未知不能借窗口reset建立可消费基准。
        status = usage_budget_status(response, proposed)
        if changed and status not in ('usage_budget_unknown', 'usage_budget_window_changed', 'usage_budget_credits_changed'):
            config['usage_budget'] = proposed
            atomic(runtime / 'config.json', config)
    except (KeyError, TypeError, ValueError, InvalidOperation):
        return


def refresh_account_budget(runtime, config, response, now):
    """只读取用户已切换的账号；逐账号保留预算，切回不能补满保留线。"""
    original = config.get('usage_budget') or {}
    account = response.get('accountId')
    limits = response.get('rateLimits') or {}
    if (not isinstance(account, str) or not account or account == original.get('account_id')
            or limits.get('limitId') != original.get('limit_id')):
        return False
    try:
        candidate = initialize_usage_budget(response, now)
        # 不把登录失败、缺字段或硬限制当作可用的新账号。
        if quota_delay(response, now, config.get('allow_existing_credits', False), candidate):
            return False
    except (ValueError, TypeError):
        return False
    history = dict(config.get('account_budgets') or {})
    key = lambda budget: json.dumps([budget['account_id'], budget['limit_id']])
    history[key(original)] = original
    candidate = history.get(key(candidate), candidate)
    config.update(usage_budget=candidate, account_budgets=history)
    atomic(runtime / 'config.json', config)
    refresh_usage_cycles(runtime, config, response, now)
    return True


def percentage_budget_breaches(response, budget):
    """仅返回百分比保留线；credits触线同时发生时不授予自动恢复资格。"""
    if budget.get('_full_budget_once') or budget.get('_invalid_grant'):
        # 完整授权由官方资格判定；旧保留线不能再登记为等待reset的原因。
        return {}
    try:
        credits = budget['credits']
        current_credits = response['rateLimits']['credits']
        balance = budget_decimal(current_credits['balance'])
        if (current_credits['hasCredits'] != credits['has_credits']
                or current_credits['unlimited'] != credits['unlimited']
                or balance > budget_decimal(credits['baseline_remaining'])
                or (credits['participating'] and balance <= budget_decimal(credits['reserve']))):
            return {}
        return {name: item['resets_at'] for name, item in budget['windows'].items()
                if item['participating'] and Decimal(100) - budget_decimal(
                    response['rateLimits'][name]['usedPercent']) <= budget_decimal(item['reserve'])}
    except (KeyError, TypeError, ValueError, InvalidOperation):
        return {}


def grant_document(config):
    """临时覆盖独立落盘，结束后保留授权摘要以阻止同次授权重放。"""
    path = config.get('full_budget_grant_file')
    if not path:
        return None, {}
    path = Path(path)
    return path, json.loads(path.read_text(encoding='utf-8')) if path.exists() else {}


def end_full_budget(config, reason):
    """撤销有效覆盖；不触碰原基线、账号历史或问答账本。"""
    path, document = grant_document(config)
    key = document.get('active')
    if path and key:
        document['grants'][key].update(status='ended', ended_at=time.time(), reason=reason)
        document['active'] = None
        atomic(path, document)


def full_budget_end_reason(config):
    """查询最近一次覆盖的结束原因，实际耗尽后不自动跨周期复用授权。"""
    _, document = grant_document(config)
    grants = [g for g in document.get('grants', {}).values()
              if all(g.get(k) == config.get(k) for k in ('agent', 'thread', 'goal_id'))]
    if document.get('active') or not grants:
        return None
    return max(grants, key=lambda g: g['granted_at']).get('reason')


def effective_usage_budget(config):
    """只向本次判定附加授权，不更改持久化的80%预算。"""
    budget = config.get('usage_budget')
    path, document = grant_document(config)
    grant = document.get('grants', {}).get(document.get('active'))
    if not grant or grant.get('status') != 'active':
        return budget
    if any(grant.get(k) != config.get(k) for k in ('agent', 'thread', 'goal_id')):
        end_full_budget(config, 'task_identity_changed')
        return dict(budget or {}, _invalid_grant=True)
    current = goal(config)
    if not current or any(current.get(k) != grant.get('goal', {}).get(k)
                          for k in ('id', 'created_at_ms', 'objective')):
        end_full_budget(config, 'goal_identity_changed')
        return dict(budget or {}, _invalid_grant=True)
    if not budget or any(grant.get(k) != budget.get(k) for k in ('account_id', 'limit_id')):
        end_full_budget(config, 'account_identity_changed')
        return dict(budget or {}, _invalid_grant=True)
    result = copy.deepcopy(budget)
    result['_full_budget_once'] = dict(grant, path=str(path), allow_existing_credits=(
        grant.get('allow_existing_credits') is True and config.get('allow_existing_credits') is True))
    return result


def full_budget_status(response, budget):
    """100%仅覆盖本地保留线；正式资格、未知状态与硬限制继续生效。"""
    grant = budget['_full_budget_once']
    try:
        limits = response['rateLimits']
        if response['accountId'] != grant['account_id'] or limits['limitId'] != grant['limit_id']:
            end_full_budget({'full_budget_grant_file': grant['path']}, 'account_identity_changed')
            return 'usage_budget_unknown'
        # 完整快照验证仍复用基线解析，但不保存或采用新的基线。
        initialize_usage_budget(response, time.time())
        if set(budget['windows']) != {k for k in ('primary', 'secondary') if limits.get(k) is not None}:
            return 'usage_budget_unknown'
        for name, item in budget['windows'].items():
            if budget_decimal(limits[name]['windowDurationMins']) != budget_decimal(item['duration_mins']):
                return 'usage_budget_unknown'
            if budget_decimal(limits[name]['resetsAt']) != budget_decimal(item['resets_at']):
                return 'usage_budget_window_changed'
        credits = limits['credits']
        balance = budget_decimal(credits['balance'])
        if (balance > budget_decimal(grant['credits_balance'])
                or credits['unlimited'] != grant['credits_unlimited']):
            return 'usage_budget_credits_changed'
        paid = (grant['allow_existing_credits'] and credits['hasCredits'] is True
                and (balance > 0 or credits['unlimited'] is True))
        # 不先应用本地reserve，复用官方included/已有credits资格与硬限判断。
        delay = quota_delay(response, time.time(), paid)
        exhausted = (any(budget_decimal(limits[k]['usedPercent']) >= 100 for k in budget['windows'])
                     and response['ordinaryUsageAllowed'] is False and not paid)
        if exhausted:
            end_full_budget({'full_budget_grant_file': grant['path']}, 'actual_quota_exhausted')
        return 'usage_budget_reached' if exhausted else ('usage_budget_unknown' if delay else 'usage_budget_available')
    except (KeyError, TypeError, ValueError, InvalidOperation):
        return 'usage_budget_unknown'


def authorize_full_budget(runtime, config, current, controls, response):
    """CLI显式授权绑定正式用户水位与原Goal；同次已结束授权不能再发放。"""
    if not controls or not controls.get('last_user_at') or not controls.get('user_key'):
        raise ValueError('缺少正式授权会话水位，不授予一次性额度')
    bound_goal(config)
    if goal(config) != current or not binding_valid(config):
        raise ValueError('授权期间原Goal改变')
    initialize_usage_budget(response, time.time())
    budget = config['usage_budget']
    if response['accountId'] != budget['account_id'] or response['rateLimits']['limitId'] != budget['limit_id']:
        raise ValueError('额度账号与原预算不一致')
    path = runtime / 'full-budget-once.json'
    config['full_budget_grant_file'] = str(path)
    _, document = grant_document(config)
    # 水位来自正式快照，调用者不能靠更换任意token绕过单次限制。
    authorization = {k: controls[k] for k in ('last_user_at', 'user_key')}
    identity = {k: current[k] for k in ('id', 'created_at_ms', 'objective')}
    key = hashlib.sha256(json.dumps([config['agent'], config['thread'], identity, authorization],
                                   sort_keys=True).encode()).hexdigest()
    previous = document.get('grants', {}).get(key)
    if previous:
        if previous.get('status') != 'active' or document.get('active') != key:
            raise ValueError('本次授权已结束，不能重复使用')
        return
    if document.get('active'):
        raise ValueError('已有有效的一次性授权，请普通重载沿用')
    credits = response['rateLimits']['credits']
    grant = dict(agent=config['agent'], thread=config['thread'], goal_id=config['goal_id'],
                 goal=identity, authorization=authorization, account_id=budget['account_id'],
                 limit_id=budget['limit_id'], status='active', granted_at=time.time(),
                 allow_existing_credits=config.get('allow_existing_credits') is True,
                 credits_balance=str(budget_decimal(credits['balance'])), credits_unlimited=credits['unlimited'])
    document.setdefault('grants', {})[key] = grant
    document['active'] = key
    atomic(path, document)


def resume_budget_pause(runtime, config, state, requested_at=None):
    """新明确恢复授权只解除守护拥有的原预算暂停，未知操作不重试。"""
    requested_at = time.time() if requested_at is None else requested_at
    def concurrent_stop():
        disabled = runtime / 'disabled'
        return disabled.exists() and disabled.stat().st_mtime >= requested_at
    pause = state.get('percentage_pause') or {}
    current = goal(config)
    if (concurrent_stop() or pause.get('resume_status') not in ('waiting', 'cancelled') or pause.get('goal') != current
            or not current or current.get('status') != 'paused' or not state.get('usage_budget_paused')):
        raise ValueError('非已核实的守护预算暂停，拒绝恢复')
    # bound_goal还核验正式协议status/objective/createdAt与本地原Goal。
    remote = bound_goal(config)
    if (remote.get('objective') != current.get('objective')
            or remote.get('createdAt') != current.get('created_at_ms', -1000) // 1000
            or remote.get('status') != 'paused'):
        raise ValueError('正式原Goal目标、创建时间或状态不匹配')
    controls = percentage_pause_controls(config, allow_old_control=True)
    if not controls:
        raise ValueError('存在权限、未知输入或用户暂停边界，拒绝恢复')
    old = pause.get('controls') or {}
    # 协调消息只推进rollout水位；人类水位不变时必须沿用已验证的本次授权。
    human_before = completed_at({'key': old.get('last_user_at'), 'payload': {}})
    human_after = completed_at({'key': controls.get('last_user_at'), 'payload': {}})
    user_before = completed_at({'key': old.get('user_key'), 'payload': {}})
    user_after = completed_at({'key': controls.get('user_key'), 'payload': {}})
    if (None in (human_before, human_after, user_before, user_after)
            or human_after < human_before or user_after <= user_before):
        raise ValueError('没有新的明确恢复授权水位')
    if human_after == human_before:
        grant = (effective_usage_budget(config) or {}).get('_full_budget_once') or {}
        authorization = grant.get('authorization') or {}
        granted_human = completed_at({'key': authorization.get('last_user_at'), 'payload': {}})
        granted_user = completed_at({'key': authorization.get('user_key'), 'payload': {}})
        if (granted_human != human_after or granted_user is None
                or not user_before < granted_user <= user_after):
            raise ValueError('同人类水位缺少有效的本次恢复授权')
    response = rpc('account/rateLimits/read', {})
    if quota_delay(response, time.time(), config.get('allow_existing_credits', False), effective_usage_budget(config)):
        raise ValueError('实际额度不可用或未知，不恢复原Goal')
    if (concurrent_stop() or goal(config) != current or not binding_valid(config)
            or percentage_pause_controls(config, allow_old_control=True) != controls
            or bound_goal(config).get('status') != 'paused'):
        raise ValueError('恢复前身份或授权水位变化')
    pause['resume_status'] = 'activating'
    atomic(runtime / 'state.json', state)
    rpc('thread/goal/set', {'threadId': config['thread'], 'status': 'active'})
    remote = bound_goal(config)
    active = goal(config)
    if (concurrent_stop() or not active
            or any(active.get(k) != current.get(k) for k in ('id', 'created_at_ms', 'objective'))
            or remote.get('status') != 'active' or remote.get('objective') != current.get('objective')
            or remote.get('createdAt') != current.get('created_at_ms', -1000) // 1000
            or not binding_valid(config)
            or percentage_pause_controls(config, allow_old_control=True) != controls):
        raise ValueError('正式恢复结果未确认，不重复操作')
    # 只清已被明确新授权取代的旧控制；问答、发送和退避记录完全保留。
    control = (timeline(config['rollout']) or {}).get('goal_control')
    if control:
        state['control_watermark'] = dict(control)
    state.pop('goal_control', None)
    state.pop('usage_budget_paused', None)
    state.pop('quota_wait_started_at', None)
    state.pop('quota_check_at', None)
    pause['resume_status'] = 'explicitly_resumed'
    atomic(runtime / 'state.json', state)


def usage_budget_status(response, budget=None):
    """各参与项独立保留20%；重置/身份变化不自动补满本任务预算。"""
    if budget is None:
        return None
    if budget.get('_invalid_grant'):
        return 'usage_budget_unknown'
    if budget.get('_full_budget_once'):
        return full_budget_status(response, budget)
    try:
        limits = response['rateLimits']
        if (budget['version'] != 1 or budget['fraction'] != '0.8'
                or response['accountId'] != budget['account_id'] or limits['limitId'] != budget['limit_id']):
            return 'usage_budget_unknown'
        if {k for k in ('primary', 'secondary') if limits.get(k) is not None} != set(budget['windows']):
            return 'usage_budget_unknown'
        participating = 0
        for name, item in budget['windows'].items():
            if not item['participating']:
                continue
            participating += 1
            window = limits[name]
            if (budget_decimal(window['resetsAt']) != budget_decimal(item['resets_at'])
                    or budget_decimal(window['windowDurationMins']) != budget_decimal(item['duration_mins'])):
                return 'usage_budget_window_changed'
            used = budget_decimal(window['usedPercent'])
            if used > 100:
                return 'usage_budget_unknown'
            remaining = Decimal(100) - used
            if remaining > budget_decimal(item['baseline_remaining']):
                return 'usage_budget_window_changed'
            if remaining <= budget_decimal(item['reserve']):
                return 'usage_budget_reached'
        item = budget['credits']
        credits = limits['credits']
        balance = budget_decimal(credits['balance'])
        if (not isinstance(credits.get('hasCredits'), bool) or not isinstance(credits.get('unlimited'), bool)):
            return 'usage_budget_unknown'
        if (balance > budget_decimal(item['baseline_remaining'])
                or credits['hasCredits'] != item['has_credits'] or credits['unlimited'] != item['unlimited']):
            return 'usage_budget_credits_changed'
        if item['participating']:
            participating += 1
            if balance <= budget_decimal(item['reserve']):
                return 'usage_budget_reached'
        return 'usage_budget_available' if participating else 'usage_budget_reached'
    except (KeyError, TypeError, ValueError, InvalidOperation):
        return 'usage_budget_unknown'


def pause_at_usage_budget(runtime, config):
    """只暂停已确认的本Goal；替换目标、人工暂停与停止标记始终优先。"""
    with (runtime / 'control.lock').open('a') as control:
        platform.lock_file(control)
        if (runtime / 'disabled').exists() or not binding_valid(config):
            return False
        current = goal(config)
        if not current or current['id'] != config['goal_id'] or current['status'] in ('paused', 'complete'):
            return False
        bound_goal(config)
        rpc('thread/goal/set', {'threadId': config['thread'], 'status': 'paused'})
        # 暂停往返后重新核实，绝不中断被替换的新目标。
        current = goal(config)
        if not binding_valid(config) or not current or current['id'] != config['goal_id']:
            return False
        if bound_goal(config).get('status') != 'paused':
            return False
        run_json(['paseo', 'stop', config['agent'], '--json'])
        return True


def percentage_pause_controls(config, allow_old_control=False):
    """原任务用户/控制快照；缺可靠证据不授予自动恢复资格。"""
    agent = inspect(config['agent'])
    if agent.get('Archived') or agent.get('PendingPermissions'):
        return None
    if agent.get('QuestionBinding') is None:
        questions.enrich_snapshot(agent, globals())
    binding = agent.get('QuestionBinding') or {}
    if binding.get('id') != config['agent'] or binding.get('thread') != config['thread']:
        return None
    stamp = binding.get('lastUserMessageAt')
    if not isinstance(stamp, str) or not stamp:
        return None
    try:
        if datetime.fromisoformat(stamp.replace('Z', '+00:00')).tzinfo is None:
            return None
    except ValueError:
        return None
    turn = timeline(config['rollout'])
    if allow_old_control and turn:
        old_control = turn.get('goal_control')
        user_key = (turn.get('user_context') or {}).get('key')
        control_at = completed_at({'key': (old_control or {}).get('key'), 'payload': {}})
        user_at = completed_at({'key': user_key, 'payload': {}})
        if ((turn.get('semantic_blocked') and not old_control)
                or old_control and (control_at is None or user_at is None or control_at >= user_at)):
            return None
    error = json.dumps((turn or {}).get('payload', {}).get('error')).lower()
    if (not turn or (turn.get('semantic_blocked') and not allow_old_control)
            or any(token in error for token in ('unauthorized', 'authentication', 'permission denied', '401', '403'))
            or (not allow_old_control and (turn.get('goal_control') or {}).get('status') in ('paused', 'blocked', 'complete'))):
        return None
    return {'last_user_at': binding.get('lastUserMessageAt'),
            'user_key': (turn.get('user_context') or {}).get('key'),
            'control': turn.get('goal_control'), 'turn_key': turn.get('key'), 'turn_kind': turn.get('kind')}


def login_revision():
    """只看现有登录文件元数据，不读取或保存凭据。"""
    path = Path(os.environ.get('CODEX_HOME', str(Path.home() / '.codex'))) / 'auth.json'
    try:
        info = path.stat()
        return [info.st_mtime_ns, info.st_size, info.st_ino]
    except OSError:
        return None


def update_quota_wait(state, status, reason, now):
    """连续额度等待不因账号切换或进程重启重计，实际恢复后才结束。"""
    if status == 'sent' or reason == 'percentage_resumed' or (status == 'working' and state.get('task_running')):
        state.pop('quota_wait_started_at', None)
    elif (status == 'quota_wait' or reason in ('question_quota_wait', 'budget_limited')
          or reason.startswith('usage_budget_')):
        state.setdefault('quota_wait_started_at', now)


def quota_wait_expired(state, now):
    started = state.get('quota_wait_started_at')
    return isinstance(started, (int, float)) and now - started >= QUOTA_WAIT_TIMEOUT


def budget_wait_deadline(pause, response, now):
    """长期耗尽退避到六小时；正式reset更近时提前复核。"""
    attempts = min(pause.get('wait_attempts', 0) + 1, 8)
    pause['wait_attempts'] = attempts
    delay = min(21600, 300 * 2 ** (attempts - 1))
    windows = (response.get('rateLimits') or {})
    resets = [w.get('resetsAt') for k in ('primary', 'secondary')
              if isinstance(w := windows.get(k), dict)]
    future = [r + 30 - now for r in resets if type(r) is int and r > now]
    if future:
        delay = min(delay, min(future))
    return now + max(300, delay)


def owned_percentage_pause(runtime, state, config, before, windows):
    """正式暂停已确认后才登记自动恢复所有权；不伪造用户暂停来源。"""
    if (not before or (not windows and not config.get('auto_account_recovery'))
            or (state.get('percentage_pause') or {}).get('resume_status') in ('activating', 'sending')):
        return False
    try:
        after = percentage_pause_controls(config)
        current = goal(config)
        stable = ('last_user_at', 'user_key', 'control')
        if (not after or any(before.get(k) != after.get(k) for k in stable)
                or not current or current['id'] != config['goal_id'] or current['status'] != 'paused'):
            return False
        state['percentage_pause'] = {'goal': current, 'controls': after, 'windows': windows,
                                     'resume_status': 'waiting', 'account_id': config.get('usage_budget', {}).get('account_id'),
                                     'account_recovery': config.get('auto_account_recovery', False), 'login_revision': login_revision(),
                                     'next_quota_at': (time.time() + 300 if config.get('auto_account_recovery') else
                                         max(time.time() + 300, min(float(reset) + 30 for reset in windows.values())))}
        atomic(runtime / 'state.json', state)
        return True
    except Exception:
        return False


def monitor_percentage_pause(runtime, state, config, current, now):
    """认证或客户端故障同样休眠退避；未知激活和发送不重试。"""
    try:
        return _monitor_percentage_pause(runtime, state, config, current, now)
    except Exception as error:
        pause = state.get('percentage_pause') or {}
        if (not pause.get('account_recovery') or pause.get('resume_status') != 'waiting'
                or pause.get('goal') != current):
            raise
        pause.update(next_quota_at=budget_wait_deadline(pause, {}, now),
                     last_error_kind=type(error).__name__)
        atomic(runtime / 'state.json', state)
        return 'account_budget_unavailable'


def _monitor_percentage_pause(runtime, state, config, current, now):
    """同一自动百分比暂停低频等正式周期；只恢复一次，未知发送不重试。"""
    pause = state.get('percentage_pause') or {}
    if pause.get('resume_status') != 'waiting' or pause.get('goal') != current:
        return 'percentage_pause_not_owned'
    if (runtime / 'disabled').exists() or not binding_valid(config):
        return 'percentage_pause_not_owned'
    if pause.get('account_recovery'):
        revision = login_revision()
        if revision is not None and revision != pause.get('login_revision'):
            pause.update(login_revision=revision, next_quota_at=0, wait_attempts=0)
            atomic(runtime / 'state.json', state)
        # 长期等待只读本地Goal/停止标记和登录元数据，不启动Paseo或Codex进程。
        if now < pause.get('next_quota_at', 0):
            return 'account_budget_sleep'
    controls = percentage_pause_controls(config)
    if controls != pause.get('controls'):
        pause['resume_status'] = 'cancelled'
        atomic(runtime / 'state.json', state)
        return 'percentage_pause_controls_changed'
    if now < pause.get('next_quota_at', 0):
        return 'percentage_reset_wait'
    if full_budget_end_reason(config) in ('actual_quota_exhausted', 'account_identity_changed',
                                        'task_identity_changed', 'goal_identity_changed'):
        return 'percentage_pause_not_owned'
    response = rpc('account/rateLimits/read', {})
    budget = effective_usage_budget(config)
    if budget.get('_invalid_grant'):
        return 'percentage_pause_not_owned'
    if not budget.get('_full_budget_once'):
        if pause.get('account_recovery'):
            refresh_account_budget(runtime, config, response, now)
        refresh_usage_cycles(runtime, config, response, now)
    apply_task_policy(state, config)
    budget = effective_usage_budget(config)
    full_budget = bool(budget.get('_full_budget_once'))
    status = usage_budget_status(response, budget)
    if full_budget_end_reason(config) == 'actual_quota_exhausted':
        return 'percentage_pause_not_owned'
    if status == 'usage_budget_unknown':
        pause['next_quota_at'] = (budget_wait_deadline(pause, response, now)
                                      if pause.get('account_recovery') else now + 300)
        atomic(runtime / 'state.json', state)
        return 'percentage_reset_wait'
    account_changed = (pause.get('account_recovery') and
                       config['usage_budget']['account_id'] != pause.get('account_id'))
    causes = percentage_budget_breaches(response, budget)
    credits = budget['credits']
    balance = budget_decimal(response.get('rateLimits', {}).get('credits', {}).get('balance'))
    if (status == 'usage_budget_credits_changed'
            or not full_budget and credits['participating'] and balance <= budget_decimal(credits['reserve'])):
        if pause.get('account_recovery'):
            pause['next_quota_at'] = budget_wait_deadline(pause, response, now)
            atomic(runtime / 'state.json', state)
            return 'account_budget_wait'
        pause['resume_status'] = 'credit_reserve_or_state'
        atomic(runtime / 'state.json', state)
        return 'percentage_pause_credit_boundary'
    for name, reset in causes.items():
        pause['windows'].setdefault(name, reset)
    advanced = full_budget or account_changed or all(config['usage_budget']['windows'][name]['resets_at'] != reset
                   and config['usage_budget']['windows'][name]['participating']
                   and not config['usage_budget']['windows'][name].get('awaiting_available')
                   for name, reset in pause['windows'].items())
    # 有效完整授权可在原周期继续；默认保留线恢复仍要求已登记窗口推进。
    if status != 'usage_budget_available' or causes or not advanced or (
            not full_budget and not pause['windows'] and not account_changed and not pause.get('account_recovery')) or quota_delay(
            response, now, config.get('allow_existing_credits', False), effective_usage_budget(config)):
        future = [] if pause.get('account_recovery') else [float(config['usage_budget']['windows'][name]['resets_at']) + 30
                  for name in pause['windows']
                  if (float(config['usage_budget']['windows'][name]['resets_at']) > now
                      and not config['usage_budget']['windows'][name].get('awaiting_available'))]
        pause['next_quota_at'] = (budget_wait_deadline(pause, response, now) if pause.get('account_recovery') else
                                  max(now + 300, min(future) if future else now + 300))
        atomic(runtime / 'state.json', state)
        return 'percentage_reset_wait'
    with (runtime / 'control.lock').open('a') as control:
        platform.lock_file(control)
        if ((runtime / 'disabled').exists() or not binding_valid(config) or goal(config) != current
                or percentage_pause_controls(config) != controls or bound_goal(config).get('status') != 'paused'):
            pause['resume_status'] = 'cancelled'; atomic(runtime / 'state.json', state)
            return 'percentage_pause_controls_changed'
        fresh = rpc('account/rateLimits/read', {})
        if quota_delay(fresh, time.time(), config.get('allow_existing_credits', False), effective_usage_budget(config)):
            fresh_budget = effective_usage_budget(config)
            fresh_status = usage_budget_status(fresh, fresh_budget)
            fresh_credit = fresh.get('rateLimits', {}).get('credits', {})
            try:
                credit_hit = (not fresh_budget.get('_full_budget_once') and credits['participating']
                              and budget_decimal(fresh_credit.get('balance'))
                              <= budget_decimal(credits['reserve']))
            except ValueError:
                credit_hit = False
            if fresh_status == 'usage_budget_credits_changed' or credit_hit:
                if pause.get('account_recovery'):
                    pause['next_quota_at'] = budget_wait_deadline(pause, response, now)
                    atomic(runtime / 'state.json', state)
                    return 'account_budget_wait'
                pause['resume_status'] = 'credit_reserve_or_state'; atomic(runtime / 'state.json', state)
                return 'percentage_pause_credit_boundary'
            pause['next_quota_at'] = (budget_wait_deadline(pause, response, now)
                                      if pause.get('account_recovery') else now + 300); atomic(runtime / 'state.json', state)
            return 'percentage_reset_wait'
        if (goal(config) != current or not binding_valid(config) or (runtime / 'disabled').exists()
                or percentage_pause_controls(config) != controls or bound_goal(config).get('status') != 'paused'):
            pause['resume_status'] = 'cancelled'; atomic(runtime / 'state.json', state)
            return 'percentage_pause_controls_changed'
        # 意图先落盘，Goal激活或继续消息结果未知时不重试。
        pause['resume_status'] = 'activating'; atomic(runtime / 'state.json', state)
        rpc('thread/goal/set', {'threadId': config['thread'], 'status': 'active'})
        if (bound_goal(config).get('status') != 'active' or not binding_valid(config)
                or (runtime / 'disabled').exists() or percentage_pause_controls(config) != controls):
            return 'percentage_resume_unconfirmed'
        active_goal = goal(config)
        fresh = rpc('account/rateLimits/read', {})
        if (goal(config) != active_goal or not binding_valid(config)
                or (runtime / 'disabled').exists() or percentage_pause_controls(config) != controls):
            return 'percentage_resume_unconfirmed'
        if quota_delay(fresh, time.time(), config.get('allow_existing_credits', False), effective_usage_budget(config)):
            # 不发新模型调用；恢复过程中额度变化，重新暂停本Goal。
            pause['resume_status'] = 'quota_changed'; atomic(runtime / 'state.json', state)
            bound_goal(config)
            rpc('thread/goal/set', {'threadId': config['thread'], 'status': 'paused'})
            return 'percentage_resume_quota_changed'
        if (bound_goal(config).get('status') != 'active' or goal(config) != active_goal
                or not binding_valid(config) or (runtime / 'disabled').exists()
                or percentage_pause_controls(config) != controls):
            return 'percentage_resume_unconfirmed'
        pause['resume_status'] = 'sending'; atomic(runtime / 'state.json', state)
        run_json(['paseo', 'send', config['agent'], PROMPT, '--no-wait', '--json'])
        pause['resume_status'] = 'sent'
        state['usage_budget_paused'] = False
        atomic(runtime / 'state.json', state)
        return 'percentage_resumed'


def quota_delay(response, now, allow_existing_credits=False, usage_budget=None):
    """默认只用普通窗口；当前任务明确获准后才消费已有余额，绝不购买。"""
    if usage_budget_status(response, usage_budget) not in (None, 'usage_budget_available'):
        return 300
    if not isinstance(response, dict) or not isinstance(response.get('rateLimits'), dict):
        return 300
    limits = response['rateLimits']
    windows = [limits[k] for k in ('primary', 'secondary') if limits.get(k)]
    if (not windows or any(not isinstance(w, dict) or type(w.get('usedPercent')) not in (int, float)
                           or not math.isfinite(w['usedPercent']) or w['usedPercent'] < 0 for w in windows)
            or limits.get('spendControlReached') is True
            or limits.get('rateLimitReachedType') not in (None, 'rate_limit_reached')):
        return 300
    # 付费资格须有完整官方状态，消费控制和未知硬限制始终优先。
    if allow_existing_credits is True and isinstance(response.get('ordinaryUsageAllowed'), bool):
        credits = limits.get('credits') or {}
        if not isinstance(credits, dict):
            return 300
        try:
            balance = Decimal(str(credits.get('balance')))
            positive = balance.is_finite() and balance > 0
        except (InvalidOperation, ValueError):
            positive = False
        if (limits.get('spendControlReached') is False
                and limits.get('rateLimitReachedType') in (None, 'rate_limit_reached')
                and credits.get('hasCredits') is True
                and isinstance(credits.get('unlimited'), bool)
                and (positive or credits.get('unlimited') is True)):
            return 0
    # 新协议明确要求由后台确认included usage可用，不能仅从百分比推断。
    exhausted = [w for w in windows if w['usedPercent'] >= 100]
    if exhausted:
        resets = [w.get('resetsAt') for w in exhausted]
        if any(type(t) not in (int, float) or not math.isfinite(t) for t in resets):
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
        time.sleep(min(5, max(0, end - time.monotonic())))


def binding_valid(config):
    record = json.loads(Path(config['agent_record']).read_text(encoding='utf-8'))
    persistence = record.get('persistence') or {}
    return (record.get('id') == config['agent'] and record.get('provider') == 'codex'
            and Path(record.get('cwd', '/')).resolve() == ROOT
            and persistence.get('provider') == 'codex'
            and persistence.get('sessionId') == config['thread']
            and persistence.get('metadata', {}).get('threadId') == config['thread'])


def apply_task_policy(state, config):
    """策略变化只失效额度等待；保留退避、发送与问答幂等记录。"""
    policy = {'allow_existing_credits': config.get('allow_existing_credits') is True,
              'usage_budget': effective_usage_budget(config)}
    previous = {'allow_existing_credits': False, 'usage_budget': None}
    previous.update(state.get('quota_policy') or {})
    if policy != previous:
        state.pop('quota_check_at', None)
    state['quota_policy'] = policy


def worker(runtime, parent_pid):
    config = json.loads((runtime / 'config.json').read_text(encoding='utf-8'))
    path = runtime / 'state.json'
    state = json.loads(path.read_text(encoding='utf-8')) if path.exists() else {}
    apply_task_policy(state, config)
    attempts = state.get('attempts', 0)

    def record(status, **extra):
        changed = state.get('status') != status
        state.update(status=status, attempts=attempts, checked_at=time.time(),
                         updated_at=datetime.now().astimezone().isoformat(), **extra)
        if (status in ('complete', 'paused', 'quota_wait_expired')
                or status == 'stopped' and not (runtime / 'disabled').exists()):
            end_full_budget(config, status)
        if status != 'working':
            state['task_running'] = False
        state['reason'] = extra.get('reason', {
            'working': 'task_running' if state.get('task_running') else 'waiting_for_turn',
            'cooldown': 'recovery_cooldown', 'backoff': 'recovery_backoff',
            'quota_wait': 'quota_check_pending', 'needs_attention': 'hard_boundary',
        }.get(status, status))
        update_quota_wait(state, status, state['reason'], time.time())
        state['next_recovery_at'] = (state.get('quota_check_at') if status == 'quota_wait' else
                                     state.get('retry_at') if status in ('backoff', 'cooldown') else None)
        state['next_check_at'] = None if status in ('complete', 'paused', 'stopped', 'parent_lost', 'quota_wait_expired') else time.time() + CHECK_INTERVAL
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
            if quota_wait_expired(state, time.time()):
                # 探测失败也能退出；超时清理不依赖客户端或数据库可用。
                running = False
                try:
                    current = goal(config)
                    if current and current['id'] == config['goal_id'] and current['status'] == 'complete':
                        record('complete')
                        return
                    running = (current and current['id'] == config['goal_id']
                               and current['status'] != 'paused' and binding_valid(config)
                               and inspect(config['agent']).get('Status') == 'running')
                except Exception:
                    pass
                if running:
                    state.pop('quota_wait_started_at', None)
                else:
                    # 只退出守护，不改Goal、不终止业务、不删除防重记录。
                    (runtime / 'disabled').touch()
                    state['quota_wait_expired_at'] = time.time()
                    record('quota_wait_expired')
                    notify('额度等待已达一小时，守护已退出；重新要求继续开发可启动。')
                    return
            terminal = goal(config)
            if terminal and terminal['id'] == config['goal_id'] and terminal['status'] == 'complete':
                record('complete')
                return
            if terminal and terminal['id'] == config['goal_id'] and terminal['status'] == 'paused':
                if (state.get('percentage_pause') or {}).get('resume_status') == 'waiting':
                    outcome = monitor_percentage_pause(runtime, state, config, terminal, time.time())
                    if outcome in ('percentage_pause_not_owned', 'percentage_pause_controls_changed',
                                   'percentage_pause_credit_boundary'):
                        record('paused', reason=outcome)
                        return
                    record('quota_wait', reason=outcome,
                           quota_check_at=(state.get('percentage_pause') or {}).get('next_quota_at', 0))
                    sleep_until(runtime, CHECK_INTERVAL)
                    continue
                record('paused')
                return
            if not binding_valid(config):
                record('needs_attention', reason='task_binding_changed')
                sleep_until(runtime, CHECK_INTERVAL)
                continue
            current = terminal
            if config.get('usage_budget') is not None:
                response = rpc('account/rateLimits/read', {})
                if full_budget_end_reason(config) in ('account_identity_changed', 'task_identity_changed', 'goal_identity_changed'):
                    record('needs_attention', reason='full_budget_identity_changed')
                    sleep_until(runtime, CHECK_INTERVAL)
                    continue
                budget = effective_usage_budget(config)
                if budget.get('_invalid_grant'):
                    record('needs_attention', reason='full_budget_identity_changed')
                    sleep_until(runtime, CHECK_INTERVAL)
                    continue
                if not budget.get('_full_budget_once'):
                    if config.get('auto_account_recovery'):
                        refresh_account_budget(runtime, config, response, time.time())
                    refresh_usage_cycles(runtime, config, response, time.time())
                apply_task_policy(state, config)
                floor = usage_budget_status(response, effective_usage_budget(config))
                state['usage_budget_checked_at'] = time.time()
                if floor in ('usage_budget_reached', 'usage_budget_window_changed', 'usage_budget_credits_changed'):
                    unknown_resume = (state.get('percentage_pause') or {}).get('resume_status') in ('activating', 'sending')
                    windows = (percentage_budget_breaches(response, effective_usage_budget(config))
                               if floor == 'usage_budget_reached' and not unknown_resume else {})
                    before = None
                    if windows or config.get('auto_account_recovery'):
                        try:
                            before = percentage_pause_controls(config)
                        except Exception:
                            pass
                    if (not windows or not before) and state.get('percentage_pause') and not unknown_resume:
                        state['percentage_pause']['resume_status'] = 'superseded_boundary'
                    record('needs_attention', reason=floor, usage_budget_paused=True)
                    if pause_at_usage_budget(runtime, config):
                        if full_budget_end_reason(config) == 'actual_quota_exhausted':
                            record('paused', reason='actual_quota_exhausted')
                            return
                        if not unknown_resume and owned_percentage_pause(runtime, state, config, before, windows):
                            record('quota_wait', reason='percentage_reset_wait', usage_budget_paused=True,
                                   quota_check_at=state['percentage_pause']['next_quota_at'])
                            sleep_until(runtime, CHECK_INTERVAL)
                            continue
                        record('paused', reason=floor, usage_budget_paused=True)
                        return
                    record('needs_attention', reason=floor)
                    sleep_until(runtime, CHECK_INTERVAL)
                    continue
                if floor == 'usage_budget_available' and current and current.get('status') == 'active':
                    state.pop('usage_budget_paused', None)
                if floor != 'usage_budget_available':
                    record('needs_attention', reason=floor)
                    sleep_until(runtime, CHECK_INTERVAL)
                    continue
            agent = inspect(config['agent'])
            state['task_running'] = agent.get('Status') == 'running'
            current = goal(config)
            turn = timeline(config['rollout'])
            turn = controlled_turn(state, turn, current, config)
            resumed = state.get('percentage_pause') or {}
            if resumed.get('resume_status') in ('activating', 'sending'):
                record('needs_attention', reason='percentage_resume_unconfirmed')
                sleep_until(runtime, CHECK_INTERVAL)
                continue
            if (resumed.get('resume_status') == 'sent' and turn
                    and turn.get('key') == (resumed.get('controls') or {}).get('turn_key')):
                record('working' if agent.get('Status') == 'running' else 'needs_attention',
                       reason='awaiting_percentage_resume_turn')
                sleep_until(runtime, CHECK_INTERVAL)
                continue
            action = decision(agent, current, turn, config, time.time())
            if action in ('complete', 'paused', 'stopped'):
                record(action)
                return
            question_status = questions.step(runtime, state, config, agent, current, turn, SimpleNamespace(**globals()))
            if question_status:
                record('needs_attention', reason=question_status)
                sleep_until(runtime, min(CHECK_INTERVAL, questions.WAIT_SECONDS))
                continue
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
            delay = quota_delay(rpc('account/rateLimits/read', {}), time.time(), config.get('allow_existing_credits', False), effective_usage_budget(config))
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
                if config.get('usage_budget') is not None and quota_delay(
                        rpc('account/rateLimits/read', {}), time.time(),
                        config.get('allow_existing_credits', False), effective_usage_budget(config)):
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
                if config.get('usage_budget') is not None and quota_delay(
                        rpc('account/rateLimits/read', {}), time.time(),
                        config.get('allow_existing_credits', False), effective_usage_budget(config)):
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
                time.sleep(5)
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
    parser.add_argument('--allow-existing-credits', action=argparse.BooleanOptionalAction, default=None,
                        help='仅当前任务经用户明确授权消费已有credits，不购买或充值；默认关闭')
    parser.add_argument('--use-full-budget-once', action='store_true',
                        help='仅本次明确授权使用全部可用额度，不更改80%%基准')
    parser.add_argument('--resume-budget-paused', action='store_true',
                        help='新的明确授权恢复守护自身预算暂停的原Goal；不恢复人工暂停')
    parser.add_argument('--end-full-budget-once', action='store_true',
                        help='普通重载时撤销本次覆盖，恢复默认80%%政策')
    parser.add_argument('--question-model', help='仅当前任务的无工具问答分类模型，须来自现有catalog')
    parser.add_argument('--stop-task', action='store_true', help='同时暂停Goal并中断主控，须用户明确停止')
    args = parser.parse_args()
    if ((args.use_full_budget_once or args.resume_budget_paused or args.end_full_budget_once)
            and args.command != 'start'):
        parser.error('一次性额度政策选项仅用于start')
    if args.end_full_budget_once and (args.use_full_budget_once or args.resume_budget_paused):
        parser.error('撤销与授予/恢复不能同时使用')
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
            if args.command == 'finish' or args.stop_task:
                saved_config = json.loads((runtime / 'config.json').read_text(encoding='utf-8'))
                end_full_budget(saved_config, 'complete' if args.command == 'finish' else 'user_stop')
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
            same_goal = prior_config.get('goal_id') == config['goal_id']
            if args.resume_budget_paused and (not same_goal or any(
                    prior_config.get(k) != config.get(k) for k in ('agent', 'thread'))):
                raise SystemExit('原Goal或任务绑定已改变，不写入预算/授权，不恢复其他任务')
            config['allow_existing_credits'] = (args.allow_existing_credits if args.allow_existing_credits is not None
                                                else prior_config.get('allow_existing_credits', False) if same_goal else False)
            config['auto_account_recovery'] = True
            config['full_budget_grant_file'] = str(runtime / 'full-budget-once.json')
            config['account_budgets'] = prior_config.get('account_budgets', {}) if same_goal else {}
            config['usage_budget'] = (prior_config.get('usage_budget') if same_goal else None)
            if args.resume_budget_paused and not args.use_full_budget_once and not (
                    effective_usage_budget(config) or {}).get('_full_budget_once'):
                raise SystemExit('缺少同任务有效的一次性额度授权，不恢复预算暂停')
            if config['usage_budget'] is None:
                config['usage_budget'] = initialize_usage_budget(rpc('account/rateLimits/read', {}), time.time())
            # 旧版窗口基准与全局baseline_at来自同次正式快照；迁移只补已知观测时间，不重算额度。
            for window in config['usage_budget']['windows'].values():
                window.setdefault('cycle_observed_at', config['usage_budget']['baseline_at'])
            config.pop('minimum_existing_credits', None)
            config['question_model'] = (args.question_model or
                                        (prior_config.get('question_model') if same_goal else None) or questions.MODEL)
            current_for_grant = goal(config)
            if args.end_full_budget_once:
                end_full_budget(config, 'explicit_revoke')
            if args.use_full_budget_once:
                if current_for_grant.get('status') == 'paused' and not args.resume_budget_paused:
                    raise SystemExit('暂停Goal须显式使用--resume-budget-paused并核验预算暂停来源')
                controls = percentage_pause_controls(config, allow_old_control=args.resume_budget_paused)
                authorize_full_budget(runtime, config, current_for_grant, controls,
                                      rpc('account/rateLimits/read', {}))
            atomic(runtime / 'config.json', config)
            if args.resume_budget_paused:
                saved = json.loads((runtime / 'state.json').read_text(encoding='utf-8'))
                resume_budget_pause(runtime, config, saved, requested_at)
            # 用户明确重新授权start，才可清除之前停止标记并恢复暂停的原目标。
            (runtime / 'disabled').unlink(missing_ok=True)
            current_goal = goal(config)
            if not current_goal or current_goal['id'] != config['goal_id']:
                raise SystemExit('原Goal已变化，未启动守护')
            if current_goal['status'] == 'paused':
                remote = bound_goal(config)
                saved_state = json.loads((runtime / 'state.json').read_text(encoding='utf-8')) if (runtime / 'state.json').exists() else {}
                monitoring_percentage = (saved_state.get('percentage_pause') or {}).get('resume_status') == 'waiting'
                if effective_usage_budget(config).get('_full_budget_once') and not monitoring_percentage:
                    end_full_budget(config, 'user_pause')
                    raise SystemExit('人工暂停优先；一次性覆盖已结束')
                if not monitoring_percentage and (saved_state.get('usage_budget_paused') or saved_state.get('credit_floor_paused')
                        or saved_state.get('reason') in ('usage_budget_reached', 'credit_floor_reached')):
                    raise SystemExit('任务用量预算已暂停原Goal；守护重启不会自动恢复，请用户明确恢复原Goal后再启动')
                if not monitoring_percentage and config.get('usage_budget') is not None and usage_budget_status(
                        rpc('account/rateLimits/read', {}), effective_usage_budget(config)) != 'usage_budget_available':
                    raise SystemExit('任务用量预算已触发或未知，不恢复暂停目标')
                if remote.get('status') != 'paused':
                    raise SystemExit('暂停Goal状态不一致，未恢复任务')
                if not monitoring_percentage:
                    rpc('thread/goal/set', {'threadId': config['thread'], 'status': 'active'})
            # 同一Goal重启保留退避、语义阻塞及不明确发送记录，只有新目标才重置。
            if prior_config.get('goal_id') != config['goal_id']:
                atomic(runtime / 'state.json', {})
            pending_state = json.loads((runtime / 'state.json').read_text(encoding='utf-8')) if (runtime / 'state.json').exists() else {}
            if pending_state.pop('quota_wait_expired_at', None) is not None:
                pending_state.pop('quota_wait_started_at', None)
                pause = pending_state.get('percentage_pause') or {}
                if pause.get('resume_status') == 'waiting':
                    pause['next_quota_at'] = 0
            apply_task_policy(pending_state, config)
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
