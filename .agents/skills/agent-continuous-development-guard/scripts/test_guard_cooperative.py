"""仅隔离临时目录与注入工具调用，验证合作式队列边界。"""
import os
from pathlib import Path
import subprocess
import tempfile
import sqlite3
import threading
import unittest
from unittest.mock import patch

import guard_cooperative as guard

THREAD = '11111111-1111-4111-8111-111111111111'


class CooperativeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()
        self.path = self.root / '.agents/project/guards' / THREAD
        self.path.mkdir(parents=True)
        self.env = patch.dict(os.environ, CODEX_THREAD_ID=THREAD, CODEX_HOME=str(self.root))
        self.env.start()
        self.root_patch = patch.object(guard.heartbeat, 'ROOT', self.root)
        self.root_patch.start()
        self.binding = {'thread': THREAD, 'project': str(self.root)}
        self.goal = {'id': 'original', 'objective_sha256': 'unchanged'}
        self.ledger = {'version': 1, 'binding': self.binding, 'goal': self.goal,
                       'fraction': '0.8', 'allow_existing_credits': False,
                       'account_budgets': {'account': {'reserve': 20}},
                       'usage_status': 'usage_budget_available',
                       'last_check': {'at': 100, 'action': 'continue'}}
        self.native = self.path / 'native-heartbeat.json'
        guard.heartbeat.atomic(self.native, self.ledger)
        self.identity = patch.object(guard.heartbeat, 'identity',
                                     return_value=(self.binding, self.goal, 'active'))
        self.identity.start()
        self.state = {'run_id': 'run', 'binding': self.binding, 'goal': self.goal,
                      'status': 'running', 'interval': 300, 'next_at': 400,
                      'codex': '/existing/codex', 'pending': None}

    def tearDown(self):
        self.identity.stop()
        self.root_patch.stop()
        self.env.stop()
        self.temp.cleanup()

    def tick(self, now=400, status='active', ledger=None):
        return guard.tick(self.state, self.binding, self.goal, status,
                          self.ledger if ledger is None else ledger, now)

    def test_wrong_caller_writes_nothing(self):
        with patch.dict(os.environ, CODEX_THREAD_ID='other'):
            before = sorted(self.path.iterdir())
            with self.assertRaisesRegex(ValueError, 'caller_mismatch'):
                guard.start(THREAD, '/does/not/exist')
            self.assertEqual(before, sorted(self.path.iterdir()))

    def test_binding_goal_and_budget_rejected_without_native_write(self):
        before = self.native.read_bytes()
        for binding, goal in [({}, self.goal), (self.binding, {'id': 'new'})]:
            with patch.object(guard.heartbeat, 'identity', return_value=(binding, goal, 'active')):
                with self.assertRaises(ValueError):
                    guard.evidence(THREAD)
        self.assertEqual(before, self.native.read_bytes())

    def test_interval_and_cache_expiration(self):
        self.assertIsNone(self.tick(399))
        self.assertIsNotNone(self.tick(400))
        self.assertEqual(self.state['next_at'], 700)
        self.state['pending'] = None
        self.assertIsNone(self.tick(700))  # 旧资格不续期、不换时间。
        self.assertEqual(self.ledger['last_check']['at'], 100)

    def test_pending_unknown_never_retries_and_expires(self):
        self.assertIsNotNone(self.tick())
        self.state['pending']['phase'] = 'unknown'
        self.assertIsNone(self.tick(401))
        self.assertIsNone(self.tick(4000))
        self.assertEqual(self.state['status'], 'stopped')
        self.assertTrue(self.state['cleanup_pending'])

    def test_unknown_budget_one_hour_timeout(self):
        ledger = dict(self.ledger, usage_status='unknown')
        self.tick(400, ledger=ledger)
        self.tick(3999, ledger=ledger)
        self.assertEqual(self.state['status'], 'running')
        self.tick(4000, ledger=ledger)
        self.assertEqual(self.state['reason'], 'wait_expired')

    def test_pause_complete_and_changed_goal_exit(self):
        for status in ['paused', 'complete']:
            self.state['status'] = 'running'
            self.tick(status=status)
            self.assertEqual(self.state['status'], 'stopped')
        self.state['status'] = 'running'
        guard.tick(self.state, self.binding, {'id': 'new'}, 'active', self.ledger, 400)
        self.assertEqual(self.state['reason'], 'binding_or_goal_changed')

    def save(self):
        guard.heartbeat.atomic(self.path / 'cooperative.json', self.state)

    def test_ack_requires_exact_pending_and_preserves_budget(self):
        identifier = self.tick()
        self.save()
        before = self.native.read_bytes()
        with self.assertRaisesRegex(ValueError, 'ack_rejected'):
            guard.control(THREAD, 'ack', 'wrong-run-heartbeat')
        result = guard.control(THREAD, 'ack', identifier)
        self.assertIsNone(result['pending'])
        self.assertEqual(result['last_ack']['run_id'], 'run')
        self.assertEqual(before, self.native.read_bytes())
        with self.assertRaises(ValueError):
            guard.control(THREAD, 'ack', identifier)

    def test_stopped_pending_is_explicit_and_old_ack_refused(self):
        identifier = self.tick()
        self.save()
        result = guard.control(THREAD, 'stop')
        self.assertTrue(result['cleanup_pending'])
        with self.assertRaisesRegex(ValueError, 'ack_rejected'):
            guard.control(THREAD, 'ack', identifier)

    def test_worker_lock_excludes_second_owner(self):
        with guard.locked(self.path, 'cooperative-worker.lock', False):
            with self.assertRaises(BlockingIOError):
                with guard.locked(self.path, 'cooperative-worker.lock', False):
                    self.fail('double worker')

    def test_start_idempotent_one_background_python(self):
        executable = self.root / 'codex'
        executable.write_text('fake')
        executable.chmod(0o700)
        calls = []
        runner = lambda *a, **k: subprocess.CompletedProcess(a[0], 0,
            'Usage: codex queue [OPTIONS] --thread <THREAD> --message <TEXT>\n'
            '  --resume\n'
            'After queueing, try to resume the thread on the local daemon or selected --remote server.\n'
            'A failed resume produces a warning and the command still succeeds', '')
        first = guard.start(THREAD, str(executable), runner=runner,
                            spawn=lambda *a, **k: calls.append((a, k)))
        self.assertEqual(first['interval'], 1800)
        second = guard.start(THREAD, str(executable), runner=runner,
                             spawn=lambda *a, **k: calls.append((a, k)))
        self.assertEqual(first['run_id'], second['run_id'])
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][0][0][1], '-B')
        self.assertNotIn('--resume', calls[0][0][0])

    def test_old_cli_without_explicit_resume_capability_refused(self):
        executable = self.root / 'codex'
        executable.write_text('fake')
        executable.chmod(0o700)
        runner = lambda *a, **k: subprocess.CompletedProcess(a[0], 0,
            'Usage: codex queue [OPTIONS] --thread <THREAD> --message <TEXT>', '')
        with self.assertRaisesRegex(ValueError, 'queue_only_contract_unverified'):
            guard.validate_cli(str(executable), runner)

    def test_receipt_requires_exact_success_and_target(self):
        queue_id = '22222222-2222-4222-8222-222222222222'
        valid = f'Queued message {queue_id} for thread {THREAD}.\n'
        self.assertEqual(guard.receipt(subprocess.CompletedProcess([], 0, valid), THREAD), queue_id)
        for code, text in [(0, 'success'), (1, valid), (0, valid + 'unexpected'),
                           (0, valid.replace(THREAD, queue_id))]:
            self.assertIsNone(guard.receipt(subprocess.CompletedProcess([], code, text), THREAD))

    def test_status_readonly_pending_recovery_requires_exact_absence(self):
        identifier = self.tick()
        queue_id = '22222222-2222-4222-8222-222222222222'
        self.state['pending']['queue_id'] = queue_id
        self.state.update(status='stopped', cleanup_pending=True)
        self.save()
        with sqlite3.connect(self.root / 'queue_1.sqlite') as db:
            db.execute('CREATE TABLE queued_items (id TEXT, thread_id TEXT)')
            db.execute('INSERT INTO queued_items VALUES (?,?)', (queue_id, THREAD))
        before = (self.path / 'cooperative.json').read_bytes()
        self.assertFalse(guard.control(THREAD, 'status')['queue_absent'])
        self.assertEqual(before, (self.path / 'cooperative.json').read_bytes())
        with sqlite3.connect(self.root / 'queue_1.sqlite') as db:
            db.execute('DELETE FROM queued_items WHERE id=?', (queue_id,))
        self.assertTrue(guard.control(THREAD, 'status')['queue_absent'])
        self.assertEqual(before, (self.path / 'cooperative.json').read_bytes())
        with self.assertRaises(ValueError):
            guard.control(THREAD, 'ack', identifier)
        self.state['pending'].pop('queue_id')
        self.assertIsNone(guard.queue_absent(self.state))

    def test_child_waits_for_start_lock_and_rechecks_run(self):
        executable = self.root / 'codex'
        executable.write_text('fake')
        executable.chmod(0o700)
        errors, workers = [], []
        attempted = threading.Event()
        def spawn(argv, **_):
            def child():
                attempted.set()
                try:
                    guard.worker(THREAD, argv[-1], sleep=lambda _: guard.control(THREAD, 'stop'))
                except BaseException as error:
                    errors.append(error)
            child_thread = threading.Thread(target=child)
            workers.append(child_thread)
            child_thread.start()
            self.assertTrue(attempted.wait(1))
            child_thread.join(.03)
            self.assertTrue(child_thread.is_alive(), 'child must wait, not fail startup lock')
        with patch.object(guard, 'validate_cli', return_value=str(executable)):
            guard.start(THREAD, str(executable), spawn=spawn)
        workers[0].join(2)
        self.assertFalse(workers[0].is_alive())
        self.assertEqual(errors, [])
        self.assertEqual(guard.control(THREAD, 'status')['status'], 'stopped')

    def test_restart_only_after_known_queue_disappears(self):
        queue_id = '22222222-2222-4222-8222-222222222222'
        self.tick()
        self.state.update(status='stopped', cleanup_pending=True)
        self.save()
        spawned = []
        with patch.object(guard, 'validate_cli', return_value='/existing/codex'):
            result = guard.start(THREAD, '/existing/codex', spawn=lambda *a, **k: spawned.append(a))
            self.assertEqual(result['run_id'], 'run')  # unknown无ID不能重启。
            self.assertEqual(spawned, [])
            self.state['pending']['queue_id'] = queue_id
            self.save()
            with sqlite3.connect(self.root / 'queue_1.sqlite') as db:
                db.execute('CREATE TABLE queued_items (id TEXT, thread_id TEXT)')
                db.execute('INSERT INTO queued_items VALUES (?,?)', (queue_id, THREAD))
            guard.start(THREAD, '/existing/codex', spawn=lambda *a, **k: spawned.append(a))
            self.assertEqual(spawned, [])
            with sqlite3.connect(self.root / 'queue_1.sqlite') as db:
                db.execute('DELETE FROM queued_items WHERE id=?', (queue_id,))
            result = guard.start(THREAD, '/existing/codex', spawn=lambda *a, **k: spawned.append(a))
            self.assertNotEqual(result['run_id'], 'run')
            self.assertEqual(len(spawned), 1)
            self.assertEqual(result['previous_queue_absent']['queue_id'], queue_id)
            self.assertIsNone(result['pending'])

    def test_running_queue_absent_does_not_clear_pending(self):
        identifier = self.tick()
        self.state['pending']['queue_id'] = '22222222-2222-4222-8222-222222222222'
        self.save()
        with sqlite3.connect(self.root / 'queue_1.sqlite') as db:
            db.execute('CREATE TABLE queued_items (id TEXT, thread_id TEXT)')
        before = (self.path / 'cooperative.json').read_bytes()
        with patch.object(guard, 'validate_cli', return_value='/existing/codex'):
            result = guard.start(THREAD, '/existing/codex', spawn=lambda *a, **k: self.fail('new run'))
        self.assertEqual(result['pending']['heartbeat_id'], identifier)
        self.assertEqual(before, (self.path / 'cooperative.json').read_bytes())
        self.assertIsNone(self.tick(701))
        self.assertEqual(self.state['pending']['heartbeat_id'], identifier)

    def test_queue_id_other_thread_is_unknown_not_absent(self):
        self.tick()
        queue_id = '22222222-2222-4222-8222-222222222222'
        self.state['pending']['queue_id'] = queue_id
        with sqlite3.connect(self.root / 'queue_1.sqlite') as db:
            db.execute('CREATE TABLE queued_items (id TEXT, thread_id TEXT)')
            db.execute('INSERT INTO queued_items VALUES (?,?)', (queue_id, 'other-thread'))
        self.assertIsNone(guard.queue_absent(self.state))

    def test_stopped_worker_lock_blocks_pending_cleanup(self):
        self.tick()
        self.state.update(status='stopped', cleanup_pending=True)
        self.state['pending']['queue_id'] = '22222222-2222-4222-8222-222222222222'
        self.save()
        before = (self.path / 'cooperative.json').read_bytes()
        with patch.object(guard, 'validate_cli', return_value='/existing/codex'), \
             patch.object(guard, 'queue_absent', return_value=True), \
             guard.locked(self.path, 'cooperative-worker.lock', False):
            with self.assertRaises(BlockingIOError):
                guard.start(THREAD, '/existing/codex')
        self.assertEqual(before, (self.path / 'cooperative.json').read_bytes())

    def test_stderr_marks_unknown_but_keeps_exact_queue_id(self):
        queue_id = '22222222-2222-4222-8222-222222222222'
        self.state['next_at'] = 100
        self.save()
        def runner(argv, **kwargs):
            return subprocess.CompletedProcess(argv, 0,
                f'Queued message {queue_id} for thread {THREAD}.', 'warning')
        with patch.object(guard.time, 'time', return_value=100):
            guard.worker(THREAD, 'run', runner=runner,
                         sleep=lambda _: guard.control(THREAD, 'stop'))
        state = guard.heartbeat.read_json(self.path / 'cooperative.json')
        self.assertEqual(state['pending']['phase'], 'unknown')
        self.assertEqual(state['pending']['queue_id'], queue_id)
        self.assertTrue(state['cleanup_pending'])

    def test_wrong_caller_stop_and_status_no_write(self):
        self.save()
        before = (self.path / 'cooperative.json').read_bytes()
        with patch.dict(os.environ, CODEX_THREAD_ID='other'):
            for action in ['stop', 'status']:
                with self.assertRaises(ValueError):
                    guard.control(THREAD, action)
        self.assertEqual(before, (self.path / 'cooperative.json').read_bytes())

    def test_worker_unknown_send_once_argv_and_stop(self):
        self.state['next_at'] = 100
        self.save()
        calls = []
        def runner(argv, **kwargs):
            calls.append((argv, kwargs))
            raise subprocess.TimeoutExpired(argv, 4)
        count = 0
        def sleep(_):
            nonlocal count
            count += 1
            if count == 2:
                guard.control(THREAD, 'stop')
        with patch.object(guard.time, 'time', return_value=100):
            guard.worker(THREAD, 'run', runner=runner, sleep=sleep)
        self.assertEqual(len(calls), 1)
        argv = calls[0][0]
        self.assertEqual(argv[:5], ['/existing/codex', 'queue', '--thread', THREAD, '--message'])
        self.assertFalse(set(argv) & {'--resume', 'model', 'daemon'})
        self.assertIn('source=codex_cli_queue', argv[-1])
        self.assertIn('checkpoint', argv[-1])
        self.assertLessEqual(calls[0][1]['timeout'], 4)
        result = guard.heartbeat.read_json(self.path / 'cooperative.json')
        self.assertEqual(result['pending']['phase'], 'unknown')
        self.assertTrue(result['cleanup_pending'])


if __name__ == '__main__':
    unittest.main()
