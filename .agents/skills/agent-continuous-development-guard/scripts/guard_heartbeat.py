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

from guard import (budget_decimal, end_full_budget, full_budget_status, grant_document,
                   initialize_usage_budget, quota_delay, usage_budget_status)
from guard_platform import lock_file

ROOT = Path.cwd().resolve()
WAIT_TIMEOUT = 3600
MAX_JSON = 1024 * 1024
USAGE_TTL = 120
RESET_PRECISION_SECONDS = 60


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


def ledger_serialization(value):
    """限长写入与读取一致，必要时仅收紧空白，不丢账本字段或值。"""
    serialized = json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + '\n'
    if len(serialized.encode()) <= MAX_JSON:
        return serialized
    serialized = json.dumps(value, ensure_ascii=False, separators=(',', ':'), allow_nan=False) + '\n'
    if len(serialized.encode()) > MAX_JSON:
        raise ValueError('JSON过大')
    return serialized


def atomic(path, value):
    serialized = ledger_serialization(value)
    descriptor, temporary = tempfile.mkstemp(prefix=path.name + '.', suffix='.tmp', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'w', encoding='utf-8') as output:
            output.write(serialized)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def usage_snapshot(path, thread, now):
    """调用Agent应将刚取得的正式工具输出原样封装；JSON本身不是来源签名。"""
    return usage_envelope(read_json(path), thread, now)


def usage_envelope(envelope, thread, now):
    """校验同一次限长读取的正式envelope，避免审计时再次打开路径。"""
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


def refresh_cycles(response, budget, now, full_budget=False):
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
            or (not full_budget and original['participating'] and balance <= budget_decimal(original['reserve']))):
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
        # 正式接口重置时间有整秒抖动；允许一分钟精度差，固定原锚点不累积漂移。
        same_cycle = (Decimal(reset) == old_reset or
                      (abs(Decimal(reset) - old_reset) <= RESET_PRECISION_SECONDS
                       and Decimal(str(now)) < min(old_reset, Decimal(reset))))
        if same_cycle:
            if item.get('awaiting_available') and item.get('previous_resets_at') and used < 100:
                item.update(baseline_remaining=str(remaining), reserve=str(remaining * Decimal('0.2')),
                            participating=True, cycle_observed_at=now, awaiting_available=False)
            elif remaining > budget_decimal(item['baseline_remaining']):
                raise ValueError('同周期额度上涨')
            continue
        if abs(Decimal(reset) - old_reset) <= RESET_PRECISION_SECONDS:
            raise ValueError('相邻秒跨过期边界未知')
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
    config = {'full_budget_grant_file': ledger.get('full_budget_grant_file')}
    path, document = grant_document(config)
    grant = document.get('grants', {}).get(document.get('active'))
    full_budget = bool(grant and grant.get('status') == 'active')
    if full_budget and (grant.get('thread') != ledger['binding']['thread']
            or grant.get('goal') != ledger['goal']
            or grant.get('account_id') != candidate['account_id']
            or grant.get('limit_id') != candidate['limit_id']):
        end_full_budget(config, 'account_or_goal_identity_changed')
        ledger['usage_status'] = 'usage_budget_unknown'
        return False
    proposed = refresh_cycles(response, original, now, full_budget)

    # 仅在 refresh_cycles 已通过全部校验后统一精度；不改原始额度回执。
    normalized = copy.deepcopy(response)
    for name, item in proposed['windows'].items():
        normalized['rateLimits'][name]['resetsAt'] = int(budget_decimal(item['resets_at']))
    if full_budget:
        effective = copy.deepcopy(proposed)
        effective['_full_budget_once'] = dict(grant, path=str(path), allow_existing_credits=False)
        status = full_budget_status(normalized, effective)
    else:
        status = usage_budget_status(normalized, proposed)
    if status not in ('usage_budget_unknown', 'usage_budget_window_changed', 'usage_budget_credits_changed'):
        history[key] = proposed
    ledger['active_budget_key'] = key
    ledger['usage_status'] = status
    ledger['full_budget_status'] = 'active' if full_budget and grant_document(config)[1].get('active') else 'inactive'
    return status == 'usage_budget_available' and quota_delay(normalized, now, False, effective if full_budget else proposed) == 0


def full_budget_state(ledger):
    """结束回执以独立一次性授权文档为准，不展示旧缓存的active。"""
    try:
        _, document = grant_document(ledger)
        grant = document.get('grants', {}).get(document.get('active'))
        return 'active' if grant and grant.get('status') == 'active' else 'inactive'
    except (OSError, ValueError, TypeError, KeyError):
        return 'unknown'


def output(ledger, action, reason, thread):
    key = ledger.get('active_budget_key')
    budget = ledger.get('account_budgets', {}).get(key, {})
    recovery = 'none'
    diagnostic = ledger.get('usage_error') if reason == 'usage_budget_unknown' else reason
    if diagnostic in ('identity_temporarily_unavailable', 'usage_tool_error', 'usage_snapshot_invalid'):
        recovery = 'refresh_official_evidence'
    elif diagnostic in ('usage_tool_result_invalid', 'usage_budget_validation_failed'):
        recovery = 'diagnose_guard_then_recheck'
    elif reason == 'goal_state_wait' and ledger.get('goal_status') == 'blocked':
        recovery = 'official_goal_resume_required'
    elif reason in ('usage_budget_reached', 'usage_budget_unavailable', 'usage_wait_expired'):
        recovery = 'wait_for_official_eligibility'
    elif reason == 'usage_budget_unknown':
        recovery = 'diagnose_guard_then_recheck'
    return {'action': action, 'reason': reason, 'recovery': recovery, 'thread': thread,
            'goal_id': ledger.get('goal', {}).get('id'), 'automation_id': ledger.get('automation_id'),
            'goal_status': ledger.get('goal_status'), 'last_check': ledger.get('last_check'),
            'budget': {'fraction': '0.8', 'allow_existing_credits': False,
                       'full_budget_once': full_budget_state(ledger),
                       'status': ledger.get('usage_status', 'unknown'),
                       'error': ledger.get('usage_error'),
                       'accounts': len(ledger.get('account_budgets', {})),
                       'active_key_sha256': hashlib.sha256(key.encode()).hexdigest()[:16] if key else None,
                       'windows': {name: {field: item.get(field) for field in
                                         ('baseline_remaining', 'reserve', 'resets_at')}
                                   for name, item in budget.get('windows', {}).items()}}}


def authorize_full_budget(ledger, runtime, thread, binding, current_goal, status,
                          home, usage_file, evidence_file, now, authorization):
    """调用者核实正式真人来源后显式登记；摘要不是来源签名，不修改历史预算。"""
    if (status != 'active' or not ledger.get('automation_id')
            or ledger.get('terminal_reason') not in (None, 'usage_wait_expired')):
        return output(ledger, 'stop', 'full_budget_state_rejected', thread)
    try:
        evidence = read_json(evidence_file)
        observed = evidence.get('observed_at')
        if (evidence.get('source') != 'codex_app.read_thread'
                or evidence.get('target_thread') != thread
                or type(observed) not in (int, float) or not math.isfinite(observed)
                or not -5 <= now - observed <= USAGE_TTL
                or not isinstance(authorization, str) or not authorization.strip()
                or len(authorization) > 4096 or evidence.get('user_text') != authorization):
            raise ValueError('授权来源无效')
        provenance = {key: str(uuid.UUID(evidence[key])) for key in
                      ('source_thread', 'turn_id', 'user_message_id')}
        provenance['authorization_sha256'] = hashlib.sha256(authorization.encode()).hexdigest()
        rebase_history = ledger.get('percentage_rebases', [])
        if not isinstance(rebase_history, list):
            raise ValueError('审计历史无效')
        if any(all(x.get('authorization', {}).get(k) == provenance[k]
                   for k in ('source_thread', 'turn_id', 'user_message_id')) for x in rebase_history):
            return output(ledger, 'stop', 'full_budget_authorization_consumed', thread)
        response = usage_snapshot(usage_file, thread, now)
        candidate = initialize_usage_budget(response, now)
        key = json.dumps([candidate['account_id'], candidate['limit_id']], separators=(',', ':'))
        if key != ledger.get('active_budget_key') or key not in ledger['account_budgets']:
            raise ValueError('账号不匹配')
        # 完整授权同样不能使官方已拒绝的普通额度或未授权 credits 可用。
        if quota_delay(response, now, False) != 0:
            return output(ledger, 'wait', 'full_budget_official_usage_unavailable', thread)
        path = runtime / 'full-budget-once.json'
        config = {'full_budget_grant_file': str(path)}
        _, document = grant_document(config)
        source_identity = {key: provenance[key] for key in ('source_thread', 'turn_id', 'user_message_id')}
        grant_key = hashlib.sha256(json.dumps([binding, current_goal, source_identity], sort_keys=True).encode()).hexdigest()
        previous = document.get('grants', {}).get(grant_key)
        if previous and previous.get('authorization') != provenance:
            return output(ledger, 'stop', 'full_budget_evidence_invalid', thread)
        if previous and (previous.get('status') != 'active' or document.get('active') != grant_key):
            return output(ledger, 'stop', 'full_budget_authorization_consumed', thread)
        if document.get('active') not in (None, grant_key):
            return output(ledger, 'stop', 'full_budget_already_active', thread)
        if identity(thread, home) != (binding, current_goal, 'active'):
            return output(ledger, 'stop', 'full_budget_identity_changed', thread)
        credits = response['rateLimits']['credits']
        if not previous:
            document.setdefault('grants', {})[grant_key] = {
                'thread': thread, 'goal': current_goal, 'authorization': provenance,
                'account_id': candidate['account_id'], 'limit_id': candidate['limit_id'],
                'status': 'active', 'granted_at': now, 'allow_existing_credits': False,
                'previous_terminal_reason': ledger.get('terminal_reason'),
                'credits_balance': str(budget_decimal(credits['balance'])),
                'credits_unlimited': credits['unlimited']}
        document['active'] = grant_key
        atomic(path, document)
        ledger['full_budget_grant_file'] = str(path)
        ledger['full_budget_status'] = 'active'
        ledger.pop('terminal_action', None)
        ledger.pop('terminal_reason', None)
        ledger.pop('first_wait_at', None)
        ledger['last_check'] = {'at': now, 'mode': 'authorize-full-budget',
                                'caller_thread': thread, 'action': 'wait'}
        return output(ledger, 'wait', 'full_budget_authorized_checkpoint_required', thread)
    except (OSError, ValueError, TypeError, KeyError, InvalidOperation):
        return output(ledger, 'stop', 'full_budget_evidence_invalid', thread)


def resume_ledger(ledger, thread, binding, current_goal, status, home, usage_file, now, authorization):
    """原线程在真人明确恢复后解除可恢复停态；不修改 Goal 或重建预算。"""
    if (not isinstance(authorization, str) or not authorization.strip()
            or len(authorization) > 4096):
        return output(ledger, 'stop', 'resume_authorization_required', thread)
    if (status != 'active' or ledger.get('terminal_action') != 'stop'
            or ledger.get('terminal_reason') not in ('goal_paused', 'usage_wait_expired')):
        return output(ledger, 'stop', 'resume_state_rejected', thread)
    if (ledger.get('version') != 1 or ledger.get('fraction') != '0.8'
            or ledger.get('allow_existing_credits') is not False
            or not ledger.get('automation_id')):
        return output(ledger, 'stop', 'ledger_invalid', thread)
    candidate = copy.deepcopy(ledger)
    try:
        available = budget_check(candidate, usage_snapshot(usage_file, thread, now), now)
    except UsageSnapshotError as error:
        candidate['usage_status'] = 'usage_budget_unknown'
        candidate['usage_error'] = str(error)
        return output(candidate, 'wait', 'resume_usage_unavailable', thread)
    except (OSError, ValueError, TypeError, KeyError, InvalidOperation):
        return output(ledger, 'wait', 'resume_usage_unavailable', thread)
    if not available:
        return output(candidate, 'wait', candidate.get('usage_status', 'usage_budget_unknown'), thread)
    if identity(thread, home) != (binding, current_goal, 'active'):
        return output(ledger, 'stop', 'resume_identity_changed', thread)
    history = candidate.get('explicit_resumptions', [])
    if not isinstance(history, list):
        return output(ledger, 'stop', 'resume_history_invalid', thread)
    history.append({'at': now, 'previous_reason': ledger['terminal_reason'],
                    'previous_account_budgets': copy.deepcopy(ledger['account_budgets']),
                    'authorization_sha256': hashlib.sha256(authorization.encode()).hexdigest()})
    candidate['explicit_resumptions'] = history
    candidate.pop('terminal_action', None)
    candidate.pop('terminal_reason', None)
    candidate.pop('first_wait_at', None)
    candidate.pop('usage_error', None)
    candidate['goal_status'] = 'active'
    candidate['last_check'] = {'at': now, 'mode': 'resume', 'caller_thread': thread,
                               'action': 'continue'}
    checkpoint_required = candidate.get('goal_revision_checkpoint_required_at') is not None
    if checkpoint_required:
        candidate['last_check']['action'] = 'wait'
        if not revision_ledger_fits(candidate):
            return output(ledger, 'stop', 'goal_revision_ledger_limit', thread)
    ledger.clear()
    ledger.update(candidate)
    if checkpoint_required:
        return output(ledger, 'wait', 'goal_revision_checkpoint_required', thread)
    return output(ledger, 'continue', 'explicit_user_resume', thread)


def rebase_budget(ledger, thread, binding, current_goal, status, home,
                  usage_file, evidence_file, now, authorization, clock=None,
                  end_full_budget_once=False):
    """真人显式重建百分比基线；完整历史与credits基线不变。"""
    if (status != 'active' or ledger.get('version') != 1
            or ledger.get('fraction') != '0.8' or ledger.get('allow_existing_credits') is not False
            or not ledger.get('automation_id')
            or ledger.get('terminal_reason') not in (None, 'goal_paused', 'usage_wait_expired')
            or (full_budget_state(ledger) == 'active' and not end_full_budget_once)):
        return output(ledger, 'stop', 'rebase_state_rejected', thread)
    try:
        evidence = read_json(evidence_file)
        observed = evidence.get('observed_at')
        if (evidence.get('source') != 'codex_app.read_thread'
                or evidence.get('target_thread') != thread
                or type(observed) not in (int, float) or not math.isfinite(observed)
                or not -5 <= now - observed <= USAGE_TTL
                or not isinstance(authorization, str) or not authorization.strip()
                or len(authorization) > 4096 or evidence.get('user_text') != authorization):
            raise ValueError('授权来源无效')
        provenance = {key: str(uuid.UUID(evidence[key])) for key in
                      ('source_thread', 'turn_id', 'user_message_id')}
        provenance['authorization_sha256'] = hashlib.sha256(authorization.encode()).hexdigest()
        history = ledger.get('percentage_rebases', [])
        if not isinstance(history, list) or any(not isinstance(x, dict) for x in history):
            raise ValueError('审计历史无效')
        if any(all(x.get('authorization', {}).get(k) == provenance[k]
                   for k in ('source_thread', 'turn_id', 'user_message_id')) for x in history):
            return output(ledger, 'stop', 'rebase_authorization_consumed', thread)
        _, grants = grant_document(ledger)
        if any(all((x.get('authorization') or {}).get(k) == provenance[k]
                   for k in ('source_thread', 'turn_id', 'user_message_id'))
               for x in grants.get('grants', {}).values()):
            return output(ledger, 'stop', 'rebase_authorization_consumed', thread)
        envelope = read_json(usage_file)
        response = usage_envelope(envelope, thread, now)
        fresh = initialize_usage_budget(response, now)
        key = json.dumps([fresh['account_id'], fresh['limit_id']], separators=(',', ':'))
        if key != ledger.get('active_budget_key') or key not in ledger['account_budgets']:
            raise ValueError('账号不匹配')
        old = ledger['account_budgets'][key]
        active_grant_key = grants.get('active')
        active_grant = grants.get('grants', {}).get(active_grant_key)
        if active_grant_key is not None and (
                not isinstance(active_grant, dict) or active_grant.get('status') != 'active'
                or active_grant.get('thread') != thread
                or active_grant.get('goal') != current_goal
                or active_grant.get('account_id') != fresh['account_id']
                or active_grant.get('limit_id') != fresh['limit_id']):
            raise ValueError('额度覆盖身份变化')
        if (set(fresh['windows']) != set(old['windows'])
                or any(x['duration_mins'] != old['windows'][name]['duration_mins']
                       for name, x in fresh['windows'].items())):
            raise ValueError('窗口身份变化')
        if quota_delay(response, now, False) != 0:
            return output(ledger, 'wait', 'rebase_official_usage_unavailable', thread)
        candidate = copy.deepcopy(ledger)
        budget = copy.deepcopy(old)
        budget['windows'] = fresh['windows']
        budget['baseline_at'] = fresh['baseline_at']
        candidate['account_budgets'][key] = budget
        grant_file = candidate.pop('full_budget_grant_file', None)
        available = budget_check(candidate, response, now)
        if grant_file is not None:
            candidate['full_budget_grant_file'] = grant_file
        if not available:
            return output(candidate, 'wait', candidate.get('usage_status', 'usage_budget_unknown'), thread)
        candidate['percentage_rebases'] = copy.deepcopy(history) + [{
            'at': now, 'authorization': provenance,
            'previous_account_budgets': copy.deepcopy(ledger['account_budgets']),
            'previous_terminal_reason': ledger.get('terminal_reason'),
            'validated_usage_envelope_sha256': hashlib.sha256(json.dumps(
                envelope, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()}]
        candidate.pop('terminal_action', None)
        candidate.pop('terminal_reason', None)
        candidate.pop('first_wait_at', None)
        candidate.pop('usage_error', None)
        candidate['goal_status'] = 'active'
        candidate['last_check'] = {'at': now, 'mode': 'rebase-budget',
                                   'caller_thread': thread, 'action': 'wait'}
        if len((json.dumps(candidate, ensure_ascii=False, indent=2, allow_nan=False) + '\n').encode()) > MAX_JSON:
            return output(ledger, 'stop', 'rebase_ledger_limit', thread)
        if identity(thread, home) != (binding, current_goal, 'active'):
            return output(ledger, 'stop', 'rebase_identity_changed', thread)
        final_now = now if clock is None else clock()
        if not -5 <= final_now - observed <= USAGE_TTL:
            raise ValueError('授权已过期')
        usage_envelope(envelope, thread, final_now)
        if active_grant is not None:
            end_full_budget(ledger, 'explicit_revoke')
        ledger.clear()
        ledger.update(candidate)
        return output(ledger, 'wait', 'budget_rebased_checkpoint_required', thread)
    except UsageSnapshotError as error:
        result = output(ledger, 'wait', 'rebase_usage_unavailable', thread)
        result['budget']['error'] = str(error)
        return result
    except (OSError, ValueError, TypeError, KeyError, InvalidOperation):
        return output(ledger, 'stop', 'rebase_evidence_invalid', thread)



def revision_ledger_fits(value):
    """所有新增修订写入与下次限长读取使用同一序列化上限。"""
    try:
        ledger_serialization(value)
        return True
    except (ValueError, TypeError):
        return False


def poison_goal_revision_origin(ledger):
    """污染来源收缩为不可恢复最小记录，不删预算或审计历史。"""
    origin = ledger.get('goal_mismatch_origin')
    if not isinstance(origin, dict) or origin.get('eligible') is not True:
        return False
    candidate = copy.deepcopy(ledger)
    candidate['goal_mismatch_origin'] = {'eligible': False}
    if not revision_ledger_fits(candidate):
        # 极小畸形来源原本缺完整归因，不能采用；不为一个布尔值扩坏合法旧账本。
        return False
    ledger.clear()
    ledger.update(candidate)
    return True


def accept_goal_revision(ledger, thread, binding, current_goal, status, home,
                         evidence_file, expected_hash, authorization, now, clock=None):
    """显式采纳同一Goal正文修订；调用Agent先核真人来源，参数不是授权证明。"""
    if (not isinstance(authorization, str) or not authorization.strip()
            or len(authorization) > 4096):
        return output(ledger, 'stop', 'goal_revision_authorization_required', thread)
    if (ledger.get('version') != 1 or ledger.get('fraction') != '0.8'
            or ledger.get('allow_existing_credits') is not False
            or not isinstance(ledger.get('automation_id'), str) or not ledger['automation_id'].strip()
            or status != 'active' or ledger.get('binding') != binding):
        return output(ledger, 'stop', 'goal_revision_state_rejected', thread)
    old_goal = ledger.get('goal')
    if (not isinstance(old_goal, dict) or not isinstance(current_goal, dict)
            or set(old_goal) != {'id', 'created_at_ms', 'objective_sha256'}
            or set(current_goal) != set(old_goal)
            or not isinstance(old_goal['id'], str) or not old_goal['id']
            or type(old_goal['created_at_ms']) is not int
            or type(current_goal['created_at_ms']) is not int
            or {k: v for k, v in old_goal.items() if k != 'objective_sha256'}
            != {k: v for k, v in current_goal.items() if k != 'objective_sha256'}):
        return output(ledger, 'stop', 'goal_revision_identity_rejected', thread)
    try:
        history = ledger.get('goal_revisions', [])
        if not isinstance(history, list) or any(not isinstance(item, dict) for item in history):
            raise ValueError('修订历史无效')
        valid_hash = lambda value: (isinstance(value, str) and len(value) == 64
                                    and all(c in '0123456789abcdef' for c in value))
        if not all(valid_hash(value) for value in
                   (expected_hash, old_goal['objective_sha256'], current_goal['objective_sha256'])):
            raise ValueError('目标摘要无效')
        # 明确拒绝已采用的同次重放，不重写账本，也不解除后来新增的其他停态。
        if old_goal == current_goal and any(
                item.get('previous_goal', {}).get('objective_sha256') == expected_hash
                and item.get('accepted_goal') == current_goal for item in history
                if isinstance(item.get('previous_goal'), dict)):
            origin = ledger.get('goal_mismatch_origin')
            if ((ledger.get('terminal_action') is not None or ledger.get('terminal_reason') is not None)
                    and (ledger.get('terminal_action'), ledger.get('terminal_reason'))
                    != ('stop', 'goal_or_binding_changed')) or (
                    isinstance(origin, dict) and origin.get('eligible') is False):
                return output(ledger, 'stop', 'goal_revision_state_rejected', thread)
            return output(ledger, 'stop', 'goal_revision_already_accepted', thread)
        if (expected_hash != old_goal['objective_sha256']
                or current_goal['objective_sha256'] == old_goal['objective_sha256']):
            return output(ledger, 'stop', 'goal_revision_old_hash_mismatch', thread)
        if (ledger.get('terminal_action') != 'stop'
                or ledger.get('terminal_reason') != 'goal_or_binding_changed'):
            return output(ledger, 'stop', 'goal_revision_state_rejected', thread)
        origin = ledger.get('goal_mismatch_origin')
        if (not isinstance(origin, dict) or origin.get('eligible') is not True
                or origin.get('observed_status') != 'active' or origin.get('previous_goal') != old_goal
                or origin.get('binding') != binding
                or 'previous_terminal_action' not in origin or origin['previous_terminal_action'] is not None
                or 'previous_terminal_reason' not in origin or origin['previous_terminal_reason'] is not None
                or type(origin.get('at')) not in (int, float) or not math.isfinite(origin['at'])):
            return output(ledger, 'stop', 'goal_revision_origin_rejected', thread)
        # 原始正式get_goal envelope限长只读一次；digest与验证必须来自同一份字节。
        with Path(evidence_file).open('rb') as source:
            raw = source.read(MAX_JSON + 1)
        if len(raw) > MAX_JSON:
            raise ValueError('Goal证据过大')
        evidence = json.loads(raw, object_pairs_hook=unique_object)
        if not isinstance(evidence, dict):
            raise ValueError('Goal证据未知')
        observed = evidence.get('observed_at')
        payload = evidence.get('payload')
        goal = payload.get('goal') if isinstance(payload, dict) else None
        if (evidence.get('source') != 'get_goal' or evidence.get('thread') != thread
                or type(observed) not in (int, float) or not math.isfinite(observed)
                or not -5 <= now - observed <= USAGE_TTL
                or not isinstance(payload, dict)
                or not {'goal', 'remainingTokens', 'completionBudgetReport'} <= set(payload)
                or any(key in payload for key in ('isError', 'content', 'structuredContent', 'error'))
                or not isinstance(goal, dict) or goal.get('threadId') != thread
                or goal.get('status') != 'active' or not isinstance(goal.get('objective'), str)
                or any(key in goal and goal[key] != current_goal['id'] for key in ('id', 'goalId'))
                or hashlib.sha256(goal['objective'].encode()).hexdigest() != current_goal['objective_sha256']
                or type(goal.get('createdAt')) is not int
                or goal['createdAt'] != current_goal['created_at_ms'] // 1000):
            raise ValueError('Goal证据无效')
        candidate = copy.deepcopy(ledger)
        candidate['goal'] = copy.deepcopy(current_goal)
        candidate['goal_status'] = 'active'
        candidate.pop('terminal_action', None)
        candidate.pop('terminal_reason', None)
        candidate['goal_revisions'] = copy.deepcopy(history) + [{
            'at': now, 'previous_goal': copy.deepcopy(old_goal),
            'accepted_goal': copy.deepcopy(current_goal),
            'goal_evidence_sha256': hashlib.sha256(raw).hexdigest(),
            'authorization_sha256': hashlib.sha256(authorization.encode()).hexdigest()}]
        candidate['last_check'] = {'at': now, 'mode': 'accept-goal-revision',
                                   'caller_thread': thread, 'action': 'wait'}
        candidate['goal_revision_checkpoint_required_at'] = now
        if len((json.dumps(candidate, ensure_ascii=False, indent=2, allow_nan=False) + '\n').encode()) > MAX_JSON:
            return output(ledger, 'stop', 'goal_revision_ledger_limit', thread)
        try:
            final_identity = identity(thread, home)
        except (ValueError, TypeError):
            final_identity = None
        if final_identity != (binding, current_goal, 'active'):
            # 同一active Goal再次纯正文编辑只使本次证据过时，不污染可刷新来源。
            if (isinstance(final_identity, tuple) and len(final_identity) == 3
                    and final_identity[0] == binding and final_identity[2] == 'active'
                    and isinstance(final_identity[1], dict)
                    and set(final_identity[1]) == set(current_goal)
                    and {k: v for k, v in final_identity[1].items() if k != 'objective_sha256'}
                    == {k: v for k, v in current_goal.items() if k != 'objective_sha256'}
                    and valid_hash(final_identity[1].get('objective_sha256'))):
                return output(ledger, 'stop', 'goal_revision_objective_changed', thread)
            poison_goal_revision_origin(ledger)
            return output(ledger, 'stop', 'goal_revision_identity_changed', thread)
        final_now = time.time() if clock is None else clock()
        if (type(final_now) not in (int, float) or not math.isfinite(final_now)
                or not -5 <= final_now - observed <= USAGE_TTL):
            raise ValueError('Goal证据已过期')
        candidate['goal_revisions'][-1]['at'] = final_now
        candidate['last_check']['at'] = final_now
        candidate['goal_revision_checkpoint_required_at'] = final_now
        # 最终时间字段也参与限长，之后才原子交回run落盘。
        if len((json.dumps(candidate, ensure_ascii=False, indent=2, allow_nan=False) + '\n').encode()) > MAX_JSON:
            return output(ledger, 'stop', 'goal_revision_ledger_limit', thread)
        ledger.clear()
        ledger.update(candidate)
        return output(ledger, 'wait', 'goal_revision_accepted_checkpoint_required', thread)
    except (OSError, ValueError, TypeError, KeyError, sqlite3.Error):
        return output(ledger, 'stop', 'goal_revision_evidence_invalid', thread)


def origin_json_bytes(path):
    """证据限长只读一次；摘要与校验使用相同字节，拒歧义及非有限JSON。"""
    with Path(path).open('rb') as source:
        raw = source.read(MAX_JSON + 1)
    if len(raw) > MAX_JSON:
        raise ValueError('来源证据过大')
    def invalid_constant(_):
        raise ValueError('非有限JSON')
    value = json.loads(raw, object_pairs_hook=unique_object, parse_constant=invalid_constant)
    if not isinstance(value, dict):
        raise ValueError('来源证据必须为对象')
    return value, raw


def origin_item_digest(item):
    """正式item摘要采用独立审计约定的规范JSON。"""
    return hashlib.sha256(json.dumps(item, sort_keys=True, ensure_ascii=False,
                                    separators=(',', ':'), allow_nan=False).encode()).hexdigest()


def recover_goal_revision_origin(path, thread, home, evidence_file, goal_file,
                                 expected_ledger_hash, expected_objective_hash,
                                 authorization, now, clock):
    """锁内显式登记旧停态审计来源；不接纳新正文、不解除终态或触碰预算。"""
    ledger = {}
    try:
        valid_hash = lambda x: (isinstance(x, str) and len(x) == 64
                               and all(c in '0123456789abcdef' for c in x))
        if not isinstance(authorization, str) or not authorization.strip() or len(authorization) > 4096:
            return output(ledger, 'stop', 'origin_recovery_authorization_required', thread)
        ledger, ledger_raw = origin_json_bytes(path)
        if (not valid_hash(expected_ledger_hash)
                or hashlib.sha256(ledger_raw).hexdigest() != expected_ledger_hash):
            return output(ledger, 'stop', 'origin_recovery_ledger_changed', thread)
        if (type(ledger.get('version')) is not int or ledger.get('version') != 1
                or ledger.get('fraction') != '0.8' or ledger.get('allow_existing_credits') is not False
                or 'goal_mismatch_origin' in ledger
                or ledger.get('terminal_action') != 'stop'
                or ledger.get('terminal_reason') != 'goal_or_binding_changed'):
            return output(ledger, 'stop', 'origin_recovery_state_rejected', thread)
        binding, current_goal, status = identity(thread, home)
        old = ledger.get('goal')
        if (status != 'active' or ledger.get('binding') != binding
                or not isinstance(old, dict)
                or set(old) != set(current_goal) or set(old) != {'id', 'created_at_ms', 'objective_sha256'}
                or not isinstance(old['id'], str) or not old['id']
                or type(old['created_at_ms']) is not int
                or type(current_goal['created_at_ms']) is not int
                or {k: v for k, v in old.items() if k != 'objective_sha256'}
                != {k: v for k, v in current_goal.items() if k != 'objective_sha256'}
                or not all(valid_hash(x) for x in (expected_objective_hash, old['objective_sha256'],
                                                   current_goal['objective_sha256']))
                or expected_objective_hash != old['objective_sha256']
                or current_goal['objective_sha256'] == expected_objective_hash
                or not isinstance(ledger.get('automation_id'), str) or not ledger['automation_id']):
            return output(ledger, 'stop', 'origin_recovery_identity_rejected', thread)
        decision, decision_raw = origin_json_bytes(evidence_file)
        observed = decision.get('observed_at')
        fresh = lambda stamp, moment: (type(stamp) in (int, float) and math.isfinite(stamp)
                                       and -5 <= moment - stamp <= USAGE_TTL)
        if (type(decision.get('version')) is not int or decision.get('version') != 1 or decision.get('kind') != 'legacy_root_origin_decision'
                or decision.get('thread') != thread or decision.get('binding') != binding
                or decision.get('old_goal') != old
                or decision.get('expected_ledger_sha256') != expected_ledger_hash
                or not fresh(observed, now)):
            raise ValueError('来源决策不符')
        single = decision.get('single_writer')
        if (not isinstance(single, dict) or single.get('adjudicated_no_prior_terminal') is not True
                or single != {'contract': 'trusted_single_root_native_ledger_writer',
                              'thread': thread, 'automation_id': ledger['automation_id'],
                              'adjudicated_no_prior_terminal': True,
                              'limitation': 'not_a_full_host_write_journal'}):
            raise ValueError('缺少明确单owner裁定')
        refs = decision.get('evidence')
        if not isinstance(refs, dict) or set(refs) != {'formal_thread', 'audit', 'controller_inventory'}:
            raise ValueError('来源清单不完整')
        documents, digests = {}, {}
        for label, ref in refs.items():
            if (not isinstance(ref, dict) or set(ref) != {'path', 'sha256'}
                    or not isinstance(ref['path'], str) or not ref['path']
                    or not valid_hash(ref['sha256'])):
                raise ValueError('来源摘要无效')
            doc, raw = origin_json_bytes(ref['path'])
            digest = hashlib.sha256(raw).hexdigest()
            if digest != ref['sha256']:
                raise ValueError('来源内容已变')
            documents[label], digests[label] = doc, digest
        controllers = documents['controller_inventory']
        if (type(controllers.get('version')) is not int or controllers.get('version') != 1 or controllers.get('source') != 'root_controller_inventory'
                or controllers.get('thread') != thread
                or not fresh(controllers.get('observed_at'), now)
                or not isinstance(controllers.get('automation'), dict)
                or controllers['automation'].get('unique') is not True
                or controllers.get('automation') not in (
                    {'id': ledger['automation_id'], 'target_thread_id': thread,
                     'kind': 'heartbeat', 'status': 'ACTIVE', 'unique': True},
                    {'id': ledger['automation_id'], 'target_thread_id': thread,
                     'kind': 'heartbeat', 'status': 'PAUSED', 'unique': True})
                or controllers.get('native_ledger_writers') != [{'kind': 'root_checkpoint', 'thread': thread}]
                or controllers.get('other_guard_writers') != []
                or not isinstance(controllers.get('cooperative'), dict)
                or controllers['cooperative'].get('writes_native_ledger') is not False
                or controllers['cooperative'].get('manual_stop_preserved') is not True
                or controllers['cooperative'].get('pending_preserved') is not True
                or controllers.get('cooperative') != {
                    'role': 'queue_and_read_only_native_ledger', 'writes_native_ledger': False,
                    'manual_stop_preserved': True, 'pending_preserved': True}):
            raise ValueError('controller排他条件不符')
        formal = documents['formal_thread']
        content = formal.get('content')
        if (formal.get('isError') is not False or 'structuredContent' in formal
                or not isinstance(content, list) or len(content) != 1
                or not isinstance(content[0], dict) or content[0].get('type') != 'text'
                or not isinstance(content[0].get('text'), str)):
            raise ValueError('正式线程envelope无效')
        snapshot = json.loads(content[0]['text'], object_pairs_hook=unique_object,
                              parse_constant=lambda _: (_ for _ in ()).throw(ValueError('非有限JSON')))
        if (not isinstance(snapshot, dict) or snapshot.get('thread', {}).get('id') != thread
                or snapshot['thread'].get('cwd') != binding['project']
                or not isinstance(snapshot.get('turns'), list)):
            raise ValueError('正式线程身份不符')
        endpoints = decision.get('endpoints')
        if not isinstance(endpoints, dict) or set(endpoints) != {'turn_id', 'continue_id', 'stop_id', 'receipt_id'}:
            raise ValueError('端点无效')
        turns = [x for x in snapshot['turns'] if isinstance(x, dict) and x.get('id') == endpoints['turn_id']]
        if len(turns) != 1 or not isinstance(turns[0].get('items'), list):
            raise ValueError('turn缺失或重复')
        items = turns[0]['items']
        ids = [x.get('id') for x in items if isinstance(x, dict)]
        if len(ids) != len(items) or any(not isinstance(x, str) or not x for x in ids) or len(set(ids)) != len(ids):
            raise ValueError('item缺失或重复')
        start, end, receipt = [ids.index(endpoints[k]) for k in ('continue_id', 'stop_id', 'receipt_id')]
        if not start < end < receipt:
            raise ValueError('端点顺序不符')
        if any(x.get('type') not in {'commandExecution', 'mcpToolCall', 'reasoning', 'agentMessage', 'subAgentActivity'}
               or (x.get('type') == 'subAgentActivity' and x.get('kind') != 'completed')
               for x in items[start:end + 1]):
            raise ValueError('区间存在未审写事件或新派发')
        audit = documents['audit']
        assessment = audit.get('assessment', {})
        if (assessment.get('root_current_turn_tool_chain_gap_closed') is not True
                or assessment.get('root_tool_chain_has_intervening_guard_or_ledger_or_goal_write') is not False
                or assessment.get('first_stop_in_current_turn_established') is not True
                or assessment.get('stop_reason_confirmed_by_item78') != 'goal_or_binding_changed'
                or assessment.get('first_stop_summary') != 'stop active usage_budget_available'
                or assessment.get('sufficient_for_unconditional_legacy_origin_clearance') is not False
                or assessment.get('supports_no_other_root_guard_terminal_between_item41_and_item76') is not True
                or assessment.get('sufficient_to_prove_no_other_writer_changed_original_ledger_globally') is not False
                or audit.get('formal_source', {}).get('sha256') != digests['formal_thread']
                or audit.get('formal_source', {}).get('thread_id') != thread
                or audit.get('formal_source', {}).get('turn_id') != endpoints['turn_id']):
            raise ValueError('独立审计未闭合或夸大排他')
        interval = audit.get('interval', {})
        if interval.get('start_item_index') != start or interval.get('end_item_index') != end:
            raise ValueError('审计区间不符')
        audited = interval.get('items')
        if not isinstance(audited, list):
            raise ValueError('审计items缺失')
        rows = [x for x in audited if isinstance(x, dict) and x.get('type') in ('commandExecution', 'mcpToolCall')
                and start <= x.get('item_index_zero_based', -1) <= end]
        actual = [(i, x) for i, x in enumerate(items) if start <= i <= end
                  and x.get('type') in ('commandExecution', 'mcpToolCall')]
        if len(rows) != len(actual) or not actual:
            raise ValueError('写事件缺失')
        interval_facts = {
            'strict_between_command_executions': sum(x['type'] == 'commandExecution' for i, x in actual if start < i < end),
            'strict_between_mcp_tool_calls': sum(x['type'] == 'mcpToolCall' for i, x in actual if start < i < end),
            'command_executions_including_endpoints': sum(x['type'] == 'commandExecution' for _, x in actual),
            'command_truncations': 0, 'output_truncations': 0}
        if any(type(interval.get(k)) is not int or interval[k] != value
               for k, value in interval_facts.items()):
            raise ValueError('审计计数或截断结论矛盾')
        missing_outputs = [i for i, x in actual if x['type'] == 'commandExecution' and 'output' not in x]
        child_completions = [i for i in range(start, end + 1) if items[i].get('type') == 'subAgentActivity']
        if (interval.get('missing_output_fields') != missing_outputs
                or interval.get('subagent_completion_indices') != child_completions):
            raise ValueError('审计缺输出或子任务元数据不符')
        benign = decision.get('benign_failures', [])
        if not isinstance(benign, list) or any(not isinstance(x, dict) for x in benign):
            raise ValueError('失败裁定无效')
        failures, used_failures = [], []
        shell_classes = {'read_only', 'temporary_script_patch_and_scan', 'routing_script_creation_and_scan',
                         'routing_source_move', 'usage_envelope_write'}
        # 所有审计行都按实际类型及位置绑定角色，保留元事件也不得冒用写工具分类。
        expected_indices = sorted([i for i, _ in actual] + child_completions + [receipt])
        if [x.get('item_index_zero_based') for x in audited if isinstance(x, dict)] != expected_indices:
            raise ValueError('审计事件行缺失、重复或多出')
        for row in audited:
            if not isinstance(row, dict) or type(row.get('item_index_zero_based')) is not int:
                raise ValueError('审计事件索引无效')
            index = row['item_index_zero_based']
            item = items[index]
            role = row.get('classification')
            if (row.get('event_id') != item['id'] or row.get('type') != item['type']
                    or row.get('item_sha256_canonical_json') != origin_item_digest(item)):
                raise ValueError('审计角色与事件摘要不符')
            if index in (start, end):
                valid_role = item['type'] == 'commandExecution' and role == 'guard_checkpoint'
            elif index == receipt:
                valid_role = item['type'] == 'commandExecution' and role == 'read_receipt'
            elif item['type'] == 'commandExecution':
                valid_role = start < index < end and role in shell_classes
            elif item['type'] == 'mcpToolCall':
                valid_role = start < index < end and role == 'formal_usage_read'
            else:
                valid_role = (index in child_completions and role == 'subagent_completion'
                              and item['type'] == 'subAgentActivity' and item.get('kind') == 'completed')
            if not valid_role:
                raise ValueError('审计分类不能跨类型或冒用端点')
        for (index, item), row in zip(actual, rows):
            digest = origin_item_digest(item)
            if (row.get('event_id') != item['id'] or type(row.get('item_index_zero_based')) is not int
                    or row.get('item_index_zero_based') != index
                    or row.get('item_sha256_canonical_json') != digest
                    or row.get('status') != item.get('status')
                    or row.get('exit_code') != item.get('exitCode')
                    or item.get('commandTruncated') is True
                    or (isinstance(item.get('output'), dict) and item['output'].get('truncated') is True)
                    or row.get('type') != item['type']):
                raise ValueError('执行事件与审计不符')
            if index in (start, end):
                if item['type'] != 'commandExecution' or row.get('classification') != 'guard_checkpoint':
                    raise ValueError('端点必须是原checkpoint分类')
            elif item['type'] == 'mcpToolCall':
                if row.get('classification') != 'formal_usage_read':
                    raise ValueError('MCP分类与实际类型不符')
            elif row.get('classification') not in shell_classes:
                raise ValueError('中间shell不得冒用checkpoint或读回分类')
            if item['type'] == 'commandExecution' and (
                    not isinstance(item.get('command'), str) or not item['command'].strip()
                    or type(item.get('exitCode')) is not int
                    or ('commandTruncated' in item and item['commandTruncated'] is not False)
                    or (item.get('output') is not None and (
                        not isinstance(item['output'], dict)
                        or item['output'].get('truncated') is not False
                        or not isinstance(item['output'].get('text'), str)))):
                raise ValueError('完整命令或输出结构缺失')
            if item['type'] == 'mcpToolCall':
                if item.get('server') != 'codex_app' or item.get('tool') != 'get_usage_limits' or item.get('status') != 'completed':
                    raise ValueError('中间存在MCP写工具')
            elif index in (start, end):
                if item.get('status') != 'completed' or item.get('exitCode') != 0:
                    raise ValueError('checkpoint失败')
            elif any(token in item.get('command', '') for token in
                     ('guard_heartbeat.py', 'native-heartbeat', '/guard.py', 'get_goal', 'update_goal')):
                raise ValueError('中间存在守护或Goal操作')
            elif item.get('status') != 'completed' or item.get('exitCode') != 0:
                matches = [x for x in benign if x.get('event_id') == item['id']]
                if (len(matches) != 1 or matches[0] != {
                        'event_id': item['id'], 'item_sha256_canonical_json': digest,
                        'status': item.get('status'), 'exit_code': item.get('exitCode'),
                        'adjudication': 'benign_non_guard_failure',
                        'classification': row['classification']}
                        or item.get('status') != 'failed' or item.get('exitCode') != 1
                        or row['classification'] not in {'read_only', 'temporary_script_patch_and_scan', 'routing_script_creation_and_scan'}):
                    raise ValueError('未经逐事件裁定的失败')
                failures.append(copy.deepcopy(matches[0])); used_failures.append(item['id'])
        if len(used_failures) != len(benign):
            raise ValueError('失败裁定包含未知事件')
        for key in ('continue_id', 'stop_id', 'receipt_id'):
            item = items[ids.index(endpoints[key])]
            matches = [x for x in audit.get('key_items', []) if x.get('event_id') == item['id']]
            if len(matches) != 1 or matches[0].get('item_sha256_canonical_json') != origin_item_digest(item):
                raise ValueError('关键端点摘要不符')
        if items[start]['command'] != items[end]['command']:
            raise ValueError('两端checkpoint不同')
        import shlex
        outer = shlex.split(items[start]['command'])
        if len(outer) != 3 or outer[1] != '-lc':
            raise ValueError('checkpoint命令无效')
        command = shlex.split(outer[2].splitlines()[0])
        if (len(command) != 10 or Path(command[0]).name != 'python3' or command[1] != '-B'
                or command[2] != '.agents/skills/agent-continuous-development-guard/scripts/guard_heartbeat.py'
                or command[3:6] != ['--thread', thread, 'checkpoint']
                or command[6] != '--usage-file' or command[8] != '>'):
            raise ValueError('不是原thread checkpoint')
        if (items[start].get('output') != {'text': 'continue active usage_budget_available\n', 'truncated': False}
                or items[end].get('output') != {'text': 'stop active usage_budget_available\n', 'truncated': False}):
            raise ValueError('checkpoint输出不符')
        # 本case的stop与读回之间仅允许无写reasoning；未知工具或子任务活动都不能穿过。
        if any(x.get('type') != 'reasoning' for x in items[end + 1:receipt]):
            raise ValueError('stop回执前有未知或可能写入事件')
        receipt_rows = [x for x in audited if isinstance(x, dict)
                        and x.get('event_id') == items[receipt]['id']]
        if (len(receipt_rows) != 1 or receipt_rows[0].get('classification') != 'read_receipt'
                or receipt_rows[0].get('type') != 'commandExecution'
                or type(receipt_rows[0].get('item_index_zero_based')) is not int
                or receipt_rows[0]['item_index_zero_based'] != receipt
                or receipt_rows[0].get('item_sha256_canonical_json') != origin_item_digest(items[receipt])):
            raise ValueError('stop读回事件未审计')
        response = items[receipt]
        if (response.get('type') != 'commandExecution' or response.get('commandTruncated') is True
                or response.get('status') != 'completed' or response.get('exitCode') != 0
                or not isinstance(response.get('output'), dict)
                or response['output'].get('truncated') is not False):
            raise ValueError('stop回执不完整')
        stop = json.loads(response['output']['text'], object_pairs_hook=unique_object)
        if (stop.get('action') != 'stop' or stop.get('reason') != 'goal_or_binding_changed'
                or stop.get('goal_id') != old['id'] or stop.get('automation_id') != ledger['automation_id']
                or stop.get('thread') != thread or stop.get('last_check') != ledger.get('last_check')
                or not isinstance(stop.get('last_check'), dict)
                or stop['last_check'].get('mode') != 'checkpoint'
                or stop['last_check'].get('caller_thread') != thread
                or stop['last_check'].get('action') != 'continue'):
            raise ValueError('原始stop字段不符')
        goal_evidence, goal_raw = origin_json_bytes(goal_file)
        goal_observed = goal_evidence.get('observed_at')
        payload = goal_evidence.get('payload')
        goal = payload.get('goal') if isinstance(payload, dict) else None
        if (goal_evidence.get('source') != 'get_goal' or goal_evidence.get('thread') != thread
                or not fresh(goal_observed, now) or not isinstance(payload, dict)
                or not {'goal', 'remainingTokens', 'completionBudgetReport'} <= set(payload)
                or any(k in payload for k in ('isError', 'content', 'structuredContent', 'error'))
                or not isinstance(goal, dict) or goal.get('threadId') != thread
                or goal.get('status') != 'active' or not isinstance(goal.get('objective'), str)
                or any(k in goal and goal[k] != current_goal['id'] for k in ('id', 'goalId'))
                or hashlib.sha256(goal['objective'].encode()).hexdigest() != current_goal['objective_sha256']
                or type(goal.get('createdAt')) is not int
                or goal['createdAt'] != current_goal['created_at_ms'] // 1000):
            raise ValueError('当前正式Goal不符')
        history = ledger.get('goal_origin_recoveries', [])
        if not isinstance(history, list) or any(not isinstance(x, dict) for x in history):
            raise ValueError('来源历史无效')
        if any(x.get('legacy_ledger_sha256') == expected_ledger_hash
               or (x.get('kind') == 'recovered_root_single_writer' and x.get('previous_goal') == old
                   and x.get('binding') == binding) for x in history):
            return output(ledger, 'stop', 'origin_recovery_replayed', thread)
        record = {'kind': 'recovered_root_single_writer', 'at': now,
                  'binding': copy.deepcopy(binding), 'previous_goal': copy.deepcopy(old),
                  'observed_status': 'active', 'eligible': True,
                  'previous_terminal_action': None, 'previous_terminal_reason': None,
                  'historical_basis': 'explicit_audit_adjudication_not_live_observation',
                  'active_basis': 'current_formal_goal_and_verified_user_control_not_cached_stop',
                  'legacy_stop': {'terminal_action': ledger['terminal_action'],
                                  'terminal_reason': ledger['terminal_reason'],
                                  'cached_goal_status': ledger.get('goal_status'),
                                  'last_check': copy.deepcopy(ledger.get('last_check'))},
                  'legacy_ledger_sha256': expected_ledger_hash,
                  'decision_sha256': hashlib.sha256(decision_raw).hexdigest(),
                  'evidence_sha256': digests, 'evidence': copy.deepcopy(refs),
                  'decision_path': str(evidence_file),
                  'audit_method': 'formal_complete_root_toolchain_plus_explicit_single_writer_adjudication',
                  'validated_audit_assessment': {key: copy.deepcopy(assessment[key]) for key in (
                      'first_stop_in_current_turn_established', 'first_stop_summary',
                      'stop_reason_confirmed_by_item78', 'root_current_turn_tool_chain_gap_closed',
                      'root_tool_chain_has_intervening_guard_or_ledger_or_goal_write',
                      'supports_no_other_root_guard_terminal_between_item41_and_item76',
                      'sufficient_to_prove_no_other_writer_changed_original_ledger_globally',
                      'sufficient_for_unconditional_legacy_origin_clearance')},
                  'validated_interval': dict(interval_facts, missing_output_fields=missing_outputs,
                                             subagent_completion_indices=child_completions),
                  'endpoints': copy.deepcopy(endpoints),
                  'benign_failures': failures,
                  'goal_evidence_sha256': hashlib.sha256(goal_raw).hexdigest(),
                  'authorization_sha256': hashlib.sha256(authorization.encode()).hexdigest(),
                  'single_writer': copy.deepcopy(single)}
        candidate = copy.deepcopy(ledger)
        candidate['goal_mismatch_origin'] = record
        candidate['goal_origin_recoveries'] = copy.deepcopy(history) + [copy.deepcopy(record)]
        if not revision_ledger_fits(candidate):
            return output(ledger, 'stop', 'origin_recovery_ledger_limit', thread)
        if identity(thread, home) != (binding, current_goal, 'active'):
            return output(ledger, 'stop', 'origin_recovery_identity_changed', thread)
        final_now = clock()
        if not all(fresh(stamp, final_now) for stamp in (observed, goal_observed, controllers['observed_at'])):
            raise ValueError('最终来源证据过期')
        candidate['goal_mismatch_origin']['at'] = final_now
        candidate['goal_origin_recoveries'][-1]['at'] = final_now
        if not revision_ledger_fits(candidate):
            return output(ledger, 'stop', 'origin_recovery_ledger_limit', thread)
        _, final_raw = origin_json_bytes(path)
        if final_raw != ledger_raw or hashlib.sha256(final_raw).hexdigest() != expected_ledger_hash:
            return output(ledger, 'stop', 'origin_recovery_ledger_changed', thread)
        atomic(path, candidate)
        return output(candidate, 'wait', 'origin_recovered_acceptance_required', thread)
    except (OSError, ValueError, TypeError, KeyError, IndexError, AttributeError,
            RecursionError, OverflowError, sqlite3.Error):
        return output(ledger, 'stop', 'origin_recovery_evidence_invalid', thread)


def run(thread, command, usage_file=None, automation_id=None, now=None, previous_automation_id=None,
        user_authorization=None, authorization_file=None, goal_evidence_file=None,
        expected_objective_sha256=None, origin_evidence_file=None, expected_ledger_sha256=None,
        end_full_budget_once=False):
    """一次门禁检查；官方自动化的创建、暂停与删除由调用方执行。"""
    thread = str(uuid.UUID(thread))
    if command in ('checkpoint', 'resume', 'authorize-full-budget', 'rebase-budget', 'accept-goal-revision', 'recover-goal-revision-origin') and os.environ.get('CODEX_THREAD_ID') != thread:
        return output({}, 'wait', 'checkpoint_caller_mismatch', thread)
    fixed_now = now
    now = time.time() if now is None else now
    home = Path(os.environ.get('CODEX_HOME', str(Path.home() / '.codex'))).resolve()
    guards = ROOT / '.agents/project/guards'
    runtime = guards / thread
    if not runtime.resolve().is_relative_to(ROOT):
        raise ValueError('账本路径越界')
    if command != 'init' and not runtime.is_dir():
        return output({}, 'stop', 'ledger_missing', thread)
    runtime.mkdir(parents=True, exist_ok=True)
    if command != 'recover-goal-revision-origin':
        try:
            # 运行账本不进入源码提交；已有项目忽略策略由宿主保留。
            with (guards / '.gitignore').open('x', encoding='utf-8') as ignored:
                ignored.write('*\n!.gitignore\n')
        except FileExistsError:
            pass
    path = runtime / 'native-heartbeat.json'
    with (runtime / 'native-heartbeat.lock').open('a') as lock:
        lock_file(lock)
        if fixed_now is None:
            now = time.time()
        ledger = read_json(path) if path.exists() else {}
        grant_file = ledger.get('full_budget_grant_file')
        if grant_file is not None and grant_file != str(runtime / 'full-budget-once.json'):
            return output({}, 'stop', 'ledger_invalid', thread)
        if command == 'recover-goal-revision-origin':
            return recover_goal_revision_origin(
                path, thread, home, origin_evidence_file, goal_evidence_file,
                expected_ledger_sha256, expected_objective_sha256, user_authorization, now,
                time.time if fixed_now is None else lambda: now)
        try:
            binding, current_goal, status = identity(thread, home)
        except (OSError, sqlite3.Error):
            # App索引暂忙或不可读不等于绑定已改变；本轮禁业务，下轮重新核验。
            if ledger:
                return output(ledger, 'wait', 'identity_temporarily_unavailable', thread)
            return output({}, 'stop', 'binding_unknown', thread)
        except (ValueError, TypeError):
            if command == 'accept-goal-revision':
                origin = ledger.get('goal_mismatch_origin')
                if poison_goal_revision_origin(ledger):
                    atomic(path, ledger)
                return output(ledger, 'stop', 'goal_revision_identity_rejected', thread)
            if ledger:
                origin = ledger.get('goal_mismatch_origin')
                poison_goal_revision_origin(ledger)
                end_full_budget(ledger, 'binding_unknown')
                ledger.update(terminal_action='stop', terminal_reason='binding_unknown')
                atomic(path, ledger)
            return output(ledger, 'stop', 'binding_unknown', thread)
        old_goal = ledger.get('goal')
        same_revision_identity = (
            ledger.get('binding') == binding and isinstance(old_goal, dict)
            and set(old_goal) == set(current_goal) == {'id', 'created_at_ms', 'objective_sha256'}
            and isinstance(old_goal.get('id'), str) and bool(old_goal['id'])
            and old_goal['id'] == current_goal['id']
            and type(old_goal.get('created_at_ms')) is int
            and type(current_goal.get('created_at_ms')) is int
            and old_goal['created_at_ms'] == current_goal['created_at_ms'])
        origin = ledger.get('goal_mismatch_origin')
        if (isinstance(origin, dict) and origin.get('eligible') is True
                and not ledger.get('terminal_action')
                and not {'at', 'binding', 'previous_goal', 'observed_status',
                         'previous_terminal_action', 'previous_terminal_reason'} <= set(origin)):
            return output(ledger, 'stop', 'goal_revision_origin_rejected', thread)
        if (isinstance(origin, dict) and origin.get('eligible') is True
                and (not same_revision_identity or status != 'active')):
            # 观察到身份或控制边界后永久污染本次来源；切回原Goal也不能复活。
            if poison_goal_revision_origin(ledger):
                atomic(path, ledger)
        if command == 'accept-goal-revision':
            result = accept_goal_revision(
                ledger, thread, binding, current_goal, status, home,
                goal_evidence_file, expected_objective_sha256, user_authorization, now,
                clock=time.time if fixed_now is None else lambda: now)
            if result['reason'] in ('goal_revision_accepted_checkpoint_required',
                                    'goal_revision_identity_changed'):
                atomic(path, ledger)
            return result
        if ledger and (ledger.get('binding') != binding or ledger.get('goal') != current_goal):
            end_full_budget(ledger, 'goal_or_binding_changed')
            # 正文变化不能掩盖先前暂停、完成或额度终态；旧版无归因的mismatch也不补造来源。
            if ledger.get('terminal_action'):
                return output(ledger, ledger['terminal_action'], ledger['terminal_reason'], thread)
            candidate = copy.deepcopy(ledger)
            candidate['goal_mismatch_origin'] = {
                'at': now, 'binding': copy.deepcopy(ledger.get('binding')),
                'previous_goal': copy.deepcopy(ledger.get('goal')),
                'observed_status': status,
                'eligible': (same_revision_identity and status == 'active'
                             and old_goal['objective_sha256'] != current_goal['objective_sha256']),
                'previous_terminal_action': None, 'previous_terminal_reason': None}
            candidate.update(terminal_action='stop', terminal_reason='goal_or_binding_changed')
            if not revision_ledger_fits(candidate):
                return output(ledger, 'stop', 'goal_revision_ledger_limit', thread)
            ledger.clear()
            ledger.update(candidate)
            atomic(path, ledger)
            return output(ledger, 'stop', 'goal_or_binding_changed', thread)
        if status in ('complete', 'paused'):
            action, reason = ('finish', 'goal_complete') if status == 'complete' else ('stop', 'goal_' + str(status))
            if ledger:
                end_full_budget(ledger, reason)
                ledger.update(terminal_action=action, terminal_reason=reason, goal_status=status)
                atomic(path, ledger)
            return output(ledger or {'goal': current_goal, 'goal_status': status}, action, reason, thread)
        if (ledger.get('goal_revision_checkpoint_required_at') is not None
                and not ledger.get('terminal_action') and command != 'checkpoint'):
            return output(ledger, 'wait', 'goal_revision_checkpoint_required', thread)
        if command == 'rebase-budget':
            result = rebase_budget(ledger, thread, binding, current_goal, status, home,
                                   usage_file, authorization_file, now, user_authorization,
                                   clock=time.time if fixed_now is None else None,
                                   end_full_budget_once=end_full_budget_once)
            if result['reason'] == 'budget_rebased_checkpoint_required':
                atomic(path, ledger)
            return result
        if command == 'resume':
            result = resume_ledger(ledger, thread, binding, current_goal, status, home,
                                   usage_file, now, user_authorization)
            if result['action'] == 'continue' or result['reason'] == 'goal_revision_checkpoint_required':
                atomic(path, ledger)
            return result
        if command == 'authorize-full-budget':
            if (ledger.get('version') != 1 or ledger.get('fraction') != '0.8'
                    or ledger.get('allow_existing_credits') is not False):
                return output(ledger, 'stop', 'ledger_invalid', thread)
            result = authorize_full_budget(ledger, runtime, thread, binding, current_goal, status,
                                           home, usage_file, authorization_file, now, user_authorization)
            if result['reason'] == 'full_budget_authorized_checkpoint_required':
                atomic(path, ledger)
            return result
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
        revision_checkpoint = ledger.get('goal_revision_checkpoint_required_at')
        if revision_checkpoint is not None and command != 'checkpoint':
            return output(ledger, 'wait', 'goal_revision_checkpoint_required', thread)
        available = False
        ledger.pop('usage_error', None)
        try:
            if revision_checkpoint is not None:
                envelope = read_json(usage_file)
                response = usage_envelope(envelope, thread, now)
                if (type(revision_checkpoint) not in (int, float)
                        or not math.isfinite(revision_checkpoint)
                        or envelope['observed_at'] <= revision_checkpoint):
                    return output(ledger, 'wait', 'goal_revision_checkpoint_required', thread)
            else:
                response = usage_snapshot(usage_file, thread, now)
            available = budget_check(ledger, response, now)
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
            except (OSError, sqlite3.Error):
                return output(ledger, 'wait', 'identity_temporarily_unavailable', thread)
            except (ValueError, TypeError):
                final_binding, final_goal, final_status = None, None, None
            if (final_binding != binding or final_goal != current_goal or final_status != status):
                end_full_budget(ledger, 'binding_changed_during_check')
                ledger.update(terminal_action='stop', terminal_reason='binding_changed_during_check')
                atomic(path, ledger)
                return output(ledger, 'stop', 'binding_changed_during_check', thread)
            ledger.pop('first_wait_at', None)
            ledger.pop('goal_revision_checkpoint_required_at', None)
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
    resume = commands.add_parser('resume')
    resume.add_argument('--usage-file', type=Path, required=True)
    resume.add_argument('--user-authorization', required=True,
                        help='调用Agent核验本次真人明确恢复原任务后填写；此参数本身不是授权证明')
    full_budget = commands.add_parser('authorize-full-budget')
    full_budget.add_argument('--usage-file', type=Path, required=True)
    full_budget.add_argument('--authorization-file', type=Path, required=True)
    full_budget.add_argument('--user-authorization', required=True,
                             help='须与正式read_thread真人原文相同；调用者先核验来源，此参数不是授权证明')
    rebase = commands.add_parser('rebase-budget')
    rebase.add_argument('--usage-file', type=Path, required=True)
    rebase.add_argument('--authorization-file', type=Path, required=True)
    rebase.add_argument('--user-authorization', required=True,
                        help='真人明确重建百分比基线；调用者核read_thread来源，不能由额度恢复推断')
    rebase.add_argument('--end-full-budget-once', action='store_true',
                        help='仅在同一真人授权明确结束旧覆盖时启用')
    revision = commands.add_parser('accept-goal-revision')
    revision.add_argument('--goal-evidence-file', type=Path, required=True)
    revision.add_argument('--expected-objective-sha256', required=True)
    revision.add_argument('--user-authorization', required=True,
                          help='调用Agent先核本次真人修订同一Goal；参数本身不是授权证明，不重算预算')
    origin = commands.add_parser('recover-goal-revision-origin')
    origin.add_argument('--origin-evidence-file', type=Path, required=True)
    origin.add_argument('--goal-evidence-file', type=Path, required=True)
    origin.add_argument('--expected-ledger-sha256', required=True)
    origin.add_argument('--expected-objective-sha256', required=True)
    origin.add_argument('--user-authorization', required=True,
                        help='Agent已核本次真人修订/active与单Root审计；参数本身不是授权证明')
    commands.add_parser('bind').add_argument('--automation-id', required=True)
    replacement = commands.add_parser('rebind')
    replacement.add_argument('--automation-id', required=True)
    replacement.add_argument('--previous-automation-id', required=True)
    args = parser.parse_args()
    try:
        result = run(args.thread, args.command, getattr(args, 'usage_file', None),
                     getattr(args, 'automation_id', None),
                     previous_automation_id=getattr(args, 'previous_automation_id', None),
                     user_authorization=getattr(args, 'user_authorization', None),
                     authorization_file=getattr(args, 'authorization_file', None),
                     goal_evidence_file=getattr(args, 'goal_evidence_file', None),
                     expected_objective_sha256=getattr(args, 'expected_objective_sha256', None),
                     origin_evidence_file=getattr(args, 'origin_evidence_file', None),
                     expected_ledger_sha256=getattr(args, 'expected_ledger_sha256', None),
                     end_full_budget_once=getattr(args, 'end_full_budget_once', False))
    except (OSError, ValueError, TypeError, KeyError, sqlite3.Error):
        result = output({}, 'stop', 'local_evidence_unavailable', args.thread)
    print(json.dumps(result, ensure_ascii=False, allow_nan=False))


if __name__ == '__main__':
    main()
