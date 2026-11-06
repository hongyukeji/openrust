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
ORIGINAL_NOTIFY = recovery.notify


def setUpModule():
    # 所有进程内worker测试默认隔离系统通知；个别回归可显式覆盖此桩。
    global _notifications
    _notifications = patch.object(recovery, 'notify', return_value=False)
    _notifications.start()


def tearDownModule():
    _notifications.stop()


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
        self.assertEqual(self.decide(turn=dict(self.turn, kind='turn_aborted')), 'needs_attention')
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
            self.assertEqual(self.decide(goal=blocked, turn=dict(turn, kind='turn_aborted')), 'needs_attention')
            self.assertEqual(self.decide(goal=blocked, turn=turn, agent=dict(self.agent, PendingPermissions=[1])), 'needs_attention')
        for message in ('401 unauthorized', 'authentication invalid', '403 permission denied', '401 unauthorized; quota exceeded'):
            self.assertEqual(self.decide(goal=blocked, turn=dict(self.turn, payload={'error': {'message': message}})), 'needs_attention')

    def test_ambiguous_abort_keeps_supervisor_and_requires_new_activity(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder)
            rollout = runtime / 'rollout.jsonl'
            config = {'agent': 'a', 'thread': 't', 'rollout': str(rollout), 'goal_id': 'g'}
            (runtime / 'config.json').write_text(json.dumps(config))
            clock = [time.time()]
            rows = [
                {'type': 'event_msg', 'timestamp': '2026-10-31T00:00:00Z',
                 'payload': {'type': 'turn_aborted'}},
            ]

            def write_rows():
                rollout.write_text(''.join(json.dumps(row) + '\n' for row in rows))

            write_rows()
            observations = []

            def waiting(*_):
                observations.append(json.loads((runtime / 'state.json').read_text()))
                if len(observations) == 1:
                    rows.extend([
                        {'type': 'event_msg', 'timestamp': '2026-10-31T00:01:00Z',
                         'payload': {'type': 'task_started'}},
                        {'type': 'response_item', 'timestamp': '2026-10-31T00:01:00.5Z',
                         'payload': {'type': 'message', 'role': 'user',
                             'content': [{'type': 'input_text', 'text': '继续原任务'}],
                             'internal_chat_message_metadata_passthrough': {'content_item_kinds': ['user.text']}}},
                        {'type': 'response_item', 'timestamp': '2026-10-31T00:01:01Z',
                         'payload': {'type': 'function_call', 'name': 'functions.exec'}},
                        {'type': 'event_msg', 'timestamp': '2026-10-31T00:01:02Z',
                         'payload': {'type': 'task_complete', 'error': {'message': 'runner transport failed'}}},
                    ])
                    write_rows()
                elif len(observations) == 2:
                    clock[0] += 180
                else:
                    (runtime / 'disabled').touch()

            def dispatched(_command):
                return {}

            with patch.object(recovery, 'binding_valid', return_value=True), \
                 patch.object(recovery, 'inspect', return_value=self.agent), \
                 patch.object(recovery, 'goal', return_value=self.goal), \
                 patch.object(recovery, 'bound_goal', return_value={'status': 'active'}), \
                 patch.object(recovery, 'notify', return_value=True), \
                 patch.object(recovery, 'rpc', return_value={
                     'ordinaryUsageAllowed': True, 'rateLimits': {'primary': {'usedPercent': 1}}}) as remote, \
                 patch.object(recovery, 'run_json', side_effect=dispatched) as send, \
                 patch.object(recovery.time, 'time', side_effect=lambda: clock[0]), \
                 patch.object(recovery, 'sleep_until', side_effect=waiting):
                recovery.worker(runtime, os.getppid())
            self.assertEqual(observations[0]['status'], 'needs_attention')
            self.assertEqual(observations[0]['reason'], 'turn_aborted_unclassified')
            self.assertIsNotNone(observations[0]['next_check_at'])
            self.assertEqual(observations[1]['status'], 'backoff')
            self.assertEqual(observations[2]['status'], 'sent')
            send.assert_called_once()
            self.assertEqual(remote.call_args.args[0], 'account/rateLimits/read')

    def test_ambiguous_abort_never_dispatches_without_new_turn(self):
        for status in ('active', 'blocked', 'paused'):
            with self.subTest(status=status), tempfile.TemporaryDirectory() as folder:
                runtime = Path(folder)
                (runtime / 'config.json').write_text(json.dumps(
                    {'agent': 'a', 'thread': 't', 'rollout': 'r', 'goal_id': 'g'}))
                observations = []

                def waiting(*_):
                    observations.append(json.loads((runtime / 'state.json').read_text()))
                    (runtime / 'disabled').touch()

                with patch.object(recovery, 'binding_valid', return_value=True), \
                     patch.object(recovery, 'inspect', return_value=self.agent), \
                     patch.object(recovery, 'goal', return_value=dict(self.goal, status=status)), \
                     patch.object(recovery, 'timeline', return_value=dict(self.turn, kind='turn_aborted')), \
                     patch.object(recovery, 'notify', return_value=True), \
                     patch.object(recovery, 'rpc') as remote, \
                     patch.object(recovery, 'run_json') as send, \
                     patch.object(recovery, 'sleep_until', side_effect=waiting):
                    recovery.worker(runtime, os.getppid())
                state = observations[0] if observations else json.loads((runtime / 'state.json').read_text())
                self.assertEqual(state['status'], 'paused' if status == 'paused' else 'needs_attention')
                remote.assert_not_called()
                send.assert_not_called()

    def test_running_and_auth_controls_override_usage_limited_enum(self):
        limited = {'id': 'g', 'status': 'usage_limited'}
        self.assertEqual(self.decide(goal=limited, agent=dict(self.agent, Status='running')), 'working')
        self.assertEqual(self.decide(goal=limited, turn=dict(self.turn, kind='user_message')), 'needs_attention')
        self.assertEqual(self.decide(goal=limited, turn=dict(self.turn, payload={'error': {'message': '401 unauthorized'}})), 'needs_attention')

    def test_steer_activity_requires_a_tool_after_latest_user_input(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            rows = [
                {'type': 'event_msg', 'timestamp': 'start', 'payload': {'type': 'task_started'}},
                {'type': 'response_item', 'timestamp': 'before', 'payload': {'type': 'function_call', 'name': 'exec'}},
                {'type': 'response_item', 'timestamp': 'steer', 'payload': {'type': 'message', 'role': 'user',
                    'content': [{'type': 'input_text', 'text': '继续并行处理'}]}},
            ]
            path.write_text('\n'.join(json.dumps(r) for r in rows))
            self.assertFalse(recovery.timeline(path)['post_user_activity'])
            rows.append({'type': 'response_item', 'timestamp': 'after',
                         'payload': {'type': 'function_call', 'name': 'exec'}})
            path.write_text('\n'.join(json.dumps(r) for r in rows))
            turn = recovery.timeline(path)
            self.assertTrue(turn['post_user_activity'])
            self.assertEqual(turn['kind'], 'user_message')
            self.assertEqual(self.decide(turn=turn), 'needs_attention')  # 不授权普通恢复派发。
            rows.append({'type': 'event_msg', 'timestamp': 'new-user',
                         'payload': {'type': 'user_message', 'message': '先暂停'}})
            path.write_text('\n'.join(json.dumps(r) for r in rows))
            self.assertFalse(recovery.timeline(path)['post_user_activity'])

    def test_goal_continuation_keeps_verified_user_context_and_tool_activity(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            rows = [
                {'type': 'event_msg', 'timestamp': 'old', 'payload': {'type': 'task_started', 'pad': 'x' * 1000}},
                {'type': 'response_item', 'timestamp': 'human', 'payload': {'type': 'message', 'role': 'user',
                    'content': [{'type': 'input_text', 'text': '继续已授权任务'}],
                    'internal_chat_message_metadata_passthrough': {'content_item_kinds': ['user.text']}}},
                {'type': 'response_item', 'timestamp': 'tool', 'payload': {'type': 'function_call', 'name': 'exec'}},
            ]
            path.write_text('\n'.join(json.dumps(row) for row in rows) + '\n')
            with patch.object(recovery, 'TAIL_BYTES', 800):
                initial = recovery.timeline(path)
            rows.append({'type': 'response_item', 'timestamp': 'goal', 'payload': {'type': 'message', 'role': 'user',
                'content': [{'type': 'input_text', 'text': '<codex_internal_context source="goal">继续</codex_internal_context>'}],
                'internal_chat_message_metadata_passthrough': {'content_item_kinds': ['goal.internal_context']}}})
            rows.append({'type': 'event_msg', 'timestamp': 'next', 'payload': {'type': 'task_started'}})
            path.write_text('\n'.join(json.dumps(row) for row in rows) + '\n')
            with patch.object(recovery, 'TAIL_BYTES', 800):
                continued = recovery.timeline(path)
            self.assertEqual(continued['user_context'], initial['user_context'])
            self.assertTrue(continued['post_user_activity'])
            self.assertEqual(continued['input_provenance'], 'verified_user')

    def test_goal_tag_in_real_user_text_and_unknown_metadata_are_not_trusted(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            base = {'type': 'response_item', 'timestamp': 'human', 'payload': {'type': 'message', 'role': 'user',
                'content': [{'type': 'input_text', 'text': '继续'}],
                'internal_chat_message_metadata_passthrough': {'content_item_kinds': ['user.text']}}}
            path.write_text(json.dumps(base) + '\n')
            first = recovery.timeline(path)
            for kinds in (['user.text'], ['goal.internal_context', 'user.text'], None):
                with self.subTest(kinds=kinds):
                    payload = {'type': 'message', 'role': 'user',
                        'content': [{'type': 'input_text', 'text': '<codex_internal_context source="goal">先暂停</codex_internal_context>'}]}
                    if kinds is not None:
                        payload['internal_chat_message_metadata_passthrough'] = {'content_item_kinds': kinds}
                    path.write_text(json.dumps(base) + '\n' + json.dumps({'type': 'response_item',
                        'timestamp': str(kinds), 'payload': payload}) + '\n')
                    turn = recovery.timeline(path)
                    if turn is None:
                        turn = recovery.timeline(path)  # 同inode重写先丢弃旧快照，下一轮重建。
                    self.assertNotEqual(turn['user_context'], first['user_context'])
                    self.assertFalse(turn['post_user_activity'])
                    self.assertEqual(turn['input_provenance'], 'verified_user' if kinds == ['user.text'] else 'unknown')

    def test_tail_gap_cannot_inherit_verified_user_context(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            path.write_text(json.dumps({'type': 'response_item', 'timestamp': 'human', 'payload': {
                'type': 'message', 'role': 'user', 'content': [{'type': 'input_text', 'text': '继续'}],
                'internal_chat_message_metadata_passthrough': {'content_item_kinds': ['user.text']}}}) + '\n')
            self.assertEqual(recovery.timeline(path)['input_provenance'], 'verified_user')
            with patch.object(recovery, 'TAIL_BYTES', 128):
                with path.open('a') as output:
                    for n in range(10):
                        output.write(json.dumps({'type': 'event_msg', 'timestamp': str(n),
                            'payload': {'type': 'task_started', 'pad': 'x' * 128}}) + '\n')
                    output.write(json.dumps({'type': 'event_msg', 'timestamp': 'end',
                        'payload': {'type': 'task_complete'}}) + '\n')
                turn = recovery.timeline(path)
            self.assertEqual(turn['input_provenance'], 'unknown')
            self.assertIsNone(turn['user_context'])

    def test_abort_invalidates_context_even_after_internal_goal_turn(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            rows = [
                {'type': 'response_item', 'timestamp': 'human', 'payload': {'type': 'message', 'role': 'user',
                    'content': [{'type': 'input_text', 'text': '继续'}],
                    'internal_chat_message_metadata_passthrough': {'content_item_kinds': ['user.text']}}},
                {'type': 'response_item', 'timestamp': 'tool', 'payload': {'type': 'function_call', 'name': 'exec'}},
                {'type': 'event_msg', 'timestamp': 'abort', 'payload': {'type': 'turn_aborted'}},
                {'type': 'response_item', 'timestamp': 'goal', 'payload': {'type': 'message', 'role': 'user',
                    'content': [{'type': 'input_text', 'text': '继续'}],
                    'internal_chat_message_metadata_passthrough': {'content_item_kinds': ['goal.internal_context']}}},
                {'type': 'event_msg', 'timestamp': 'next', 'payload': {'type': 'task_started'}},
            ]
            path.write_text('\n'.join(json.dumps(row) for row in rows) + '\n')
            turn = recovery.timeline(path)
            self.assertEqual(turn['input_provenance'], 'unknown')
            self.assertFalse(turn['post_user_activity'])
            self.assertIsNone(turn['user_context'])
            self.assertTrue(turn['abort_barrier'])
            self.assertEqual(self.decide(turn=turn), 'needs_attention')
            with path.open('a') as output:
                output.write(json.dumps({'type': 'event_msg', 'timestamp': 'complete',
                    'payload': {'type': 'task_complete', 'completed_at': 0}}) + '\n')
            runtime = Path(folder) / 'runtime'
            runtime.mkdir()
            (runtime / 'config.json').write_text(json.dumps({'agent': 'a', 'thread': 't',
                'rollout': str(path), 'goal_id': 'g'}))
            def finish_check(*_):
                (runtime / 'disabled').touch()
            with patch.object(recovery, 'binding_valid', return_value=True), \
                 patch.object(recovery, 'inspect', return_value=self.agent), \
                 patch.object(recovery, 'goal', return_value=self.goal), \
                 patch.object(recovery, 'notify', return_value=True), \
                 patch.object(recovery, 'rpc') as remote, \
                 patch.object(recovery, 'run_json') as send, \
                 patch.object(recovery, 'sleep_until', side_effect=finish_check):
                recovery.worker(runtime, os.getppid())
            send.assert_not_called()
            remote.assert_not_called()

    def test_same_inode_prefix_rewrite_does_not_reuse_old_user_authorization(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            def human(message):
                return {'type': 'response_item', 'timestamp': 'human', 'payload': {'type': 'message', 'role': 'user',
                    'content': [{'type': 'input_text', 'text': message}],
                    'internal_chat_message_metadata_passthrough': {'content_item_kinds': ['user.text']}}}
            suffix = [{'type': 'response_item', 'timestamp': 'tool', 'payload': {'type': 'function_call', 'name': 'exec'}},
                      {'type': 'event_msg', 'timestamp': 'end', 'payload': {'type': 'task_complete', 'pad': 'x' * 200}}]
            path.write_text('\n'.join(json.dumps(row) for row in [human('继续'), *suffix]) + '\n')
            before = recovery.timeline(path)
            path.write_text('\n'.join(json.dumps(row) for row in [human('暂停'), *suffix]) + '\n')
            with path.open('a') as output:
                output.write(json.dumps({'type': 'event_msg', 'timestamp': 'next',
                    'payload': {'type': 'task_started'}}) + '\n')
            self.assertIsNone(recovery.timeline(path))
            after = recovery.timeline(path)
            self.assertNotEqual(after['user_context'], before['user_context'])
            self.assertEqual(after['user_context']['message'], '暂停')

    def test_rewrite_during_parse_cannot_pair_old_events_with_new_digest(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            rows = [
                {'type': 'response_item', 'timestamp': 'human', 'payload': {'type': 'message', 'role': 'user',
                    'content': [{'type': 'input_text', 'text': '继续'}],
                    'internal_chat_message_metadata_passthrough': {'content_item_kinds': ['user.text']}}},
                {'type': 'response_item', 'timestamp': 'tool', 'payload': {'type': 'function_call', 'name': 'exec'}},
                {'type': 'event_msg', 'timestamp': 'end', 'payload': {'type': 'task_complete', 'pad': 'x' * 200}},
            ]
            path.write_text('\n'.join(json.dumps(row) for row in rows) + '\n')
            recovery.timeline(path)
            with path.open('a') as output:
                output.write(json.dumps({'type': 'response_item', 'timestamp': 'next-tool',
                    'payload': {'type': 'function_call', 'name': 'exec'}}) + '\n')
            original = recovery.explicit_goal_status
            def rewrite_during_parse(payload):
                path.write_text(path.read_text().replace(json.dumps('继续')[1:-1],
                                                         json.dumps('暂停')[1:-1], 1))
                return original(payload)
            with patch.object(recovery, 'explicit_goal_status', side_effect=rewrite_during_parse):
                self.assertIsNone(recovery.timeline(path))
            self.assertEqual(recovery.timeline(path)['user_context']['message'], '暂停')

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
            append('response_item', {'type': 'message', 'role': 'user',
                                    'content': [{'type': 'input_text', 'text': '继续原任务'}],
                                    'internal_chat_message_metadata_passthrough': {'content_item_kinds': ['user.text']}}, 'human-resume')
            append('response_item', {'type': 'function_call', 'name': 'functions.exec'}, 'human-tool')
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

    def test_bound_goal_allows_only_timestamp_drift_and_rejects_semantic_changes(self):
        config = {'goal_id': 'g', 'thread': 't'}
        local = dict(self.goal, created_at_ms=123456, objective='原始目标', updated_at_ms=1)
        remote = {'threadId': 't', 'createdAt': 123, 'objective': '原始目标', 'status': local['status']}
        for field, value in [('updated_at_ms', 2), ('id', 'replacement'),
                             ('status', 'paused'), ('objective', '其他目标'), ('created_at_ms', 999999)]:
            with self.subTest(field=field), patch.object(recovery, 'binding_valid', return_value=True), \
                 patch.object(recovery, 'goal', side_effect=[local, dict(local, **{field: value})]), \
                 patch.object(recovery, 'rpc', return_value={'goal': remote}):
                if field == 'updated_at_ms':
                    self.assertEqual(recovery.bound_goal(config), remote)
                else:
                    with self.assertRaises(ValueError): recovery.bound_goal(config)
        for response in [None, dict(remote, status='paused'), dict(remote, threadId='other'),
                         dict(remote, objective='其他目标'), dict(remote, createdAt=124)]:
            with self.subTest(response=response), patch.object(recovery, 'binding_valid', return_value=True), \
                 patch.object(recovery, 'goal', return_value=local), \
                 patch.object(recovery, 'rpc', return_value={'goal': response}):
                with self.assertRaises(ValueError): recovery.bound_goal(config)

    def test_bound_goal_accepts_protocol_limited_status_spelling_only(self):
        config = {'goal_id': 'g', 'thread': 't'}
        for local_status, remote_status in [('usage_limited', 'usageLimited'), ('budget_limited', 'budgetLimited')]:
            local = dict(self.goal, status=local_status, created_at_ms=123456, objective='原始目标')
            remote = {'threadId': 't', 'createdAt': 123, 'objective': '原始目标', 'status': remote_status}
            with self.subTest(status=local_status), patch.object(recovery, 'binding_valid', return_value=True), \
                 patch.object(recovery, 'goal', return_value=local), \
                 patch.object(recovery, 'rpc', return_value={'goal': remote}):
                self.assertEqual(recovery.bound_goal(config), remote)
            with patch.object(recovery, 'binding_valid', return_value=True), \
                 patch.object(recovery, 'goal', return_value=local), \
                 patch.object(recovery, 'rpc', return_value={'goal': dict(remote, status='active')}):
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
        shutil.copyfile(SCRIPT.with_name('guard_codex.py'), self.script.with_name('guard_codex.py'))
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
# 通知替身立即结束，不等待Codex协议stdin，避免5秒超时污染监督恢复时序。
if Path(sys.argv[0]).name=='osascript':sys.exit(0)
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
        signalled_owned_supervisor = False
        if self.runtime.exists():
            (self.runtime / 'disabled').touch()
            supervisor_path = self.runtime / 'supervisor.json'
            if supervisor_path.exists():
                supervisor = json.loads(supervisor_path.read_text())
                # SIGKILL回归的监督进程有30秒重启冷却；直接通知本fixture的已验证进程退出。
                with patch.object(recovery, 'SCRIPT', self.script):
                    owned = recovery.supervisor_alive(supervisor, self.runtime)
                if owned:
                    signalled_owned_supervisor = True
                    try:
                        os.kill(supervisor['pid'], signal.SIGTERM)
                    except ProcessLookupError:
                        pass
            self.wait_for(lambda: not (self.runtime / 'supervisor.json').exists(), timeout=45)
        self.temp.cleanup()
        if getattr(self, 'require_owned_signal', False):
            self.assertTrue(signalled_owned_supervisor)

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

    def test_resume_replaced_goal_refuses_before_config_or_grant_write(self):
        self.call('start')
        self.call('stop')
        previous = (self.runtime / 'config.json').read_text()
        with sqlite3.connect(self.home / 'goals_1.sqlite') as db:
            db.execute("UPDATE thread_goals SET goal_id='replacement',status='paused',objective='other'")
        result = self.call('start', '--use-full-budget-once', '--resume-budget-paused', check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.runtime / 'config.json').read_text(), previous)
        self.assertFalse((self.runtime / 'full-budget-once.json').exists())

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

    def test_quota_timeout_reaps_processes_and_plain_restart_preserves_wait(self):
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
        self.wait_for(lambda: json.loads(state_path.read_text()).get('status') == 'quota_wait_expired')
        self.wait_for(lambda: not (self.runtime / 'supervisor.json').exists())
        state = json.loads(state_path.read_text())
        self.assertIn('quota_wait_started_at', state)
        self.assertIn('quota_wait_expired_at', state)
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
        self.require_owned_signal = True

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
                     patch.object(sys, 'argv', ['guard.py', 'start', '--backend', 'paseo', '--agent', agent, '--thread', agent]), \
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
            with patch.object(recovery, 'ROOT', root), patch.object(sys, 'argv', ['guard.py', 'start', '--backend', 'paseo', '--agent', agent, '--thread', agent]), patch.object(recovery.platform, 'command', side_effect=FileNotFoundError('missing')):
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


class FullBudgetOnce(unittest.TestCase):
    """一次性额度授权不得重算历史、重放授权或解除其他来源的暂停。"""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.runtime = Path(self.temp.name)
        self.current = {'id': 'g', 'status': 'paused', 'created_at_ms': 1000,
                        'objective': '原完整目标', 'updated_at_ms': 1500}
        self.response = quota_snapshot(balance='0', used=1)
        self.response['rateLimits']['credits']['hasCredits'] = False
        self.config = {'agent': 'a', 'thread': 't', 'goal_id': 'g', 'rollout': 'r',
                       'usage_budget': recovery.initialize_usage_budget(self.response, 100),
                       'account_budgets': {'previous': {'immutable': True}},
                       'allow_existing_credits': True}
        self.controls = {'last_user_at': '2026-10-31T12:00:00Z', 'user_key': '2026-10-31T12:00:00Z',
                         'control': None, 'turn_key': 't', 'turn_kind': 'task_complete'}
        self.before = json.loads(json.dumps(self.config))
        self.goal_patch = patch.object(recovery, 'goal', side_effect=lambda c: dict(self.current))
        self.bound_patch = patch.object(recovery, 'bound_goal', side_effect=lambda c: dict(
            self.current, createdAt=self.current['created_at_ms'] // 1000))
        self.valid_patch = patch.object(recovery, 'binding_valid', return_value=True)
        self.goal_patch.start(); self.bound_patch.start(); self.valid_patch.start()
        self.addCleanup(self.temp.cleanup)
        self.addCleanup(self.goal_patch.stop); self.addCleanup(self.bound_patch.stop)
        self.addCleanup(self.valid_patch.stop)

    def grant(self, response=None):
        recovery.authorize_full_budget(self.runtime, self.config, dict(self.current), self.controls,
                                       response or self.response)

    def status(self, response):
        return recovery.usage_budget_status(response, recovery.effective_usage_budget(self.config))

    def test_public_cli_help_renders_budget_policy_flags(self):
        result = subprocess.run([sys.executable, '-B', str(SCRIPT), '--help'],
                                capture_output=True, text=True, timeout=5, check=True)
        for option in ('--use-full-budget-once', '--resume-budget-paused', '--end-full-budget-once', '--rebase-usage-budget'):
            self.assertIn(option, result.stdout)
        self.assertIn('80%', result.stdout)

    def test_twenty_percent_reserve_is_overridden_without_history_change(self):
        self.grant()
        low = json.loads(json.dumps(self.response)); low['rateLimits']['primary']['usedPercent'] = 90
        self.assertEqual(recovery.usage_budget_status(low, self.config['usage_budget']), 'usage_budget_reached')
        self.assertEqual(self.status(low), 'usage_budget_available')
        self.assertEqual(self.config['usage_budget'], self.before['usage_budget'])
        self.assertEqual(self.config['account_budgets'], self.before['account_budgets'])
        self.assertEqual(self.config['usage_budget']['fraction'], '0.8')

    def test_actual_zero_consumes_authorization_and_cannot_replay(self):
        self.grant()
        zero = json.loads(json.dumps(self.response)); zero['ordinaryUsageAllowed'] = False
        zero['rateLimits']['primary']['usedPercent'] = 100
        self.assertEqual(self.status(zero), 'usage_budget_reached')
        path, document = recovery.grant_document(self.config)
        self.assertIsNone(document['active'])
        self.assertEqual(next(iter(document['grants'].values()))['reason'], 'actual_quota_exhausted')
        with self.assertRaisesRegex(ValueError, '不能重复使用'):
            self.grant()
        self.assertFalse(recovery.effective_usage_budget(self.config).get('_full_budget_once'))

    def test_reload_preserves_active_grant_and_revoke_restores_default(self):
        self.grant()
        reloaded = json.loads(json.dumps(self.config))
        self.assertIn('_full_budget_once', recovery.effective_usage_budget(reloaded))
        self.grant()  # 同一仍有效授权幂等，不能重建基准。
        recovery.end_full_budget(reloaded, 'explicit_revoke')
        self.assertEqual(recovery.effective_usage_budget(self.config), self.before['usage_budget'])
        with self.assertRaises(ValueError): self.grant()

    def test_existing_credits_can_continue_at_zero_ordinary_then_end_at_zero_credits(self):
        paid = json.loads(json.dumps(self.response))
        paid['rateLimits']['credits'].update(hasCredits=True, balance='7')
        self.grant(paid)
        paid['rateLimits']['primary']['usedPercent'] = 100
        paid['ordinaryUsageAllowed'] = False
        self.assertEqual(self.status(paid), 'usage_budget_available')
        self.config['allow_existing_credits'] = False
        self.assertFalse(recovery.effective_usage_budget(self.config)['_full_budget_once']['allow_existing_credits'])
        self.config['allow_existing_credits'] = True
        paid['rateLimits']['credits'].update(hasCredits=False, balance='0')
        self.assertEqual(self.status(paid), 'usage_budget_reached')
        self.assertEqual(self.config['usage_budget']['credits'], self.before['usage_budget']['credits'])

    def test_credits_require_separate_authorization_and_unknown_hard_limit_waits(self):
        self.config['allow_existing_credits'] = False
        paid = json.loads(json.dumps(self.response)); paid['rateLimits']['credits'].update(hasCredits=True, balance='7')
        self.grant(paid)
        paid['rateLimits']['primary']['usedPercent'] = 100; paid['ordinaryUsageAllowed'] = False
        self.assertEqual(self.status(paid), 'usage_budget_reached')
        # 一个新的独立授权快照；未知状态与消费硬限均不允许继续。
        self.controls['user_key'] = '2026-10-31T12:01:00Z'
        self.grant()
        for change in ('unknown', 'hard'):
            snapshot = json.loads(json.dumps(self.response))
            if change == 'unknown': snapshot.pop('ordinaryUsageAllowed')
            else: snapshot['rateLimits']['spendControlReached'] = True
            self.assertEqual(self.status(snapshot), 'usage_budget_unknown')

    def test_changed_identity_or_goal_retires_grant_without_mutating_history(self):
        for kind in ('account', 'goal'):
            with self.subTest(kind=kind):
                self.controls['user_key'] = kind
                self.grant()
                response = json.loads(json.dumps(self.response))
                if kind == 'account': response['accountId'] = 'other'
                else: self.current['objective'] = '被替换'
                self.assertEqual(self.status(response), 'usage_budget_unknown')
                self.assertIsNone(recovery.grant_document(self.config)[1]['active'])
                self.current['objective'] = '原完整目标'
        self.assertEqual(self.config['usage_budget'], self.before['usage_budget'])

    def rebase(self, response=None, state=None):
        with patch.object(recovery, 'percentage_pause_controls', return_value=self.controls), \
             patch.object(recovery, 'rpc', return_value=response or self.response):
            return recovery.prepare_usage_budget_rebase(
                self.runtime, self.config, self.before, state or self.state(), resume=True)

    def test_rebase_early_reset_and_same_window_topup_require_explicit_entry(self):
        for kind in ('early_reset', 'topup'):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                self.runtime = Path(directory)
                response = json.loads(json.dumps(self.response))
                response['rateLimits']['primary']['usedPercent'] = 0
                if kind == 'early_reset': response['rateLimits']['primary']['resetsAt'] = 3000
                unchanged = json.loads(json.dumps(self.config))
                recovery.refresh_usage_cycles(self.runtime, self.config, response, 1000)
                self.assertEqual(self.config, unchanged)
                candidate, authorization = self.rebase(response)
                self.assertEqual(candidate['usage_budget']['fraction'], '0.8')
                self.assertEqual(candidate['usage_budget']['windows']['primary']['reserve'], '20.0')
                self.assertEqual(candidate['account_budgets'], self.before['account_budgets'])
                self.assertEqual(self.config, unchanged)
                audit = json.loads(Path(candidate['usage_budget_rebases'][-1]['audit_file']).read_text())
                self.assertEqual(audit['previous_config'], self.before)
                self.assertEqual(audit['previous_state']['questions'], {'q': {'status': 'unknown'}})
                self.assertEqual(audit['authorization'], authorization)
                self.assertFalse((self.runtime / 'config.json').exists())

    def test_rebase_rejects_inconsistent_unknown_identity_and_hard_limit_without_writes(self):
        for kind in ('changed', 'unknown', 'identity', 'hard', 'permission', 'paused', 'duration'):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                runtime = Path(directory)
                response = json.loads(json.dumps(self.response))
                second = json.loads(json.dumps(response))
                if kind == 'changed': second['rateLimits']['primary']['usedPercent'] = 0
                if kind == 'unknown': response.pop('ordinaryUsageAllowed')
                if kind == 'identity': response['accountId'] = 'other'
                if kind == 'hard': response['rateLimits']['spendControlReached'] = True
                if kind == 'duration': response['rateLimits']['primary']['windowDurationMins'] = 60
                controls = None if kind == 'permission' else self.controls
                with patch.object(recovery, 'percentage_pause_controls', return_value=controls), \
                     patch.object(recovery, 'rpc', side_effect=[response, second]):
                    with self.assertRaises(ValueError):
                        recovery.prepare_usage_budget_rebase(runtime, self.config, self.before, self.state(),
                                                            resume=kind != 'paused')
                self.assertEqual(list(runtime.iterdir()), [])
                self.assertEqual(self.config, self.before)

    def test_rebase_resume_default_eighty_percent_and_not_full_budget(self):
        state = self.state(); state['quota_wait_started_at'] = 1
        candidate, authorization = self.rebase(state=state)
        def rpc(method, params):
            if method == 'thread/goal/set': self.current['status'] = params['status']; return {}
            return self.response
        with patch.object(recovery, 'percentage_pause_controls', return_value=self.controls), \
             patch.object(recovery, 'rpc', side_effect=rpc), patch.object(recovery, 'timeline', return_value={}):
            recovery.resume_budget_pause(self.runtime, candidate, state, rebase_authorization=authorization)
        self.assertEqual(self.current['status'], 'active')
        self.assertNotIn('quota_wait_started_at', state)
        self.assertEqual(state['questions'], {'q': {'status': 'unknown'}})
        self.assertEqual(candidate['usage_budget']['fraction'], '0.8')
        self.assertFalse((self.runtime / 'full-budget-once.json').exists())
        low = json.loads(json.dumps(self.response)); low['rateLimits']['primary']['usedPercent'] = 90
        self.assertEqual(recovery.usage_budget_status(low, candidate['usage_budget']), 'usage_budget_reached')

    def test_rebase_depletion_before_resume_does_not_activate_or_commit(self):
        candidate, authorization = self.rebase()
        low = json.loads(json.dumps(self.response)); low['rateLimits']['primary']['usedPercent'] = 99
        state = self.state()
        with patch.object(recovery, 'percentage_pause_controls', return_value=self.controls), \
             patch.object(recovery, 'rpc', return_value=low) as calls:
            with self.assertRaises(ValueError):
                recovery.resume_budget_pause(self.runtime, candidate, state, rebase_authorization=authorization)
        self.assertEqual(self.current['status'], 'paused')
        self.assertFalse((self.runtime / 'config.json').exists())
        self.assertFalse(any(call.args[0] == 'thread/goal/set' for call in calls.call_args_list))

    def test_verified_active_and_available_recovery_alone_clears_old_quota_wait(self):
        for kind in ('available', 'depleted', 'paused', 'permission', 'unknown', 'identity', 'unknown_resume'):
            self.current['status'] = 'paused' if kind == 'paused' else 'active'
            state = {'quota_wait_started_at': 1, 'quota_wait_expired_at': 3601,
                     'questions': {'q': {'status': 'unknown'}}, 'attempts': 7}
            if kind == 'unknown_resume': state['percentage_pause'] = {'resume_status': 'sending'}
            response = json.loads(json.dumps(self.response))
            if kind == 'depleted': response['rateLimits']['primary']['usedPercent'] = 99
            if kind == 'unknown': response.pop('ordinaryUsageAllowed')
            if kind == 'identity': response['accountId'] = 'other'
            with self.subTest(kind=kind), \
                 patch.object(recovery, 'percentage_pause_controls', return_value=None if kind == 'permission' else self.controls), \
                 patch.object(recovery, 'rpc', return_value=response):
                self.assertEqual(recovery.clear_verified_quota_wait(state, self.config), kind == 'available')
            self.assertEqual('quota_wait_started_at' in state, kind != 'available')
            self.assertEqual('quota_wait_expired_at' in state, kind != 'available')
            self.assertEqual(state['questions'], {'q': {'status': 'unknown'}})
            self.assertEqual(state['attempts'], 7)

    def state(self):
        return {'usage_budget_paused': True, 'questions': {'q': {'status': 'unknown'}},
                'dispatch_key': 'uncertain', 'attempts': 2,
                'percentage_pause': {'goal': dict(self.current), 'resume_status': 'waiting',
                                     'controls': dict(self.controls, last_user_at='2026-10-31T11:00:00Z',
                                                      user_key='2026-10-31T11:00:00Z')}}

    def test_explicit_resume_uses_original_goal_protocol_and_keeps_idempotency(self):
        self.grant()
        self.response['rateLimits']['primary']['usedPercent'] = 90
        state = self.state(); calls = []
        def rpc(method, params):
            calls.append((method, params))
            if method == 'thread/goal/set': self.current['status'] = params['status']; return {}
            return self.response
        with patch.object(recovery, 'percentage_pause_controls', return_value=self.controls), \
             patch.object(recovery, 'rpc', side_effect=rpc), \
             patch.object(recovery, 'timeline', return_value={'goal_control': {'status': 'blocked', 'key': 'old'}}):
            recovery.resume_budget_pause(self.runtime, self.config, state)
        self.assertEqual(self.current['id'], 'g')
        self.assertEqual(self.current['status'], 'active')
        self.assertEqual(state['percentage_pause']['resume_status'], 'explicitly_resumed')
        self.assertEqual(state['questions'], {'q': {'status': 'unknown'}})
        self.assertEqual(state['dispatch_key'], 'uncertain')
        self.assertEqual(state['attempts'], 2)
        self.assertEqual([p for m, p in calls if m == 'thread/goal/set'], [{'threadId': 't', 'status': 'active'}])
        with self.assertRaises(ValueError): recovery.resume_budget_pause(self.runtime, self.config, state)

    def monitor_full_budget(self, response, *, controls_changed=False, now=1000, windows=None):
        state = self.state()
        state['percentage_pause'].update(controls=dict(self.controls), windows={'primary': '2000'} if windows is None else windows,
                                         next_quota_at=0, account_id='account', account_recovery=True)
        calls = []
        def remote(method, params):
            calls.append(method)
            if method == 'thread/goal/set':
                self.current['status'] = params['status']
                return {}
            return response
        controls = dict(self.controls, control='pause') if controls_changed else self.controls
        with patch.object(recovery, 'percentage_pause_controls', return_value=controls), \
             patch.object(recovery, 'rpc', side_effect=remote), \
             patch.object(recovery, 'run_json') as send, \
             patch.object(recovery, 'login_revision', return_value=None):
            outcome = recovery.monitor_percentage_pause(self.runtime, state, self.config,
                                                         dict(self.current), now)
        return outcome, state, calls, send.call_count

    def test_exhausted_grant_waits_under_default_budget_then_resumes_after_verified_cycle(self):
        self.grant()
        exhausted = json.loads(json.dumps(self.response))
        exhausted['ordinaryUsageAllowed'] = False
        exhausted['rateLimits']['primary']['usedPercent'] = 100
        self.assertEqual(self.status(exhausted), 'usage_budget_reached')
        grant_path, ended = recovery.grant_document(self.config)
        grant_bytes = grant_path.read_bytes()
        self.assertEqual(next(iter(ended['grants'].values()))['reason'], 'actual_quota_exhausted')
        for kind in ('exhausted', 'reserve', 'unknown', 'manual'):
            snapshot = json.loads(json.dumps(exhausted))
            if kind == 'reserve':
                snapshot.update(ordinaryUsageAllowed=True)
                snapshot['rateLimits']['primary']['usedPercent'] = 90
            elif kind == 'unknown':
                snapshot.pop('ordinaryUsageAllowed')
            elif kind == 'manual':
                snapshot.update(ordinaryUsageAllowed=True)
                snapshot['rateLimits']['primary'].update(usedPercent=75, resetsAt=4000)
            outcome, _, calls, sends = self.monitor_full_budget(
                snapshot, controls_changed=kind == 'manual', now=2500)
            self.assertNotEqual(outcome, 'percentage_resumed', kind)
            self.assertNotIn('thread/goal/set', calls, kind)
            self.assertEqual(sends, 0, kind)
        available = json.loads(json.dumps(self.response))
        available['rateLimits']['primary'].update(usedPercent=75, resetsAt=4000)
        outcome, state, calls, sends = self.monitor_full_budget(available, now=2500)
        self.assertEqual(outcome, 'percentage_resumed')
        self.assertEqual(state['percentage_pause']['resume_status'], 'sent')
        self.assertEqual(calls.count('thread/goal/set'), 1)
        self.assertEqual(sends, 1)
        self.assertEqual(grant_path.read_bytes(), grant_bytes)
        self.assertFalse(recovery.effective_usage_budget(self.config).get('_full_budget_once'))

    def test_old_account_ended_grant_does_not_block_owned_default_budget_recovery(self):
        self.grant()
        recovery.end_full_budget(self.config, 'actual_quota_exhausted')
        path, document = recovery.grant_document(self.config)
        grant = next(iter(document['grants'].values()))
        grant['account_id'] = 'old-account'
        recovery.atomic(path, document)
        frozen = path.read_bytes()
        outcome, state, calls, sends = self.monitor_full_budget(self.response, windows={})
        self.assertEqual(outcome, 'percentage_resumed')
        self.assertEqual(calls.count('thread/goal/set'), 1)
        self.assertEqual(sends, 1)
        self.assertEqual(path.read_bytes(), frozen)
        self.assertNotIn('_full_budget_once', recovery.effective_usage_budget(self.config))
        self.assertEqual(state['questions'], {'q': {'status': 'unknown'}})

    def test_old_account_identity_ended_grant_allows_only_default_budget(self):
        self.grant()
        recovery.end_full_budget(self.config, 'account_identity_changed')
        path, document = recovery.grant_document(self.config)
        grant = next(iter(document['grants'].values()))
        grant['account_id'] = 'old-account'
        recovery.atomic(path, document)
        frozen = path.read_bytes()
        baseline = json.loads(json.dumps(self.config))
        low = json.loads(json.dumps(self.response))
        low['rateLimits']['primary']['usedPercent'] = 90
        outcome, _, calls, sends = self.monitor_full_budget(low, windows={})
        self.assertNotEqual(outcome, 'percentage_resumed')
        self.assertNotIn('thread/goal/set', calls)
        self.assertEqual(sends, 0)
        outcome, _, calls, sends = self.monitor_full_budget(self.response, windows={})
        self.assertEqual(outcome, 'percentage_resumed')
        self.assertEqual(calls.count('thread/goal/set'), 1)
        self.assertEqual(sends, 1)
        self.assertEqual(self.config, baseline)
        self.assertEqual(path.read_bytes(), frozen)

    def test_recovered_old_account_grant_worker_keeps_default_floor_monitoring(self):
        """恢复成功后的主循环必须继续检查80%预算，并在随后触线时暂停。"""
        self.grant()
        recovery.end_full_budget(self.config, 'account_identity_changed')
        path, document = recovery.grant_document(self.config)
        next(iter(document['grants'].values()))['account_id'] = 'old-account'
        recovery.atomic(path, document)
        frozen = path.read_bytes()
        baseline = json.loads(json.dumps(self.config))
        state = self.state()
        state['percentage_pause'].update(controls=dict(self.controls), windows={},
                                         next_quota_at=0, account_id='account', account_recovery=True)
        recovery.atomic(self.runtime / 'config.json', self.config)
        recovery.atomic(self.runtime / 'state.json', state)
        observed = []
        low = json.loads(json.dumps(self.response))
        low['rateLimits']['primary']['usedPercent'] = 90
        def remote(method, params):
            if method == 'thread/goal/set':
                self.current['status'] = params['status']
                return {}
            self.assertEqual(method, 'account/rateLimits/read')
            return low if len(observed) >= 2 else self.response
        def next_cycle(*_):
            observed.append(json.loads((self.runtime / 'state.json').read_text()))
            if len(observed) >= 3:
                (self.runtime / 'disabled').touch()
        with patch.object(recovery, 'rpc', side_effect=remote) as calls, \
             patch.object(recovery, 'percentage_pause_controls', return_value=self.controls), \
             patch.object(recovery, 'login_revision', return_value=None), \
             patch.object(recovery, 'run_json') as send, \
             patch.object(recovery, 'inspect', return_value={'Status': 'running'}), \
             patch.object(recovery, 'timeline', return_value={'key': 't'}), \
             patch.object(recovery, 'publish_registry'), \
             patch.object(recovery, 'pause_at_usage_budget', return_value=True) as pause, \
             patch.object(recovery, 'owned_percentage_pause', return_value=False), \
             patch.object(recovery, 'sleep_until', side_effect=next_cycle):
            recovery.worker(self.runtime, os.getppid())
        final = json.loads((self.runtime / 'state.json').read_text())
        self.assertEqual(observed[0]['reason'], 'percentage_resumed')
        self.assertEqual(observed[1]['status'], 'working')
        self.assertIn('usage_budget_checked_at', observed[1])
        self.assertEqual(final['reason'], 'usage_budget_reached')
        self.assertEqual(final['status'], 'paused')
        pause.assert_called_once()
        self.assertEqual(sum(c.args[0] == 'thread/goal/set' for c in calls.call_args_list), 1)
        self.assertEqual(send.call_count, 1)
        self.assertEqual(self.config, baseline)
        self.assertEqual(path.read_bytes(), frozen)
        self.assertEqual(final['questions'], state['questions'])
        persisted = json.loads((self.runtime / 'config.json').read_text())
        self.assertEqual(persisted['usage_budget'], baseline['usage_budget'])
        self.assertEqual(persisted['account_budgets'], baseline['account_budgets'])

    def test_worker_ended_identity_grant_keeps_ambiguous_or_same_identity_blocked(self):
        """主循环不能借旧授权放行同账号、未知身份、目标变化或人工暂停。"""
        self.grant()
        recovery.end_full_budget(self.config, 'account_identity_changed')
        path, original = recovery.grant_document(self.config)
        for kind in ('same_account', 'unknown_account', 'unknown_limit', 'different_goal',
                     'task_changed', 'goal_changed', 'missing_goal', 'manual'):
            with self.subTest(kind=kind):
                (self.runtime / 'disabled').unlink(missing_ok=True)
                (self.runtime / 'state.json').unlink(missing_ok=True)
                document = json.loads(json.dumps(original))
                grant = next(iter(document['grants'].values()))
                grant['account_id'] = 'old-account'
                if kind == 'same_account': grant['account_id'] = self.config['usage_budget']['account_id']
                if kind == 'unknown_account': grant['account_id'] = None
                if kind == 'unknown_limit': grant['limit_id'] = None
                if kind == 'different_goal': grant['goal']['objective'] = 'other'
                if kind == 'task_changed': grant['reason'] = 'task_identity_changed'
                if kind == 'goal_changed': grant['reason'] = 'goal_identity_changed'
                recovery.atomic(path, document)
                frozen = path.read_bytes()
                self.current['status'] = 'paused' if kind == 'manual' else 'active'
                recovery.atomic(self.runtime / 'config.json', self.config)
                observed = []
                def finish_check(*_):
                    observed.append(json.loads((self.runtime / 'state.json').read_text()))
                    (self.runtime / 'disabled').touch()
                with patch.object(recovery, 'goal', return_value=None if kind == 'missing_goal' else dict(self.current)), \
                     patch.object(recovery, 'rpc', return_value=self.response) as calls, \
                     patch.object(recovery, 'run_json') as send, \
                     patch.object(recovery, 'publish_registry'), \
                     patch.object(recovery, 'sleep_until', side_effect=finish_check):
                    recovery.worker(self.runtime, os.getppid())
                final = observed[0] if observed else json.loads((self.runtime / 'state.json').read_text())
                self.assertEqual(final['reason'], 'paused' if kind == 'manual' else 'full_budget_identity_changed')
                self.assertNotIn('usage_budget_checked_at', final)
                self.assertFalse(any(c.args[0] == 'thread/goal/set' for c in calls.call_args_list))
                send.assert_not_called()
                self.assertEqual(path.read_bytes(), frozen)

    def test_ended_grant_unknown_identity_or_goal_never_bypasses_ownership(self):
        self.grant()
        recovery.end_full_budget(self.config, 'actual_quota_exhausted')
        path, original = recovery.grant_document(self.config)
        for kind in ('unknown_account', 'unknown_limit', 'different_goal', 'manual'):
            with self.subTest(kind=kind):
                document = json.loads(json.dumps(original))
                grant = next(iter(document['grants'].values()))
                grant['account_id'] = 'old-account'
                if kind == 'unknown_account': grant['account_id'] = None
                if kind == 'unknown_limit': grant['limit_id'] = None
                if kind == 'different_goal': grant['goal']['objective'] = 'other'
                recovery.atomic(path, document)
                outcome, _, calls, sends = self.monitor_full_budget(
                    self.response, controls_changed=kind == 'manual', windows={})
                self.assertNotEqual(outcome, 'percentage_resumed')
                self.assertNotIn('thread/goal/set', calls)
                self.assertEqual(sends, 0)

    def test_exhaustion_worker_keeps_guard_owned_wait_without_send(self):
        self.grant()
        self.current['status'] = 'active'
        exhausted = json.loads(json.dumps(self.response))
        exhausted['ordinaryUsageAllowed'] = False
        exhausted['rateLimits']['primary']['usedPercent'] = 100
        (self.runtime / 'config.json').write_text(json.dumps(self.config))
        observed = []
        def remote(method, params):
            if method == 'thread/goal/set':
                self.current['status'] = params['status']
                return {}
            return exhausted
        def finish_wait(*_):
            observed.append(json.loads((self.runtime / 'state.json').read_text()))
            (self.runtime / 'disabled').touch()
        with patch.object(recovery, 'rpc', side_effect=remote), \
             patch.object(recovery, 'percentage_pause_controls', return_value=self.controls), \
             patch.object(recovery, 'binding_valid', return_value=True), \
             patch.object(recovery, 'run_json') as send, \
             patch.object(recovery, 'publish_registry'), \
             patch.object(recovery, 'notify', return_value=False), \
             patch.object(recovery, 'sleep_until', side_effect=finish_wait):
            recovery.worker(self.runtime, os.getppid())
        state = observed[0]
        self.assertEqual(state['status'], 'quota_wait')
        self.assertEqual(state['percentage_pause']['resume_status'], 'waiting')
        self.assertEqual(state['percentage_pause']['windows'], {'primary': '2000'})
        self.assertEqual(recovery.full_budget_end_reason(self.config), 'actual_quota_exhausted')
        self.assertFalse(any(call.args[0][:2] == ['paseo', 'send'] for call in send.call_args_list))

    def test_legacy_exhaustion_does_not_treat_sdk_watermark_as_new_human_authorization(self):
        self.grant()
        exhausted = json.loads(json.dumps(self.response))
        exhausted['ordinaryUsageAllowed'] = False
        exhausted['rateLimits']['primary']['usedPercent'] = 100
        self.assertEqual(self.status(exhausted), 'usage_budget_reached')
        grant_path, _ = recovery.grant_document(self.config)
        grant_bytes = grant_path.read_bytes()
        available = json.loads(json.dumps(self.response))
        available['rateLimits']['primary'].update(usedPercent=75, resetsAt=4000)
        state = {'reason': 'actual_quota_exhausted', 'usage_budget_paused': True,
                 'questions': {'q': {'status': 'unknown'}}}
        for kind in ('internal_watermark', 'human_pause'):
            with self.subTest(kind=kind):
                controls = dict(self.controls, last_user_at='2026-10-31T12:01:00Z',
                                user_key='2026-10-31T12:01:00Z',
                                control='paused' if kind == 'human_pause' else None)
                with patch.object(recovery, 'percentage_pause_controls', return_value=controls), \
                     patch.object(recovery, 'rpc', return_value=available) as rpc:
                    with self.assertRaises(ValueError):
                        recovery.resume_budget_pause(self.runtime, self.config, dict(state))
                rpc.assert_not_called()
        self.assertEqual(self.current['status'], 'paused')
        self.assertNotIn('percentage_pause', state)
        self.assertEqual(state['questions'], {'q': {'status': 'unknown'}})
        self.assertEqual(grant_path.read_bytes(), grant_bytes)
        self.assertFalse(recovery.effective_usage_budget(self.config).get('_full_budget_once'))

    def test_owned_pause_resumes_at_ninety_without_reset_or_credit_reserve(self):
        # 原credits参与预算，已耗尽仍有普通资格；不能残留旧credits保留线。
        self.response['rateLimits']['credits'].update(hasCredits=True, balance='10')
        self.config['usage_budget'] = recovery.initialize_usage_budget(self.response, 100)
        baseline = json.loads(json.dumps(self.config['usage_budget']))
        self.grant()
        path, _ = recovery.grant_document(self.config)
        grant_bytes = path.read_bytes()
        low = json.loads(json.dumps(self.response))
        low['rateLimits']['primary']['usedPercent'] = 90
        low['rateLimits']['credits'].update(hasCredits=False, balance='0')
        outcome, state, calls, sends = self.monitor_full_budget(low)
        self.assertEqual(outcome, 'percentage_resumed')
        self.assertEqual(self.current['status'], 'active')
        self.assertEqual(state['percentage_pause']['resume_status'], 'sent')
        self.assertEqual(calls.count('thread/goal/set'), 1)
        self.assertEqual(sends, 1)
        self.assertEqual(path.read_bytes(), grant_bytes)
        self.assertEqual(self.config['usage_budget'], baseline)
        self.assertEqual(self.config['account_budgets'], self.before['account_budgets'])
        self.assertEqual(state['questions'], {'q': {'status': 'unknown'}})
        self.assertEqual(state['dispatch_key'], 'uncertain')

    def test_owned_pause_default_and_ended_grant_do_not_upgrade_plain_continue(self):
        low = json.loads(json.dumps(self.response))
        low['rateLimits']['primary']['usedPercent'] = 90
        for policy in ('default', 'ended'):
            with self.subTest(policy=policy):
                if policy == 'ended':
                    self.grant()
                    recovery.end_full_budget(self.config, 'explicit_revoke')
                outcome, _, calls, sends = self.monitor_full_budget(low)
                self.assertIn(outcome, ('percentage_reset_wait', 'account_budget_wait'))
                self.assertNotIn('thread/goal/set', calls)
                self.assertEqual(sends, 0)
                self.assertNotIn('_full_budget_once', recovery.effective_usage_budget(self.config))

    def test_owned_pause_changed_goal_never_refreshes_account_history(self):
        self.grant()
        before = json.loads(json.dumps(self.config))
        self.current['objective'] = '不同目标'
        with patch.object(recovery, 'refresh_account_budget') as account, \
             patch.object(recovery, 'refresh_usage_cycles') as cycles:
            outcome, _, calls, sends = self.monitor_full_budget(self.response)
        self.assertEqual(outcome, 'percentage_pause_not_owned')
        account.assert_not_called(); cycles.assert_not_called()
        self.assertEqual(sends, 0)
        self.assertNotIn('thread/goal/set', calls)
        self.assertEqual(self.config, before)
        self.assertEqual(recovery.full_budget_end_reason(self.config), 'goal_identity_changed')

    def test_owned_pause_grant_never_resumes_unknown_hard_cycle_identity_or_manual(self):
        self.grant()
        for kind in ('unknown', 'hard', 'cycle', 'manual', 'account', 'exhausted'):
            with self.subTest(kind=kind):
                snapshot = json.loads(json.dumps(self.response))
                snapshot['rateLimits']['primary']['usedPercent'] = 90
                if kind == 'unknown': snapshot.pop('ordinaryUsageAllowed')
                elif kind == 'hard': snapshot['rateLimits']['spendControlReached'] = True
                elif kind == 'cycle': snapshot['rateLimits']['primary']['resetsAt'] = 4000
                elif kind == 'account': snapshot['accountId'] = 'other'
                elif kind == 'exhausted':
                    # 独立的新明确授权；前一个账号变化已撤销。
                    self.controls['user_key'] = '2026-10-31T12:02:00Z'
                    self.grant()
                    snapshot['ordinaryUsageAllowed'] = False
                    snapshot['rateLimits']['primary']['usedPercent'] = 100
                outcome, _, calls, sends = self.monitor_full_budget(snapshot, controls_changed=kind == 'manual')
                self.assertNotEqual(outcome, 'percentage_resumed')
                self.assertNotIn('thread/goal/set', calls)
                self.assertEqual(sends, 0)
        self.assertEqual(recovery.full_budget_end_reason(self.config), 'actual_quota_exhausted')
        reset = json.loads(json.dumps(self.response))
        reset['rateLimits']['primary']['resetsAt'] = 4000
        outcome, _, calls, _ = self.monitor_full_budget(reset)
        self.assertEqual(outcome, 'percentage_reset_wait')
        self.assertNotIn('thread/goal/set', calls)

    def run_resume_cli(self, has_grant, same_human=False):
        root = Path(self.temp.name)
        self.config.update(agent='6b56ea51-0bb7-44a7-8547-7073f8f2c7e4',
                           thread='01a175a5-d24c-71f0-86d7-ae693a28931d')
        self.runtime = root / '.agents/project/guards' / self.config['agent']
        self.runtime.mkdir(parents=True)
        if same_human:
            self.controls.update(last_user_at='2026-10-30T15:04:39.850Z', user_key='2026-10-30T17:10:30.105Z')
        if has_grant:
            self.grant()
        (self.runtime / 'config.json').write_text(json.dumps(self.config))
        state = self.state()
        state['percentage_pause']['resume_status'] = 'cancelled'
        if same_human:
            state['percentage_pause']['controls'].update(
                last_user_at='2026-10-30T15:04:39.850Z', user_key='2026-10-30T16:29:38.580Z')
        (self.runtime / 'state.json').write_text(json.dumps(state))
        grant_path = self.runtime / 'full-budget-once.json'
        before = grant_path.read_bytes() if grant_path.exists() else None
        # 新协调消息改变水位，恢复应沿用原grant，不重新授予。
        self.controls.update(
            last_user_at='2026-10-30T15:04:39.850Z' if same_human else '2026-10-31T13:00:00Z',
            user_key='2026-10-30T17:16:38.684Z' if same_human else '2026-10-31T13:00:00Z')
        def rpc(method, params):
            if method == 'thread/goal/set':
                self.current['status'] = params['status']
                return {}
            return self.response
        def launch(*args, **kwargs):
            (self.runtime / 'supervisor.json').write_text('{}')
            return type('FakeProcess', (), {'pid': 123, 'poll': lambda _: None})()
        argv = [str(SCRIPT), 'start', '--agent', self.config['agent'], '--thread', self.config['thread'],
                '--resume-budget-paused', '--allow-existing-credits']
        with patch.object(recovery, 'ROOT', root), patch.object(sys, 'argv', argv), \
             patch.object(recovery.platform, 'command'), \
             patch.object(recovery, 'config_for', side_effect=lambda _: dict(self.config)), \
             patch.object(recovery, 'percentage_pause_controls', return_value=self.controls), \
             patch.object(recovery, 'timeline', return_value={}), \
             patch.object(recovery, 'rpc', side_effect=rpc) as calls, \
             patch.object(recovery, 'authorize_full_budget') as authorize, \
             patch.object(recovery.subprocess, 'Popen', side_effect=launch):
            if has_grant:
                recovery.main()
                self.assertEqual(self.current['status'], 'active')
                self.assertEqual(grant_path.read_bytes(), before)
                saved = json.loads((self.runtime / 'state.json').read_text())
                self.assertEqual(saved['percentage_pause']['resume_status'], 'explicitly_resumed')
            else:
                with self.assertRaisesRegex(SystemExit, '缺少同任务有效'):
                    recovery.main()
                self.assertEqual(self.current['status'], 'paused')
                self.assertFalse(grant_path.exists())
                self.assertFalse(any(c.args[0] == 'thread/goal/set' for c in calls.call_args_list))
            authorize.assert_not_called()

    def run_rebase_cli(self, failure=None, existing_grant=False):
        root = self.runtime / 'root'; root.mkdir()
        self.config.update(agent='6b56ea51-0bb7-44a7-8547-7073f8f2c7e4',
                           thread='01a175a5-d24c-71f0-86d7-ae693a28931d')
        self.runtime = root / '.agents/project/guards' / self.config['agent']; self.runtime.mkdir(parents=True)
        if existing_grant: self.grant()
        (self.runtime / 'config.json').write_text(json.dumps(self.config))
        original = (self.runtime / 'config.json').read_bytes()
        state = self.state(); state.update(quota_wait_started_at=1, quota_wait_expired_at=3601)
        if failure == 'manual': state.pop('usage_budget_paused')
        (self.runtime / 'state.json').write_text(json.dumps(state))
        count = 0
        def rpc(method, params):
            nonlocal count
            if method == 'thread/goal/set': self.current['status'] = params['status']; return {}
            count += 1
            response = json.loads(json.dumps(self.response))
            if failure == 'depleted' and count >= 3: response['rateLimits']['primary']['usedPercent'] = 99
            return response
        def launch(*args, **kwargs):
            (self.runtime / 'supervisor.json').write_text('{}')
            return type('FakeProcess', (), {'pid': 123, 'poll': lambda _: None})()
        argv = [str(SCRIPT), 'start', '--agent', self.config['agent'], '--thread', self.config['thread'],
                '--rebase-usage-budget', '--resume-budget-paused']
        if existing_grant and failure != 'grant': argv.append('--end-full-budget-once')
        original_atomic = recovery.atomic
        def atomic(path, value):
            if failure == 'audit_write' and path.name.startswith('usage-budget-rebase-'): raise OSError('audit failed')
            if failure == 'config_write' and path.name == 'config.json': raise OSError('config failed')
            original_atomic(path, value)
        with patch.object(recovery, 'ROOT', root), patch.object(sys, 'argv', argv), \
             patch.object(recovery.platform, 'command'), \
             patch.object(recovery, 'config_for', side_effect=lambda _: dict(self.config)), \
             patch.object(recovery, 'percentage_pause_controls', return_value=self.controls), \
             patch.object(recovery, 'timeline', return_value={}), \
             patch.object(recovery, 'rpc', side_effect=rpc) as calls, \
             patch.object(recovery, 'atomic', side_effect=atomic), \
             patch.object(recovery.subprocess, 'Popen', side_effect=launch):
            if failure:
                with self.assertRaises((ValueError, OSError)): recovery.main()
                self.assertEqual((self.runtime / 'config.json').read_bytes(), original)
                self.assertEqual(self.current['status'], 'active' if failure == 'config_write' else 'paused')
                self.assertEqual(any(call.args[0] == 'thread/goal/set' for call in calls.call_args_list), failure == 'config_write')
                if failure == 'config_write':
                    saved_state = json.loads((self.runtime / 'state.json').read_text())
                    self.assertEqual(saved_state['percentage_pause']['resume_status'], 'explicitly_resumed')
                    self.assertTrue(list(self.runtime.glob('usage-budget-rebase-*.json')))
            else:
                recovery.main()
                saved = json.loads((self.runtime / 'config.json').read_text())
                self.assertEqual(saved['usage_budget']['fraction'], '0.8')
                self.assertEqual(saved['account_budgets'], self.config['account_budgets'])
                self.assertEqual(self.current['status'], 'active')
                final_state = json.loads((self.runtime / 'state.json').read_text())
                self.assertEqual(final_state['questions'], state['questions'])
                self.assertNotIn('quota_wait_started_at', final_state)
                self.assertNotIn('quota_wait_expired_at', final_state)
                self.assertFalse(recovery.grant_document(saved)[1].get('active'))

    def test_public_rebase_resume_without_full_budget_and_with_explicit_revoke(self):
        for grant in (False, True):
            with self.subTest(existing_grant=grant), tempfile.TemporaryDirectory() as directory:
                self.runtime = Path(directory); self.current['status'] = 'paused'
                self.run_rebase_cli(existing_grant=grant)

    def test_public_rebase_failed_checks_never_half_write_baseline_or_activate(self):
        for kind in ('manual', 'depleted', 'audit_write', 'config_write', 'grant'):
            with self.subTest(failure=kind), tempfile.TemporaryDirectory() as directory:
                self.runtime = Path(directory); self.current['status'] = 'paused'
                self.run_rebase_cli(failure=kind, existing_grant=kind == 'grant')

    def test_active_rebase_new_stop_wins_and_old_stop_does_not_block(self):
        for phase in ('prepare', 'second_rpc', 'commit', 'old_stop'):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.current['status'] = 'active'
                self.config.update(agent='6b56ea51-0bb7-44a7-8547-7073f8f2c7e4',
                                   thread='01a175a5-d24c-71f0-86d7-ae693a28931d')
                runtime = root / '.agents/project/guards' / self.config['agent']; runtime.mkdir(parents=True)
                config_path = runtime / 'config.json'; config_path.write_text(json.dumps(self.config))
                previous = config_path.read_bytes()
                disabled = runtime / 'disabled'
                if phase == 'old_stop':
                    disabled.touch(); os.utime(disabled, (1, 1))
                configs = 0; quotas = 0
                def config_for(_):
                    nonlocal configs
                    configs += 1
                    if phase == 'prepare' and configs == 2: disabled.touch()
                    return dict(self.config)
                def rpc(method, params):
                    nonlocal quotas
                    self.assertEqual(method, 'account/rateLimits/read')
                    quotas += 1
                    if phase == 'second_rpc' and quotas == 2: disabled.touch()
                    return self.response
                original_atomic = recovery.atomic
                def atomic(path, value):
                    original_atomic(path, value)
                    if phase == 'commit' and path.name.startswith('usage-budget-rebase-'): disabled.touch()
                def launch(*args, **kwargs):
                    (runtime / 'supervisor.json').write_text('{}')
                    return type('FakeProcess', (), {'pid': 123, 'poll': lambda _: None})()
                argv = [str(SCRIPT), 'start', '--agent', self.config['agent'], '--thread', self.config['thread'],
                        '--rebase-usage-budget']
                with patch.object(recovery, 'ROOT', root), patch.object(sys, 'argv', argv), \
                     patch.object(recovery.platform, 'command'), patch.object(recovery, 'config_for', side_effect=config_for), \
                     patch.object(recovery, 'percentage_pause_controls', return_value=self.controls), \
                     patch.object(recovery, 'rpc', side_effect=rpc), patch.object(recovery, 'atomic', side_effect=atomic), \
                     patch.object(recovery.subprocess, 'Popen', side_effect=launch) as spawn:
                    if phase == 'old_stop':
                        recovery.main()
                        self.assertFalse(disabled.exists())
                        self.assertNotEqual(config_path.read_bytes(), previous)
                        spawn.assert_called_once()
                    else:
                        with self.assertRaisesRegex(ValueError, '并发停止优先'): recovery.main()
                        self.assertTrue(disabled.exists())
                        self.assertEqual(config_path.read_bytes(), previous)
                        spawn.assert_not_called()
                self.assertEqual(self.current['status'], 'active')

    def ordinary_cli(self, root, text, existing_grant=False):
        self.current['status'] = 'active'
        self.config.update(agent='6b56ea51-0bb7-44a7-8547-7073f8f2c7e4',
                           thread='01a175a5-d24c-71f0-86d7-ae693a28931d')
        runtime = root / '.agents/project/guards' / self.config['agent']
        runtime.mkdir(parents=True)
        if existing_grant:
            recovery.authorize_full_budget(runtime, self.config, dict(self.current), self.controls, self.response)
            (runtime / 'config.json').write_text(json.dumps(self.config))
        grant = runtime / 'full-budget-once.json'
        before = grant.read_bytes() if grant.exists() else None
        def launch(*args, **kwargs):
            (runtime / 'supervisor.json').write_text('{}')
            return type('FakeProcess', (), {'pid': 123, 'poll': lambda _: None})()
        argv = [str(SCRIPT), 'start', '--agent', self.config['agent'], '--thread', self.config['thread']]
        with patch.object(recovery, 'ROOT', root), patch.object(sys, 'argv', argv), \
             patch.object(recovery.platform, 'command'), \
             patch.object(recovery, 'config_for', side_effect=lambda _: dict(self.config)), \
             patch.object(recovery, 'timeline', return_value={'kind': 'user_message',
                          'user_context': {'message': text}}), \
             patch.object(recovery, 'rpc', return_value=self.response), \
             patch.object(recovery, 'authorize_full_budget') as authorize, \
             patch.object(recovery.subprocess, 'Popen', side_effect=launch):
            recovery.main()
        authorize.assert_not_called()
        saved = json.loads((runtime / 'config.json').read_text())
        self.assertEqual(saved['usage_budget']['fraction'], '0.8')
        self.assertEqual(grant.read_bytes() if grant.exists() else None, before)
        return saved

    def test_public_cli_requires_explicit_flag_independent_of_control_text(self):
        # 只验证公开入口不按文本授予；主控的任意自然语言理解不在此单元测试内。
        cases = [('ordinary', '继续'), ('development', '持续开发'), ('complete', '全部处理好'),
                 ('fast', '尽快完成'), ('negative', '不要解除限制'),
                 ('question', '是否能不限制额度'), ('quote', '讨论“额度用完为止”的含义'),
                 ('affirmative_without_mapping', '允许用满100%')]
        for kind, text in cases:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                saved = self.ordinary_cli(Path(directory), text)
                self.assertFalse(Path(saved['full_budget_grant_file']).exists())
                self.assertNotIn('_full_budget_once', recovery.effective_usage_budget(saved))

    def test_public_ordinary_reload_preserves_grant_bytes_and_original_budget(self):
        with tempfile.TemporaryDirectory() as directory:
            saved = self.ordinary_cli(Path(directory), '继续把全部处理好', existing_grant=True)
            self.assertEqual(saved['usage_budget'], self.before['usage_budget'])
            self.assertIn('_full_budget_once', recovery.effective_usage_budget(saved))

    def test_public_resume_reuses_existing_grant_after_new_user_message(self):
        self.run_resume_cli(has_grant=True)

    def test_public_resume_same_human_uses_actual_grant_and_coordination_watermarks(self):
        self.run_resume_cli(has_grant=True, same_human=True)

    def test_same_human_requires_current_valid_grant_between_pause_and_coordination(self):
        self.controls.update(last_user_at='2026-10-30T15:04:39.850Z', user_key='2026-10-30T17:10:30.105Z')
        self.grant()
        path, original = recovery.grant_document(self.config)
        self.controls['user_key'] = '2026-10-30T17:16:38.684Z'
        for kind in ('old_grant', 'future_grant', 'unknown_grant', 'other_human', 'ended', 'identity', 'stale_user', 'manual'):
            document = json.loads(json.dumps(original))
            grant = document['grants'][document['active']]
            if kind == 'old_grant': grant['authorization']['user_key'] = '2026-10-30T16:29:38.580Z'
            if kind == 'future_grant': grant['authorization']['user_key'] = '2026-10-30T17:20:00Z'
            if kind == 'unknown_grant': grant['authorization']['user_key'] = None
            if kind == 'other_human': grant['authorization']['last_user_at'] = '2026-10-30T14:00:00Z'
            if kind == 'ended': grant['status'] = 'ended'; document['active'] = None
            if kind == 'identity': grant['goal_id'] = 'other'
            path.write_text(json.dumps(document))
            state = self.state()
            state['percentage_pause'].update(resume_status='cancelled', controls={
                'last_user_at': '2026-10-30T15:04:39.850Z', 'user_key': '2026-10-30T16:29:38.580Z'})
            controls = dict(self.controls)
            if kind == 'stale_user': controls['user_key'] = '2026-10-30T16:29:38.580Z'
            if kind == 'manual': state.pop('usage_budget_paused')
            with self.subTest(kind=kind), patch.object(recovery, 'percentage_pause_controls', return_value=controls), \
                 patch.object(recovery, 'rpc') as rpc:
                with self.assertRaises(ValueError): recovery.resume_budget_pause(self.runtime, self.config, state)
                rpc.assert_not_called()

    def test_public_resume_without_grant_refuses_without_goal_write(self):
        self.run_resume_cli(has_grant=False)

    def test_cancelled_auto_recovery_accepts_only_explicit_new_authorization(self):
        self.grant()
        state = self.state()
        state.update(status='stopped', reason='stopped')
        state['percentage_pause']['resume_status'] = 'cancelled'
        def rpc(method, params):
            if method == 'thread/goal/set':
                self.current['status'] = params['status']
                return {}
            return self.response
        with patch.object(recovery, 'percentage_pause_controls', return_value=self.controls), \
             patch.object(recovery, 'rpc', side_effect=rpc), \
             patch.object(recovery, 'timeline', return_value={}):
            recovery.resume_budget_pause(self.runtime, self.config, state)
        self.assertEqual(self.current['status'], 'active')
        self.assertEqual(state['percentage_pause']['resume_status'], 'explicitly_resumed')
        self.assertEqual(state['questions'], {'q': {'status': 'unknown'}})
        self.assertEqual(self.config['usage_budget'], self.before['usage_budget'])

    def test_cancelled_recovery_rejects_stale_manual_or_changed_goal(self):
        self.grant()
        for kind in ('older', 'same', 'unknown_watermark', 'manual', 'goal', 'manual_repause'):
            state = self.state()
            state['percentage_pause']['resume_status'] = 'cancelled'
            controls = dict(self.controls)
            if kind == 'older': controls['last_user_at'] = '2026-10-31T10:00:00Z'
            if kind == 'same': controls = dict(state['percentage_pause']['controls'])
            if kind == 'unknown_watermark': controls['user_key'] = None
            if kind == 'manual': state.pop('usage_budget_paused')
            if kind == 'goal': state['percentage_pause']['goal']['objective'] = 'other'
            if kind == 'manual_repause': state['percentage_pause']['goal']['updated_at_ms'] -= 1
            with self.subTest(kind=kind), patch.object(recovery, 'percentage_pause_controls', return_value=controls), \
                 patch.object(recovery, 'rpc') as rpc:
                with self.assertRaises(ValueError):
                    recovery.resume_budget_pause(self.runtime, self.config, state)
                rpc.assert_not_called()
                self.assertEqual(state['percentage_pause']['resume_status'], 'cancelled')

    def test_manual_pause_changed_goal_permissions_or_same_authorization_cannot_resume(self):
        self.grant()
        for kind in ('manual', 'goal', 'permission', 'same_user', 'unknown_send'):
            state = self.state()
            controls = self.controls
            if kind == 'manual': state.pop('usage_budget_paused')
            if kind == 'goal': state['percentage_pause']['goal']['objective'] = 'other'
            if kind == 'permission': controls = None
            if kind == 'same_user': state['percentage_pause']['controls'] = self.controls
            if kind == 'unknown_send': state['percentage_pause']['resume_status'] = 'activating'
            with self.subTest(kind=kind), patch.object(recovery, 'percentage_pause_controls', return_value=controls), \
                 patch.object(recovery, 'rpc') as rpc:
                with self.assertRaises(ValueError): recovery.resume_budget_pause(self.runtime, self.config, state)
                rpc.assert_not_called()

    def test_worker_plain_stop_retains_grant_but_user_pause_clears_it(self):
        self.current['status'] = 'active'
        self.grant()
        (self.runtime / 'config.json').write_text(json.dumps(self.config))
        # plain stop的disabled标记终止worker，不代表用户停止完整任务。
        (self.runtime / 'disabled').touch()
        recovery.worker(self.runtime, os.getppid())
        self.assertIsNotNone(recovery.grant_document(self.config)[1]['active'])
        (self.runtime / 'disabled').unlink()
        self.current['status'] = 'paused'
        recovery.worker(self.runtime, os.getppid())
        self.assertIsNone(recovery.grant_document(self.config)[1]['active'])

    def test_official_goal_creation_or_objective_mismatch_refuses_resume(self):
        self.grant()
        with patch.object(recovery, 'bound_goal', return_value=dict(self.current, createdAt=99)), \
             patch.object(recovery, 'rpc') as rpc:
            with self.assertRaises(ValueError): recovery.resume_budget_pause(self.runtime, self.config, self.state())
            rpc.assert_not_called()
        state = self.state()
        def change_during_activation(method, params):
            if method == 'thread/goal/set':
                self.current.update(status='active', objective='异步替换目标')
                return {}
            return self.response
        with patch.object(recovery, 'percentage_pause_controls', return_value=self.controls), \
             patch.object(recovery, 'rpc', side_effect=change_during_activation):
            with self.assertRaises(ValueError):
                recovery.resume_budget_pause(self.runtime, self.config, state)
        self.assertEqual(state['percentage_pause']['resume_status'], 'activating')
        self.assertNotIn('control_watermark', state)

    def test_new_control_after_authorization_cannot_be_watermarked_away(self):
        turn = {'semantic_blocked': True, 'goal_control': {'status': 'paused', 'key': '2026-10-31T12:01:00Z'},
                'user_context': {'key': '2026-10-31T12:00:00Z'}, 'payload': {}}
        agent = {'PendingPermissions': [], 'QuestionBinding': {'id': 'a', 'thread': 't',
                 'lastUserMessageAt': '2026-10-31T12:00:00Z'}}
        with patch.object(recovery, 'inspect', return_value=agent), patch.object(recovery, 'timeline', return_value=turn):
            self.assertIsNone(recovery.percentage_pause_controls(self.config, allow_old_control=True))


class NotificationRegression(unittest.TestCase):
    def test_system_notification_uses_agent_display_name_without_external_delivery(self):
        with patch.object(recovery.sys, 'platform', 'darwin'), \
             patch.object(recovery.subprocess, 'run') as run:
            self.assertTrue(ORIGINAL_NOTIFY('隔离测试通知'))
        self.assertIn('with title "Agent 持续开发守护"', run.call_args.args[0][2])
        self.assertEqual(run.call_args.args[0][-1], '隔离测试通知')

    def test_pending_request_cannot_mask_new_control_boundary_in_worker(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder)
            config = {'goal_id': 'g', 'agent': 'a', 'thread': 't', 'rollout': 'r'}
            (runtime / 'config.json').write_text(json.dumps(config))
            agent = {'Status': 'idle', 'Cwd': str(recovery.ROOT),
                     'PendingPermissions': [{'id': 'same', 'kind': 'tool'}]}
            turns = [
                {'kind': 'task_complete', 'key': 'one', 'payload': {}, 'tools': 0},
                {'kind': 'turn_aborted', 'key': 'two', 'payload': {}, 'tools': 0},
                {'kind': 'user_message', 'key': 'three', 'payload': {}, 'tools': 0,
                 'input_provenance': 'unknown', 'post_user_activity': False}]
            reasons = []
            def advance(*_):
                reasons.append(json.loads((runtime / 'state.json').read_text())['reason'])
                if len(reasons) == len(turns):
                    (runtime / 'disabled').touch()
            with patch.object(recovery, 'binding_valid', return_value=True), \
                 patch.object(recovery, 'goal', return_value={'id': 'g', 'status': 'active'}), \
                 patch.object(recovery, 'inspect', return_value=agent), \
                 patch.object(recovery, 'timeline', side_effect=lambda *_: turns[len(reasons)]), \
                 patch.object(recovery.questions, 'step', return_value=None), \
                 patch.object(recovery.time, 'time', return_value=1000), \
                 patch.object(recovery, 'sleep_until', side_effect=advance), \
                 patch.object(recovery, 'run_json') as send, \
                 patch.object(recovery, 'notify', return_value=True) as notify:
                recovery.worker(runtime, os.getppid())
            self.assertEqual(reasons, ['permissions_pending', 'turn_aborted_unclassified',
                                       'input_provenance_unverified'])
            self.assertEqual(notify.call_count, 3)
            send.assert_not_called()

    def test_binding_snapshot_failure_in_worker_remains_attention(self):
        for error in (FileNotFoundError('secret-path'), json.JSONDecodeError('secret', '', 0)):
            with self.subTest(error=type(error).__name__), tempfile.TemporaryDirectory() as folder:
                runtime = Path(folder)
                record = runtime / 'agent.json'
                config = {'goal_id': 'g', 'agent': 'a', 'thread': 't', 'agent_record': str(record)}
                (runtime / 'config.json').write_text(json.dumps(config))
                original = Path.read_text
                def read(path, *args, **kwargs):
                    if path == record:
                        raise error
                    return original(path, *args, **kwargs)
                observed = []
                def stop(*_):
                    observed.append(json.loads((runtime / 'state.json').read_text()))
                    (runtime / 'disabled').touch()
                with patch.object(Path, 'read_text', read), \
                     patch.object(recovery, 'goal', return_value={'id': 'g', 'status': 'active'}), \
                     patch.object(recovery, 'sleep_until', side_effect=stop), \
                     patch.object(recovery, 'notify', return_value=True) as notify, \
                     patch.object(recovery, 'inspect') as inspect:
                    recovery.worker(runtime, os.getppid())
                self.assertEqual(observed[0]['status'], 'needs_attention')
                self.assertEqual(observed[0]['reason'], 'task_binding_unverifiable')
                self.assertNotIn('secret', json.dumps(observed))
                self.assertEqual(notify.call_count, 1)
                inspect.assert_not_called()

    def test_paused_does_not_infer_human_or_reuse_old_budget_evidence(self):
        config = {'goal_id': 'g'}
        state = {'status': 'paused', 'reason': 'paused', 'usage_budget_paused': True,
                 'percentage_pause': {'goal': {'id': 'old', 'status': 'paused'},
                                      'resume_status': 'cancelled'}}
        event = recovery.notification_event(state, config, 100)
        self.assertIn('来源尚未确认', event['message'])
        self.assertNotIn('人工', event['message'])
        self.assertNotIn('额度保护', event['message'])
        # 真正的人工暂停仍由原decision硬边界保留，无论有无可报告的来源证据。
        self.assertEqual(recovery.decision({'Cwd': str(recovery.ROOT)},
                         {'id': 'g', 'status': 'paused'}, None, config, 100), 'paused')
        state.update(reason='usage_budget_reached')  # 仅正式暂停调用成功后该分支记录paused。
        event = recovery.notification_event(state, config, 101)
        self.assertIn('额度保护', event['message'])
        self.assertNotIn('人工', event['message'])
        state.update(status='needs_attention', reason='usage_budget_window_changed')
        self.assertIsNotNone(recovery.notification_event(state, config, 102))
        state['status'] = 'paused'
        self.assertIsNone(recovery.notification_event(state, config, 103))

    def test_transient_wait_and_reason_only_changes(self):
        config = {'goal_id': 'g'}
        state = {'status': 'needs_attention', 'reason': 'awaiting_user_input_activity'}
        self.assertIsNone(recovery.notification_event(state, config, 10))
        state['status'] = 'working'
        self.assertIsNone(recovery.notification_event(state, config, 100))
        state['status'] = 'needs_attention'
        self.assertIsNone(recovery.notification_event(state, config, 200))
        self.assertIsNone(recovery.notification_event(state, config, 319))
        state['reason'] = 'authentication_or_permission_error'
        self.assertIsNotNone(recovery.notification_event(state, config, 320))
        state['reason'] = 'task_binding_changed'
        self.assertIsNotNone(recovery.notification_event(state, config, 321))
        state = json.loads(json.dumps(state))
        self.assertIsNone(recovery.notification_event(state, config, 322))
        self.assertIsNone(recovery.notification_event(state, config, 1))  # 时钟回退。
        self.assertIsNotNone(recovery.notification_event(state, {'goal_id': 'new-goal'}, 323))

    def test_question_reason_keeps_control_reason_and_filters_stale_or_free_text(self):
        agent = {'PendingPermissions': [{'id': 'current-secret-id'}]}
        state = {'question_wait_reasons': [
            {'id': 'old', 'reason': 'sensitive_action'},
            {'id': 'current-secret-id', 'reason': 'classifier_exception'},
            {'id': 'current-secret-id', 'reason': 'secret-free-text'}]}
        details = recovery.question_attention(state, agent, 'question_wait')
        self.assertEqual(details['reason'], 'question_wait')
        self.assertEqual(details['notification_reason'], 'question_classifier_exception')
        self.assertNotIn('secret', json.dumps(details))
        state['question_wait_reasons'].append({'id': 'current-secret-id', 'reason': 'external_authorization'})
        details = recovery.question_attention(state, agent, 'question_quota_wait')
        self.assertEqual(details['reason'], 'question_quota_wait')
        self.assertEqual(details['notification_reason'], 'question_external_authorization')
        state.update(status='needs_attention', **details)
        self.assertIsNotNone(recovery.notification_event(state, {'goal_id': 'g'}, 0))

    def test_same_reason_new_identity_or_permission_has_new_object(self):
        config = {'goal_id': 'g'}
        current = {'id': 'g', 'status': 'active'}
        agent = {'Status': 'idle', 'Cwd': str(recovery.ROOT),
                 'PendingPermissions': [{'id': 'a', 'kind': 'tool', 'input': 'secret'}]}
        first = recovery.attention_details(agent, current, None, config)
        agent['PendingPermissions'][0]['id'] = 'b'
        second = recovery.attention_details(agent, current, None, config)
        self.assertEqual(first['reason'], second['reason'])
        self.assertNotEqual(first['attention_object'], second['attention_object'])
        self.assertNotIn('secret', json.dumps(first))
        one = recovery.binding_attention_object({'id': 'a', 'credential': 'never-read-field'})
        self.assertEqual(one, recovery.binding_attention_object({'id': 'a', 'credential': 'different-secret'}))
        self.assertNotEqual(one, recovery.binding_attention_object({'id': 'b', 'credential': 'different-secret'}))

    def test_binding_validation_and_digest_share_one_read_even_if_file_changes(self):
        config = {'agent_record': 'unused.json', 'agent': 'a', 'thread': 't'}
        first = {'id': 'changed', 'provider': 'codex', 'cwd': str(recovery.ROOT),
                 'persistence': {'provider': 'codex', 'sessionId': 't', 'metadata': {'threadId': 't'}}}
        evidence = {}
        with patch.object(Path, 'read_text', side_effect=[json.dumps(first), FileNotFoundError()]) as read:
            self.assertFalse(recovery.binding_valid(config, evidence))
        self.assertEqual(read.call_count, 1)
        self.assertEqual(evidence['reason'], 'task_binding_changed')
        self.assertEqual(evidence['attention_object'], recovery.binding_attention_object(first))

    def test_worker_flapping_permission_is_not_notified_twice(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder)
            config = {'goal_id': 'g', 'agent': 'a', 'thread': 't', 'rollout': 'r'}
            (runtime / 'config.json').write_text(json.dumps(config))
            agent = {'Status': 'idle', 'Cwd': str(recovery.ROOT),
                     'PendingPermissions': [{'id': 'p', 'kind': 'tool'}]}
            turn = {'kind': 'task_complete', 'key': 't', 'payload': {}, 'tools': 0}
            clock = [1000]
            count = [0]
            def advance(*_):
                count[0] += 1
                clock[0] += 120
                if count[0] == 1:
                    agent.update(Status='running', PendingPermissions=[])
                elif count[0] == 2:
                    agent.update(Status='idle', PendingPermissions=[{'id': 'p', 'kind': 'tool'}])
                else:
                    (runtime / 'disabled').touch()
            with patch.object(recovery, 'binding_valid', return_value=True), \
                 patch.object(recovery, 'goal', return_value={'id': 'g', 'status': 'active'}), \
                 patch.object(recovery, 'inspect', side_effect=lambda *_: dict(agent)), \
                 patch.object(recovery, 'timeline', return_value=turn), \
                 patch.object(recovery.questions, 'step', return_value=None), \
                 patch.object(recovery.time, 'time', side_effect=lambda: clock[0]), \
                 patch.object(recovery, 'sleep_until', side_effect=advance), \
                 patch.object(recovery, 'notify', return_value=True) as notify:
                recovery.worker(runtime, os.getppid())
            self.assertEqual(notify.call_count, 1)

    def test_precise_attention_reason_does_not_invent_authentication(self):
        agent = {'Status': 'running', 'Cwd': str(recovery.ROOT)}
        config = {'goal_id': 'g'}
        current = {'id': 'g', 'status': 'active'}
        turn = {'kind': 'user_message', 'key': 'input', 'payload': {},
                'input_provenance': 'verified_user', 'post_user_activity': False}
        details = recovery.attention_details(agent, current, turn, config)
        self.assertEqual(details['reason'], 'awaiting_user_input_activity')
        self.assertEqual(recovery.decision(agent, current, turn, config, 0), 'needs_attention')
        auth = dict(turn, input_provenance='verified_user', post_user_activity=True,
                    kind='task_complete', payload={'error': {'message': '401 secret-token'}})
        details = recovery.attention_details(dict(agent, Status='idle'), current, auth, config)
        self.assertEqual(details['reason'], 'authentication_or_permission_error')
        self.assertNotIn('secret-token', json.dumps(details))

    def test_stable_wait_restart_reminder_and_new_hard_object(self):
        config = {'goal_id': 'g', 'agent': 'a', 'thread': 't'}
        state = {'status': 'needs_attention', 'reason': 'awaiting_new_turn', 'attention_object': 'turn-a'}
        self.assertIsNone(recovery.notification_event(state, config, 100))
        self.assertIsNone(recovery.notification_event(state, config, 219))
        event = recovery.notification_event(state, config, 220)
        self.assertNotIn('权限', event['message'])
        state = json.loads(json.dumps(state))  # 模拟持久状态重启。
        state['status'] = 'working'
        self.assertIsNone(recovery.notification_event(state, config, 230))
        state['status'] = 'needs_attention'
        self.assertIsNone(recovery.notification_event(state, config, 240))
        self.assertIsNone(recovery.notification_event(state, config, 360))
        self.assertIsNotNone(recovery.notification_event(state, config, 2020))
        state.update(reason='permissions_pending', attention_object='permission-a')
        self.assertIsNotNone(recovery.notification_event(state, config, 2021))
        self.assertIsNone(recovery.notification_event(state, config, 2022))
        state['attention_object'] = 'permission-b'
        self.assertIsNotNone(recovery.notification_event(state, config, 2023))
        state.update(status='paused', reason='paused')
        self.assertIsNotNone(recovery.notification_event(state, config, 2024))


class SupervisorCleanupOrder(unittest.TestCase):
    """监督标记消失必须代表子进程标记已清；不依赖调度命中竞态窗口。"""
    def test_supervisor_completion_marker_is_removed_after_worker_and_pid(self):
        with tempfile.TemporaryDirectory() as directory:
            runtime = Path(directory)
            for name in ('disabled', 'worker.json', 'pid', 'supervisor.json'):
                (runtime / name).touch()
            removed = []
            original = Path.unlink
            def unlink(path, *args, **kwargs):
                if path.parent == runtime:
                    removed.append(path.name)
                return original(path, *args, **kwargs)
            with patch.object(recovery.platform, 'WINDOWS', False), \
                 patch.object(recovery.signal, 'signal'), \
                 patch.object(recovery.os, 'close'), \
                 patch.object(recovery, 'publish_registry'), \
                 patch.object(Path, 'unlink', autospec=True, side_effect=unlink):
                recovery.supervise(runtime, 999)
            self.assertEqual(removed, ['worker.json', 'pid', 'supervisor.json'])


if __name__ == '__main__':
    unittest.main()
