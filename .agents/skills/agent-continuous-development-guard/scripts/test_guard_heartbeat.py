"""只用临时项目、rollout与只读数据库验证原生 heartbeat 本地门禁。"""
import copy
import hashlib
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest import mock
import uuid

import guard_heartbeat as heartbeat


def usage(account='account-a', limit='codex', used=10, reset=2000):
    return {'accountId': account, 'ordinaryUsageAllowed': True,
            'rateLimits': {'limitId': limit,
                           'primary': {'usedPercent': used, 'windowDurationMins': 60, 'resetsAt': reset},
                           'credits': {'hasCredits': False, 'unlimited': False, 'balance': '0'},
                           'spendControlReached': False, 'rateLimitReachedType': None}}


class HeartbeatTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='native-heartbeat-', dir='/tmp')
        self.root = (Path(self.temporary.name) / 'project').resolve()
        self.home = (Path(self.temporary.name) / 'codex-home').resolve()
        self.root.mkdir()
        self.home.mkdir()
        self.thread = str(uuid.uuid4())
        self.goal_id = str(uuid.uuid4())
        self.rollout = self.home / 'rollout.jsonl'
        self.metadata = {'id': self.thread, 'cwd': str(self.root), 'source': 'cli'}
        self.write_metadata()
        with closing(sqlite3.connect(self.home / 'state_5.sqlite')) as db, db:
            db.execute('CREATE TABLE threads (id TEXT, rollout_path TEXT, cwd TEXT)')
            db.execute('INSERT INTO threads VALUES (?,?,?)', (self.thread, str(self.rollout), str(self.root)))
        with closing(sqlite3.connect(self.home / 'goals_1.sqlite')) as db, db:
            db.execute('CREATE TABLE thread_goals (thread_id TEXT,goal_id TEXT,status TEXT,created_at_ms INTEGER,objective TEXT)')
            db.execute('INSERT INTO thread_goals VALUES (?,?,?,?,?)',
                       (self.thread, self.goal_id, 'active', 100, '私有目标内容与凭据不输出'))
        self.snapshot = usage()
        self.usage_file = self.root / 'usage.json'
        self.write_usage()
        self.root_patch = mock.patch.object(heartbeat, 'ROOT', self.root)
        self.env_patch = mock.patch.dict(os.environ, {'CODEX_HOME': str(self.home)})
        self.process_patch = mock.patch('subprocess.Popen', side_effect=AssertionError('不能启动进程'))
        self.root_patch.start()
        self.env_patch.start()
        self.process_patch.start()
        self.ledger_path = self.root / '.agents/project/guards' / self.thread / 'native-heartbeat.json'

    def tearDown(self):
        self.process_patch.stop()
        self.env_patch.stop()
        self.root_patch.stop()
        self.temporary.cleanup()

    def write_metadata(self):
        self.rollout.write_text(json.dumps({'type': 'session_meta', 'payload': self.metadata}) + '\n')

    def write_usage(self, now=1000):
        self.usage_file.write_text(json.dumps({'source': 'codex_app.get_usage_limits',
                                              'thread': self.thread, 'observed_at': now,
                                              'payload': self.snapshot}))

    def run_gate(self, command='check', now=1000, thread=None, automation_id=None):
        self.write_usage(now)
        if thread:
            envelope = json.loads(self.usage_file.read_text())
            envelope['thread'] = thread
            self.usage_file.write_text(json.dumps(envelope))
        return heartbeat.run(thread or self.thread, command, self.usage_file, automation_id, now)

    def init(self):
        return self.run_gate('init')

    def bind(self, automation_id='automation-1'):
        return self.run_gate('bind', automation_id=automation_id)

    def prepared(self):
        self.assertEqual(self.init()['action'], 'continue')
        self.assertEqual(self.bind()['action'], 'continue')

    def ledger(self):
        return json.loads(self.ledger_path.read_text())

    def update_goal(self, **fields):
        with closing(sqlite3.connect(self.home / 'goals_1.sqlite')) as db, db:
            db.execute('UPDATE thread_goals SET ' + ','.join(key + '=?' for key in fields)
                       + ' WHERE thread_id=?', (*fields.values(), self.thread))

    def paused_then_active(self):
        self.prepared()
        self.update_goal(status='paused')
        self.assertEqual(self.run_gate()['reason'], 'goal_paused')
        self.update_goal(status='active')

    def resume_gate(self, authorization='真人明确继续原任务', now=1000):
        self.write_usage(now)
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            return heartbeat.run(self.thread, 'resume', self.usage_file, now=now,
                                 user_authorization=authorization)

    def test_explicit_resume_preserves_budget_and_goal_then_checkpoint_continues(self):
        self.paused_then_active()
        before = self.ledger()
        self.assertEqual(self.resume_gate()['action'], 'continue')
        after = self.ledger()
        for key in ('binding', 'goal', 'account_budgets', 'automation_id', 'fraction', 'allow_existing_credits'):
            self.assertEqual(after[key], before[key])
        self.assertNotIn('terminal_action', after)
        self.assertEqual(after['explicit_resumptions'][0]['previous_account_budgets'], before['account_budgets'])
        self.assertNotIn('真人明确', self.ledger_path.read_text())
        self.assertEqual(self.run_gate()['action'], 'continue')

    def test_resume_requires_user_authorization_and_original_caller(self):
        self.paused_then_active()
        before = self.ledger_path.read_bytes()
        self.assertEqual(self.resume_gate(None)['reason'], 'resume_authorization_required')
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': 'wrong'}):
            self.assertEqual(heartbeat.run(self.thread, 'resume')['reason'], 'checkpoint_caller_mismatch')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_resume_never_releases_pause_or_other_terminal_causes(self):
        self.paused_then_active()
        self.update_goal(status='paused')
        self.assertEqual(self.resume_gate()['reason'], 'goal_paused')
        self.update_goal(status='active')
        for reason in ('goal_or_binding_changed', 'binding_unknown', 'binding_changed_during_check'):
            ledger = self.ledger()
            ledger['terminal_reason'] = reason
            heartbeat.atomic(self.ledger_path, ledger)
            before = self.ledger_path.read_bytes()
            self.assertEqual(self.resume_gate()['reason'], 'resume_state_rejected')
            self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_resume_unknown_or_exhausted_usage_leaves_terminal_unchanged(self):
        self.paused_then_active()
        before = self.ledger_path.read_bytes()
        for snapshot in ({}, usage(used=99)):
            self.snapshot = snapshot
            self.assertEqual(self.resume_gate()['action'], 'wait')
            self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_resume_new_cycle_keeps_old_baseline_in_audit(self):
        self.paused_then_active()
        before = self.ledger()['account_budgets']
        self.snapshot = usage(used=0, reset=4000)
        self.assertEqual(self.resume_gate(now=2500)['action'], 'continue')
        ledger = self.ledger()
        self.assertEqual(ledger['explicit_resumptions'][0]['previous_account_budgets'], before)
        budget = next(iter(ledger['account_budgets'].values()))
        self.assertEqual(budget['windows']['primary']['baseline_remaining'], '100')
        self.assertEqual(budget['windows']['primary']['reserve'], '20.0')

    def test_resume_rechecks_identity_after_budget_check(self):
        self.paused_then_active()
        before = self.ledger_path.read_bytes()
        original = heartbeat.budget_check
        def changed(*args):
            result = original(*args)
            self.update_goal(status='paused')
            return result
        with mock.patch.object(heartbeat, 'budget_check', side_effect=changed):
            self.assertEqual(self.resume_gate()['reason'], 'resume_identity_changed')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_adjacent_reset_seconds_keep_fixed_cycle_and_raw_snapshot(self):
        self.prepared()
        before = copy.deepcopy(self.ledger()['account_budgets'])
        for reset in (2001, 1999, 2060, 1940, 2000, 2060):
            self.snapshot = usage(used=20, reset=reset)
            raw = copy.deepcopy(self.snapshot)
            self.assertEqual(self.run_gate()['action'], 'continue')
            self.assertEqual(self.snapshot, raw)
            self.assertEqual(self.ledger()['account_budgets'], before)
        self.snapshot = usage(used=20, reset=2061)
        self.assertEqual(self.run_gate()['action'], 'wait')
        self.assertEqual(self.ledger()['account_budgets'], before)

    def test_precision_never_allows_balance_increase_or_reserve_bypass(self):
        self.prepared()
        before = copy.deepcopy(self.ledger()['account_budgets'])
        self.snapshot = usage(used=0, reset=2001)
        self.assertEqual(self.run_gate()['action'], 'wait')
        self.assertEqual(self.ledger()['account_budgets'], before)
        self.snapshot = usage(used=83, reset=2001)
        self.assertEqual(self.run_gate()['reason'], 'usage_budget_reached')

    def test_precision_does_not_extend_an_expired_anchor(self):
        self.prepared()
        self.snapshot = usage(used=20, reset=2001)
        self.assertEqual(self.run_gate(now=2000)['action'], 'wait')
        self.snapshot = usage(used=0, reset=4000)
        self.assertEqual(self.run_gate(now=2500)['action'], 'continue')
        budget = next(iter(self.ledger()['account_budgets'].values()))
        self.assertEqual(budget['windows']['primary']['resets_at'], '4000')
        self.assertEqual(budget['windows']['primary']['previous_resets_at'], '2000')

    def test_precision_does_not_accept_duration_or_window_changes(self):
        self.prepared()
        for change in ('duration', 'window'):
            self.snapshot = usage(used=20, reset=2001)
            if change == 'duration':
                self.snapshot['rateLimits']['primary']['windowDurationMins'] = 120
            else:
                self.snapshot['rateLimits']['secondary'] = copy.deepcopy(self.snapshot['rateLimits']['primary'])
            self.assertEqual(self.run_gate()['action'], 'wait')

    def test_temporary_identity_io_failure_does_not_poison_ledger(self):
        self.prepared()
        before = self.ledger_path.read_bytes()
        for error in (OSError('private-path'), sqlite3.OperationalError('database is locked')):
            with mock.patch.object(heartbeat, 'identity', side_effect=error):
                result = self.run_gate()
            self.assertEqual(result['action'], 'wait')
            self.assertEqual(result['recovery'], 'refresh_official_evidence')
            self.assertNotIn('private-path', json.dumps(result))
            self.assertEqual(self.ledger_path.read_bytes(), before)
        self.assertEqual(self.run_gate()['action'], 'continue')

    def test_final_identity_io_failure_is_retryable_without_persisting_changes(self):
        self.prepared()
        before = self.ledger_path.read_bytes()
        evidence = heartbeat.identity(self.thread, self.home)
        with mock.patch.object(heartbeat, 'identity', side_effect=[evidence, sqlite3.OperationalError('busy')]):
            result = self.run_gate()
        self.assertEqual(result['action'], 'wait')
        self.assertEqual(result['recovery'], 'refresh_official_evidence')
        self.assertEqual(self.ledger_path.read_bytes(), before)
        self.assertEqual(self.run_gate()['action'], 'continue')

    def test_usage_diagnostic_selects_the_actual_recovery_action(self):
        self.prepared()
        for diagnostic, recovery in [('usage_tool_error', 'refresh_official_evidence'),
                                     ('usage_snapshot_invalid', 'refresh_official_evidence'),
                                     ('usage_tool_result_invalid', 'diagnose_guard_then_recheck')]:
            with mock.patch.object(heartbeat, 'usage_snapshot', side_effect=heartbeat.UsageSnapshotError(diagnostic)):
                result = self.run_gate()
            self.assertEqual(result['budget']['error'], diagnostic)
            self.assertEqual(result['recovery'], recovery)
        self.assertEqual(self.run_gate()['action'], 'continue')

    def test_unknown_quota_recovers_without_goal_or_budget_recreation(self):
        self.prepared()
        before = copy.deepcopy(self.ledger()['account_budgets'])
        self.snapshot = {}
        self.assertEqual(self.run_gate()['recovery'], 'diagnose_guard_then_recheck')
        self.snapshot = usage(used=20)
        self.assertEqual(self.run_gate(now=1100)['action'], 'continue')
        self.assertEqual(self.ledger()['account_budgets'], before)

    def test_blocked_goal_requests_official_resume_and_never_continues(self):
        self.prepared()
        self.update_goal(status='blocked')
        result = self.run_gate()
        self.assertEqual(result['action'], 'wait')
        self.assertEqual(result['recovery'], 'official_goal_resume_required')
        self.update_goal(status='active')
        self.assertEqual(self.run_gate()['action'], 'continue')

    def test_init_keeps_goal_digest_and_reserve_without_database_writes(self):
        originals = {path: path.read_bytes() for path in self.home.iterdir()}
        result = self.init()
        self.assertEqual(result['action'], 'continue')
        self.assertEqual((heartbeat.ROOT / '.agents/project/guards/.gitignore').read_text(), '*\n!.gitignore\n')
        ledger = self.ledger()
        self.assertEqual(ledger['goal']['id'], self.goal_id)
        self.assertNotIn('objective', ledger['goal'])
        budget = next(iter(ledger['account_budgets'].values()))
        self.assertEqual(budget['windows']['primary']['baseline_remaining'], '90')
        self.assertEqual(budget['windows']['primary']['reserve'], '18.0')
        self.assertFalse(ledger['allow_existing_credits'])
        self.assertNotIn('私有目标', json.dumps(result, ensure_ascii=False))
        self.assertEqual(originals, {path: path.read_bytes() for path in self.home.iterdir()})

    def test_same_goal_init_preserves_budget_and_automation(self):
        self.prepared()
        before = copy.deepcopy(self.ledger()['account_budgets'])
        self.snapshot['rateLimits']['primary']['usedPercent'] = 50
        self.assertEqual(self.init()['action'], 'continue')
        after = self.ledger()
        self.assertEqual(after['account_budgets'], before)
        self.assertEqual(after['automation_id'], 'automation-1')

    def test_goal_complete_finishes_and_pause_stops_without_goal_mutation(self):
        for status, action in (('paused', 'stop'), ('complete', 'finish')):
            with self.subTest(status=status):
                if self.ledger_path.exists():
                    self.ledger_path.unlink()
                self.update_goal(status='active')
                self.prepared()
                self.update_goal(status=status)
                result = self.run_gate()
                self.assertEqual(result['action'], action)
                with closing(sqlite3.connect(self.home / 'goals_1.sqlite')) as db, db:
                    self.assertEqual(db.execute('SELECT status FROM thread_goals').fetchone()[0], status)

    def test_paused_goal_never_automatically_resumes(self):
        self.prepared()
        self.update_goal(status='paused')
        self.assertEqual(self.run_gate()['action'], 'stop')
        self.update_goal(status='active')
        self.assertEqual(self.init()['action'], 'stop')
        self.assertEqual(self.run_gate()['action'], 'stop')

    def test_goal_replacement_never_reinitializes_old_automation(self):
        self.prepared()
        before = self.ledger()['goal']
        self.update_goal(goal_id=str(uuid.uuid4()), objective='新目标')
        self.assertEqual(self.init()['reason'], 'goal_or_binding_changed')
        self.assertEqual(self.ledger()['goal'], before)
        self.assertEqual(self.ledger()['automation_id'], 'automation-1')

    def test_goal_creation_or_objective_change_also_stops(self):
        for fields in ({'created_at_ms': 101}, {'objective': '不同目标'}):
            self.update_goal(created_at_ms=100, objective='私有目标内容与凭据不输出')
            if self.ledger_path.exists():
                self.ledger_path.unlink()
            self.prepared()
            self.update_goal(**fields)
            self.assertEqual(self.run_gate()['action'], 'stop')

    def test_subagent_or_other_project_binding_stops(self):
        for changed in ({'source': {'subagent': 'worker'}}, {'cwd': '/tmp'}, {'id': str(uuid.uuid4())}):
            self.metadata.update({'source': 'cli', 'cwd': str(self.root), 'id': self.thread})
            self.write_metadata()
            if self.ledger_path.exists():
                self.ledger_path.unlink()
            self.prepared()
            self.metadata.update(changed)
            self.write_metadata()
            self.assertEqual(self.run_gate()['reason'], 'binding_unknown')

    def test_missing_goal_not_created(self):
        with closing(sqlite3.connect(self.home / 'goals_1.sqlite')) as db, db:
            db.execute('DELETE FROM thread_goals')
        self.assertEqual(self.init()['action'], 'stop')
        self.assertFalse(self.ledger_path.exists())
        with closing(sqlite3.connect(self.home / 'goals_1.sqlite')) as db, db:
            self.assertEqual(db.execute('SELECT COUNT(*) FROM thread_goals').fetchone()[0], 0)

    def test_check_requires_ledger_and_automation_binding(self):
        self.assertEqual(self.run_gate()['reason'], 'ledger_missing')
        self.init()
        self.assertEqual(self.run_gate()['reason'], 'automation_unbound')

    def test_automation_id_is_idempotent_and_cannot_be_replaced(self):
        self.prepared()
        self.assertEqual(self.bind()['action'], 'continue')
        self.assertEqual(self.bind('automation-2')['reason'], 'automation_id_changed')
        self.assertEqual(self.ledger()['automation_id'], 'automation-1')

    def test_automation_id_cannot_bind_to_different_task(self):
        self.prepared()
        second = str(uuid.uuid4())
        other_rollout = self.home / 'other.jsonl'
        other_rollout.write_text(json.dumps({'payload': {'id': second, 'cwd': str(self.root), 'source': 'cli'}}) + '\n')
        with closing(sqlite3.connect(self.home / 'state_5.sqlite')) as db, db:
            db.execute('INSERT INTO threads VALUES (?,?,?)', (second, str(other_rollout), str(self.root)))
        with closing(sqlite3.connect(self.home / 'goals_1.sqlite')) as db, db:
            db.execute('INSERT INTO thread_goals VALUES (?,?,?,?,?)', (second, str(uuid.uuid4()), 'active', 100, 'other'))
        self.assertEqual(self.run_gate('init', thread=second)['action'], 'continue')
        self.assertEqual(self.run_gate('bind', thread=second, automation_id='automation-1')['reason'],
                         'automation_id_other_task')

    def test_reserve_breach_waits_without_new_baseline(self):
        self.prepared()
        self.snapshot['rateLimits']['primary']['usedPercent'] = 82
        result = self.run_gate()
        self.assertEqual(result['action'], 'wait')
        self.assertEqual(result['reason'], 'usage_budget_reached')
        self.assertEqual(result['budget']['windows']['primary']['baseline_remaining'], '90')

    def test_new_account_and_switch_back_preserve_original_budget(self):
        self.prepared()
        first = copy.deepcopy(self.ledger()['account_budgets'])
        self.snapshot = usage(account='account-b', used=50)
        self.assertEqual(self.run_gate()['action'], 'continue')
        self.assertEqual(len(self.ledger()['account_budgets']), 2)
        self.snapshot = usage(used=60)
        self.assertEqual(self.run_gate()['action'], 'continue')
        self.assertEqual(self.ledger()['account_budgets'][next(iter(first))], next(iter(first.values())))
        self.snapshot = usage(used=82)
        self.assertEqual(self.run_gate()['action'], 'wait')

    def test_different_limit_has_separate_baseline(self):
        self.prepared()
        self.snapshot = usage(limit='other-limit', used=50)
        self.assertEqual(self.run_gate()['action'], 'continue')
        self.assertEqual(len(self.ledger()['account_budgets']), 2)
        self.snapshot = usage(used=82)
        self.assertEqual(self.run_gate()['action'], 'wait')

    def test_unknown_values_never_become_zero_usage(self):
        self.prepared()
        for change in ('account', 'ordinary', 'percent', 'credits'):
            self.snapshot = usage()
            if change == 'account': self.snapshot['accountId'] = None
            if change == 'ordinary': self.snapshot.pop('ordinaryUsageAllowed')
            if change == 'percent': self.snapshot['rateLimits']['primary']['usedPercent'] = None
            if change == 'credits': self.snapshot['rateLimits']['credits']['balance'] = None
            self.assertEqual(self.run_gate()['action'], 'wait')
        self.assertEqual(len(self.ledger()['account_budgets']), 1)

    def test_credits_never_grant_paid_eligibility(self):
        self.snapshot['rateLimits']['credits'].update(hasCredits=True, balance='500')
        self.prepared()
        self.snapshot['ordinaryUsageAllowed'] = False
        self.snapshot['rateLimits']['primary']['usedPercent'] = 100
        self.assertEqual(self.run_gate()['action'], 'wait')
        self.assertFalse(self.ledger()['allow_existing_credits'])

    def test_hard_limit_does_not_clear_wait(self):
        self.prepared()
        self.snapshot['rateLimits']['spendControlReached'] = True
        self.assertEqual(self.run_gate()['action'], 'wait')
        self.assertEqual(self.ledger()['first_wait_at'], 1000)

    def test_one_hour_wait_stops_and_reinit_does_not_rearm(self):
        self.prepared()
        self.snapshot.pop('ordinaryUsageAllowed')
        self.assertEqual(self.run_gate(now=1000)['action'], 'wait')
        self.assertEqual(self.run_gate(now=4599)['action'], 'wait')
        self.assertEqual(self.run_gate(now=4600)['reason'], 'usage_wait_expired')
        self.snapshot = usage(reset=10000)
        self.assertEqual(self.run_gate('init', now=4700)['action'], 'stop')

    def test_available_account_clears_wait_timer(self):
        self.prepared()
        self.snapshot.pop('ordinaryUsageAllowed')
        self.run_gate(now=1000)
        self.snapshot = usage(account='account-b')
        self.assertEqual(self.run_gate(now=1100)['action'], 'continue')
        self.assertNotIn('first_wait_at', self.ledger())

    def test_true_official_reset_recovers_and_keeps_credits_baseline(self):
        self.prepared()
        before = copy.deepcopy(next(iter(self.ledger()['account_budgets'].values()))['credits'])
        self.snapshot = usage(used=82)
        self.assertEqual(self.run_gate(now=1500)['action'], 'wait')
        self.snapshot = usage(used=10, reset=5600)
        self.assertEqual(self.run_gate(now=2001)['action'], 'continue')
        budget = next(iter(self.ledger()['account_budgets'].values()))
        self.assertEqual(budget['windows']['primary']['previous_resets_at'], '2000')
        self.assertEqual(budget['credits'], before)

    def test_early_reset_or_same_cycle_increase_never_rebases(self):
        for snapshot in (usage(used=5), usage(used=5, reset=5600)):
            if self.ledger_path.exists(): self.ledger_path.unlink()
            self.snapshot = usage()
            self.prepared()
            before = copy.deepcopy(self.ledger()['account_budgets'])
            self.snapshot = snapshot
            self.assertEqual(self.run_gate(now=1500)['action'], 'wait')
            self.assertEqual(self.ledger()['account_budgets'], before)

    def test_reset_with_changed_duration_or_credits_increase_refused(self):
        for change in ('duration', 'credits'):
            if self.ledger_path.exists(): self.ledger_path.unlink()
            self.snapshot = usage()
            self.prepared()
            self.snapshot = usage(reset=5600)
            if change == 'duration': self.snapshot['rateLimits']['primary']['windowDurationMins'] = 120
            else: self.snapshot['rateLimits']['credits']['balance'] = '100'
            self.assertEqual(self.run_gate(now=2001)['action'], 'wait')

    def test_new_cycle_zero_then_first_available_initializes_only_once(self):
        self.prepared()
        self.snapshot = usage(used=100, reset=5600)
        self.assertEqual(self.run_gate(now=2001)['action'], 'wait')
        self.snapshot = usage(used=20, reset=5600)
        self.assertEqual(self.run_gate(now=2100)['action'], 'continue')
        self.snapshot = usage(used=10, reset=5600)
        self.assertEqual(self.run_gate(now=2200)['action'], 'wait')

    def test_imported_budget_functions_cannot_apply_full_budget_grant(self):
        self.prepared()
        ledger = self.ledger()
        next(iter(ledger['account_budgets'].values()))['_full_budget_once'] = {'path': '/never/read'}
        self.ledger_path.write_text(json.dumps(ledger))
        self.assertEqual(self.run_gate()['action'], 'wait')

    def test_usage_limited_goal_waits_even_when_quota_available_until_goal_active(self):
        self.prepared()
        for status in ('usage_limited', 'budget_limited'):
            self.update_goal(status=status)
            self.snapshot = usage(used=100)
            self.snapshot['ordinaryUsageAllowed'] = False
            self.assertEqual(self.run_gate(now=1500)['action'], 'wait')
            self.snapshot = usage(account='fresh-' + status)
            result = self.run_gate(now=1600)
            self.assertEqual(result['action'], 'wait')
            self.assertEqual(result['goal_status'], status)
            self.assertEqual(self.ledger()['first_wait_at'], 1500)
            with closing(sqlite3.connect(self.home / 'goals_1.sqlite')) as db, db:
                self.assertEqual(db.execute('SELECT status FROM thread_goals').fetchone()[0], status)
            self.update_goal(status='active')
            self.assertEqual(self.run_gate(now=1700)['action'], 'continue')
            self.assertNotIn('first_wait_at', self.ledger())

    def test_blocked_and_unknown_goal_states_wait_without_erasing_ledger(self):
        self.prepared()
        before = copy.deepcopy(self.ledger()['account_budgets'])
        for status in ('blocked', 'unknown-future-status'):
            self.update_goal(status=status)
            result = self.run_gate(now=6000)
            self.assertEqual(result['action'], 'wait')
            self.assertEqual(result['reason'], 'goal_state_wait')
            self.assertEqual(self.ledger()['account_budgets'], before)
            self.assertNotIn('terminal_action', self.ledger())

    def test_usage_envelope_rejects_bare_wrong_source_thread_old_and_future(self):
        self.prepared()
        valid = {'source': 'codex_app.get_usage_limits', 'thread': self.thread,
                 'observed_at': 1000, 'payload': usage()}
        envelopes = [usage(), {**valid, 'source': 'untrusted'},
                     {**valid, 'thread': str(uuid.uuid4())}, {**valid, 'observed_at': 879},
                     {**valid, 'observed_at': 1006}, {**valid, 'observed_at': None}]
        for envelope in envelopes:
            with self.subTest(envelope=envelope):
                self.usage_file.write_text(json.dumps(envelope))
                result = heartbeat.run(self.thread, 'check', self.usage_file, now=1000)
                self.assertEqual(result['action'], 'wait')
                self.assertEqual(result['reason'], 'usage_budget_unknown')

    def test_usage_envelope_ttl_and_future_drift_boundaries(self):
        self.prepared()
        for observed in (880, 1005):
            self.usage_file.write_text(json.dumps({'source': 'codex_app.get_usage_limits',
                'thread': self.thread, 'observed_at': observed, 'payload': usage()}))
            self.assertEqual(heartbeat.run(self.thread, 'check', self.usage_file, now=1000)['action'], 'continue')

    def test_mcp_text_result_recovers_wait_without_rebasing(self):
        self.prepared()
        before = copy.deepcopy(self.ledger()['account_budgets'])
        self.snapshot = {'content': [{'type': 'text', 'text': 'unavailable'}], 'isError': True}
        self.assertEqual(self.run_gate()['action'], 'wait')
        self.snapshot = {'content': [{'type': 'text', 'text': json.dumps(usage(used=20))}],
                         'isError': False}
        result = self.run_gate(now=1100)
        self.assertEqual(result['action'], 'continue')
        self.assertEqual(self.ledger()['account_budgets'], before)
        self.assertNotIn('first_wait_at', self.ledger())
        self.assertIsNone(result['budget']['error'])

    def test_checkpoint_requires_same_thread_and_does_not_mutate_on_mismatch(self):
        self.prepared()
        before = self.ledger_path.read_bytes()
        for caller in ('', str(uuid.uuid4())):
            with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': caller}):
                self.assertEqual(self.run_gate('checkpoint')['reason'], 'checkpoint_caller_mismatch')
            self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_wrong_checkpoint_caller_never_reads_identity_or_creates_runtime(self):
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': str(uuid.uuid4())}), \
                mock.patch.object(heartbeat, 'identity', side_effect=AssertionError('不能读身份')):
            self.assertEqual(self.run_gate('checkpoint')['reason'], 'checkpoint_caller_mismatch')
        self.assertFalse((self.root / '.agents').exists())
        self.prepared()
        before = self.ledger_path.read_bytes()
        self.update_goal(goal_id=str(uuid.uuid4()))
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': str(uuid.uuid4())}):
            self.assertEqual(self.run_gate('checkpoint')['reason'], 'checkpoint_caller_mismatch')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_checkpoint_uses_original_goal_budget_and_records_actual_caller(self):
        self.prepared()
        before = copy.deepcopy(self.ledger())
        self.snapshot = {'content': [{'type': 'text', 'text': json.dumps(usage(used=20))}]}
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            result = self.run_gate('checkpoint', now=1100)
        self.assertEqual(result['action'], 'continue')
        self.assertEqual(result['last_check'], {'at': 1100, 'mode': 'checkpoint',
                                               'caller_thread': self.thread, 'action': 'continue'})
        self.assertEqual(self.ledger()['goal'], before['goal'])
        self.assertEqual(self.ledger()['account_budgets'], before['account_budgets'])

    def test_checkpoint_rejects_unknown_quota_and_keeps_human_pause(self):
        self.prepared()
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            self.snapshot = {'isError': True, 'content': []}
            self.assertEqual(self.run_gate('checkpoint')['action'], 'wait')
            self.update_goal(status='paused')
            self.snapshot = usage()
            self.assertEqual(self.run_gate('checkpoint')['reason'], 'goal_paused')
        with closing(sqlite3.connect(self.home / 'goals_1.sqlite')) as db:
            self.assertEqual(db.execute('SELECT status FROM thread_goals').fetchone()[0], 'paused')

    def test_checkpoint_does_not_skip_automation_binding(self):
        self.init()
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            self.assertEqual(self.run_gate('checkpoint')['reason'], 'automation_unbound')

    def test_explicit_rebind_preserves_goal_and_budget_and_is_idempotent(self):
        self.prepared()
        before = copy.deepcopy(self.ledger())
        for _ in range(2):
            result = heartbeat.run(self.thread, 'rebind', automation_id='replacement', now=1100,
                                   previous_automation_id='automation-1')
            self.assertEqual(result['reason'], 'automation_bound')
        after = self.ledger()
        self.assertEqual(after['goal'], before['goal'])
        self.assertEqual(after['account_budgets'], before['account_budgets'])
        self.assertEqual(after['automation_rebindings'],
                         [{'from': 'automation-1', 'to': 'replacement', 'at': 1100}])

    def test_rebind_compare_rejects_stale_or_missing_old_id_without_mutation(self):
        self.prepared()
        before = self.ledger_path.read_bytes()
        for previous in (None, 'unrelated', 'replacement'):
            self.assertEqual(heartbeat.run(self.thread, 'rebind', automation_id='replacement',
                             previous_automation_id=previous, now=1100)['reason'],
                             'automation_rebind_mismatch')
            self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_rebind_cannot_claim_another_tasks_automation(self):
        self.prepared()
        other = self.root / '.agents/project/guards' / str(uuid.uuid4())
        other.mkdir()
        (other / 'native-heartbeat.json').write_text(json.dumps({'automation_id': 'replacement'}))
        before = self.ledger_path.read_bytes()
        self.assertEqual(heartbeat.run(self.thread, 'rebind', automation_id='replacement',
                         previous_automation_id='automation-1', now=1100)['reason'],
                         'automation_id_other_task')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_rebind_rejects_corrupt_history_without_writing(self):
        self.prepared()
        valid = self.ledger()
        for history in ('private', [None], [{}], [{'from': 'a', 'to': 'b', 'at': None}]):
            damaged = {**valid, 'automation_rebindings': history}
            self.ledger_path.write_text(json.dumps(damaged))
            before = self.ledger_path.read_bytes()
            result = heartbeat.run(self.thread, 'rebind', automation_id='replacement',
                                   previous_automation_id='automation-1', now=1100)
            self.assertEqual(result['reason'], 'automation_rebinding_history_invalid')
            self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_mcp_structured_result_and_matching_text_are_accepted(self):
        self.prepared()
        for content in ([], [{'type': 'text', 'text': json.dumps(usage())}]):
            self.snapshot = {'structuredContent': usage(), 'content': content}
            self.assertEqual(self.run_gate()['action'], 'continue')

    def test_mcp_errors_malformed_and_conflicting_results_never_grant_usage(self):
        self.prepared()
        before = copy.deepcopy(self.ledger()['account_budgets'])
        text = {'type': 'text', 'text': json.dumps(usage())}
        invalid = [
            {'isError': True, 'content': [text]},
            {'isError': 'false', 'content': [text]},
            {'content': []}, {'content': [text, text]},
            {'content': [{'type': 'image', 'data': 'private'}]},
            {'content': [{'type': 'text', 'text': 'private-not-json'}]},
            {'content': [{'type': 'text', 'text': '[]'}]},
            {'content': [{'type': 'text', 'text': '{}'}]},
            {'content': [{'type': 'text', 'text': json.dumps(usage()).replace(
                '"ordinaryUsageAllowed": true',
                '"ordinaryUsageAllowed": false, "ordinaryUsageAllowed": true')}]},
            {'content': [{'type': 'text', 'text': json.dumps(usage()).replace(
                '"usedPercent": 10', '"usedPercent": 100, "usedPercent": 10')}]},
            {'structuredContent': usage(used=99), 'content': [text]},
            {'structuredContent': None, 'content': [text]},
            {'structuredContent': usage(), 'content': [text, text]},
            {**usage(), 'isError': True},
            {**usage(), 'content': [text]},
        ]
        for payload in invalid:
            with self.subTest(payload=payload):
                self.snapshot = payload
                result = self.run_gate()
                self.assertEqual(result['action'], 'wait')
                self.assertIn(result['budget']['error'],
                              ('usage_tool_error', 'usage_tool_result_invalid',
                               'usage_budget_validation_failed'))
                self.assertNotIn('private', json.dumps(result))
                self.assertEqual(self.ledger()['account_budgets'], before)

    def test_mcp_result_still_enforces_reserve_and_no_paid_eligibility(self):
        self.prepared()
        for snapshot in (usage(used=82), {**usage(), 'ordinaryUsageAllowed': False}):
            self.snapshot = {'content': [{'type': 'text', 'text': json.dumps(snapshot)}]}
            self.assertEqual(self.run_gate()['action'], 'wait')
        self.assertFalse(self.ledger()['allow_existing_credits'])

    def test_goal_changed_during_budget_check_never_continues(self):
        self.prepared()
        original = heartbeat.budget_check

        def changed(*arguments):
            available = original(*arguments)
            self.update_goal(goal_id=str(uuid.uuid4()))
            return available

        with mock.patch.object(heartbeat, 'budget_check', side_effect=changed):
            result = self.run_gate()
        self.assertEqual(result['action'], 'stop')
        self.assertEqual(result['reason'], 'binding_changed_during_check')

    def test_goal_paused_during_budget_check_never_continues(self):
        self.prepared()
        original = heartbeat.budget_check

        def paused(*arguments):
            available = original(*arguments)
            self.update_goal(status='paused')
            return available

        with mock.patch.object(heartbeat, 'budget_check', side_effect=paused):
            self.assertEqual(self.run_gate()['action'], 'stop')


    def full_budget_evidence(self, **changes):
        evidence = {'source': 'codex_app.read_thread', 'target_thread': self.thread,
                    'observed_at': 1000, 'source_thread': str(uuid.uuid4()),
                    'turn_id': str(uuid.uuid4()), 'user_message_id': str(uuid.uuid4()),
                    'user_text': '现在全速并发开发，同时取消额度限制条件。'}
        evidence.update(changes)
        path = self.root / 'authorization.json'
        path.write_text(json.dumps(evidence))
        return path, evidence

    def authorize_full(self, evidence=None, now=1000, authorization=None):
        if evidence is None:
            evidence, _ = self.full_budget_evidence()
        text = json.loads(evidence.read_text())['user_text'] if authorization is None else authorization
        self.write_usage(now)
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            return heartbeat.run(self.thread, 'authorize-full-budget', self.usage_file, now=now,
                                 user_authorization=text, authorization_file=evidence)

    def rebase_gate(self, evidence=None, now=1000):
        if evidence is None:
            evidence, _ = self.full_budget_evidence(user_text='允许重建百分比预算，保留历史和20%，禁用credits')
        self.write_usage(now)
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            return heartbeat.run(self.thread, 'rebase-budget', self.usage_file, now=now,
                                 user_authorization=json.loads(evidence.read_text())['user_text'],
                                 authorization_file=evidence)

    def test_rebase_early_reset_preserves_all_history_credits_and_default_reserve(self):
        self.paused_then_active()
        before = self.ledger()
        self.snapshot = usage(used=0, reset=5000)
        self.assertEqual(self.rebase_gate()['reason'], 'budget_rebased_checkpoint_required')
        after = self.ledger()
        key = after['active_budget_key']
        self.assertEqual(after['percentage_rebases'][0]['previous_account_budgets'], before['account_budgets'])
        self.assertEqual(after['account_budgets'][key]['credits'], before['account_budgets'][key]['credits'])
        self.assertEqual(after['account_budgets'][key]['windows']['primary']['baseline_remaining'], '100')
        self.assertEqual(after['account_budgets'][key]['windows']['primary']['reserve'], '20.0')
        for name in ('binding', 'goal', 'automation_id', 'fraction', 'allow_existing_credits'):
            self.assertEqual(after[name], before[name])
        self.assertNotIn('terminal_action', after)
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            self.assertEqual(self.run_gate('checkpoint')['action'], 'continue')
        self.snapshot['rateLimits']['primary']['usedPercent'] = 80
        self.assertEqual(self.run_gate()['action'], 'wait')

    def test_rebase_authorization_cannot_replay_even_with_changed_text(self):
        self.prepared()
        evidence, data = self.full_budget_evidence(user_text='允许重建预算')
        self.assertEqual(self.rebase_gate(evidence)['reason'], 'budget_rebased_checkpoint_required')
        before = self.ledger_path.read_bytes()
        for text in (data['user_text'], '换个文本不应重放同一真人水位'):
            data['user_text'] = text
            evidence.write_text(json.dumps(data))
            self.assertEqual(self.rebase_gate(evidence)['reason'], 'rebase_authorization_consumed')
            self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_rebase_rejects_stale_wrong_thread_and_malformed_authorization_without_writing(self):
        self.prepared()
        before = self.ledger_path.read_bytes()
        for changes in ({'observed_at': 800}, {'target_thread': str(uuid.uuid4())},
                        {'user_message_id': 'not-uuid'}, {'user_text': ''}, {'source': 'heartbeat'}):
            evidence, _ = self.full_budget_evidence(**changes)
            self.assertEqual(self.rebase_gate(evidence)['reason'], 'rebase_evidence_invalid')
            self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_rebase_rejects_account_duration_and_official_unavailable_without_writing(self):
        self.prepared()
        before = self.ledger_path.read_bytes()
        for snapshot in (usage(account='other'), usage()):
            self.snapshot = snapshot
            if snapshot['accountId'] != 'other':
                self.snapshot['rateLimits']['primary']['windowDurationMins'] = 120
            self.assertEqual(self.rebase_gate()['reason'], 'rebase_evidence_invalid')
            self.assertEqual(self.ledger_path.read_bytes(), before)
        self.snapshot = usage(used=100)
        self.snapshot['ordinaryUsageAllowed'] = False
        self.assertEqual(self.rebase_gate()['reason'], 'rebase_official_usage_unavailable')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_rebase_rejects_active_full_grant_and_does_not_revive_ended_grant(self):
        self.prepared()
        self.authorize_full()
        before = self.ledger_path.read_bytes()
        self.assertEqual(self.rebase_gate()['reason'], 'rebase_state_rejected')
        self.assertEqual(self.ledger_path.read_bytes(), before)
        heartbeat.end_full_budget(self.ledger(), 'actual_quota_exhausted')
        self.assertEqual(self.rebase_gate()['reason'], 'budget_rebased_checkpoint_required')
        self.assertEqual(self.run_gate()['budget']['full_budget_once'], 'inactive')

    def test_rebase_rejects_goal_race_and_wrong_caller(self):
        self.prepared()
        evidence, data = self.full_budget_evidence()
        before = self.ledger_path.read_bytes()
        self.assertEqual(heartbeat.run(self.thread, 'rebase-budget', self.usage_file, now=1000,
                         user_authorization=data['user_text'], authorization_file=evidence)['reason'],
                         'checkpoint_caller_mismatch')
        real = heartbeat.identity
        calls = [0]
        def changed(*args):
            calls[0] += 1
            binding, goal, status = real(*args)
            return binding, goal, ('paused' if calls[0] > 1 else status)
        with mock.patch.object(heartbeat, 'identity', side_effect=changed):
            self.assertEqual(self.rebase_gate(evidence)['reason'], 'rebase_identity_changed')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_rebase_cannot_reuse_ended_full_grant_watermark(self):
        self.prepared()
        evidence, _ = self.full_budget_evidence()
        self.authorize_full(evidence)
        heartbeat.end_full_budget(self.ledger(), 'actual_quota_exhausted')
        before = self.ledger_path.read_bytes()
        self.assertEqual(self.rebase_gate(evidence)['reason'], 'rebase_authorization_consumed')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_rebase_final_freshness_rejects_slow_validation(self):
        self.prepared()
        evidence, data = self.full_budget_evidence()
        ledger = self.ledger()
        before = copy.deepcopy(ledger)
        result = heartbeat.rebase_budget(ledger, self.thread, ledger['binding'], ledger['goal'],
            'active', self.home, self.usage_file, evidence, 1000, data['user_text'], clock=lambda: 1121)
        self.assertEqual(result['reason'], 'rebase_evidence_invalid')
        self.assertEqual(ledger, before)

    def test_rebase_ledger_size_limit_never_writes_unreadable_history(self):
        self.prepared()
        ledger = self.ledger()
        ledger['account_budgets']['historical-other-account'] = {'padding': 'x' * 600000}
        self.ledger_path.write_text(json.dumps(ledger))
        before = self.ledger_path.read_bytes()
        self.assertEqual(self.rebase_gate()['reason'], 'rebase_ledger_limit')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_rebase_uses_validated_envelope_for_audit_not_reopened_file(self):
        self.prepared()
        original = json.loads(self.usage_file.read_text())
        real = heartbeat.identity
        calls = [0]
        def replace_snapshot(*args):
            calls[0] += 1
            if calls[0] > 1:
                self.usage_file.write_text('not-json-after-validation')
            return real(*args)
        with mock.patch.object(heartbeat, 'identity', side_effect=replace_snapshot):
            self.assertEqual(self.rebase_gate()['reason'], 'budget_rebased_checkpoint_required')
        import hashlib
        digest = hashlib.sha256(json.dumps(original, sort_keys=True, separators=(',', ':'),
                               ensure_ascii=False).encode()).hexdigest()
        self.assertEqual(self.ledger()['percentage_rebases'][0]['validated_usage_envelope_sha256'], digest)

    def test_rebase_watermark_cannot_upgrade_to_full_budget(self):
        self.prepared()
        evidence, _ = self.full_budget_evidence(user_text='允许授权')
        self.assertEqual(self.rebase_gate(evidence)['reason'], 'budget_rebased_checkpoint_required')
        before = self.ledger_path.read_bytes()
        self.assertEqual(self.authorize_full(evidence)['reason'], 'full_budget_authorization_consumed')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_full_budget_override_crosses_reserve_without_rebasing(self):
        self.prepared()
        baseline = copy.deepcopy(self.ledger()['account_budgets'])
        self.snapshot['rateLimits']['primary']['usedPercent'] = 90
        self.assertEqual(self.run_gate()['action'], 'wait')
        self.assertEqual(self.authorize_full()['reason'], 'full_budget_authorized_checkpoint_required')
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            result = self.run_gate('checkpoint')
        self.assertEqual(result['action'], 'continue')
        self.assertEqual(result['budget']['full_budget_once'], 'active')
        self.assertEqual(self.ledger()['account_budgets'], baseline)
        self.assertIs(self.ledger()['allow_existing_credits'], False)
        self.assertEqual(self.ledger()['fraction'], '0.8')

    def test_full_budget_requires_original_thread_caller(self):
        self.prepared()
        path, evidence = self.full_budget_evidence()
        result = heartbeat.run(self.thread, 'authorize-full-budget', self.usage_file, now=1000,
                               user_authorization=evidence['user_text'], authorization_file=path)
        self.assertEqual(result['reason'], 'checkpoint_caller_mismatch')
        self.assertFalse((self.ledger_path.parent / 'full-budget-once.json').exists())

    def test_full_budget_invalid_provenance_and_stale_evidence_preserve_ledger(self):
        self.prepared()
        before = self.ledger_path.read_bytes()
        for changes in ({'source': 'heartbeat'}, {'target_thread': str(uuid.uuid4())},
                        {'observed_at': 1}, {'user_message_id': 'not-an-id'}):
            path, _ = self.full_budget_evidence(**changes)
            self.assertEqual(self.authorize_full(path)['reason'], 'full_budget_evidence_invalid')
            self.assertEqual(self.ledger_path.read_bytes(), before)
        path, _ = self.full_budget_evidence()
        self.assertEqual(self.authorize_full(path, authorization='伪造摘要')['reason'], 'full_budget_evidence_invalid')

    def test_full_budget_rejects_account_change_without_mutation(self):
        self.prepared()
        before = self.ledger_path.read_bytes()
        self.snapshot['accountId'] = 'different-account'
        self.assertEqual(self.authorize_full()['reason'], 'full_budget_evidence_invalid')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_full_budget_active_authorization_is_idempotent_but_ended_cannot_replay(self):
        self.prepared()
        path, _ = self.full_budget_evidence()
        self.assertEqual(self.authorize_full(path)['reason'], 'full_budget_authorized_checkpoint_required')
        grant_path = self.ledger_path.parent / 'full-budget-once.json'
        original = grant_path.read_bytes()
        self.assertEqual(self.authorize_full(path)['reason'], 'full_budget_authorized_checkpoint_required')
        self.assertEqual(grant_path.read_bytes(), original)
        evidence = json.loads(path.read_text())
        evidence['user_text'] = '同一消息不能更换原文重新授权'
        path.write_text(json.dumps(evidence))
        self.assertEqual(self.authorize_full(path)['reason'], 'full_budget_evidence_invalid')
        evidence['user_text'] = '现在全速并发开发，同时取消额度限制条件。'
        path.write_text(json.dumps(evidence))
        heartbeat.end_full_budget(self.ledger(), 'actual_quota_exhausted')
        self.assertEqual(self.authorize_full(path)['reason'], 'full_budget_authorization_consumed')

    def test_full_budget_never_grants_existing_credits_or_overrides_official_hard_limits(self):
        self.prepared()
        self.authorize_full()
        for hard_limit in ('spendControlReached', 'rateLimitReachedType'):
            original = copy.deepcopy(self.snapshot)
            self.snapshot['rateLimits'][hard_limit] = True if hard_limit == 'spendControlReached' else 'rate_limit_reached'
            self.assertEqual(self.run_gate()['action'], 'wait')
            self.snapshot = original
        self.snapshot['ordinaryUsageAllowed'] = False
        self.snapshot['rateLimits']['primary']['usedPercent'] = 100
        self.snapshot['rateLimits']['credits'] = {'hasCredits': True, 'unlimited': False, 'balance': '50'}
        self.assertEqual(self.run_gate()['action'], 'wait')
        self.assertIs(self.ledger()['allow_existing_credits'], False)

    def test_full_budget_actual_exhaustion_ends_grant_and_reset_does_not_reactivate(self):
        self.prepared()
        self.authorize_full()
        self.snapshot['ordinaryUsageAllowed'] = False
        self.snapshot['rateLimits']['primary']['usedPercent'] = 100
        self.assertEqual(self.run_gate()['action'], 'wait')
        document = json.loads((self.ledger_path.parent / 'full-budget-once.json').read_text())
        self.assertIsNone(document['active'])
        self.assertEqual(next(iter(document['grants'].values()))['reason'], 'actual_quota_exhausted')
        self.snapshot = usage(used=10, reset=4000)
        self.assertEqual(self.run_gate(now=2100)['budget']['full_budget_once'], 'inactive')

    def test_full_budget_pause_ends_override_and_cannot_authorize_paused_goal(self):
        self.prepared()
        self.authorize_full()
        self.update_goal(status='paused')
        self.assertEqual(self.run_gate()['reason'], 'goal_paused')
        document = json.loads((self.ledger_path.parent / 'full-budget-once.json').read_text())
        self.assertIsNone(document['active'])
        self.assertEqual(self.authorize_full()['reason'], 'goal_paused')

    def test_full_budget_second_goal_check_rejects_race(self):
        self.prepared()
        original = heartbeat.identity
        count = 0
        def changed(*args):
            nonlocal count
            count += 1
            result = original(*args)
            if count == 2:
                return result[0], dict(result[1], id=str(uuid.uuid4())), result[2]
            return result
        with mock.patch.object(heartbeat, 'identity', side_effect=changed):
            self.assertEqual(self.authorize_full()['reason'], 'full_budget_identity_changed')
        self.assertFalse((self.ledger_path.parent / 'full-budget-once.json').exists())

    def test_full_budget_keeps_unknown_snapshot_blocked(self):
        self.prepared()
        self.authorize_full()
        self.snapshot['ordinaryUsageAllowed'] = 'unknown'
        self.assertEqual(self.run_gate()['action'], 'wait')

    def test_full_budget_account_switch_ends_override_and_return_does_not_revive(self):
        self.prepared()
        self.authorize_full()
        self.snapshot['accountId'] = 'other-account'
        self.assertEqual(self.run_gate()['action'], 'wait')
        document = json.loads((self.ledger_path.parent / 'full-budget-once.json').read_text())
        self.assertIsNone(document['active'])
        self.snapshot['accountId'] = 'account-a'
        self.snapshot['rateLimits']['primary']['usedPercent'] = 90
        self.assertEqual(self.run_gate()['action'], 'wait')


    def test_full_budget_goal_replacement_ends_original_authorization(self):
        self.prepared()
        self.authorize_full()
        self.update_goal(goal_id=str(uuid.uuid4()))
        self.assertEqual(self.run_gate()['reason'], 'goal_or_binding_changed')
        document = json.loads((self.ledger_path.parent / 'full-budget-once.json').read_text())
        self.assertIsNone(document['active'])

    def test_full_budget_expired_budget_wait_can_be_explicitly_released_not_reinitialized(self):
        self.prepared()
        baseline = copy.deepcopy(self.ledger()['account_budgets'])
        self.snapshot['rateLimits']['primary']['usedPercent'] = 90
        with mock.patch.object(heartbeat, 'WAIT_TIMEOUT', 0):
            self.assertEqual(self.run_gate()['reason'], 'usage_wait_expired')
        self.assertEqual(self.authorize_full()['reason'], 'full_budget_authorized_checkpoint_required')
        self.assertNotIn('terminal_reason', self.ledger())
        self.assertEqual(self.ledger()['account_budgets'], baseline)
        self.assertEqual(self.run_gate()['action'], 'continue')

    def test_full_budget_registration_requires_current_official_ordinary_eligibility(self):
        self.prepared()
        self.snapshot['ordinaryUsageAllowed'] = False
        self.assertEqual(self.authorize_full()['reason'], 'full_budget_official_usage_unavailable')
        self.assertFalse((self.ledger_path.parent / 'full-budget-once.json').exists())


    def revision_prepared(self):
        self.prepared()
        self.revision_old_hash = self.ledger()['goal']['objective_sha256']
        self.revision_objective = '真人经Goal编辑器修订的同一目标，正文不写审计'
        self.update_goal(objective=self.revision_objective)
        self.assertEqual(self.run_gate()['reason'], 'goal_or_binding_changed')
        self.goal_evidence_file = self.root / 'goal-evidence.json'
        return self.revision_evidence()

    def revision_evidence(self, now=1000):
        return {'source': 'get_goal', 'thread': self.thread, 'observed_at': now,
                'payload': {'goal': {'threadId': self.thread, 'status': 'active',
                                     'objective': self.revision_objective, 'createdAt': 0},
                            'remainingTokens': None, 'completionBudgetReport': None}}

    def revision_gate(self, evidence=None, authorization='本次真人明确同Goal修订并继续，不重算预算',
                      expected_hash=None, now=1000):
        self.goal_evidence_file.write_text(json.dumps(evidence or self.revision_evidence()))
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            return heartbeat.run(self.thread, 'accept-goal-revision', now=now,
                                 user_authorization=authorization,
                                 goal_evidence_file=self.goal_evidence_file,
                                 expected_objective_sha256=(self.revision_old_hash if expected_hash is None
                                                            else expected_hash))

    def test_goal_revision_accepts_only_explicitly_then_requires_new_checkpoint(self):
        self.revision_prepared()
        before = self.ledger()
        self.assertEqual(self.run_gate()['reason'], 'goal_or_binding_changed')
        result = self.revision_gate()
        self.assertEqual((result['action'], result['reason']),
                         ('wait', 'goal_revision_accepted_checkpoint_required'))
        after = self.ledger()
        for key in ('binding', 'created_at', 'automation_id', 'fraction', 'allow_existing_credits',
                    'account_budgets', 'active_budget_key'):
            self.assertEqual(after[key], before[key])
        self.assertEqual(after['goal']['id'], before['goal']['id'])
        self.assertEqual(after['goal']['created_at_ms'], before['goal']['created_at_ms'])
        self.assertNotEqual(after['goal']['objective_sha256'], self.revision_old_hash)
        self.assertNotIn('terminal_action', after)
        self.assertNotIn(self.revision_objective, self.ledger_path.read_text())
        self.assertNotIn('本次真人', self.ledger_path.read_text())
        self.assertEqual(self.run_gate()['reason'], 'goal_revision_checkpoint_required')
        self.assertEqual(self.run_gate('bind', automation_id='automation-1')['action'], 'wait')
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            self.assertEqual(self.run_gate('checkpoint', now=1001)['action'], 'continue')
        self.assertNotIn('goal_revision_checkpoint_required_at', self.ledger())

    def test_goal_revision_rejects_caller_authorization_expected_hash_without_write(self):
        self.revision_prepared()
        before = self.ledger_path.read_bytes()
        for authorization in (None, '', ' ', 'x' * 4097):
            self.assertEqual(self.revision_gate(authorization=authorization)['reason'],
                             'goal_revision_authorization_required')
        self.assertEqual(self.revision_gate(expected_hash='0' * 64)['reason'],
                         'goal_revision_old_hash_mismatch')
        self.assertEqual(self.revision_gate(expected_hash='malformed')['reason'],
                         'goal_revision_evidence_invalid')
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': str(uuid.uuid4())}):
            self.assertEqual(heartbeat.run(self.thread, 'accept-goal-revision')['reason'],
                             'checkpoint_caller_mismatch')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_goal_revision_does_not_accept_new_id_creation_binding_or_unbound(self):
        self.revision_prepared()
        original = self.ledger()
        self.update_goal(goal_id=str(uuid.uuid4()))
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_identity_rejected')
        self.update_goal(goal_id=self.goal_id, created_at_ms=101)
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_identity_rejected')
        self.update_goal(created_at_ms=100)
        binding, goal, status = heartbeat.identity(self.thread, self.home)
        with mock.patch.object(heartbeat, 'identity', return_value=(dict(binding, rollout='changed'), goal, status)):
            self.assertEqual(self.revision_gate()['reason'], 'goal_revision_state_rejected')
        self.assertFalse(self.ledger()['goal_mismatch_origin']['eligible'])
        self.assertEqual({k: v for k, v in self.ledger().items() if k != 'goal_mismatch_origin'},
                         {k: v for k, v in original.items() if k != 'goal_mismatch_origin'})
        ledger = self.ledger()
        ledger['automation_id'] = None
        heartbeat.atomic(self.ledger_path, ledger)
        unbound = self.ledger_path.read_bytes()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_state_rejected')
        self.assertEqual(self.ledger_path.read_bytes(), unbound)

    def test_goal_revision_never_releases_nonactive_or_other_terminal_causes(self):
        self.revision_prepared()
        for status in ('paused', 'complete', 'blocked', 'usage_limited', 'budget_limited'):
            self.update_goal(status=status)
            before = self.ledger()
            self.assertEqual(self.revision_gate()['reason'], 'goal_revision_state_rejected')
            self.assertFalse(self.ledger()['goal_mismatch_origin']['eligible'])
            self.assertEqual({k: v for k, v in self.ledger().items() if k != 'goal_mismatch_origin'},
                             {k: v for k, v in before.items() if k != 'goal_mismatch_origin'})
        self.update_goal(status='active')
        for reason in ('goal_paused', 'goal_complete', 'usage_wait_expired', 'binding_unknown',
                       'binding_changed_during_check'):
            ledger = self.ledger()
            ledger.update(terminal_action='stop', terminal_reason=reason)
            heartbeat.atomic(self.ledger_path, ledger)
            before = self.ledger_path.read_bytes()
            self.assertEqual(self.revision_gate()['reason'], 'goal_revision_state_rejected')
            self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_goal_revision_evidence_source_thread_goal_status_objective_creation_and_time(self):
        self.revision_prepared()
        original = self.ledger_path.read_bytes()
        bad = []
        for key, value in [('source', 'codex_app.read_thread'), ('thread', 'wrong'),
                           ('observed_at', 879), ('observed_at', 1006),
                           ('observed_at', True), ('observed_at', float('nan'))]:
            envelope = self.revision_evidence()
            envelope[key] = value
            bad.append(envelope)
        for key, value in [('threadId', 'wrong'), ('status', 'paused'), ('objective', 'other'),
                           ('createdAt', 1), ('createdAt', False), ('createdAt', 0.0)]:
            envelope = self.revision_evidence()
            envelope['payload']['goal'][key] = value
            bad.append(envelope)
        bad.extend([dict(self.revision_evidence(), payload={}),
                    dict(self.revision_evidence(), payload={'isError': True}),
                    dict(self.revision_evidence(), payload=dict(self.revision_evidence()['payload'], isError=True))])
        for evidence in bad:
            with self.subTest(evidence=evidence):
                self.assertEqual(self.revision_gate(evidence)['reason'], 'goal_revision_evidence_invalid')
                self.assertEqual(self.ledger_path.read_bytes(), original)

    def test_goal_revision_budget_all_accounts_history_and_ended_grant_bytes_are_preserved(self):
        self.prepared()
        self.authorize_full()
        self.revision_old_hash = self.ledger()['goal']['objective_sha256']
        self.revision_objective = '真人修改同一Goal'
        self.update_goal(objective=self.revision_objective)
        self.assertEqual(self.run_gate()['reason'], 'goal_or_binding_changed')
        self.goal_evidence_file = self.root / 'goal-evidence.json'
        grant = self.ledger_path.parent / 'full-budget-once.json'
        grant_bytes = grant.read_bytes()
        ledger = self.ledger()
        ledger['account_budgets']['old-account-history'] = {'credits': {'baseline_remaining': '123.456'},
                                                           'sentinel': ['preserve', 17]}
        ledger['percentage_rebases'] = [{'sentinel': 'original-rebase-history'}]
        ledger['explicit_resumptions'] = [{'sentinel': 'original-resume-history'}]
        ledger['goal_revisions'] = [{'sentinel': 'old-revision-history'}]
        heartbeat.atomic(self.ledger_path, ledger)
        before = self.ledger()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_accepted_checkpoint_required')
        after = self.ledger()
        changed = {'goal', 'goal_status', 'terminal_action', 'terminal_reason', 'goal_revisions',
                   'last_check', 'goal_revision_checkpoint_required_at'}
        self.assertEqual({k: v for k, v in before.items() if k not in changed},
                         {k: v for k, v in after.items() if k not in changed})
        self.assertEqual(after['goal_revisions'][:-1], before['goal_revisions'])
        self.assertEqual(grant.read_bytes(), grant_bytes)
        self.assertEqual(heartbeat.full_budget_state(after), 'inactive')

    def test_goal_revision_race_poisons_origin_and_stale_final_clock_never_accepts(self):
        self.revision_prepared()
        original = self.ledger_path.read_bytes()
        baseline = self.ledger()
        binding, goal, status = heartbeat.identity(self.thread, self.home)
        with mock.patch.object(heartbeat, 'identity', side_effect=[(binding, goal, status),
                                                                 (binding, goal, 'paused')]):
            self.assertEqual(self.revision_gate()['reason'], 'goal_revision_identity_changed')
        self.assertFalse(self.ledger()['goal_mismatch_origin']['eligible'])
        self.assertEqual(self.ledger()['account_budgets'], baseline['account_budgets'])
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_origin_rejected')
        heartbeat.atomic(self.ledger_path, baseline)
        ledger = self.ledger()
        self.goal_evidence_file.write_text(json.dumps(self.revision_evidence()))
        result = heartbeat.accept_goal_revision(
            ledger, self.thread, binding, goal, status, self.home, self.goal_evidence_file,
            self.revision_old_hash, '真人本次修订', 1000, clock=lambda: 1121)
        self.assertEqual(result['reason'], 'goal_revision_evidence_invalid')
        self.assertEqual(ledger, self.ledger())
        self.assertEqual(self.ledger_path.read_bytes(), original)

    def test_goal_revision_replay_is_rejected_without_write_and_different_new_cannot_reuse_old(self):
        self.revision_prepared()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_accepted_checkpoint_required')
        before = self.ledger_path.read_bytes()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_already_accepted')
        self.assertEqual(self.ledger_path.read_bytes(), before)
        self.update_goal(objective='第二次编辑仍须当前旧hash与独立显式接纳')
        self.assertEqual(self.run_gate()['reason'], 'goal_or_binding_changed')
        before = self.ledger_path.read_bytes()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_old_hash_mismatch')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_goal_revision_history_limit_and_oversized_evidence_fail_without_write(self):
        self.revision_prepared()
        ledger = self.ledger()
        ledger['goal_revisions'] = [{'padding': ''}]
        overhead = len((json.dumps(ledger, ensure_ascii=False, indent=2) + '\n').encode())
        ledger['goal_revisions'][0]['padding'] = 'x' * (heartbeat.MAX_JSON - overhead - 300)
        heartbeat.atomic(self.ledger_path, ledger)
        self.assertLess(self.ledger_path.stat().st_size, heartbeat.MAX_JSON)
        before = self.ledger_path.read_bytes()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_ledger_limit')
        self.assertEqual(self.ledger_path.read_bytes(), before)
        self.goal_evidence_file.write_bytes(b' ' * (heartbeat.MAX_JSON + 1))
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            result = heartbeat.run(self.thread, 'accept-goal-revision', now=1000,
                                   user_authorization='真人本次修订', goal_evidence_file=self.goal_evidence_file,
                                   expected_objective_sha256=self.revision_old_hash)
        self.assertEqual(result['reason'], 'goal_revision_evidence_invalid')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_goal_revision_reads_evidence_once_and_audits_original_raw_digest(self):
        self.revision_prepared()
        evidence = self.revision_evidence()
        raw = json.dumps(evidence).encode()
        self.goal_evidence_file.write_bytes(raw)
        original_identity = heartbeat.identity
        calls = []
        def replace_after_validation(*args):
            calls.append(None)
            if len(calls) == 2:
                self.goal_evidence_file.write_text('{}')
            return original_identity(*args)
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}), \
                mock.patch.object(heartbeat, 'identity', side_effect=replace_after_validation):
            result = heartbeat.run(self.thread, 'accept-goal-revision', now=1000,
                                   user_authorization='真人本次修订', goal_evidence_file=self.goal_evidence_file,
                                   expected_objective_sha256=self.revision_old_hash)
        self.assertEqual(result['reason'], 'goal_revision_accepted_checkpoint_required')
        self.assertEqual(self.ledger()['goal_revisions'][-1]['goal_evidence_sha256'],
                         hashlib.sha256(raw).hexdigest())

    def test_goal_revision_old_quota_receipt_cannot_continue_without_fresh_checkpoint(self):
        self.revision_prepared()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_accepted_checkpoint_required')
        self.write_usage(999)
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            result = heartbeat.run(self.thread, 'checkpoint', self.usage_file, now=1000)
        self.assertEqual((result['action'], result['reason']), ('wait', 'goal_revision_checkpoint_required'))
        self.assertIn('goal_revision_checkpoint_required_at', self.ledger())

    def test_goal_revision_cli_passes_explicit_arguments_without_executing_acceptance(self):
        arguments = ['guard_heartbeat.py', '--thread', self.thread, 'accept-goal-revision',
                     '--goal-evidence-file', '/tmp/formal-goal.json',
                     '--expected-objective-sha256', 'a' * 64, '--user-authorization', '本次真人修改']
        with mock.patch('sys.argv', arguments), mock.patch.object(heartbeat, 'run', return_value={}) as gate, \
                mock.patch('builtins.print'):
            heartbeat.main()
        self.assertEqual(gate.call_args.kwargs['goal_evidence_file'], Path('/tmp/formal-goal.json'))
        self.assertEqual(gate.call_args.kwargs['expected_objective_sha256'], 'a' * 64)


    def test_goal_revision_prior_terminal_survives_ordinary_checkpoint_and_cannot_be_accepted(self):
        self.prepared()
        baseline = self.ledger()
        self.revision_old_hash = baseline['goal']['objective_sha256']
        self.revision_objective = '旧停态期间真人修订，不能用修订解除旧边界'
        self.goal_evidence_file = self.root / 'goal-evidence.json'
        for action, reason in [('stop', 'usage_wait_expired'), ('stop', 'goal_paused'),
                               ('finish', 'goal_complete'), ('stop', 'binding_unknown'),
                               ('stop', 'binding_changed_during_check')]:
            with self.subTest(reason=reason):
                ledger = copy.deepcopy(baseline)
                ledger.update(terminal_action=action, terminal_reason=reason)
                heartbeat.atomic(self.ledger_path, ledger)
                before = self.ledger_path.read_bytes()
                self.update_goal(objective=self.revision_objective)
                with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
                    result = self.run_gate('checkpoint')
                self.assertEqual((result['action'], result['reason']), (action, reason))
                self.assertEqual(self.ledger_path.read_bytes(), before)
                self.assertEqual(self.revision_gate()['reason'], 'goal_revision_state_rejected')
                self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_goal_revision_legacy_mismatch_without_provenance_fails_closed(self):
        self.revision_prepared()
        ledger = self.ledger()
        ledger.pop('goal_mismatch_origin')
        ledger['last_check']['action'] = 'continue'
        heartbeat.atomic(self.ledger_path, ledger)
        before = self.ledger_path.read_bytes()
        # 重复普通检查不能为旧版歧义停态补造“无先前终态”的证据。
        self.assertEqual(self.run_gate()['reason'], 'goal_or_binding_changed')
        self.assertEqual(self.ledger_path.read_bytes(), before)
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_origin_rejected')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_goal_revision_prior_terminal_origin_is_not_accepted_as_clean_mismatch(self):
        self.revision_prepared()
        for key in ('previous_terminal_action', 'previous_terminal_reason'):
            ledger = self.ledger()
            clean = copy.deepcopy(ledger['goal_mismatch_origin'])
            ledger['goal_mismatch_origin'][key] = 'stop' if key.endswith('action') else 'goal_paused'
            heartbeat.atomic(self.ledger_path, ledger)
            before = self.ledger_path.read_bytes()
            self.assertEqual(self.revision_gate()['reason'], 'goal_revision_origin_rejected')
            self.assertEqual(self.ledger_path.read_bytes(), before)
            ledger['goal_mismatch_origin'] = clean
            heartbeat.atomic(self.ledger_path, ledger)


    def test_goal_revision_first_mismatch_requires_active_original_binding_and_goal_identity(self):
        self.prepared()
        baseline = self.ledger()
        self.revision_old_hash = baseline['goal']['objective_sha256']
        self.revision_objective = '首次观察时必须同一active Goal纯正文变动'
        self.goal_evidence_file = self.root / 'goal-evidence.json'
        for field, value in [('goal_id', str(uuid.uuid4())), ('created_at_ms', 101),
                             ('status', 'paused'), ('status', 'complete'), ('binding', 'different')]:
            with self.subTest(field=field):
                heartbeat.atomic(self.ledger_path, copy.deepcopy(baseline))
                self.update_goal(goal_id=self.goal_id, created_at_ms=100, status='active',
                                 objective=self.revision_objective)
                if field == 'binding':
                    binding, goal, status = heartbeat.identity(self.thread, self.home)
                    with mock.patch.object(heartbeat, 'identity', return_value=(dict(binding, home='other'), goal, status)):
                        self.assertEqual(self.run_gate()['action'], 'stop')
                else:
                    self.update_goal(**{field: value})
                    self.assertEqual(self.run_gate()['action'], 'stop')
                self.assertFalse(self.ledger()['goal_mismatch_origin']['eligible'])
                self.update_goal(goal_id=self.goal_id, created_at_ms=100, status='active')
                self.assertEqual(self.revision_gate()['reason'], 'goal_revision_origin_rejected')

    def test_goal_revision_valid_origin_is_permanently_poisoned_after_control_or_identity_observation(self):
        self.revision_prepared()
        baseline = self.ledger()
        for field, value in [('goal_id', str(uuid.uuid4())), ('created_at_ms', 101),
                             ('status', 'paused'), ('status', 'complete'), ('binding', 'different')]:
            with self.subTest(field=field):
                heartbeat.atomic(self.ledger_path, copy.deepcopy(baseline))
                self.update_goal(goal_id=self.goal_id, created_at_ms=100, status='active')
                if field == 'binding':
                    binding, goal, status = heartbeat.identity(self.thread, self.home)
                    with mock.patch.object(heartbeat, 'identity', return_value=(dict(binding, rollout='other'), goal, status)):
                        self.assertEqual(self.run_gate()['action'], 'stop')
                else:
                    self.update_goal(**{field: value})
                    self.assertEqual(self.run_gate()['action'], 'stop')
                self.assertFalse(self.ledger()['goal_mismatch_origin']['eligible'])
                self.update_goal(goal_id=self.goal_id, created_at_ms=100, status='active')
                self.assertEqual(self.run_gate()['action'], 'stop')
                self.assertFalse(self.ledger()['goal_mismatch_origin']['eligible'])
                self.assertEqual(self.revision_gate()['reason'], 'goal_revision_origin_rejected')
                self.assertEqual(self.ledger()['account_budgets'], baseline['account_budgets'])

    def test_goal_revision_two_active_objective_edits_keep_same_original_origin(self):
        self.revision_prepared()
        origin = copy.deepcopy(self.ledger()['goal_mismatch_origin'])
        self.revision_objective = '同一Goal仍active时的第二次纯正文编辑'
        self.update_goal(objective=self.revision_objective)
        self.assertEqual(self.run_gate()['reason'], 'goal_or_binding_changed')
        self.assertEqual(self.ledger()['goal_mismatch_origin'], origin)
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_accepted_checkpoint_required')
        self.assertEqual(self.ledger()['goal']['objective_sha256'],
                         hashlib.sha256(self.revision_objective.encode()).hexdigest())

    def test_goal_revision_optional_formal_goal_id_must_not_contradict_index(self):
        self.revision_prepared()
        before = self.ledger_path.read_bytes()
        for key in ('id', 'goalId'):
            evidence = self.revision_evidence()
            evidence['payload']['goal'][key] = str(uuid.uuid4())
            self.assertEqual(self.revision_gate(evidence)['reason'], 'goal_revision_evidence_invalid')
            self.assertEqual(self.ledger_path.read_bytes(), before)
        evidence = self.revision_evidence()
        evidence['payload']['goal'].update(id=self.goal_id, goalId=self.goal_id)
        self.assertEqual(self.revision_gate(evidence)['reason'], 'goal_revision_accepted_checkpoint_required')

    def test_goal_revision_marker_survives_pause_active_explicit_resume_until_fresh_checkpoint(self):
        self.revision_prepared()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_accepted_checkpoint_required')
        accepted_at = self.ledger()['goal_revision_checkpoint_required_at']
        self.update_goal(status='paused')
        self.assertEqual(self.run_gate()['reason'], 'goal_paused')
        self.update_goal(status='active')
        result = self.resume_gate(now=1001)
        self.assertEqual((result['action'], result['reason']), ('wait', 'goal_revision_checkpoint_required'))
        self.assertEqual(self.ledger()['goal_revision_checkpoint_required_at'], accepted_at)
        self.assertNotIn('terminal_action', self.ledger())
        self.assertEqual(self.run_gate()['action'], 'wait')
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            self.assertEqual(self.run_gate('checkpoint', now=1002)['action'], 'continue')
        self.assertNotIn('goal_revision_checkpoint_required_at', self.ledger())

    def test_goal_revision_marker_blocks_all_noncheckpoint_continue_dispatches(self):
        self.revision_prepared()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_accepted_checkpoint_required')
        before = self.ledger_path.read_bytes()
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}), \
                mock.patch.object(heartbeat, 'authorize_full_budget', side_effect=AssertionError('不能绕过checkpoint')), \
                mock.patch.object(heartbeat, 'rebase_budget', side_effect=AssertionError('不能绕过checkpoint')):
            for command in ('bind', 'rebind', 'rebase-budget', 'authorize-full-budget', 'init', 'resume', 'check'):
                result = heartbeat.run(self.thread, command, self.usage_file, now=1001,
                                       automation_id='automation-1', user_authorization='真人继续')
                self.assertEqual((result['action'], result['reason']), ('wait', 'goal_revision_checkpoint_required'))
                self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_goal_revision_preexisting_equal_timestamp_quota_cannot_pass_real_run_checkpoint(self):
        self.revision_prepared()
        # 提前写好的正式额度与接纳落账同秒，不能冒充接纳之后取得的新证据。
        self.write_usage(1000)
        self.goal_evidence_file.write_text(json.dumps(self.revision_evidence(1000)))
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}), \
                mock.patch.object(heartbeat.time, 'time', return_value=1000):
            accepted = heartbeat.run(self.thread, 'accept-goal-revision',
                                     user_authorization='本次真人修订', goal_evidence_file=self.goal_evidence_file,
                                     expected_objective_sha256=self.revision_old_hash)
            self.assertEqual(accepted['reason'], 'goal_revision_accepted_checkpoint_required')
            before = self.ledger_path.read_bytes()
            result = heartbeat.run(self.thread, 'checkpoint', self.usage_file)
        self.assertEqual((result['action'], result['reason']), ('wait', 'goal_revision_checkpoint_required'))
        self.assertEqual(self.ledger_path.read_bytes(), before)
        self.assertIn('goal_revision_checkpoint_required_at', self.ledger())


    def test_goal_revision_pure_objective_race_in_run_rejects_stale_evidence_then_accepts_refreshed(self):
        self.revision_prepared()
        before = self.ledger_path.read_bytes()
        origin = copy.deepcopy(self.ledger()['goal_mismatch_origin'])
        original_identity = heartbeat.identity
        calls = []
        def edit_only_objective_at_final_identity(*args):
            calls.append(None)
            if len(calls) == 2:
                self.revision_objective = '同binding原Goal仍active，在本次证据后再次编辑正文'
                self.update_goal(objective=self.revision_objective)
            return original_identity(*args)
        with mock.patch.object(heartbeat, 'identity', side_effect=edit_only_objective_at_final_identity):
            result = self.revision_gate()
        self.assertEqual(result['reason'], 'goal_revision_objective_changed')
        self.assertEqual(self.ledger_path.read_bytes(), before)
        self.assertEqual(self.ledger()['goal_mismatch_origin'], origin)
        self.assertTrue(self.ledger()['goal_mismatch_origin']['eligible'])
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_accepted_checkpoint_required')

    def test_goal_revision_hard_identity_or_control_race_in_run_permanently_poisons_origin(self):
        self.revision_prepared()
        baseline = self.ledger()
        original_identity = heartbeat.identity
        for field, value in [('goal_id', str(uuid.uuid4())), ('created_at_ms', 101),
                             ('status', 'paused'), ('status', 'complete'), ('binding', 'different')]:
            with self.subTest(field=field):
                heartbeat.atomic(self.ledger_path, copy.deepcopy(baseline))
                self.update_goal(goal_id=self.goal_id, created_at_ms=100, status='active')
                calls = []
                def change_at_final_identity(*args):
                    calls.append(None)
                    if len(calls) == 2:
                        if field == 'binding':
                            binding, goal, status = original_identity(*args)
                            return dict(binding, rollout='different'), goal, status
                        self.update_goal(**{field: value})
                    return original_identity(*args)
                with mock.patch.object(heartbeat, 'identity', side_effect=change_at_final_identity):
                    result = self.revision_gate()
                self.assertEqual(result['reason'], 'goal_revision_identity_changed')
                self.assertFalse(self.ledger()['goal_mismatch_origin']['eligible'])
                self.update_goal(goal_id=self.goal_id, created_at_ms=100, status='active')
                self.assertEqual(self.revision_gate()['reason'], 'goal_revision_origin_rejected')
                self.assertEqual(self.ledger()['account_budgets'], baseline['account_budgets'])


    def write_revision_near_max_ledger(self, ledger, spare=8):
        ledger['account_budgets']['old-large-account-history'] = {'padding': ''}
        overhead = len((json.dumps(ledger, ensure_ascii=False, separators=(',', ':')) + '\n').encode())
        ledger['account_budgets']['old-large-account-history']['padding'] = 'x' * (heartbeat.MAX_JSON - overhead - spare)
        heartbeat.atomic(self.ledger_path, ledger)
        self.assertLessEqual(self.ledger_path.stat().st_size, heartbeat.MAX_JSON)
        self.assertEqual(heartbeat.read_json(self.ledger_path), ledger)
        return self.ledger_path.read_bytes()

    def test_goal_revision_first_origin_overflow_keeps_legal_history_readable_and_cannot_adopt(self):
        self.prepared()
        self.revision_old_hash = self.ledger()['goal']['objective_sha256']
        self.revision_objective = '近MAX合法历史不能被新增origin写坏'
        self.goal_evidence_file = self.root / 'goal-evidence.json'
        before = self.write_revision_near_max_ledger(self.ledger())
        self.update_goal(objective=self.revision_objective)
        self.assertEqual(self.run_gate()['reason'], 'goal_revision_ledger_limit')
        self.assertEqual(self.ledger_path.read_bytes(), before)
        self.assertNotIn('goal_mismatch_origin', heartbeat.read_json(self.ledger_path))
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_state_rejected')
        self.assertEqual(self.run_gate()['reason'], 'goal_revision_ledger_limit')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_goal_revision_near_max_valid_origin_poison_shrinks_then_next_run_still_rejects(self):
        self.revision_prepared()
        self.write_revision_near_max_ledger(self.ledger())
        budget_history = copy.deepcopy(self.ledger()['account_budgets'])
        revision_history = copy.deepcopy(self.ledger().get('goal_revisions', []))
        self.update_goal(status='paused')
        self.assertEqual(self.run_gate()['action'], 'stop')
        poisoned = heartbeat.read_json(self.ledger_path)
        self.assertEqual(poisoned['goal_mismatch_origin'], {'eligible': False})
        self.assertEqual(poisoned['account_budgets'], budget_history)
        self.assertEqual(poisoned.get('goal_revisions', []), revision_history)
        self.assertLessEqual(self.ledger_path.stat().st_size, heartbeat.MAX_JSON)
        self.update_goal(status='active')
        self.assertEqual(self.run_gate()['action'], 'stop')
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_origin_rejected')
        self.assertEqual(heartbeat.read_json(self.ledger_path)['account_budgets'], budget_history)

    def test_goal_revision_near_max_poison_plus_new_pause_terminal_remains_readable_and_stopped(self):
        self.revision_prepared()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_accepted_checkpoint_required')
        self.write_revision_near_max_ledger(self.ledger())
        before = self.ledger()
        self.update_goal(status='paused')
        self.assertEqual(self.run_gate()['reason'], 'goal_paused')
        paused = heartbeat.read_json(self.ledger_path)
        self.assertEqual(paused['goal_mismatch_origin'], {'eligible': False})
        self.assertEqual(paused['terminal_reason'], 'goal_paused')
        self.assertEqual(paused['account_budgets'], before['account_budgets'])
        self.assertEqual(paused['goal_revisions'], before['goal_revisions'])
        self.assertEqual(paused['goal_revision_checkpoint_required_at'], before['goal_revision_checkpoint_required_at'])
        self.assertLessEqual(self.ledger_path.stat().st_size, heartbeat.MAX_JSON)
        self.update_goal(status='active')
        self.assertEqual(self.run_gate()['reason'], 'goal_paused')
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_state_rejected')
        self.assertEqual(heartbeat.read_json(self.ledger_path)['terminal_reason'], 'goal_paused')

    def test_goal_revision_tiny_invalid_origin_near_max_cannot_be_adopted_or_release_terminal(self):
        self.revision_prepared()
        ledger = self.ledger()
        ledger['goal_mismatch_origin'] = {'eligible': True}
        self.write_revision_near_max_ledger(ledger, spare=0)
        before = self.ledger_path.read_bytes()
        self.update_goal(status='paused')
        self.assertEqual(self.run_gate()['action'], 'stop')
        # true→false可能多一字节；畸形来源缺证明，已有终态保持，不能采用。
        self.assertEqual(self.ledger_path.read_bytes(), before)
        self.update_goal(status='active')
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_origin_rejected')
        self.assertEqual(self.run_gate()['action'], 'stop')
        self.assertEqual(heartbeat.read_json(self.ledger_path)['terminal_reason'], 'goal_or_binding_changed')


    def test_goal_revision_replay_does_not_mask_later_terminal_or_explicit_poison(self):
        self.revision_prepared()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_accepted_checkpoint_required')
        accepted = self.ledger()
        for action, reason, poison in [('stop', 'goal_paused', False),
                                       ('finish', 'goal_complete', False),
                                       ('stop', 'usage_wait_expired', False),
                                       ('stop', 'binding_unknown', False),
                                       (None, None, True)]:
            with self.subTest(reason=reason, poison=poison):
                ledger = copy.deepcopy(accepted)
                if action is not None:
                    ledger.update(terminal_action=action, terminal_reason=reason)
                if poison:
                    ledger['goal_mismatch_origin'] = {'eligible': False}
                heartbeat.atomic(self.ledger_path, ledger)
                before = self.ledger_path.read_bytes()
                self.assertEqual(self.revision_gate()['reason'], 'goal_revision_state_rejected')
                self.assertEqual(self.ledger_path.read_bytes(), before)
        heartbeat.atomic(self.ledger_path, accepted)
        clean = self.ledger_path.read_bytes()
        self.assertEqual(self.revision_gate()['reason'], 'goal_revision_already_accepted')
        self.assertEqual(self.ledger_path.read_bytes(), clean)


    def origin_prepared(self):
        self.prepared()
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            self.assertEqual(self.run_gate('checkpoint')['action'], 'continue')
        self.revision_old_hash = self.ledger()['goal']['objective_sha256']
        self.revision_objective = '真人经Goal编辑器修订的同一目标，正文不写审计'
        self.update_goal(objective=self.revision_objective)
        self.assertEqual(self.run_gate()['reason'], 'goal_or_binding_changed')
        self.goal_evidence_file = self.root / 'goal-evidence.json'
        ledger = self.ledger()
        ledger.pop('goal_mismatch_origin')
        heartbeat.atomic(self.ledger_path, ledger)
        self.origin_expected_sha = hashlib.sha256(self.ledger_path.read_bytes()).hexdigest()
        self.origin_goal = self.revision_evidence()
        self.origin_decision_file = self.root / 'origin-decision.json'
        self.origin_formal_file = self.root / 'formal-thread.json'
        self.origin_audit_file = self.root / 'formal-origin-audit.json'
        self.origin_controller_file = self.root / 'controller-inventory.json'
        command = ("/bin/zsh -lc \"python3 -B .agents/skills/agent-continuous-development-guard/scripts/guard_heartbeat.py "
                   f"--thread {self.thread} checkpoint --usage-file tmp/usage.json > tmp/result.json\n"
                   "python3 -B - <<'PY'\nprint('summary')\nPY\"")
        self.origin_items = [
            {'type': 'commandExecution', 'id': 'checkpoint-start', 'command': command,
             'status': 'completed', 'exitCode': 0,
             'output': {'text': 'continue active usage_budget_available\n', 'truncated': False}},
            {'type': 'commandExecution', 'id': 'read-source', 'command': 'rg source path',
             'status': 'completed', 'exitCode': 0, 'output': {'text': 'match\n', 'truncated': False}},
            {'type': 'commandExecution', 'id': 'benign-rg-no-match', 'command': 'rg absent path',
             'status': 'failed', 'exitCode': 1, 'output': {'text': '', 'truncated': False}},
            {'type': 'commandExecution', 'id': 'checkpoint-stop', 'command': command,
             'status': 'completed', 'exitCode': 0,
             'output': {'text': 'stop active usage_budget_available\n', 'truncated': False}},
            {'type': 'commandExecution', 'id': 'stop-receipt', 'command': 'cat tmp/result.json',
             'status': 'completed', 'exitCode': 0,
             'output': {'text': json.dumps(heartbeat.output(ledger, 'stop', 'goal_or_binding_changed', self.thread)),
                        'truncated': False}}]
        self.origin_controller = {
            'version': 1, 'source': 'root_controller_inventory', 'thread': self.thread, 'observed_at': 1000,
            'automation': {'id': 'automation-1', 'target_thread_id': self.thread,
                           'kind': 'heartbeat', 'status': 'PAUSED', 'unique': True},
            'native_ledger_writers': [{'kind': 'root_checkpoint', 'thread': self.thread}],
            'other_guard_writers': [],
            'cooperative': {'role': 'queue_and_read_only_native_ledger', 'writes_native_ledger': False,
                            'manual_stop_preserved': True, 'pending_preserved': True}}
        self.origin_decision = {
            'version': 1, 'kind': 'legacy_root_origin_decision', 'thread': self.thread,
            'observed_at': 1000, 'binding': ledger['binding'], 'old_goal': ledger['goal'],
            'expected_ledger_sha256': self.origin_expected_sha,
            'single_writer': {'contract': 'trusted_single_root_native_ledger_writer', 'thread': self.thread,
                              'automation_id': 'automation-1', 'adjudicated_no_prior_terminal': True,
                              'limitation': 'not_a_full_host_write_journal'},
            'endpoints': {'turn_id': 'turn-audit', 'continue_id': 'checkpoint-start',
                          'stop_id': 'checkpoint-stop', 'receipt_id': 'stop-receipt'}}
        self.origin_audit = {'formal_source': {'thread_id': self.thread, 'turn_id': 'turn-audit'},
                            'assessment': {'root_current_turn_tool_chain_gap_closed': True,
                                           'root_tool_chain_has_intervening_guard_or_ledger_or_goal_write': False,
                                           'first_stop_in_current_turn_established': True,
                                           'first_stop_summary': 'stop active usage_budget_available',
                                           'stop_reason_confirmed_by_item78': 'goal_or_binding_changed',
                                           'sufficient_for_unconditional_legacy_origin_clearance': False,
                                           'supports_no_other_root_guard_terminal_between_item41_and_item76': True,
                                           'sufficient_to_prove_no_other_writer_changed_original_ledger_globally': False}}
        self.origin_refresh(rebuild_audit=True)

    def origin_refresh(self, rebuild_audit=False):
        formal = {'isError': False, 'content': [{'type': 'text', 'text': json.dumps({
            'thread': {'id': self.thread, 'cwd': str(self.root)},
            'turns': [{'id': 'turn-audit', 'items': self.origin_items}]})}]}
        self.origin_formal_file.write_text(json.dumps(formal))
        digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
        self.origin_audit['formal_source']['sha256'] = digest(self.origin_formal_file)
        if rebuild_audit:
            rows = []
            for i, item in enumerate(self.origin_items[:4]):
                rows.append({'item_index_zero_based': i, 'event_id': item['id'], 'type': item['type'],
                             'item_sha256_canonical_json': heartbeat.origin_item_digest(item),
                             'status': item['status'], 'exit_code': item.get('exitCode'),
                             'classification': ('guard_checkpoint' if i in (0, 3) else
                                                ('formal_usage_read' if item['type'] == 'mcpToolCall' else 'read_only'))})
            receipt_item = self.origin_items[4]
            receipt_row = {'item_index_zero_based': 4, 'event_id': receipt_item['id'],
                           'type': receipt_item['type'],
                           'item_sha256_canonical_json': heartbeat.origin_item_digest(receipt_item),
                           'status': receipt_item['status'], 'exit_code': receipt_item['exitCode'],
                           'classification': 'read_receipt'}
            self.origin_audit['interval'] = {
                'start_item_index': 0, 'end_item_index': 3, 'items': rows + [receipt_row],
                'strict_between_command_executions': sum(x['type'] == 'commandExecution' for x in self.origin_items[1:3]),
                'strict_between_mcp_tool_calls': sum(x['type'] == 'mcpToolCall' for x in self.origin_items[1:3]),
                'command_executions_including_endpoints': sum(x['type'] == 'commandExecution' for x in self.origin_items[:4]),
                'command_truncations': 0, 'output_truncations': 0,
                'missing_output_fields': [], 'subagent_completion_indices': []}
            self.origin_audit['key_items'] = [{'event_id': self.origin_items[i]['id'],
                                              'item_sha256_canonical_json': heartbeat.origin_item_digest(self.origin_items[i])}
                                             for i in (0, 3, 4)]
            self.origin_decision['benign_failures'] = [
                {'event_id': row['event_id'], 'item_sha256_canonical_json': row['item_sha256_canonical_json'],
                 'status': row['status'], 'exit_code': row['exit_code'],
                 'adjudication': 'benign_non_guard_failure', 'classification': row['classification']}
                for row in rows if row['status'] == 'failed']
        self.origin_audit_file.write_text(json.dumps(self.origin_audit))
        self.origin_controller_file.write_text(json.dumps(self.origin_controller))
        self.origin_decision['evidence'] = {name: {'path': str(path), 'sha256': digest(path)}
                                          for name, path in [('formal_thread', self.origin_formal_file),
                                                             ('audit', self.origin_audit_file),
                                                             ('controller_inventory', self.origin_controller_file)]}
        self.origin_decision_file.write_text(json.dumps(self.origin_decision))
        self.goal_evidence_file.write_text(json.dumps(self.origin_goal))

    def origin_gate(self, expected_sha=None, now=1000, authorization='真人本次修订与active已核；单Root契约裁定'):
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            return heartbeat.run(self.thread, 'recover-goal-revision-origin', now=now,
                                 origin_evidence_file=self.origin_decision_file,
                                 goal_evidence_file=self.goal_evidence_file,
                                 expected_ledger_sha256=self.origin_expected_sha if expected_sha is None else expected_sha,
                                 expected_objective_sha256=self.revision_old_hash,
                                 user_authorization=authorization)

    def test_origin_recovery_preserves_stop_then_accept_requires_strictly_new_checkpoint(self):
        self.origin_prepared()
        before = self.ledger()
        result = self.origin_gate()
        self.assertEqual((result['action'], result['reason']), ('wait', 'origin_recovered_acceptance_required'))
        after = self.ledger()
        self.assertEqual({k: v for k, v in after.items() if k not in ('goal_mismatch_origin', 'goal_origin_recoveries')}, before)
        self.assertEqual(after['goal_mismatch_origin']['kind'], 'recovered_root_single_writer')
        self.assertEqual(after['goal_mismatch_origin']['benign_failures'][0]['status'], 'failed')
        self.assertEqual(after['terminal_reason'], 'goal_or_binding_changed')
        self.assertEqual(self.run_gate()['action'], 'stop')
        self.assertEqual(self.revision_gate(now=1001)['reason'], 'goal_revision_accepted_checkpoint_required')
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            self.assertEqual(self.run_gate('checkpoint', now=1001)['reason'], 'goal_revision_checkpoint_required')
            self.assertEqual(self.run_gate('checkpoint', now=1002)['action'], 'continue')

    def test_origin_recovery_rejects_cas_mismatch_and_replay_without_write(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        self.assertEqual(self.origin_gate('0' * 64)['reason'], 'origin_recovery_ledger_changed')
        self.assertEqual(self.ledger_path.read_bytes(), before)
        self.assertEqual(self.origin_gate()['reason'], 'origin_recovered_acceptance_required')
        before = self.ledger_path.read_bytes()
        self.assertEqual(self.origin_gate()['action'], 'stop')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_origin_recovery_rejects_other_terminal_poison_and_any_existing_origin(self):
        self.origin_prepared()
        original = self.ledger()
        for fields in ({'terminal_reason': 'goal_paused'}, {'terminal_action': 'finish'},
                       {'goal_mismatch_origin': None}, {'goal_mismatch_origin': {}},
                       {'goal_mismatch_origin': {'eligible': False}}, {'goal_mismatch_origin': {'eligible': True}}):
            with self.subTest(fields=fields):
                ledger = dict(original, **fields)
                heartbeat.atomic(self.ledger_path, ledger)
                before = self.ledger_path.read_bytes()
                result = self.origin_gate(hashlib.sha256(before).hexdigest())
                self.assertEqual(result['reason'], 'origin_recovery_state_rejected')
                self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_origin_recovery_controller_single_writer_is_mandatory_and_no_controls_are_changed(self):
        self.origin_prepared()
        original = copy.deepcopy(self.origin_controller)
        before = self.ledger_path.read_bytes()
        cases = [dict(original, other_guard_writers=['guard.py']), dict(original, native_ledger_writers=[]),
                 dict(original, thread='other'), dict(original, cooperative=dict(original['cooperative'], writes_native_ledger=True)),
                 dict(original, automation=dict(original['automation'], unique=False)),
                 dict(original, automation=dict(original['automation'], target_thread_id='other'))]
        for controller in cases:
            self.origin_controller = controller
            self.origin_refresh()
            self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
            self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_origin_recovery_rejects_digest_wrong_goal_optional_id_and_stale_evidence(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        good = copy.deepcopy(self.origin_goal)
        for evidence in [dict(good, observed_at=879), dict(good, thread='other'),
                         dict(good, payload=dict(good['payload'], goal=dict(good['payload']['goal'], id='other'))),
                         dict(good, payload=dict(good['payload'], goal=dict(good['payload']['goal'], goalId='other'))),
                         dict(good, payload=dict(good['payload'], goal=dict(good['payload']['goal'], status='paused')))]:
            self.goal_evidence_file.write_text(json.dumps(evidence))
            self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
            self.assertEqual(self.ledger_path.read_bytes(), before)
        self.origin_goal = good
        self.origin_refresh()
        self.origin_formal_file.write_text(self.origin_formal_file.read_text() + ' ')
        self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_origin_recovery_rejects_truncation_missing_interval_reversed_endpoint_and_other_mcp(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        original = copy.deepcopy(self.origin_items)
        for change in ['command_truncated', 'output_truncated', 'missing_row', 'reverse', 'mcp_write', 'failed_guard', 'unknown_failure']:
            self.origin_items = copy.deepcopy(original)
            self.origin_decision['endpoints']['continue_id'] = 'checkpoint-start'
            self.origin_decision['endpoints']['stop_id'] = 'checkpoint-stop'
            if change == 'command_truncated': self.origin_items[1]['commandTruncated'] = True
            if change == 'output_truncated': self.origin_items[1]['output']['truncated'] = True
            if change == 'mcp_write': self.origin_items[1] = {'type': 'mcpToolCall', 'id': 'read-source', 'server': 'codex_app', 'tool': 'automation_update', 'status': 'completed'}
            if change == 'failed_guard': self.origin_items[2]['command'] = 'python3 guard_heartbeat.py checkpoint'
            self.origin_refresh(rebuild_audit=True)
            if change == 'missing_row': self.origin_audit['interval']['items'].pop(1)
            if change == 'reverse':
                self.origin_decision['endpoints']['continue_id'] = 'checkpoint-stop'
                self.origin_decision['endpoints']['stop_id'] = 'checkpoint-start'
            if change == 'unknown_failure': self.origin_decision['benign_failures'] = []
            self.origin_refresh()
            with self.subTest(change=change):
                self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
                self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_origin_recovery_stop_receipt_fields_are_not_taken_from_cached_active_status(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        original = copy.deepcopy(self.origin_items)
        for field, value in [('reason', 'usage_wait_expired'), ('goal_id', 'other'),
                             ('automation_id', 'other'), ('last_check', {})]:
            self.origin_items = copy.deepcopy(original)
            receipt = json.loads(self.origin_items[4]['output']['text'])
            receipt[field] = value
            self.origin_items[4]['output']['text'] = json.dumps(receipt)
            self.origin_refresh(rebuild_audit=True)
            self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
            self.assertEqual(self.ledger_path.read_bytes(), before)
        self.origin_items = copy.deepcopy(original)
        receipt = json.loads(self.origin_items[4]['output']['text'])
        receipt['goal_status'] = 'paused'
        self.origin_items[4]['output']['text'] = json.dumps(receipt)
        self.origin_refresh(rebuild_audit=True)
        self.assertEqual(self.origin_gate()['reason'], 'origin_recovered_acceptance_required')
        self.assertEqual(self.ledger()['goal_mismatch_origin']['active_basis'],
                         'current_formal_goal_and_verified_user_control_not_cached_stop')

    def test_origin_recovery_identity_final_ttl_and_final_raw_cas_races_never_write(self):
        self.origin_prepared()
        original = self.ledger_path.read_bytes()
        binding, goal, status = heartbeat.identity(self.thread, self.home)
        with mock.patch.object(heartbeat, 'identity', side_effect=[(binding, goal, status), (binding, goal, 'paused')]):
            self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_identity_changed')
        self.assertEqual(self.ledger_path.read_bytes(), original)
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': self.thread}):
            result = heartbeat.recover_goal_revision_origin(self.ledger_path, self.thread, self.home,
                self.origin_decision_file, self.goal_evidence_file, self.origin_expected_sha,
                self.revision_old_hash, '本次已核授权', 1000, lambda: 1121)
        self.assertEqual(result['reason'], 'origin_recovery_evidence_invalid')
        self.assertEqual(self.ledger_path.read_bytes(), original)
        count = 0
        def identity_race(*_):
            nonlocal count
            count += 1
            if count == 2: self.ledger_path.write_bytes(original + b' ')
            return binding, goal, status
        with mock.patch.object(heartbeat, 'identity', side_effect=identity_race):
            self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_ledger_changed')
        self.assertEqual(self.ledger_path.read_bytes(), original + b' ')

    def test_origin_recovery_preserves_all_budget_history_and_grant_file_bytes(self):
        self.origin_prepared()
        ledger = self.ledger()
        ledger['account_budgets']['preserved-account'] = {'sentinel': [12, {'credits': '9.8765'}]}
        ledger['percentage_rebases'] = [{'sentinel': 'rebase'}]
        ledger['explicit_resumptions'] = [{'sentinel': 'resume'}]
        ledger['goal_revisions'] = [{'sentinel': 'revision'}]
        ledger['goal_origin_recoveries'] = [{'sentinel': 'old-audit'}]
        grant = self.ledger_path.parent / 'full-budget-once.json'
        grant.write_bytes(b'{"grants":{},"active":null,"sentinel":"private-ended-grant"}\n')
        ledger['full_budget_grant_file'] = str(grant)
        heartbeat.atomic(self.ledger_path, ledger)
        self.origin_expected_sha = hashlib.sha256(self.ledger_path.read_bytes()).hexdigest()
        self.origin_decision['expected_ledger_sha256'] = self.origin_expected_sha
        self.origin_refresh()
        before = self.ledger()
        grant_bytes = grant.read_bytes()
        self.assertEqual(self.origin_gate()['reason'], 'origin_recovered_acceptance_required')
        after = self.ledger()
        self.assertEqual({k:v for k,v in after.items() if k not in ('goal_mismatch_origin', 'goal_origin_recoveries')},
                         {k:v for k,v in before.items() if k != 'goal_origin_recoveries'})
        self.assertEqual(after['goal_origin_recoveries'][:-1], before['goal_origin_recoveries'])
        self.assertEqual(grant.read_bytes(), grant_bytes)

    def test_origin_recovery_duplicate_json_limit_and_final_candidate_size_are_fail_closed(self):
        self.origin_prepared()
        original = self.ledger_path.read_bytes()
        raw = self.origin_decision_file.read_text()
        self.origin_decision_file.write_text(raw[:-1] + ',"version":1}')
        self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
        self.assertEqual(self.ledger_path.read_bytes(), original)
        self.origin_refresh()
        self.origin_formal_file.write_bytes(b' ' * (heartbeat.MAX_JSON + 1))
        self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
        self.assertEqual(self.ledger_path.read_bytes(), original)
        self.origin_refresh()
        with mock.patch.object(heartbeat, 'revision_ledger_fits', side_effect=[True, False]):
            self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_ledger_limit')
        self.assertEqual(self.ledger_path.read_bytes(), original)

    def test_origin_recovery_caller_and_authorization_are_required_without_touching_state(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        with mock.patch.dict(os.environ, {'CODEX_THREAD_ID': str(uuid.uuid4())}):
            self.assertEqual(heartbeat.run(self.thread, 'recover-goal-revision-origin')['reason'], 'checkpoint_caller_mismatch')
        self.assertEqual(self.origin_gate(authorization=' ')['reason'], 'origin_recovery_authorization_required')
        self.assertEqual(self.ledger_path.read_bytes(), before)



    def test_origin_recovery_live_paused_or_changed_goal_identity_never_poisons_or_writes(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        binding, goal, status = heartbeat.identity(self.thread, self.home)
        for changed in [(binding, goal, 'paused'), (binding, goal, 'complete'),
                        (binding, dict(goal, id='replaced'), status),
                        (binding, dict(goal, created_at_ms=101), status),
                        (dict(binding, thread='other'), goal, status)]:
            with mock.patch.object(heartbeat, 'identity', return_value=changed):
                self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_identity_rejected')
            self.assertEqual(self.ledger_path.read_bytes(), before)
        old = self.revision_old_hash
        self.revision_old_hash = '0' * 64
        self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_identity_rejected')
        self.revision_old_hash = old
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_origin_recovery_no_singlewriter_decision_no_interval_hash_no_unknown_write_event(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        original_items = copy.deepcopy(self.origin_items)
        original_decision = copy.deepcopy(self.origin_decision)
        for mutation in ['no_contract', 'overstated_journal', 'item_digest', 'extra_event', 'new_child', 'duplicate_id']:
            self.origin_items = copy.deepcopy(original_items)
            self.origin_decision = copy.deepcopy(original_decision)
            self.origin_refresh(rebuild_audit=True)
            if mutation == 'no_contract': self.origin_decision['single_writer'] = {}
            if mutation == 'overstated_journal': self.origin_audit['assessment']['sufficient_to_prove_no_other_writer_changed_original_ledger_globally'] = True
            if mutation == 'item_digest': self.origin_audit['interval']['items'][1]['item_sha256_canonical_json'] = '0' * 64
            if mutation == 'extra_event': self.origin_items[1] = {'type': 'fileChange', 'id': 'read-source', 'status': 'completed'}
            if mutation == 'new_child': self.origin_items[1] = {'type': 'subAgentActivity', 'id': 'read-source', 'kind': 'spawn'}
            if mutation == 'duplicate_id': self.origin_items[1]['id'] = 'checkpoint-start'
            self.origin_refresh()
            self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
            self.assertEqual(self.ledger_path.read_bytes(), before)
            self.origin_audit['assessment']['sufficient_to_prove_no_other_writer_changed_original_ledger_globally'] = False

    def test_origin_recovery_verified_usage_mcp_is_allowed_and_failed_mcp_is_rejected(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        self.origin_items[1] = {'type': 'mcpToolCall', 'id': 'read-source', 'server': 'codex_app',
                                'tool': 'get_usage_limits', 'arguments': {}, 'status': 'failed'}
        self.origin_refresh(rebuild_audit=True)
        self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
        self.assertEqual(self.ledger_path.read_bytes(), before)
        self.origin_items[1]['status'] = 'completed'
        self.origin_refresh(rebuild_audit=True)
        self.assertEqual(self.origin_gate()['reason'], 'origin_recovered_acceptance_required')

    def test_origin_recovery_real_ledger_capacity_not_only_evidence_capacity(self):
        self.origin_prepared()
        paths = [self.origin_decision_file, self.origin_formal_file, self.origin_audit_file,
                 self.origin_controller_file, self.goal_evidence_file, self.ledger_path]
        cap = max(len(p.read_bytes()) for p in paths) + 2048
        ledger = self.ledger()
        ledger['padding'] = ''
        overhead = len(heartbeat.ledger_serialization(ledger).encode())
        ledger['padding'] = 'x' * (cap - overhead - 50)
        heartbeat.atomic(self.ledger_path, ledger)
        before = self.ledger_path.read_bytes()
        self.origin_expected_sha = hashlib.sha256(before).hexdigest()
        self.origin_decision['expected_ledger_sha256'] = self.origin_expected_sha
        self.origin_refresh()
        with mock.patch.object(heartbeat, 'MAX_JSON', cap):
            self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_ledger_limit')
        self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_origin_recovery_cli_is_separate_from_accept_and_requires_explicit_flags(self):
        args = ['guard_heartbeat.py', '--thread', self.thread, 'recover-goal-revision-origin',
                '--origin-evidence-file', 'decision.json', '--goal-evidence-file', 'goal.json',
                '--expected-ledger-sha256', 'a' * 64, '--expected-objective-sha256', 'b' * 64,
                '--user-authorization', '已核本次真人控制']
        with mock.patch('sys.argv', args), mock.patch.object(heartbeat, 'run', return_value={}) as run, mock.patch('builtins.print'):
            heartbeat.main()
        self.assertEqual(run.call_args.args[:2], (self.thread, 'recover-goal-revision-origin'))
        self.assertEqual(run.call_args.kwargs['origin_evidence_file'], Path('decision.json'))
        self.assertEqual(run.call_args.kwargs['expected_ledger_sha256'], 'a' * 64)
        self.assertEqual(run.call_args.kwargs['goal_evidence_file'], Path('goal.json'))



    def test_origin_recovery_classification_is_bound_to_exact_tool_type_and_endpoint_role(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        items = copy.deepcopy(self.origin_items)
        for kind in ['middle_guard', 'middle_receipt', 'shell_usage', 'mcp_guard', 'mcp_shell', 'endpoint_shell', 'meta_receipt']:
            self.origin_items = copy.deepcopy(items)
            self.origin_items[1]['command'] = 'sh /tmp/plain-wrapper'
            if kind in ('mcp_guard', 'mcp_shell'):
                self.origin_items[1] = {'type': 'mcpToolCall', 'id': 'read-source', 'server': 'codex_app',
                                        'tool': 'get_usage_limits', 'status': 'completed', 'arguments': {}}
            if kind == 'meta_receipt':
                self.origin_items[1] = {'type': 'subAgentActivity', 'id': 'read-source',
                                        'kind': 'completed', 'status': 'completed'}
            self.origin_refresh(rebuild_audit=True)
            if kind == 'meta_receipt':
                self.origin_audit['interval']['subagent_completion_indices'] = [1]
            if kind == 'endpoint_shell':
                self.origin_audit['interval']['items'][0]['classification'] = 'read_only'
            else:
                self.origin_audit['interval']['items'][1]['classification'] = {
                    'middle_guard': 'guard_checkpoint', 'middle_receipt': 'read_receipt',
                    'shell_usage': 'formal_usage_read', 'mcp_guard': 'guard_checkpoint',
                    'mcp_shell': 'read_only', 'meta_receipt': 'read_receipt'}[kind]
            self.origin_refresh()
            with self.subTest(kind=kind):
                self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
                self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_origin_recovery_stop_to_receipt_accepts_only_reasoning_not_unknown_or_child_tools(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        original = copy.deepcopy(self.origin_items)
        for gap in [{'type': 'subAgentActivity', 'id': 'gap', 'kind': 'spawn'},
                    {'type': 'subAgentActivity', 'id': 'gap', 'kind': 'completed'},
                    {'type': 'functionCall', 'id': 'gap', 'name': 'opaque_write'},
                    {'type': 'fileChange', 'id': 'gap', 'status': 'completed'},
                    {'type': 'agentMessage', 'id': 'gap', 'text': 'not in allowed gap metadata'}]:
            self.origin_items = copy.deepcopy(original)
            self.origin_refresh(rebuild_audit=True)
            self.origin_items.insert(4, gap)
            self.origin_audit['interval']['items'][-1]['item_index_zero_based'] = 5
            self.origin_refresh()
            with self.subTest(gap_type=gap['type']):
                self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
                self.assertEqual(self.ledger_path.read_bytes(), before)
        self.origin_items = copy.deepcopy(original)
        self.origin_refresh(rebuild_audit=True)
        self.origin_items.insert(4, {'type': 'reasoning', 'id': 'gap', 'summary': []})
        self.origin_audit['interval']['items'][-1]['item_index_zero_based'] = 5
        self.origin_refresh()
        self.assertEqual(self.origin_gate()['reason'], 'origin_recovered_acceptance_required')

    def test_origin_recovery_rejects_audit_first_stop_reason_or_unconditional_claim_contradictions(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        original = copy.deepcopy(self.origin_audit['assessment'])
        for key, value in [('first_stop_in_current_turn_established', False),
                           ('first_stop_in_current_turn_established', None),
                           ('stop_reason_confirmed_by_item78', 'goal_paused'),
                           ('stop_reason_confirmed_by_item78', None),
                           ('first_stop_summary', 'continue active usage_budget_available'),
                           ('sufficient_for_unconditional_legacy_origin_clearance', True)]:
            self.origin_audit['assessment'] = dict(original, **{key: value})
            self.origin_refresh()
            with self.subTest(key=key, value=value):
                self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
                self.assertEqual(self.ledger_path.read_bytes(), before)

    def test_origin_recovery_rejects_audit_truncation_count_or_missing_event_metadata_contradictions(self):
        self.origin_prepared()
        before = self.ledger_path.read_bytes()
        original = copy.deepcopy(self.origin_audit['interval'])
        for key, value in [('command_truncations', 1), ('output_truncations', 1),
                           ('command_truncations', False), ('output_truncations', None),
                           ('strict_between_command_executions', 99),
                           ('strict_between_mcp_tool_calls', 99),
                           ('command_executions_including_endpoints', 99),
                           ('missing_output_fields', [1]), ('subagent_completion_indices', [1])]:
            self.origin_audit['interval'] = dict(original, **{key: value})
            self.origin_refresh()
            with self.subTest(key=key, value=value):
                self.assertEqual(self.origin_gate()['reason'], 'origin_recovery_evidence_invalid')
                self.assertEqual(self.ledger_path.read_bytes(), before)
        self.origin_audit['interval'] = original
        self.origin_refresh()
        self.assertEqual(self.origin_gate()['reason'], 'origin_recovered_acceptance_required')
        recorded = self.ledger()['goal_mismatch_origin']
        self.assertIs(recorded['validated_audit_assessment']['first_stop_in_current_turn_established'], True)
        self.assertEqual(recorded['validated_audit_assessment']['stop_reason_confirmed_by_item78'], 'goal_or_binding_changed')
        self.assertEqual(recorded['validated_interval']['command_truncations'], 0)
        self.assertEqual(recorded['validated_interval']['output_truncations'], 0)



if __name__ == '__main__':
    unittest.main()
