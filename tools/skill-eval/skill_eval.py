#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""同梱の Agent Skill の評価の道具（設計書 06-06「Skill の評価」）。

課題ごとに作業ディレクトリを作り、Skill を置き、ハーネス（コーディングエージェント）を起動し、書かれた
スクリプトを実行して期待する出力と比べ、検査と修正の回数（包みのコマンドの記録の check・run・test の行の数）
を数える。使い方は README.md を見る。Python の標準ライブラリだけを使い、python3 3.9 で動く書き方にする。

    python3 tools/skill-eval/skill_eval.py --self-test            # 解のスクリプトが期待する出力を出すか（LLM を呼ばない）
    python3 tools/skill-eval/skill_eval.py --dry-run              # 準備と判定だけを行う（LLM を呼ばない）
    python3 tools/skill-eval/skill_eval.py --harness codex --trials 3   # 評価（LLM を呼ぶ。設計者の了承が要る）
"""
import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
from typing import Dict, List, Optional, Tuple

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
TASKS_DIR = os.path.join(HERE, 'tasks')
WRAPPER_DIR = os.path.join(HERE, 'bin')
HARNESSES_FILE = os.path.join(HERE, 'harnesses.json')
CODES_FILE = os.path.join(REPO, 'crates', 'benitoite', 'src', 'diag', 'codes.rs')
# 標準エラー出力の診断のコード（02-10 の文章の形と JSON の形）。包みのコマンドの grep と同じ形にする
CODE_PATTERNS = (re.compile(r'(?:error|warning)\[([A-Z][0-9]{4})\]'), re.compile(r'"code":"([A-Z][0-9]{4})"'))
RESULTS_DIR = os.path.join(HERE, 'results')
# 一回の起動の中のすべての試行（全体の集計に使う）
ALL_ROWS = []  # type: List[Dict[str, object]]

sys.path.insert(0, HERE)
import http_server  # noqa: E402

# 検査と修正の回数に数えるサブコマンド（06-06「Skill の評価」）
COUNTED_COMMANDS = ('check', 'run', 'test')
# 一回の処理系の起動の時間の上限（秒）。止まらないスクリプトで評価が止まらないようにする
RUN_TIMEOUT = 60
# 一回のハーネスの起動の時間の上限（秒）
HARNESS_TIMEOUT = 30 * 60

# ハーネスに渡す依頼。課題の中身は task.md に書き、ここには進め方だけを書く
PROMPT = (
    'Your task is described in `task.md` in the current directory. Write the Benitoite script it asks for in the '
    'current directory.\n'
    'Learn the language only from the `benitoite` Agent Skill installed in this directory. Do not read other '
    'documentation, do not look outside the current directory, and do not use the Internet.\n'
    'Check the script with `benitoite check <file>` (for a test file, run `benitoite test <file>` instead), fix the '
    'reported errors, and run it with `benitoite run <file>` until it does what the task asks. '
    'You may run `benitoite check`, `run` and `test` at most {limit} times in total.'
)


class Task(object):
    def __init__(self, name: str) -> None:
        self.name = name
        self.dir = os.path.join(TASKS_DIR, name)
        with open(os.path.join(self.dir, 'task.json'), encoding='utf-8') as f:
            config = json.load(f)
        self.kind = config.get('kind', 'run')  # type: str
        self.script = config.get('script', 'main.bnt')  # type: str
        self.attempt_limit = int(config.get('attempt_limit', 10))
        self.http = bool(config.get('http', False))
        self.exit_code = int(config.get('exit_code', 0))
        self.min_tests = int(config.get('min_tests', 0))
        self.require_text = list(config.get('require_text', []))  # type: List[str]
        self.acceptance = dict(config.get('acceptance', {}))  # type: Dict[str, str]

    def path(self, *parts: str) -> str:
        return os.path.join(self.dir, *parts)


def load_tasks(names: List[str]) -> List[Task]:
    available = sorted(n for n in os.listdir(TASKS_DIR) if os.path.isfile(os.path.join(TASKS_DIR, n, 'task.json')))
    chosen = names or available
    unknown = [n for n in chosen if n not in available]
    if unknown:
        raise SystemExit('unknown task: %s (available: %s)' % (', '.join(unknown), ', '.join(available)))
    return [Task(n) for n in chosen]


def find_benitoite(given: Optional[str]) -> str:
    """本物の benitoite のパス。指定がなければ、リポジトリの target/release、target/debug、PATH の順に探す。"""
    if given:
        return os.path.abspath(given)
    # target/release と target/debug の両方があれば、新しく作った方を使う
    candidates = [os.path.join(REPO, 'target', profile, 'benitoite') for profile in ('release', 'debug')]
    built = [c for c in candidates if os.access(c, os.X_OK)]
    if built:
        return max(built, key=os.path.getmtime)
    found = shutil.which('benitoite')
    if found and os.path.dirname(os.path.abspath(found)) != WRAPPER_DIR:
        return found
    raise SystemExit('benitoite not found; build it with `cargo build` or pass --benitoite')


def copy_tree(source: str, target: str) -> None:
    if not os.path.isdir(source):
        return
    for root, _dirs, files in os.walk(source):
        relative = os.path.relpath(root, source)
        os.makedirs(os.path.join(target, relative), exist_ok=True)
        for name in files:
            shutil.copyfile(os.path.join(root, name), os.path.join(target, relative, name))


def prepare_inputs(task: Task, directory: str) -> None:
    """課題の入力を作業ディレクトリに写す。解のスクリプト（reference.bnt）と期待する出力は写さない。"""
    os.makedirs(directory, exist_ok=True)
    copy_tree(task.path('input'), directory)


def run_command(command: List[str], cwd: str, env: Dict[str, str], timeout: int = RUN_TIMEOUT) -> Tuple[int, str, str]:
    """コマンドを新しいプロセスの群れで起動する。時間切れなら群れをまとめて止める（孫のプロセスを残さない）。

    起動できないとき（コマンドがないなど）は、終了状態 -2 と理由を返し、例外にしない。
    """
    try:
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   start_new_session=True)
    except OSError as error:
        return -2, '', 'cannot start %s: %s' % (command[0] if command else '', error)
    try:
        out, err = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except OSError:
            pass
        process.communicate()
        return -1, '', 'timed out after %d seconds' % timeout
    return process.returncode, out.decode('utf-8', 'replace'), err.decode('utf-8', 'replace')


def same_file(expected: str, actual: str) -> bool:
    """期待するファイルと比べる。.json は値として比べ（空白と改行の違いを許す）、ほかはバイト列で比べる。"""
    if not os.path.isfile(actual):
        return False
    with open(expected, 'rb') as f:
        want = f.read()
    with open(actual, 'rb') as f:
        got = f.read()
    if expected.endswith('.json'):
        try:
            return json.loads(want.decode('utf-8')) == json.loads(got.decode('utf-8'))
        except ValueError:
            return False
    return want == got


class Verdict(object):
    def __init__(self, success: bool, reason: str, tests_passed: Optional[int] = None) -> None:
        self.success = success
        self.reason = reason
        self.tests_passed = tests_passed
        # 判定のときに最後のスクリプトの check（テストの課題では test）が出した診断のコード
        self.final_codes = []  # type: List[str]


def codes_in(text: str) -> List[str]:
    found = []  # type: List[str]
    for pattern in CODE_PATTERNS:
        found.extend(pattern.findall(text))
    return found


def warning_codes() -> List[str]:
    """diag::codes の表の警告のコードの全件（集計の表で、出なかった警告も 0 として載せるため）。"""
    try:
        with open(CODES_FILE, encoding='utf-8') as f:
            text = f.read()
    except OSError:
        return []
    return sorted(set(re.findall(r'^\s+(W[0-9]{4})\s', text, re.MULTILINE)))


def judge(task: Task, script_text: str, benitoite: str, http_base: Optional[str]) -> Verdict:
    """書かれたスクリプトを、入力を写し直した新しいディレクトリで検査・実行し、期待と比べる。

    エージェントが作業ディレクトリの入力を書き換えていても判定が変わらないよう、作業ディレクトリでは実行しない。
    判定のための起動は包みのコマンドを通さないので、検査と修正の回数に数えない。
    """
    env = dict(os.environ)
    if http_base is not None:
        env[http_server.BASE_VARIABLE] = http_base
    for text in task.require_text:
        if text not in script_text:
            return Verdict(False, 'the script does not contain `%s`' % text)
    directory = tempfile.mkdtemp(prefix='skill-eval-judge-')
    try:
        prepare_inputs(task, directory)
        with open(os.path.join(directory, task.script), 'w', encoding='utf-8') as f:
            f.write(script_text)
        if task.kind == 'test':
            return judge_test(task, benitoite, directory, env)
        code, _out, err = run_command([benitoite, 'check', task.script], directory, env)
        final_codes = codes_in(err)
        if code != 0:
            return with_codes(Verdict(False, 'check failed (exit %d): %s' % (code, first_line(err))), final_codes)
        verdict = judge_run(task, benitoite, directory, env)
        return with_codes(verdict, final_codes)
    finally:
        shutil.rmtree(directory, ignore_errors=True)


def with_codes(verdict: Verdict, codes: List[str]) -> Verdict:
    verdict.final_codes = codes
    return verdict


def judge_run(task: Task, benitoite: str, directory: str, env: Dict[str, str]) -> Verdict:
    code, out, err = run_command([benitoite, 'run', task.script], directory, env)
    if code != task.exit_code:
        return Verdict(False, 'run exited with %d, expected %d: %s' % (code, task.exit_code, first_line(err)))
    with open(task.path('expected.stdout'), encoding='utf-8') as f:
        expected = f.read()
    if out != expected:
        return Verdict(False, 'standard output differs: %s' % first_difference(expected, out))
    expected_files = task.path('expected.files')
    if os.path.isdir(expected_files):
        for root, _dirs, files in os.walk(expected_files):
            for name in sorted(files):
                want = os.path.join(root, name)
                relative = os.path.relpath(want, expected_files)
                if not same_file(want, os.path.join(directory, relative)):
                    return Verdict(False, 'file %s differs or is missing' % relative)
    return Verdict(True, 'ok')


def judge_test(task: Task, benitoite: str, directory: str, env: Dict[str, str]) -> Verdict:
    """テストを書く課題の判定。`benitoite test --diagnostics=json` の集計の行（06-04「結果の報告」）を読む。"""
    code, out, err = run_command([benitoite, 'test', '--diagnostics=json', task.script], directory, env)
    final_codes = codes_in(err)
    return with_codes(judge_test_output(task, code, out, err), final_codes)


def judge_test_output(task: Task, code: int, out: str, err: str) -> Verdict:
    summary = None
    for line in out.splitlines():
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if isinstance(value, dict) and value.get('kind') == 'testSummary':
            summary = value
    if summary is None:
        return Verdict(False, 'no test summary (exit %d): %s' % (code, first_line(err or out)))
    passed = int(summary.get('passed', 0))
    failed = int(summary.get('failed', 0))
    if code != 0 or failed != 0 or summary.get('filesNotRun', 0) != 0:
        return Verdict(False, 'tests failed: %d passed, %d failed (exit %d)' % (passed, failed, code), passed)
    if passed < task.min_tests:
        return Verdict(False, '%d tests passed, expected at least %d' % (passed, task.min_tests), passed)
    return Verdict(True, 'ok', passed)


def first_line(text: str) -> str:
    for line in text.splitlines():
        if line.strip():
            return line.strip()
    return ''


def first_difference(expected: str, actual: str) -> str:
    want = expected.splitlines()
    got = actual.splitlines()
    for index in range(max(len(want), len(got))):
        a = want[index] if index < len(want) else '<end>'
        b = got[index] if index < len(got) else '<end>'
        if a != b:
            return 'line %d: expected %r, got %r' % (index + 1, a, b)
    return 'trailing newline'


def read_log(path: str) -> List[List[str]]:
    """包みのコマンドの記録を読む。一行は [開始, 終了, 終了状態, 診断のコード, 引数...] である。"""
    if not os.path.isfile(path):
        return []
    with open(path, encoding='utf-8', errors='replace') as f:
        return [line.rstrip('\n').split('\t') for line in f if line.strip()]


def counted_command(arguments: List[str]) -> Optional[str]:
    """一回の起動が検査と修正の回数に数える起動なら、そのサブコマンドを返す。

    CLI はサブコマンドの前の選択肢（`--diagnostics=json check x.bnt`）と、サブコマンドのない実行
    （`benitoite x.bnt`）を受け付けるので、`-` で始まらない最初の引数を見る。
    """
    for argument in arguments:
        if argument.startswith('-'):
            continue
        if argument in COUNTED_COMMANDS:
            return argument
        if argument.endswith('.bnt'):
            return 'run'
        return None
    return None


def count_attempts(entries: List[List[str]]) -> int:
    return sum(1 for entry in entries if counted_command(entry[4:]) is not None)


def entry_codes(entry: List[str]) -> List[str]:
    return [c for c in entry[3].split(',') if c] if len(entry) > 3 else []


def diagnostic_stats(entries: List[List[str]]) -> Dict[str, object]:
    """試行の中の診断の統計。

    counts: コードごとの、出た回数（一回の起動で同じコードが二度出れば二と数える）。
    resolved・remained: あるコードが出た起動の、次の数える起動（check・run・test）で、そのコードが消えたか残ったか。
    first_check: 最初の check（なければ最初の数える起動）で出たコード。
    """
    counted = [e for e in entries if counted_command(e[4:]) is not None]
    counts = {}  # type: Dict[str, int]
    resolved = {}  # type: Dict[str, int]
    remained = {}  # type: Dict[str, int]
    for index, entry in enumerate(counted):
        codes = entry_codes(entry)
        for code in codes:
            counts[code] = counts.get(code, 0) + 1
        if index + 1 < len(counted):
            following = set(entry_codes(counted[index + 1]))
            for code in sorted(set(codes)):
                target = remained if code in following else resolved
                target[code] = target.get(code, 0) + 1
    checks = [e for e in counted if counted_command(e[4:]) == 'check'] or counted
    first = entry_codes(checks[0]) if checks else []
    return {'counts': counts, 'resolved': resolved, 'remained': remained, 'first_check': first}


def skill_digest(directory: str) -> str:
    digest = hashlib.sha256()
    for root, dirs, files in os.walk(directory):
        dirs.sort()
        for name in sorted(files):
            path = os.path.join(root, name)
            digest.update(os.path.relpath(path, directory).encode('utf-8'))
            with open(path, 'rb') as f:
                digest.update(f.read())
    return digest.hexdigest()[:12]


def skill_version(directory: str) -> str:
    version = 'unknown'
    try:
        with open(os.path.join(directory, 'SKILL.md'), encoding='utf-8') as f:
            for line in f:
                if 'benitoite-version:' in line:
                    version = line.split(':', 1)[1].strip().strip('"')
                    break
    except OSError:
        pass
    return '%s (sha256 %s)' % (version, skill_digest(directory))


def install_skill(benitoite: str, harness: Dict[str, object], workspace: str, skill_dir: Optional[str]) -> str:
    """Skill を作業ディレクトリに置き、置いたディレクトリを返す。--skill-dir があればそれを写す（docs/todo の TODO-017）。"""
    target = os.path.join(workspace, str(harness['skill_parent']), 'benitoite')
    if skill_dir:
        shutil.copytree(skill_dir, target)
        return target
    code, _out, err = run_command([benitoite, 'skill', 'install', '--project', '--agent', str(harness['agent'])],
                                  workspace, dict(os.environ))
    if code != 0 or not os.path.isfile(os.path.join(target, 'SKILL.md')):
        raise SystemExit('skill install failed in %s: %s' % (workspace, first_line(err)))
    return target


def load_harnesses() -> Dict[str, Dict[str, object]]:
    with open(HARNESSES_FILE, encoding='utf-8') as f:
        data = json.load(f)
    return {k: v for k, v in data.items() if not k.startswith('_')}


def render(template: List[str], values: Dict[str, str]) -> List[str]:
    return [part.format(**values) for part in template]


def version_of(command: List[str]) -> str:
    try:
        completed = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        return 'unknown'
    return first_line(completed.stdout.decode('utf-8', 'replace')) or 'unknown'


def run_trial(task: Task, trial_dir: str, benitoite: str, harness_name: str, harness: Dict[str, object],
              model: str, skill_dir: Optional[str], dry_run: bool) -> Dict[str, object]:
    """一回の試行。作業ディレクトリ（workspace）と、エージェントに見せない記録（benitoite.log など）を分けて置く。"""
    workspace = os.path.join(trial_dir, 'workspace')
    log = os.path.join(trial_dir, 'benitoite.log')
    # 包みのコマンドと本物の処理系は、試行のディレクトリ（リポジトリの外）に写して使う。リポジトリの中のパスを
    # PATH や環境変数に出すと、エージェントがそこから解のスクリプトや設計書に辿り着けるため（06-06）
    tools_dir = os.path.join(trial_dir, 'bin')
    real_dir = os.path.join(trial_dir, 'real')
    os.makedirs(tools_dir)
    os.makedirs(real_dir)
    wrapper_copy = os.path.join(tools_dir, 'benitoite')
    shutil.copy2(os.path.join(WRAPPER_DIR, 'benitoite'), wrapper_copy)
    real_copy = os.path.join(real_dir, 'benitoite')
    shutil.copy2(benitoite, real_copy)
    benitoite = real_copy
    prepare_inputs(task, workspace)
    shutil.copyfile(task.path('task.md'), os.path.join(workspace, 'task.md'))
    skill_path = install_skill(benitoite, harness, workspace, skill_dir)
    for name, text in dict(harness.get('files', {})).items():  # type: ignore
        with open(os.path.join(workspace, name), 'w', encoding='utf-8') as f:
            f.write(str(text))
    env = dict(os.environ)
    env['PATH'] = tools_dir + os.pathsep + env.get('PATH', '')
    env['SKILL_EVAL_LOG'] = log
    env['SKILL_EVAL_REAL_BENITOITE'] = benitoite
    server = None
    http_base = None
    if task.http:
        server, http_base = http_server.start()
        env[http_server.BASE_VARIABLE] = http_base
    warning = ''
    try:
        if dry_run:
            # ハーネスの代わりに解のスクリプトを置き、エージェントと同じく包みのコマンドで一度検査する
            shutil.copyfile(task.path('reference.bnt'), os.path.join(workspace, task.script))
            command = ['benitoite', 'test' if task.kind == 'test' else 'check', task.script]
            run_command(command, workspace, env)
            harness_exit = 0
            err = ''
        else:
            values = dict(harness.get('options', {}))  # type: ignore
            values.update(workspace=workspace, model=model, prompt=PROMPT.format(limit=task.attempt_limit))
            command = render(list(harness['start']), values)  # type: ignore
            cwd = str(harness.get('cwd', '{workspace}')).format(**values)
            harness_exit, out, err = run_command(command, cwd, env, HARNESS_TIMEOUT)
            # 生の対話の記録はリポジトリの外（試行のディレクトリ）に置く
            with open(os.path.join(trial_dir, 'harness.stdout'), 'w', encoding='utf-8') as f:
                f.write(out)
            with open(os.path.join(trial_dir, 'harness.stderr'), 'w', encoding='utf-8') as f:
                f.write(err)
        entries = read_log(log)
        attempts = count_attempts(entries)
        script_path = os.path.join(workspace, task.script)
        if harness_exit == -2:
            verdict = Verdict(False, 'the harness could not be started: %s' % first_line(err))
        elif not os.path.isfile(log) or attempts == 0:
            # 包みのコマンドを通らずに処理系を使った（迂回した、サンドボックスが記録を書けない）試行を、成功と記録しない
            warning = 'WARNING: no counted benitoite command in the wrapper log'
            verdict = Verdict(False, warning)
        elif not os.path.isfile(script_path):
            verdict = Verdict(False, '%s was not written' % task.script)
        elif attempts > task.attempt_limit:
            verdict = Verdict(False, '%d attempts exceed the limit %d' % (attempts, task.attempt_limit))
        else:
            with open(script_path, encoding='utf-8', errors='replace') as f:
                verdict = judge(task, f.read(), benitoite, http_base)
    finally:
        if server is not None:
            server.shutdown()
            server.server_close()
    return {
        'task': task.name, 'success': verdict.success, 'reason': verdict.reason, 'attempts': attempts,
        'log_exists': os.path.isfile(log), 'skill': os.path.relpath(skill_path, workspace),
        'tests_passed': verdict.tests_passed, 'harness_exit': harness_exit, 'trial_dir': trial_dir,
        'diagnostics': diagnostic_stats(entries), 'final_codes': verdict.final_codes, 'harness': harness_name,
        'model': model,
    }


def self_test(tasks: List[Task], benitoite: str) -> int:
    """解のスクリプトが期待する出力を出すか、受け入れテストと同じ入力か、包みのコマンドが働くかを確かめる。"""
    failures = 0
    for task in tasks:
        problems = []
        for relative, source in sorted(task.acceptance.items()):
            if not same_file(os.path.join(REPO, source), task.path(relative)):
                problems.append('%s differs from %s' % (relative, source))
        server = None
        http_base = None
        if task.http:
            server, http_base = http_server.start()
        try:
            with open(task.path('reference.bnt'), encoding='utf-8') as f:
                verdict = judge(task, f.read(), benitoite, http_base)
        finally:
            if server is not None:
                server.shutdown()
                server.server_close()
        if not verdict.success:
            problems.append(verdict.reason)
        failures += 1 if problems else 0
        print('%-16s %s' % (task.name, 'ok' if not problems else 'FAILED: ' + '; '.join(problems)))
    wrapper_problem = check_wrapper(benitoite)
    print('%-16s %s' % ('(wrapper)', wrapper_problem or 'ok'))
    failures += 1 if wrapper_problem else 0
    print('self-test: %d of %d failed' % (failures, len(tasks) + 1))
    return 1 if failures else 0


def check_wrapper(benitoite: str) -> Optional[str]:
    """包みのコマンドが本物の終了状態を返し、起動ごとに一行を記録するか。"""
    directory = tempfile.mkdtemp(prefix='skill-eval-wrapper-')
    try:
        log = os.path.join(directory, 'benitoite.log')
        env = dict(os.environ, SKILL_EVAL_LOG=log, SKILL_EVAL_REAL_BENITOITE=benitoite)
        wrapper = os.path.join(WRAPPER_DIR, 'benitoite')
        with open(os.path.join(directory, 'ok.bnt'), 'w', encoding='utf-8') as f:
            f.write('import Benitoite.Unofficial.IO.Console\n\nfunction main() -> Unit uses Console.Write\n'
                    '  return Console.writeLine("ok")\nend function\n')
        runs = [(['check', 'ok.bnt'], 0), (['run', 'ok.bnt'], 0), (['check', 'missing file.bnt'], None),
                (['--version'], 0), (['--diagnostics=json', 'check', 'ok.bnt'], 0), (['ok.bnt'], 0)]
        for arguments, want in runs:
            code, _out, _err = run_command([wrapper] + arguments, directory, env)
            real, _out, _err = run_command([benitoite] + arguments, directory, env)
            if code != real or (want is not None and code != want):
                return 'exit code of %s: wrapper %d, real %d' % (' '.join(arguments), code, real)
        entries = read_log(log)
        if [entry[4:] for entry in entries] != [arguments for arguments, _want in runs]:
            return 'unexpected log: %r' % entries
        if [entry[2] for entry in entries][2] == '0' or count_attempts(entries) != 5:
            return 'the log does not record the exit codes or the attempts: %r' % entries
        return check_diagnostic_log(wrapper, benitoite, directory)
    finally:
        shutil.rmtree(directory, ignore_errors=True)


# 統計の自己検査に使うスクリプト: 誤り（E0405）→ 警告（W0401）→ 診断なし、と直していく
DIAG_STEPS = [
    ('error', 'function main() -> Unit\n  bind x <- 1 / 0\n  return ()\nend function\n', ['E0405']),
    ('warning', 'import Benitoite.Unofficial.IO.Console\n\nfunction main() -> Unit uses Console.Write\n'
     '  return Console.writeLine(Integer.toString(1 div 0))\nend function\n', ['W0401']),
    ('clean', 'import Benitoite.Unofficial.IO.Console\n\nfunction main() -> Unit uses Console.Write\n'
     '  return Console.writeLine(Integer.toString(1 div 1))\nend function\n', []),
]


def check_diagnostic_log(wrapper: str, benitoite: str, directory: str) -> Optional[str]:
    """包みのコマンドが診断のコードを記録し、次の起動で消えたことを統計で数えられるか。"""
    log = os.path.join(directory, 'diag.log')
    env = dict(os.environ, SKILL_EVAL_LOG=log, SKILL_EVAL_REAL_BENITOITE=benitoite)
    for _name, text, _codes in DIAG_STEPS:
        with open(os.path.join(directory, 'main.bnt'), 'w', encoding='utf-8') as f:
            f.write(text)
        _code, _out, err_wrapped = run_command([wrapper, 'check', 'main.bnt'], directory, env)
        _code, _out, err_real = run_command([benitoite, 'check', 'main.bnt'], directory, dict(os.environ))
        if err_wrapped != err_real:
            return 'the wrapper changed the standard error of check'
    entries = read_log(log)
    got = [entry_codes(e) for e in entries]
    if got != [codes for _n, _t, codes in DIAG_STEPS]:
        return 'unexpected diagnostic codes in the log: %r' % got
    stats = diagnostic_stats(entries)
    if stats['resolved'] != {'E0405': 1, 'W0401': 1} or stats['remained'] or stats['first_check'] != ['E0405']:
        return 'unexpected diagnostic statistics: %r' % stats
    return None


def code_table(rows: List[Dict[str, object]]) -> List[str]:
    """診断のコードごとの集計の表。警告は diag::codes の全件を、出なかったものも 0 として載せる。"""
    total = {}  # type: Dict[str, List[int]]
    for row in rows:
        stats = row['diagnostics']  # type: ignore
        final = set(row.get('final_codes') or [])  # type: ignore
        for code in set(stats['counts']) | final:  # type: ignore
            item = total.setdefault(code, [0, 0, 0, 0, 0])
            item[0] += stats['counts'].get(code, 0)  # type: ignore
            item[1] += 1 if code in stats['counts'] else 0  # type: ignore
            item[2] += stats['resolved'].get(code, 0)  # type: ignore
            item[3] += stats['remained'].get(code, 0)  # type: ignore
            item[4] += 1 if code in final else 0
    for code in warning_codes():
        total.setdefault(code, [0, 0, 0, 0, 0])
    lines = ['| コード | 出た回数 | 出た試行の数 | 解消 | 残った | 解消率 | 最後に残った試行の数 |',
             '|---|---|---|---|---|---|---|']
    for code in sorted(total, key=lambda c: (c[0] != 'W', c)):
        n, trials, fixed, kept, last = total[code]
        rate = '%.0f%%' % (100.0 * fixed / (fixed + kept)) if fixed + kept else '-'
        lines.append('| %s | %d | %d | %d | %d | %s | %d |' % (code, n, trials, fixed, kept, rate, last))
    return lines


def diagnostics_section(rows: List[Dict[str, object]]) -> List[str]:
    lines = ['', '## 診断のコードの集計', '',
             '「解消」と「残った」は、そのコードが出た起動の次の check・run・test で、コードが消えたか残ったかの回数である。'
             '「最後に残った試行の数」は、判定のときに最後のスクリプトの check（テストの課題では test）でそのコードが出た試行の数である。'
             '警告（W）は diag::codes の全件を載せ、出なかったものも 0 とする。', '']
    pairs = sorted(set((str(r['harness']), str(r['model'])) for r in rows))
    for harness, model in pairs:
        mine = [r for r in rows if (str(r['harness']), str(r['model'])) == (harness, model)]
        lines += ['### %s（%s）' % (harness, model), ''] + code_table(mine) + ['']
    if len(pairs) > 1:
        lines += ['### 全体', ''] + code_table(rows) + ['']
    lines += ['### 試行ごとの最初の check のコードと最後の警告', '', '| ハーネス | 課題 | 試行 | 最初の check | 最後の check の警告 |',
              '|---|---|---|---|---|']
    for row in rows:
        stats = row['diagnostics']  # type: ignore
        warnings = [c for c in (row.get('final_codes') or []) if c.startswith('W')]  # type: ignore
        lines.append('| %s | %s | %s | %s | %s |' % (row['harness'], row['task'], os.path.basename(str(row['trial_dir'])),
                                                    ', '.join(stats['first_check']) or '-',  # type: ignore
                                                    ', '.join(warnings) or '-'))
    return lines


def write_record(path: str, header: Dict[str, str], rows: List[Dict[str, object]], tasks: List[Task]) -> None:
    """記録（Markdown）を書く。生の対話の記録は含めず、試行のディレクトリのパスだけを残す。"""
    lines = ['# Skill の評価の記録', '']
    for key, value in header.items():
        lines.append('- %s: %s' % (key, value))
    lines += ['', '| 課題 | 成功 / 試行 | 成功した試行の検査と修正の回数 |', '|---|---|---|']
    for task in tasks:
        mine = [r for r in rows if r['task'] == task.name]
        wins = [r for r in mine if r['success']]
        counts = ', '.join(str(r['attempts']) for r in wins) or '-'
        lines.append('| %s | %d / %d | %s |' % (task.name, len(wins), len(mine), counts))
    lines += ['', '## 試行ごとの結果', '', '| 課題 | 成功 | 回数 | 理由 |', '|---|---|---|---|']
    for row in rows:
        lines.append('| %s | %s | %s | %s |' % (row['task'], 'yes' if row['success'] else 'no', row['attempts'],
                                               str(row['reason']).replace('|', '\\|')))
    lines += diagnostics_section(rows)
    with open(path, 'w', encoding='utf-8') as f:
        f.write('\n'.join(lines) + '\n')


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description='Evaluate the bundled Benitoite Agent Skill.')
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--self-test', action='store_true', help='run the reference scripts (no LLM)')
    mode.add_argument('--dry-run', action='store_true', help='prepare and judge without a harness (no LLM)')
    mode.add_argument('--harness', help='harness names in harnesses.json, comma-separated, or `default` '
                      'for the default set (calls an LLM)')
    parser.add_argument('--task', action='append', default=[], help='task name (repeatable; default: all)')
    parser.add_argument('--trials', type=int, default=1, help='trials per task')
    parser.add_argument('--model', help='model passed to the harness (default: harnesses.json)')
    parser.add_argument('--agent', default='codex', help='harness whose Skill location --dry-run uses')
    parser.add_argument('--skill-dir', help='Skill directory to use instead of `benitoite skill install`')
    parser.add_argument('--benitoite', help='path of the real benitoite (default: target/release, target/debug, PATH)')
    parser.add_argument('--work-root', help='directory for workspaces and raw logs (default: a new temporary one)')
    parser.add_argument('--allow-unverified', action='store_true',
                        help='start a harness whose settings are not verified (harnesses.json "verified": false)')
    parser.add_argument('--record', help='record file (default: results/<date>-<harness>.md)')
    args = parser.parse_args(argv)

    tasks = load_tasks(args.task)
    benitoite = find_benitoite(args.benitoite)
    if args.self_test:
        return self_test(tasks, benitoite)

    harnesses = load_harnesses()
    if args.harness == 'default':
        names = sorted(n for n, h in harnesses.items() if h.get('default', False))
    elif args.harness:
        names = [n.strip() for n in args.harness.split(',') if n.strip()]
    else:
        names = [args.agent]
    for name in names:
        if name not in harnesses:
            raise SystemExit('unknown harness: %s (available: %s)' % (name, ', '.join(sorted(harnesses))))
        if args.harness and not harnesses[name].get('verified', False) and not args.allow_unverified:
            raise SystemExit('the settings of harness %s are not verified; check them with the designer (skill '
                             '`skill-eval`) and pass --allow-unverified' % name)
    if len(names) > 1 and (args.record or args.model):
        raise SystemExit('--record and --model need a single harness')
    skill_dir = os.path.abspath(args.skill_dir) if args.skill_dir else None
    work_root = os.path.realpath(args.work_root) if args.work_root else tempfile.mkdtemp(prefix='skill-eval-')
    if os.path.commonpath([work_root, os.path.realpath(REPO)]) == os.path.realpath(REPO):
        raise SystemExit('--work-root must be outside the repository (raw logs are not kept in it)')
    status = 0
    for name in names:
        root = os.path.join(work_root, name) if len(names) > 1 else work_root
        status = max(status, evaluate(args, tasks, benitoite, name, harnesses[name], skill_dir, root))
    if len(names) > 1:
        # ハーネスとモデルの組をまたぐ全体の集計
        directory = work_root if args.dry_run else RESULTS_DIR
        record = os.path.join(directory, '%s-overall.md' % datetime.datetime.now().strftime('%Y-%m-%d-%H%M'))
        write_record(record, {'ハーネス': ', '.join(names), '生の記録': work_root}, ALL_ROWS, tasks)
        print('overall record: %s' % record)
    return status


def evaluate(args: argparse.Namespace, tasks: List[Task], benitoite: str, harness_name: str,
             harness: Dict[str, object], skill_dir: Optional[str], work_root: str) -> int:
    """一つのハーネスで全課題を試し、--dry-run でなければ記録を書く。"""
    model = args.model or str(harness['model'])
    rows = []
    for task in tasks:
        for trial in range(1, args.trials + 1):
            # 同じ --work-root で再び実行しても前の試行を消さないよう、空いた番号のディレクトリを使う
            number = trial
            while os.path.exists(os.path.join(work_root, task.name, str(number))):
                number += args.trials
            trial_dir = os.path.join(work_root, task.name, str(number))
            os.makedirs(trial_dir)
            row = run_trial(task, trial_dir, benitoite, harness_name, harness, model, skill_dir, args.dry_run)
            rows.append(row)
            print('%s %-16s trial %d: %s, attempts %d, %s' % (
                harness_name, task.name, trial, 'success' if row['success'] else 'failure', row['attempts'],
                row['reason']), flush=True)
    wins = sum(1 for r in rows if r['success'])
    print('%s: %d of %d trials succeeded; workspaces in %s' % (harness_name, wins, len(rows), work_root))
    if args.dry_run:
        # 試しの実行の記録は results/ でなく作業ディレクトリに書く（集計の節が出ることを確かめるため）
        record = os.path.join(work_root, 'record.md')
        write_record(record, {'種類': 'dry-run', 'ハーネス': harness_name}, rows, tasks)
        bad = [r for r in rows if not (r['success'] and r['log_exists'] and r['attempts'] >= 1)]
        with open(record, encoding='utf-8') as f:
            if '## 診断のコードの集計' not in f.read():
                bad.append({'reason': 'no diagnostics section'})
        print('record: %s' % record)
        ALL_ROWS.extend(rows)
        return 1 if bad else 0

    ALL_ROWS.extend(rows)
    first_skill = os.path.join(str(rows[0]['trial_dir']), 'workspace', str(harness['skill_parent']), 'benitoite')
    header = {
        '日時': datetime.datetime.now().strftime('%Y-%m-%d %H:%M'),
        '処理系': version_of([benitoite, '--version']),
        'Skill': ('%s（--skill-dir %s）' % (skill_version(first_skill), args.skill_dir)) if skill_dir
        else skill_version(first_skill),
        'ハーネス': '%s（%s）' % (harness_name, version_of(list(harness['version']))),  # type: ignore
        'モデル': '%s %s' % (model, json.dumps(harness.get('options', {}), ensure_ascii=False)),
        '試行の回数': str(args.trials),
        '生の記録': work_root + '（リポジトリの外）',
    }
    record = args.record or os.path.join(
        RESULTS_DIR, '%s-%s.md' % (datetime.datetime.now().strftime('%Y-%m-%d-%H%M'), harness_name))
    write_record(record, header, rows, tasks)
    print('record: %s' % record)
    return 0


if __name__ == '__main__':
    sys.exit(main())
