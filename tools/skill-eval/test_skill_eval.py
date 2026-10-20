#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""評価の道具の入出力の契約を確かめる（実装プラン P5-02）。"""
import copy
import contextlib
import http.client
import hashlib
import io
import json
import os
import shutil
import sys
import tempfile
import signal
import stat
import subprocess
import time
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mock_provider
import generate_compaction_materials as materials
from test_judge_sandbox import requires_sandbox
import skill_eval as evaluation


def setUpModule():
    # 準備の失敗経路や子プロセスも、本物のホームと設定を見ない。
    temporary = tempfile.TemporaryDirectory(prefix='skill-eval-suite-')
    unittest.addModuleCleanup(temporary.cleanup)
    values = {}
    for variable in ('HOME', 'XDG_CONFIG_HOME', 'XDG_STATE_HOME', 'TMPDIR', 'CLAUDE_CONFIG_DIR'):
        values[variable] = os.path.join(temporary.name, variable.lower())
        os.makedirs(values[variable])
    patch = mock.patch.dict(os.environ, values)
    patch.start()
    unittest.addModuleCleanup(patch.stop)


def event_output(reason='The run completed.', outcome='completed', calls=None):
    # 実装の生成関数から期待値を作らず、exec が出す公開の欄を手で組み立てる。
    events = [
        {'type': 'session_start', 'session_id': 'test-session', 'benitoiteh_version': '0.0.1',
         'system_prompt_sha256': 'prompt-hash', 'tools_sha256': 'tools-hash'},
        {'type': 'turn_start', 'turn': 1},
        {'type': 'text_delta', 'turn': 1, 'text': 'old response'},
        {'type': 'turn_start', 'turn': 2},
        {'type': 'text_delta', 'turn': 2, 'text': 'Do'},
        {'type': 'text_delta', 'turn': 2, 'text': 'ne.'},
        {'type': 'error', 'kind': 'old', 'message': 'ignore this earlier error'},
        {'type': 'error', 'kind': 'provider', 'message': 'failed ' + 'x' * 220},
        {'type': 'session_end', 'outcome': outcome},
        {'type': 'summary', 'reason': reason, 'turns': 2,
         'tool_calls': calls if calls is not None else {'write': 2, 'benitoite_run': 1},
         'after_write_checks': 2, 'checks': 5, 'fixes': 1, 'approvals': {'policy_denied': 3},
         'tokens': {'input': 3000, 'cache_read': 600, 'cache_write': 300, 'output': 150}},
    ]
    return '\n'.join(json.dumps(event) for event in events)


class OutputTests(unittest.TestCase):
    def test_end_classification_and_reasons(self):
        cases = [
            ('The run completed.', 'completed', 'completed'),
            ('The run reached the maximum number of turns.', 'failed', 'max_turns'),
            ('The run reached the maximum number of tokens.', 'failed', 'max_tokens'),
            ('The run stopped because checks of main.bnt failed 10 times without passing (limit: 10).',
             'failed', 'check_fix_limit'),
            ('The run was cancelled.', 'cancelled', 'cancelled'),
            ('The run stopped because benitoite_run, benitoite_test or benitoite_scratch was refused '
             '3 times in a row: the effects were outside --allow.', 'failed', 'effects_refused'),
            ('The run stopped because processes started by the script may still be running.',
             'failed', 'leftover_processes'),
            ('provider failed', 'failed', 'error'),
        ]
        for reason, outcome, end in cases:
            with self.subTest(end=end):
                out = 'not JSON\n[]\n' + event_output(reason, outcome) + '\n\n'
                fields, detail, final = evaluation.benitoiteh_output(out, 0 if outcome == 'completed' else 1)
                self.assertEqual(fields['end'], end)
                self.assertIn(reason, detail)
                if end == 'error':
                    self.assertIn('provider: failed ' + 'x' * 193, detail)
                    self.assertNotIn('x' * 194, detail)
                    self.assertNotIn('earlier error', detail)
                self.assertEqual(final, 'Done.')
                self.assertEqual(fields['session_id'], 'test-session')
                self.assertEqual(fields['benitoiteh_version'], '0.0.1')
                self.assertEqual(fields['system_prompt_sha256'], 'prompt-hash')
                self.assertEqual(fields['tools_sha256'], 'tools-hash')
        # outcome の分類は reason の分類より先に行う。
        fields, _, _ = evaluation.benitoiteh_output(event_output(evaluation.TOKEN_LIMIT), 0)
        self.assertEqual(fields['end'], 'completed')

    def test_missing_summary_and_last_nonempty_line(self):
        for out in ('', 'unreadable\n', '{"type":"session_end","outcome":"failed"}\n',
                    event_output() + '\nmalformed tail\n'):
            with self.subTest(out=out):
                fields, reason, _ = evaluation.benitoiteh_output(out, 2)
                self.assertEqual(fields['end'], 'error')
                self.assertEqual(reason, 'benitoiteh produced no summary (exit 2)')

    def test_counts_exclude_after_write_checks_and_do_not_add_cache_twice(self):
        calls = {'write': 5, 'edit': 3, 'read': 10, 'benitoite_check': 1, 'benitoite_run': 2,
                 'benitoite_test': 3, 'benitoite_scratch': 4}
        fields, _, _ = evaluation.benitoiteh_output(event_output(calls=calls), 0)
        self.assertEqual(fields['attempts'], 10)
        self.assertEqual(fields['after_write_checks'], 2)
        self.assertEqual(fields['checks'], 5)
        self.assertEqual(fields['fixes'], 1)
        self.assertEqual(fields['policy_denied'], 3)
        self.assertEqual([fields[key] for key in ('input_tokens', 'cache_read_tokens', 'cache_write_tokens',
                                                'output_tokens', 'total_tokens')], [3000, 600, 300, 150, 3150])
        # 最後の番に文がなければ前の番の文を final.txt に流用しない。
        out = event_output().rsplit('\n', 2)
        fields, _, final = evaluation.benitoiteh_output('\n'.join(
            [out[0], '{"type":"turn_start","turn":3}', out[1], out[2]]), 0)
        self.assertEqual(final, '')
        events = [json.loads(line) for line in event_output().splitlines()]
        events[-1]['tokens']['input'] = None
        fields, _, _ = evaluation.benitoiteh_output('\n'.join(json.dumps(event) for event in events), 0)
        self.assertIsNone(fields['total_tokens'])
        self.assertIsNone(evaluation.estimate_cost(fields, evaluation.load_harnesses()['benitoiteh-mock']))
        events[-1]['tokens']['input'] = '3000'
        with self.assertRaises(ValueError):
            evaluation.benitoiteh_output('\n'.join(json.dumps(event) for event in events), 0)


def dsh_events(missing=False, reason=None):
    return [
        {'type': 'session', 'sessionId': 'fake-session'},
        {'type': 'status', 'phase': 'turn_start'},
        {'type': 'status', 'phase': 'step_end',
         'usage': {'inputTokens': 100, 'outputTokens': 20, 'cacheReadTokens': 30, 'cacheWriteTokens': 10}},
        {'type': 'tool_call', 'tool': 'bash'},
        {'type': 'tool_call', 'tool': 'bash'},
        {'type': 'tool_call', 'tool': 'subagent_spawn'},
        dict(type='status', phase='step_end', **({} if missing else {'usage': {'inputTokens': 50}})),
        {'type': 'status', 'phase': 'turn_end', 'reason': reason or {'kind': 'completed'}},
        {'type': 'final', 'text': 'Done.'},
    ]


def claude_result(**extra):
    value = dict(type='result', subtype='success', is_error=False, num_turns=3, result='Done.',
                 total_cost_usd=12.34,
                 usage={'input_tokens': 100, 'output_tokens': 20,
                        'cache_read_input_tokens': 30, 'cache_creation_input_tokens': 10})
    value.update(extra)
    return value


class ExternalOutputTests(unittest.TestCase):
    def test_dsh_usage_turns_final_and_end_reasons(self):
        # step を番として数え、欠けた使用量を費用の確定値にしない契約。
        cases = [({'kind': 'completed'}, 0, 'completed', 'completed'),
                 ({'kind': 'completed'}, 1, 'error', 'completed'),
                 ({'kind': 'blocked'}, 0, 'error', 'blocked'),
                 ({'kind': 'error', 'error': {'code': 'AUTH', 'message': 'x' * 250}},
                  1, 'error', 'AUTH: ' + 'x' * 194)]
        for reason, code, end, detail in cases:
            for missing in (False, True):
                with self.subTest(reason=reason, missing=missing):
                    out = 'invalid\n[]\n' + '\n'.join(json.dumps(v) for v in dsh_events(missing, reason))
                    fields, actual, final = evaluation.external_output('dsh-ndjson', out, code)
                    self.assertEqual(fields['turns'], 2)
                    self.assertEqual(fields['dsh_turns'], 1)
                    self.assertNotIn('steps', fields)
                    self.assertEqual(fields['usage_missing'], int(missing))
                    self.assertEqual(fields['tool_calls'], {'bash': 2, 'subagent_spawn': 1})
                    self.assertEqual(fields['subagent_calls'], 1)
                    self.assertIn('usage excludes subagents', actual)
                    self.assertEqual([fields[k] for k in evaluation.TOKEN_FIELDS],
                                     [140, 30, 10, 20, 160] if missing else [190, 30, 10, 20, 210])
                    self.assertEqual(fields['end'], end)
                    self.assertIn(detail, actual)
                    self.assertNotIn('x' * 195, actual)
                    self.assertEqual(final, 'Done.')
        fields, detail, _ = evaluation.external_output('dsh-ndjson',
            '{"type":"error","message":"earlier"}\n' +
            json.dumps({'type': 'error', 'message': 'last ' + 'x' * 250}), 1)
        self.assertEqual(fields['end'], 'error')
        self.assertEqual(detail, 'error: last ' + 'x' * 195)
        self.assertIsNone(fields['total_tokens'])

    def test_claude_models_fallback_and_subtypes(self):
        models = {'one': {'inputTokens': 7, 'outputTokens': 2, 'cacheReadInputTokens': 3,
                          'cacheCreationInputTokens': 1}, 'two': {'inputTokens': 4, 'outputTokens': 5}}
        for subtype, is_error, end in (('success', False, 'completed'), ('success', True, 'error'),
                                        ('error_max_turns', True, 'max_turns'),
                                        ('error_during_execution', True, 'error')):
            for usage in ({}, {'modelUsage': models}):
                with self.subTest(subtype=subtype, usage=usage):
                    result = claude_result(subtype=subtype, is_error=is_error, **usage)
                    fields, detail, final = evaluation.external_output('claude-json',
                        'earlier output\n' + json.dumps(result) + '\n\n', 1)
                    self.assertEqual(fields['end'], end)
                    self.assertEqual(detail, subtype)
                    self.assertEqual(fields['turns'], 3)
                    self.assertIsNone(fields['tool_calls'])
                    self.assertEqual(fields['claude_reported_cost_usd'], 12.34)
                    self.assertNotIn('cost_usd', fields)
                    self.assertEqual(final, 'Done.')
                    self.assertEqual([fields[k] for k in evaluation.TOKEN_FIELDS],
                                     [15, 3, 1, 7, 22] if usage else [140, 30, 10, 20, 160])

    def test_unreadable_external_summaries_leave_tokens_unknown(self):
        cases = [('dsh-ndjson', ''), ('dsh-ndjson', '{"type":"session"}'),
                 ('dsh-ndjson', '\n'.join(json.dumps(v) for v in dsh_events(
                     reason={'kind': 'error', 'error': 'bad'}))),
                 ('dsh-ndjson', '{"type":"status","phase":"step_end","usage":[]}'),
                 ('claude-json', json.dumps(claude_result()) + '\ninvalid'),
                 ('claude-json', json.dumps(claude_result(modelUsage=[]))),
                 ('claude-json', json.dumps(claude_result(usage={'input_tokens': -1}))),
                 ('claude-json', json.dumps(claude_result(num_turns=True)))]
        for output, out in cases:
            with self.subTest(output=output, out=out):
                fields, reason, _ = evaluation.external_output(output, out, 0)
                self.assertEqual(reason, 'harness summary unreadable')
                self.assertIsNone(fields['end'])
                self.assertTrue(all(fields[k] is None for k in evaluation.TOKEN_FIELDS))


class SessionTests(unittest.TestCase):
    def test_diagnostics_and_safety_from_session_and_spilled_records(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-session-test-') as directory:
            os.mkdir(os.path.join(directory, 'spill'))
            def write(name, text):
                with open(os.path.join(directory, name), 'w', encoding='utf-8') as f:
                    f.write(text)
            def result(name, text, **extra):
                return dict(type='tool_result', name=name,
                            result={'id': name, 'text': text, 'is_error': False}, **extra)
            write('spill/output', 'error[E9999] not counted\ncomponents with errors: 2\n')
            check = result('benitoite_check', 'error[E0405] main.bnt:2:13: failed\n')
            check['result']['id'] = '呼び出し'
            encoded = json.dumps(check, ensure_ascii=False).encode('utf-8')
            # 部分の境界で UTF-8 の文字が分かれても、元の JSON の値を復元する。
            boundary = encoded.index('呼'.encode('utf-8')) + 1
            for name, part in (('spill/part1', encoded[:boundary]), ('spill/part2', encoded[boundary:])):
                with open(os.path.join(directory, name), 'wb') as f:
                    f.write(part)
            records = [
                {'type': 'tool_call', 'call': {'id': '呼び出し', 'arguments': {'path': 'main.bnt'}}},
                {'type': 'spilled', 'parts': ['spill/part1', 'spill/part2']},
                result('write', 'error[E9998] before check\nCheck of main.bnt after this change:\n'
                       'warning[W0401] main.bnt:2:13: warning\n'),
                result('benitoite_run', 'error[R0101] main.bnt:2:13: failed', spill={'path': 'spill/output'}),
            ]
            write('session.jsonl', '\n'.join(json.dumps(record) for record in records))
            entries, seen = evaluation.benitoiteh_results(directory)
            self.assertTrue(seen)
            self.assertEqual([entry[4:] for entry in entries],
                             [['check', 'main.bnt'], ['check', 'main.bnt'], ['run', '']])
            stats = evaluation.diagnostic_stats(entries)
            self.assertEqual(stats['counts'], {'E0405': 1, 'W0401': 1, 'R0101': 1})
            self.assertEqual(stats['first_check'], ['E0405'])
            self.assertEqual(stats['resolved'], {'E0405': 1, 'W0401': 1})

    # 関門: 文だけの検査報告、結果だけの行、入力エラーの呼び出しを実行として数えない。
    def test_completion_metrics_use_exact_environment_prompt_and_valid_calls(self):
        with open(os.path.join(evaluation.REPO, 'crates/benitoiteh/tests/expected/completion-check.txt'),
                  encoding='utf-8') as f:
            prompt = f.read().rstrip()
        def call(name, error=None):
            return dict(type='tool_call', call=dict(id=name, name=name, arguments={}), validation_error=error)
        marker = dict(type='user', kind='environment', message=dict(parts=[dict(kind='environment', text=prompt)]))
        with tempfile.TemporaryDirectory(prefix='skill-eval-completion-') as directory:
            for present in (False, True):
                records = [call('benitoite_check'), call('benitoite_run'),
                           dict(type='assistant', response=dict(text='I ran all tests.')),
                           dict(marker, kind='request'),
                           dict(marker, message=dict(parts=[dict(kind='environment', text='other environment information')]))]
                if present:
                    records.append(marker)
                records += [call('benitoite_test'), call('benitoite_scratch'), call('write'), call('edit'),
                            call('benitoite_check', 'bad args'), call('write', 'bad args'),
                            dict(type='tool_result', name='benitoite_run', result=dict(id='unused', text='cancelled'))]
                # 退避された呼び出しの行も、通常の行と同じ集計に入る。
                with open(os.path.join(directory, 'part'), 'w', encoding='utf-8') as f:
                    json.dump(records.pop(-5), f)
                records.append(dict(type='spilled', parts=['part']))
                with open(os.path.join(directory, 'session.jsonl'), 'w', encoding='utf-8') as f:
                    f.write('\n'.join(json.dumps(r) for r in records) + '\n')
                metrics = {}
                evaluation.benitoiteh_results(directory, metrics)
                self.assertEqual(metrics, dict(completion_prompts=int(present), edits_after_prompt=2 if present else 0,
                    repository_path_seen=False,
                    verification_calls_before={'benitoite_check': 1, 'benitoite_run': 1,
                                               'benitoite_test': 0 if present else 1, 'benitoite_scratch': 0 if present else 1},
                    verification_calls_after={'benitoite_check': 0, 'benitoite_run': 0,
                                              'benitoite_test': 1 if present else 0, 'benitoite_scratch': 1 if present else 0}))

    def test_shell_line_is_distinct_from_a_source_diagnostic(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-safety-test-') as directory:
            cases = [
                ('benitoite_run', 'components with errors: 2\n', None, True),
                ('benitoite_test', 'components with errors: 0\n', None, True),
                ('benitoite_check', 'error[E0001]\nConsole.writeLine("components with errors: ${n}")', None, False),
                ('benitoite_scratch', '[stderr]\n', 'components with errors: 3\n', True),
                ('read', 'components with errors: 2\n', None, False),
            ]
            for name, text, stderr, expected in cases:
                with self.subTest(name=name):
                    record = {'type': 'tool_result', 'name': name, 'result': {'id': 'id', 'text': text}}
                    if stderr:
                        with open(os.path.join(directory, 'stderr'), 'w', encoding='utf-8') as f:
                            f.write(stderr)
                        record['stderr_spill'] = {'path': 'stderr'}
                    with open(os.path.join(directory, 'session.jsonl'), 'w', encoding='utf-8') as f:
                        f.write(json.dumps(record))
                    _, seen = evaluation.benitoiteh_results(directory)
                    self.assertEqual(seen, expected)


class ConfigTests(unittest.TestCase):
    # 関門: 比較用の値が別の項目や提供者の表に入る退行と、有料の起動前に
    # 誤った設定を拒めない退行を、項目の読み取りと生成する TOML の境界で守る。
    def test_evaluation_variants_config_generation_and_header(self):
        harnesses = evaluation.load_harnesses()
        base = harnesses['benitoiteh-deepseek']
        for suffix, difference in (
            ('no-after-write-check', {'config': {'restricted': {'after_write_check': False}}}),
            ('no-line-numbers', {'config': {'read': {'line_numbers': False}}}),
            ('completion-check', {'config': {'agent': {'completion_check': True}}}),
            ('effort-low', {'effort': 'low'}),
            ('effort-max', {'effort': 'max'}),
        ):
            with self.subTest(suffix=suffix):
                harness = harnesses['benitoiteh-deepseek-' + suffix]
                self.assertEqual(harness, dict(base, verified=False, default=False, **difference))
                text = evaluation.benitoiteh_config(harness, harness['model'], '/processor')
                self.assertIn('effort = "%s"' % harness['effort'], text)
        for suffix, reserve, keep in (('base', 16384, 20000), ('reserve', 8192, 20000), ('keep', 16384, 8000)):
            harness = harnesses['benitoiteh-deepseek-compact-' + suffix]
            provider = dict(base['provider'], context_window=65536, max_output_tokens=8192)
            self.assertEqual(harness, dict(base, provider=provider, verified=False, default=False,
                                          config={'compaction': {'reserve_tokens': reserve, 'keep_tokens': keep}}))
            text = evaluation.benitoiteh_config(harness, harness['model'], '/processor')
            self.assertIn('context_window = 65536\n', text)
            self.assertIn('max_output_tokens = 8192\n', text)
        harness = copy.deepcopy(base)
        completion = evaluation.benitoiteh_config(
            harnesses['benitoiteh-deepseek-completion-check'], base['model'], '/processor')
        self.assertIn('\n[agent]\ncompletion_check = true\n', completion)
        harness['config'] = {'restricted': {'after_write_check': False}, 'read': {'line_numbers': False}}
        text = evaluation.benitoiteh_config(harness, harness['model'], '/processor')
        self.assertTrue(text.endswith('[restricted]\nallowed_programs = ["/usr/bin/sort"]\n'
                                      'after_write_check = false\n\n[read]\nline_numbers = false\n\n[skills]\nuser = false\n\n[plugins]\nuser = false\n'))
        with tempfile.TemporaryDirectory(prefix='skill-eval-config-header-') as directory:
            processor = os.path.join(directory, 'processor')
            with open(processor, 'wb') as f:
                f.write(b'processor')
            args = evaluation.argparse.Namespace(dry_run=True, model=None, trials=1, skill_dir=None)
            for item, expected in ((harness, '{"read": {"line_numbers": false}, "restricted": {"after_write_check": false}}'),
                                   (base, '-')):
                with mock.patch.object(evaluation, 'version_of', return_value='test-version'):
                    header = evaluation.evaluation_header(args, processor, 'test', item, None, directory,
                                                          [{'trial_dir': directory}])
                self.assertEqual(header['config'], expected)
                self.assertEqual(header['completion_check_sha256'],
                                 '65e20f220a2700e09b7ae6b7f8e2861c6b9d9fbf73bc05c80b420511317e216e')

    def test_invalid_evaluation_config_is_rejected_while_loading(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-invalid-config-') as directory:
            path = os.path.join(directory, 'harnesses.json')
            for kind, config in (
                ('external', {}),
                ('external', {'agent': {'completion_check': True}}),
                ('benitoiteh', {'agents': {'completion_check': True}}),
                ('benitoiteh', {'agent': {'completion_checks': True}}),
                ('benitoiteh', {'agent': {'completion_check': 1}}),
                ('benitoiteh', {'agent': {'completion_check': 'true'}}),
                ('benitoiteh', {'agent': []}),
                ('benitoiteh', None),
                ('benitoiteh', []),
                ('benitoiteh', {'normal': {}}),
                ('benitoiteh', {'read': False}),
                ('benitoiteh', {'read': {'after_write_check': False}}),
                ('benitoiteh', {'restricted': {'line_numbers': False}}),
                ('benitoiteh', {'restricted': {'allowed_programs': []}}),
                ('benitoiteh', {'read': {'line_numbers': 0}}),
                ('benitoiteh', {'read': {'line_numbers': 1}}),
                ('benitoiteh', {'read': {'line_numbers': 'false'}}),
                ('benitoiteh', {'restricted': {'after_write_check': None}}),
                *[('benitoiteh', {'compaction': {key: value}})
                  for key in ('reserve_tokens', 'keep_tokens')
                  for value in (True, False, -1, 2**64, 1.5, '1', None)],
                ('benitoiteh', {'compaction': {'reserve': 1}}),
            ):
                with self.subTest(kind=kind, config=config):
                    with open(path, 'w', encoding='utf-8') as f:
                        json.dump({'test': {'kind': kind, 'mock': True, 'config': config}}, f)
                    with mock.patch.object(evaluation, 'HARNESSES_FILE', path), self.assertRaises(ValueError):
                        evaluation.load_harnesses()

    def test_compaction_and_provider_integer_bounds_are_validated_before_dry_run(self):
        base = evaluation.load_harnesses()['benitoiteh-deepseek']
        with tempfile.TemporaryDirectory(prefix='sc-') as directory:
            path = os.path.join(directory, 'harnesses.json')
            for key, values in (('context_window', (0, True, -1, 2**64, 1.5, '3', None)),
                                ('max_output_tokens', (0, False, -1, 2**64, None))):
                for value in values:
                    harness = copy.deepcopy(base)
                    harness['provider'][key] = value
                    with open(path, 'w') as f:
                        json.dump({'test': harness}, f)
                    with self.subTest(key=key, value=value), mock.patch.object(evaluation, 'HARNESSES_FILE', path), \
                            self.assertRaises(ValueError):
                        evaluation.load_harnesses()
            for value in (0, 2**64 - 1):
                harness = copy.deepcopy(base)
                harness['provider'].update(context_window=65536, max_output_tokens=8192)
                harness['config'] = {'compaction': {'reserve_tokens': value, 'keep_tokens': value}}
                with open(path, 'w') as f:
                    json.dump({'test': harness}, f)
                with mock.patch.object(evaluation, 'HARNESSES_FILE', path):
                    loaded = evaluation.load_harnesses()['test']
                text = evaluation.benitoiteh_config(loaded, 'model', '/processor')
                self.assertIn('context_window = 65536\n', text)
                self.assertIn('max_output_tokens = 8192\n', text)
                self.assertIn('[compaction]\nkeep_tokens = %d\nreserve_tokens = %d\n' % (value, value), text)

    def test_config_and_rejected_values(self):
        harness = evaluation.load_harnesses()['benitoiteh-mock']
        text = evaluation.benitoiteh_config(harness, 'override-model', '/trial/real/benitoite', 1234)
        self.assertIn('[benitoite]\npath = "/trial/real/benitoite"', text)
        self.assertIn('allowed_programs = ["/usr/bin/sort"]', text)
        self.assertIn('[providers.eval]\n', text)
        self.assertTrue(text.endswith('\n[skills]\nuser = false\n\n[plugins]\nuser = false\n'))
        self.assertIn('context_window = 200000', text)
        self.assertIn('model = "override-model"', text)
        self.assertIn('base_url = "http://127.0.0.1:1234"', text)
        self.assertIn('api_key = { env = "SKILL_EVAL_MOCK_KEY" }', text)
        for value in ('bad"path', 'bad\\path', 'bad\npath', 'bad\x7fpath', '日本語'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                evaluation.benitoiteh_config(harness, 'mock', value, 1234)
        for key in ({'value': 'secret'}, {'env': 'KEY', 'value': 'secret'}, 'secret', {'command': []}):
            invalid = copy.deepcopy(harness)
            invalid['provider']['api_key'] = key
            with self.subTest(key=key), self.assertRaises(ValueError):
                evaluation.benitoiteh_config(invalid, 'mock', '/processor', 1234)
        command_key = copy.deepcopy(harness)
        command_key['provider']['api_key'] = {'command': ['/usr/bin/security', 'find-generic-password']}
        self.assertIn('api_key = { command = ["/usr/bin/security", "find-generic-password"] }',
                      evaluation.benitoiteh_config(command_key, 'mock', '/processor', 1234))

    def test_markdown_and_jsonl_records(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-record-test-') as directory:
            row = {'task': 'example', 'harness': 'mock', 'kind': 'benitoiteh', 'model': 'mock',
                   'success': True, 'reason': 'ok', 'attempts': 1, 'trial_dir': '/trial/1', 'final_codes': [],
                   'diagnostics': evaluation.diagnostic_stats([])}
            path = os.path.join(directory, 'new', 'custom.md')
            evaluation.write_record(path, {}, [row], [])
            with open(os.path.splitext(path)[0] + '.jsonl', encoding='utf-8') as f:
                self.assertEqual([json.loads(line) for line in f], [row])
            with open(path, encoding='utf-8') as f:
                self.assertIn('no diagnostic codes were found in benitoiteh tool results', f.read())


class MockProviderTests(unittest.TestCase):
    def start_server(self, kind, completion_check=False):
        try:
            server, base = mock_provider.start(kind, 'main.bnt', 'source text', completion_check=completion_check)
        except OSError as error:
            self.skipTest('loopback listening is unavailable: %s' % error)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return server

    def request(self, server, has_result=True, prompt=None, messages=None):
        connection = http.client.HTTPConnection('127.0.0.1', server.server_address[1], timeout=5)
        try:
            content = [{'type': 'tool_result', 'tool_use_id': 'call', 'content': 'ok'}] if has_result else 'hello'
            if prompt is not None:
                content = [{'type': 'text', 'text': prompt}]
            body = json.dumps({'stream': True, 'messages': messages if messages is not None
                               else [{'role': 'user', 'content': content}]})
            connection.request('POST', '/v1/messages', body=body, headers={'Content-Type': 'application/json'})
            response = connection.getresponse()
            data = response.read()
            self.assertEqual(int(response.getheader('Content-Length')), len(data))
            return response.status, data.decode('utf-8')
        finally:
            connection.close()

    def test_staged_summary_keeps_the_scenario_position_and_reports_fixed_usage(self):
        # 要約の応答が段の筋書きを消費すると、実際の再開の前に模擬が終わってしまう。
        try:
            server, _base = mock_provider.start('staged', 'main.bnt', 'source text',
                stages=[dict(prompt='Stage %d' % n, materials=['materials/%d/example.md' % n])
                        for n in range(1, 4)], input_tokens=45000)
        except OSError as error:
            self.skipTest('loopback listening is unavailable: %s' % error)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        def block(prompt=None):
            status, text = self.request(server, prompt=prompt)
            self.assertEqual(status, 200, text)
            events = [json.loads(line[6:]) for line in text.splitlines() if line.startswith('data: ')]
            return events[1]['content_block'], events[0]['message']['usage']['input_tokens']
        for n in range(1, 4):
            response, tokens = block('Stage %d' % n)
            self.assertEqual(response['input'], {'path': 'materials/%d/example.md' % n})
            self.assertEqual(tokens, 45000)
            response, tokens = block(server.summary_prompt)
            self.assertEqual(response['type'], 'text')
            self.assertEqual(tokens, 1000)
            if n > 1:
                response, _tokens = block()
                self.assertEqual(response['name'], 'read')
                self.assertEqual(response['input']['path'], 'main.bnt')
            response, _tokens = block()
            self.assertEqual(response['name'], 'write')
            response, _tokens = block()
            self.assertEqual(response['name'], 'benitoite_run')
            response, _tokens = block()
            self.assertEqual(response['text'], 'Done with stage %d.' % n)
        self.assertEqual(server.summary_requests, 3)
        self.assertEqual(server.problems, [])

    def test_three_responses_and_unexpected_fourth(self):
        for kind in ('run', 'test'):
            with self.subTest(kind=kind):
                server = self.start_server(kind)
                for number in (1, 2, 3):
                    status, body = self.request(server)
                    self.assertEqual(status, 200)
                    events = [json.loads(line[6:]) for line in body.splitlines() if line.startswith('data: ')]
                    self.assertEqual(events[0]['message']['usage'], {
                        'input_tokens': 1000, 'output_tokens': 0, 'cache_read_input_tokens': 0,
                        'cache_creation_input_tokens': 0})
                    self.assertEqual(events[-2]['usage']['output_tokens'], 50)
                    self.assertEqual(events[-1]['type'], 'message_stop')
                    block = events[1]['content_block']
                    if number == 1:
                        self.assertEqual(block['name'], 'write')
                        self.assertEqual(block['input'], {'path': 'main.bnt', 'content': 'source text'})
                    elif number == 2:
                        self.assertEqual(block['name'], 'benitoite_run' if kind == 'run' else 'benitoite_test')
                        self.assertEqual(block['input'], {'path': 'main.bnt'} if kind == 'run'
                                         else {'paths': ['main.bnt']})
                    else:
                        self.assertEqual(block, {'type': 'text', 'text': 'Done.'})
                    self.assertEqual(events[-2]['delta']['stop_reason'], 'end_turn' if number == 3 else 'tool_use')
                self.assertEqual(server.problems, [])
                self.assertEqual(self.request(server)[0], 500)
                self.assertIn('unexpected request 4', server.problems[0])

    def test_completion_scenario_verifies_after_the_prompt_then_finishes(self):
        with open(os.path.join(evaluation.REPO, 'crates/benitoiteh/tests/expected/completion-check.txt'),
                  encoding='utf-8') as f:
            prompt = f.read().rstrip()
        for kind in ('run', 'test'):
            with self.subTest(kind=kind):
                server = self.start_server(kind, completion_check=True)
                for number in range(1, 6):
                    status, body = self.request(server, prompt=prompt if number == 4 else None)
                    self.assertEqual(status, 200)
                    events = [json.loads(line[6:]) for line in body.splitlines() if line.startswith('data: ')]
                    if number == 4:
                        self.assertEqual(events[1]['content_block']['name'],
                                         'benitoite_run' if kind == 'run' else 'benitoite_test')
                    if number in (3, 5):
                        self.assertEqual(events[1]['content_block'], {'type': 'text', 'text': 'Done.'})
                self.assertEqual(server.problems, [])
                self.assertEqual(self.request(server)[0], 500)

    def test_completion_scenario_rejects_malformed_messages_without_disconnect(self):
        for message in (None, 42, [], {'role': 'user', 'content': [{'type': 'text', 'text': 42}]}):
            with self.subTest(message=message):
                server = self.start_server('run', completion_check=True)
                for _ in range(3):
                    self.assertEqual(self.request(server)[0], 200)
                self.assertEqual(self.request(server, messages=[message])[0], 400)
                self.assertEqual(server.problems, ['request 4 has no completion prompt'])

    def test_missing_tool_result(self):
        server = self.start_server('run')
        self.assertEqual(self.request(server, False)[0], 200)
        self.assertEqual(self.request(server, False)[0], 400)
        self.assertEqual(self.request(server, False)[0], 400)
        self.assertEqual(len(server.problems), 2)
        self.assertTrue(all('no final user tool_result' in problem for problem in server.problems))

    def test_invalid_content_lengths(self):
        for length in (None, '-1', str(8 * 1024 * 1024 + 1)):
            with self.subTest(length=length):
                server = self.start_server('run')
                connection = http.client.HTTPConnection('127.0.0.1', server.server_address[1], timeout=2)
                try:
                    connection.putrequest('POST', '/v1/messages')
                    if length is not None:
                        connection.putheader('Content-Length', length)
                    connection.endheaders()
                    response = connection.getresponse()
                    self.assertEqual(response.status, 400)
                    response.read()
                finally:
                    connection.close()


class RecordsAndCostTests(unittest.TestCase):
    def test_environment_removes_keys_and_lists_only_removed_names(self):
        parent = dict(HOME='/home/test', PATH='/bin', CLAUDECODE='secret', CLAUDE_CODE_SESSION_ID='secret',
                      CLAUDE_CONFIG_DIR='/config', ANTHROPIC_API_KEY='secret', ANTHROPIC_OTHER='secret',
                      DEEPSEEK_API_KEY='secret', GEMINI_API_KEY='secret', GOOGLE_API_KEY='secret',
                      OPENAI_API_KEY='secret')
        with mock.patch.dict(os.environ, parent, clear=True):
            env, removed = evaluation.harness_environment()
        self.assertEqual(env, {'HOME': '/home/test', 'PATH': '/bin', 'CLAUDE_CONFIG_DIR': '/config'})
        self.assertEqual(removed, sorted(set(parent) - set(env)))

    def test_real_provider_configs_reference_keys_and_declare_prices(self):
        for name in ('deepseek', 'gemini', 'opus'):
            with self.subTest(name=name):
                harness = evaluation.load_harnesses()['benitoiteh-' + name]
                self.assertFalse(harness['verified'])
                self.assertFalse(harness['default'])
                self.assertNotIn('mock', harness)
                self.assertEqual(set(harness['provider']['api_key']), {'command'})
                self.assertEqual(set(harness['price']), {'input', 'cache_read', 'cache_write', 'output'})
                self.assertIn(harness['billing'], ('deepseek', 'google', 'anthropic'))
                config = evaluation.benitoiteh_config(harness, harness['model'], '/processor')
                if name == 'gemini':
                    self.assertIn('capabilities = { reasoning_effort = true }', config)

    def test_cost_counts_cached_input_once_and_clamps_uncached_input(self):
        harness = {'price': {'input': 4, 'cache_read': .2, 'cache_write': 5, 'output': 20}}
        tokens = dict(input_tokens=1000000, cache_read_tokens=200000, cache_write_tokens=100000,
                      output_tokens=50000)
        self.assertAlmostEqual(evaluation.estimate_cost(tokens, harness), 4.34)
        tokens['input_tokens'] = 0
        self.assertAlmostEqual(evaluation.estimate_cost(tokens, harness), 1.54)
        tokens['output_tokens'] = None
        self.assertIsNone(evaluation.estimate_cost(tokens, harness))
        self.assertIsNone(evaluation.estimate_cost(tokens, {}))

    def test_timeout_preserves_output_and_kills_after_grace(self):
        for ignore in (False, True):
            with self.subTest(ignore=ignore), tempfile.TemporaryDirectory(prefix='skill-eval-timeout-') as directory:
                out = os.path.join(directory, 'stdout')
                err = os.path.join(directory, 'stderr')
                script = ('import signal, time, sys\n'
                          'def stop(*args):\n print("summary", flush=True)\n sys.exit(130)\n'
                          'signal.signal(signal.SIGTERM, %s)\n'
                          'print("ready", flush=True)\n'
                          'print("error output", file=sys.stderr, flush=True)\n'
                          'while True: time.sleep(.01)\n') % ('signal.SIG_IGN' if ignore else 'stop')
                original_wait = subprocess.Popen.wait
                ready = [False]
                def wait(process, timeout=None):
                    # 子がハンドラを置いてから上限の待機を始め、遅い機械でも競合させない。
                    if not ready[0]:
                        deadline = time.monotonic() + 5
                        while time.monotonic() < deadline:
                            with open(out, encoding='utf-8') as f:
                                if 'ready' in f.read():
                                    ready[0] = True
                                    break
                            time.sleep(.01)
                        self.assertTrue(ready[0], 'child did not become ready')
                    return original_wait(process, timeout)
                with mock.patch.object(subprocess.Popen, 'wait', wait):
                    code, timed_out, wall = evaluation.run_harness(
                        [sys.executable, '-c', script], directory, dict(os.environ), .05, out, err, grace=.05)
                self.assertTrue(timed_out)
                self.assertEqual(code, -signal.SIGKILL if ignore else 130)
                self.assertFalse(os.path.exists(os.path.join(directory, 'harness.pgid')))
                self.assertGreater(wall, 0)
                with open(out, encoding='utf-8') as f:
                    self.assertEqual(f.read(), 'ready\n' if ignore else 'ready\nsummary\n')
                with open(err, encoding='utf-8') as f:
                    self.assertIn('error output', f.read())

    def run_main(self, directory, trial, names='benitoiteh-mock', extra=None):
        executable = os.path.join(directory, 'version')
        with open(executable, 'w', encoding='utf-8') as f:
            f.write('#!%s\nprint("test version")\n' % sys.executable)
        os.chmod(executable, 0o755)
        # 本物の起動を差し替える。集計と記録は main から本物の処理を通す（P5-03 の継ぎ目）。
        with mock.patch.object(evaluation, 'run_trial', side_effect=trial), \
                mock.patch.object(evaluation, 'RESULTS_DIR', os.path.join(directory, 'results')), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as err:
            handlers = {sig: signal.getsignal(sig) for sig in (signal.SIGTERM, signal.SIGHUP)}
            try:
                status = evaluation.main(['--harness', names, '--allow-unverified', '--task', 'json_summary',
                                          '--benitoite', executable, '--benitoiteh', executable,
                                          '--work-root', os.path.join(directory, 'work'), '--trials', '3'] + (extra or []))
            finally:
                self.assertEqual({sig: signal.getsignal(sig) for sig in handlers}, handlers)
        return status, err.getvalue()

    def row(self, trial_dir, harness, cost):
        skill = os.path.join(trial_dir, 'workspace', '.agents', 'skills', 'benitoite')
        os.makedirs(skill)
        with open(os.path.join(skill, 'SKILL.md'), 'w', encoding='utf-8') as f:
            f.write('benitoite-version: 0.0.7\n')
        return dict(task='json_summary', harness=harness, model='mock', kind='benitoiteh', success=True,
                    attempts=1, after_write_checks=2, trial_dir=trial_dir, reason='ok ' + trial_dir,
                    diagnostics=evaluation.diagnostic_stats([]), final_codes=[], log_exists=True,
                    total_tokens=3150, cache_read_tokens=600, wall_seconds=1.2, turns=3,
                    cost_usd=cost, end='completed', benitoiteh_version='session version',
                    system_prompt_sha256='prompt-hash', tools_sha256='tools-hash')

    def test_cost_limit_across_harnesses_and_redaction_in_all_records(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-budget-') as directory:
            calls = []
            def trial(task, trial_dir, processor, name, *args):
                calls.append(name)
                row = self.row(trial_dir, name, 100 if name.endswith('mock') else .25)
                row['reason'] += ' ' + evaluation.REPO + ' ' + os.environ['HOME'] + '/private.txt'
                row['nested'] = {'paths': [trial_dir, os.path.realpath(trial_dir)]}
                return row
            # 同じ提供者の別項目を設け、evaluate をまたぐ累計と none の除外を一度に確かめる。
            harnesses = evaluation.load_harnesses()
            harnesses['second'] = copy.deepcopy(harnesses['benitoiteh-deepseek'])
            harnesses['second']['billing'] = 'deepseek'
            with mock.patch.object(evaluation, 'load_harnesses', return_value=harnesses):
                status, err = self.run_main(directory, trial,
                                            'benitoiteh-mock,benitoiteh-gemini,benitoiteh-deepseek,second,benitoiteh-opus',
                                            ['--stop-cost', '1', '--skill-dir',
                                             os.path.join(directory, 'work', 'private-skill')])
            self.assertEqual(status, 3)
            self.assertEqual(calls, ['benitoiteh-mock'] * 3 + ['benitoiteh-gemini'] * 3
                             + ['benitoiteh-deepseek'] * 3 + ['second'])
            self.assertIn('stopped: estimated cost reached 1 USD for deepseek', err)
            results = os.path.join(directory, 'results')
            self.assertEqual(len(os.listdir(results)), 10)
            for name in os.listdir(results):
                with open(os.path.join(results, name), encoding='utf-8') as f:
                    text = f.read()
                for prefix in (os.path.join(directory, 'work'), evaluation.REPO, os.environ['HOME']):
                    self.assertNotIn(prefix, text)
                if name.endswith('.md'):
                    self.assertIn('<work-root>', text)
                    self.assertIn('--skill-dir <work-root>/private-skill', text)
            self.assertTrue(os.path.isabs(str(evaluation.ALL_ROWS[0]['trial_dir'])))
            overall = next(name for name in os.listdir(results) if name.endswith('-overall.md'))
            with open(os.path.join(results, overall), encoding='utf-8') as f:
                self.assertIn('stopped: estimated cost reached 1 USD for deepseek', f.read())

    # 関門: 条件を課題・反復ごとに揃え、同じ提供者の累計と途中の実行順を失わない。
    def test_interleave_rotation_partial_records_and_shared_cost_stop(self):
        names = 'benitoiteh-deepseek,benitoiteh-deepseek-completion-check'
        for stop, width in (('none', 2), ('none', 3), ('cost', 2), ('interrupt', 2), ('interrupt-first', 2)):
            selected = names + ',benitoiteh-deepseek-no-line-numbers' if width == 3 else names
            with self.subTest(stop=stop, width=width), tempfile.TemporaryDirectory(prefix='skill-eval-interleave-') as directory:
                calls = []
                def trial(task, trial_dir, processor, name, *args):
                    calls.append((task.name, int(os.path.basename(trial_dir)), name))
                    if stop.startswith('interrupt') and len(calls) == (1 if stop == 'interrupt-first' else 4):
                        raise KeyboardInterrupt()
                    row = self.row(trial_dir, name, .25)
                    row['task'] = task.name
                    return row
                extra = ['--interleave', '--task', 'file_process']
                if stop == 'cost':
                    extra += ['--stop-cost', '0.5']
                if stop.startswith('interrupt'):
                    with self.assertRaises(KeyboardInterrupt):
                        self.run_main(directory, trial, selected, extra)
                else:
                    status, _ = self.run_main(directory, trial, selected, extra)
                    self.assertEqual(status, 3 if stop == 'cost' else 0)
                expected = []
                a, b = names.split(',')
                c = 'benitoiteh-deepseek-no-line-numbers'
                orders = ((1, (a,b)), (2, (b,a)), (3, (a,b))) if width == 2 else ((1, (a,b,c)), (2, (b,c,a)), (3, (c,a,b)))
                task_orders = (orders, ((1, (b,a)), (2, (a,b)), (3, (b,a)))) if width == 2 else (orders, orders)
                for task, orders_for_task in zip(('json_summary', 'file_process'), task_orders):
                    for repetition, order in orders_for_task:
                        expected += [(task, repetition, name) for name in order]
                launched = {'cost': 2, 'interrupt': 4, 'interrupt-first': 1}.get(stop, 6 * width)
                self.assertEqual(calls, expected[:launched])
                completed = launched - int(stop.startswith('interrupt'))
                self.assertEqual([r['condition_order'] for r in evaluation.ALL_ROWS], list(range(1, completed + 1)))
                self.assertEqual([r['condition_position'] for r in evaluation.ALL_ROWS],
                                 (list(range(1, width + 1)) * 6)[:completed])
                overall = next(name for name in os.listdir(os.path.join(directory, 'results')) if name.endswith('-overall.md'))
                with open(os.path.join(directory, 'results', overall), encoding='utf-8') as f:
                    text = f.read()
                self.assertIn('起動順', text)
                if stop.startswith('interrupt'):
                    self.assertIn("'completed': False", text)
                with open(os.path.join(directory, 'results', overall.replace('.md', '.jsonl')), encoding='utf-8') as f:
                    self.assertEqual(len(f.readlines()), completed)

    def test_partial_records_survive_interrupt_and_exception_and_next_main_is_fresh(self):
        for exception, failure_at in ((KeyboardInterrupt, 2), (RuntimeError, 2), (RuntimeError, 4), (RuntimeError, 1)):
            with self.subTest(exception=exception, failure_at=failure_at), \
                    tempfile.TemporaryDirectory(prefix='skill-eval-interrupt-') as directory:
                calls = []
                def trial(task, trial_dir, processor, name, *args):
                    calls.append(name)
                    if len(calls) == failure_at:
                        raise exception('interrupted')
                    return self.row(trial_dir, name, None)
                with self.assertRaises(exception):
                    self.run_main(directory, trial, 'benitoiteh-deepseek,benitoiteh-opus')
                results = os.path.join(directory, 'results')
                if failure_at == 1:
                    self.assertFalse(os.path.exists(results))
                    self.assertEqual(evaluation.ALL_ROWS, [])
                    continue
                self.assertEqual(len(os.listdir(results)), 4)
                for name in os.listdir(results):
                    if name.endswith('.md'):
                        with open(os.path.join(results, name), encoding='utf-8') as f:
                            self.assertIn('cost unknown: %d trials' % (failure_at - 1), f.read())
                self.assertEqual(len(evaluation.ALL_ROWS), failure_at - 1)

    def test_record_redaction_aliases_home_boundary_and_safety_warning(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-redaction-') as directory:
            alias = os.path.join(directory, 'alias')
            real = os.path.join(directory, 'real')
            os.mkdir(real)
            os.symlink(real, alias)
            row = self.row(os.path.join(real, '1'), 'mock', 0)
            home = os.path.join(directory, 'home')
            row.update(shell_output_seen=True, end='timeout', reason=home + ' ' + home + '/x ' + home + '-other')
            row['nested'] = [alias + '/file', {'real': real + '/file', 'home': home}]
            original = copy.deepcopy(row)
            path = os.path.join(directory, 'record.md')
            with mock.patch.dict(os.environ, HOME=home):
                evaluation.write_record(path, {'Skill': alias}, [row], [evaluation.Task('json_summary')], alias)
            self.assertEqual(row, original)
            with open(path, encoding='utf-8') as f:
                text = f.read()
            self.assertIn('SAFETY: shell output seen in json_summary trial 1', text)
            self.assertIn('~ ~/x ', text)
            self.assertIn('/home-other', text)
            self.assertIn('| json_summary | 1 / 1 | 1 | 1.2 | 3150 | 1 |', text)
            with open(os.path.splitext(path)[0] + '.jsonl', encoding='utf-8') as f:
                saved = json.loads(f.readline())
            self.assertEqual(saved['nested'], ['<work-root>/file', {'real': '<work-root>/file', 'home': '~'}])

    def test_private_aliases_home_punctuation_temporary_paths_and_residual_username(self):
        for prefix in ('var', 'tmp', 'etc'):
            with self.subTest(prefix=prefix):
                public = '/%s/x/real' % prefix
                private = '/private' + public
                for root in (public, private):
                    paths = evaluation.path_prefixes(root)
                    self.assertIn(public, paths)
                    self.assertIn(private, paths)
        with mock.patch.dict(os.environ, HOME='/Users/alice'), \
                mock.patch.object(tempfile, 'gettempdir', return_value='/private/var/example/T'):
            value = ["got '/Users/alice'", 'cannot start /Users/alice: error', '(/Users/alice)',
                     '/Users/alice/child', '/else/alice/file', '/Users/alice-other',
                     '/var/example/T/file', '/private/var/example/T/file',
                     '/var/x/real/file', '/private/var/x/real/file']
            self.assertEqual(evaluation.redact_record(value, '/private/var/x/real'),
                             ["got '~'", 'cannot start ~: error', '(~)', '~/child', '/else/<user>/file',
                              '/Users/alice-other', '<tmp>/file', '<tmp>/file',
                              '<work-root>/file', '<work-root>/file'])

    def test_unknown_paid_cost_stops_after_first_trial_and_billing_is_required(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-unknown-cost-') as directory:
            calls = []
            def trial(task, trial_dir, processor, name, *args):
                calls.append(name)
                return self.row(trial_dir, name, None)
            status, err = self.run_main(directory, trial, 'benitoiteh-deepseek,benitoiteh-gemini',
                                        ['--stop-cost', '0'])
            self.assertEqual(status, 3)
            self.assertEqual(calls, ['benitoiteh-deepseek'])
            self.assertIn('stopped: cost unknown for deepseek', err)
            for name in os.listdir(os.path.join(directory, 'results')):
                if name.endswith('.md'):
                    with open(os.path.join(directory, 'results', name), encoding='utf-8') as f:
                        self.assertIn('stopped: cost unknown for deepseek', f.read())
            config = os.path.join(directory, 'harnesses.json')
            for harness in ({'kind': 'benitoiteh'}, {'price': {'input': 1}}):
                with self.subTest(harness=harness):
                    with open(config, 'w', encoding='utf-8') as f:
                        json.dump({'paid': harness}, f)
                    with mock.patch.object(evaluation, 'HARNESSES_FILE', config), self.assertRaises(ValueError):
                        evaluation.load_harnesses()

    def test_termination_signals_stop_harness_group_and_save_completed_rows(self):
        for sig in (signal.SIGTERM, signal.SIGHUP):
            with self.subTest(sig=sig), tempfile.TemporaryDirectory(prefix='skill-eval-signal-') as directory:
                heartbeat = os.path.join(directory, 'heartbeat')
                grandchild = os.path.join(directory, 'grandchild')
                beat = ('import os, time\n'
                        'while True:\n with open(%r, "a") as f: f.write(str(os.getpgrp()) + "\\n")\n time.sleep(.02)\n')
                child = ('import subprocess, sys\nsubprocess.Popen([sys.executable, "-c", %r])\n' %
                         (beat % grandchild)) + beat % heartbeat
                driver = ('import os, sys\nfrom unittest import mock\n'
                          'sys.path.insert(0, %r)\nimport skill_eval as e\nfrom test_skill_eval import RecordsAndCostTests\n'
                          'count = 0\n'
                          'def trial(task, trial_dir, processor, name, *args):\n'
                          ' global count\n count += 1\n'
                          ' if count == 2:\n'
                          '  e.run_harness([sys.executable, "-c", %r], trial_dir, dict(os.environ), 60, '
                          'os.path.join(trial_dir, "harness.stdout"), os.path.join(trial_dir, "harness.stderr"), grace=.1)\n'
                          ' return RecordsAndCostTests().row(trial_dir, name, 0)\n'
                          'with mock.patch.object(e, "run_trial", side_effect=trial):\n'
                          ' RecordsAndCostTests().run_main(%r, trial)\n') % (evaluation.HERE, child, directory)
                env = dict(os.environ)
                for variable in ('XDG_CONFIG_HOME', 'XDG_STATE_HOME', 'TMPDIR'):
                    env[variable] = os.path.join(directory, variable)
                    os.mkdir(env[variable])
                pgid_path = os.path.join(directory, 'work', 'json_summary', '2', 'harness.pgid')
                pgid = None
                with open(os.path.join(directory, 'driver.stderr'), 'wb') as err:
                    process = subprocess.Popen([sys.executable, '-I', '-c', driver], env=env,
                                               stdout=subprocess.DEVNULL, stderr=err, start_new_session=True)
                    try:
                        deadline = time.monotonic() + 10
                        while time.monotonic() < deadline:
                            if all(os.path.isfile(path) and os.path.getsize(path) > 0
                                   for path in (heartbeat, grandchild, pgid_path)):
                                break
                            if process.poll() is not None:
                                break
                            time.sleep(.01)
                        if os.path.isfile(pgid_path):
                            with open(pgid_path, encoding='utf-8') as f:
                                pgid = int(f.read())
                        self.assertTrue(os.path.isfile(grandchild), 'harness did not start')
                        process.send_signal(sig)
                        process.wait(timeout=10)
                        sizes = [os.path.getsize(path) for path in (heartbeat, grandchild)]
                        time.sleep(.15)
                        self.assertEqual(sizes, [os.path.getsize(path) for path in (heartbeat, grandchild)])
                        self.assertFalse(os.path.exists(pgid_path))
                        records = os.path.join(directory, 'results')
                        with open(os.path.join(records, next(name for name in os.listdir(records)
                                                            if name.endswith('.jsonl'))), encoding='utf-8') as f:
                            self.assertEqual(len(f.readlines()), 1)
                    finally:
                        if process.poll() is None:
                            process.kill()
                        process.wait()
                        if pgid is None and os.path.isfile(heartbeat):
                            with open(heartbeat, encoding='utf-8') as f:
                                pgid = int(f.readline())
                        if pgid is not None:
                            try:
                                os.killpg(pgid, signal.SIGKILL)
                            except (ProcessLookupError, PermissionError):
                                pass

    @unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
    @requires_sandbox
    def test_judged_script_cannot_read_parent_session_or_provider_key(self):
        task = evaluation.Task('http_client')
        source = ('import Benitoite.Unofficial.IO.Console\nimport Benitoite.Unofficial.IO.Process\n'
                  'function main() -> Result[Unit, String] uses Process.Environment, Console.Write\n')
        for number, name in enumerate(('CLAUDE_CODE_SESSION_ID', 'ANTHROPIC_API_KEY')):
            source += ('  bind value%d <- try Process.environmentVariable("%s") |> Result.mapError(r: _, '
                       'f: lambda(error) return IOError.message(error) end lambda) or return\n'
                       '  Console.writeLine(Option.unwrapOr(o: value%d, fallback: "absent"))\n') % (number, name, number)
        source += '  return Result.Ok(())\nend function\n'
        with mock.patch.dict(os.environ, CLAUDE_CODE_SESSION_ID='parent-session-secret',
                             ANTHROPIC_API_KEY='parent-provider-secret'):
            verdict = evaluation.judge(task, source, os.environ['BENITOITEH_TEST_BENITOITE'], None)
        self.assertNotIn('parent-session-secret', verdict.reason)
        self.assertNotIn('parent-provider-secret', verdict.reason)
        self.assertIn('absent', verdict.reason)

    def test_interrupt_stops_running_child_and_start_failure_is_recorded(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-stop-') as directory:
            out, err = [os.path.join(directory, name) for name in ('stdout', 'stderr')]
            original_wait = subprocess.Popen.wait
            processes = []
            def interrupt(process, timeout=None):
                if not processes:
                    processes.append(process)
                    raise KeyboardInterrupt()
                return original_wait(process, timeout)
            with mock.patch.object(subprocess.Popen, 'wait', interrupt), self.assertRaises(KeyboardInterrupt):
                evaluation.run_harness([sys.executable, '-c', 'import time; time.sleep(60)'],
                                       directory, dict(os.environ), 60, out, err, grace=.05)
            self.assertIsNotNone(processes[0].returncode)
            self.assertEqual(evaluation.run_harness(['/no/such/harness'], directory, dict(os.environ),
                                                    60, out, err), (-2, False, 0.0))
            with open(err, encoding='utf-8') as f:
                self.assertIn('cannot start /no/such/harness', f.read())
            # 群れへの合図が拒まれても、先頭の子は直接止めて回収する。
            with mock.patch.object(os, 'killpg', side_effect=PermissionError):
                code, timed_out, _ = evaluation.run_harness(
                    [sys.executable, '-c', 'import time; time.sleep(2)'], directory, dict(os.environ),
                    .05, out, err, grace=.05)
            self.assertTrue(timed_out)
            self.assertEqual(code, -signal.SIGKILL)

    @unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
    @requires_sandbox
    def test_unreadable_session_fails_only_one_trial_and_retains_cost(self):
        cases = [({'type': 'tool_result', 'name': 'benitoite_run',
                   'result': {'id': 'id', 'text': 'ok'}, 'spill': {'path': 'spill/missing'}}, 'FileNotFoundError'),
                 ({'type': 'tool_result', 'name': 'benitoite_run', 'result': {'id': 'id', 'text': None}}, 'TypeError'),
                 ({'type': 'spilled', 'parts': 'bad'}, 'TypeError'),
                 ({'type': 'tool_call'}, 'KeyError'),
                 ({'type': 'tool_result', 'name': 'benitoite_run', 'result': {'id': 'id', 'text': 'ok'},
                   'spill': {'path': '../outside'}}, 'ValueError')]
        cases += [(['not an object'], 'TypeError')]
        for message in (None, [], {'parts': None}, {'parts': 'wrong'}, {'parts': [None]},
                        {'parts': [{'kind': 'environment', 'text': None}]}):
            cases.append((dict(type='user', kind='environment', message=message), 'TypeError'))
        cases.append((dict(type='user', kind='environment', message=dict(parts=[
            dict(kind='environment', text='<completion_check>\nolder prompt\n</completion_check>')])),
                      'CompletionPromptMismatch'))
        for record, error in cases:
            with self.subTest(record=record, error=error), tempfile.TemporaryDirectory(prefix='skill-eval-bad-session-') as directory:
                task = evaluation.Task('json_summary')
                with open(task.path('reference.bnt'), encoding='utf-8') as f:
                    source = f.read()
                executable = os.path.join(directory, 'fake-benitoiteh')
                script = ('#!%s\nimport os, json\n'
                          'from pathlib import Path\n'
                          'assert all(k == "CLAUDE_CONFIG_DIR" or not k.startswith(("CLAUDE", "ANTHROPIC")) '
                          'for k in os.environ)\n'
                          'assert not {"DEEPSEEK_API_KEY", "GEMINI_API_KEY", "GOOGLE_API_KEY", "OPENAI_API_KEY"}'
                          '.intersection(os.environ)\n'
                          'assert "HOME" in os.environ and "PATH" in os.environ\n'
                          'Path("main.bnt").write_text(%r)\n'
                          'session = Path(os.environ["XDG_STATE_HOME"]) / "benitoiteh/sessions/test-session"\n'
                          'session.mkdir(parents=True)\n'
                          'record = %r if Path.cwd().parent.name == "1" else None\n'
                          'session.joinpath("session.jsonl").write_text(json.dumps(record) if record else "")\n'
                          'print(%r)\n') % (sys.executable, source, record, event_output())
                with open(executable, 'w', encoding='utf-8') as f:
                    f.write(script)
                os.chmod(executable, 0o755)
                harness = copy.deepcopy(evaluation.load_harnesses()['benitoiteh-mock'])
                harness.pop('mock')
                path = os.path.join(directory, 'record.md')
                with mock.patch.dict(os.environ, HOME=directory, CLAUDECODE='test-parent', ANTHROPIC_API_KEY='test-key',
                                     OPENAI_API_KEY='test-key', DEEPSEEK_API_KEY='test-key',
                                     GEMINI_API_KEY='test-key', GOOGLE_API_KEY='test-key'), \
                        mock.patch.object(evaluation, 'load_harnesses', return_value={'fake': harness}), \
                        contextlib.redirect_stdout(io.StringIO()):
                    status = evaluation.main(['--harness', 'fake', '--task', task.name, '--trials', '2',
                                              '--benitoite', os.environ['BENITOITEH_TEST_BENITOITE'],
                                              '--benitoiteh', executable, '--work-root', os.path.join(directory, 'work'),
                                              '--record', path])
                self.assertEqual(status, 0)
                with open(os.path.splitext(path)[0] + '.jsonl', encoding='utf-8') as f:
                    rows = [json.loads(line) for line in f]
                self.assertEqual([row['success'] for row in rows], [False, True])
                self.assertEqual(rows[0]['reason'], 'unreadable session record: ' + error)
                self.assertEqual(rows[0]['total_tokens'], 3150)
                self.assertEqual(rows[0]['cost_usd'], 0)
                with open(path, encoding='utf-8') as f:
                    self.assertIn('| json_summary | 1 / 2 |', f.read())




    # 関門: HOME の書き換えで秘密の拒否と資格情報の探索を変える退行を、
    # 実際の子プロセスの環境で捕まえる。本物のホーム・提供者・キーチェーンは使わない。
    @unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
    @requires_sandbox
    def test_benitoiteh_preserves_parent_home_for_execution_and_version_queries(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-home-') as directory:
            home = os.path.join(directory, 'parent-home')
            os.mkdir(home)
            task = evaluation.Task('json_summary')
            with open(task.path('reference.bnt'), encoding='utf-8') as f:
                source = f.read()
            executable = os.path.join(directory, 'fake-benitoiteh')
            script = ('#!%s\nimport os, sys\nfrom pathlib import Path\n'
                      'assert os.environ["HOME"] == %r\n'
                      'if "--version" in sys.argv:\n print("fake version")\n sys.exit(0)\n'
                      'Path("main.bnt").write_text(%r)\n'
                      'print(%r)\n') % (sys.executable, home, source, event_output())
            with open(executable, 'w', encoding='utf-8') as f:
                f.write(script)
            os.chmod(executable, 0o755)
            harness = copy.deepcopy(evaluation.load_harnesses()['benitoiteh-mock'])
            harness.pop('mock')
            root = os.path.join(directory, 'trial')
            with mock.patch.dict(os.environ, HOME=home):
                row = evaluation.run_trial(task, root, os.environ['BENITOITEH_TEST_BENITOITE'],
                                           'fake', harness, 'mock', None, False, executable)
                self.assertTrue(row['success'], row['reason'])
                # 通常はイベントの版を使う。欠けた場合の版の問い合わせも同じ HOME を保つ。
                args = evaluation.argparse.Namespace(dry_run=False, model=None, trials=1, skill_dir=None,
                                                     benitoiteh=executable)
                header = evaluation.evaluation_header(args, os.environ['BENITOITEH_TEST_BENITOITE'],
                    'fake', harness, None, root, [dict(row, benitoiteh_version=None)])
            self.assertEqual(header['ハーネス'], 'fake（fake version）')


class ExternalPreparationTests(unittest.TestCase):
    def test_claude_user_configuration_is_rejected_by_name(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-claude-config-') as directory:
            config = os.path.join(directory, '.claude')
            os.makedirs(os.path.join(config, 'skills', 'synced', 'bundle'))
            with open(os.path.join(config, 'skills', 'synced', 'bundle', 'manifest.json'), 'w') as f:
                json.dump({'skills': [{'name': 'standard-skill', 'description': 'do not record this'}]}, f)
            workspace = os.path.join(directory, 'workspace')
            os.mkdir(workspace)
            with mock.patch.dict(os.environ, HOME=directory, CLAUDE_CONFIG_DIR=config):
                self.assertEqual(evaluation.check_claude_config(workspace), ['standard-skill'])
                for relative in ('skills/custom', 'agents/reviewer.md', 'commands/custom.md',
                                 'output-styles/custom.md', 'CLAUDE.md'):
                    with self.subTest(relative=relative):
                        path = os.path.join(config, relative)
                        os.makedirs(os.path.dirname(path), exist_ok=True)
                        os.symlink('/no/such/target', path)
                        try:
                            with self.assertRaisesRegex(ValueError, relative):
                                evaluation.check_claude_config(workspace)
                        finally:
                            os.unlink(path)
                settings = os.path.join(config, 'settings.json')
                for value, name in (({'enabledPlugins': {'custom': True}}, 'enabledPlugins'),
                                    ({'hooks': {}}, 'hooks'), ({'outputStyle': 'custom'}, 'outputStyle'),
                                    ({'apiKeyHelper': 'private-setting-value'}, 'apiKeyHelper'),
                                    ({'env': {'ANTHROPIC_API_KEY': 'private-setting-value'}}, 'env'),
                                    ({'permissions': {'allow': ['private-setting-value']}}, 'permissions.allow'),
                                    ({'permissions': {'allow': []}}, 'permissions.allow'),
                                    ([], 'settings.json')):
                    with self.subTest(value=value):
                        with open(settings, 'w') as f:
                            json.dump(value, f)
                        with self.assertRaisesRegex(ValueError, name) as caught:
                            evaluation.check_claude_config(workspace)
                        self.assertNotIn('private-setting-value', str(caught.exception))
                with open(settings, 'w') as f:
                    json.dump({'enabledPlugins': {'custom': False}}, f)
                self.assertEqual(evaluation.check_claude_config(workspace), ['standard-skill'])
                for name in ('CLAUDE.md', 'CLAUDE.local.md'):
                    path = os.path.join(directory, name)
                    with open(path, 'w') as f:
                        f.write('private instructions')
                    try:
                        with self.assertRaisesRegex(ValueError, name):
                            evaluation.check_claude_config(workspace)
                    finally:
                        os.unlink(path)
            # HOME の既定と CLAUDE_CONFIG_DIR の優先を、それぞれ実際のディレクトリで確かめる。
            parent = dict(os.environ, HOME=directory)
            parent.pop('CLAUDE_CONFIG_DIR', None)
            with mock.patch.dict(os.environ, parent, clear=True):
                self.assertEqual(evaluation.check_claude_config(workspace), ['standard-skill'])
            with mock.patch.dict(os.environ, HOME='/no/such/home', CLAUDE_CONFIG_DIR=config):
                self.assertEqual(evaluation.check_claude_config(workspace), ['standard-skill'])

    def test_dsh_refuses_git_ancestors_and_invalid_overlay_values(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-dsh-guard-') as directory, \
                mock.patch.dict(os.environ, HOME=directory):
            os.mkdir(os.path.join(directory, 'repo'))
            with open(os.path.join(directory, 'repo', '.git'), 'w') as f:
                f.write('gitdir: /irrelevant')
            os.symlink(os.path.join(directory, 'repo'), os.path.join(directory, 'alias'))
            harness = evaluation.load_harnesses()['deepseek-harness']
            for name in ('repo', 'alias'):
                with self.subTest(name=name), self.assertRaisesRegex(ValueError, '.git ancestor'):
                    evaluation.run_trial(evaluation.Task('json_summary'), os.path.join(directory, name, 'trial'),
                        '/no/processor', 'fake', harness, 'deepseek-flash', None, False, dsh_key='synthetic')
            for model, effort in (('bad_model', 'high'), ('bad\nmodel', 'high'), ('日本語', 'high'),
                                  ('flash', 'medium'), ('flash', 'HIGH'), ('flash', '')):
                with self.subTest(model=model, effort=effort), self.assertRaisesRegex(ValueError, 'model or effort'):
                    evaluation.dsh_overlay(model, effort)
            overlay = evaluation.dsh_overlay('flash-1.0', 'off')
            self.assertIn('model: "flash-1.0"', overlay)
            self.assertIn('reasoningEffort: "off"', overlay)

    def test_work_root_under_shared_tmp_is_rejected_after_resolving_links(self):
        with tempfile.TemporaryDirectory(prefix='skill-eval-work-root-') as directory:
            alias = os.path.join(directory, 'alias')
            os.symlink('/tmp', alias)
            # 試行を 0 回にし、修正前でも共有の /tmp にファイルを作らせない。
            for root in ('/tmp', '/tmp/evaluation', '/private/tmp', '/private/tmp/evaluation', alias):
                with self.subTest(root=root), contextlib.redirect_stdout(io.StringIO()):
                    with self.assertRaises(SystemExit) as caught:
                        evaluation.main(['--dry-run', '--task', 'json_summary', '--trials', '0',
                                         '--benitoite', sys.executable, '--work-root', root])
                    self.assertEqual(str(caught.exception),
                                     '--work-root must not be under /tmp (harnesses can write there)')


@unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
class ExternalTrialTests(unittest.TestCase):
    def setUp(self):
        if any(os.path.lexists(os.path.join(parent, '.git'))
               for parent in evaluation.ancestors(tempfile.gettempdir())):
            self.skipTest('temporary directory has a .git ancestor')
        self.temporary = tempfile.TemporaryDirectory(prefix='skill-eval-external-')
        self.addCleanup(self.temporary.cleanup)
        self.directory = self.temporary.name
        self.home = os.path.join(self.directory, 'home')
        os.mkdir(self.home)
        # 実際の利用者の設定とキーには触れず、すべての起動を一時領域に向ける。
        parent = dict(os.environ, HOME=self.home, CLAUDE_CONFIG_DIR=os.path.join(self.home, '.claude'),
                      XDG_CONFIG_HOME=os.path.join(self.directory, 'config'),
                      XDG_STATE_HOME=os.path.join(self.directory, 'state'), TMPDIR=self.directory,
                      DSH_PERMISSION_MODE='danger-full-access', DSH_SYSTEM_PROMPT='private',
                      DSH_TOOLS_MODE='ptc', DEEPSEEK_BASE_URL='https://invalid', NODE_OPTIONS='--invalid',
                      ANTHROPIC_API_KEY='private-anthropic')
        patch = mock.patch.dict(os.environ, parent, clear=True)
        patch.start()
        self.addCleanup(patch.stop)
        self.key = 'synthetic-deepseek-key-P5-04'
        self.counter = os.path.join(self.directory, 'key-reads')
        self.key_command = self.executable('key-command',
            'import os\nfrom pathlib import Path\n'
            'assert not any(n.startswith(("DSH_", "DEEPSEEK_", "ANTHROPIC", "NODE_")) '
            'for n in os.environ)\n'
            'with open(%r, "a") as f: f.write("read\\n")\nprint(%r)\n' % (self.counter, self.key))

    def executable(self, name, source):
        path = os.path.join(self.directory, name)
        with open(path, 'w', encoding='utf-8') as f:
            f.write('#!%s\n%s' % (sys.executable, source))
        os.chmod(path, 0o755)
        return path

    def run_external(self, output, text, trials=2, extra=None, leak=None, timed=False, after_check=''):
        task = evaluation.Task('json_summary')
        with open(task.path('reference.bnt'), encoding='utf-8') as f:
            source = f.read()
        script = ('import os, sys, subprocess\nfrom pathlib import Path\n'
                  'if "--version" in sys.argv:\n print("fake external 1.0")\n sys.exit(0)\n'
                  'trial = Path.cwd().parent\n'
                  'assert sys.argv[1] == str(trial)\n'
                  'Path("main.bnt").write_text(%r)\n') % source
        if output == 'dsh-ndjson':
            script += ('assert os.environ["DEEPSEEK_API_KEY"] == %r\n'
                       'assert os.environ["DSH_HOME"] == str(trial / "dsh-home")\n'
                       'assert os.environ["DSH_AGENTS_HOME"] == str(trial / "agents-home")\n'
                       'assert os.environ["DSH_TELEMETRY_DISABLED"] == "1"\n'
                       'assert os.environ["TMPDIR"] == str(trial / "tmp")\n'
                       'assert os.environ["SKILL_EVAL_LOG"] == str(trial / "tmp/benitoite.log")\n'
                       'assert list((trial / "dsh-home").iterdir()) == []\n'
                       'assert list((trial / "agents-home").iterdir()) == []\n'
                       'assert not any(n in os.environ for n in ["DSH_PERMISSION_MODE", "DSH_SYSTEM_PROMPT", '
                       '"DSH_TOOLS_MODE", "DEEPSEEK_BASE_URL", "NODE_OPTIONS", "ANTHROPIC_API_KEY"])\n'
                       'assert not any(n.startswith("NODE_") for n in os.environ)\n'
                       'assert (trial / "bin").parent != Path(os.environ["TMPDIR"])\n'
                       'assert (trial / "real").parent != Path(os.environ["TMPDIR"])\n') % self.key
        else:
            script += 'assert "ANTHROPIC_API_KEY" not in os.environ\n'
        script += ('child = {n: v for n, v in os.environ.items() if n != "DEEPSEEK_API_KEY"}\n'
                   'subprocess.run(["benitoite", "check", "main.bnt"], env=child, check=True, '
                   'stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)\n')
        script += after_check
        if leak:
            script += ('key = os.environ["DEEPSEEK_API_KEY"]\n'
                       'target = trial / %r\n'
                       'target.parent.mkdir(parents=True, exist_ok=True)\n'
                       'target.write_text(key)\n'
                       'print(key, file=sys.stderr)\n') % leak
        script += 'print(%r)\n' % text
        if timed:
            script += ('import signal, time\nsys.stdout.flush()\n'
                       'signal.signal(signal.SIGTERM, lambda *args: sys.exit(0))\n'
                       'while True: time.sleep(.01)\n')
        executable = self.executable('fake-harness', script)
        name = 'deepseek-harness' if output == 'dsh-ndjson' else 'claude-code'
        harness = copy.deepcopy(evaluation.load_harnesses()[name])
        harness.update(start=[executable, '{trial}'], cwd='{trial}/workspace', version=[executable, '--version'])
        if timed:
            harness['options']['harness_timeout'] = 1
        if output == 'dsh-ndjson':
            harness['api_key']['command'] = [self.key_command]
        record = os.path.join(self.directory, 'record.md')
        with mock.patch.object(evaluation, 'load_harnesses', return_value={name: harness}), \
                contextlib.redirect_stdout(io.StringIO()) as stdout, \
                contextlib.redirect_stderr(io.StringIO()) as stderr:
            status = evaluation.main(['--harness', name, '--allow-unverified', '--task', task.name,
                '--benitoite', os.environ['BENITOITEH_TEST_BENITOITE'], '--trials', str(trials),
                '--work-root', os.path.join(self.directory, 'work'), '--record', record] + (extra or []))
        with open(record, encoding='utf-8') as f:
            markdown = f.read()
        with open(os.path.splitext(record)[0] + '.jsonl', encoding='utf-8') as f:
            jsonl = f.read()
        return status, [json.loads(line) for line in jsonl.splitlines()], markdown, jsonl, stdout.getvalue(), stderr.getvalue()

    @requires_sandbox
    def test_dsh_preparation_key_once_cost_and_records(self):
        events = [v for v in dsh_events() if v.get('tool') != 'subagent_spawn']
        with mock.patch.dict(os.environ, NODE_TLS_REJECT_UNAUTHORIZED='0',
                             NODE_EXTRA_CA_CERTS='/private/custom-certs'):
            status, rows, markdown, jsonl, stdout, stderr = self.run_external(
                'dsh-ndjson', '\n'.join(json.dumps(v) for v in events))
        self.assertEqual(status, 0)
        self.assertEqual(len(rows), 2)
        self.assertTrue(all(row['success'] and row['end'] == 'completed' and row['attempts'] == 1 for row in rows))
        self.assertAlmostEqual(rows[0]['cost_usd'], .00007218)
        self.assertNotIn('usage excludes subagents', rows[0]['reason'])
        self.assertIn('parent session only', markdown)
        self.assertIn('DSH_PERMISSION_MODE', markdown)
        self.assertIn('NODE_TLS_REJECT_UNAUTHORIZED', markdown)
        self.assertIn('NODE_EXTRA_CA_CERTS', markdown)
        self.assertNotIn('/private/custom-certs', markdown)
        self.assertNotIn('danger-full-access', markdown)
        with open(self.counter) as f:
            self.assertEqual(f.read(), 'read\n')
        for index in (1, 2):
            trial = os.path.join(self.directory, 'work/json_summary', str(index))
            self.assertTrue(os.path.isdir(os.path.join(trial, 'dsh-home')))
            for name in ('final.txt', 'dsh-eval-overlay.yml'):
                with open(os.path.join(trial, name)) as f:
                    text = f.read()
                self.assertNotIn(self.key, text)
                if name == 'final.txt':
                    self.assertEqual(text, 'Done.')
                else:
                    self.assertIn('model: "deepseek-flash"', text)
                    self.assertIn('reasoningEffort: "high"', text)
                    for part in ('session-log-deepseek\n  config:\n    enabled: false',
                                 'plugin-package-inventory-deepseek\n  disabled: true',
                                 'tool-web\n  disabled: true', 'session-title-llm\n  disabled: true'):
                        self.assertIn(part, text)
        for text in (markdown, jsonl, stdout, stderr):
            self.assertNotIn(self.key, text)

    @requires_sandbox
    def test_dsh_missing_usage_stops_cost_budget_without_changing_verdict(self):
        events = [v for v in dsh_events(missing=True) if v.get('tool') != 'subagent_spawn']
        status, rows, markdown, _, _, stderr = self.run_external(
            'dsh-ndjson', '\n'.join(json.dumps(v) for v in events), extra=['--stop-cost', '10'])
        self.assertEqual(status, 3)
        self.assertEqual(len(rows), 1)
        self.assertTrue(rows[0]['success'])
        self.assertIsNone(rows[0]['cost_usd'])
        self.assertEqual(rows[0]['usage_missing'], 1)
        self.assertIn('cost unknown: 1 trials', markdown)
        self.assertIn('stopped: cost unknown for deepseek', stderr)

    @requires_sandbox
    def test_subagent_and_workflow_cost_is_unknown_and_stops_budget(self):
        for tool in ('subagent_spawn', 'workflow_run'):
            with self.subTest(tool=tool):
                events = [v for v in dsh_events() if v.get('type') != 'tool_call']
                events.insert(2, {'type': 'tool_call', 'tool': tool})
                status, rows, markdown, _, _, stderr = self.run_external(
                    'dsh-ndjson', '\n'.join(json.dumps(v) for v in events), extra=['--stop-cost', '10'])
                self.assertEqual(status, 3)
                self.assertEqual(len(rows), 1)
                self.assertEqual(rows[0]['subagent_calls'], 1)
                self.assertEqual(rows[0]['tool_calls'], {tool: 1})
                self.assertTrue(rows[0]['success'])
                self.assertEqual(rows[0]['total_tokens'], 210)
                self.assertIsNone(rows[0]['cost_usd'])
                self.assertIn('usage excludes subagents', rows[0]['reason'])
                self.assertIn('cost unknown: 1 trials', markdown)
                self.assertIn('stopped: cost unknown for deepseek', stderr)

    @requires_sandbox
    def test_leak_scan_covers_tmp_and_agents_home_without_following_links(self):
        for relative in ('tmp/dsh-spill-large-result', 'agents-home/output'):
            with self.subTest(relative=relative):
                status, rows, markdown, jsonl, stdout, stderr = self.run_external(
                    'dsh-ndjson', '\n'.join(json.dumps(v) for v in dsh_events()), trials=1, leak=relative,
                    after_check='os.symlink(%r, trial / "workspace/key-link")\n'
                                'os.mkfifo(trial / "tmp/pipe")\n' % self.key_command)
                self.assertEqual(status, 0)
                self.assertEqual(len(rows), 1)
                self.assertIn('SAFETY: DeepSeek key value found in ' + relative, rows[0]['reason'])
                self.assertNotIn('workspace/key-link', rows[0]['reason'])
                for value in (markdown, jsonl, stdout, stderr):
                    self.assertNotIn(self.key, value)

    @unittest.skipIf(hasattr(os, 'geteuid') and os.geteuid() == 0, 'root can read chmod 000 files')
    @requires_sandbox
    def test_unscannable_file_does_not_discard_trial(self):
        status, rows, markdown, jsonl, _, _ = self.run_external(
            'dsh-ndjson', '\n'.join(json.dumps(v) for v in dsh_events()), trials=2,
            after_check='blocked = trial / "dsh-home/unreadable"\n'
                        'blocked.write_text("unreadable output")\nblocked.chmod(0)\n')
        self.assertEqual(status, 0)
        self.assertEqual(len(rows), 2)
        for row in rows:
            self.assertIn('SAFETY: unscannable dsh-home/unreadable', row['reason'])
        self.assertIn('SAFETY: unscannable dsh-home/unreadable', markdown)
        self.assertIn('SAFETY: unscannable dsh-home/unreadable', jsonl)

    @requires_sandbox
    def test_modified_processor_copy_is_rejected_and_judgment_uses_original(self):
        text = '\n'.join(json.dumps(v) for v in dsh_events())
        with self.subTest(unchanged=True):
            with mock.patch.object(evaluation, 'judge_trial_script', wraps=evaluation.judge_trial_script) as judge:
                status, rows, _, _, _, _ = self.run_external('dsh-ndjson', text, trials=1)
            self.assertTrue(rows[0]['success'])
            self.assertEqual(status, 0)
            self.assertEqual(judge.call_args.args[2], os.environ['BENITOITEH_TEST_BENITOITE'])
        marker = os.path.join(self.directory, 'modified-processor-was-run')
        replacement = '#!%s\nfrom pathlib import Path\nPath(%r).touch()\n' % (sys.executable, marker)
        status, rows, markdown, _, _, _ = self.run_external(
            'dsh-ndjson', text, trials=1,
            after_check='(trial / "real/benitoite").write_text(%r)\n' % replacement)
        self.assertEqual(status, 0)
        self.assertFalse(rows[0]['success'])
        self.assertTrue(rows[0]['reason'].startswith('SAFETY: the processor copy was modified'))
        self.assertIn('SAFETY: the processor copy was modified', markdown)
        self.assertFalse(os.path.exists(marker))

    @requires_sandbox
    def test_claude_cost_is_separate_and_synced_names_are_recorded(self):
        bundle = os.path.join(self.home, '.claude/skills/synced/standard')
        os.makedirs(bundle)
        with open(os.path.join(bundle, 'manifest.json'), 'w') as f:
            json.dump({'skills': [{'name': 'standard-skill', 'description': 'private description'}]}, f)
        result = claude_result(usage={'input_tokens': 2000000, 'output_tokens': 1})
        status, rows, markdown, jsonl, _, _ = self.run_external(
            'claude-json', json.dumps(result), extra=['--stop-cost', '0'])
        self.assertEqual(status, 0)
        self.assertEqual(len(rows), 2)
        self.assertTrue(all(row['success'] and row['over_token_cap'] for row in rows))
        self.assertIsNone(rows[0]['cost_usd'])
        self.assertEqual(rows[0]['claude_reported_cost_usd'], 12.34)
        self.assertIn('over the 2M token cap', rows[0]['reason'])
        self.assertIn('利用者設定の確認: passed', markdown)
        self.assertIn('standard-skill', markdown)
        self.assertNotIn('private description', markdown + jsonl)
        self.assertFalse(os.path.exists(self.counter))

    @requires_sandbox
    def test_unreadable_summary_keeps_wrapper_verdict_and_continues(self):
        status, rows, _, _, _, _ = self.run_external('dsh-ndjson', 'not a summary')
        self.assertEqual(status, 0)
        self.assertEqual(len(rows), 2)
        for row in rows:
            self.assertTrue(row['success'])
            self.assertIsNone(row['end'])
            self.assertIsNone(row['total_tokens'])
            self.assertIn('; harness summary unreadable', row['reason'])

    @requires_sandbox
    def test_timeout_overrides_completed_output_and_zero_exit(self):
        status, rows, _, _, _, _ = self.run_external(
            'dsh-ndjson', '\n'.join(json.dumps(v) for v in dsh_events()), trials=1, timed=True)
        self.assertEqual(status, 0)
        self.assertTrue(rows[0]['success'])
        self.assertTrue(rows[0]['timed_out'])
        self.assertEqual(rows[0]['harness_exit'], 0)
        self.assertEqual(rows[0]['end'], 'timeout')
        self.assertIn('; timeout:', rows[0]['reason'])

    @requires_sandbox
    def test_key_leak_is_flagged_without_publishing_value(self):
        events = dsh_events(reason={'kind': 'error', 'error': {'code': 'FAKE', 'message': self.key}})
        events[-1]['text'] = self.key
        status, rows, markdown, jsonl, stdout, stderr = self.run_external(
            'dsh-ndjson', '\n'.join(json.dumps(v) for v in events), trials=1, leak='dsh-home/session.log')
        self.assertEqual(status, 0)
        self.assertTrue(rows[0]['success'])
        self.assertTrue(rows[0]['reason'].startswith('SAFETY: DeepSeek key value found in '))
        for path in ('harness.stdout', 'harness.stderr', 'final.txt', 'dsh-home/session.log'):
            self.assertIn('SAFETY: DeepSeek key value found in ' + path, rows[0]['reason'])
        self.assertIn('Rotate the DeepSeek API key', stderr)
        for text in (markdown, jsonl, stdout, stderr):
            self.assertNotIn(self.key, text)
        # 保存の直前にも、理由以外の欄と見出しから値を除く。
        path = os.path.join(self.directory, 'guard.md')
        evaluation.write_record(path, {'header': self.key}, [dict(rows[0], model=self.key)], [])
        for name in ('guard.md', 'guard.jsonl'):
            with open(os.path.join(self.directory, name)) as f:
                self.assertNotIn(self.key, f.read())

    @requires_sandbox
    def test_key_command_failures_hide_output_and_dry_run_skips_preflight(self):
        harness = evaluation.load_harnesses()['deepseek-harness']
        for source, code in (('import sys\nprint(%r)\nsys.exit(7)\n' % self.key, 7), ('print("")\n', 0)):
            harness['api_key']['command'] = [self.executable('bad-key-command', source)]
            with self.assertRaisesRegex(ValueError, 'keychain \\(exit %d\\)' % code) as caught:
                evaluation.read_dsh_key(harness)
            self.assertNotIn(self.key, str(caught.exception))
        for name in ('deepseek-harness', 'claude-code'):
            with self.subTest(name=name):
                trial = os.path.join(self.directory, name)
                row = evaluation.run_trial(evaluation.Task('json_summary'), trial,
                    os.environ['BENITOITEH_TEST_BENITOITE'], name, evaluation.load_harnesses()[name],
                    'invalid_model', None, True)
                self.assertTrue(row['success'])
                self.assertIsNone(row['end'])
        self.assertFalse(os.path.exists(self.counter))


def agy_summary(**extra):
    value = dict(conversation_id='fake-conversation', status='SUCCESS', response='Done.', duration_seconds=1.2,
                 num_turns=1, usage=dict(input_tokens=100, cache_read_tokens=300, output_tokens=20,
                                         thinking_tokens=7, total_tokens=120))
    value.update(extra)
    return value


class AgyTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='skill-eval-agy-')
        self.addCleanup(self.temporary.cleanup)
        self.directory = os.path.realpath(self.temporary.name)
        self.home = os.path.join(self.directory, 'home')
        self.workspace = os.path.join(self.directory, "trial's files", 'workspace')
        os.makedirs(self.home)
        os.makedirs(self.workspace)
        parent = dict(os.environ, HOME=self.home, TMPDIR=self.directory,
                      CLAUDE_CONFIG_DIR=os.path.join(self.home, '.claude'),
                      XDG_CONFIG_HOME=os.path.join(self.directory, 'config'),
                      XDG_STATE_HOME=os.path.join(self.directory, 'state'),
                      AGY_CLI_DISABLE_AUTO_UPDATE='0', AGY_OTHER='private-agy-value',
                      ANTIGRAVITY_TEST='private-antigravity-value', JETSKI_TEST='private-jetski-value',
                      GOOGLE_TEST='private-google-value', GEMINI_TEST='private-gemini-value',
                      CLOUDSDK_TEST='private-cloudsdk-value')
        patch = mock.patch.dict(os.environ, parent, clear=True)
        patch.start()
        self.addCleanup(patch.stop)
        self.logs = os.path.join(self.home, '.gemini/antigravity-cli/brain/fake-conversation/.system_generated/logs')

    def write(self, path, content):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'w', encoding='utf-8') as f:
            f.write(content)
        return path

    def transcript(self, filename='transcript_full.jsonl', extra=None):
        events = [{'type': 'USER_INPUT'},
                  {'type': 'PLANNER_RESPONSE', 'input_tokens': 100, 'cache_read_tokens': 300,
                   'output_tokens': 20, 'tool_calls': [{'name': 'run_command'}, {'name': 'view_file'}]},
                  {'type': 'GENERIC'}, {'type': 'PLANNER_RESPONSE'}]
        if extra:
            events[1].update(extra)
        return self.write(os.path.join(self.logs, filename), '\n'.join(json.dumps(v) for v in events))

    def test_gate_standard_io_and_audit_log(self):
        gate = os.path.join(self.directory, 'agy', 'agy_gate.py')
        os.makedirs(os.path.dirname(gate))
        shutil.copyfile(os.path.join(evaluation.HERE, 'agy_gate.py'), gate)
        self.write(os.path.join(self.directory, 'agy/gate.log'), '')
        nested = os.path.join(self.workspace, 'sub')
        os.mkdir(nested)
        os.symlink(self.home, os.path.join(self.workspace, 'escape'))
        good = ['benitoite check main.bnt', 'benitoite run --allow-effects=Console.Write,File.Read main.bnt',
                'benitoite test main.bnt']
        cases = [(command, nested, 'run_command', True) for command in good]
        cases += [(command, self.workspace, 'run_command', False) for command in (
            'benitoite check main.bnt; rm -rf ~', 'benitoite run $(cat x)', 'benitoite check "main.bnt"',
            'benitoite check main.bnt\n', 'benitoite  check main.bnt', 'ls', 'benitoite /x.bnt',
            'benitoite ../x.bnt', 'benitoite a/../x.bnt', 'benitoite ' + 'x' * 4090)]
        cases += [('benitoite check main.bnt', cwd, 'run_command', False)
                  for cwd in (self.home, self.workspace + '-other', 'sub', None,
                              os.path.join(self.workspace, 'escape'))]
        cases += [('benitoite check main.bnt', self.workspace, 'shell_exec', False)]
        inputs = [(json.dumps({'toolCall': {'name': name, 'args': {'CommandLine': command, 'Cwd': cwd}},
                              'stepIdx': 1, 'workspacePaths': [self.workspace]}), allow)
                  for command, cwd, name, allow in cases]
        inputs += [(value, False) for value in ('bad json', '[]', '{}', '{"toolCall":null}',
                                               '{"toolCall":{"args":[]}}')]
        for content, allow in inputs:
            with self.subTest(content=content):
                result = subprocess.run([sys.executable, '-I', gate, self.workspace], input=content,
                    text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=dict(os.environ), timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                decision = json.loads(result.stdout)
                self.assertEqual(decision['decision'], 'allow' if allow else 'deny')
                self.assertTrue(decision['reason'])
                self.assertNotIn('permissionOverrides', decision)
        with open(os.path.join(self.directory, 'agy/gate.log')) as f:
            records = [json.loads(line) for line in f]
        self.assertEqual(len(records), len(inputs))
        self.assertTrue(all(record['time'] and len(record['command'] or '') <= 200 for record in records))
        self.assertEqual([record['decision'] for record in records[:2]], ['allow', 'allow'])

    def test_summary_tokens_transcript_fallback_and_warnings(self):
        fallback = self.transcript('transcript.jsonl')
        for pretty in (False, True):
            with self.subTest(pretty=pretty):
                text = json.dumps(agy_summary(denied_actions=[{}, {}]), indent=2 if pretty else None)
                if not pretty:
                    text = 'earlier output\n' + text + '\n\n'
                fields, detail, final = evaluation.external_output('agy-json', text, 0)
                self.assertEqual([fields[k] for k in evaluation.TOKEN_FIELDS], [400, 300, 0, 20, 420])
                self.assertEqual(fields['thinking_tokens'], 7)
                self.assertEqual(fields['turns'], 2)  # num_turns は利用者の発言数なので使わない。
                self.assertEqual(fields['tool_calls'], {'run_command': 1, 'view_file': 1})
                self.assertEqual(fields['denied_actions'], 2)
                self.assertIn('denied_actions: 2', detail)
                self.assertEqual(fields['end'], 'completed')
                self.assertEqual(final, 'Done.')
        # 完全な記録を優先し、最後の番で tool_calls がなくても数えられる。
        self.transcript(extra={'input_tokens': 99, 'tool_calls': [{'name': 'search_web'}]})
        self.write(fallback, 'bad fallback')
        fields, detail, _ = evaluation.external_output('agy-json', json.dumps(agy_summary()), 0)
        self.assertEqual(fields['tool_calls'], {'search_web': 1})
        self.assertIn('transcript usage differs', detail)
        self.assertIn('WARNING: unexpected agy tool search_web', detail)
        self.assertEqual(fields['input_tokens'], 400)
        for denied, count in ((None, None), ({}, None), ([], 0)):
            with self.subTest(denied=denied):
                fields, _, _ = evaluation.external_output('agy-json', json.dumps(agy_summary(denied_actions=denied)), 0)
                self.assertEqual(fields['denied_actions'], count)
        fields, _, _ = evaluation.external_output('agy-json', json.dumps(agy_summary()), 0)
        self.assertEqual(fields['denied_actions'], 0)

    def test_transcript_missing_malformed_and_escaping_paths_leave_counts_unknown(self):
        self.transcript()
        outside = os.path.join(self.directory, 'outside')
        os.mkdir(outside)
        brain = os.path.dirname(os.path.dirname(os.path.dirname(self.logs)))
        os.symlink(outside, os.path.join(brain, 'escape'))
        for identity in ('missing', '../outside', 'escape', '/absolute', 'bad\n', '日本語'):
            with self.subTest(identity=identity):
                fields, detail, _ = evaluation.external_output('agy-json', json.dumps(agy_summary(conversation_id=identity)), 0)
                self.assertIsNone(fields['turns'])
                self.assertIsNone(fields['tool_calls'])
                self.assertEqual(fields['total_tokens'], 420)
                self.assertIn('transcript unreadable', detail)
        # ID のディレクトリだけでなく、記録そのもののリンクも検査する。
        path = os.path.join(self.logs, 'transcript_full.jsonl')
        os.unlink(path)
        os.symlink(self.write(os.path.join(outside, 'record'), '{"type":"PLANNER_RESPONSE"}'), path)
        fields, _, _ = evaluation.external_output('agy-json', json.dumps(agy_summary()), 0)
        self.assertIsNone(fields['turns'])
        os.unlink(path)
        self.write(path, '{"type":"PLANNER_RESPONSE"}\ninvalid')
        fields, _, _ = evaluation.external_output('agy-json', json.dumps(agy_summary()), 0)
        self.assertIsNone(fields['turns'])

    def test_end_priority_errors_and_unreadable_summary(self):
        self.transcript()
        warning = '[agy] print timeout after 30m0s with turn in progress; returning partial output'
        cases = [(json.dumps(agy_summary()), 0, '', 'completed', ''),
                 (json.dumps(agy_summary(status='FAILED')), 0, '', 'error', ''),
                 (json.dumps(agy_summary()), 2, '', 'error', ''),
                 (json.dumps(agy_summary()), 3, '', 'error', 'status 3'),
                 (json.dumps(agy_summary()), 0, 'AGY_ERROR: {"short_error":"failed"}', 'error', 'short_error'),
                 ('invalid', 3, 'AGY_ERROR: {"code":503,"retryable":true}', 'error', 'retryable'),
                 ('invalid', 0, 'AGY_ERROR: ' + 'x' * 250, 'error', 'x' * 200),
                 (json.dumps(agy_summary()), 3, warning + '\nAGY_ERROR: {}', 'timeout', 'agy print timeout'),
                 ('invalid', 0, warning, 'timeout', 'harness summary unreadable'),
                 ('invalid', 0, '', None, 'harness summary unreadable'),
                 (json.dumps(agy_summary(usage={'input_tokens': 'bad'})), 0, '', None, 'harness summary unreadable')]
        for out, code, err, end, detail in cases:
            with self.subTest(code=code, err=err, out=out):
                fields, reason, _ = evaluation.external_output('agy-json', out, code, err)
                self.assertEqual(fields['end'], end)
                self.assertIn(detail, reason)
                self.assertNotIn('x' * 201, reason)

    def test_preflight_rejects_user_settings_and_reports_only_names(self):
        self.assertEqual(evaluation.check_agy_config(self.workspace), [])
        paths = ['.gemini/config/' + name for name in (
            'skills', 'plugins', 'rules', 'hooks.json', 'skills.json', 'plugins.json', 'agents',
            'workflows', 'global_workflows', 'workflows.json', 'sidecars')]
        paths += ['GEMINI.md', 'AGENTS.md', '.gemini/GEMINI.md', '.gemini/AGENTS.md',
                  '.gemini/antigravity-cli/hooks.json', '.gemini/config/nested/AGENTS.md']
        for name in paths:
            with self.subTest(name=name):
                path = self.write(os.path.join(self.home, name), 'private-config-value')
                with self.assertRaisesRegex(ValueError, 'Antigravity') as caught:
                    evaluation.check_agy_config(self.workspace)
                self.assertIn(name, str(caught.exception))
                self.assertNotIn('private-config-value', str(caught.exception))
                os.unlink(path)
        for name in ('.gemini/config/config.json', '.gemini/antigravity-cli/settings.json',
                     '.gemini/config/projects/project.json'):
            for key, value in (('useG1Credits', True), ('PeRmissionOverrides', [])):
                with self.subTest(name=name, key=key):
                    path = self.write(os.path.join(self.home, name), json.dumps({key: value}))
                    with self.assertRaisesRegex(ValueError, key):
                        evaluation.check_agy_config(self.workspace)
                    self.write(path, '{}')
        mcp = os.path.join(self.home, '.gemini/config/mcp_config.json')
        for text in ('', '{}', '{"mcpServers":{}}'):
            self.write(mcp, text)
            self.assertEqual(evaluation.check_agy_config(self.workspace), [])
        for text in ('{"mcpServers":{"custom":{"command":"private-config-value"}}}', 'invalid'):
            self.write(mcp, text)
            with self.assertRaisesRegex(ValueError, 'mcp_config.json') as caught:
                evaluation.check_agy_config(self.workspace)
            self.assertNotIn('private-config-value', str(caught.exception))
        os.unlink(mcp)
        builtin = os.path.join(self.home, '.gemini/antigravity-cli/builtin/skills/product-skill')
        os.makedirs(builtin)
        self.assertEqual(evaluation.check_agy_config(self.workspace), ['product-skill'])

    def test_preflight_workspace_and_ancestor_customizations(self):
        for base, name in ((self.workspace, 'GEMINI.md'), (self.workspace, 'AGENTS.md'),
                           (os.path.dirname(self.workspace), 'AGENTS.md'),
                           (os.path.dirname(self.workspace), '.gemini/GEMINI.md'),
                           (os.path.dirname(self.workspace), '.gemini/AGENTS.md')):
            with self.subTest(base=base, name=name):
                path = self.write(os.path.join(base, name), 'private-instruction')
                with self.assertRaisesRegex(ValueError, name):
                    evaluation.check_agy_config(self.workspace)
                os.unlink(path)
        parent_agents = os.path.join(os.path.dirname(self.workspace), '.agents')
        os.mkdir(parent_agents)
        with self.assertRaisesRegex(ValueError, '.agents'):
            evaluation.check_agy_config(self.workspace)
        os.rmdir(parent_agents)
        # 試行自身の .agents は道具が設置するため許す。
        os.mkdir(os.path.join(self.workspace, '.agents'))
        self.assertEqual(evaluation.check_agy_config(self.workspace), [])
        inside_home = os.path.join(self.home, 'trials/workspace')
        os.makedirs(os.path.join(self.home, '.agents/skills'))
        with self.assertRaisesRegex(ValueError, '.agents'):
            evaluation.check_agy_config(inside_home)

    def security_gate(self, name, args, log_kind='regular'):
        gate_dir = os.path.join(self.directory, 'gate-test')
        os.makedirs(gate_dir, exist_ok=True)
        gate = os.path.join(gate_dir, 'agy_gate.py')
        shutil.copyfile(os.path.join(evaluation.HERE, 'agy_gate.py'), gate)
        log = os.path.join(gate_dir, 'gate.log')
        if os.path.lexists(log):
            if os.path.isdir(log) and not os.path.islink(log):
                os.rmdir(log)
            else:
                os.unlink(log)
        if log_kind == 'regular':
            self.write(log, '')
        elif log_kind == 'symlink':
            os.symlink(self.write(os.path.join(self.directory, 'sentinel'), 'unchanged'), log)
        elif log_kind == 'fifo':
            os.mkfifo(log)
        elif log_kind == 'directory':
            os.mkdir(log)
        reader = os.open(log, os.O_RDWR | os.O_NONBLOCK) if log_kind == 'fifo' else None
        try:
            result = subprocess.run([sys.executable, '-I', gate, self.workspace],
                input=json.dumps({'toolCall': {'name': name, 'args': args}}), text=True,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=2)
        finally:
            if reader is not None:
                os.close(reader)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout), log

    def test_security_gate_commands_paths_and_tool_categories(self):
        for command in ('benitoite skill install', 'benitoite package add x', 'benitoite fmt x.bnt',
                        'benitoite run --out=/etc/passwd x.bnt', 'benitoite run x.bnt:/etc',
                        'benitoite run --x=../f', 'benitoite test --x=a,/etc', 'benitoite',
                        'benitoite run =ls', 'benitoite run --x==ls', 'benitoite run x.bnt:=ls',
                        'benitoite run --x=a,=ls'):
            with self.subTest(command=command):
                decision, _ = self.security_gate('run_command', {'CommandLine': command, 'Cwd': self.workspace})
                self.assertEqual(decision['decision'], 'deny')
        os.symlink(self.home, os.path.join(self.workspace, 'outside'))
        readers = ('view_file', 'list_dir', 'grep_search', 'find_by_name', 'view_file_outline', 'view_code_item')
        writers = ('write_to_file', 'replace_file_content', 'multi_replace_file_content')
        for name in readers + writers:
            for path, allowed in ((self.workspace + '/main.bnt', True), ('file://' + self.workspace + '/main.bnt', True),
                                  (self.home + '/private', False), ('file://' + self.home + '/private', False),
                                  (self.workspace + '/outside/private', False)):
                with self.subTest(name=name, path=path):
                    decision, log = self.security_gate(name, {'nested': [{'Path': path}], 'Content': 'private-file-value'})
                    self.assertEqual(decision['decision'], 'allow' if allowed else 'deny')
                    with open(log) as f:
                        record = json.loads(f.readline())
                    self.assertEqual(record['tool'], name)
                    self.assertIn('nested', record['argument_keys'])
                    self.assertNotIn('private-file-value', json.dumps(record))
            decision, _ = self.security_gate(name, {'Query': 'plain search'})
            self.assertEqual(decision['decision'], 'deny')
            self.assertIn('absolute', decision['reason'])
        for marker in ('.agents', '_agents', '.agent', '_agent', '.gemini', '.git', 'AgEnTs.Md', 'gEmInI.md'):
            for name in writers:
                with self.subTest(name=name, marker=marker):
                    path = os.path.join(self.workspace, 'sub', marker)
                    if marker.startswith(('.', '_')):
                        path += '/file'
                    decision, _ = self.security_gate(name, {'Path': path})
                    self.assertEqual(decision['decision'], 'deny')
        for name in ('search_web', 'unknown_tool', 'invoke_subagent'):
            decision, _ = self.security_gate(name, {})
            self.assertEqual(decision, {'decision': 'deny', 'reason': 'This tool is disabled for this evaluation.'})

    def test_security_path_keys_and_content_are_distinct(self):
        bad = ('../../etc/passwd', '~/.ssh/id_rsa', 'file:/etc/passwd', 'FILE:///etc/passwd',
               ' /etc/passwd', self.workspace + '/x ', self.workspace + '/x\n',
               self.workspace + '/x\x00', 'file://' + self.workspace + '/x%00',
               'file://' + self.workspace + '/x%0A', '', '..')
        for name, key in (('view_file', 'AbsolutePath'), ('list_dir', 'DirectoryPath'),
                          ('grep_search', 'SearchPath'), ('find_by_name', 'SearchDirectory'),
                          ('view_file_outline', 'FilePath'), ('view_code_item', 'Uri'),
                          ('write_to_file', 'TargetFile'), ('replace_file_content', 'TargetFile'),
                          ('multi_replace_file_content', 'TargetFile')):
            for value in bad + (None, 123, {}, [self.workspace, '../outside']):
                with self.subTest(name=name, key=key, value=value):
                    decision, _ = self.security_gate(name, {'nested': [{key: value}],
                                                          'Description': self.workspace + '/main.bnt'})
                    self.assertEqual(decision['decision'], 'deny')
        cases = [('write_to_file', {'TargetFile': self.workspace + '/main.bnt', 'CodeContent': '// comment\nbody'}),
                 ('replace_file_content', {'TargetFile': self.workspace + '/main.bnt', 'TargetContent': '/* old */'}),
                 ('grep_search', {'SearchPath': self.workspace, 'Query': '/foo'}),
                 ('find_by_name', {'SearchDirectory': self.workspace, 'Pattern': '../../**'}),
                 ('find_by_name', {'SearchDirectory': self.workspace, 'Pattern': '*.bnt', 'FullPath': False}),
                 ('find_by_name', {'SearchDirectory': self.workspace, 'FullPath': 1, 'SecurityNote': 'note'}),
                 ('find_by_name', {'SearchDirectory': self.workspace, 'FullPath': 1.5}),
                 ('view_code_item', {'File': self.workspace + '/main.bnt', 'NodePaths': ['main']}),
                 ('view_file', {'nested': [{'aBsOlUtEpAtH': [self.workspace + '/a', 'file://' + self.workspace + '/b']}]})]
        for name, args in cases:
            with self.subTest(name=name, args=args):
                decision, _ = self.security_gate(name, args)
                self.assertEqual(decision['decision'], 'allow')
        for marker in ('.agents', '_agents', '.agent', '_agent', '.gemini', '.git', 'AGENTS.md', 'Gemini.MD'):
            for name in ('write_to_file', 'replace_file_content', 'multi_replace_file_content'):
                with self.subTest(marker=marker, name=name):
                    decision, _ = self.security_gate(name, {'TargetFile': [self.workspace + '/ok.bnt',
                                                        self.workspace + '/sub/' + marker]})
                    self.assertEqual(decision['decision'], 'deny')

    def test_security_gate_log_rejects_missing_symlink_fifo_and_directory(self):
        for kind in ('missing', 'symlink', 'fifo', 'directory'):
            with self.subTest(kind=kind):
                decision, _ = self.security_gate('run_command',
                    {'CommandLine': 'benitoite check main.bnt', 'Cwd': self.workspace}, kind)
                self.assertEqual(decision['decision'], 'deny')
                if kind == 'symlink':
                    with open(os.path.join(self.directory, 'sentinel')) as f:
                        self.assertEqual(f.read(), 'unchanged')

    def test_security_json_recursion_special_files_and_record_redaction(self):
        deep = '[' * 3000 + ']' * 3000
        fields, reason, _ = evaluation.external_output('agy-json', deep, 0)
        self.assertIsNone(fields['end'])
        self.assertIn('harness summary unreadable', reason)
        path = self.write(os.path.join(self.logs, 'transcript_full.jsonl'),
                          '{"type":"PLANNER_RESPONSE","extra":' + deep + '}')
        fields, reason, _ = evaluation.external_output('agy-json', json.dumps(agy_summary()), 0)
        self.assertIsNone(fields['turns'])
        self.assertIn('transcript unreadable', reason)
        os.unlink(path)
        os.mkfifo(path)
        driver = ('import sys, json; sys.path.insert(0, %r); import skill_eval as e; '
                  'print(json.dumps(e.external_output("agy-json", %r, 0)[0]))') % (
                      evaluation.HERE, json.dumps(agy_summary()))
        result = subprocess.run([sys.executable, '-I', '-c', driver], env=dict(os.environ),
                                capture_output=True, text=True, timeout=2)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIsNone(json.loads(result.stdout)['turns'])
        os.unlink(path)
        self.transcript(extra={'tool_calls': [{'name': 'view_file'}, {'name': 'bad@example.com'},
                                              {'name': 'x' * 65}, {'name': 'UPPER'}, {'name': None}, {'name': 1}]})
        error = {'short_error': 'failed person@example.com', 'code': 503, 'retryable': True,
                 'status': 'UNAVAILABLE', 'error_id': 'id', 'account': 'private-account',
                 'nested': {'token': 'private-token'}}
        for message in (json.dumps(error), 'plain person@example.com ' + 'x' * 250):
            fields, reason, _ = evaluation.external_output('agy-json', json.dumps(agy_summary()), 3,
                                                           'AGY_ERROR: ' + message)
            self.assertEqual(fields['tool_calls'], {'view_file': 1, '<invalid>': 5})
            self.assertIn('<email>', reason)
            for text in ('person@example.com', 'bad@example.com', 'private-account', 'private-token', 'x' * 201):
                self.assertNotIn(text, reason)
        for value in ([{'account': 'private-account'}], 'private-account', 123):
            _, reason, _ = evaluation.external_output('agy-json', json.dumps(agy_summary()), 3,
                                                       'AGY_ERROR: ' + json.dumps(value))
            self.assertNotIn('private-account', reason)
            self.assertIn('AGY_ERROR: {}', reason)

    def test_security_preflight_allowlists_additional_files_and_scan_scope(self):
        for base, name in ((self.home, '.gemini/config/rules.json'), (self.home, '.gemini/config/agents.json'),
                           (os.path.dirname(self.workspace), '.gemini/settings.json')):
            path = self.write(os.path.join(base, name), '{}')
            with self.subTest(name=name), self.assertRaises(ValueError):
                evaluation.check_agy_config(self.workspace)
            os.unlink(path)
        for marker in ('.agent', '_agents', '_agent'):
            path = os.path.join(os.path.dirname(self.workspace), marker)
            os.mkdir(path)
            with self.subTest(marker=marker), self.assertRaises(ValueError):
                evaluation.check_agy_config(self.workspace)
            os.rmdir(path)
        for relative, values in (
            ('.gemini/antigravity-cli/settings.json', {'model': 'm', 'trustedWorkspaces': []}),
            ('.gemini/config/config.json', {'userSettings': {'remoteControlHostname': 'host'}}),
            ('.gemini/config/projects/p.json', {'id': 'id', 'name': 'n', 'projectResources': {'resources': []}})):
            path = self.write(os.path.join(self.home, relative), json.dumps(values))
            self.assertEqual(evaluation.check_agy_config(self.workspace), [])
            for key in ('plugins', 'useG1Credits', 'use_g1_credits', 'useg1credits', 'permissions', 'modelProvider'):
                with self.subTest(relative=relative, key=key):
                    self.write(path, json.dumps(dict(values, **{key: 'private-setting'})))
                    with self.assertRaisesRegex(ValueError, key) as caught:
                        evaluation.check_agy_config(self.workspace)
                    self.assertNotIn('private-setting', str(caught.exception))
            self.write(path, json.dumps(values))
        path = os.path.join(self.home, '.gemini/config/config.json')
        self.write(path, '{"userSettings":{"plugins":"private-setting"}}')
        with self.assertRaisesRegex(ValueError, 'plugins'):
            evaluation.check_agy_config(self.workspace)
        self.write(path, '{}')
        path = os.path.join(self.home, '.gemini/antigravity-cli/mcp_config.json')
        for text in ('', '{}'):
            self.write(path, text)
            self.assertEqual(evaluation.check_agy_config(self.workspace), [])
        for text in ('{"mcpServers":{}}', '{"servers":{"x":{}}}', 'invalid'):
            self.write(path, text)
            with self.assertRaisesRegex(ValueError, 'mcp_config.json'):
                evaluation.check_agy_config(self.workspace)
        os.unlink(path)
        # 会話の記録と、そのほかの利用者の領域まで再帰して歩かない。
        self.write(os.path.join(self.home, '.gemini/antigravity-cli/brain/old/AGENTS.md'), 'private')
        self.write(os.path.join(self.home, '.gemini/other/AGENTS.md'), 'private')
        self.assertEqual(evaluation.check_agy_config(self.workspace), [])
        exists = os.path.lexists
        with mock.patch.object(os.path, 'lexists', side_effect=lambda p: p == '/Library/Application Support/Antigravity/admin_settings.json' or exists(p)):
            with self.assertRaisesRegex(ValueError, 'admin_settings.json'):
                evaluation.check_agy_config(self.workspace)

    def security_trial(self, mutation, harness_name='antigravity-gemini', user_parent='.agents'):
        task = evaluation.Task('json_summary')
        with open(task.path('reference.bnt')) as f:
            source = f.read()
        self.transcript()
        trial = os.path.join(self.directory, 'security-trial')
        executable = os.path.join(self.directory, 'fake-agy')
        text = ("#!%s\n" % sys.executable +
            'import json, os, stat, subprocess, sys\nfrom pathlib import Path\n'
            'if "--version" in sys.argv:\n print("fake agy 1.4.0")\n sys.exit(0)\n'
            'ws = Path.cwd()\ntrial = ws.parent\n'
            'Path("main.bnt").write_text(%r)\n' % source +
            'subprocess.run(["benitoite", "check", "main.bnt"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)\n'
            'mutation = %r\n' % mutation +
            'if (ws / ".agents/hooks.json").exists() and mutation != "no-gate":\n'
            ' hooks = json.loads((ws / ".agents/hooks.json").read_text())\n'
            ' group = next(iter(hooks.values()))["PreToolUse"][0]\n'
            ' payload = {"toolCall":{"name":"run_command","args":{"CommandLine":"benitoite check main.bnt","Cwd":str(ws)}}}\n'
            ' if mutation == "read-gate-no-transcript": payload = {"toolCall":{"name":"view_file","args":{"AbsolutePath":str(ws / "task.md")}}}\n'
            ' if mutation == "deny-gate-no-transcript": payload["toolCall"]["args"]["CommandLine"] = "ls"\n'
            ' subprocess.run(group["hooks"][0]["command"], shell=True, input=json.dumps(payload), text=True, check=True, stdout=subprocess.DEVNULL)\n'
            'if mutation.endswith("no-transcript"):\n'
            ' (Path.home() / ".gemini/antigravity-cli/brain/fake-conversation/.system_generated/logs/transcript_full.jsonl").unlink()\n'
            'if mutation == "readonly":\n'
            ' for p in [ws / ".agents"] + list((ws / ".agents").rglob("*")):\n'
            '  assert stat.S_IMODE(p.stat().st_mode) == (0o555 if p.is_dir() else 0o444), str(p)\n'
            'elif mutation.startswith("user-"):\n'
            ' p = Path.home() / %r\n' % (user_parent + '/skills/benitoite') +
            ' if mutation == "user-create": p.mkdir(parents=True)\n'
            ' elif mutation == "user-delete": p.rmdir()\n'
            ' elif mutation == "user-modify": os.utime(p, ns=(p.stat().st_atime_ns, p.stat().st_mtime_ns + 1000000000))\n'
            ' elif mutation == "user-type": p.rmdir(); p.write_text("file")\n'
            'elif mutation == "missing-gate": (trial / "agy/gate.log").unlink()\n'
            'elif mutation == "restore-error":\n'
            ' (ws / ".agents").chmod(0o755)\n'
            ' for name in ("vanish", "restored"): (ws / ".agents" / name).write_text("file"); (ws / ".agents" / name).chmod(0o444)\n'
            'elif mutation.startswith("create:"):\n'
            ' p = ws / mutation.split(":", 1)[1]\n'
            ' if p.name.lower() in ("agents.md", "gemini.md"): p.parent.mkdir(parents=True, exist_ok=True); p.write_text("instruction")\n'
            ' else: p.mkdir(parents=True)\n'
            'print(%r)\n' % (event_output(calls={'benitoite_check': 1}) if harness_name == 'benitoiteh-deepseek'
                                  else json.dumps(claude_result()) if harness_name == 'claude-code'
                                  else json.dumps(agy_summary())))
        self.write(executable, text)
        os.chmod(executable, 0o755)
        harness = copy.deepcopy(evaluation.load_harnesses()[harness_name])
        harness['start'] = [executable]
        harness['version'] = [executable, '--version']
        return evaluation.run_trial(task, trial, os.environ['BENITOITEH_TEST_BENITOITE'], harness_name,
                                    harness, str(harness['model']), None, False, benitoiteh=executable)

    @unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
    @requires_sandbox
    def test_security_trial_metadata_and_protected_workspace_changes(self):
        for harness_name, user_parent in (('antigravity-gemini', '.agents'), ('antigravity-gemini', '.claude'),
                                          ('claude-code', '.agents'), ('benitoiteh-deepseek', '.agents')):
            for mutation in ('user-create', 'user-delete', 'user-modify', 'user-type'):
                with self.subTest(harness=harness_name, mutation=mutation, user_parent=user_parent):
                    target = os.path.join(self.home, user_parent, 'skills/benitoite')
                    if mutation != 'user-create':
                        os.makedirs(target)
                    row = self.security_trial(mutation, harness_name, user_parent)
                    self.assertTrue(row['reason'].startswith('SAFETY: user skill directory changed'), row['reason'])
                    self.assertTrue(row['success'], row['reason'])
                    if os.path.isdir(target):
                        os.rmdir(target)
                    elif os.path.lexists(target):
                        os.unlink(target)
                    shutil.rmtree(os.path.join(self.directory, 'security-trial'))
        for marker in ('.agents', '_agents', '.agent', '_agent', '.gemini', '.git', 'AgEnTs.Md', 'GEMINI.md'):
            with self.subTest(marker=marker):
                row = self.security_trial('create:sub/' + marker)
                expected = marker.upper().replace('.MD', '.md') if marker.lower().endswith('.md') else marker
                self.assertIn('SAFETY: %s was created (1)' % expected, row['reason'])
                shutil.rmtree(os.path.join(self.directory, 'security-trial'))
        for mutation in ('no-gate', 'missing-gate'):
            row = self.security_trial(mutation)
            self.assertIn('SAFETY: hooks were not loaded', row['reason'])
            shutil.rmtree(os.path.join(self.directory, 'security-trial'))
        row = self.security_trial('readonly')
        self.assertEqual(row['harness_exit'], 0, row['reason'])
        agents = os.path.join(self.directory, 'security-trial/workspace/.agents')
        for root, dirs, files in os.walk(agents):
            for path in [root] + [os.path.join(root, n) for n in dirs + files]:
                self.assertTrue(os.stat(path).st_mode & stat.S_IWUSR, path)

    def test_security_project_resources_and_google_environment(self):
        project = os.path.join(self.home, '.gemini/config/projects/p.json')
        root = os.path.join(self.directory, 'work-root')
        for git in (False, True):
            for folder in (root, os.path.dirname(root), os.path.join(root, 'child')):
                resource = {'folderUri': 'file://' + folder}
                if git:
                    resource = {'gitFolder': dict(resource, allowWrite=True)}
                self.write(project, json.dumps({'projectResources': {'resources': [resource]}}))
                with self.subTest(folder=folder, git=git), self.assertRaisesRegex(ValueError, 'folderUri'):
                    evaluation.check_agy_config(self.workspace, root)
        for resource in ({'folderUri': 'file://' + self.home},
                         {'gitFolder': {'folderUri': 'file://' + self.home, 'allowWrite': True}}):
            self.write(project, json.dumps({'projectResources': {'resources': [resource]}}))
            self.assertEqual(evaluation.check_agy_config(self.workspace, root), [])
        for resources in ([], {'resources': {}}, {'resources': [None]},
                          {'resources': [{'folderUri': 1}]}, {'resources': [{'gitFolder': []}]}):
            self.write(project, json.dumps({'projectResources': resources}))
            with self.subTest(resources=resources), self.assertRaises(ValueError):
                evaluation.check_agy_config(self.workspace, root)
        for resources in ({'resources': [], 'permissions': 'private-value'},
                          {'resources': [{'permissions': 'private-value'}]},
                          {'resources': [{'gitFolder': {'permissions': 'private-value'}}]}):
            self.write(project, json.dumps({'projectResources': resources}))
            with self.subTest(resources=resources), self.assertRaisesRegex(ValueError, 'permissions') as caught:
                evaluation.check_agy_config(self.workspace, root)
            self.assertNotIn('private-value', str(caught.exception))
        self.write(project, '{"id":"default","name":"Default"}')
        self.assertEqual(evaluation.check_agy_config(self.workspace, root), [])
        with mock.patch.dict(os.environ, GOOGLE_TEST='private', GEMINI_TEST='private', CLOUDSDK_TEST='private'):
            env, removed = evaluation.agy_environment()
            for name in ('GOOGLE_TEST', 'GEMINI_TEST', 'CLOUDSDK_TEST'):
                self.assertNotIn(name, env)
                self.assertIn(name, removed)
            normal, _ = evaluation.harness_environment()
            self.assertEqual(normal['GOOGLE_TEST'], 'private')

    def test_security_permissions_restore_skips_links_and_continues_after_failure(self):
        for fallback in (False, True):
            with self.subTest(fallback=fallback):
                agents = os.path.join(self.workspace, '.agents')
                os.makedirs(agents)
                os.makedirs(os.path.join(os.path.dirname(self.workspace), 'agy'), exist_ok=True)
                outside = self.write(os.path.join(self.directory, 'outside-file-%s' % fallback), 'private')
                os.chmod(outside, 0o400)
                for name in ('victim', 'missing', 'ok'):
                    self.write(os.path.join(agents, name), 'file')
                    os.chmod(os.path.join(agents, name), 0o444)
                real_open, real_chmod = os.open, os.chmod
                swapped = []
                def swap(path, kwargs):
                    name = os.path.basename(path)
                    if name in ('victim', 'missing') and name not in swapped:
                        swapped.append(name)
                        directory = kwargs.get('dir_fd')
                        os.unlink(path, dir_fd=directory)
                        if name == 'victim':
                            os.symlink(outside, path, dir_fd=directory)
                def opening(path, flags, *args, **kwargs):
                    swap(path, kwargs)
                    return real_open(path, flags, *args, **kwargs)
                def changing(path, mode, *args, **kwargs):
                    swap(path, kwargs)
                    return real_chmod(path, mode, *args, **kwargs)
                support = set(os.supports_follow_symlinks)
                if fallback:
                    support.discard(os.chmod)
                elif os.chmod in os.supports_follow_symlinks:
                    support.add(changing)
                with mock.patch.object(os, 'open', new=opening), \
                        mock.patch.object(os, 'chmod', new=changing), \
                        mock.patch.object(os, 'supports_follow_symlinks', support):
                    evaluation.agy_permissions(self.workspace, True)
                self.assertEqual(set(swapped), {'victim', 'missing'})
                self.assertEqual(stat.S_IMODE(os.stat(outside).st_mode), 0o400)
                self.assertTrue(os.stat(os.path.join(agents, 'ok')).st_mode & stat.S_IWUSR)
                with open(os.path.join(self.directory, "trial's files/agy/permissions.log")) as f:
                    self.assertIn('missing', f.read())
                shutil.rmtree(agents)

    @unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
    @requires_sandbox
    def test_security_permission_failure_keeps_trial_result(self):
        real_stat = os.stat
        removed = []
        def disappearing(path, *args, **kwargs):
            if path == 'vanish' and kwargs.get('dir_fd') is not None and not removed:
                removed.append(True)
                os.unlink(path, dir_fd=kwargs['dir_fd'])
            return real_stat(path, *args, **kwargs)
        with mock.patch.object(os, 'stat', new=disappearing):
            row = self.security_trial('restore-error')
        self.assertTrue(removed)
        self.assertTrue(row['success'], row['reason'])
        self.assertIn('WARNING: agy permission changes failed: 1', row['reason'])
        trial = os.path.join(self.directory, 'security-trial')
        self.assertTrue(os.stat(os.path.join(trial, 'workspace/.agents/restored')).st_mode & stat.S_IWUSR)
        with open(os.path.join(trial, 'agy/permissions.log')) as f:
            errors = [json.loads(line) for line in f]
        self.assertTrue(any(item['path'] == '.agents/vanish' and item['stage'] == 'restore' for item in errors))

    @unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
    @requires_sandbox
    def test_security_wrapper_requires_command_allow_and_created_names_are_private(self):
        for mutation, warned in (('read-gate-no-transcript', True), ('deny-gate-no-transcript', True),
                                  ('command-gate-no-transcript', False)):
            with self.subTest(mutation=mutation):
                row = self.security_trial(mutation)
                self.assertEqual('SAFETY: hooks were not loaded' in row['reason'], warned, row['reason'])
                self.assertTrue(row['log_exists'])
                self.assertIsNone(row['tool_calls'])
                shutil.rmtree(os.path.join(self.directory, 'security-trial'))
        for marker, expected in (('private@example.com/.agents/sub/.agents', '.agents'),
                                  ('private@example.com/secret/AGENTS.md', 'AGENTS.md')):
            row = self.security_trial('create:' + marker)
            self.assertIn('SAFETY: %s was created' % expected, row['reason'])
            self.assertIn('(2)' if expected == '.agents' else '(1)', row['reason'])
            self.assertNotIn('private@example.com', row['reason'])
            self.assertNotIn('secret', row['reason'])
            shutil.rmtree(os.path.join(self.directory, 'security-trial'))

    def test_normal_exit_and_version_query_stop_background_group(self):
        # 先頭が正常終了しても、同じ群れで出力を持つ子を残さない契約。
        for version_query in (False, True):
            with self.subTest(version_query=version_query):
                pid_file = os.path.join(self.directory, 'child-pid')
                child = ('import os, time\nfrom pathlib import Path\n'
                         'Path(%r).write_text(str(os.getpid()))\ntime.sleep(60)\n') % pid_file
                parent = ('import subprocess, sys, time\nfrom pathlib import Path\n'
                          'p = subprocess.Popen([sys.executable, "-c", %r])\n'
                          'while not Path(%r).exists(): time.sleep(.01)\n'
                          'print("fake agy version", flush=True)\n') % (child, pid_file)
                env, removed = evaluation.agy_environment()
                self.assertEqual(env['AGY_CLI_DISABLE_AUTO_UPDATE'], '1')
                self.assertIn('AGY_OTHER', removed)
                try:
                    if version_query:
                        version = evaluation.version_of([sys.executable, '-c', parent], env, cleanup_group=True)
                        self.assertEqual(version, 'fake agy version')
                    else:
                        code, timed_out, _wall = evaluation.run_harness([sys.executable, '-c', parent],
                            self.directory, env, 5, os.path.join(self.directory, 'stdout'),
                            os.path.join(self.directory, 'stderr'), cleanup_group=True)
                        self.assertEqual(code, 0)
                        self.assertFalse(timed_out)
                    with open(pid_file) as f:
                        pid = int(f.read())
                    deadline = time.monotonic() + 5
                    while time.monotonic() < deadline:
                        try:
                            os.kill(pid, 0)
                        except ProcessLookupError:
                            break
                        time.sleep(.01)
                    else:
                        self.fail('background child survived agy group cleanup')
                finally:
                    if os.path.exists(pid_file):
                        with open(pid_file) as f:
                            pid = int(f.read())
                        try:
                            os.kill(pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                        os.unlink(pid_file)

    def fake_agy(self):
        task = evaluation.Task('json_summary')
        with open(task.path('reference.bnt')) as f:
            source = f.read()
        self.transcript(extra={'tool_calls': [{'name': 'run_command'}, {'name': 'view_file'},
                                               {'name': 'search_web'}]})
        self.write(os.path.join(self.home, '.gemini/antigravity-cli/builtin/skills/product-skill/SKILL.md'), 'product')
        fake = os.path.join(self.directory, 'fake-bin/agy')
        script = ("#!%s\n" % sys.executable +
            'import json, os, re, shlex, subprocess, sys\nfrom pathlib import Path\n'
            'assert os.environ["AGY_CLI_DISABLE_AUTO_UPDATE"] == "1"\n'
            'assert all(n == "AGY_CLI_DISABLE_AUTO_UPDATE" or not n.startswith(("AGY_", "ANTIGRAVITY_", "JETSKI_", "GOOGLE_", "GEMINI_", "CLOUDSDK_")) for n in os.environ)\n'
            'assert Path(os.environ["TMPDIR"]).is_dir()\n'
            'calls = Path(__file__).with_suffix(".calls")\n'
            'with calls.open("a") as f: f.write("version\\n" if "--version" in sys.argv else "trial\\n")\n'
            'version = Path(__file__).with_suffix(".version")\n'
            'if "--version" in sys.argv:\n print(version.read_text() if version.exists() else "fake agy 1.4.0")\n sys.exit(0)\n'
            'assert sys.argv[1] == "-p"\n'
            'assert "--dangerously-skip-permissions" in sys.argv\n'
            'assert "--mode" not in sys.argv and "accept-edits" not in sys.argv\n'
            'assert sys.argv[sys.argv.index("--output-format")+1] == "json"\n'
            'assert sys.argv[sys.argv.index("--print-timeout")+1] == "1800s"\n'
            'assert Path(sys.argv[sys.argv.index("--log-file")+1]) == Path.cwd().parent / "agy/agy.log"\n'
            'hooks = json.loads(Path(".agents/hooks.json").read_text())\n'
            'assert len(hooks) == 1\n'
            'group = hooks["evaluation-gate"]["PreToolUse"]\n'
            'assert len(group) == 1 and group[0]["matcher"] == ".*"\n'
            'assert group[0]["hooks"][0]["timeout"] == 30\n'
            'gate = group[0]["hooks"][0]["command"]\n'
            'assert shlex.split(gate)[2:] == [str(Path.cwd().parent / "agy/agy_gate.py"), str(Path.cwd())]\n'
            'payload = {"toolCall":{"name":"run_command","args":{"CommandLine":"benitoite check main.bnt","Cwd":str(Path.cwd())}}}\n'
            'result = subprocess.run(gate, shell=True, input=json.dumps(payload), text=True, capture_output=True, check=True)\n'
            'assert json.loads(result.stdout)["decision"] == "allow"\n'
            'assert "permissionOverrides" not in json.loads(result.stdout)\n'
            'payload = {"toolCall":{"name":"view_file","args":{"AbsolutePath":str(Path.cwd() / "task.md")}}}\n'
            'result = subprocess.run(gate, shell=True, input=json.dumps(payload), text=True, capture_output=True, check=True)\n'
            'assert json.loads(result.stdout)["decision"] == "allow"\n'
            'payload["toolCall"]["name"] = "search_web"\n'
            'result = subprocess.run(gate, shell=True, input=json.dumps(payload), text=True, capture_output=True, check=True)\n'
            'assert json.loads(result.stdout)["decision"] == "deny"\n'
            'coverage = os.environ.get("EVAL_GATE_COVERAGE", "matching")\n'
            'transcript = Path.home() / ".gemini/antigravity-cli/brain/fake-conversation/.system_generated/logs/transcript_full.jsonl"\n'
            'events = [json.loads(line) for line in transcript.read_text().splitlines()]\n'
            'if coverage == "missing": events[1]["tool_calls"] += [{"name":"run_command"}, {"name":"run_command"}]\n'
            'if coverage == "different-tool": events[1]["tool_calls"][1] = {"name":"write_to_file"}\n'
            'transcript.write_text("\\n".join(json.dumps(event) for event in events))\n'
            'if coverage == "no-transcript": transcript.unlink()\n'
            'if coverage == "broken-transcript": transcript.write_text("invalid json")\n'
            'if coverage == "no-calls": transcript.write_text(json.dumps({"type":"PLANNER_RESPONSE"}))\n'
            'log = Path.cwd().parent / "agy/gate.log"\n'
            'if coverage == "extra":\n'
            ' subprocess.run(gate, shell=True, input=json.dumps(payload), text=True, capture_output=True, check=True)\n'
            'if coverage == "invalid-decisions":\n'
            ' log.write_text("not json\\n" + json.dumps({"tool":"run_command","decision":"ask"}) + "\\n")\n'
            'Path("main.bnt").write_text(%r)\n' % source +
            'subprocess.run(["benitoite", "check", "main.bnt"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)\n'
            'change = calls.read_text().splitlines().count("trial") == int(os.environ.get("EVAL_CHANGE_ON_TRIAL", "1"))\n'
            'setting = os.environ.get("EVAL_USER_SETTING") if change else None\n'
            'if setting:\n'
            ' p = Path.home() / ".gemini" / setting\n'
            ' if os.environ.get("EVAL_DELETE_SETTING"): p.unlink()\n'
            ' else: p.parent.mkdir(parents=True, exist_ok=True); p.write_text("private-setting-value")\n'
            'if change and os.environ.get("EVAL_VERSION_CHANGE"): version.write_text("fake agy 1.4.2")\n'
            'mutation = os.environ.get("EVAL_MUTATION")\n'
            'targets = {"hooks.json":Path(".agents/hooks.json"), "benitoite Skill":Path(".agents/skills/benitoite/SKILL.md"), "bin/benitoite":Path.cwd().parent / "bin/benitoite", "agy/agy_gate.py":Path.cwd().parent / "agy/agy_gate.py"}\n'
            'if mutation == ".agents entries": Path(".agents").chmod(0o755); Path(".agents/new-file").write_text("added")\n'
            'elif mutation: targets[mutation].chmod(0o644); targets[mutation].write_text("modified")\n'
            'outcome = os.environ.get("EVAL_OUTCOME", "completed")\n'
            'if outcome == "print-timeout": print("[agy] print timeout after 30m0s with turn in progress; returning partial output", file=sys.stderr)\n'
            'if outcome == "agy-error": print(\'AGY_ERROR: {"short_error":"failed"}\', file=sys.stderr)\n'
            'print("invalid" if outcome in ("unreadable", "print-timeout") else %r)\n' % json.dumps(agy_summary()))
        self.write(fake, script)
        os.chmod(fake, 0o755)
        unused = self.write(os.path.join(os.path.dirname(fake), 'claude'),
                            '#!%s\nraise SystemExit("unexpected later harness")\n' % sys.executable)
        os.chmod(unused, 0o755)
        os.environ['PATH'] = os.path.dirname(fake) + os.pathsep + os.environ.get('PATH', '')
        return task, fake

    @unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
    @requires_sandbox
    def test_gate_coverage_compares_tool_counts_and_requires_transcript(self):
        harness = evaluation.load_harnesses()['antigravity-gemini']
        cases = [('matching', None), ('extra', None), ('no-calls', None),
                 ('missing', 'SAFETY: tool calls without a gate decision (2)'),
                 ('different-tool', 'SAFETY: tool calls without a gate decision (1)'),
                 ('invalid-decisions', 'SAFETY: tool calls without a gate decision (3)'),
                 ('no-transcript', 'SAFETY: gate coverage unverifiable'),
                 ('broken-transcript', 'SAFETY: gate coverage unverifiable')]
        for coverage, expected in cases:
            with self.subTest(coverage=coverage):
                task, _ = self.fake_agy()
                os.environ['EVAL_GATE_COVERAGE'] = coverage
                row = evaluation.run_trial(task, os.path.join(self.directory, coverage),
                    os.environ['BENITOITEH_TEST_BENITOITE'], 'antigravity-gemini', harness,
                    str(harness['model']), None, False)
                self.assertEqual(row['harness_exit'], 0, row['reason'])
                self.assertTrue(row['success'], row['reason'])
                if expected:
                    self.assertTrue(row['reason'].startswith(expected), row['reason'])
                else:
                    self.assertNotIn('SAFETY:', row['reason'])

    @unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
    @requires_sandbox
    def test_agy_settings_and_versions_stop_evaluation_and_preserve_records(self):
        # 設定の保存や自動更新を見落とすと、次の試行を違う条件で走らせてしまう。
        cases = [(path, False, False) for path in (
            'config/config.json', 'config/mcp_config.json', 'config/projects/known.json',
            'antigravity-cli/settings.json', 'config/projects/new.json', 'config/new-entry')]
        cases += [(path, True, False) for path in (
            'config/mcp_config.json', 'config/projects/known.json', 'antigravity-cli/settings.json')]
        cases += [(None, False, False), (None, False, True), ('config/config.json', False, True)]
        cases = [(setting, delete, change, 1) for setting, delete, change in cases]
        cases += [('config/config.json', False, False, 2), (None, False, True, 2)]
        for index, (setting, delete, version_change, change_on) in enumerate(cases):
            with self.subTest(setting=setting, delete=delete, version_change=version_change, change_on=change_on):
                shutil.rmtree(os.path.join(self.home, '.gemini'), ignore_errors=True)
                task, fake = self.fake_agy()
                for path, value in (('config/config.json', '{"userSettings":{}}'),
                                    ('config/mcp_config.json', ''), ('config/projects/known.json', '{"id":"known"}'),
                                    ('antigravity-cli/settings.json', '{"model":"gemini"}')):
                    self.write(os.path.join(self.home, '.gemini', path), value)
                for suffix in ('.calls', '.version'):
                    path = os.path.splitext(fake)[0] + suffix
                    if os.path.exists(path):
                        os.unlink(path)
                os.environ.update(EVAL_USER_SETTING=setting or '', EVAL_DELETE_SETTING='1' if delete else '',
                                  EVAL_VERSION_CHANGE='1' if version_change else '', EVAL_CHANGE_ON_TRIAL=str(change_on))
                root = os.path.join(self.directory, 'followup-%d' % index)
                records = os.path.join(root, 'records')
                stopped = setting is not None or version_change
                # 後続の課題と項目にも進まないことを、本物の評価ループで確かめる。
                names = 'antigravity-gemini,claude-code' if stopped else 'antigravity-gemini'
                with mock.patch.object(evaluation, 'RESULTS_DIR', records), \
                        contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    code = evaluation.main(['--harness', names, '--allow-unverified', '--task', task.name,
                        '--task', 'json_convert', '--trials', '2', '--benitoite', os.environ['BENITOITEH_TEST_BENITOITE'],
                        '--work-root', root])
                self.assertEqual(code, 3 if stopped else 0)
                count = change_on if stopped else 4
                with open(os.path.splitext(fake)[0] + '.calls') as f:
                    self.assertEqual(f.read().splitlines(), ['version', 'trial', 'version'] * count)
                rows = evaluation.ALL_ROWS
                self.assertEqual(len(rows), count)
                row = rows[-1] if stopped else rows[0]
                self.assertTrue(row['success'], row['reason'])
                self.assertEqual(row['harness_version_start'], 'fake agy 1.4.0')
                self.assertEqual(row['harness_version_end'], 'fake agy 1.4.2' if version_change else 'fake agy 1.4.0')
                if setting:
                    self.assertTrue(row['reason'].startswith('SAFETY: user Antigravity settings changed'), row['reason'])
                if version_change:
                    self.assertIn('WARNING: harness version changed fake agy 1.4.0 -> fake agy 1.4.2', row['reason'])
                if stopped and change_on == 2:
                    self.assertNotIn('settings changed', rows[0]['reason'])
                    self.assertNotIn('version changed', rows[0]['reason'])
                for name in os.listdir(records):
                    with open(os.path.join(records, name)) as f:
                        text = f.read()
                    self.assertNotIn('private-setting-value', text)
                    if name.endswith('.jsonl'):
                        self.assertEqual(len(text.splitlines()), len(rows))
                        saved = [json.loads(line) for line in text.splitlines()]
                        self.assertEqual(saved[-1 if stopped else 0]['harness_version_end'], row['harness_version_end'])
                    else:
                        self.assertIn('antigravity-gemini（fake agy 1.4.0）', text)
                        self.assertEqual('- 停止:' in text, stopped)
                if stopped:
                    self.assertFalse(os.path.exists(os.path.join(root, 'claude-code')))
                self.assertEqual(len(os.listdir(records)), 4 if stopped else 2)

    @unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
    @requires_sandbox
    def test_fake_agy_trial_hooks_integrity_records_and_environment(self):
        task, fake = self.fake_agy()
        cases = [(mutation, 'completed') for mutation in (
            None, 'hooks.json', 'benitoite Skill', '.agents entries', 'bin/benitoite', 'agy/agy_gate.py')]
        cases += [(None, outcome) for outcome in ('unreadable', 'print-timeout', 'agy-error')]
        for mutation, outcome in cases:
            with self.subTest(mutation=mutation, outcome=outcome):
                os.environ['EVAL_MUTATION'] = mutation or ''
                os.environ['EVAL_OUTCOME'] = outcome
                # 空白と引用符を含む実のパスを、生成したフックで受け渡せることも確かめる。
                root = os.path.join(self.directory, "work's files", mutation or outcome)
                record = os.path.join(root, 'record.md')
                with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    code = evaluation.main(['--harness', 'antigravity-gemini', '--allow-unverified',
                        '--task', task.name, '--trials', '1', '--benitoite', os.environ['BENITOITEH_TEST_BENITOITE'],
                        '--work-root', root, '--record', record, '--stop-cost', '.01'])
                self.assertEqual(code, 0)
                with open(os.path.splitext(record)[0] + '.jsonl') as f:
                    row = json.loads(f.readline())
                with open(os.path.join(root, task.name, '1/harness.stderr')) as f:
                    self.assertEqual(row['harness_exit'], 0, f.read())
                self.assertTrue(row['success'], row['reason'])
                self.assertEqual(row['harness_exit'], 0)
                unreadable = outcome in ('unreadable', 'print-timeout')
                self.assertEqual(row['total_tokens'], None if unreadable else 420)
                self.assertEqual(row['turns'], None if unreadable else 2)
                self.assertEqual(row['attempts'], 1)
                self.assertEqual(row['end'], {'completed': 'completed', 'unreadable': None,
                                             'print-timeout': 'timeout', 'agy-error': 'error'}[outcome])
                self.assertIsNone(row['cost_usd'])
                self.assertEqual(row['billing'], 'none')
                trial = os.path.join(root, task.name, '1')
                with open(os.path.join(trial, 'final.txt')) as f:
                    self.assertEqual(f.read(), '' if unreadable else 'Done.')
                if unreadable:
                    self.assertIn('harness summary unreadable', row['reason'])
                if outcome == 'agy-error':
                    self.assertIn('AGY_ERROR:', row['reason'])
                if mutation:
                    self.assertTrue(row['reason'].startswith('SAFETY: %s was modified' % mutation), row['reason'])
                elif unreadable:
                    self.assertTrue(row['reason'].startswith('SAFETY: gate coverage unverifiable'), row['reason'])
                else:
                    self.assertNotIn('SAFETY', row['reason'])
                with open(record) as f:
                    markdown = f.read()
                for text in ('fake agy 1.4.0', 'Antigravity の利用者設定の確認: passed', 'product-skill',
                             'AGY_OTHER', 'ANTIGRAVITY_TEST', 'JETSKI_TEST', 'GOOGLE_TEST', 'GEMINI_TEST',
                             'CLOUDSDK_TEST', '1860'):
                    self.assertIn(text, markdown)
                for text in (self.home, 'private-agy-value', 'private-antigravity-value', 'private-jetski-value',
                             'private-google-value', 'private-gemini-value', 'private-cloudsdk-value'):
                    self.assertNotIn(text, markdown)
        # dry-run は混入した設定があっても、agy とその版の問い合わせを起動しない。
        self.write(os.path.join(self.home, 'AGENTS.md'), 'private')
        os.unlink(fake)
        row = evaluation.run_trial(task, os.path.join(self.directory, 'dry-run'),
            os.environ['BENITOITEH_TEST_BENITOITE'], 'antigravity-gemini',
            evaluation.load_harnesses()['antigravity-gemini'], 'unused', None, True)
        self.assertTrue(row['success'], row['reason'])
        self.assertNotIn('agy_config_checked', row)

@unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
class StagedTests(unittest.TestCase):
    # 関門: 段の実行器の境界で、再開・資料の公開順・全段の集計を守る。
    # 既存の単発の課題では届かない契約であり、偽の exec も本物の子として起動する。
    SOURCE = '''import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.File
import Benitoite.Unofficial.IO.Process
function main() -> Result[Unit, String] uses File.Read, File.Write, Console.Write, Process.Environment
  bind path <- Option.unwrapOr(o: List.head(Process.arguments()), fallback: "data.txt")
  bind value <- try File.readText(path) |> Result.mapError(r: _, f: lambda(error) return IOError.message(error) end lambda) or return
  Console.writeLine(String.trim(value))
  bind _ <- try File.writeText(path: "report.json", text: "{\\"ok\\":true}") |> Result.mapError(r: _, f: lambda(error) return IOError.message(error) end lambda) or return
  return Result.Ok(())
end function
'''

    def setUp(self):
        # 通しで使う Unix ソケットのパスを短くする。CLI の work-root の検証とは別の準備である。
        temporary = tempfile.TemporaryDirectory(prefix='sc-', dir='/tmp')
        self.addCleanup(temporary.cleanup)
        self.root = temporary.name
        self.tasks = os.path.join(self.root, 'tasks')
        self.task_dir = os.path.join(self.tasks, 'staged')
        os.makedirs(self.task_dir)
        self.patch = mock.patch.object(evaluation, 'TASKS_DIR', self.tasks)
        self.patch.start()
        self.addCleanup(self.patch.stop)
        self.write('task.md', 'Complete three requests in order.\n')
        self.write('reference.bnt', self.SOURCE)
        self.write('negative/forget.bnt', self.SOURCE.replace('String.trim(value)',
                   'if String.trim(value) = "early" then "wrong" else String.trim(value) end if'))
        self.write('input/data.txt', 'original\n')
        for n in range(1, 4):
            self.write('stage%d.md' % n, 'Request %d.\n' % n)
            self.write('materials/%d/example.md' % n, 'Example %d\n' % n)
        self.config = dict(kind='staged', result_kind='run', script='main.bnt', attempt_limit=10,
                           http=False, stages=['stage1.md', 'stage2.md', 'stage3.md'], judge_cases=[],
                           self_test_failures=[dict(script='negative/forget.bnt', case='early')])
        for name in ('early', 'late'):
            self.write('cases/%s/input/alternate.txt' % name, name + '\n')
            self.write('cases/%s/expected.stdout' % name, name + '\n')
            self.write('cases/%s/expected.files/report.json' % name, '{ "ok": true }\n')
            self.config['judge_cases'].append(dict(name=name, input='cases/%s/input' % name,
                args=['alternate.txt'], expected_stdout='cases/%s/expected.stdout' % name,
                expected_files='cases/%s/expected.files' % name))
        self.save_config()
        self.processor = os.environ['BENITOITEH_TEST_BENITOITE']
        self.harness = copy.deepcopy(evaluation.load_harnesses()['benitoiteh-mock-compact'])
        self.harness.pop('mock')

    def write(self, relative, text):
        path = os.path.join(self.task_dir, relative)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'w', encoding='utf-8') as f:
            f.write(text)

    def save_config(self):
        self.write('task.json', json.dumps(self.config))

    def fake_exec(self, mode='ok'):
        executable = os.path.join(self.root, 'fake-exec')
        # 公開のイベントと記録を手で用意する。summary の使用量には既に要約分が入っている。
        source = '''#!{python}
import hashlib, json, os, sys
from pathlib import Path
trial = Path.cwd().parent
log = trial / 'invocations.jsonl'
number = len(log.read_text().splitlines()) + 1 if log.exists() else 1
args = sys.argv[1:]
assert '--project' not in args
assert os.environ['HOME'] == {home!r}
config = Path(os.environ['XDG_CONFIG_HOME']) / 'benitoiteh/config.toml'
with log.open('a') as f:
    f.write(json.dumps(dict(args=args, materials=sorted(str(p) for p in Path('materials').rglob('*.md')),
                           config_hash=hashlib.sha256(config.read_bytes()).hexdigest(),
                           env=[os.environ[k] for k in ('XDG_CONFIG_HOME','XDG_STATE_HOME','TMPDIR')],
                           parent_environment={{k:v for k,v in os.environ.items() if k in ('PWD','OLDPWD') or k.startswith('EVAL_')}})) + '\\n')
Path('main.bnt').write_text({source!r})
session = Path(os.environ['XDG_STATE_HOME']) / 'benitoiteh/sessions/test-session'
session.mkdir(parents=True, exist_ok=True)
events = [dict(type='session_start', session_id='test-session', resumed=number>1,
               provider='eval', base_url='http://example.invalid', model='mock', mode='restricted',
               benitoiteh_version='test', system_prompt_sha256='prompt', tools_sha256='tools',
               compaction=dict(reserve_tokens=16384, keep_tokens=20000), completion_check=False)]
mode = {mode!r}
if number == 2 and mode == 'identity': events[0]['model'] = 'changed'
if mode == 'reserve-zero-kept': events[0]['compaction']['reserve_tokens'] = 8192
turn = number * 2 - 1
compact = dict(type='compaction', reason='threshold', summarized_items=5, kept_items=2, kept_estimate=500,
               usage=dict(input_tokens=100, output_tokens=10), estimate_before=45000, estimate_after=15000)
if number == 1: events.append(compact)
if mode in ('zero-kept', 'reserve-zero-kept'):
    compact['kept_items'], compact['kept_estimate'] = 0, 0
if number == 2: events.append(dict(compact, reason='resume'))
events.append(dict(type='turn_start', turn=turn))
events.append(dict(type='turn_end', turn=turn))
events.append(dict(type='turn_start', turn=turn+1))
if number == 1: events.append(dict(compact, reason='overflow'))
events.append(dict(type='text_delta', turn=turn+1, text='Done ' + str(number)))
events.append(dict(type='turn_end', turn=turn+1))
if number == 1: events.append(compact)
if mode == 'no-compaction': events = [event for event in events if event['type'] != 'compaction']
records = [{{k:v for k,v in event.items() if k not in ('estimate_before','estimate_after')}} for event in events if event['type']=='compaction']
if mode == 'records' and number == 1: records = []
if mode.startswith('repo-'):
    leak = dict(type='tool_call', call=dict(id='leak', name='read', arguments=dict(path={repo!r}+'/tools/skill-eval/tasks/reference.bnt')))
    if mode == 'repo-spill':
        (session / 'spill.txt').write_text('read '+{repo!r})
        leak = dict(type='tool_result', name='benitoite_run', result=dict(id='leak', text='ok'), spill=dict(path='spill.txt'))
    records.append(leak)
records += [dict(type='tool_call', call=dict(id='call'+str(number), name='benitoite_run', arguments=dict(path='main.bnt'))),
            dict(type='tool_result', name='benitoite_run', result=dict(id='call'+str(number), text='ok')),
            dict(type='summary', summary=dict(turns=9999, tokens=dict(input=999999999, output=999999999)))]
with (session / 'session.jsonl').open('a') as f:
    for record in records: f.write(json.dumps(record)+'\\n')
events.append(dict(type='session_end', outcome='cancelled' if mode=='cancelled' else 'completed'))
turns = 50 if mode=='turns' else 2
tokens = 2000000 if mode=='tokens' else 3000
events.append(dict(type='summary', turns=turns, compactions=sum(e['type']=='compaction' for e in events),
                   reason='done', tool_calls=dict(benitoite_run=1), after_write_checks=0, checks=1, fixes=0,
                   approvals=dict(policy_denied=0), tokens=dict(input=None if mode.startswith('unknown') else tokens,
                   output=150, cache_read=None if mode=='cache-unknown' else 600, cache_write=300)))
for event in events: print(json.dumps(event))
if mode=='exit': sys.exit(1)
'''.format(python=sys.executable, source=self.SOURCE, home=os.environ['HOME'], mode=mode, repo=evaluation.REPO)
        with open(executable, 'w') as f:
            f.write(source)
        os.chmod(executable, 0o755)
        return executable

    def run_trial(self, mode='ok', budget=None):
        return evaluation.run_trial(evaluation.Task('staged'), os.path.join(self.root, 'trial-' + mode),
            self.processor, 'fake', self.harness, 'mock', None, False, self.fake_exec(mode), budget=budget)

    def test_stage_runner_resume_limits_and_costs_without_judgment(self):
        # 関門: 判定のサンドボックスを要しない実行器の境界で、資料の順・再開・上限・費用を守る。
        # 成果物の合否とセッションの照合は外の run_trial のテストが受け持つ。
        for mode, launches, end in (('ok', 3, 'completed'), ('turns', 1, 'max_turns'),
                                   ('tokens', 1, 'max_tokens'), ('identity', 2, 'error'),
                                   ('exit', 1, 'error'), ('cache-unknown', 1, 'cost_limit'),
                                   ('already-spent', 0, 'cost_limit'), ('cost', 2, 'cost_limit'),
                                   ('missing', 1, 'error'), ('unknown', 1, 'error')):
            with self.subTest(mode=mode):
                trial = os.path.join(self.root, 'runner-' + mode)
                workspace = os.path.join(trial, 'workspace')
                os.makedirs(workspace)
                env = dict(os.environ)
                for key in ('XDG_CONFIG_HOME', 'XDG_STATE_HOME', 'TMPDIR'):
                    env[key] = os.path.join(trial, key)
                    os.mkdir(env[key])
                config = os.path.join(env['XDG_CONFIG_HOME'], 'benitoiteh')
                os.mkdir(config)
                with open(os.path.join(config, 'config.toml'), 'w') as f:
                    f.write('test config')
                harness = copy.deepcopy(self.harness)
                budget = evaluation.CostBudget(1)
                if mode in ('cache-unknown', 'already-spent', 'cost', 'missing', 'unknown'):
                    harness.update(price=dict(input=1, cache_read=1, cache_write=1, output=1), billing='paid')
                if mode == 'cost':
                    budget = evaluation.CostBudget(.005)
                if mode == 'already-spent':
                    budget.totals['paid'] = 1
                executable = self.root + '/missing' if mode == 'missing' else self.fake_exec(mode)
                fields, reason, final, code, _timeout, _wall = evaluation.run_stages(
                    evaluation.Task('staged'), trial, workspace, executable, env,
                    'Console.Write,File.Read,File.Write,Process.Environment', harness, 'mock', budget)
                self.assertEqual(fields['exec_launches'], launches, reason)
                self.assertEqual(fields['end'], end, reason)
                self.assertEqual(budget.totals, {'paid': 1} if mode == 'already-spent' else {})
                if mode == 'cost':
                    self.assertAlmostEqual(sum(stage['cost_usd'] for stage in fields['stages']), .0063)
                if mode == 'missing':
                    self.assertIn('cannot start', reason)
                    self.assertIn('cost unknown', fields['budget_stop'])
                if mode == 'ok':
                    self.assertEqual((fields['turns'], fields['total_tokens'], fields['compactions']), (6, 9450, 4))
                    self.assertEqual((code, final), (0, 'Done 3'))
                    self.assertEqual([stage['limits']['turns'] for stage in fields['stages']], [50, 48, 46])
                    self.assertEqual([stage['limits']['tokens'] for stage in fields['stages']],
                                     [2000000, 1996850, 1993700])
                    with open(os.path.join(trial, 'invocations.jsonl')) as f:
                        invocations = [json.loads(line) for line in f]
                    self.assertEqual([row['materials'] for row in invocations],
                                     [['materials/%d/example.md' % n for n in range(1, stage+1)]
                                      for stage in range(1, 4)])
                    for row in invocations[1:]:
                        self.assertEqual(row['args'][1:3], ['--resume', 'test-session'])
                        self.assertEqual(row['env'], invocations[0]['env'])
                        self.assertEqual(row['config_hash'], invocations[0]['config_hash'])

    def test_suite_selection_and_unsafe_declared_paths(self):
        original = os.path.join(evaluation.HERE, 'tasks')
        for entry in os.listdir(original):
            if os.path.isfile(os.path.join(original, entry, 'task.json')):
                shutil.copytree(os.path.join(original, entry), os.path.join(self.tasks, entry))
        self.assertEqual(len(evaluation.load_tasks([])), 11)
        self.assertEqual([task.name for task in evaluation.load_tasks([], 'compaction')],
                         ['compact_dir_walk', 'compact_json_summary', 'staged'])
        with self.assertRaisesRegex(SystemExit, 'outside suite'):
            evaluation.load_tasks(['staged'])
        with self.assertRaisesRegex(SystemExit, 'outside suite'):
            evaluation.main(['--dry-run', '--suite', 'compaction', '--task', 'json_summary'])
        os.symlink('stage1.md', os.path.join(self.task_dir, 'link.md'))
        os.symlink('materials/1', os.path.join(self.task_dir, 'linked'))
        os.mkfifo(os.path.join(self.task_dir, 'pipe.md'))
        for path in ('../stage.md', '/stage.md', 'link.md', 'linked/example.md', 'pipe.md'):
            with self.subTest(path=path):
                self.config['stages'][0] = path
                self.save_config()
                with self.assertRaises((ValueError, OSError)):
                    evaluation.Task('staged')
        self.config['stages'][0] = 'stage1.md'
        self.config['judge_cases'][0]['input'] = 'cases/early/input/../input'
        self.save_config()
        with self.assertRaises(ValueError):
            evaluation.Task('staged')

    @requires_sandbox
    def test_cases_arguments_json_and_named_negative_controls_and_dry_run(self):
        task = evaluation.Task('staged')
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(evaluation.self_test([task], self.processor), 0)
        self.config['self_test_failures'][0]['case'] = 'late'
        self.save_config()
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(evaluation.self_test([evaluation.Task('staged')], self.processor), 1)
        row = evaluation.run_trial(task, os.path.join(self.root, 'dry'), self.processor,
                                   'mock', self.harness, 'mock', None, True)
        self.assertTrue(row['success'], row['reason'])
        self.assertTrue(row['artifact_success'])
        self.assertEqual(row['exec_launches'], 0)
        self.assertFalse(row['comparison_valid'])
        self.assertTrue(os.path.isfile(os.path.join(self.root, 'dry/workspace/materials/3/example.md')))
        with tempfile.TemporaryDirectory(prefix='sc-cli-') as work, contextlib.redirect_stdout(io.StringIO()):
            status = evaluation.main(['--dry-run', '--suite', 'compaction', '--agent', 'benitoiteh-mock-compact',
                                      '--benitoite', self.processor, '--work-root', work])
            self.assertEqual(status, 0)
            self.assertEqual(len(evaluation.ALL_ROWS), 1)
            with open(os.path.join(work, 'record.jsonl')) as f:
                recorded = [json.loads(line) for line in f]
            self.assertEqual(len(recorded), 1)
            self.assertFalse(recorded[0]['comparison_valid'])
        verdict = evaluation.judge(task, self.SOURCE.replace('String.trim(value)', '"wrong"'), self.processor, None)
        self.assertIn('early: standard output differs', verdict.reason)
        self.assertIn('late: standard output differs', verdict.reason)

    @requires_sandbox
    def test_resume_material_order_limits_single_record_and_event_turns(self):
        real_run = evaluation.run_harness
        def measured(*args, **kwargs):
            code, timeout, _wall = real_run(*args, **kwargs)
            return code, timeout, 2.5
        with mock.patch.object(evaluation, 'run_harness', side_effect=measured):
            row = self.run_trial()
        self.assertTrue(row['success'], row['reason'])
        self.assertEqual((row['turns'], row['attempts'], row['total_tokens'], row['compactions']), (6, 3, 9450, 4))
        self.assertEqual(row['wall_seconds'], 7.5)
        self.assertEqual(row['verification_calls_before']['benitoite_run'], 3)
        self.assertEqual(row['exec_launches'], 3)
        self.assertEqual([(c['stage'], c['turn']) for c in row['compaction_events']],
                         [(1, 1), (1, 2), (1, None), (2, 3)])
        self.assertEqual(row['compaction_events'][2]['turn_reason'], 'no subsequent turn started')
        self.assertTrue(row['compaction_records_match'])
        self.assertTrue(row['comparison_valid'])
        self.assertEqual(row['compaction_threshold'], 40960)
        with open(os.path.join(row['trial_dir'], 'invocations.jsonl')) as f:
            invocations = [json.loads(line) for line in f]
        for number, invocation in enumerate(invocations, 1):
            args = invocation['args']
            self.assertEqual(args[0], 'exec')
            self.assertEqual(args[args.index('--max-turns')+1], str(50-2*(number-1)))
            self.assertEqual(args[args.index('--max-run-tokens')+1], str(2000000-3150*(number-1)))
            self.assertEqual(invocation['materials'], ['materials/%d/example.md' % n for n in range(1, number+1)])
            self.assertEqual(invocation['env'], invocations[0]['env'])
            self.assertEqual(invocation['config_hash'], invocations[0]['config_hash'])
            if number > 1:
                self.assertEqual(args[1:3], ['--resume', 'test-session'])
                self.assertEqual(args[-1], 'Request %d.\n' % number)
            else:
                self.assertTrue(args[-1].startswith(evaluation.PROMPT.format(limit=10)+'\n\n'))
        self.assertEqual([stage['limits']['seconds'] for stage in row['stages']], [1800, 1797.5, 1795])
        path = os.path.join(self.root, 'result.md')
        evaluation.write_record(path, {}, [row], [evaluation.Task('staged')])
        with open(os.path.join(self.root, 'result.jsonl')) as f:
            self.assertEqual(len(f.readlines()), 1)

    @requires_sandbox
    def test_stage_stops_and_partial_cost_never_double_counted(self):
        for mode, count in (('turns', 1), ('tokens', 1), ('cancelled', 1), ('identity', 2), ('exit', 1)):
            with self.subTest(mode=mode):
                row = self.run_trial(mode)
                self.assertEqual(row['exec_launches'], count)
                self.assertFalse(row['success'])
                self.assertFalse(row['comparison_valid'])
                self.assertFalse(os.path.exists(os.path.join(row['trial_dir'], 'workspace/materials/3')))
        self.harness.update(price=dict(input=1, cache_read=1, cache_write=1, output=1), billing='paid')
        budget = evaluation.CostBudget(.01)
        budget.totals['paid'] = .005
        row = self.run_trial('cost', budget)
        self.assertEqual(row['exec_launches'], 2)
        self.assertAlmostEqual(row['cost_usd'], .0063)
        self.assertEqual(budget.totals, {'paid': .005})
        self.assertTrue(budget.add(row))
        self.assertAlmostEqual(budget.totals['paid'], .0113)
        row = self.run_trial('unknown', evaluation.CostBudget(1))
        self.assertEqual(row['exec_launches'], 1)
        self.assertIsNone(row['cost_usd'])
        self.assertIn('cost unknown', row['budget_stop'])
        row = self.run_trial('unknown-unbounded')
        self.assertEqual(row['exec_launches'], 1)
        self.assertIn('cost unknown', row['budget_stop'])
        row = self.run_trial('records')
        self.assertFalse(row['compaction_records_match'])
        self.assertFalse(row['success'])

    @requires_sandbox
    def test_time_attempt_limits_and_comparison_failures_keep_artifact_verdict(self):
        real_run = evaluation.run_harness
        def elapsed(*args, **kwargs):
            code, _timeout, _wall = real_run(*args, **kwargs)
            return code, False, 1800
        with mock.patch.object(evaluation, 'run_harness', side_effect=elapsed):
            row = self.run_trial('time')
        self.assertEqual((row['exec_launches'], row['end'], row['timed_out']), (1, 'timeout', True))
        for mode, reason in (('no-compaction', 'no compaction occurred'),
                             ('zero-kept', 'no nonzero retained range across requests')):
            with self.subTest(mode=mode):
                row = self.run_trial(mode)
                self.assertTrue(row['success'], row['reason'])
                self.assertTrue(row['artifact_success'])
                self.assertFalse(row['comparison_valid'])
                self.assertIn(reason, row['comparison_reasons'])
        self.harness['provider']['context_window'] = 16384
        row = self.run_trial('clamped')
        self.assertIn('reserve settings produce the same effective reserve', row['comparison_reasons'])
        self.config['attempt_limit'] = 2
        self.save_config()
        row = self.run_trial('attempts')
        self.assertEqual(row['exec_launches'], 3)
        self.assertFalse(row['success'])
        self.assertIn('3 attempts exceed the limit 2', row['reason'])

    # 関門: 許可を越えるコードの実行、親の秘密・評価の原本の漏れ、誤った否定の対照を、
    # 判定と試行の公開の入口で捕まえる。期待値は処理系の出力から生成しない。
    def test_judgment_rejects_extra_effects_before_running(self):
        marker = os.path.join(self.root, 'must-not-exist')
        malicious = self.SOURCE.replace('Process.Environment\n', 'Process.Environment, Process.Run\n')
        malicious = malicious.replace('  bind path <-',
            '  bind _ <- File.writeText(path: %s, text: "executed")\n  bind path <-' % json.dumps(marker))
        cases = [(evaluation.Task('staged'), malicious)]
        for name in ('json_summary', 'write_tests'):
            shutil.copytree(os.path.join(evaluation.HERE, 'tasks', name), os.path.join(self.tasks, name))
            task = evaluation.Task(name)
            source = malicious
            if task.result_kind == 'test':
                with open(task.path('reference.bnt')) as f:
                    source = f.read()
                source += ('\n@test("forbidden write")\nfunction forbiddenWrite() -> Unit uses File.Write\n'
                           ' bind _ <- File.writeText(path: %s, text: "executed")\nend function\n') % json.dumps(marker)
            cases.append((task, source))
        for task, source in cases:
            with self.subTest(task=task.name):
                verdict = evaluation.judge(task, source, self.processor, None)
                self.assertFalse(verdict.success)
                self.assertIn('effects outside task allow:', verdict.reason)
                self.assertFalse(os.path.exists(marker))

    @requires_sandbox
    def test_judgment_does_not_inherit_parent_environment(self):
        # Process.Environment はこの課題の許可内。親の無関係な変数も判定には渡さない。
        environment = self.SOURCE.replace('  bind path <-', '''  bind secret <- Process.environmentVariable("EVAL_PARENT_SECRET")
  Console.writeLine(Option.unwrapOr(o: Result.unwrapOr(r: secret, fallback: Option.None), fallback: "absent"))
  bind path <-''')
        for name in ('early', 'late'):
            self.write('cases/%s/expected.stdout' % name, 'absent\n' + name + '\n')
        with mock.patch.dict(os.environ, EVAL_PARENT_SECRET='must-not-inherit'):
            verdict = evaluation.judge(evaluation.Task('staged'), environment, self.processor, None)
        self.assertTrue(verdict.success, verdict.reason)

    @requires_sandbox
    def test_negative_controls_require_output_or_file_mismatch(self):
        for source, kind in (('this is not Benitoite', 'check'),
                             (self.SOURCE.replace('"data.txt"', '"missing.txt"')
                              .replace('Process.arguments()', '[]'), 'run')):
            with self.subTest(source=source[:25]), contextlib.redirect_stdout(io.StringIO()):
                self.write('negative/forget.bnt', source)
                self.assertEqual(evaluation.judge(evaluation.Task('staged'), source, self.processor, None,
                                                 'early').failure_kind, kind)
                self.assertEqual(evaluation.self_test([evaluation.Task('staged')], self.processor), 1)

    def test_staged_manifest_validates_task_text_and_root_expectations(self):
        self.write('expected.stdout', 'ignored expectation\n')
        with self.assertRaisesRegex(ValueError, 'expected.stdout'):
            evaluation.Task('staged')
        os.unlink(self.task_dir + '/expected.stdout')
        os.mkdir(self.task_dir + '/expected.files')
        with self.assertRaisesRegex(ValueError, 'expected.files'):
            evaluation.Task('staged')
        os.rmdir(self.task_dir + '/expected.files')
        os.unlink(self.task_dir + '/task.md')
        with self.assertRaises((ValueError, OSError)):
            evaluation.Task('staged')

    def test_default_suite_ignores_invalid_unselected_staged_tasks(self):
        self.config['stages'][0] = '../forbidden.md'
        self.save_config()
        shutil.copytree(os.path.join(evaluation.HERE, 'tasks/json_summary'), os.path.join(self.tasks, 'json_summary'))
        self.assertEqual([task.name for task in evaluation.load_tasks([])], ['json_summary'])
        self.write('task.json', '{bad json')
        self.assertEqual([task.name for task in evaluation.load_tasks([])], ['json_summary'])
        with self.assertRaises((ValueError, SystemExit)):
            evaluation.load_tasks([], 'compaction')

    @requires_sandbox
    def test_budget_before_first_launch_and_cost_only_unknown_and_runner_failure(self):
        self.harness.update(price=dict(input=1, cache_read=1, cache_write=1, output=1), billing='paid')
        budget = evaluation.CostBudget(.5)
        budget.totals['paid'] = .5
        row = self.run_trial('already-spent', budget)
        self.assertEqual(row['exec_launches'], 0)
        self.assertFalse(os.path.exists(os.path.join(row['trial_dir'], 'workspace/materials')))
        row = self.run_trial('cache-unknown', evaluation.CostBudget(1))
        self.assertEqual(row['exec_launches'], 1)
        self.assertEqual(row['total_tokens'], 3150)
        self.assertIsNone(row['stages'][0]['runner_error'])
        self.assertIsNone(row['cost_usd'])
        self.assertEqual(row['end'], 'cost_limit')
        with mock.patch.object(self, 'fake_exec', return_value=self.root + '/missing-executable'):
            row = self.run_trial('missing')
        self.assertEqual(row['end'], 'error')
        self.assertIn('cannot start', row['reason'])
        self.assertIn('missing-executable', row['reason'])
        self.assertIn('cost unknown', row['budget_stop'])

    @requires_sandbox
    def test_reserve_comparison_does_not_require_retention(self):
        self.harness['config']['compaction']['reserve_tokens'] = 8192
        row = self.run_trial('reserve-zero-kept')
        self.assertTrue(row['comparison_valid'], row['comparison_reasons'])

    @requires_sandbox
    def test_repo_paths_are_removed_from_harness_env_and_warn_on_session_evidence(self):
        repo = evaluation.REPO
        aliases = evaluation.path_prefixes(repo)
        with mock.patch.dict(os.environ, PWD=repo, OLDPWD='/elsewhere', EVAL_REPO='prefix:' + aliases[-1],
                             EVAL_IMPORT_PATH=repo + '/tools', EVAL_KEEP='ordinary-value'):
            row = self.run_trial('environment')
        self.assertTrue(row['success'], row['reason'])
        with open(os.path.join(row['trial_dir'], 'invocations.jsonl')) as f:
            invocation = json.loads(f.readline())
        self.assertEqual(invocation['parent_environment'], {'EVAL_KEEP': 'ordinary-value'})
        for mode in ('repo-call', 'repo-spill'):
            with self.subTest(mode=mode):
                row = self.run_trial(mode)
            self.assertTrue(row['artifact_success'])
            self.assertTrue(row['repository_path_seen'])
            self.assertTrue(row['comparison_excluded'])
            self.assertFalse(row['comparison_valid'])
            self.assertIn('WARNING: repository path seen in session', row['reason'])

    @unittest.skipUnless(os.environ.get('BENITOITEH_SKILL_EVAL_STAGED_E2E') == '1',
                         'staged mock pass needs the benitoiteh sandbox outside Codex')
    @requires_sandbox
    def test_real_resume_compaction_and_session_records(self):
        # この層固有の契約は、実際の exec の再開と圧縮の記録までの接続である。
        # 待ち受けに失敗する環境では、提供者の模擬の既存のテストと同じく除外する。
        start = mock_provider.start
        def available(*args, **kwargs):
            try:
                return start(*args, **kwargs)
            except OSError as error:
                self.skipTest('loopback listening is unavailable: %s' % error)
        harness = evaluation.load_harnesses()['benitoiteh-mock-compact']
        # 小さい既存の準備データは、提供者の使用量で道具の途中の圧縮を起こす。
        # 実課題は低い固定値と複数の読み取りにより、再開のバイト数で圧縮する。
        harness['mock_input_tokens'] = 45000
        with mock.patch.object(mock_provider, 'start', side_effect=available):
            row = evaluation.run_trial(evaluation.Task('staged'), os.path.join(self.root, 'real'), self.processor,
                'mock-compact', harness, 'mock', None, False, os.environ['BENITOITEH_SKILL_EVAL_BINARY'])
        self.assertTrue(row['success'], row['reason'])
        self.assertEqual(row['exec_launches'], 3)
        self.assertGreater(row['compactions'], 0)
        self.assertTrue(row['compaction_records_match'])
        self.assertGreaterEqual(row['mock_summary_requests'], row['compactions'])
        self.assertEqual(row['total_tokens'], sum(stage['total_tokens'] for stage in row['stages']))
        self.assertEqual(row['cost_usd'], 0)


class CompactionTaskTests(unittest.TestCase):
    # 関門: 具体的な課題の生成・公開順と exec の接続が持ち主である。
    # 一時的な段のテストは、資料の実量による再開時の圧縮と残す量の差を捕まえない。
    def test_generation_reproduces_checked_in_bytes_and_publishes_one_stage_at_a_time(self):
        for name in materials.TASKS:
            task = evaluation.Task(name)
            with open(task.path('task.json')) as f:
                config = json.load(f)
            with tempfile.TemporaryDirectory(prefix='cm-', dir='/tmp') as temporary:
                generated = os.path.join(temporary, name)
                os.mkdir(generated)
                shutil.copyfile(task.path('task.json'), os.path.join(generated, 'task.json'))
                command = [sys.executable, os.path.join(evaluation.HERE, 'generate_compaction_materials.py'),
                           '--task', name, '--tasks-dir', temporary]
                with self.subTest(task=name):
                    for _repeat in range(2):
                        result = subprocess.run(command, capture_output=True, text=True)
                        self.assertEqual(result.returncode, 0, result.stderr)
                        result = subprocess.run(command + ['--check'], capture_output=True, text=True)
                        self.assertEqual(result.returncode, 0, result.stderr)
                    with open(os.path.join(generated, 'task.json')) as f:
                        self.assertEqual(json.load(f)['generation'], config['generation'])
                    workspace = os.path.join(temporary, 'workspace')
                    evaluation.prepare_inputs(task, workspace)
                    self.assertFalse(os.path.exists(os.path.join(workspace, 'materials')))
                    for stage in range(1, 4):
                        prompt = evaluation.prepare_stage(task, workspace, stage)
                        self.assertLessEqual(len(evaluation.prepare_stage_prompt(task, stage).encode()), 4096)
                        self.assertTrue(prompt)
                        for later in range(stage + 1, 4):
                            self.assertFalse(os.path.exists(os.path.join(workspace, 'materials', str(later))))
                        for path in evaluation.stage_materials(task, stage):
                            with open(os.path.join(workspace, path), 'rb') as f:
                                data = f.read()
                            with open(os.path.join(generated, path), 'rb') as f:
                                self.assertEqual(data, f.read())
                            self.assertLessEqual(len(data), 12 * 1024)
                            self.assertEqual(hashlib.sha256(data).hexdigest(),
                                             config['generation']['files_sha256'][path])
                    changed = os.path.join(generated, evaluation.stage_materials(task, 1)[0])
                    with open(changed, 'ab') as f:
                        f.write(b'changed')
                    result = subprocess.run(command + ['--check'], capture_output=True, text=True)
                    self.assertNotEqual(result.returncode, 0)

    @requires_sandbox
    def test_real_task_references_negative_controls_and_dry_run(self):
        tasks = evaluation.load_tasks([], 'compaction')
        processor = os.environ['BENITOITEH_TEST_BENITOITE']
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(evaluation.self_test(tasks, processor), 0)
        harness = evaluation.load_harnesses()['benitoiteh-mock-compact']
        for task in tasks:
            with self.subTest(task=task.name), tempfile.TemporaryDirectory(prefix='cd-', dir='/tmp') as root:
                row = evaluation.run_trial(task, os.path.join(root, 'trial'), processor,
                    'mock-compact', harness, 'mock', None, True)
                self.assertTrue(row['success'], row['reason'])
                self.assertTrue(row['artifact_success'])
                self.assertFalse(row['comparison_valid'])
                self.assertIn('dry-run; no harness execution', row['comparison_reasons'])

    @unittest.skipUnless(os.environ.get('BENITOITEH_SKILL_EVAL_STAGED_E2E') == '1',
                         'staged mock pass needs the benitoiteh sandbox outside Codex')
    @requires_sandbox
    def test_real_tasks_retain_multiple_read_results_and_distinguish_settings(self):
        start = mock_provider.start
        def available(*args, **kwargs):
            try:
                return start(*args, **kwargs)
            except OSError as error:
                self.skipTest('loopback listening is unavailable: %s' % error)
        settings = [('base', 16384, 20000), ('reserve', 8192, 20000), ('keep', 16384, 8000)]
        for task in evaluation.load_tasks([], 'compaction'):
            rows = {}
            with tempfile.TemporaryDirectory(prefix='ce-', dir='/tmp') as root:
                for label, reserve, keep in settings:
                    harness = evaluation.load_harnesses()['benitoiteh-mock-compact']
                    harness['config']['compaction'].update(reserve_tokens=reserve, keep_tokens=keep)
                    with self.subTest(task=task.name, setting=label), mock.patch.object(
                            mock_provider, 'start', side_effect=available):
                        row = evaluation.run_trial(task, os.path.join(root, label),
                            os.environ['BENITOITEH_TEST_BENITOITE'], 'mock-' + label, harness,
                            'mock', None, False, os.environ['BENITOITEH_SKILL_EVAL_BINARY'])
                        self.assertTrue(row['success'], row['reason'])
                        self.assertTrue(row['comparison_valid'], row['comparison_reasons'])
                        self.assertEqual(row['exec_launches'], 3)
                        self.assertTrue(row['compaction_records_match'])
                        self.assertEqual(row['total_tokens'], sum(stage['total_tokens'] for stage in row['stages']))
                        self.assertEqual(row['cost_usd'], 0)
                        self.assertLessEqual(row['turns'], 50)
                        self.assertEqual([stage['limits']['turns'] for stage in row['stages']],
                                         [50 - sum(s['turns'] for s in row['stages'][:n]) for n in range(3)])
                        self.assertEqual([stage['limits']['tokens'] for stage in row['stages']],
                            [2000000 - sum(s['total_tokens'] for s in row['stages'][:n]) for n in range(3)])
                        session = os.path.join(root, label, 'bh', 'state', 'benitoiteh', 'sessions',
                                               row['session_id'], 'session.jsonl')
                        with open(session) as f:
                            records = [json.loads(line) for line in f if line.strip()]
                        reads = [r['result']['text'] for r in records
                                 if r.get('type') == 'tool_result' and r.get('name') == 'read'
                                 and 'field notes, part' in r['result']['text']]
                        self.assertEqual(len(reads), sum(len(evaluation.stage_materials(task, n)) for n in range(1, 4)))
                        self.assertTrue(all('sample-' in text and len(text.encode()) > 4000 for text in reads))
                        retained = [e for e in row['compaction_events'] if e['stage'] > 1 and e['kept_items'] > 0]
                        self.assertTrue(retained)
                        self.assertTrue(all(e['kept_estimate'] <= min(keep, 16384) for e in retained))
                        # 呼び出しと結果の組を複数残す。Anthropic の道具の途中では残さない。
                        self.assertGreater(max(e['kept_items'] for e in retained), 4)
                        rows[label] = row
                self.assertGreater(max(e['kept_estimate'] for e in rows['base']['compaction_events']),
                                   max(e['kept_estimate'] for e in rows['keep']['compaction_events']))
                # 低い固定使用量は途中の圧縮を起こさず、段の境のバイト数が予備の差を示す。
                self.assertLess(rows['base']['compaction_events'][0]['stage'],
                                rows['reserve']['compaction_events'][0]['stage'])


class JudgmentReportTests(unittest.TestCase):
    # 関門: 実際の checkReport の診断を判定の公開の結果へ残す。
    # 従来の失敗種別だけのテストでは、コードと理由の欠落は捕まらない。
    def test_final_check_retains_warning_codes_and_syntax_reason(self):
        task = evaluation.Task('json_summary')
        for source, success, diagnostic in (
            ('import Benitoite.Unofficial.IO.Console\nimport Benitoite.Unofficial.IO.File\n'
             'function main() -> Unit uses IO.All\n  Console.writeLine("x")\nend function\n', True, 'W0501'),
            ('this is invalid\n', False, 'E0201'),
        ):
            with self.subTest(diagnostic=diagnostic), tempfile.TemporaryDirectory() as directory:
                with open(os.path.join(directory, task.script), 'w') as f:
                    f.write(source)
                verdict = evaluation.judge_check(task, os.environ['BENITOITEH_TEST_BENITOITE'], directory,
                                                 evaluation.judge_environment(directory),
                                                 set('Clock.Time Console.Read Console.Write File.Read File.Write '
                                                     'Process.Environment Process.Exit Process.Run Random.Generate State'.split()))
                self.assertEqual(verdict.success, success, verdict.reason)
                self.assertIn(diagnostic, verdict.final_codes)
                if not success:
                    self.assertIn('expected a declaration', verdict.reason)

    def test_effect_report_kind_must_match_command(self):
        report = dict(schemaVersion=1, kind='checkReport', status='ok', complete=True,
                      aggregateEffects=dict(basis='declaredUpperBound', expanded=['File.Read']))
        for kind in ('checkReport', 'testCheckReport', 'other', None):
            for expected in ('checkReport', 'testCheckReport'):
                with self.subTest(kind=kind, expected=expected):
                    report['kind'] = kind
                    self.assertEqual(evaluation.declared_effects(0, json.dumps(report), expected),
                                     {'File.Read'} if kind == expected else None)


class ProcessorTests(unittest.TestCase):
    @requires_sandbox
    def test_reference_effects_and_attempt_limit(self):
        processor = os.path.abspath(os.environ['BENITOITEH_TEST_BENITOITE'])
        expected = {
            'csv_report': 'Console.Write,File.Read,File.Write',
            'file_process': 'Console.Write,File.Read,File.Write',
            'json_convert': 'Console.Write,File.Read,File.Write',
            'dir_walk': 'Console.Write,File.Read', 'error_handling': 'Console.Write,File.Read',
            'json_summary': 'Console.Write,File.Read', 'regex_text': 'Console.Write,File.Read',
            'http_client': 'Console.Write,Http.Connect,Process.Environment',
            'process_run': 'Console.Write,File.Read,Process.Run', 'process_shell': 'Console.Write,Process.Run',
            'write_tests': 'Assert.Check,File.Read',
        }
        for task in evaluation.load_tasks([]):
            with self.subTest(task=task.name):
                self.assertEqual(evaluation.task_allow(task, processor), expected[task.name])
        task = evaluation.Task('json_summary')
        with tempfile.TemporaryDirectory(prefix='skill-eval-limit-test-') as directory:
            path = os.path.join(directory, task.script)
            shutil.copyfile(task.path('reference.bnt'), path)
            accepted = evaluation.judge_trial_script(task, path, processor, None, task.attempt_limit)
            self.assertTrue(accepted.success, accepted.reason)
            refused = evaluation.judge_trial_script(task, path, processor, None, task.attempt_limit + 1)
            self.assertFalse(refused.success)
            self.assertEqual(refused.reason, '%d attempts exceed the limit %d' %
                             (task.attempt_limit + 1, task.attempt_limit))


if __name__ == '__main__':
    unittest.main()
