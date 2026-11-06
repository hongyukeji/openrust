"""只用临时项目、rollout与只读数据库验证原生 heartbeat 本地门禁。"""
import copy
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


if __name__ == '__main__':
    unittest.main()
