"""高价值回归：防跨任务恢复、停止竞态与守护进程泄漏，不调用真实模型或Paseo。"""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name('guard.py')
spec = importlib.util.spec_from_file_location('recovery', SCRIPT)
recovery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recovery)


def quota_snapshot(balance='15000', used=100, secondary=None):
    """隔离的正式额度形态，用于验证预算，不连接商业模型。"""
    return {'accountId': 'account', 'ordinaryUsageAllowed': used < 100,
            'rateLimits': {'limitId': 'codex', 'primary': {'usedPercent': used, 'resetsAt': 2000,
            'windowDurationMins': 10080}, 'secondary': secondary, 'spendControlReached': False,
            'credits': {'hasCredits': True, 'unlimited': False, 'balance': balance}}}


def example_budget():
    return recovery.initialize_usage_budget(quota_snapshot(), 100)


class Decisions(unittest.TestCase):
    def setUp(self):
        self.agent = {'Status': 'idle', 'Cwd': str(recovery.ROOT)}
        self.goal = {'id': 'g', 'status': 'active'}
        self.config = {'goal_id': 'g'}
        self.turn = {'kind': 'task_complete', 'key': 't', 'payload': {'completed_at': 0}, 'tools': 0}

    def decide(self, **changes):
        return recovery.decision(changes.get('agent', self.agent), changes.get('goal', self.goal),
                                 changes.get('turn', self.turn), self.config, 120)

    def test_terminal_and_human_control_override_recovery(self):
        for state in ('paused', 'complete'):
            self.assertEqual(self.decide(goal={'id': 'g', 'status': state}), state)
        self.assertEqual(self.decide(turn=dict(self.turn, kind='turn_aborted')), 'stopped')
        self.assertEqual(self.decide(agent=dict(self.agent, PendingPermissions=[1])), 'needs_attention')
        self.assertEqual(self.decide(goal={'id': 'other', 'status': 'active'}), 'needs_attention')
        self.assertEqual(self.decide(agent=dict(self.agent, Status='running')), 'working')

    def test_pending_questions_never_become_timeout_approval(self):
        # Paseo将问答与审批放在同一容器；推荐标签/长时间等待不应触发permit或恢复发送。
        requests = (
            {'id': 'permission-q', 'kind': 'question', 'name': 'request_user_input',
             'input': {'questions': [{'id': 'layout', 'options': [{'label': 'A (Recommended)'}]}]}},
            {'id': 'permission-async', 'kind': 'question', 'name': 'request_user_input_async'},
            {'id': 'permission-tool', 'kind': 'tool', 'name': 'CodexCommand'},
            {'id': 'unknown', 'kind': 'unknown'},
        )
        for request in requests:
            for boundary in ('pending', 'user_answered', 'stop', 'replaced_goal'):
                with self.subTest(request=request['id'], boundary=boundary), tempfile.TemporaryDirectory() as folder:
                    runtime = Path(folder)
                    config = dict(self.config, agent='a', thread='t', rollout='r')
                    (runtime / 'config.json').write_text(json.dumps(config))
                    current = dict(self.goal)
                    agent = dict(self.agent, PendingPermissions=[request])
                    turn = dict(self.turn)
                    if boundary == 'user_answered':
                        # 人工回答后新的业务轮次正在运行，不注入重复继续或伪造答案。
                        agent.update(PendingPermissions=[], Status='running')
                    elif boundary == 'stop':
                        current['status'] = 'paused'
                    elif boundary == 'replaced_goal':
                        current['id'] = 'replacement'
                    observed = []
                    def finish_check(*_):
                        observed.append(json.loads((runtime / 'state.json').read_text()))
                        (runtime / 'disabled').touch()
                    with patch.object(recovery, 'binding_valid', return_value=True), patch.object(recovery, 'inspect', return_value=agent), patch.object(recovery, 'goal', return_value=current), patch.object(recovery, 'timeline', return_value=turn), patch.object(recovery, 'notify', return_value=True), patch.object(recovery.time, 'time', return_value=3600), patch.object(recovery, 'rpc') as remote, patch.object(recovery, 'run_json') as send, patch.object(recovery, 'sleep_until', side_effect=finish_check):
                        recovery.worker(runtime, os.getppid())
                    remote.assert_not_called()
                    send.assert_not_called()
                    state = observed[0] if observed else json.loads((runtime / 'state.json').read_text())
                    self.assertEqual(state['status'], 'paused' if boundary == 'stop' else 'working' if boundary == 'user_answered' else 'needs_attention')

    def test_blocked_goal_overrides_error_and_quota_recovery(self):
        for error in ('500 network', 'usage_limit exceeded'):
            turn = dict(self.turn, semantic_blocked=True, payload={'error': {'message': error}})
            self.assertEqual(self.decide(goal={'id': 'g', 'status': 'blocked'}, turn=turn), 'needs_attention')

    def test_runner_blocked_and_unknown_errors_resume_except_explicit_hard_controls(self):
        blocked = {'id': 'g', 'status': 'blocked'}
        for error in ({'message': 'unknown transient failure'}, {'message': '429', 'codex_error_info': {'response_too_many_failed_attempts': {'http_status_code': 429}}}):
            turn = dict(self.turn, payload={'error': error})
            self.assertEqual(self.decide(goal=blocked, turn=turn), 'recover_error')
            self.assertEqual(self.decide(goal=blocked, turn=turn, agent=dict(self.agent, Status='running')), 'working')
            self.assertEqual(self.decide(goal=blocked, turn=dict(turn, kind='user_message')), 'needs_attention')
            self.assertEqual(self.decide(goal=blocked, turn=dict(turn, kind='turn_aborted')), 'stopped')
            self.assertEqual(self.decide(goal=blocked, turn=turn, agent=dict(self.agent, PendingPermissions=[1])), 'needs_attention')
        for message in ('401 unauthorized', 'authentication invalid', '403 permission denied', '401 unauthorized; quota exceeded'):
            self.assertEqual(self.decide(goal=blocked, turn=dict(self.turn, payload={'error': {'message': message}})), 'needs_attention')

    def test_running_and_auth_controls_override_usage_limited_enum(self):
        limited = {'id': 'g', 'status': 'usage_limited'}
        self.assertEqual(self.decide(goal=limited, agent=dict(self.agent, Status='running')), 'working')
        self.assertEqual(self.decide(goal=limited, turn=dict(self.turn, kind='user_message')), 'needs_attention')
        self.assertEqual(self.decide(goal=limited, turn=dict(self.turn, payload={'error': {'message': '401 unauthorized'}})), 'needs_attention')

    def test_explicit_goal_block_is_preserved_in_timeline_across_later_runner_error(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            for name, arguments in [('functions.update_goal', '{"status":"blocked"}'), ('functions.exec', 'await tools.update_goal({status: "blocked"})'), ('functions.exec', json.dumps({'code': 'await tools.update_goal({status: "blocked"})'}))]:
                rows = [
                    {'type': 'response_item', 'timestamp': 'control', 'payload': {'type': 'function_call', 'name': name, 'arguments': arguments}},
                    {'type': 'event_msg', 'timestamp': 't', 'payload': {'type': 'task_complete', 'error': {'message': '429'}}},
                ]
                path.write_text('\n'.join(json.dumps(row) for row in rows))
                turn = recovery.timeline(path)
                self.assertTrue(turn['semantic_blocked'])
                self.assertEqual(self.decide(goal={'id': 'g', 'status': 'blocked'}, turn=turn), 'needs_attention')

    def test_worker_reactivates_runner_blocked_only_with_quota_and_fresh_controls(self):
        for boundary in ('none', 'expired_quota_deadline', 'unknown_quota', 'exhausted_quota', 'semantic_blocked', 'persisted_semantic_blocked', 'new_permission', 'new_pause', 'new_cancel', 'new_goal'):
            with self.subTest(boundary=boundary), tempfile.TemporaryDirectory() as folder:
                runtime = Path(folder)
                config = {'agent': 'a', 'thread': 't', 'rollout': 'r', 'goal_id': 'g'}
                (runtime / 'config.json').write_text(json.dumps(config))
                (runtime / 'state.json').write_text(json.dumps(dict({'pending_key': 't', 'retry_at': 0, 'quota_check_at': 1 if boundary == 'expired_quota_deadline' else 0}, **({'goal_control': {'status': 'blocked', 'key': 'old-control'}} if boundary == 'persisted_semantic_blocked' else {}))))
                current = {'id': 'g', 'status': 'blocked'}
                turn = dict(self.turn, payload={'error': {'message': 'unknown temporary transport failure'}}, semantic_blocked=boundary == 'semantic_blocked')
                agent = dict(self.agent)
                operations = []
                def remote(method, params):
                    operations.append((method, params))
                    if method == 'account/rateLimits/read':
                        if boundary == 'unknown_quota':return {}
                        return {'ordinaryUsageAllowed': True, 'rateLimits': {'primary': {'usedPercent': 100 if boundary == 'exhausted_quota' else 1}}}
                    if method == 'thread/goal/get':return {'goal': {'id': current['id'], 'status': current['status']}}
                    if method == 'thread/goal/set':
                        current['status'] = 'active'
                        if boundary == 'new_permission':agent['PendingPermissions'] = [1]
                        if boundary == 'new_pause':current['status'] = 'paused'
                        if boundary == 'new_cancel':turn['kind'] = 'turn_aborted'
                        if boundary == 'new_goal':current['id'] = 'replacement'
                        return {}
                    raise AssertionError(method)
                with patch.object(recovery, 'binding_valid', return_value=True), patch.object(recovery, 'inspect', side_effect=lambda *_: dict(agent)), patch.object(recovery, 'goal', side_effect=lambda *_: dict(current)), patch.object(recovery, 'timeline', side_effect=lambda *_: dict(turn)), patch.object(recovery, 'notify', return_value=True), patch.object(recovery, 'rpc', side_effect=remote), patch.object(recovery, 'run_json', return_value={}) as send, patch.object(recovery, 'sleep_until', side_effect=lambda *_: (runtime / 'disabled').touch()):
                    recovery.worker(runtime, os.getppid())
                if boundary in ('none', 'expired_quota_deadline'):
                    send.assert_called_once()
                    self.assertEqual(current['status'], 'active')
                else:
                    send.assert_not_called()
                if boundary in ('unknown_quota', 'exhausted_quota', 'semantic_blocked', 'persisted_semantic_blocked'):
                    self.assertFalse(any(method == 'thread/goal/set' for method, _ in operations))

    def test_confirmed_active_watermark_prevents_old_block_replay(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            def append(kind, payload, key):
                with path.open('a') as output:
                    output.write(json.dumps({'type': kind, 'timestamp': key, 'payload': payload}) + '\n')
            append('response_item', {'type': 'function_call', 'name': 'update_goal',
                                     'arguments': '{"status":"blocked"}'}, '2026-10-29T12:00:00Z')
            append('event_msg', {'type': 'task_complete', 'error': {'message': '429'}}, 't')
            turn = recovery.timeline(path)
            state = {}
            blocked = dict(self.goal, status='blocked')
            self.assertTrue(recovery.controlled_turn(state, turn, blocked, self.config)['semantic_blocked'])
            # 普通输入、启动轮次及state重载均不能解除硬阻塞。
            for kind in ('user_message', 'task_started', 'task_complete'):
                append('event_msg', {'type': kind}, kind)
                state = json.loads(json.dumps(state))
                self.assertTrue(recovery.controlled_turn(state, recovery.timeline(path), blocked, self.config)['semantic_blocked'])
            active = dict(self.goal, updated_at_ms=int(recovery.datetime.fromisoformat('2026-10-29T12:01:00+00:00').timestamp() * 1000))
            recovered = recovery.controlled_turn(state, recovery.timeline(path), active, self.config)
            self.assertFalse(recovered['semantic_blocked'])
            self.assertNotIn('goal_control', state)
            self.assertIn('control_watermark', state)
            append('event_msg', {'type': 'task_complete', 'error': {'message': '429'}}, 'later-error')
            # 旧blocked仍在512KiB尾部，跨worker重启仍不污染后续429。
            state = json.loads(json.dumps(state))
            turn = recovery.controlled_turn(state, recovery.timeline(path), blocked, self.config)
            self.assertFalse(turn['semantic_blocked'])
            self.assertEqual(self.decide(goal=blocked, turn=turn), 'recover_error')
            append('response_item', {'type': 'function_call', 'name': 'update_goal',
                                     'arguments': '{"status":"blocked"}'}, 'new-block')
            append('event_msg', {'type': 'task_complete', 'error': {'message': '429'}}, 'new-error')
            turn = recovery.controlled_turn(state, recovery.timeline(path), blocked, self.config)
            self.assertTrue(turn['semantic_blocked'])
            self.assertEqual(self.decide(goal=blocked, turn=turn), 'needs_attention')
            self.assertEqual(self.decide(goal=dict(self.goal, status='paused'), turn=turn), 'paused')
            watermark = state['control_watermark']
            recovery.controlled_turn(state, turn, dict(self.goal, id='replacement'), self.config)
            self.assertEqual(state['control_watermark'], watermark)
            self.assertEqual(state['goal_control']['key'], 'new-block')

    def test_new_control_waits_for_goal_database_commit(self):
        for status in ('blocked', 'paused', 'complete'):
            with self.subTest(status=status):
                state = {}
                control = {'status': status, 'key': '2026-10-29T12:01:00Z',
                           'source': ['rollout', 1], 'offset': 200}
                turn = dict(self.turn, goal_control=control)
                before = int(recovery.datetime.fromisoformat('2026-10-29T12:00:00+00:00').timestamp() * 1000)
                active = dict(self.goal, updated_at_ms=before)
                unknown = recovery.controlled_turn(state, turn, self.goal, self.config)
                self.assertTrue(unknown['semantic_blocked'])
                self.assertEqual(state['goal_control'], control)
                pending = recovery.controlled_turn(state, turn, active, self.config)
                self.assertEqual(state['goal_control'], control)
                self.assertNotIn('control_watermark', state)
                self.assertEqual(self.decide(goal=active, turn=pending), 'needs_attention')
                if status == 'blocked':
                    committed = recovery.controlled_turn(state, turn, dict(active, status='blocked'), self.config)
                    self.assertEqual(self.decide(goal=dict(active, status='blocked'), turn=committed), 'needs_attention')
                    restored = recovery.controlled_turn(state, turn, dict(active, updated_at_ms=before + 120000), self.config)
                    self.assertFalse(restored['semantic_blocked'])
                    replayed = recovery.controlled_turn(state, turn, dict(active, status='blocked'), self.config)
                    self.assertFalse(replayed['semantic_blocked'])
                # 文件截短后同inode较小offset的新控制不能被旧水位吞掉。
                fresh = dict(control, key='2026-10-29T12:03:00Z', offset=50)
                latest = recovery.controlled_turn(state, dict(turn, goal_control=fresh), active, self.config)
                self.assertTrue(latest['semantic_blocked'])

    def test_quota_deadline_persists_but_controls_and_running_are_checked(self):
        for boundary in ('wait', 'running', 'permission', 'paused', 'new_goal', 'auth'):
            with self.subTest(boundary=boundary), tempfile.TemporaryDirectory() as folder:
                runtime = Path(folder)
                config = {'agent': 'a', 'thread': 't', 'rollout': 'r', 'goal_id': 'g'}
                (runtime / 'config.json').write_text(json.dumps(config))
                (runtime / 'state.json').write_text(json.dumps({'pending_key': 't', 'retry_at': 0}))
                current = dict(self.goal)
                agent = dict(self.agent)
                turn = dict(self.turn, payload={'error': {'message': '429'}})
                calls = []
                reset = time.time() + 7200
                def remote(method, params):
                    calls.append(method)
                    self.assertEqual(method, 'account/rateLimits/read')
                    return {'ordinaryUsageAllowed': True, 'rateLimits': {'primary': {'usedPercent': 100, 'resetsAt': reset}}}
                def next_cycle(*_):
                    if len(observed) == 0:
                        observed.append(json.loads((runtime / 'state.json').read_text()))
                        if boundary == 'running': agent['Status'] = 'running'
                        if boundary == 'permission': agent['PendingPermissions'] = [1]
                        if boundary == 'paused': current['status'] = 'paused'
                        if boundary == 'new_goal': current['id'] = 'replacement'
                        if boundary == 'auth': turn['payload'] = {'error': {'message': '401 unauthorized'}}
                    else:
                        observed.append(json.loads((runtime / 'state.json').read_text()))
                        (runtime / 'disabled').touch()
                observed = []
                with patch.object(recovery, 'binding_valid', return_value=True), patch.object(recovery, 'inspect', side_effect=lambda *_: dict(agent)), patch.object(recovery, 'goal', side_effect=lambda *_: dict(current)), patch.object(recovery, 'timeline', side_effect=lambda *_: dict(turn)), patch.object(recovery, 'notify', return_value=True), patch.object(recovery, 'rpc', side_effect=remote), patch.object(recovery, 'run_json') as send, patch.object(recovery, 'sleep_until', side_effect=next_cycle):
                    recovery.worker(runtime, os.getppid())
                self.assertEqual(calls, ['account/rateLimits/read'])
                self.assertAlmostEqual(observed[0]['quota_check_at'], reset + 30, delta=1)
                self.assertEqual(observed[0]['next_recovery_at'], observed[0]['quota_check_at'])
                state = json.loads((runtime / 'state.json').read_text())
                self.assertEqual(state['quota_check_at'], observed[0]['quota_check_at'])
                send.assert_not_called()
                if boundary == 'paused': self.assertEqual(state['status'], 'paused')
                elif boundary == 'running':
                    self.assertEqual(observed[1]['status'], 'working')
                    self.assertTrue(observed[1]['task_running'])
                    self.assertIsNone(observed[1]['next_recovery_at'])
                elif boundary in ('permission', 'new_goal', 'auth'):
                    self.assertEqual(observed[1]['status'], 'needs_attention')
                else: self.assertEqual(observed[1]['status'], 'quota_wait')
                # 模拟worker重启，不重查仍未到期的已知额度窗口。
                (runtime / 'disabled').unlink(missing_ok=True)
                with patch.object(recovery, 'binding_valid', return_value=True), patch.object(recovery, 'inspect', return_value=self.agent), patch.object(recovery, 'goal', return_value=self.goal), patch.object(recovery, 'timeline', return_value=dict(self.turn, payload={'error': {'message': '429'}})), patch.object(recovery, 'notify', return_value=True), patch.object(recovery, 'rpc') as remote, patch.object(recovery, 'sleep_until', side_effect=lambda *_: (runtime / 'disabled').touch()):
                    recovery.worker(runtime, os.getppid())
                    remote.assert_not_called()

    def test_completion_without_completed_at_uses_event_timestamp(self):
        turn = dict(self.turn, key='2026-10-28T12:00:00.000Z', payload={})
        now = recovery.datetime.fromisoformat('2026-10-28T12:02:00+00:00').timestamp()
        self.assertEqual(recovery.decision(self.agent, self.goal, turn, self.config, now), 'recover_idle')
        self.assertEqual(recovery.decision(self.agent, self.goal, turn, self.config, now - 1), 'working')
        for key in ('not-a-date', '2026-10-28T12:00:00'):
            self.assertEqual(self.decide(turn=dict(turn, key=key)), 'needs_attention')

    def test_bound_goal_validates_native_public_identity_and_local_id(self):
        config = {'goal_id': 'g', 'thread': 't'}
        local = dict(self.goal, created_at_ms=123456, objective='原始目标')
        remote = {'threadId': 't', 'createdAt': 123, 'objective': '原始目标', 'status': 'active'}
        with patch.object(recovery, 'binding_valid', return_value=True), patch.object(recovery, 'goal', return_value=local), patch.object(recovery, 'rpc', return_value={'goal': remote}):
            self.assertEqual(recovery.bound_goal(config), remote)
            with patch.object(recovery, 'goal', side_effect=[local, dict(local, id='replacement')]):
                with self.assertRaises(ValueError): recovery.bound_goal(config)
            with patch.object(recovery, 'rpc', return_value={'goal': dict(remote, createdAt=124)}):
                with self.assertRaises(ValueError): recovery.bound_goal(config)

    def test_stop_task_does_not_pause_or_interrupt_replacement_goal(self):
        with tempfile.TemporaryDirectory() as folder:
            project = Path(folder).resolve()
            agent = '11111111-1111-4111-8111-111111111111'
            runtime = project / '.agents/project/guards' / agent
            runtime.mkdir(parents=True)
            (runtime / 'config.json').write_text(json.dumps({'goal_id': 'old', 'thread': 't'}))
            with patch.object(recovery, 'ROOT', project), patch.object(sys, 'argv', ['guard.py', 'stop', '--stop-task', '--agent', agent]), patch.object(recovery, 'binding_valid', return_value=True), patch.object(recovery, 'goal', return_value={'id': 'replacement', 'status': 'active'}), patch.object(recovery, 'rpc') as remote, patch.object(recovery, 'run_json') as paseo:
                with self.assertRaises(ValueError): recovery.main()
                remote.assert_not_called(); paseo.assert_not_called()
            self.assertTrue((runtime / 'disabled').exists())

    def test_unavailable_notification_is_recorded_without_external_tool(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder)
            (runtime / 'config.json').write_text(json.dumps({'agent': 'a', 'thread': 't', 'goal_id': 'g'}))
            with patch.object(recovery.sys, 'platform', 'win32'), patch.object(recovery, 'goal', return_value={'id': 'g', 'status': 'complete'}), patch.object(recovery.subprocess, 'run', side_effect=AssertionError('不应启动通知工具')):
                recovery.worker(runtime, os.getppid())
            state = json.loads((runtime / 'state.json').read_text())
            self.assertFalse(state['notification_delivered'])
            self.assertEqual(state['notification_status'], 'unavailable')

    def test_idle_grace_and_generic_error(self):
        self.assertEqual(self.decide(), 'recover_idle')
        self.assertEqual(recovery.decision(self.agent, self.goal, self.turn, self.config, 119), 'working')
        for message in ('429 Too Many Requests', '500 Server Error', 'network timeout'):
            turn = dict(self.turn, payload={'error': {'message': message}})
            self.assertEqual(self.decide(turn=turn), 'recover_error')
        turn = dict(self.turn, payload={'error': {'message': '401 unauthorized'}})
        self.assertEqual(self.decide(turn=turn), 'needs_attention')

    def test_usage_budget_records_remaining_and_excludes_exhausted_windows(self):
        response = quota_snapshot('5600', 50, {'usedPercent': 100, 'resetsAt': 3000, 'windowDurationMins': 60})
        budget = recovery.initialize_usage_budget(response, 123)
        self.assertEqual(budget['baseline_at'], 123)
        self.assertEqual(budget['windows']['primary']['baseline_remaining'], '50')
        self.assertEqual(budget['windows']['primary']['reserve'], '10.0')
        self.assertFalse(budget['windows']['secondary']['participating'])
        self.assertEqual(budget['credits']['reserve'], '1120.0')
        response['rateLimits']['primary']['usedPercent'] = 90
        self.assertEqual(recovery.usage_budget_status(response, budget), 'usage_budget_reached')

    def test_usage_budget_credits_use_exact_threshold_and_do_not_depend_on_ordinary_window(self):
        budget = example_budget()
        response = quota_snapshot('3000.01')
        self.assertEqual(recovery.quota_delay(response, 100, True, budget), 0)
        for balance in ('2999.99', '3000', None, 'NaN', 'Infinity', 'unknown'):
            response['rateLimits']['credits']['balance'] = balance
            response['ordinaryUsageAllowed'] = True
            response['rateLimits']['primary']['usedPercent'] = 1
            self.assertGreater(recovery.quota_delay(response, 100, True, budget), 0)
        self.assertEqual(recovery.quota_delay(response, 100), 0)
        self.assertEqual(recovery.usage_budget_status({}, budget), 'usage_budget_unknown')

    def test_usage_budget_multiple_windows_reset_or_identity_never_rebaseline(self):
        response = quota_snapshot('5600', 40, {'usedPercent': 20, 'resetsAt': 3000, 'windowDurationMins': 60})
        budget = recovery.initialize_usage_budget(response, 123)
        response['rateLimits']['secondary']['usedPercent'] = 84
        self.assertEqual(recovery.usage_budget_status(response, budget), 'usage_budget_reached')
        response['rateLimits']['secondary']['usedPercent'] = 20
        response['rateLimits']['primary']['resetsAt'] += 1
        self.assertEqual(recovery.usage_budget_status(response, budget), 'usage_budget_window_changed')
        response = quota_snapshot('5600', 40, {'usedPercent': 20, 'resetsAt': 3000, 'windowDurationMins': 60})
        response['accountId'] = 'other'
        self.assertEqual(recovery.usage_budget_status(response, budget), 'usage_budget_unknown')
        response['accountId'] = 'account'; response['rateLimits']['credits']['balance'] = '5601'
        self.assertEqual(recovery.usage_budget_status(response, budget), 'usage_budget_credits_changed')

    def test_unfunded_credits_cannot_be_added_and_unlimited_cannot_flip(self):
        response = quota_snapshot('0', 50)
        response['rateLimits']['credits']['hasCredits'] = False
        budget = recovery.initialize_usage_budget(response, 123)
        self.assertFalse(budget['credits']['participating'])
        response['rateLimits']['credits'].update(balance='10', hasCredits=True)
        self.assertEqual(recovery.usage_budget_status(response, budget), 'usage_budget_credits_changed')
        self.assertGreater(recovery.quota_delay(response, 100, True, budget), 0)
        response = quota_snapshot('15000')
        budget = recovery.initialize_usage_budget(response, 123)
        response['rateLimits']['credits']['unlimited'] = True
        self.assertEqual(recovery.usage_budget_status(response, budget), 'usage_budget_credits_changed')
        response['rateLimits']['credits']['unlimited'] = None
        self.assertEqual(recovery.usage_budget_status(response, budget), 'usage_budget_unknown')

    def test_usage_budget_rejects_unknown_initial_snapshot(self):
        for value in ({}, quota_snapshot('NaN'), quota_snapshot(None), quota_snapshot('5600', 101)):
            with self.assertRaises(ValueError):
                recovery.initialize_usage_budget(value, 100)

    def test_usage_budget_pause_never_interrupts_replacement_goal(self):
        config = {'goal_id': 'owned', 'agent': 'agent', 'thread': 'thread'}
        for replacement in (False, True):
            with self.subTest(replacement=replacement), tempfile.TemporaryDirectory() as folder:
                current = {'id': 'owned', 'status': 'active'}
                after = {'id': 'replacement' if replacement else 'owned', 'status': 'paused'}
                with patch.object(recovery, 'binding_valid', return_value=True), \
                     patch.object(recovery, 'goal', side_effect=[current, after]), \
                     patch.object(recovery, 'bound_goal', return_value={'status': 'paused'}), \
                     patch.object(recovery, 'rpc') as rpc, patch.object(recovery, 'run_json') as run:
                    self.assertEqual(recovery.pause_at_usage_budget(Path(folder), config), not replacement)
                    rpc.assert_called_once_with('thread/goal/set', {'threadId': 'thread', 'status': 'paused'})
                    self.assertEqual(run.call_count, 0 if replacement else 1)

    def test_running_worker_checks_floor_and_stops_before_questions_or_recovery(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder)
            config = {'goal_id': 'owned', 'agent': 'agent', 'thread': 'thread',
                      'rollout': 'rollout', 'usage_budget': example_budget()}
            (runtime / 'config.json').write_text(json.dumps(config))
            with patch.object(recovery.os, 'getppid', return_value=42), \
                 patch.object(recovery, 'goal', return_value={'id': 'owned', 'status': 'active'}), \
                 patch.object(recovery, 'binding_valid', return_value=True), \
                 patch.object(recovery, 'inspect', return_value={'Status': 'running'}), \
                 patch.object(recovery, 'timeline', side_effect=RuntimeError('日志不可读')), \
                 patch.object(recovery, 'controlled_turn', return_value=None), \
                 patch.object(recovery, 'rpc', return_value=quota_snapshot('2999.99')), \
                 patch.object(recovery, 'pause_at_usage_budget', return_value=True) as stop, \
                 patch.object(recovery.questions, 'step') as question, patch.object(recovery, 'notify', return_value=False):
                recovery.worker(runtime, 42)
                stop.assert_called_once_with(runtime, config)
                question.assert_not_called()
                self.assertEqual(json.loads((runtime / 'state.json').read_text())['reason'], 'usage_budget_reached')
                stop.side_effect = RuntimeError('暂停协议往返失败')
                with patch.object(recovery, 'sleep_until', side_effect=lambda *_: (runtime / 'disabled').touch()):
                    recovery.worker(runtime, 42)
                self.assertTrue(json.loads((runtime / 'state.json').read_text())['usage_budget_paused'])

    def test_existing_credits_require_task_authorization_and_formal_eligibility(self):
        response = {'ordinaryUsageAllowed': False, 'rateLimits': {
            'primary': {'usedPercent': 100, 'resetsAt': 1000},
            'spendControlReached': False, 'rateLimitReachedType': 'rate_limit_reached',
            'credits': {'hasCredits': True, 'unlimited': False, 'balance': '6342.9273670000'}}}
        self.assertGreater(recovery.quota_delay(response, 100), 0)
        self.assertEqual(recovery.quota_delay(response, 100, True), 0)
        for balance in ('0', '-1', 'NaN', 'Infinity', None, 'invalid'):
            changed = json.loads(json.dumps(response)); changed['rateLimits']['credits']['balance'] = balance
            self.assertGreater(recovery.quota_delay(changed, 100, True), 0)
        for key, value in (('spendControlReached', True), ('spendControlReached', None),
                           ('rateLimitReachedType', 'spend_control_reached'),
                           ('rateLimitReachedType', 'unknown_future_limit')):
            changed = json.loads(json.dumps(response)); changed['rateLimits'][key] = value
            self.assertGreater(recovery.quota_delay(changed, 100, True), 0)
        for key in ('ordinaryUsageAllowed', 'rateLimits'):
            changed = dict(response); changed.pop(key)
            self.assertGreater(recovery.quota_delay(changed, 100, True), 0)
        changed = json.loads(json.dumps(response)); changed['rateLimits']['primary']['usedPercent'] = float('nan')
        self.assertGreater(recovery.quota_delay(changed, 100, True), 0)

    def test_policy_change_rechecks_only_quota_and_survives_restart(self):
        state = {'quota_check_at': 99999999, 'retry_at': 10, 'attempts': 2,
                 'dispatch_key': 'sent', 'questions': {'q': {'status': 'sending'}}}
        preserved = {key: value for key, value in state.items() if key != 'quota_check_at'}
        recovery.apply_task_policy(state, {'allow_existing_credits': True})
        self.assertNotIn('quota_check_at', state)
        self.assertTrue(all(state[key] == value for key, value in preserved.items()))
        state['quota_check_at'] = 123
        restarted = json.loads(json.dumps(state))
        recovery.apply_task_policy(restarted, {'allow_existing_credits': True})
        self.assertEqual(restarted['quota_check_at'], 123)
        recovery.apply_task_policy(restarted, {})
        self.assertNotIn('quota_check_at', restarted)
        self.assertEqual(restarted['questions'], preserved['questions'])

    def test_quota_requires_known_available_windows(self):
        self.assertEqual(recovery.quota_delay({}, 100), 300)
        self.assertEqual(recovery.quota_delay({'ordinaryUsageAllowed': True, 'rateLimits': {'primary': {'usedPercent': 99}}}, 100), 0)
        self.assertEqual(recovery.quota_delay({'ordinaryUsageAllowed': False, 'rateLimits': {'primary': {'usedPercent': 1}}}, 100), 300)
        limits = {'primary': {'usedPercent': 100, 'resetsAt': 150},
                  'secondary': {'usedPercent': 100, 'resetsAt': 500}}
        self.assertEqual(recovery.quota_delay({'ordinaryUsageAllowed': True, 'rateLimits': limits}, 100), 430)
        self.assertEqual(recovery.quota_delay({'ordinaryUsageAllowed': False, 'rateLimits': limits}, 100), 430)
        limits['secondary'].pop('resetsAt')
        self.assertEqual(recovery.quota_delay({'ordinaryUsageAllowed': True, 'rateLimits': limits}, 100), 300)

    def test_new_input_invalidates_old_error(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            path.write_text(json.dumps({'type': 'event_msg', 'timestamp': 'a',
                                        'payload': {'type': 'task_complete', 'error': {'message': '500'}}}) + '\n' +
                            json.dumps({'type': 'event_msg', 'timestamp': 'b', 'payload': {'type': 'user_message'}}) + '\n')
            self.assertEqual(recovery.timeline(path)['kind'], 'user_message')

    def test_unchanged_log_reuses_parse_but_new_event_is_read(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            path.write_text(json.dumps({'type': 'event_msg', 'timestamp': 'a', 'payload': {'type': 'task_started'}}) + '\n')
            self.assertEqual(recovery.timeline(path)['kind'], 'task_started')
            with patch.object(Path, 'open', side_effect=AssertionError('不应重读未变化日志')):
                self.assertEqual(recovery.timeline(path)['kind'], 'task_started')
            with path.open('a') as output:
                output.write(json.dumps({'type': 'event_msg', 'timestamp': 'b', 'payload': {'type': 'task_complete'}}) + '\n')
            self.assertEqual(recovery.timeline(path)['kind'], 'task_complete')

    def test_held_lock_without_supervisor_is_not_active(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder)
            (runtime / 'state.json').write_text(json.dumps({'status': 'working', 'checked_at': time.time()}))
            (runtime / 'supervisor.json').write_text(json.dumps({'pid': os.getpid()}))
            with (runtime / 'lock').open('a') as lock:
                recovery.platform.lock_file(lock)
                view = recovery.summary(runtime)
            self.assertTrue(view['lock_held'])
            self.assertFalse(view['active'])
            self.assertEqual(view['status'], 'orphaned')

    def test_summary_separates_guard_liveness_from_business_execution(self):
        for status, running in (('working', True), ('working', False), ('quota_wait', False), ('cooldown', False)):
            with self.subTest(status=status, running=running), tempfile.TemporaryDirectory() as folder:
                runtime = Path(folder)
                state = {'status': status, 'checked_at': time.time(), 'task_running': running,
                         'reason': 'quota_check_pending' if status == 'quota_wait' else status,
                         'next_recovery_at': 7200 if status == 'quota_wait' else None}
                (runtime / 'state.json').write_text(json.dumps(state))
                with (runtime / 'lock').open('a') as lock, patch.object(recovery, 'supervisor_alive', return_value=True):
                    recovery.platform.lock_file(lock)
                    view = recovery.summary(runtime)
                self.assertTrue(view['active'])
                self.assertTrue(view['guard_alive'])
                self.assertEqual(view['task_running'], running)
                self.assertEqual(view['next_recovery_at'], state['next_recovery_at'])

    def test_finish_rechecks_goal_under_control_lock(self):
        with tempfile.TemporaryDirectory() as folder:
            project = Path(folder).resolve()
            agent = '11111111-1111-4111-8111-111111111111'
            runtime = project / '.agents/project/guards' / agent
            runtime.mkdir(parents=True)
            (runtime / 'config.json').write_text(json.dumps({'goal_id': 'old'}))
            with patch.object(recovery, 'ROOT', project), patch.object(sys, 'argv', ['guard.py', 'finish', '--agent', agent]), patch.object(recovery, 'goal', side_effect=[{'id': 'old', 'status': 'complete'}, {'id': 'new', 'status': 'active'}]):
                with self.assertRaises(SystemExit):
                    recovery.main()
            self.assertFalse((runtime / 'disabled').exists())

    def test_ambiguous_dispatch_is_not_repeated(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder)
            (runtime / 'config.json').write_text(json.dumps({'agent': 'a', 'thread': 't', 'rollout': 'r', 'goal_id': 'g'}))
            (runtime / 'state.json').write_text(json.dumps({'dispatch_key': 't', 'attempts': 3}))
            turn = dict(self.turn, payload={'error': {'message': '500'}})
            with patch.object(recovery, 'binding_valid', return_value=True), patch.object(recovery, 'inspect', return_value=self.agent), patch.object(recovery, 'goal', return_value=self.goal), patch.object(recovery, 'timeline', return_value=turn), patch.object(recovery, 'notify', return_value=True), patch.object(recovery, 'run_json') as send, patch.object(recovery, 'sleep_until', side_effect=lambda *_: (runtime / 'disabled').touch()):
                recovery.worker(runtime, os.getppid())
                send.assert_not_called()

    def test_confirmed_unstarted_send_can_retry_after_cooldown(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder)
            (runtime / 'config.json').write_text(json.dumps({'agent': 'a', 'thread': 't', 'rollout': 'r', 'goal_id': 'g'}))
            (runtime / 'state.json').write_text(json.dumps({'pending_key': 't', 'retry_at': 0}))
            turn = dict(self.turn, payload={'error': {'message': '500'}})
            def remote(method, _params):
                if method == 'account/rateLimits/read':
                    return {'ordinaryUsageAllowed': True, 'rateLimits': {'primary': {'usedPercent': 1}}}
                return {'goal': {'id': 'g', 'status': 'active'}}
            with patch.object(recovery, 'binding_valid', return_value=True), patch.object(recovery, 'inspect', return_value=self.agent), patch.object(recovery, 'goal', return_value=self.goal), patch.object(recovery, 'timeline', return_value=turn), patch.object(recovery, 'notify', return_value=True), patch.object(recovery, 'rpc', side_effect=remote), patch.object(recovery, 'run_json', side_effect=FileNotFoundError('not launched')), patch.object(recovery, 'sleep_until', side_effect=lambda *_: (runtime / 'disabled').touch()):
                recovery.worker(runtime, os.getppid())
            state = json.loads((runtime / 'state.json').read_text())
            self.assertNotIn('dispatch_key', state)
            self.assertEqual(state['check_failures'], 1)

    def test_three_errors_enter_cooldown_instead_of_exiting(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder)
            (runtime / 'config.json').write_text(json.dumps({'agent': 'a', 'thread': 't', 'rollout': 'r', 'goal_id': 'g'}))
            (runtime / 'state.json').write_text(json.dumps({'attempts': 3}))
            turn = dict(self.turn, payload={'error': {'message': '500'}})
            checks = []

            def waiting(*_):
                checks.append(json.loads((runtime / 'state.json').read_text()))
                if len(checks) == 2:
                    (runtime / 'disabled').touch()

            with patch.object(recovery, 'binding_valid', return_value=True), patch.object(recovery, 'inspect', return_value=self.agent), patch.object(recovery, 'goal', return_value=self.goal), patch.object(recovery, 'timeline', return_value=turn), patch.object(recovery, 'notify', return_value=True), patch.object(recovery, 'run_json') as send, patch.object(recovery, 'sleep_until', side_effect=waiting):
                recovery.worker(runtime, os.getppid())
                send.assert_not_called()
            self.assertEqual(len(checks), 2)
            self.assertEqual(checks[0]['status'], 'cooldown')
            self.assertGreaterEqual(checks[0]['retry_at'] - time.time(), 1790)

    @unittest.skipIf(os.name == 'nt', 'POSIX进程组场景')
    def test_cleanup_kills_descendant_after_worker_has_exited(self):
        with tempfile.TemporaryDirectory() as folder:
            pid_file = Path(folder) / 'child.pid'
            child = ('import os,signal,time;from pathlib import Path;'
                     'signal.signal(signal.SIGTERM,signal.SIG_IGN);'
                     f'Path({str(pid_file)!r}).write_text(str(os.getpid()));time.sleep(60)')
            parent = f'import subprocess,sys;subprocess.Popen([sys.executable,"-c",{child!r}])'
            process = subprocess.Popen([sys.executable, '-c', parent], start_new_session=True,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                end = time.monotonic() + 5
                while not pid_file.exists() and time.monotonic() < end:
                    time.sleep(.01)
                self.assertTrue(pid_file.exists())
                process.wait(timeout=5)
                recovery.terminate_child(process)
                pid = int(pid_file.read_text())
                end = time.monotonic() + 3
                while time.monotonic() < end:
                    result = subprocess.run(['ps', '-p', str(pid), '-o', 'stat='], capture_output=True, text=True)
                    if not result.stdout.strip() or result.stdout.lstrip().startswith('Z'):
                        break
                    time.sleep(.1)
                else:
                    self.fail('忽略TERM的协议后代仍在运行')
            finally:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait()

    def test_consecutive_goal_rpc_failures_reach_long_cooldown(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder)
            (runtime / 'config.json').write_text(json.dumps({'agent': 'a', 'thread': 't', 'rollout': 'r', 'goal_id': 'g'}))
            (runtime / 'state.json').write_text(json.dumps({'pending_key': 't', 'retry_at': 0}))
            turn = dict(self.turn, payload={'error': {'message': '500'}})
            clock = [1000]
            failures = []

            def waiting(*_):
                state = json.loads((runtime / 'state.json').read_text())
                failures.append(state)
                clock[0] += 1801
                if state.get('check_failures') == 3:
                    (runtime / 'disabled').touch()

            def remote(method, _params):
                if method == 'account/rateLimits/read':
                    return {'ordinaryUsageAllowed': True, 'rateLimits': {'primary': {'usedPercent': 1}}}
                raise RuntimeError('transport')

            with patch.object(recovery, 'binding_valid', return_value=True), patch.object(recovery, 'inspect', return_value=self.agent), patch.object(recovery, 'goal', return_value=self.goal), patch.object(recovery, 'timeline', return_value=turn), patch.object(recovery, 'notify', return_value=True), patch.object(recovery, 'rpc', side_effect=remote), patch.object(recovery.time, 'time', side_effect=lambda: clock[0]), patch.object(recovery, 'sleep_until', side_effect=waiting):
                recovery.worker(runtime, os.getppid())
            self.assertEqual([s['check_failures'] for s in failures], [1, 2, 3])
            self.assertEqual(failures[-1]['check_after'] - failures[-1]['checked_at'], 1800)


class QuotaCycles(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.runtime = Path(self.temp.name)
        self.response = quota_snapshot('5600', 50)
        self.config = {'goal_id': 'goal', 'agent': 'agent', 'thread': 'thread', 'rollout': 'rollout',
                       'allow_existing_credits': True,
                       'usage_budget': recovery.initialize_usage_budget(self.response, 100)}
        self.credits = dict(self.config['usage_budget']['credits'])

    def refresh(self, now=2001):
        recovery.refresh_usage_cycles(self.runtime, self.config, self.response, now)

    def test_quota_wait_clock_survives_switch_restart_and_failures(self):
        state = {}
        recovery.update_quota_wait(state, 'quota_wait', 'account_budget_sleep', 100)
        self.assertFalse(recovery.quota_wait_expired(state, 3699))
        state = json.loads(json.dumps(state))
        for status, reason in (('quota_wait', 'account_budget_wait'),
                               ('cooldown', 'OSError'), ('starting', 'starting')):
            recovery.update_quota_wait(state, status, reason, 3600)
        self.assertEqual(state['quota_wait_started_at'], 100)
        self.assertTrue(recovery.quota_wait_expired(state, 3700))
        recovery.update_quota_wait(state, 'quota_wait', 'percentage_resumed', 3701)
        self.assertNotIn('quota_wait_started_at', state)
        recovery.update_quota_wait(state, 'needs_attention', 'question_quota_wait', 4000)
        self.assertEqual(state['quota_wait_started_at'], 4000)
        state['task_running'] = True
        recovery.update_quota_wait(state, 'working', 'task_running', 4100)
        self.assertNotIn('quota_wait_started_at', state)

    def test_expired_paused_worker_exits_without_rpc_or_business_stop(self):
        config = self.config
        current = {'id': 'goal', 'status': 'paused'}
        (self.runtime / 'config.json').write_text(json.dumps(config))
        (self.runtime / 'state.json').write_text(json.dumps({
            'quota_wait_started_at': 1, 'dispatch_key': 'keep-unknown',
            'percentage_pause': {'goal': current, 'resume_status': 'waiting'}}))
        with patch.object(recovery.os, 'getppid', return_value=42), \
             patch.object(recovery.time, 'time', return_value=3601), \
             patch.object(recovery, 'goal', return_value=current), \
             patch.object(recovery, 'rpc') as rpc, patch.object(recovery, 'run_json') as run, \
             patch.object(recovery, 'notify', return_value=False):
            recovery.worker(self.runtime, 42)
        state = json.loads((self.runtime / 'state.json').read_text())
        self.assertEqual(state['status'], 'quota_wait_expired')
        self.assertIsNone(state['next_check_at'])
        self.assertEqual(state['dispatch_key'], 'keep-unknown')
        self.assertTrue((self.runtime / 'disabled').exists())
        rpc.assert_not_called(); run.assert_not_called()

    def test_expiry_does_not_depend_on_goal_or_client_availability(self):
        (self.runtime / 'config.json').write_text(json.dumps(self.config))
        for failure in ('goal', 'inspect'):
            with self.subTest(failure=failure):
                (self.runtime / 'disabled').unlink(missing_ok=True)
                (self.runtime / 'state.json').write_text(json.dumps({'quota_wait_started_at': 1}))
                with patch.object(recovery.os, 'getppid', return_value=42), \
                     patch.object(recovery.time, 'time', return_value=3601), \
                     patch.object(recovery, 'goal', **({'side_effect': OSError('unavailable')} if failure == 'goal' else {'return_value': {'id': 'goal', 'status': 'active'}})), \
                     patch.object(recovery, 'binding_valid', return_value=True), \
                     patch.object(recovery, 'inspect', side_effect=OSError('unavailable')), \
                     patch.object(recovery, 'notify', return_value=False):
                    recovery.worker(self.runtime, 42)
                self.assertEqual(json.loads((self.runtime / 'state.json').read_text())['status'], 'quota_wait_expired')

    def test_initial_zero_window_joins_only_after_formal_new_cycle(self):
        original = quota_snapshot('5600', 100)
        self.config['usage_budget'] = recovery.initialize_usage_budget(original, 100)
        self.response['rateLimits']['primary']['usedPercent'] = 0
        self.refresh()
        self.assertFalse(self.config['usage_budget']['windows']['primary']['participating'])
        self.response['rateLimits']['primary']['resetsAt'] = 4000
        self.refresh()
        window = self.config['usage_budget']['windows']['primary']
        self.assertTrue(window['participating'])
        self.assertEqual(window['baseline_remaining'], '100')
        self.assertEqual(window['reserve'], '20.0')
        self.assertEqual(window['cycle_observed_at'], 2001)
        self.assertEqual(self.config['usage_budget']['credits'], self.credits)
        saved = json.loads((self.runtime / 'config.json').read_text())
        self.assertEqual(saved['usage_budget'], self.config['usage_budget'])
        self.refresh(2100)
        self.assertEqual(self.config['usage_budget']['windows']['primary'], window)

    def test_first_available_observation_after_confirmed_reset_sets_baseline(self):
        self.response['rateLimits']['primary'].update(resetsAt=4000, usedPercent=100)
        self.refresh()
        self.assertTrue(self.config['usage_budget']['windows']['primary']['awaiting_available'])
        self.response['rateLimits']['primary']['usedPercent'] = 10
        self.refresh(2010)
        window = self.config['usage_budget']['windows']['primary']
        self.assertEqual(window['baseline_remaining'], '90')
        self.assertEqual(window['reserve'], '18.0')
        self.assertEqual(window['cycle_observed_at'], 2010)

    def test_early_or_unknown_reset_duration_or_credit_never_rebases(self):
        before = json.loads(json.dumps(self.config['usage_budget']))
        self.response['rateLimits']['primary']['resetsAt'] = 4000
        self.refresh(1999)
        self.assertEqual(self.config['usage_budget'], before)
        self.response['rateLimits']['primary']['resetsAt'] = None
        self.refresh()
        self.assertEqual(self.config['usage_budget'], before)
        self.response['rateLimits']['primary'].update(resetsAt=4000, windowDurationMins=60)
        self.refresh()
        self.assertEqual(self.config['usage_budget'], before)
        self.response['rateLimits']['primary']['windowDurationMins'] = 10080
        self.response['rateLimits']['credits']['balance'] = None
        self.refresh()
        self.assertEqual(self.config['usage_budget'], before)

    def monitor_case(self, *, credit='5600', user_changed=False, goal_changed=False, extra_window=False, new_used=0, send_unknown=False, pause_on_quota=None, primary_cause_only=False, credit_on_quota=None, future_wait=False, new_account=False, same_account_recovery=False):
        self.response = quota_snapshot('5600', 50)
        self.config['usage_budget'] = recovery.initialize_usage_budget(self.response, 100)
        if extra_window:
            self.response['rateLimits']['secondary'] = {'usedPercent': 50, 'resetsAt': 3000, 'windowDurationMins': 60}
            self.config['usage_budget'] = recovery.initialize_usage_budget(self.response, 100)
        current = {'id': 'goal', 'status': 'paused', 'updated_at_ms': 123}
        holder = {'goal': dict(current)}
        controls = {'last_user_at': 1, 'user_key': 'u', 'control': None, 'turn_key': 'stopped', 'turn_kind': 'turn_aborted'}
        windows = {name: item['resets_at'] for name, item in self.config['usage_budget']['windows'].items()}
        if primary_cause_only:
            windows = {'primary': windows['primary']}
        state = {'percentage_pause': {'goal': current, 'controls': controls, 'windows': windows,
                                     'resume_status': 'waiting', 'next_quota_at': 3100 if future_wait else 0}}
        if new_account or same_account_recovery:
            self.config['auto_account_recovery'] = True
            state['percentage_pause'].update(account_recovery=True, account_id=self.response['accountId'], windows={})
            if new_account:
                self.response['accountId'] = 'second-account'
        self.response['rateLimits']['primary'].update(usedPercent=new_used, resetsAt=2000 if same_account_recovery else 4000)
        self.response['rateLimits']['credits']['balance'] = credit
        if extra_window:
            self.response['rateLimits']['secondary']['usedPercent'] = 90
        if new_account and extra_window:
            self.response['rateLimits']['secondary'] = None
        fresh_controls = dict(controls, last_user_at=2) if user_changed else controls
        if goal_changed:
            current = dict(current, updated_at_ms=124)
        calls = []
        def remote(method, params):
            calls.append(method)
            if method == 'account/rateLimits/read':
                if credit_on_quota == calls.count('account/rateLimits/read'):
                    self.response['rateLimits']['credits']['balance'] = '1120'
                if pause_on_quota == calls.count('account/rateLimits/read'):
                    holder['goal'] = dict(holder['goal'], status='paused', updated_at_ms=999)
                return self.response
            if method == 'thread/goal/set': holder['goal'] = dict(holder['goal'], status=params['status'], updated_at_ms=125)
            return {}
        with patch.object(recovery, 'binding_valid', return_value=True), \
             patch.object(recovery, 'percentage_pause_controls', return_value=fresh_controls), \
             patch.object(recovery, 'goal', side_effect=lambda _: holder['goal']), \
             patch.object(recovery, 'bound_goal', side_effect=lambda _: {'status': holder['goal']['status']}), \
             patch.object(recovery, 'rpc', side_effect=remote), patch.object(recovery, 'run_json') as send, \
             patch.object(recovery.time, 'time', return_value=2001):
            if send_unknown:
                send.side_effect = RuntimeError('发送结果未知')
            try:
                outcome = recovery.monitor_percentage_pause(self.runtime, state, self.config, current, 2001)
            except RuntimeError:
                outcome = 'send_unknown'
                self.assertEqual(recovery.monitor_percentage_pause(self.runtime, state, self.config, current, 2002),
                                 'percentage_pause_not_owned')
            if outcome == 'percentage_resumed':
                self.assertEqual(recovery.monitor_percentage_pause(self.runtime, state, self.config, current, 2002),
                                 'percentage_pause_not_owned')
            return outcome, calls, send.call_count, state

    def test_same_account_available_quota_resumes_without_cycle_or_account_change(self):
        outcome, calls, sends, state = self.monitor_case(same_account_recovery=True, new_used=50)
        self.assertEqual(outcome, 'percentage_resumed')
        self.assertEqual(sends, 1)
        self.assertEqual(calls.count('thread/goal/set'), 1)
        self.assertEqual(state['percentage_pause']['resume_status'], 'sent')
        self.assertEqual(self.config['usage_budget']['windows']['primary']['baseline_remaining'], '50')
        self.assertEqual(self.config['usage_budget']['windows']['primary']['resets_at'], '2000')

    def test_same_account_recovery_preserves_budget_manual_and_unknown_send_boundaries(self):
        for options in ({'new_used': 90}, {'credit': '1120'}, {'user_changed': True},
                        {'goal_changed': True}, {'future_wait': True}):
            with self.subTest(options=options):
                outcome, calls, sends, state = self.monitor_case(same_account_recovery=True, **options)
                self.assertNotEqual(outcome, 'percentage_resumed')
                self.assertNotIn('thread/goal/set', calls)
                self.assertEqual(sends, 0)
        outcome, calls, sends, state = self.monitor_case(same_account_recovery=True, new_used=50, send_unknown=True)
        self.assertEqual(outcome, 'send_unknown')
        self.assertEqual(sends, 1)
        self.assertEqual(state['percentage_pause']['resume_status'], 'sending')

    def test_long_quota_sleep_has_no_client_calls_and_login_change_wakes_it(self):
        current = {'id': 'goal', 'status': 'paused'}
        pause = {'goal': current, 'resume_status': 'waiting', 'account_recovery': True,
                 'login_revision': [1, 2, 3], 'next_quota_at': 999999, 'controls': {}}
        state = {'percentage_pause': pause}
        with patch.object(recovery, 'binding_valid', return_value=True), \
             patch.object(recovery, 'login_revision', return_value=[1, 2, 3]), \
             patch.object(recovery, 'percentage_pause_controls') as controls, patch.object(recovery, 'rpc') as rpc:
            self.assertEqual(recovery.monitor_percentage_pause(self.runtime, state, self.config, current, 100),
                             'account_budget_sleep')
            controls.assert_not_called(); rpc.assert_not_called()
        with patch.object(recovery, 'binding_valid', return_value=True), \
             patch.object(recovery, 'login_revision', return_value=[2, 2, 3]), \
             patch.object(recovery, 'percentage_pause_controls', return_value={}), \
             patch.object(recovery, 'rpc', return_value={}) as rpc:
            recovery.monitor_percentage_pause(self.runtime, state, self.config, current, 200)
            rpc.assert_called_once_with('account/rateLimits/read', {})
        self.assertEqual(pause['login_revision'], [2, 2, 3])

    def test_client_failure_enters_sleep_without_repeating_until_deadline(self):
        current = {'id': 'goal', 'status': 'paused'}
        pause = {'goal': current, 'resume_status': 'waiting', 'account_recovery': True,
                 'login_revision': [1, 2, 3], 'next_quota_at': 0, 'controls': {}}
        state = {'percentage_pause': pause}
        for fail_controls in (True, False):
            with self.subTest(fail_controls=fail_controls):
                pause.update(next_quota_at=0, wait_attempts=0)
                with patch.object(recovery, 'binding_valid', return_value=True), \
                     patch.object(recovery, 'login_revision', return_value=[1, 2, 3]), \
                     patch.object(recovery, 'percentage_pause_controls', **({'side_effect': OSError('offline')} if fail_controls else {'return_value': {}})) as controls, \
                     patch.object(recovery, 'rpc', side_effect=OSError('offline')) as rpc:
                    self.assertEqual(recovery.monitor_percentage_pause(self.runtime, state, self.config, current, 100),
                                     'account_budget_unavailable')
                    self.assertEqual(pause['next_quota_at'], 400)
                    self.assertEqual(recovery.monitor_percentage_pause(self.runtime, state, self.config, current, 220),
                                     'account_budget_sleep')
                    self.assertEqual(controls.call_count, 1)
                    self.assertEqual(rpc.call_count, 0 if fail_controls else 1)
                    recovery.monitor_percentage_pause(self.runtime, state, self.config, current, 400)
                    self.assertEqual(pause['next_quota_at'], 1000)

    def test_exhausted_accounts_back_off_and_honour_near_reset(self):
        pause = {}; response = quota_snapshot('0', 100)
        response['rateLimits']['primary']['resetsAt'] = 600000
        delays = [recovery.budget_wait_deadline(pause, response, 100) - 100 for _ in range(10)]
        self.assertEqual(delays[:3], [300, 600, 1200])
        self.assertEqual(delays[-1], 21600)
        response['rateLimits']['primary'].update(resetsAt=700, usedPercent=80)
        self.assertEqual(recovery.budget_wait_deadline(pause, response, 100), 730)

    def test_new_single_window_account_can_resume_old_dual_window_pause(self):
        outcome, calls, sends, state = self.monitor_case(new_account=True, credit='0', extra_window=True)
        self.assertEqual(outcome, 'percentage_resumed')
        self.assertEqual(sends, 1)

    def test_new_account_recovers_owned_credit_pause_once(self):
        outcome, calls, sends, state = self.monitor_case(new_account=True, credit='0')
        self.assertEqual(outcome, 'percentage_resumed')
        self.assertEqual(sends, 1)
        self.assertEqual(calls.count('thread/goal/set'), 1)
        self.assertEqual(self.config['usage_budget']['account_id'], 'second-account')
        self.assertEqual(self.config['usage_budget']['windows']['primary']['reserve'], '20.0')
        self.assertEqual(state['percentage_pause']['resume_status'], 'sent')

    def test_new_account_cannot_override_manual_pause_or_unknown_send(self):
        for options in ({'user_changed': True}, {'goal_changed': True}, {'send_unknown': True}):
            with self.subTest(options=options):
                outcome, calls, sends, state = self.monitor_case(new_account=True, credit='0', **options)
                self.assertNotEqual(outcome, 'percentage_resumed')
                self.assertLessEqual(sends, 1 if options.get('send_unknown') else 0)

    def test_account_switch_back_keeps_reserve_and_rejects_invalid_quota(self):
        original = json.loads(json.dumps(self.config['usage_budget']))
        changed = quota_snapshot('0', 0); changed['accountId'] = 'second-account'
        for field, value in (('ordinaryUsageAllowed', False), ('accountId', None)):
            invalid = dict(changed, **{field: value})
            self.assertFalse(recovery.refresh_account_budget(self.runtime, self.config, invalid, 200))
            self.assertEqual(self.config['usage_budget'], original)
        self.assertTrue(recovery.refresh_account_budget(self.runtime, self.config, changed, 200))
        second = json.loads(json.dumps(self.config['usage_budget']))
        old = quota_snapshot('1120', 50)
        self.assertTrue(recovery.refresh_account_budget(self.runtime, self.config, old, 300))
        self.assertEqual(self.config['usage_budget'], original)
        self.assertEqual(recovery.usage_budget_status(old, self.config['usage_budget']), 'usage_budget_reached')
        self.assertTrue(recovery.refresh_account_budget(self.runtime, self.config, changed, 400))
        self.assertEqual(self.config['usage_budget'], second)
        persisted = json.loads((self.runtime / 'config.json').read_text())
        self.assertEqual(persisted['usage_budget'], second)
        self.assertEqual(len(persisted['account_budgets']), 2)

    def test_credit_pause_has_ownership_and_keeps_account_monitoring(self):
        self.config['auto_account_recovery'] = True
        current = {'id': 'goal', 'status': 'paused', 'updated_at_ms': 123}
        controls = {'last_user_at': 'stamp', 'user_key': 'u', 'control': None}
        state = {}
        with patch.object(recovery, 'percentage_pause_controls', return_value=controls), \
             patch.object(recovery, 'goal', return_value=current):
            self.assertTrue(recovery.owned_percentage_pause(self.runtime, state, self.config, controls, {}))
        state['percentage_pause']['next_quota_at'] = 0
        exhausted = quota_snapshot('1120', 50)
        with patch.object(recovery, 'binding_valid', return_value=True), \
             patch.object(recovery, 'percentage_pause_controls', return_value=controls), \
             patch.object(recovery, 'rpc', return_value=exhausted), patch.object(recovery, 'run_json') as send:
            result = recovery.monitor_percentage_pause(self.runtime, state, self.config, current, 200)
        self.assertEqual(result, 'account_budget_wait')
        self.assertEqual(state['percentage_pause']['resume_status'], 'waiting')
        self.assertEqual(state['percentage_pause']['next_quota_at'], 500)
        send.assert_not_called()

    def test_only_owned_percentage_pause_resumes_once_after_reset(self):
        outcome, calls, sends, state = self.monitor_case()
        self.assertEqual(outcome, 'percentage_resumed')
        self.assertEqual(sends, 1)
        self.assertEqual(calls.count('thread/goal/set'), 1)
        self.assertEqual(state['percentage_pause']['resume_status'], 'sent')
        self.assertEqual(self.config['usage_budget']['credits'], self.credits)

    def test_manual_user_or_goal_change_and_credit_reserve_never_resume(self):
        for options in ({'user_changed': True}, {'goal_changed': True}, {'credit': '1120'}):
            with self.subTest(options=options):
                outcome, calls, sends, state = self.monitor_case(**options)
                self.assertNotEqual(outcome, 'percentage_resumed')
                self.assertEqual(sends, 0)
                self.assertNotIn('thread/goal/set', calls)

    def test_confirmed_reset_still_exhausted_cannot_resume_on_credits(self):
        outcome, calls, sends, state = self.monitor_case(new_used=100)
        self.assertEqual(outcome, 'percentage_reset_wait')
        self.assertEqual(sends, 0)
        self.assertNotIn('thread/goal/set', calls)
        self.assertTrue(self.config['usage_budget']['windows']['primary']['awaiting_available'])
        self.assertEqual(state['percentage_pause']['next_quota_at'], 2301)

    def test_send_result_unknown_is_never_retried(self):
        outcome, calls, sends, state = self.monitor_case(send_unknown=True)
        self.assertEqual(outcome, 'send_unknown')
        self.assertEqual(sends, 1)
        self.assertEqual(state['percentage_pause']['resume_status'], 'sending')

    def test_manual_pause_during_either_quota_rpc_is_not_overwritten_or_sent(self):
        for number in (2, 3):
            with self.subTest(quota_rpc=number):
                outcome, calls, sends, state = self.monitor_case(pause_on_quota=number)
                self.assertNotEqual(outcome, 'percentage_resumed')
                self.assertEqual(sends, 0)
                if number == 2:
                    self.assertNotIn('thread/goal/set', calls)
                else:
                    self.assertEqual(calls.count('thread/goal/set'), 1)
                    self.assertEqual(outcome, 'percentage_resume_unconfirmed')

    def test_newly_depleted_other_window_joins_reset_wait(self):
        outcome, calls, sends, state = self.monitor_case(extra_window=True, primary_cause_only=True)
        self.assertEqual(outcome, 'percentage_reset_wait')
        self.assertIn('secondary', state['percentage_pause']['windows'])
        self.assertEqual(sends, 0)

    def test_credit_reserve_hit_during_fresh_quota_check_latches_hard_boundary(self):
        outcome, calls, sends, state = self.monitor_case(credit_on_quota=2)
        self.assertEqual(outcome, 'percentage_pause_credit_boundary')
        self.assertEqual(state['percentage_pause']['resume_status'], 'credit_reserve_or_state')
        self.assertNotIn('thread/goal/set', calls)
        self.assertEqual(sends, 0)

    def test_wait_deadline_uses_local_controls_without_querying_quota_or_model(self):
        outcome, calls, sends, state = self.monitor_case(future_wait=True)
        self.assertEqual(outcome, 'percentage_reset_wait')
        self.assertEqual(calls, [])
        self.assertEqual(sends, 0)

    def test_generic_recovery_cannot_bypass_unknown_percentage_resume(self):
        (self.runtime / 'config.json').write_text(json.dumps(self.config))
        (self.runtime / 'state.json').write_text(json.dumps({'percentage_pause': {'resume_status': 'sending'}}))
        turn = {'kind': 'task_complete', 'key': 'old', 'payload': {'completed_at': 0}, 'tools': 1}
        with patch.object(recovery.os, 'getppid', return_value=42), \
             patch.object(recovery, 'goal', return_value={'id': 'goal', 'status': 'active'}), \
             patch.object(recovery, 'binding_valid', return_value=True), \
             patch.object(recovery, 'inspect', return_value={'Status': 'idle'}), \
             patch.object(recovery, 'timeline', return_value=turn), \
             patch.object(recovery, 'controlled_turn', return_value=turn), \
             patch.object(recovery, 'rpc', return_value=self.response), \
             patch.object(recovery, 'notify', return_value=False), \
             patch.object(recovery, 'sleep_until', side_effect=lambda *_: (self.runtime / 'disabled').touch()), \
             patch.object(recovery.questions, 'step') as question, patch.object(recovery, 'run_json') as send:
            recovery.worker(self.runtime, 42)
            question.assert_not_called(); send.assert_not_called()
            saved = json.loads((self.runtime / 'state.json').read_text())
            self.assertEqual(saved['percentage_pause']['resume_status'], 'sending')

    def test_new_percentage_breach_cannot_replace_unknown_resume_intent(self):
        for phase in ('sending', 'activating'):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as folder:
                runtime = Path(folder)
                (runtime / 'config.json').write_text(json.dumps(self.config))
                (runtime / 'state.json').write_text(json.dumps({'percentage_pause': {'resume_status': phase}}))
                self.response['rateLimits']['primary']['usedPercent'] = 90
                with patch.object(recovery.os, 'getppid', return_value=42), \
                     patch.object(recovery, 'goal', return_value={'id': 'goal', 'status': 'active'}), \
                     patch.object(recovery, 'binding_valid', return_value=True), \
                     patch.object(recovery, 'rpc', return_value=self.response), \
                     patch.object(recovery, 'notify', return_value=False), \
                     patch.object(recovery, 'pause_at_usage_budget', return_value=True) as stop, \
                     patch.object(recovery, 'owned_percentage_pause') as own, \
                     patch.object(recovery, 'run_json') as send:
                    recovery.worker(runtime, 42)
                    stop.assert_called_once(); own.assert_not_called(); send.assert_not_called()
                    saved = json.loads((runtime / 'state.json').read_text())
                    self.assertEqual(saved['percentage_pause']['resume_status'], phase)

    def test_missing_official_user_watermark_cannot_own_auto_pause(self):
        agent = {'Id': 'agent', 'PendingPermissions': [], 'QuestionBinding': {
            'id': 'agent', 'thread': 'thread', 'lastUserMessageAt': None}}
        with patch.object(recovery, 'inspect', return_value=agent), \
             patch.object(recovery, 'timeline', return_value={'key': 't', 'user_context': None}):
            self.assertIsNone(recovery.percentage_pause_controls(self.config))

    def test_one_window_reset_cannot_release_other_window_reserve(self):
        outcome, calls, sends, state = self.monitor_case(extra_window=True)
        self.assertEqual(outcome, 'percentage_reset_wait')
        self.assertNotIn('thread/goal/set', calls)
        self.assertEqual(sends, 0)
        self.assertGreaterEqual(state['percentage_pause']['next_quota_at'], 2301)


class Lifecycle(unittest.TestCase):
    """在临时副本与假CLI中执行真实start/stop/监督进程，不接触开发总控。"""
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()
        self.script = self.root / 'reusable-skill/scripts/guard.py'
        self.script.parent.mkdir(parents=True)
        shutil.copyfile(SCRIPT, self.script)
        shutil.copyfile(SCRIPT.with_name('guard_platform.py'), self.script.with_name('guard_platform.py'))
        shutil.copyfile(SCRIPT.with_name('guard_questions.py'), self.script.with_name('guard_questions.py'))
        self.agent = '11111111-1111-4111-8111-111111111111'
        self.thread = '22222222-2222-4222-8222-222222222222'
        self.home = self.root / 'codex'; self.home.mkdir()
        self.paseo = self.root / 'paseo'
        record = self.paseo / 'agents' / str(self.root).lstrip('/').replace('/', '-') / (self.agent + '.json')
        record.parent.mkdir(parents=True)
        self.record = record
        record.write_text(json.dumps({'id': self.agent, 'cwd': str(self.root), 'provider': 'codex',
                                     'persistence': {'provider': 'codex', 'sessionId': self.thread,
                                                     'metadata': {'threadId': self.thread}}}))
        rollout = self.home / (self.thread + '.jsonl')
        rollout.write_text(json.dumps({'payload': {'id': self.thread, 'source': 'cli'}}) + '\n')
        with sqlite3.connect(self.home / 'state_5.sqlite') as db:
            db.execute('CREATE TABLE threads(id,rollout_path,cwd)')
            db.execute('INSERT INTO threads VALUES(?,?,?)', (self.thread, str(rollout), str(self.root)))
        with sqlite3.connect(self.home / 'goals_1.sqlite') as db:
            db.execute('CREATE TABLE thread_goals(thread_id,goal_id,status,created_at_ms,objective,updated_at_ms)')
            db.execute('INSERT INTO thread_goals VALUES(?,?,?,?,?,?)', (self.thread, 'goal', 'active', 123000, 'test', 123000))
        binaries = self.root / 'bin'; binaries.mkdir()
        code = '''import json,os,sys,sqlite3
from pathlib import Path
root=Path(os.environ['TEST_ROOT'])
if Path(sys.argv[0]).name=='paseo':
 if sys.argv[1]=='inspect':
  if (root/'pause-inspect').exists():
   import time
   (root/'inspect-entered').touch()
   while not (root/'release-inspect').exists():time.sleep(.01)
  print(json.dumps({'Status':'idle' if (root/'main-stopped').exists() else 'running','Provider':'codex','Cwd':str(root)}))
 else:
  if sys.argv[1]=='stop':(root/'main-stopped').touch()
  print('{}')
else:
 for line in sys.stdin:
  request=json.loads(line);method=request.get('method')
  if 'id' not in request:continue
  result={}
  if method=='thread/goal/get':
   with sqlite3.connect(root/'codex/goals_1.sqlite') as db:row=db.execute('SELECT goal_id,status,created_at_ms,objective FROM thread_goals').fetchone()
   result={'goal':{'threadId':os.environ['CODEX_THREAD_ID'],'status':row[1],'createdAt':row[2]//1000,'objective':row[3]}}
  if method=='account/rateLimits/read':
   result={'accountId':'account','ordinaryUsageAllowed':True,'rateLimits':{'limitId':'codex','primary':{'usedPercent':1,'resetsAt':2000,'windowDurationMins':60},'credits':{'hasCredits':True,'unlimited':False,'balance':(root/'balance').read_text() if (root/'balance').exists() else '15000'},'spendControlReached':False}}
  if method=='thread/goal/set':
   with sqlite3.connect(root/'codex/goals_1.sqlite') as db:db.execute('UPDATE thread_goals SET status=?',(request['params']['status'],))
  print(json.dumps({'id':request['id'],'result':result}),flush=True)
'''
        for name in ('paseo', 'codex', 'osascript'):
            path = binaries / name
            path.write_text('#!' + sys.executable + '\n' + code)
            path.chmod(0o755)
        self.env = dict(os.environ, PATH=str(binaries) + os.pathsep + os.environ['PATH'],
                        CODEX_HOME=str(self.home), PASEO_HOME=str(self.paseo), TEST_ROOT=str(self.root),
                        CODEX_THREAD_ID=self.thread, PASEO_AGENT_ID=self.agent)
        if os.name == 'nt':
            # 临时测试副本以 Python 假CLI替代shebang；不依赖Node或修改系统PATHEXT。
            helper = self.script.with_name('guard_platform.py')
            with helper.open('a', encoding='utf-8') as output:
                output.write("\n_original_command = command\ndef command(arguments):\n"
                             "    if arguments[0] in ('paseo', 'codex'):\n"
                             "        return [os.sys.executable, str(Path(os.environ['TEST_ROOT']) / 'bin' / arguments[0]), *arguments[1:]]\n"
                             "    return _original_command(arguments)\n")
        self.runtime = self.root / '.agents/project/guards' / self.agent

    def call(self, command, *args, check=True):
        return subprocess.run([sys.executable, '-B', str(self.script), command, *args],
                              env=self.env, cwd=self.root, capture_output=True, text=True, timeout=55, check=check)

    def wait_for(self, predicate, timeout=20):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if predicate(): return
            time.sleep(.1)
        self.fail('未在期限内达到预期进程状态')

    def tearDown(self):
        if self.runtime.exists():
            (self.runtime / 'disabled').touch()
            self.wait_for(lambda: not (self.runtime / 'supervisor.json').exists(), timeout=45)
        self.temp.cleanup()

    def test_binding_mismatch_cannot_enrol(self):
        value = json.loads(self.record.read_text())
        value['persistence']['sessionId'] = 'other'
        self.record.write_text(json.dumps(value))
        self.assertNotEqual(self.call('start', check=False).returncode, 0)
        self.assertFalse((self.runtime / 'supervisor.json').exists())

    def test_start_is_singleton_stop_cleans_processes_and_pid(self):
        first = json.loads(self.call('start').stdout)
        self.assertEqual((self.runtime.parent / '.gitignore').read_text(), '*\n!.gitignore\n')
        self.assertFalse((self.root / '.gitignore').exists())
        self.assertNotEqual(self.call('finish', check=False).returncode, 0)
        self.assertFalse((self.runtime / 'disabled').exists())
        self.assertEqual(json.loads(self.call('start').stdout)['status'], 'already_running')
        self.wait_for(lambda: (self.runtime / 'worker.json').exists())
        self.assertTrue(json.loads(self.call('status').stdout)['lock_held'])
        self.call('stop', '--stop-task')
        self.assertFalse((self.runtime / 'supervisor.json').exists())
        self.assertFalse((self.runtime / 'worker.json').exists())
        self.assertFalse(json.loads(self.call('status').stdout)['lock_held'])
        self.assertTrue((self.runtime / 'disabled').exists())
        self.assertTrue((self.root / 'main-stopped').exists())
        with sqlite3.connect(self.home / 'goals_1.sqlite') as db:
            self.assertEqual(db.execute('SELECT status FROM thread_goals').fetchone()[0], 'paused')
        self.assertGreater(first['pid'], 0)

    def test_plain_guard_restart_preserves_backoff_and_uncertain_dispatch(self):
        self.call('start')
        self.call('stop')
        original_budget = json.loads((self.runtime / 'config.json').read_text())['usage_budget']
        state_path = self.runtime / 'state.json'
        state = json.loads(state_path.read_text())
        state.update(attempts=2, retry_at=time.time() + 3600, pending_key='original', dispatch_key='uncertain',
                     quota_check_at=time.time() + 86400, questions={'q': {'status': 'sending'}},
                     goal_control={'status': 'blocked', 'key': 'original-control'})
        state_path.write_text(json.dumps(state))
        self.call('start', '--allow-existing-credits', '--question-model', 'gpt-6.1-sol')
        self.call('stop')
        saved_budget = json.loads((self.runtime / 'config.json').read_text())['usage_budget']
        self.assertEqual(saved_budget, original_budget)
        after = json.loads(state_path.read_text())
        for key in ('attempts', 'retry_at', 'pending_key', 'dispatch_key', 'questions'):
            self.assertEqual(after[key], state[key])
        self.assertNotIn('quota_check_at', after)
        self.call('start')
        self.call('stop')
        config = json.loads((self.runtime / 'config.json').read_text())
        self.assertTrue(config['allow_existing_credits'])
        self.assertEqual(config['question_model'], 'gpt-6.1-sol')
        self.assertEqual(config['usage_budget'], saved_budget)

    def test_only_a_new_goal_creates_a_new_budget_baseline(self):
        self.call('start'); self.call('stop')
        before = json.loads((self.runtime / 'config.json').read_text())['usage_budget']
        (self.root / 'balance').write_text('8000')
        with sqlite3.connect(self.home / 'goals_1.sqlite') as db:
            db.execute("UPDATE thread_goals SET goal_id='new-goal'")
        self.call('start'); self.call('stop')
        config = json.loads((self.runtime / 'config.json').read_text())
        self.assertEqual(config['goal_id'], 'new-goal')
        self.assertEqual(config['usage_budget']['credits']['baseline_remaining'], '8000')
        self.assertEqual(config['usage_budget']['credits']['reserve'], '1600.0')
        self.assertNotEqual(config['usage_budget']['baseline_at'], before['baseline_at'])

    def test_supervisor_bounds_long_running_log(self):
        self.call('start')
        self.wait_for(lambda: (self.runtime / 'worker.json').exists())
        log = self.runtime / 'watch.log'
        with log.open('ab') as output:
            output.write(b'x' * (recovery.TAIL_BYTES * 3))
        self.wait_for(lambda: log.stat().st_size <= recovery.TAIL_BYTES * 2)
        self.call('stop')

    def test_quota_timeout_reaps_processes_and_explicit_start_reopens_wait(self):
        self.call('start'); self.call('stop')
        state_path = self.runtime / 'state.json'
        state = json.loads(state_path.read_text())
        (self.root / 'main-stopped').touch()
        state.update(quota_wait_started_at=time.time() - 3601, dispatch_key='preserve')
        state_path.write_text(json.dumps(state))
        self.call('start')
        self.wait_for(lambda: json.loads(state_path.read_text()).get('status') == 'quota_wait_expired')
        self.wait_for(lambda: not (self.runtime / 'supervisor.json').exists())
        self.assertFalse((self.runtime / 'worker.json').exists())
        self.assertFalse((self.runtime / 'pid').exists())
        status = json.loads(self.call('status').stdout)
        self.assertEqual(status['status'], 'quota_wait_expired')
        self.assertFalse(status['guard_alive'])
        before = json.loads((self.runtime / 'config.json').read_text())['usage_budget']
        self.call('start')
        self.wait_for(lambda: (self.runtime / 'worker.json').exists())
        state = json.loads(state_path.read_text())
        self.assertNotIn('quota_wait_started_at', state)
        self.assertNotIn('quota_wait_expired_at', state)
        self.assertEqual(state['dispatch_key'], 'preserve')
        self.assertEqual(json.loads((self.runtime / 'config.json').read_text())['usage_budget'], before)
        self.call('stop')

    def test_completed_goal_automatically_cleans_guard(self):
        self.call('start')
        self.wait_for(lambda: (self.runtime / 'worker.json').exists())
        with sqlite3.connect(self.home / 'goals_1.sqlite') as db:
            db.execute("UPDATE thread_goals SET status='complete'")
        self.call('finish')
        self.wait_for(lambda: not (self.runtime / 'supervisor.json').exists())
        self.assertFalse((self.runtime / 'worker.json').exists())
        self.assertEqual(json.loads((self.runtime / 'state.json').read_text())['status'], 'complete')

    @unittest.skipIf(os.name == 'nt', 'POSIX SIGKILL 注入；原生Windows Job 清理另测')
    def test_killed_worker_is_restarted_without_second_supervisor(self):
        self.call('start')
        self.wait_for(lambda: (self.runtime / 'worker.json').exists())
        old = json.loads((self.runtime / 'worker.json').read_text())['pid']
        os.kill(old, signal.SIGKILL)
        self.wait_for(lambda: json.loads((self.runtime / 'worker.json').read_text())['pid'] != old, timeout=40)
        self.assertEqual(json.loads(self.call('start').stdout)['status'], 'already_running')

    def test_stop_wins_while_start_is_registering(self):
        (self.root / 'pause-inspect').touch()
        command = [sys.executable, '-B', str(self.script)]
        start = subprocess.Popen(command + ['start'], env=self.env, cwd=self.root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        stop = None
        try:
            self.wait_for(lambda: (self.root / 'inspect-entered').exists())
            stop = subprocess.Popen(command + ['stop'], env=self.env, cwd=self.root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            self.wait_for(lambda: (self.runtime / 'disabled').exists())
            (self.root / 'release-inspect').touch()
            start.communicate(timeout=10)
            _, error = stop.communicate(timeout=20)
            self.assertEqual(stop.returncode, 0, error)
            self.assertTrue((self.runtime / 'disabled').exists())
            self.assertFalse((self.runtime / 'supervisor.json').exists())
        finally:
            (self.root / 'release-inspect').touch()
            for process in (start, stop):
                if process and process.poll() is None:
                    process.terminate()
                    process.communicate(timeout=10)


class PlatformBoundaries(unittest.TestCase):
    def test_pipe_timeout_and_unicode_without_select(self):
        process = subprocess.Popen([sys.executable, '-u', '-c',
                                    "import sys,time;sys.stdout.reconfigure(encoding='utf-8');print('中文');time.sleep(10)"], stdout=subprocess.PIPE)
        lines = recovery.platform.Lines(process.stdout)
        try:
            self.assertEqual(lines.get(time.monotonic() + 5).decode('utf-8').strip(), '中文')
            with self.assertRaises(TimeoutError):
                lines.get(time.monotonic() + .05)
        finally:
            process.kill(); process.wait(); process.stdout.close(); lines.close()
        self.assertFalse(lines.thread.is_alive())

    def test_npm_cmd_resolves_bin_without_shell_interpretation(self):
        with tempfile.TemporaryDirectory() as folder:
            base = Path(folder)
            shim = base / 'paseo.cmd'; shim.touch()
            package = base / 'node_modules/@vendor/paseo'; package.mkdir(parents=True)
            entry = package / 'cli.js'; entry.touch()
            (package / 'package.json').write_text(json.dumps({'bin': {'paseo': 'cli.js'}}))
            with patch.object(recovery.platform, 'WINDOWS', True), patch.object(recovery.platform.shutil, 'which', side_effect=lambda name: str(shim) if name == 'paseo' else 'node'):
                argument = '原目标 %PATH% & echo $HOME'
                result = recovery.platform.command(['paseo', 'send', argument])
                self.assertEqual(result, ['node', str(entry.resolve()), 'send', argument])
                (package / 'package.json').write_text(json.dumps({'bin': {'paseo': '../../escape.js'}}))
                with self.assertRaises(FileNotFoundError):
                    recovery.platform.command(['paseo'])

    def test_start_paused_goal_rechecks_identity_before_activation(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            agent = '11111111-1111-4111-8111-111111111111'
            config = {'agent': agent, 'thread': 't', 'goal_id': 'old'}
            runtime = root / '.agents/project/guards' / agent
            runtime.mkdir(parents=True)
            (runtime / 'config.json').write_text(json.dumps(dict(config, usage_budget=example_budget())))
            with patch.object(recovery, 'ROOT', root), patch.object(sys, 'argv', ['guard.py', 'start', '--agent', agent]), patch.object(recovery.platform, 'command'), patch.object(recovery, 'config_for', return_value=config), patch.object(recovery, 'goal', return_value={'id': 'old', 'status': 'paused'}), patch.object(recovery, 'bound_goal', side_effect=[{'status': 'paused'}, ValueError('replaced')]), patch.object(recovery, 'rpc') as remote:
                with self.assertRaises(ValueError): recovery.main()
                remote.assert_not_called()

    def test_plain_restart_never_reactivates_usage_budget_pause(self):
        for marker, balance in ((True, '6000'), (False, '3000'), (False, None)):
            with self.subTest(marker=marker, balance=balance), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                agent = '11111111-1111-4111-8111-111111111111'
                runtime = root / '.agents/project/guards' / agent
                runtime.mkdir(parents=True)
                (runtime / 'state.json').write_text(json.dumps({'reason': 'stopped', 'usage_budget_paused': marker}))
                config = {'agent': agent, 'thread': 't', 'goal_id': 'old'}
                (runtime / 'config.json').write_text(json.dumps(dict(config, usage_budget=example_budget())))
                with patch.object(recovery, 'ROOT', root), \
                     patch.object(sys, 'argv', ['guard.py', 'start', '--agent', agent]), \
                     patch.object(recovery.platform, 'command'), patch.object(recovery, 'config_for', return_value=config), \
                     patch.object(recovery, 'goal', return_value={'id': 'old', 'status': 'paused'}), \
                     patch.object(recovery, 'bound_goal', return_value={'status': 'paused'}), \
                     patch.object(recovery, 'rpc', return_value=quota_snapshot(balance)) as remote:
                    with self.assertRaises(SystemExit):
                        recovery.main()
                    self.assertFalse(any(c.args[0] == 'thread/goal/set' for c in remote.call_args_list))

    def test_missing_dependency_does_not_create_runtime(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            agent = '11111111-1111-4111-8111-111111111111'
            with patch.object(recovery, 'ROOT', root), patch.object(sys, 'argv', ['guard.py', 'start', '--agent', agent]), patch.object(recovery.platform, 'command', side_effect=FileNotFoundError('missing')):
                with self.assertRaises(FileNotFoundError):
                    recovery.main()
            self.assertFalse((root / '.agents').exists())

    def test_lock_exclusion_and_release_across_real_process(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'lock'
            code = ("import sys;sys.path.insert(0,sys.argv[1]);import guard_platform as p;"
                    "f=open(sys.argv[2],'a');p.lock_file(f);print('ready',flush=True);sys.stdin.read()")
            process = subprocess.Popen([sys.executable, '-B', '-c', code, str(SCRIPT.parent), str(path)],
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE)
            try:
                self.assertEqual(process.stdout.readline().strip(), b'ready')
                with path.open('a') as lock:
                    with self.assertRaises(BlockingIOError):
                        recovery.platform.lock_file(lock, blocking=False)
                process.communicate(timeout=5)
                with path.open('a') as lock:
                    recovery.platform.lock_file(lock, blocking=False)
            finally:
                if process.poll() is None:
                    process.kill(); process.communicate()



@unittest.skipIf(os.name == 'nt', 'POSIX生命管道与进程组回归')
class PosixOwnedTools(unittest.TestCase):
    def wait_for(self, condition, seconds=8):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if condition(): return
            time.sleep(.05)
        self.fail('工具树未在期限内回收')

    def dead(self, pid):
        result = subprocess.run(['ps', '-p', str(pid), '-o', 'stat='], capture_output=True, text=True, check=False)
        return not result.stdout.strip() or result.stdout.lstrip().startswith('Z')

    def tool(self, marker):
        descendant = 'import signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);time.sleep(60)'
        return ('import os,signal,subprocess,sys,time,json;from pathlib import Path;'
                'signal.signal(signal.SIGTERM,signal.SIG_IGN);'
                f'p=subprocess.Popen([sys.executable,"-c",{descendant!r}]);'
                f'Path({str(marker)!r}).write_text(json.dumps([os.getpid(),p.pid]));time.sleep(60)')

    def test_actual_launch_tool_tree_dies_after_worker_sigkill(self):
        with tempfile.TemporaryDirectory() as folder:
            marker = Path(folder) / 'tool.json'
            bootstrap = Path(folder) / 'bootstrap.pid'
            code = ('import sys,time;from pathlib import Path;'
                    f'sys.path.insert(0,{str(SCRIPT.parent)!r});import guard_platform as p;'
                    f'q=p.launch_tool([sys.executable,"-c",{self.tool(marker)!r}],stdin=p.subprocess.PIPE,stdout=p.subprocess.DEVNULL,stderr=p.subprocess.DEVNULL);'
                    f'Path({str(bootstrap)!r}).write_text(str(q.pid));time.sleep(60)')
            unrelated = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(60)'])
            worker = subprocess.Popen([sys.executable, '-B', '-c', code], start_new_session=True)
            try:
                self.wait_for(lambda: marker.exists() and bootstrap.exists())
                pids = json.loads(marker.read_text()) + [int(bootstrap.read_text())]
                os.killpg(worker.pid, signal.SIGKILL)
                worker.wait(timeout=5)
                self.wait_for(lambda: all(self.dead(pid) for pid in pids))
                self.assertIsNone(unrelated.poll())
            finally:
                if worker.poll() is None:
                    os.killpg(worker.pid, signal.SIGKILL); worker.wait()
                unrelated.kill(); unrelated.wait()

    def test_timeout_reclaims_stubborn_tool_tree_without_killing_owner(self):
        with tempfile.TemporaryDirectory() as folder:
            marker = Path(folder) / 'tool.json'
            process = recovery.platform.launch_tool([sys.executable, '-c', self.tool(marker)],
                                                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                self.wait_for(marker.exists)
                with self.assertRaises(subprocess.TimeoutExpired):
                    process.communicate(timeout=.05)
                recovery.terminate_child(process)
                self.wait_for(lambda: all(self.dead(pid) for pid in json.loads(marker.read_text())))
                with patch.object(recovery.os, 'killpg', side_effect=AssertionError('已回收PID不得再用于发信号')):
                    recovery.terminate_child(process)
            finally:
                recovery.terminate_child(process)
                for stream in (process.stdin, process.stdout, process.stderr): stream.close()

    def test_normal_tool_exit_cleans_its_stubborn_descendant(self):
        with tempfile.TemporaryDirectory() as folder:
            marker = Path(folder) / 'child.pid'
            child = 'import signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);time.sleep(60)'
            code = ('import subprocess,sys;from pathlib import Path;'
                    f'p=subprocess.Popen([sys.executable,"-c",{child!r}]);'
                    f'Path({str(marker)!r}).write_text(str(p.pid))')
            process = recovery.platform.launch_tool([sys.executable, '-c', code],
                                                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                process.communicate(timeout=5)
                self.assertEqual(process.returncode, 0)
                self.wait_for(lambda: self.dead(int(marker.read_text())))
            finally:
                recovery.terminate_child(process)
                for stream in (process.stdin, process.stdout, process.stderr): stream.close()

@unittest.skipUnless(os.name == 'nt', '需要原生Windows Job Objects，POSIX不冒充验收')
class WindowsProcesses(unittest.TestCase):
    def test_job_cleanup_after_wrapper_exit_preserves_unrelated_process(self):
        with tempfile.TemporaryDirectory() as folder:
            marker = Path(folder) / 'child.json'
            child = 'import time;time.sleep(60)'
            parent = ('import subprocess,sys,json;from pathlib import Path;'
                      f'p=subprocess.Popen([sys.executable,"-c",{child!r}]);'
                      f'Path({str(marker)!r}).write_text(str(p.pid))')
            unrelated = subprocess.Popen([sys.executable, '-c', child])
            process = recovery.platform.launch_tool([sys.executable, '-c', parent],
                                                     stdin=subprocess.PIPE, stdout=subprocess.DEVNULL)
            try:
                process.communicate(timeout=10)
                self.assertTrue(marker.exists())
                pid = int(marker.read_text())
                self.assertIsNotNone(recovery.platform.process_identity(pid))
                recovery.terminate_child(process)
                end = time.monotonic() + 5
                while recovery.platform.process_identity(pid) is not None and time.monotonic() < end:
                    time.sleep(.05)
                self.assertIsNone(recovery.platform.process_identity(pid))
                self.assertIsNone(unrelated.poll())
            finally:
                recovery.terminate_child(process)
                unrelated.kill(); unrelated.wait()

    def test_failed_job_assignment_never_runs_tool(self):
        with tempfile.TemporaryDirectory() as folder:
            marker = Path(folder) / 'must-not-exist'
            command = [sys.executable, '-c', f'from pathlib import Path;Path({str(marker)!r}).touch()']
            with patch.object(recovery.platform.Job, 'assign', side_effect=OSError('assignment denied')):
                with self.assertRaises(OSError):
                    recovery.platform.launch_tool(command, stdin=subprocess.PIPE)
            self.assertFalse(marker.exists())

    def test_creation_identity_rejects_reused_pid_record(self):
        identity = recovery.platform.process_identity(os.getpid())
        self.assertIsNotNone(identity)
        self.assertFalse(recovery.supervisor_alive({'pid': os.getpid(), 'created': identity + 1}, Path.cwd()))
        self.assertFalse(recovery.supervisor_alive({'pid': os.getpid()}, Path.cwd()))

if __name__ == '__main__':
    unittest.main()
