#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""判定の方針と、実際の OS が拒む境界（P5-32）。"""
import json
import os
import re
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import skill_eval as evaluation

requires_sandbox = unittest.skipIf(
    os.environ.get('BENITOITEH_SKILL_EVAL_JUDGE_SANDBOX') == '0' or sys.platform != 'darwin',
    'judgment needs the macOS sandbox outside Codex')


TEST_WRITE_SOURCE = '''import Benitoite.Unofficial.IO.File
@test("write succeeds")
function writeSucceeds() -> Unit uses File.Write, Assert.Check
  match File.writeText(path: "inside.txt", text: "proof") with
    case Result.Ok(_) -> ()
    case Result.Error(_) -> Assert.equal(actual: false, expected: true)
  end match
end function
'''


def policy_forms(source):
    # 方針の構文だけを読む。許可・拒否の効力は下の本物の OS のテストで確かめる。
    tokens = iter(re.findall(r'"(?:\\.|[^"\\])*"|[()]|[^\s()]+', source))
    def form(token):
        if token != '(':
            return json.loads(token) if token.startswith('"') else token
        result = []
        for token in tokens:
            if token == ')':
                return result
            result.append(form(token))
        raise ValueError('unclosed form')
    return [form(token) for token in tokens]


class PolicyTests(unittest.TestCase):
    # 関門: 許可の境界が広がる組み立ての退行と、パスからの方針の注入を捕まえる。
    # ユーザーが求めた Codex 内の検査であり、OS の拒否の代用にはしない。
    def test_writes_programs_and_network_are_scoped_and_paths_are_quoted(self):
        with tempfile.TemporaryDirectory(prefix='judge-') as root:
            directory = os.path.join(root, 'space "quote" \\ path')
            os.mkdir(directory)
            alias = os.path.join(root, 'alias')
            os.symlink(directory, alias)
            for name, helpers, remote in (
                ('json_convert', [], None), ('write_tests', [], None),
                ('process_run', ['/usr/bin/sort'], None),
                ('process_shell', ['/bin/sh', '/bin/bash', '/usr/bin/grep', '/usr/bin/cut', '/usr/bin/sort'], None),
                ('http_client', [], 'localhost:43123'),
            ):
                with self.subTest(task=name):
                    forms = policy_forms(evaluation.judge_policy(evaluation.Task(name), '/opt/benitoite', alias,
                                         {'Process.Run', 'Http.Connect'}, 'http://127.0.0.1:43123'))
                    self.assertIn(['deny', 'default'], forms)
                    self.assertNotIn(['allow', 'default'], forms)
                    writes = [rule for rule in forms if rule[:2] == ['allow', 'file-write*']]
                    self.assertEqual(writes, [['allow', 'file-write*', ['subpath', os.path.realpath(directory)]]])
                    self.assertIn(['allow', 'file-write-data', ['literal', '/dev/null'],
                                   ['vnode-type', 'CHARACTER-DEVICE']], forms)
                    programs = [rule[2] for rule in forms if rule[:2] == ['allow', 'process-exec']]
                    self.assertEqual(programs, [['literal', '/opt/benitoite']]
                                     + [['literal', os.path.realpath(path)] for path in helpers])
                    network = [rule for rule in forms if len(rule) > 1 and rule[1].startswith('network')]
                    self.assertEqual(network, [] if remote is None else
                                     [['allow', 'network-outbound', ['remote', 'ip', remote]]])
            for endpoint in ('http://localhost:1', 'http://127.0.0.1:0', 'http://127.0.0.1:65536',
                             'http://127.0.0.1:1/path', 'http://127.0.0.1:1") (allow default)'):
                with self.subTest(endpoint=endpoint), self.assertRaises(ValueError):
                    evaluation.judge_policy(evaluation.Task('http_client'), '/opt/benitoite', alias,
                                             {'Http.Connect'}, endpoint)
            with self.assertRaises(ValueError):
                evaluation.judge_policy(evaluation.Task('json_convert'), '/opt/benitoite', root+'\n', set(), None)

    def test_repo_detection_covers_main_checkout_and_home_relative_spelling(self):
        with tempfile.TemporaryDirectory() as home:
            main = os.path.join(home, 'src/MainCheckout')
            tree = os.path.join(home, 'src/Worktree')
            gitdir = os.path.join(main, '.git/worktrees/Worktree')
            os.makedirs(gitdir)
            os.makedirs(tree)
            with open(os.path.join(tree, '.git'), 'w') as f:
                f.write('gitdir: ' + gitdir + '\n')
            with open(os.path.join(gitdir, 'commondir'), 'w') as f:
                f.write('../..\n')
            with mock.patch.object(evaluation, 'REPO', tree), mock.patch.dict(os.environ, HOME=home):
                for value in (main+'/docs', '~/src/MainCheckout/docs', '${HOME}/src/MainCheckout/docs',
                              'home + "/src/MainCheckout/docs"', tree+'/tools'):
                    with self.subTest(value=value):
                        self.assertTrue(evaluation.repository_path_seen(dict(text=value)))
                self.assertFalse(evaluation.repository_path_seen('workspace/report.json'))

    def test_non_macos_fails_without_starting_the_script(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(evaluation.sys, 'platform', 'linux'):
            env = evaluation.judge_environment(directory)
            for name in ('json_convert', 'write_tests'):
                task = evaluation.Task(name)
                call = evaluation.judge_test if task.result_kind == 'test' else evaluation.judge_run
                verdict = call(task, '/does-not-exist', directory, env, set())
                self.assertFalse(verdict.success)
                self.assertEqual(verdict.failure_kind, 'sandbox')
                self.assertIn('requires the macOS sandbox', verdict.reason)


@unittest.skipUnless(os.environ.get('BENITOITEH_TEST_BENITOITE'), 'processor not set')
class RuntimeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='js-', dir='/tmp')
        self.addCleanup(temporary.cleanup)
        self.root = temporary.name
        self.processor = os.environ['BENITOITEH_TEST_BENITOITE']

    def specimens(self):
        marker = os.path.join(self.root, 'outside.txt')
        write = ('  bind _ <- try File.writeText(path: %s, text: "outside") '
                 '|> Result.mapError(r: _, f: lambda(error) return IOError.message(error) end lambda) or return\n') % json.dumps(marker)
        process = ('  bind _ <- try Process.run(Process.command(program: "/usr/bin/true", arguments: [])) '
                   '|> Result.mapError(r: _, f: lambda(error) return IOError.message(error) end lambda) or return\n')
        network = ('  bind _ <- try Http.get("http://192.0.2.1:9/") '
                   '|> Result.mapError(r: _, f: lambda(error) return NetworkError.message(error) end lambda) or return\n')
        for name, insertion in (('json_convert', write), ('process_run', process), ('http_client', network)):
            task = evaluation.Task(name)
            with open(task.path('reference.bnt')) as f:
                reference = f.read()
            start = reference.index('\n', reference.index('function main()')) + 1
            yield task, reference, reference[:start] + insertion + reference[start:], marker

    def writer_task(self):
        tasks = os.path.join(self.root, 'tasks')
        os.mkdir(tasks)
        task_dir = os.path.join(tasks, 'writer')
        os.mkdir(task_dir)
        for name, value in (('task.json', json.dumps(dict(kind='test', script='test.bnt', min_tests=1))),
                            ('task.md', 'Write one test.\n'), ('reference.bnt', TEST_WRITE_SOURCE)):
            with open(os.path.join(task_dir, name), 'w') as f:
                f.write(value)
        with mock.patch.object(evaluation, 'TASKS_DIR', tasks):
            return evaluation.Task('writer')

    def test_negative_scripts_pass_check_within_task_allow(self):
        # 関門: OS の否定のテストが構文・エフェクトの拒否で通ることを防ぐ。
        marker = os.path.join(self.root, 'outside-test.txt')
        specimens = list(self.specimens()) + [(self.writer_task(), TEST_WRITE_SOURCE,
                                               TEST_WRITE_SOURCE.replace('inside.txt', marker), marker)]
        for task, _reference, malicious, _marker in specimens:
            with self.subTest(task=task.name), tempfile.TemporaryDirectory(dir=self.root) as directory:
                with open(os.path.join(directory, task.script), 'w') as f:
                    f.write(malicious)
                allowed = set(evaluation.task_allow(task, self.processor).split(','))
                verdict = evaluation.judge_check(task, self.processor, directory,
                                                 evaluation.judge_environment(directory), allowed)
                self.assertTrue(verdict.success, verdict.reason)

    @requires_sandbox
    def test_os_refuses_outside_writes_unlisted_exec_and_external_network(self):
        try:
            server, endpoint = evaluation.http_server.start()
        except OSError as error:
            self.skipTest('loopback listening is unavailable: %s' % error)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        for task, reference, malicious, marker in self.specimens():
            with self.subTest(task=task.name):
                # 正しい解が同じ方針で通ることと対にし、方針の構文の失敗を除く。
                positive = evaluation.judge(task, reference, self.processor, endpoint)
                self.assertTrue(positive.success, positive.reason)
                negative = evaluation.judge(task, malicious, self.processor, endpoint)
                self.assertFalse(negative.success, negative.reason)
                self.assertEqual(negative.failure_kind, 'run', negative.reason)
                self.assertNotIn('timed out', negative.reason)
                self.assertFalse(os.path.exists(marker))

    @requires_sandbox
    def test_test_command_is_also_sandboxed_and_shell_helpers_are_allowed(self):
        task = evaluation.Task('process_shell')
        with open(task.path('reference.bnt')) as f:
            verdict = evaluation.judge(task, f.read(), self.processor, None)
        self.assertTrue(verdict.success, verdict.reason)
        task = self.writer_task()
        marker = os.path.join(self.root, 'outside-test.txt')
        positive = evaluation.judge(task, TEST_WRITE_SOURCE, self.processor, None)
        self.assertTrue(positive.success, positive.reason)
        negative = evaluation.judge(task, TEST_WRITE_SOURCE.replace('inside.txt', marker), self.processor, None)
        self.assertFalse(negative.success, negative.reason)
        self.assertIn('tests failed', negative.reason)
        self.assertFalse(os.path.exists(marker))
