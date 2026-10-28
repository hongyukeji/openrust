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

SCRIPT = Path(__file__).with_name('controller-recovery.py')
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
        limits['secondary'].pop('resetsAt')
        self.assertEqual(recovery.quota_delay({'ordinaryUsageAllowed': True, 'rateLimits': limits}, 100), 300)

    def test_new_input_invalidates_old_error(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rollout'
            path.write_text(json.dumps({'type': 'event_msg', 'timestamp': 'a',
                                        'payload': {'type': 'task_complete', 'error': {'message': '500'}}}) + '\n' +
                            json.dumps({'type': 'event_msg', 'timestamp': 'b', 'payload': {'type': 'user_message'}}) + '\n')
            self.assertEqual(recovery.timeline(path)['kind'], 'user_message')

    def test_ambiguous_dispatch_is_not_repeated(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder)
            (runtime / 'config.json').write_text(json.dumps({'agent': 'a', 'thread': 't', 'rollout': 'r', 'goal_id': 'g'}))
            (runtime / 'state.json').write_text(json.dumps({'dispatch_key': 't', 'attempts': 3}))
            turn = dict(self.turn, payload={'error': {'message': '500'}})
            with patch.object(recovery, 'binding_valid', return_value=True), patch.object(recovery, 'inspect', return_value=self.agent), patch.object(recovery, 'goal', return_value=self.goal), patch.object(recovery, 'timeline', return_value=turn), patch.object(recovery, 'notify', return_value=True), patch.object(recovery, 'run_json') as send, patch.object(recovery, 'sleep_until', side_effect=lambda *_: (runtime / 'disabled').touch()):
                recovery.worker(runtime, os.getppid())
                send.assert_not_called()

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
        self.script = self.root / '.agents/project/scripts/controller-recovery.py'
        self.script.parent.mkdir(parents=True)
        shutil.copyfile(SCRIPT, self.script)
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
            db.execute('CREATE TABLE thread_goals(thread_id,goal_id,status)')
            db.execute('INSERT INTO thread_goals VALUES(?,?,?)', (self.thread, 'goal', 'active'))
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
        self.runtime = self.root / 'storage/controller-recovery' / self.agent

    def call(self, command, *args, check=True):
        return subprocess.run([sys.executable, '-B', str(self.script), command, *args],
                              env=self.env, capture_output=True, text=True, timeout=55, check=check)

    def wait_for(self, predicate, timeout=20):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if predicate(): return
            time.sleep(.1)
        self.fail('未在期限内达到预期进程状态')

    def tearDown(self):
        if self.runtime.exists():
            (self.runtime / 'disabled').touch()
            self.wait_for(lambda: not (self.runtime / 'supervisor.json').exists())
        self.temp.cleanup()

    def test_binding_mismatch_cannot_enrol(self):
        value = json.loads(self.record.read_text())
        value['persistence']['sessionId'] = 'other'
        self.record.write_text(json.dumps(value))
        self.assertNotEqual(self.call('start', check=False).returncode, 0)
        self.assertFalse((self.runtime / 'supervisor.json').exists())

    def test_start_is_singleton_stop_cleans_processes_and_pid(self):
        first = json.loads(self.call('start').stdout)
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

    def test_completed_goal_automatically_cleans_guard(self):
        self.call('start')
        self.wait_for(lambda: (self.runtime / 'worker.json').exists())
        with sqlite3.connect(self.home / 'goals_1.sqlite') as db:
            db.execute("UPDATE thread_goals SET status='complete'")
        self.wait_for(lambda: not (self.runtime / 'supervisor.json').exists())
        self.assertFalse((self.runtime / 'worker.json').exists())
        self.assertEqual(json.loads((self.runtime / 'state.json').read_text())['status'], 'complete')

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
        start = subprocess.Popen(command + ['start'], env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        stop = None
        try:
            self.wait_for(lambda: (self.root / 'inspect-entered').exists())
            stop = subprocess.Popen(command + ['stop'], env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
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


if __name__ == '__main__':
    unittest.main()
