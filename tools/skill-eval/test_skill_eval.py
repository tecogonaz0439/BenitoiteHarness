#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""評価の道具の入出力の契約を確かめる（実装プラン P5-02）。"""
import copy
import http.client
import json
import os
import shutil
import sys
import tempfile
import unittest

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
