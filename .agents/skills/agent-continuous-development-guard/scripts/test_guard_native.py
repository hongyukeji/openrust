"""原生平台隔离回归：身份、共享执行生命周期和保守能力边界，不连接真实线程。"""
import copy
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sqlite3
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import guard_codex as native

spec = importlib.util.spec_from_file_location('native_guard_test', Path(__file__).with_name('guard.py'))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
THREAD = '01a197e5-3a41-7fb2-b196-9bd80615d2b1'
AGENT = '01a197e5-3a41-7fb2-b196-9bd80615d2b2'


class NativeBackend(unittest.TestCase):
    def setUp(self):
        self.config = {'backend': 'codex', 'agent': THREAD, 'thread': THREAD, 'goal_id': 'goal'}
        self.thread = {'id': THREAD, 'cwd': str(guard.ROOT), 'source': 'cli',
                       'parentThreadId': None, 'status': {'type': 'idle'},
                       'canAcceptDirectInput': True, 'turns': []}
        self.calls = []
        self.queue = []

    def rpc(self, method, params):
        self.calls.append((method, params))
        if method == 'thread/read':
            return {'thread': copy.deepcopy(self.thread)}
        if method == 'thread/turns/list':
            return {'data': copy.deepcopy(self.thread['turns'])}
        if method == 'thread/queue/list':
            return {'data': copy.deepcopy(self.queue)}
        if method == 'thread/queue/add':
            queued = {'id': 'queued', 'clientUserMessageId': params['clientUserMessageId']}
            self.queue.append(queued)
            return {'queuedSubmission': queued}
        if method == 'thread/queue/start':
            return {'turn': {'id': 'new', 'status': 'inProgress'}}
        if method == 'turn/interrupt':
            return {}
        raise AssertionError(method)

    def test_auto_without_paseo_uses_trusted_native_identity(self):
        args = SimpleNamespace(agent=None, thread=None, backend='auto')
        self.assertEqual(native.select_backend(args, {'CODEX_THREAD_ID': THREAD}), ('codex', THREAD, THREAD))
        self.assertEqual(native.select_backend(args, {'CODEX_THREAD_ID': THREAD, 'PASEO_AGENT_ID': AGENT}),
                         ('paseo', AGENT, THREAD))
        args.backend = 'codex'
        self.assertEqual(native.select_backend(args, {'CODEX_THREAD_ID': THREAD, 'PASEO_AGENT_ID': AGENT}),
                         ('codex', THREAD, THREAD))
        with self.assertRaises(ValueError):
            native.select_backend(args, {})

    def test_native_config_requires_project_metadata_and_existing_goal(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            rollout = home / 'thread.jsonl'
            metadata = {'id': THREAD, 'cwd': str(guard.ROOT), 'source': 'cli'}
            rollout.write_text(json.dumps({'payload': metadata}) + '\n', encoding='utf-8')
            with sqlite3.connect(home / 'state_5.sqlite') as db:
                db.execute('CREATE TABLE threads (id TEXT, rollout_path TEXT, cwd TEXT)')
                db.execute('INSERT INTO threads VALUES (?,?,?)', (THREAD, str(rollout), str(guard.ROOT)))
            args = SimpleNamespace(agent=None, thread=None, backend='auto', label='native', codex_socket=None)
            current = {'id': 'goal', 'status': 'active'}
            with patch.dict(os.environ, {'CODEX_HOME': str(home), 'CODEX_THREAD_ID': THREAD}, clear=True), \
                    patch.object(guard, '_BACKEND_CONFIG', {}), \
                    patch.object(guard, 'rpc', side_effect=self.rpc), \
                    patch.object(guard, 'goal', return_value=current), \
                    patch.object(guard, 'run_json', side_effect=AssertionError('no Paseo')):
                config = guard.config_for(args)
                self.assertEqual(config['backend'], 'codex')
                self.assertEqual(config['goal_id'], 'goal')
                current['status'] = 'paused'
                with self.assertRaisesRegex(ValueError, '暂停'):
                    guard.config_for(args)
                current['status'] = 'active'
                metadata['source'] = {'subAgent': {}}
                rollout.write_text(json.dumps({'payload': metadata}) + '\n', encoding='utf-8')
                with self.assertRaisesRegex(ValueError, '主线程'):
                    guard.config_for(args)

    def test_rejects_other_project_parent_and_wrong_thread(self):
        for changes in ({'cwd': '/tmp'}, {'parentThreadId': AGENT}, {'id': AGENT},
                        {'source': {'subAgent': {'parent_thread_id': AGENT}}}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                native.validate_thread(dict(self.thread, **changes), self.config, guard.ROOT)

    def test_unloaded_is_unknown_never_running_or_recovery(self):
        self.thread.update(status={'type': 'notLoaded'}, canAcceptDirectInput=None)
        snapshot = native.snapshot(self.config, guard.ROOT, self.rpc)
        self.assertEqual(snapshot['Status'], 'unknown')
        self.assertEqual(snapshot['NativeWaitReason'], 'native_state_unverifiable')
        self.assertEqual(guard.decision(snapshot, {'id': 'goal', 'status': 'active'}, None,
                                        self.config, 0), 'needs_attention')
        with self.assertRaises(ValueError):
            native.enqueue(self.config, 'continue', self.rpc, guard.ROOT, 't', lambda: None)
        self.assertFalse(any(name == 'thread/queue/add' for name, _ in self.calls))

    def test_active_does_not_dispatch(self):
        self.thread['status'] = {'type': 'active', 'activeFlags': []}
        self.assertEqual(native.snapshot(self.config, guard.ROOT, self.rpc)['Status'], 'running')
        with self.assertRaises(ValueError):
            native.enqueue(self.config, 'continue', self.rpc, guard.ROOT, 't', lambda: None)
        self.assertFalse(any(name == 'thread/queue/add' for name, _ in self.calls))

    def test_approval_input_and_unknown_flags_do_not_fake_permission_answers(self):
        for flags, reason in ((['waitingOnApproval'], 'native_approval_pending'),
                              (['waitingOnUserInput'], 'native_input_pending'),
                              (['future'], 'native_state_unverifiable')):
            self.thread['status'] = {'type': 'active', 'activeFlags': flags}
            snapshot = native.snapshot(self.config, guard.ROOT, self.rpc)
            self.assertEqual(snapshot['NativeWaitReason'], reason)
            self.assertFalse(snapshot['PendingPermissions'])
            self.assertEqual(guard.attention_details(snapshot, {}, None, self.config)['reason'], reason)

    def test_existing_queue_prevents_duplicate_recovery(self):
        rpc = Mock(side_effect=lambda name, params: {'data': [{'id': 'existing'}]}
                   if name == 'thread/queue/list' else self.rpc(name, params))
        with self.assertRaises(ValueError):
            native.enqueue(self.config, 'continue', rpc, guard.ROOT, 't', lambda: None)
        self.assertFalse(any(name == 'thread/queue/add' for name, _ in self.calls))

    def test_idle_queues_and_starts_on_existing_server_without_resume(self):
        result = native.enqueue(self.config, 'continue', self.rpc, guard.ROOT, 't', lambda: None)
        self.assertEqual(result['turn']['id'], 'new')
        self.assertEqual([name for name, _ in self.calls],
                         ['thread/read', 'thread/queue/list', 'thread/queue/add', 'thread/read',
                          'thread/queue/list', 'thread/queue/start'])
        self.assertEqual(self.calls[-1][1]['queuedSubmissionId'], 'queued')

    def test_verified_system_error_can_start_owned_recovery(self):
        self.thread['status'] = {'type': 'systemError'}
        result = native.enqueue(self.config, 'continue', self.rpc, guard.ROOT, 't', lambda: None)
        self.assertEqual(result['turn']['id'], 'new')

    def test_goal_pause_or_replacement_during_enqueue_prevents_start(self):
        """入队期间暂停或更换目标必须保留队列身份并拒绝启动。"""
        for changed in ({'id': 'goal', 'status': 'paused'},
                        {'id': 'other', 'status': 'active'},
                        {'id': 'goal', 'status': 'active', 'objective': 'changed'}):
            with self.subTest(changed=changed):
                self.calls.clear()
                self.queue.clear()
                with patch.object(guard, 'rpc', side_effect=self.rpc), \
                        patch.object(guard, 'bound_goal', side_effect=[
                            {'id': 'goal', 'status': 'active'}, changed]):
                    with self.assertRaisesRegex(ValueError, '目标或控制'):
                        guard.send_task(self.config, 't')
                self.assertEqual(len(self.queue), 1)
                self.assertFalse(any(name == 'thread/queue/start' for name, _ in self.calls))

    def test_thread_change_or_human_queue_after_enqueue_prevents_start(self):
        """入队期间启动的turn、审批或真人排队输入不能被恢复消息抢占。"""
        for change in ('running', 'approval', 'human'):
            with self.subTest(change=change):
                self.thread.update(status={'type': 'idle'}, canAcceptDirectInput=True)
                self.calls.clear()
                self.queue.clear()
                def rpc(name, params):
                    result = self.rpc(name, params)
                    if name == 'thread/queue/add':
                        if change == 'human':
                            self.queue.append({'id': 'human', 'clientUserMessageId': 'user'})
                        else:
                            self.thread['status'] = {'type': 'active', 'activeFlags':
                                                     [] if change == 'running' else ['waitingOnApproval']}
                    return result
                with self.assertRaises(ValueError):
                    native.enqueue(self.config, 'continue', rpc, guard.ROOT, 't', lambda: None)
                self.assertFalse(any(name == 'thread/queue/start' for name, _ in self.calls))

    def test_start_proxy_missing_after_enqueue_preserves_owned_queue(self):
        def rpc(name, params):
            if name == 'thread/queue/start':
                raise FileNotFoundError('proxy unavailable after enqueue')
            return self.rpc(name, params)
        with self.assertRaises(FileNotFoundError):
            native.enqueue(self.config, 'continue', rpc, guard.ROOT, 't', lambda: None)
        self.assertEqual(self.queue[0]['clientUserMessageId'], native.queue_client_id(self.config, 't'))

    def test_unknown_delivery_does_not_try_an_alternate_transport(self):
        def rpc(name, params):
            if name == 'thread/queue/add':
                raise TimeoutError('unknown')
            return self.rpc(name, params)
        with self.assertRaises(TimeoutError):
            native.enqueue(self.config, 'continue', rpc, guard.ROOT, 't', lambda: None)
        self.assertFalse(any(name == 'thread/queue/start' for name, _ in self.calls))

    def test_stop_deletes_only_own_persisted_queue_intent(self):
        own = native.queue_client_id(self.config, 't')
        calls = []
        rows = [{'id': 'human', 'clientUserMessageId': 'human'},
                {'id': 'own', 'clientUserMessageId': own}]
        def rpc(name, params):
            calls.append((name, params))
            if name == 'thread/queue/list':
                return {'data': copy.deepcopy(rows)}
            if name == 'thread/queue/delete':
                rows[:] = [row for row in rows if row['id'] != params['queuedSubmissionId']]
            return {}
        native.cancel_queued(self.config, 't', rpc)
        self.assertIn(('thread/queue/delete', {'threadId': THREAD, 'queuedSubmissionId': 'own'}), calls)
        self.assertEqual(sum(name == 'thread/queue/delete' for name, _ in calls), 1)
        calls.clear()
        native.cancel_queued(self.config, None, rpc)
        self.assertEqual(calls, [])

    def terminal_fixture(self, runtime, goal_status='complete', unknown=False):
        """start回执未知后的精确队列形态，包含真人及其他Goal的受保护输入。"""
        config = dict(self.config, rollout='isolated', project=str(guard.ROOT))
        (runtime / 'config.json').write_text(json.dumps(config), encoding='utf-8')
        (runtime / 'state.json').write_text(json.dumps({'dispatch_key': 't', 'dispatch_at': 1}), encoding='utf-8')
        own = native.queue_client_id(config, 't')
        other = native.queue_client_id(dict(config, goal_id='other-goal'), 't')
        rows = [{'id': 'human', 'clientUserMessageId': 'human'},
                {'id': 'other', 'clientUserMessageId': other},
                {'id': 'own', 'clientUserMessageId': own}]
        calls = []
        def rpc(name, params):
            calls.append((name, params))
            if name == 'thread/queue/list':
                return {'data': copy.deepcopy(rows)}
            if name == 'thread/queue/delete':
                self.assertEqual(params['queuedSubmissionId'], 'own')
                ledger = json.loads((runtime / 'native-queue-cleanup.json').read_text())
                self.assertIn('own', ledger['entries'][own]['delete_intents'])
                if unknown:
                    raise TimeoutError('unknown delete')
                rows[:] = [row for row in rows if row['id'] != 'own']
                return {}
            raise AssertionError('terminal cleanup must not interrupt or change Goal: ' + name)
        return config, rows, calls, rpc

    def test_external_complete_cleans_only_own_queue_without_goal_or_turn_changes(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = Path(temporary)
            config, rows, calls, rpc = self.terminal_fixture(runtime)
            with patch.object(guard, '_BACKEND_CONFIG', {}), \
                    patch.object(guard, 'goal', return_value={'id': 'goal', 'status': 'complete'}), \
                    patch.object(guard, 'binding_valid', return_value=True), \
                    patch.object(guard, 'rpc', side_effect=rpc), \
                    patch.object(guard, 'notify', return_value=False), contextlib.redirect_stdout(io.StringIO()):
                guard.worker(runtime, os.getppid())
            self.assertEqual([row['id'] for row in rows], ['human', 'other'])
            state = json.loads((runtime / 'state.json').read_text())
            self.assertEqual(state['status'], 'complete')
            self.assertEqual(state['native_queue_cleanup']['status'], 'confirmed')
            self.assertEqual(sum(name == 'thread/queue/delete' for name, _ in calls), 1)

    def test_terminal_cleanup_unknown_does_not_replay_delete_or_claim_complete(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = Path(temporary)
            config, rows, calls, rpc = self.terminal_fixture(runtime, unknown=True)
            with patch.object(guard, '_BACKEND_CONFIG', {}), \
                    patch.object(guard, 'goal', return_value={'id': 'goal', 'status': 'complete'}), \
                    patch.object(guard, 'binding_valid', return_value=True), \
                    patch.object(guard, 'rpc', side_effect=rpc), \
                    patch.object(guard, 'notify', return_value=False), contextlib.redirect_stdout(io.StringIO()):
                guard.worker(runtime, os.getppid())
                guard.worker(runtime, os.getppid())
                state = json.loads((runtime / 'state.json').read_text())
                self.assertEqual(state['status'], 'cleanup_pending')
                self.assertEqual(state['reason'], 'native_queue_cleanup_unconfirmed')
                self.assertEqual(sum(name == 'thread/queue/delete' for name, _ in calls), 1)
                # 正式列表确认该项已被消费/删除后，只读确认可以结束未知状态。
                rows[:] = [row for row in rows if row['id'] != 'own']
                guard.worker(runtime, os.getppid())
                self.assertEqual(json.loads((runtime / 'state.json').read_text())['status'], 'complete')
            self.assertEqual([row['id'] for row in rows], ['human', 'other'])

    def test_finish_uses_same_cleanup_without_ever_completing_active_business_goal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            runtime = root / '.agents/project/guards' / THREAD
            runtime.mkdir(parents=True)
            config, rows, calls, rpc = self.terminal_fixture(runtime)
            current = {'id': 'goal', 'status': 'active'}
            args = ['guard.py', 'finish', '--backend', 'codex', '--thread', THREAD]
            with patch.object(guard, 'ROOT', root), patch.object(guard, '_BACKEND_CONFIG', {}), \
                    patch.object(guard.sys, 'argv', args), \
                    patch.object(guard, 'goal', side_effect=lambda _config: current), \
                    patch.object(guard, 'binding_valid', return_value=True), \
                    patch.object(guard, 'rpc', side_effect=rpc), \
                    patch.object(guard, 'publish_registry'), contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaisesRegex(SystemExit, '尚未完成'):
                    guard.main()
                self.assertFalse(calls)
                current['status'] = 'complete'
                guard.main()
            self.assertEqual([row['id'] for row in rows], ['human', 'other'])
            self.assertEqual(json.loads((runtime / 'state.json').read_text())['status'], 'complete')

    def test_already_consumed_queue_is_read_only_and_terminal_pause_is_preserved(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = Path(temporary)
            config, rows, calls, rpc = self.terminal_fixture(runtime)
            rows[:] = [row for row in rows if row['id'] != 'own']
            with patch.object(guard, '_BACKEND_CONFIG', {}), \
                    patch.object(guard, 'goal', return_value={'id': 'goal', 'status': 'paused'}), \
                    patch.object(guard, 'binding_valid', return_value=True), \
                    patch.object(guard, 'rpc', side_effect=rpc), \
                    patch.object(guard, 'notify', return_value=False), contextlib.redirect_stdout(io.StringIO()):
                guard.worker(runtime, os.getppid())
            self.assertEqual(json.loads((runtime / 'state.json').read_text())['status'], 'paused')
            self.assertFalse(any(name == 'thread/queue/delete' for name, _ in calls))

    def test_finish_unknown_cleanup_never_reports_complete_or_replays_delete(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            runtime = root / '.agents/project/guards' / THREAD
            runtime.mkdir(parents=True)
            config, rows, calls, rpc = self.terminal_fixture(runtime, unknown=True)
            with patch.object(guard, 'ROOT', root), patch.object(guard, '_BACKEND_CONFIG', {}), \
                    patch.object(guard.sys, 'argv', ['guard.py', 'finish', '--backend', 'codex', '--thread', THREAD]), \
                    patch.object(guard, 'goal', return_value={'id': 'goal', 'status': 'complete'}), \
                    patch.object(guard, 'binding_valid', return_value=True), \
                    patch.object(guard, 'rpc', side_effect=rpc), \
                    patch.object(guard, 'publish_registry'), contextlib.redirect_stdout(io.StringIO()):
                for _ in range(2):
                    with self.assertRaisesRegex(SystemExit, '清理结果未知'):
                        guard.main()
            self.assertEqual(json.loads((runtime / 'state.json').read_text())['status'], 'cleanup_pending')
            self.assertEqual(sum(name == 'thread/queue/delete' for name, _ in calls), 1)
            self.assertEqual([row['id'] for row in rows], ['human', 'other', 'own'])

    def test_cleanup_success_does_not_hide_goal_change_during_roundtrip(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = Path(temporary)
            config, rows, calls, rpc = self.terminal_fixture(runtime)
            with patch.object(guard, '_BACKEND_CONFIG', {}), \
                    patch.object(guard, 'goal', side_effect=[{'id': 'goal', 'status': 'complete'},
                                                            {'id': 'new-goal', 'status': 'active'}]), \
                    patch.object(guard, 'binding_valid', return_value=True), \
                    patch.object(guard, 'rpc', side_effect=rpc), \
                    patch.object(guard, 'notify', return_value=False), contextlib.redirect_stdout(io.StringIO()):
                guard.worker(runtime, os.getppid())
            state = json.loads((runtime / 'state.json').read_text())
            self.assertEqual(state['status'], 'cleanup_pending')
            self.assertEqual(state['reason'], 'native_goal_terminal_unconfirmed')
            self.assertEqual([row['id'] for row in rows], ['human', 'other'])

    def test_different_goal_cleanup_preserves_previous_unknown_intent(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = Path(temporary)
            config, rows, calls, rpc = self.terminal_fixture(runtime, unknown=True)
            state = {'dispatch_key': 't'}
            with patch.object(guard, 'binding_valid', return_value=True), \
                    patch.object(guard, 'rpc', side_effect=rpc):
                self.assertFalse(guard.cleanup_native_queue(runtime, config, state))
                # 新Goal没有自己的待清项，旧Goal未知delete证据不能被覆盖或重放。
                new = dict(config, goal_id='new-goal')
                self.assertTrue(guard.cleanup_native_queue(runtime, new, {'dispatch_key': 't'}))
            ledger = json.loads((runtime / 'native-queue-cleanup.json').read_text())['entries']
            self.assertEqual(ledger[native.queue_client_id(config, 't')]['status'], 'unknown')
            self.assertEqual(ledger[native.queue_client_id(new, 't')]['status'], 'confirmed')
            self.assertEqual(sum(name == 'thread/queue/delete' for name, _ in calls), 1)

    def test_other_guard_terminal_exits_cancel_only_self_without_completing_goal(self):
        for terminal in ('stopped', 'parent_lost', 'quota_wait_expired'):
            with self.subTest(terminal=terminal), tempfile.TemporaryDirectory() as temporary:
                runtime = Path(temporary)
                config, rows, calls, rpc = self.terminal_fixture(runtime)
                if terminal == 'stopped':
                    (runtime / 'disabled').touch()
                with patch.object(guard, '_BACKEND_CONFIG', {}), \
                        patch.object(guard, 'goal', return_value={'id': 'goal', 'status': 'active'}), \
                        patch.object(guard, 'binding_valid', return_value=True), \
                        patch.object(guard, 'inspect', return_value={'Status': 'idle'}), \
                        patch.object(guard, 'quota_wait_expired', return_value=terminal == 'quota_wait_expired'), \
                        patch.object(guard, 'rpc', side_effect=rpc), \
                        patch.object(guard, 'notify', return_value=False), contextlib.redirect_stdout(io.StringIO()):
                    guard.worker(runtime, -1 if terminal == 'parent_lost' else os.getppid())
                self.assertEqual(json.loads((runtime / 'state.json').read_text())['status'], terminal)
                self.assertEqual([row['id'] for row in rows], ['human', 'other'])

    def test_interrupt_requires_exact_active_turn(self):
        self.thread.update(status={'type': 'active', 'activeFlags': []},
                           turns=[{'id': 'active', 'status': 'inProgress'}])
        native.interrupt(self.config, guard.ROOT, self.rpc)
        self.assertEqual(self.calls[-1], ('turn/interrupt', {'threadId': THREAD, 'turnId': 'active'}))
        self.thread['turns'].append({'id': 'other', 'status': 'inProgress'})
        with self.assertRaises(ValueError):
            native.interrupt(self.config, guard.ROOT, self.rpc)

    def test_no_native_control_watermark_means_no_automatic_budget_resume(self):
        with patch.object(guard, 'inspect', side_effect=AssertionError('must not use Paseo')):
            self.assertIsNone(guard.percentage_pause_controls(self.config))

    def test_send_and_stop_route_native_without_paseo(self):
        with patch.object(guard, 'rpc', side_effect=self.rpc), \
                patch.object(guard, 'bound_goal', return_value={'id': 'goal', 'status': 'active'}), \
                patch.object(guard, 'run_json', side_effect=AssertionError('must not use Paseo')):
            guard.send_task(self.config, 't')
            guard.stop_task(self.config)

    def test_complete_paused_goal_never_recover(self):
        snapshot = native.snapshot(self.config, guard.ROOT, self.rpc)
        for status in ('complete', 'paused'):
            self.assertEqual(guard.decision(snapshot, {'id': 'goal', 'status': status}, None,
                                            self.config, 0), status)

    def test_worker_unknown_send_is_persisted_and_not_retried(self):
        """原生发送异常后即使重启循环且仍空闲，也不能重复队列写入。"""
        self.assert_worker_send_intent_preserved(TimeoutError('unknown'))

    def test_worker_native_proxy_missing_preserves_dispatch_key(self):
        self.assert_worker_send_intent_preserved(FileNotFoundError('proxy missing after enqueue'))

    def assert_worker_send_intent_preserved(self, error):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = Path(temporary)
            config = dict(self.config, rollout='isolated')
            (runtime / 'config.json').write_text(json.dumps(config), encoding='utf-8')
            (runtime / 'state.json').write_text(json.dumps({'pending_key': 't', 'retry_at': 0}), encoding='utf-8')
            turn = {'kind': 'task_complete', 'key': 't', 'payload': {'completed_at': 0}, 'tools': 0}
            snapshots = native.snapshot(config, guard.ROOT, self.rpc)
            sleeps = []
            def sleep(_runtime, _seconds):
                sleeps.append(True)
                if len(sleeps) == 2:
                    (runtime / 'disabled').touch()
            with patch.object(guard, '_BACKEND_CONFIG', {}), \
                    patch.object(guard, 'goal', return_value={'id': 'goal', 'status': 'active'}), \
                    patch.object(guard, 'inspect', return_value=snapshots), \
                    patch.object(guard, 'binding_valid', return_value=True), \
                    patch.object(guard, 'bound_goal', return_value={'status': 'active'}), \
                    patch.object(guard, 'timeline', return_value=turn), \
                    patch.object(guard, 'rpc', return_value={}), \
                    patch.object(guard, 'quota_delay', return_value=0), \
                    patch.object(guard, 'send_task', side_effect=error) as send, \
                    patch.object(guard, 'sleep_until', side_effect=sleep), \
                    patch.object(guard, 'notify', return_value=False), \
                    patch.object(guard.questions, 'step', side_effect=AssertionError('no Paseo questions')):
                guard.worker(runtime, os.getppid())
            self.assertEqual(send.call_count, 1)
            state = json.loads((runtime / 'state.json').read_text(encoding='utf-8'))
            self.assertEqual(state['dispatch_key'], 't')
            self.assertIn('dispatch_at', state)


if __name__ == '__main__':
    unittest.main()
