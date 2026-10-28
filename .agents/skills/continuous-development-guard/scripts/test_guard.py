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
        state_path = self.runtime / 'state.json'
        state = json.loads(state_path.read_text())
        state.update(attempts=2, retry_at=time.time() + 3600, pending_key='original', dispatch_key='uncertain',
                     goal_control={'status': 'blocked', 'key': 'original-control'})
        state_path.write_text(json.dumps(state))
        self.call('start')
        self.call('stop')
        after = json.loads(state_path.read_text())
        for key in ('attempts', 'retry_at', 'pending_key', 'dispatch_key'):
            self.assertEqual(after[key], state[key])

    def test_supervisor_bounds_long_running_log(self):
        self.call('start')
        self.wait_for(lambda: (self.runtime / 'worker.json').exists())
        log = self.runtime / 'watch.log'
        with log.open('ab') as output:
            output.write(b'x' * (recovery.TAIL_BYTES * 3))
        self.wait_for(lambda: log.stat().st_size <= recovery.TAIL_BYTES * 2)
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
            with patch.object(recovery, 'ROOT', root), patch.object(sys, 'argv', ['guard.py', 'start', '--agent', agent]), patch.object(recovery.platform, 'command'), patch.object(recovery, 'config_for', return_value=config), patch.object(recovery, 'goal', return_value={'id': 'old', 'status': 'paused'}), patch.object(recovery, 'bound_goal', side_effect=[{'status': 'paused'}, ValueError('replaced')]), patch.object(recovery, 'rpc') as remote:
                with self.assertRaises(ValueError): recovery.main()
                remote.assert_not_called()

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
