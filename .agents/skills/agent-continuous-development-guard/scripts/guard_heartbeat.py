#!/usr/bin/env python3
"""原生 heartbeat 的本地门禁：只读任务数据库，仅写项目内预算账本。"""
import argparse
import copy
from contextlib import closing
from decimal import Decimal, InvalidOperation
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3
import tempfile
import time
import uuid

from guard import budget_decimal, initialize_usage_budget, quota_delay, usage_budget_status
from guard_platform import lock_file

ROOT = Path.cwd().resolve()
WAIT_TIMEOUT = 3600
MAX_JSON = 1024 * 1024
USAGE_TTL = 120


class UsageSnapshotError(ValueError):
    """只输出固定诊断码，不泄漏官方工具错误正文。"""


def unique_object(pairs):
    """JSON重复键存在歧义，不能依赖解析器以后值覆盖。"""
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('JSON重复键')
        result[key] = value
    return result


def read_json(path):
    with Path(path).open('rb') as source:
        raw = source.read(MAX_JSON + 1)
    if len(raw) > MAX_JSON:
        raise ValueError('JSON过大')
    value = json.loads(raw, object_pairs_hook=unique_object)
    if not isinstance(value, dict):
        raise ValueError('JSON对象未知')
    return value


def atomic(path, value):
    descriptor, temporary = tempfile.mkstemp(prefix=path.name + '.', suffix='.tmp', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'w', encoding='utf-8') as output:
            json.dump(value, output, ensure_ascii=False, indent=2, allow_nan=False)
            output.write('\n')
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def usage_snapshot(path, thread, now):
    """调用Agent应将刚取得的正式工具输出原样封装；JSON本身不是来源签名。"""
    envelope = read_json(path)
    observed = envelope.get('observed_at')
    if (envelope.get('source') != 'codex_app.get_usage_limits'
            or envelope.get('thread') != thread
            or type(observed) not in (int, float) or not math.isfinite(observed)
            or not -5 <= now - observed <= USAGE_TTL
            or not isinstance(envelope.get('payload'), dict)):
        raise UsageSnapshotError('usage_snapshot_invalid')
    return usage_result(envelope['payload'])


def usage_result(payload):
    """兼容正式额度对象及MCP结果；错误、歧义或混合数据不得授予资格。"""
    if 'isError' in payload and type(payload['isError']) is not bool:
        raise UsageSnapshotError('usage_tool_result_invalid')
    if payload.get('isError') is True:
        raise UsageSnapshotError('usage_tool_error')
    if 'content' not in payload and 'structuredContent' not in payload:
        return payload
    if 'rateLimits' in payload or 'accountId' in payload:
        raise UsageSnapshotError('usage_tool_result_invalid')
    structured = payload.get('structuredContent')
    if 'structuredContent' in payload and not isinstance(structured, dict):
        raise UsageSnapshotError('usage_tool_result_invalid')
    content = payload.get('content', [])
    if not isinstance(content, list) or len(content) > 1:
        raise UsageSnapshotError('usage_tool_result_invalid')
    decoded = None
    if content:
        item = content[0]
        if (not isinstance(item, dict) or item.get('type') != 'text'
                or not isinstance(item.get('text'), str)):
            raise UsageSnapshotError('usage_tool_result_invalid')
        try:
            decoded = json.loads(item['text'], object_pairs_hook=unique_object)
        except ValueError as error:
            raise UsageSnapshotError('usage_tool_result_invalid') from error
        if not isinstance(decoded, dict):
            raise UsageSnapshotError('usage_tool_result_invalid')
    if structured is not None and decoded is not None and structured != decoded:
        raise UsageSnapshotError('usage_tool_result_invalid')
    result = structured if structured is not None else decoded
    if not isinstance(result, dict):
        raise UsageSnapshotError('usage_tool_result_invalid')
    # 不递归拆包，不允许内层错误标记绕过外层成功状态。
    if any(key in result for key in ('content', 'structuredContent', 'isError')):
        raise UsageSnapshotError('usage_tool_result_invalid')
    return result


def identity(thread, home):
    """本地正式索引与 rollout 必须交叉证明同项目的主线程。"""
    with closing(sqlite3.connect((home / 'state_5.sqlite').resolve().as_uri() + '?mode=ro', uri=True)) as db:
        row = db.execute('SELECT rollout_path,cwd FROM threads WHERE id=?', (thread,)).fetchone()
    if not row or not isinstance(row[1], str) or Path(row[1]).resolve() != ROOT:
        raise ValueError('线程绑定未知')
    rollout = Path(row[0]).resolve()
    with rollout.open('rb') as source:
        first = source.readline(MAX_JSON + 1)
    if len(first) > MAX_JSON:
        raise ValueError('线程metadata过大')
    envelope = json.loads(first)
    metadata = envelope.get('payload') if isinstance(envelope, dict) else None
    if (not isinstance(metadata, dict) or metadata.get('id') != thread
            or not isinstance(metadata.get('cwd'), str)
            or Path(metadata['cwd']).resolve() != ROOT
            or not isinstance(metadata.get('source'), str) or not metadata['source']):
        raise ValueError('主线程身份未知')
    with closing(sqlite3.connect((home / 'goals_1.sqlite').resolve().as_uri() + '?mode=ro', uri=True)) as db:
        row = db.execute('SELECT goal_id,status,created_at_ms,objective FROM thread_goals WHERE thread_id=?',
                         (thread,)).fetchone()
    if (not row or not isinstance(row[0], str) or not row[0]
            or type(row[2]) is not int or not isinstance(row[3], str)):
        raise ValueError('已有Goal未知')
    goal = {'id': row[0], 'created_at_ms': row[2],
            'objective_sha256': hashlib.sha256(row[3].encode('utf-8')).hexdigest()}
    return {'project': str(ROOT), 'home': str(home), 'rollout': str(rollout), 'thread': thread}, goal, row[1]


def refresh_cycles(response, budget, now):
    """仅采纳官方新周期证据；余额上涨、时长变化或提前reset不生成预算。"""
    proposed = copy.deepcopy(budget)
    limits = response['rateLimits']
    if (response['accountId'] != budget['account_id'] or limits['limitId'] != budget['limit_id']
            or {name for name in ('primary', 'secondary') if limits.get(name) is not None}
            != set(budget['windows'])):
        raise ValueError('周期身份变化')
    credits, original = limits['credits'], budget['credits']
    balance = budget_decimal(credits['balance'])
    if (type(credits.get('hasCredits')) is not bool or type(credits.get('unlimited')) is not bool
            or credits['hasCredits'] != original['has_credits']
            or credits['unlimited'] != original['unlimited']
            or balance > budget_decimal(original['baseline_remaining'])
            or (original['participating'] and balance <= budget_decimal(original['reserve']))):
        raise ValueError('credits基线变化或触线')
    for name, item in proposed['windows'].items():
        current = limits[name]
        reset, duration = current['resetsAt'], current['windowDurationMins']
        if type(reset) is not int or type(duration) is not int:
            raise ValueError('周期字段未知')
        old_reset = budget_decimal(item['resets_at'])
        used = budget_decimal(current['usedPercent'])
        if used > 100 or budget_decimal(duration) != budget_decimal(item['duration_mins']) or reset <= now:
            raise ValueError('周期数值无效或过期')
        remaining = Decimal(100) - used
        if Decimal(reset) == old_reset:
            if item.get('awaiting_available') and item.get('previous_resets_at') and used < 100:
                item.update(baseline_remaining=str(remaining), reserve=str(remaining * Decimal('0.2')),
                            participating=True, cycle_observed_at=now, awaiting_available=False)
            elif remaining > budget_decimal(item['baseline_remaining']):
                raise ValueError('同周期额度上涨')
            continue
        if not old_reset <= Decimal(str(now)) < reset or reset <= old_reset:
            raise ValueError('提前或倒退reset')
        item.update(baseline_remaining=str(remaining), reserve=str(remaining * Decimal('0.2')),
                    participating=remaining > 0, previous_resets_at=str(old_reset),
                    resets_at=str(reset), cycle_observed_at=now, awaiting_available=remaining == 0)
    return proposed


def budget_check(ledger, response, now):
    """账号及limit分别记账；切回仅续用原基准，不叠加付费资格。"""
    candidate = initialize_usage_budget(response, now)
    key = json.dumps([candidate['account_id'], candidate['limit_id']], separators=(',', ':'))
    history = ledger['account_budgets']
    original = history.get(key)
    if original is not None and (not isinstance(original, dict)
            or any(name.startswith('_') for name in original)
            or original.get('fraction') != '0.8'):
        raise ValueError('预算账本无效')
    # 即使当前余额不可用，首次正式基准也保留；重复检查不会补满。
    if original is None:
        history[key] = candidate
        original = candidate
    proposed = refresh_cycles(response, original, now)
    status = usage_budget_status(response, proposed)
    if status not in ('usage_budget_unknown', 'usage_budget_window_changed', 'usage_budget_credits_changed'):
        history[key] = proposed
    ledger['active_budget_key'] = key
    ledger['usage_status'] = status
    return status == 'usage_budget_available' and quota_delay(response, now, False, proposed) == 0


def output(ledger, action, reason, thread):
    key = ledger.get('active_budget_key')
    budget = ledger.get('account_budgets', {}).get(key, {})
    return {'action': action, 'reason': reason, 'thread': thread,
            'goal_id': ledger.get('goal', {}).get('id'), 'automation_id': ledger.get('automation_id'),
            'goal_status': ledger.get('goal_status'), 'last_check': ledger.get('last_check'),
            'budget': {'fraction': '0.8', 'allow_existing_credits': False,
                       'status': ledger.get('usage_status', 'unknown'),
                       'error': ledger.get('usage_error'),
                       'accounts': len(ledger.get('account_budgets', {})),
                       'active_key_sha256': hashlib.sha256(key.encode()).hexdigest()[:16] if key else None,
                       'windows': {name: {field: item.get(field) for field in
                                         ('baseline_remaining', 'reserve', 'resets_at')}
                                   for name, item in budget.get('windows', {}).items()}}}


def run(thread, command, usage_file=None, automation_id=None, now=None, previous_automation_id=None):
    """一次门禁检查；官方自动化的创建、暂停与删除由调用方执行。"""
    thread = str(uuid.UUID(thread))
    if command == 'checkpoint' and os.environ.get('CODEX_THREAD_ID') != thread:
        return output({}, 'wait', 'checkpoint_caller_mismatch', thread)
    now = time.time() if now is None else now
    home = Path(os.environ.get('CODEX_HOME', str(Path.home() / '.codex'))).resolve()
    guards = ROOT / '.agents/project/guards'
    runtime = guards / thread
    if not runtime.resolve().is_relative_to(ROOT):
        raise ValueError('账本路径越界')
    if command != 'init' and not runtime.is_dir():
        return output({}, 'stop', 'ledger_missing', thread)
    runtime.mkdir(parents=True, exist_ok=True)
    try:
        # 运行账本不进入源码提交；已有项目忽略策略由宿主保留。
        with (guards / '.gitignore').open('x', encoding='utf-8') as ignored:
            ignored.write('*\n!.gitignore\n')
    except FileExistsError:
        pass
    path = runtime / 'native-heartbeat.json'
    with (runtime / 'native-heartbeat.lock').open('a') as lock:
        lock_file(lock)
        ledger = read_json(path) if path.exists() else {}
        try:
            binding, current_goal, status = identity(thread, home)
        except (OSError, ValueError, TypeError, sqlite3.Error):
            if ledger:
                ledger.update(terminal_action='stop', terminal_reason='binding_unknown')
                atomic(path, ledger)
            return output(ledger, 'stop', 'binding_unknown', thread)
        if ledger and (ledger.get('binding') != binding or ledger.get('goal') != current_goal):
            ledger.update(terminal_action='stop', terminal_reason='goal_or_binding_changed')
            atomic(path, ledger)
            return output(ledger, 'stop', 'goal_or_binding_changed', thread)
        if status in ('complete', 'paused'):
            action, reason = ('finish', 'goal_complete') if status == 'complete' else ('stop', 'goal_' + str(status))
            if ledger:
                ledger.update(terminal_action=action, terminal_reason=reason, goal_status=status)
                atomic(path, ledger)
            return output(ledger or {'goal': current_goal, 'goal_status': status}, action, reason, thread)
        if ledger.get('terminal_action'):
            return output(ledger, ledger['terminal_action'], ledger['terminal_reason'], thread)
        if not ledger:
            if command != 'init':
                return output({}, 'stop', 'ledger_missing', thread)
            ledger = {'version': 1, 'binding': binding, 'goal': current_goal, 'created_at': now,
                      'automation_id': None, 'fraction': '0.8', 'allow_existing_credits': False,
                      'account_budgets': {}}
        ledger['goal_status'] = status
        if ledger.get('version') != 1 or ledger.get('fraction') != '0.8' or ledger.get('allow_existing_credits') is not False:
            return output(ledger, 'stop', 'ledger_invalid', thread)
        if status not in ('active', 'usage_limited', 'budget_limited'):
            atomic(path, ledger)
            return output(ledger, 'wait', 'goal_state_wait', thread)
        if command in ('bind', 'rebind'):
            if not isinstance(automation_id, str) or not automation_id.strip() or len(automation_id) > 256:
                return output(ledger, 'stop', 'automation_id_invalid', thread)
            current = ledger.get('automation_id')
            if command == 'rebind':
                # 旧ID的正式删除与新ID的目标/授权由调用Agent另存工具回执。
                previous = ledger.get('automation_rebindings', [])
                if not isinstance(previous, list) or any(
                        not isinstance(item, dict)
                        or not isinstance(item.get('from'), str) or not item['from']
                        or not isinstance(item.get('to'), str) or not item['to']
                        or item['from'] == item['to']
                        or type(item.get('at')) not in (int, float) or not math.isfinite(item['at'])
                        for item in previous):
                    return output(ledger, 'stop', 'automation_rebinding_history_invalid', thread)
                repeated = (current == automation_id and previous
                            and previous[-1]['from'] == previous_automation_id
                            and previous[-1]['to'] == automation_id)
                if (not isinstance(previous_automation_id, str) or not previous_automation_id
                        or previous_automation_id == automation_id
                        or (current != previous_automation_id and not repeated)):
                    return output(ledger, 'stop', 'automation_rebind_mismatch', thread)
            with (guards / 'native-heartbeat-binding.lock').open('a') as bindings:
                lock_file(bindings)
                for other in guards.glob('*/native-heartbeat.json'):
                    if other != path and read_json(other).get('automation_id') == automation_id:
                        return output(ledger, 'stop', 'automation_id_other_task', thread)
                if command == 'bind' and current not in (None, automation_id):
                    return output(ledger, 'stop', 'automation_id_changed', thread)
                if command == 'rebind' and current != automation_id:
                    ledger.setdefault('automation_rebindings', []).append(
                        {'from': current, 'to': automation_id, 'at': now})
                ledger['automation_id'] = automation_id
                atomic(path, ledger)
            return output(ledger, 'continue', 'automation_bound', thread)
        if command in ('check', 'checkpoint') and not ledger.get('automation_id'):
            return output(ledger, 'stop', 'automation_unbound', thread)
        available = False
        ledger.pop('usage_error', None)
        try:
            available = budget_check(ledger, usage_snapshot(usage_file, thread, now), now)
        except UsageSnapshotError as error:
            ledger['usage_status'] = 'usage_budget_unknown'
            ledger['usage_error'] = str(error)
        except (OSError, ValueError, TypeError, KeyError, InvalidOperation):
            ledger['usage_status'] = 'usage_budget_unknown'
            ledger['usage_error'] = 'usage_budget_validation_failed'
        if available and status != 'active':
            available = False
            ledger['usage_status'] = 'goal_not_active'
        if available:
            try:
                final_binding, final_goal, final_status = identity(thread, home)
            except (OSError, ValueError, TypeError, sqlite3.Error):
                final_binding, final_goal, final_status = None, None, None
            if (final_binding != binding or final_goal != current_goal or final_status != status):
                ledger.update(terminal_action='stop', terminal_reason='binding_changed_during_check')
                atomic(path, ledger)
                return output(ledger, 'stop', 'binding_changed_during_check', thread)
            ledger.pop('first_wait_at', None)
            action, reason = 'continue', 'usage_available'
        else:
            ledger.setdefault('first_wait_at', now)
            action, reason = 'wait', ledger.get('usage_status', 'usage_budget_unknown')
            if now - ledger['first_wait_at'] >= WAIT_TIMEOUT:
                action, reason = 'stop', 'usage_wait_expired'
                ledger.update(terminal_action=action, terminal_reason=reason)
        ledger['last_check'] = {'at': now, 'mode': command,
                                'caller_thread': os.environ.get('CODEX_THREAD_ID'),
                                'action': action}
        atomic(path, ledger)
        return output(ledger, action, reason, thread)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--thread', required=True)
    commands = parser.add_subparsers(dest='command', required=True)
    for command in ('init', 'check', 'checkpoint'):
        commands.add_parser(command).add_argument('--usage-file', type=Path, required=True)
    commands.add_parser('bind').add_argument('--automation-id', required=True)
    replacement = commands.add_parser('rebind')
    replacement.add_argument('--automation-id', required=True)
    replacement.add_argument('--previous-automation-id', required=True)
    args = parser.parse_args()
    try:
        result = run(args.thread, args.command, getattr(args, 'usage_file', None),
                     getattr(args, 'automation_id', None),
                     previous_automation_id=getattr(args, 'previous_automation_id', None))
    except (OSError, ValueError, TypeError, KeyError, sqlite3.Error):
        result = output({}, 'stop', 'local_evidence_unavailable', args.thread)
    print(json.dumps(result, ensure_ascii=False, allow_nan=False))


if __name__ == '__main__':
    main()
