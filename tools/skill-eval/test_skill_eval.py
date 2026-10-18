#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""評価の道具の入出力の契約を確かめる（実装プラン P5-02）。"""
import copy
import contextlib
import http.client
import io
import json
import os
import shutil
import sys
import tempfile
import signal
import subprocess
import time
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mock_provider
import skill_eval as evaluation


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
    def test_config_and_rejected_values(self):
        harness = evaluation.load_harnesses()['benitoiteh-mock']
        text = evaluation.benitoiteh_config(harness, 'override-model', '/trial/real/benitoite', 1234)
        self.assertIn('[benitoite]\npath = "/trial/real/benitoite"', text)
        self.assertIn('allowed_programs = ["/usr/bin/sort"]', text)
        self.assertIn('[providers.eval]\n', text)
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
    def start_server(self, kind):
        try:
            server, base = mock_provider.start(kind, 'main.bnt', 'source text')
        except OSError as error:
            self.skipTest('loopback listening is unavailable: %s' % error)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return server

    def request(self, server, has_result=True):
        connection = http.client.HTTPConnection('127.0.0.1', server.server_address[1], timeout=5)
        try:
            content = [{'type': 'tool_result', 'tool_use_id': 'call', 'content': 'ok'}] if has_result else 'hello'
            body = json.dumps({'stream': True, 'messages': [{'role': 'user', 'content': content}]})
            connection.request('POST', '/v1/messages', body=body, headers={'Content-Type': 'application/json'})
            response = connection.getresponse()
            data = response.read()
            self.assertEqual(int(response.getheader('Content-Length')), len(data))
            return response.status, data.decode('utf-8')
        finally:
            connection.close()

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
                evaluation.benitoiteh_config(harness, harness['model'], '/processor')

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
    def test_unreadable_session_fails_only_one_trial_and_retains_cost(self):
        cases = [({'type': 'tool_result', 'name': 'benitoite_run',
                   'result': {'id': 'id', 'text': 'ok'}, 'spill': {'path': 'spill/missing'}}, 'FileNotFoundError'),
                 ({'type': 'tool_result', 'name': 'benitoite_run', 'result': {'id': 'id', 'text': None}}, 'TypeError'),
                 ({'type': 'spilled', 'parts': 'bad'}, 'TypeError'),
                 ({'type': 'tool_call'}, 'KeyError'),
                 ({'type': 'tool_result', 'name': 'benitoite_run', 'result': {'id': 'id', 'text': 'ok'},
                   'spill': {'path': '../outside'}}, 'ValueError')]
        for record, error in cases:
            with self.subTest(error=error), tempfile.TemporaryDirectory(prefix='skill-eval-bad-session-') as directory:
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
                with mock.patch.dict(os.environ, CLAUDECODE='test-parent', ANTHROPIC_API_KEY='test-key',
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




@unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'BENITOITEH_TEST_BENITOITE is not set')
class ProcessorTests(unittest.TestCase):
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
