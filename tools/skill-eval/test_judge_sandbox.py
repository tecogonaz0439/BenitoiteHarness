#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""判定の方針と、実際の OS が拒む境界（P5-32）。"""
import json
import os
import re
import signal
import socket
import socketserver
import subprocess
import threading
import time
import shutil
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

    def test_home_data_denial_precedes_only_the_required_read_exceptions(self):
        # 関門: 親 HOME の秘密と判定の専用 HOME を混同する退行を、方針の境界で捕まえる。
        with tempfile.TemporaryDirectory(prefix='jh-', dir='/tmp') as home:
            alias = os.path.join(home, 'alias')
            os.symlink(home, alias)
            directory = os.path.join(home, 'judge')
            os.mkdir(directory)
            processor = os.path.join(home, 'bin', 'benitoite')
            with mock.patch.dict(os.environ, HOME=alias):
                forms = policy_forms(evaluation.judge_policy(evaluation.Task('process_run'), processor,
                                                            directory, {'Process.Run'}, None))
            self.assertEqual(forms[-4:], [
                ['deny', 'file-read-data', ['subpath', os.path.realpath(home)]],
                ['allow', 'file-read-data', ['literal', os.path.realpath(processor)]],
                ['allow', 'file-read-data', ['literal', os.path.realpath('/usr/bin/sort')]],
                ['allow', 'file-read-data', ['subpath', os.path.realpath(directory)]],
            ])

    def test_test_output_excerpt_is_short_and_escapes_controls(self):
        # 関門: 失敗した判定の公開の理由から、長大な出力と端末制御の混入を防ぐ。
        output = 'bad\t\x1b[31m' + 'x' * 500 + 'TAIL'
        verdict = evaluation.judge_test_output(evaluation.Task('write_tests'), 1, '', output)
        self.assertFalse(verdict.success)
        self.assertNotIn('TAIL', verdict.reason)
        self.assertNotIn('\t', verdict.reason)
        self.assertNotIn('\x1b', verdict.reason)
        self.assertIn('\\t', verdict.reason)
        self.assertLess(len(verdict.reason), 170)

    def test_command_cleanup_stops_background_children_after_normal_exit(self):
        # 関門: 通常終了した子が出力を閉じた孫を残す退行を、本物の群れの副作用で捕まえる。
        with tempfile.TemporaryDirectory(prefix='jp-', dir='/tmp') as root:
            marker = os.path.join(root, 'late-write')
            child = 'import time; time.sleep(.3); open(%r, "w").write("leftover")' % marker
            parent = ('import subprocess; p=subprocess.Popen(%r, stdin=subprocess.DEVNULL, '
                      'stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); print(p.pid)') % [sys.executable, '-c', child]
            code, out, err = evaluation.run_command([sys.executable, '-c', parent], root,
                                                    evaluation.judge_environment(root))
            self.assertEqual(code, 0, err)
            pid = int(out.strip())
            try:
                time.sleep(.6)
                self.assertFalse(os.path.exists(marker), 'background child outlived the command')
            finally:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

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

    def specimens(self, blocked_endpoint="http://127.0.0.1:9/"):
        marker = os.path.join(self.root, 'outside.txt')
        write = ('  bind _ <- try File.writeText(path: %s, text: "outside") '
                 '|> Result.mapError(r: _, f: lambda(error) return IOError.message(error) end lambda) or return\n') % json.dumps(marker)
        process = ('  bind _ <- try Process.run(Process.command(program: "/bin/sh", arguments: ["-c", "exit 0"])) '
                   '|> Result.mapError(r: _, f: lambda(error) return IOError.message(error) end lambda) or return\n')
        network = ('  bind _ <- try Http.get(%s) '
                   '|> Result.mapError(r: _, f: lambda(error) return NetworkError.message(error) end lambda) or return\n') % json.dumps(blocked_endpoint)
        for name, insertion in (('json_convert', write), ('compact_json_summary', write), ('process_run', process), ('http_client', network)):
            task = evaluation.Task(name)
            with open(task.path('reference.bnt')) as f:
                reference = f.read()
            start = reference.index('\n', reference.index('function main()')) + 1
            yield task, reference, reference[:start] + insertion + reference[start:], marker
        task = evaluation.Task('process_shell')
        with open(task.path('reference.bnt')) as f:
            reference = f.read()
        for command in ('/usr/bin/id', '/usr/bin/touch ' + marker, 'printf proof > ' + marker):
            yield task, reference, self.shell_source(command), marker

    def shell_source(self, command):
        return ('import Benitoite.Unofficial.IO.Console\nimport Benitoite.Unofficial.IO.Process\n'
                'function main() -> Result[Unit, String] uses Process.Run, Console.Write\n'
                '  bind output <- try Process.shell(%s) |> Result.mapError(r: _, '
                'f: lambda(error) return IOError.message(error) end lambda) or return\n'
                '  Console.writeLine(Process.Output.standardError(output))\n'
                '  Console.writeLine("exit: ${Process.Output.exitCode(output)}")\n'
                '  return Result.Ok(())\nend function\n') % json.dumps(command)

    def blocked_listener(self, family):
        # 経路のない外部の IP には接続せず、実際に応答する別のループバックを使う。
        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                self.server.hits += 1
                self.request.sendall(b'HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nok')
        class Server(socketserver.ThreadingTCPServer):
            address_family = family
            daemon_threads = True
        try:
            server = Server(('127.0.0.1' if family == socket.AF_INET else '::1', 0), Handler)
        except OSError as error:
            self.skipTest('loopback listening is unavailable: %s' % error)
        server.hits = 0
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        address = '127.0.0.1' if family == socket.AF_INET else '::1'
        with socket.create_connection((address, server.server_address[1]), timeout=5) as connection:
            self.assertIn(b'200 OK', connection.recv(1024))
        self.assertEqual(server.hits, 1)
        server.hits = 0
        host = address if family == socket.AF_INET else '[::1]'
        return server, 'http://%s:%d/' % (host, server.server_address[1])

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
    def test_os_refuses_outside_writes_unlisted_exec_and_other_loopback(self):
        try:
            server, endpoint = evaluation.http_server.start()
        except OSError as error:
            self.skipTest('loopback listening is unavailable: %s' % error)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        blocked, blocked_endpoint = self.blocked_listener(socket.AF_INET)
        for task, reference, malicious, marker in self.specimens(blocked_endpoint):
            with self.subTest(task=task.name):
                # 正しい解が同じ方針で通ることと対にし、方針の構文の失敗を除く。
                positive = evaluation.judge(task, reference, self.processor, endpoint)
                self.assertTrue(positive.success, positive.reason)
                negative = evaluation.judge(task, malicious, self.processor, endpoint)
                self.assertFalse(negative.success, negative.reason)
                self.assertIn(negative.failure_kind, ('run', 'stdout'), negative.reason)
                self.assertIn('Operation not permitted', negative.reason)
                self.assertNotIn('timed out', negative.reason)
                self.assertFalse(os.path.exists(marker))
        self.assertEqual(blocked.hits, 0)

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

    @requires_sandbox
    def test_other_ipv6_loopback_is_refused_before_connecting(self):
        try:
            allowed, allowed_endpoint = evaluation.http_server.start()
        except OSError as error:
            self.skipTest('loopback listening is unavailable: %s' % error)
        self.addCleanup(allowed.server_close)
        self.addCleanup(allowed.shutdown)
        blocked, endpoint = self.blocked_listener(socket.AF_INET6)
        # localhost は同じ番号の ::1 も許すので、必ず別のポートを調べる。
        while blocked.server_address[1] == allowed.server_address[1]:
            blocked, endpoint = self.blocked_listener(socket.AF_INET6)
        specimen = next(item for item in self.specimens(endpoint) if item[0].name == 'http_client')
        positive = evaluation.judge(specimen[0], specimen[1], self.processor, allowed_endpoint)
        self.assertTrue(positive.success, positive.reason)
        verdict = evaluation.judge(specimen[0], specimen[2], self.processor, allowed_endpoint)
        self.assertFalse(verdict.success)
        self.assertEqual(verdict.failure_kind, 'run', verdict.reason)
        self.assertIn('Operation not permitted', verdict.reason)
        self.assertEqual(blocked.hits, 0)

    @requires_sandbox
    def test_home_secrets_are_denied_and_absent_from_stdout_and_error_reasons(self):
        # 関門: 本物の HOME を読まず、判定と処理系を偽 HOME の下に置く正の対照と対にする。
        home = os.path.join(self.root, 'home')
        os.makedirs(os.path.join(home, 'bin'))
        directory = os.path.join(home, 'judgments')
        os.mkdir(directory)
        processor = os.path.join(home, 'bin', 'benitoite')
        shutil.copy2(self.processor, processor)
        alias = os.path.join(self.root, 'home-alias')
        os.symlink(home, alias)
        secret = 'CANARY-PRIVATE-HOME-KEY-33'
        paths = ['.ssh/key', '.config/benitoiteh/config.toml',
                 '.local/state/benitoiteh/auth/token', '.local/state/benitoiteh/sessions/session.jsonl']
        task = evaluation.Task('process_run')
        with mock.patch.dict(os.environ, HOME=alias), mock.patch.object(tempfile, 'tempdir', directory):
            with open(task.path('reference.bnt')) as f:
                positive = evaluation.judge(task, f.read(), processor, None)
            self.assertTrue(positive.success, positive.reason)
            for relative in paths:
                path = os.path.join(home, relative)
                os.makedirs(os.path.dirname(path), exist_ok=True)
                with open(path, 'w') as f:
                    f.write(secret)
                # リンクの別名から読んでも実の HOME の拒否を抜けない。
                requested = os.path.join(alias, relative)
                for mode in ('stdout', 'error'):
                    source = ('import Benitoite.Unofficial.IO.Console\nimport Benitoite.Unofficial.IO.File\n'
                              'function main() -> Result[Unit, String] uses File.Read, Console.Write\n'
                              '  bind text <- try File.readText(%s) |> Result.mapError(r: _, '
                              'f: lambda(error) return IOError.message(error) end lambda) or return\n') % json.dumps(requested)
                    source += ('  Console.writeLine(text)\n  return Result.Ok(())\n' if mode == 'stdout'
                               else '  return Result.Error(text)\n') + 'end function\n'
                    with self.subTest(path=relative, mode=mode):
                        verdict = evaluation.judge(task, source, processor, None)
                        self.assertFalse(verdict.success)
                        self.assertEqual(verdict.failure_kind, 'run', verdict.reason)
                        self.assertIn('Operation not permitted', verdict.reason)
                        self.assertNotIn(secret, verdict.reason)

    @requires_sandbox
    def test_special_judgment_paths_work_with_the_real_policy_parser(self):
        task, reference, malicious, marker = next(self.specimens())
        for suffix in ('q"uote', 'back\\slash', 'paren) (allow default) (#|', 'unicode\u2028sep'):
            directory = os.path.join(self.root, suffix)
            os.mkdir(directory)
            with self.subTest(path=suffix), mock.patch.object(tempfile, 'tempdir', directory):
                positive = evaluation.judge(task, reference, self.processor, None)
                self.assertTrue(positive.success, positive.reason)
                negative = evaluation.judge(task, malicious, self.processor, None)
                self.assertFalse(negative.success)
                self.assertIn('Operation not permitted', negative.reason)
                self.assertFalse(os.path.exists(marker))

    @requires_sandbox
    def test_stdout_and_error_values_are_escaped_and_bounded_in_reasons(self):
        # Benitoite の文字列にある制御文字は、Skill のエスケープで渡す。
        literal = '"bad\\t\\u{1b}[31m' + 'x' * 500 + 'TAIL"'
        for mode in ('stdout', 'error'):
            source = ('import Benitoite.Unofficial.IO.Console\n'
                      'function main() -> Result[Unit, String] uses Console.Write\n')
            source += ('  Console.writeLine(%s)\n  return Result.Ok(())\n' % literal if mode == 'stdout'
                       else '  return Result.Error(%s)\n' % literal) + 'end function\n'
            with self.subTest(mode=mode):
                verdict = evaluation.judge(evaluation.Task('process_run'), source, self.processor, None)
                self.assertEqual(verdict.failure_kind, 'stdout' if mode == 'stdout' else 'run', verdict.reason)
                self.assertNotIn('TAIL', verdict.reason)
                self.assertNotIn('\t', verdict.reason)
                self.assertNotIn('\x1b', verdict.reason)
                self.assertIn('\\t', verdict.reason)
                self.assertLess(len(verdict.reason), 250)
