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
import functools
import hashlib
import json
import os
import re
import shutil
import shlex
import signal
import stat
import statistics
import subprocess
import sys
import tempfile
import time
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
# キーは一回の評価のメモリにだけ保ち、公開する記録の直前にも値を除く。
RECORD_SECRETS = []  # type: List[str]
JUDGE_RUNNER = None  # type: Optional[str]
JUDGE_WORKSPACE = None  # type: Optional[str]
JUDGE_ROOTS = []  # type: List[str]
NORMAL_TOOLS = {}  # type: Dict[str, Dict[str, str]]

sys.path.insert(0, HERE)
import http_server  # noqa: E402
import mock_provider  # noqa: E402
import agy_gate  # noqa: E402

# 検査と修正の回数に数えるサブコマンド（06-06「Skill の評価」）
COUNTED_COMMANDS = ('check', 'run', 'test')
# 一回の処理系の起動の時間の上限（秒）。止まらないスクリプトで評価が止まらないようにする
RUN_TIMEOUT = 60
# 一回のハーネスの起動の時間の上限（秒）
HARNESS_TIMEOUT = 30 * 60
TERM_GRACE = 30

# crates/benitoiteh/src/agent/text.rs の同名の定数から写した終了理由。
TURN_LIMIT = 'The run reached the maximum number of turns.'
TOKEN_LIMIT = 'The run reached the maximum number of tokens.'
EFFECTS_NOT_ALLOWED = ('The run stopped because benitoite_run, benitoite_test or benitoite_scratch was refused '
                       '3 times in a row: the effects were outside --allow.')
LEFTOVER_PROCESSES = 'The run stopped because processes started by the script may still be running.'
PROCESS_TOOLS = {'benitoite_check': 'check', 'benitoite_run': 'run',
                 'benitoite_test': 'test', 'benitoite_scratch': 'run'}
SHELL_OUTPUT = re.compile(r'(?m)^components with errors: [0-9]+$')
BH_FIELDS = ('kind', 'allow', 'session_id', 'benitoiteh_version', 'system_prompt_sha256', 'tools_sha256',
             'turns', 'tool_calls', 'after_write_checks', 'checks', 'fixes', 'policy_denied', 'input_tokens',
             'cache_read_tokens', 'cache_write_tokens', 'output_tokens', 'total_tokens', 'end', 'shell_output_seen',
             'completion_prompts', 'verification_calls_before', 'verification_calls_after', 'edits_after_prompt')
TOKEN_FIELDS = ('input_tokens', 'cache_read_tokens', 'cache_write_tokens', 'output_tokens', 'total_tokens')
# deepseek-harness 687f282e: packages/core/session/src/types.ts の TurnEndReasonMap と
# packages/bundle/headless/{src/json-stream.ts,README.md} の Exit mapping。
DSH_COMPLETED = 'completed'
# agy 1.4.0 の実行ファイルの文字列。P5-07 補足 8 の調査記録から写した。
AGY_PRINT_TIMEOUT = re.compile(
    r'(?m)^\[agy\] print timeout after [0-9.a-zµμ]+ with turn in progress; returning partial output\s*$')
AGY_ALLOWED_TOOLS = {'run_command'} | agy_gate.READ_TOOLS | agy_gate.WRITE_TOOLS
EMAIL = re.compile(r'[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}')

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
NORMAL_PROMPT = ('Your task is described in `task.md` in the current directory. Inspect the project, edit only '
                 'the files the task permits, and run the documented local checks. Use only the tools already '
                 'installed, stay within the current directory, and do not use the Internet. '
                 'Report what you changed and the results of your checks.')


def task_path(root: str, relative: str, directory: bool = False) -> str:
    """課題の宣言したパスだけを辿り、途中のリンクと特殊ファイルも拒む（P5-32）。"""
    if (not isinstance(relative, str) or not relative or os.path.isabs(relative)
            or any(part in ('', '.', '..') for part in relative.split('/'))):
        raise ValueError('invalid task path: %r' % relative)
    path = root
    parts = relative.split('/')
    for index, part in enumerate(parts):
        path = os.path.join(path, part)
        mode = os.lstat(path).st_mode
        want_dir = index < len(parts) - 1 or directory
        if not (stat.S_ISDIR(mode) if want_dir else stat.S_ISREG(mode)):
            raise ValueError('task path must contain only regular files and directories: %s' % relative)
    if directory:
        with os.scandir(path) as entries:
            for entry in entries:
                task_path(root, relative + '/' + entry.name, entry.is_dir(follow_symlinks=False))
    return path


class Task(object):
    def __init__(self, name: str) -> None:
        if not re.fullmatch(r'[A-Za-z0-9_-]+', name):
            raise ValueError('invalid task name')
        self.name = name
        self.dir = task_path(TASKS_DIR, name, True)
        with open(task_path(self.dir, 'task.json'), encoding='utf-8') as f:
            config = json.load(f)
        self.kind = config.get('kind', 'run')  # type: str
        self.result_kind = config.get('result_kind', 'run') if self.kind == 'staged' else self.kind
        self.suite = {'staged': 'compaction', 'normal': 'normal'}.get(self.kind, 'benitoite')
        self.script = config.get('script', 'main.bnt')  # type: str
        self.attempt_limit = int(config.get('attempt_limit', 10))
        self.http = bool(config.get('http', False))
        self.exit_code = int(config.get('exit_code', 0))
        self.min_tests = int(config.get('min_tests', 0))
        self.require_text = list(config.get('require_text', []))  # type: List[str]
        self.acceptance = dict(config.get('acceptance', {}))  # type: Dict[str, str]
        self.stages = config.get('stages', [])
        self.judge_cases = config.get('judge_cases', [])
        self.self_test_failures = config.get('self_test_failures', [])
        if self.kind == 'normal':
            self.validate_normal(config)
        if self.kind == 'staged':
            task_path(self.dir, 'task.md')
            for path in ('expected.stdout', 'expected.files'):
                if os.path.lexists(self.path(path)):
                    raise ValueError('staged expectations belong in judge_cases, not ' + path)
            if (self.result_kind != 'run' or config.get('http') is not False
                    or type(config.get('attempt_limit')) is not int or self.attempt_limit <= 0
                    or not isinstance(self.stages, list) or len(self.stages) != 3):
                raise ValueError('staged tasks require result_kind run, http false, a positive attempt_limit and 3 stages')
            if (not isinstance(self.script, str) or not self.script.endswith('.bnt')
                    or '/' in self.script or self.script in ('.', '..')):
                raise ValueError('staged script must be a .bnt filename')
            for path in self.stages:
                task_path(self.dir, path)
                if not path.endswith('.md'):
                    raise ValueError('stages must be Markdown files')
            for number in range(1, 4):
                task_path(self.dir, 'materials/%d' % number, directory=True)
            if os.path.lexists(self.path('input')):
                task_path(self.dir, 'input', directory=True)
                if os.path.lexists(self.path('input', 'materials')):
                    raise ValueError('staged materials must be outside input/')
            self.validate_cases()

    def validate_normal(self, config: dict) -> None:
        if 'attempt_limit' in config or 'script' in config or self.http:
            raise ValueError('normal tasks do not use script, attempt_limit or HTTP')
        self.category = config.get('category')
        if self.category not in ('bugfix', 'feature', 'tests'):
            raise ValueError('invalid normal category')
        task_path(self.dir, 'task.md')
        for directory in ('input', 'reference', 'judge'):
            task_path(self.dir, directory, True)
        for key in ('artifacts', 'writable_files', 'allowed_generated'):
            paths = config.get(key, [])
            if not isinstance(paths, list) or (key != 'allowed_generated' and not paths):
                raise ValueError('invalid normal paths: ' + key)
            normalized = []
            for path in paths:
                value = path[:-1] if isinstance(path, str) and path.endswith('/') else path
                if (not isinstance(value, str) or not value or os.path.isabs(value)
                        or any(part in ('', '.', '..') for part in value.split('/'))):
                    raise ValueError('invalid task path: %r' % path)
                if key != 'allowed_generated' and value != path:
                    raise ValueError('file path must not end in /')
                normalized.append(value)
            if len(set(normalized)) != len(normalized):
                raise ValueError('duplicate normal paths: ' + key)
            setattr(self, key, paths)
        if not set(self.artifacts) <= set(self.writable_files):
            raise ValueError('artifacts must be writable files')
        for path in self.writable_files:
            task_path(self.dir, 'input/' + path)
        for path in self.artifacts:
            task_path(self.dir, 'reference/' + path)
        for key in ('verify', 'judge'):
            command = config.get(key)
            if not isinstance(command, list) or not command or not all(isinstance(arg, str) and arg for arg in command):
                raise ValueError('invalid normal command: ' + key)
            setattr(self, key + '_command', command)
        if self.verify_command != ['python3', '-m', 'unittest', 'discover', '-s', 'tests']:
            raise ValueError('normal verify must discover the original public tests')
        if self.judge_command != ['python3', '-I', 'judge/verify.py']:
            raise ValueError('normal judge must use the isolated trusted verifier')
        task_path(self.dir, 'input/tests', True)
        task_path(self.dir, 'judge/verify.py')
        self.required_tools = config.get('required_tools', ['python3', '/bin/sh'])
        if (not isinstance(self.required_tools, list) or not self.required_tools
                or any(tool not in ('python3', '/bin/sh', 'find', 'sort', 'basename', 'sed', 'awk', 'cat', 'printf')
                       for tool in self.required_tools) or len(set(self.required_tools)) != len(self.required_tools)):
            raise ValueError('invalid required_tools')
        self.normal_self_test = config.get('self_test')
        if not isinstance(self.normal_self_test, dict):
            raise ValueError('normal self_test must describe the intended failures')
        case = self.normal_self_test.get('initial_case')
        if not isinstance(case, str) or not re.fullmatch(r'[A-Za-z0-9_-]+', case):
            raise ValueError('invalid normal self_test initial_case')
        source = self.normal_self_test.get('fixed_output')
        controls = self.normal_self_test.get('controls')
        if not isinstance(source, str) or not source.strip() or not isinstance(controls, list):
            raise ValueError('normal self_test requires fixed_output and controls')
        for control in controls:
            if not isinstance(control, dict):
                raise ValueError('invalid normal self_test control')
            task_path(self.dir, control.get('source'))
        groups = [self.normal_self_test.get('fixed_output_cases')] + [control.get('cases') for control in controls]
        for cases in groups:
            if (not isinstance(cases, list) or not cases
                    or any(not isinstance(case, str) or not re.fullmatch(r'[A-Za-z0-9_-]+', case) for case in cases)
                    or len(set(cases)) != len(cases)):
                raise ValueError('normal self_test requires nonempty distinct failure cases')
        if len(self.artifacts) != 1:
            raise ValueError('normal self_test requires one artifact')
        self.attempt_limit = None

    def validate_cases(self) -> None:
        if not isinstance(self.judge_cases, list) or not self.judge_cases:
            raise ValueError('staged tasks require judge_cases')
        names = set()
        for case in self.judge_cases:
            if (not isinstance(case, dict) or not isinstance(case.get('name'), str)
                    or not re.fullmatch(r'[A-Za-z0-9_-]+', case['name']) or case['name'] in names
                    or not isinstance(case.get('args'), list)
                    or not all(isinstance(arg, str) for arg in case['args'])):
                raise ValueError('invalid or duplicate judge case')
            names.add(case['name'])
            prefix = 'cases/%s/' % case['name']
            for key, directory in (('input', True), ('expected_stdout', False), ('expected_files', True)):
                path = case.get(key)
                if not isinstance(path, str) or not path.startswith(prefix):
                    raise ValueError('judge case paths must be under ' + prefix)
                task_path(self.dir, path, directory)
        if not isinstance(self.self_test_failures, list):
            raise ValueError('self_test_failures must be a list')
        for failure in self.self_test_failures:
            if (not isinstance(failure, dict) or not isinstance(failure.get('case'), str)
                    or failure['case'] not in names):
                raise ValueError('self-test failure must name a judge case')
            task_path(self.dir, failure.get('script'))

    def path(self, *parts: str) -> str:
        return os.path.join(self.dir, *parts)


def load_tasks(names: List[str], suite: str = 'benitoite') -> List[Task]:
    with os.scandir(TASKS_DIR) as entries:
        available = sorted(entry.name for entry in entries if entry.is_dir(follow_symlinks=False)
                           and os.path.lexists(os.path.join(entry.path, 'task.json')))
    selected = []
    for name in available:
        try:
            root = task_path(TASKS_DIR, name, True)
            with open(task_path(root, 'task.json'), encoding='utf-8') as f:
                kind = json.load(f).get('kind', 'run')
            task_suite = {'staged': 'compaction', 'normal': 'normal'}.get(kind, 'benitoite')
        except (OSError, ValueError, AttributeError, TypeError) as error:
            if name in names:
                raise ValueError('invalid task manifest: %s' % name) from error
            print('WARNING: cannot classify unselected task %s: %s' % (name, type(error).__name__), file=sys.stderr)
            continue
        if task_suite == suite:
            selected.append(name)
    chosen = names or sorted(selected)
    unknown = [n for n in chosen if n not in available]
    if unknown:
        raise SystemExit('unknown task: %s (available: %s)' % (', '.join(unknown), ', '.join(available)))
    mismatch = [n for n in chosen if n not in selected]
    if mismatch:
        raise SystemExit('tasks outside suite %s: %s' % (suite, ', '.join(mismatch)))
    if not chosen:
        raise SystemExit('no tasks in suite %s (tasks not created yet)' % suite)
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
    raise SystemExit('benitoite not found; pass --benitoite <path to benitoite v0.0.7>')


def find_benitoiteh(given: Optional[str]) -> str:
    if given:
        return os.path.abspath(given)
    candidates = [os.path.join(REPO, 'target', profile, 'benitoiteh') for profile in ('release', 'debug')]
    built = [path for path in candidates if os.access(path, os.X_OK)]
    if not built:
        raise SystemExit('benitoiteh not found; build it or pass --benitoiteh')
    return max(built, key=os.path.getmtime)


def toml_value(value: object) -> str:
    """P5-02 の限定した文字列だけを TOML に書き、引用の解釈の違いを避ける。"""
    if isinstance(value, str):
        if any(ord(c) < 32 or ord(c) > 126 or c in '"\\' for c in value):
            raise ValueError('config strings must be printable ASCII without quotes or backslashes')
        return json.dumps(value)
    if isinstance(value, bool):
        return 'true' if value else 'false'
    if isinstance(value, int):
        return str(value)
    if isinstance(value, list):
        return '[' + ', '.join(toml_value(item) for item in value) + ']'
    if isinstance(value, dict):
        parts = []
        for key, item in value.items():
            if not isinstance(key, str) or not re.fullmatch(r'[A-Za-z0-9_-]+', key):
                raise ValueError('invalid config key')
            parts.append('%s = %s' % (key, toml_value(item)))
        return '{ ' + ', '.join(parts) + ' }'
    raise ValueError('unsupported config value: %s' % type(value).__name__)


def benitoiteh_config(harness: Dict[str, object], model: str, processor: str,
                     mock_port: Optional[int] = None) -> str:
    provider = dict(harness['provider'])  # type: ignore
    key = provider.get('api_key')
    valid_env = (isinstance(key, dict) and set(key) == {'env'}
                 and isinstance(key['env'], str) and bool(key['env']))
    valid_command = (isinstance(key, dict) and set(key) == {'command'}
                     and isinstance(key['command'], list) and bool(key['command'])
                     and all(isinstance(part, str) and bool(part) for part in key['command']))
    if not (valid_env or valid_command):
        raise ValueError('api_key must reference an env name or a command; literal keys are not allowed')
    if mock_port is not None:
        provider['base_url'] = str(provider['base_url']).replace('{mock_port}', str(mock_port))
    provider.update(model=model, effort=harness['effort'])
    provider.setdefault('context_window', 200000)
    # 一度全体を検証し、途中まで書かれた設定を起動に使わない。
    toml_value(provider)
    normal = harness.get('mode', 'restricted') == 'normal'
    lines = ['default_mode = ' + toml_value('normal' if normal else 'restricted'),
             'default_provider = "eval"', '', '[providers.eval]']
    lines += ['%s = %s' % (key, toml_value(value)) for key, value in provider.items()]
    lines += ['', '[benitoite]', 'path = ' + toml_value(processor), '', '[restricted]',
              'allowed_programs = ["/usr/bin/sort"]']
    config = harness.get('config', {})
    lines += ['%s = %s' % (key, toml_value(value))
              for key, value in sorted(config.get('restricted', {}).items())]
    if normal:
        lines += ['', '[normal]', 'sandbox = "workspace-write"', 'network = false', 'writable_roots = []']
    for table in ('read', 'agent', 'compaction'):
        if table in config or (normal and table == 'agent'):
            lines += ['', '[%s]' % table]
            values = dict(config.get(table, {}))
            if normal and table == 'agent':
                values['completion_check'] = False
            lines += ['%s = %s' % (key, toml_value(value))
                      for key, value in sorted(values.items())]
    lines += ['', '[skills]', 'user = false', '', '[plugins]', 'user = false', '']
    return '\n'.join(lines)


def task_allow(task: Task, benitoite: str) -> str:
    """解の報告が確定しているときだけ、非対話の許可の集合を取り出す。"""
    with tempfile.TemporaryDirectory(prefix='skill-eval-effects-') as directory:
        directory = os.path.realpath(directory)
        shutil.copyfile(task.path('reference.bnt'), os.path.join(directory, task.script))
        command = [benitoite, 'test', '--check'] if task.result_kind == 'test' else [benitoite, 'check']
        code, out, err = run_command(command + ['--report=json', task.script], directory, judge_environment(directory))
    expanded = declared_effects(code, out, 'testCheckReport' if task.result_kind == 'test' else 'checkReport')
    if expanded is not None:
        return ','.join(sorted(expanded))
    raise ValueError('cannot determine --allow for %s (exit %d): %s' % (task.name, code, first_line(err)))


def declared_effects(code: int, out: str, kind: str = 'checkReport') -> Optional[set]:
    """不完全な報告を許可の空集合と取り違えない。"""
    try:
        report = json.loads(out)
    except ValueError:
        report = None
    if isinstance(report, dict):
        aggregate = report.get('aggregateEffects')
        if (code == 0 and type(report.get('schemaVersion')) is int and report['schemaVersion'] == 1
                and report.get('kind') == kind and report.get('status') == 'ok' and report.get('complete') is True
                and isinstance(aggregate, dict) and aggregate.get('basis') == 'declaredUpperBound'):
            expanded = aggregate.get('expanded')
            if isinstance(expanded, list) and all(isinstance(effect, str) and effect for effect in expanded):
                return set(expanded)
    return None


def json_object(line: str) -> Optional[Dict[str, object]]:
    try:
        value = json.loads(line)
    except (ValueError, RecursionError):
        return None
    return value if isinstance(value, dict) else None


def benitoiteh_output(out: str, harness_exit: int) -> Tuple[Dict[str, object], str, str]:
    """exec のイベントを読み、集計・終了理由・最後の番の応答を返す。"""
    fields = dict.fromkeys(BH_FIELDS)
    fields.update(kind='benitoiteh', attempts=0, shell_output_seen=False, end='error')
    start = None
    outcome = None
    error = {}
    last = None
    final = []
    final_turn = None
    for line in out.splitlines():
        if not line.strip():
            continue
        value = json_object(line)
        last = value
        if value is None:
            continue
        event = value.get('type')
        if event == 'session_start' and start is None:
            start = value
        elif event == 'session_end':
            outcome = value.get('outcome')
        elif event == 'error':
            error = value
        elif event in ('turn_start', 'text_delta'):
            if value.get('turn') != final_turn:
                final_turn = value.get('turn')
                final = []
            if event == 'text_delta':
                final.append(str(value.get('text', '')))
    if start is not None:
        for key in ('session_id', 'benitoiteh_version', 'system_prompt_sha256', 'tools_sha256'):
            fields[key] = start.get(key)
    if last is None or last.get('type') != 'summary':
        return fields, 'benitoiteh produced no summary (exit %d)' % harness_exit, ''.join(final)
    calls = last.get('tool_calls', {})
    tokens = last.get('tokens', {})
    if not all(isinstance(value, dict) for value in (calls, tokens, last.get('approvals', {}))):
        raise ValueError('invalid summary fields')
    if any(type(value) is not int or value < 0 for value in calls.values()):
        raise ValueError('invalid tool call counts')
    if any(value is not None and (type(value) is not int or value < 0) for value in tokens.values()):
        raise ValueError('invalid token counts')
    input_tokens, output_tokens = tokens.get('input', 0), tokens.get('output', 0)
    fields.update(turns=last.get('turns'), tool_calls=calls,
                  compactions=last.get('compactions', 0),
                  attempts=sum(calls.get(name, 0) for name in PROCESS_TOOLS),
                  after_write_checks=last.get('after_write_checks'), checks=last.get('checks'), fixes=last.get('fixes'),
                  policy_denied=last.get('approvals', {}).get('policy_denied', 0),
                  input_tokens=tokens.get('input', 0), cache_read_tokens=tokens.get('cache_read', 0),
                  cache_write_tokens=tokens.get('cache_write', 0), output_tokens=tokens.get('output', 0),
                  total_tokens=input_tokens + output_tokens if input_tokens is not None and output_tokens is not None
                  else None)
    reason = str(last.get('reason', ''))
    if outcome in ('completed', 'cancelled'):
        fields['end'] = outcome
    elif reason in (TURN_LIMIT, TOKEN_LIMIT, EFFECTS_NOT_ALLOWED, LEFTOVER_PROCESSES):
        fields['end'] = {TURN_LIMIT: 'max_turns', TOKEN_LIMIT: 'max_tokens',
                         EFFECTS_NOT_ALLOWED: 'effects_refused', LEFTOVER_PROCESSES: 'leftover_processes'}[reason]
    elif reason.startswith('The run stopped because checks of '):
        fields['end'] = 'check_fix_limit'
    else:
        detail = '%s: %s' % (error.get('kind', 'error'), str(error.get('message', ''))[:200])
        reason = '%s (%s)' % (reason, detail)
    return fields, reason, ''.join(final)


def token_counts(usage: Dict[str, object], names: Tuple[str, ...]) -> Dict[str, int]:
    """キャッシュを含まない外部の入力を、benitoiteh と同じ量に揃える。"""
    values = [usage.get(name, 0) for name in names]
    if any(type(value) is not int or value < 0 for value in values):
        raise ValueError('invalid token counts')
    uncached, read, write, output = values
    return dict(zip(TOKEN_FIELDS, (uncached + read + write, read, write, output,
                                   uncached + read + write + output)))


def dsh_output(out: str, harness_exit: int) -> Tuple[Dict[str, object], str, str]:
    """親セッションの step ごとの使用量を合計する（子エージェント分は含まない）。"""
    fields = dict(turns=0, dsh_turns=0, usage_missing=0, tool_calls={}, subagent_calls=0)
    tokens = dict.fromkeys(TOKEN_FIELDS, 0)
    reason = None
    error = None
    final = ''
    for line in out.splitlines():
        value = json_object(line)
        if value is None:
            continue
        event = value.get('type')
        if event == 'status':
            phase = value.get('phase')
            if phase == 'turn_start':
                fields['dsh_turns'] += 1
            elif phase == 'step_end':
                fields['turns'] += 1
                if 'usage' not in value:
                    fields['usage_missing'] += 1
                else:
                    usage = value['usage']
                    if not isinstance(usage, dict):
                        raise ValueError('invalid step usage')
                    counts = token_counts(usage, ('inputTokens', 'cacheReadTokens', 'cacheWriteTokens', 'outputTokens'))
                    for key, count in counts.items():
                        tokens[key] += count
            elif phase == 'turn_end':
                reason = value.get('reason')
                if not isinstance(reason, dict) or not isinstance(reason.get('kind'), str):
                    raise ValueError('invalid turn reason')
        elif event == 'tool_call':
            tool = value.get('tool')
            if not isinstance(tool, str):
                raise ValueError('invalid tool name')
            calls = fields['tool_calls']
            calls[tool] = calls.get(tool, 0) + 1
            fields['subagent_calls'] += int(tool.startswith(('subagent', 'workflow')))
        elif event == 'error':
            error = value.get('message')
            if not isinstance(error, str):
                raise ValueError('invalid error message')
        elif event == 'final':
            final = value.get('text')
            if not isinstance(final, str):
                raise ValueError('invalid final text')
    if reason is None and error is None:
        raise ValueError('no terminal event')
    fields.update(tokens if fields['turns'] else dict.fromkeys(TOKEN_FIELDS))
    kind = reason.get('kind') if reason is not None else 'error'
    fields['end'] = 'completed' if harness_exit == 0 and kind == DSH_COMPLETED else 'error'
    detail = str(kind)
    if reason is not None and kind == 'error':
        failure = reason.get('error')
        if not isinstance(failure, dict) or not all(isinstance(failure.get(key), str) for key in ('code', 'message')):
            raise ValueError('invalid turn error')
        detail += ': ' + ('%s: %s' % (failure.get('code', ''), failure.get('message', '')))[:200]
    elif reason is None:
        detail += ': ' + error[:200]
    if fields['subagent_calls']:
        detail += '; usage excludes subagents'
    return fields, detail, final


def claude_output(out: str, harness_exit: int) -> Tuple[Dict[str, object], str, str]:
    """購読の見かけの費用は、請求額として扱わず別の欄に保つ。"""
    lines = [line for line in out.splitlines() if line.strip()]
    result = json_object(lines[-1]) if lines else None
    if result is None or result.get('type') != 'result':
        raise ValueError('no result object')
    turns, subtype, is_error = result.get('num_turns'), result.get('subtype'), result.get('is_error')
    if type(turns) is not int or turns < 0 or not isinstance(subtype, str) or type(is_error) is not bool:
        raise ValueError('invalid result fields')
    fields = dict(turns=turns, tool_calls=None, claude_reported_cost_usd=result.get('total_cost_usd'))
    models = result.get('modelUsage')
    if models is not None:
        if not isinstance(models, dict):
            raise ValueError('invalid model usage')
        usages = list(models.values())
        names = ('inputTokens', 'cacheReadInputTokens', 'cacheCreationInputTokens', 'outputTokens')
    else:
        usages = [result.get('usage')]
        names = ('input_tokens', 'cache_read_input_tokens', 'cache_creation_input_tokens', 'output_tokens')
    if any(not isinstance(usage, dict) for usage in usages):
        raise ValueError('missing usage')
    counts = [token_counts(usage, names) for usage in usages]
    fields.update({key: sum(count[key] for count in counts) for key in TOKEN_FIELDS})
    fields['end'] = ('completed' if subtype == 'success' and not is_error else
                     'max_turns' if subtype == 'error_max_turns' else 'error')
    final = result.get('result', '')
    if not isinstance(final, str):
        raise ValueError('invalid result text')
    return fields, subtype, final


def agy_transcript(conversation_id: str, usage: Dict[str, object]) -> Tuple[Optional[int], Optional[Dict[str, int]], str]:
    """会話 ID と記録の実のパスを検査し、モデルへの要求を番として数える。"""
    try:
        if not re.fullmatch(r'[A-Za-z0-9_-]+', conversation_id):
            raise ValueError('invalid conversation_id')
        brain = os.path.realpath(os.path.expanduser('~/.gemini/antigravity-cli/brain'))
        logs = os.path.join(brain, conversation_id, '.system_generated', 'logs')
        path = os.path.join(logs, 'transcript_full.jsonl')
        if not os.path.lexists(path):
            path = os.path.join(logs, 'transcript.jsonl')
        path = os.path.realpath(path)
        if os.path.commonpath([brain, path]) != brain:
            raise ValueError('transcript is outside brain')
        turns, calls = 0, {}
        names = ('input_tokens', 'output_tokens', 'cache_read_tokens')
        totals = dict.fromkeys(names, 0)
        descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
        with os.fdopen(descriptor, encoding='utf-8') as f:
            if not stat.S_ISREG(os.fstat(f.fileno()).st_mode):
                raise ValueError('transcript is not a regular file')
            for line in f:
                if not line.strip():
                    continue
                step = json.loads(line)
                if not isinstance(step, dict):
                    raise ValueError('invalid transcript step')
                if step.get('type') != 'PLANNER_RESPONSE':
                    continue
                turns += 1
                for key in names:
                    count = step.get(key, 0)
                    if type(count) is not int or count < 0:
                        raise ValueError('invalid transcript usage')
                    totals[key] += count
                tools = step.get('tool_calls', [])
                if not isinstance(tools, list):
                    raise ValueError('invalid transcript tools')
                for tool in tools:
                    if not isinstance(tool, dict):
                        raise ValueError('invalid transcript tool')
                    raw_name = tool.get('name')
                    name = raw_name if isinstance(raw_name, str) and re.fullmatch(r'[a-z0-9_]{1,64}', raw_name) else '<invalid>'
                    calls[name] = calls.get(name, 0) + 1
        details = ['transcript usage differs'] if any(totals[key] != usage.get(key, 0) for key in names) else []
        details += ['WARNING: unexpected agy tool ' + name for name in sorted(set(calls) - AGY_ALLOWED_TOOLS)]
        return turns, calls, '; '.join(details)
    except (OSError, ValueError, TypeError, RecursionError):
        return None, None, 'transcript unreadable'


def agy_output(out: str, harness_exit: int, err: str) -> Tuple[Dict[str, object], str, str]:
    """使用量は標準出力を正とし、終了理由は標準エラー出力も含めて判定する。"""
    fields = dict.fromkeys(TOKEN_FIELDS + ('turns', 'tool_calls', 'end', 'thinking_tokens', 'denied_actions'))
    details, final = [], ''
    result = json_object(out)
    if result is None:
        lines = [line for line in out.splitlines() if line.strip()]
        result = json_object(lines[-1]) if lines else None
    try:
        if (result is None or not isinstance(result.get('conversation_id'), str)
                or not isinstance(result.get('status'), str) or not isinstance(result.get('response'), str)
                or type(result.get('duration_seconds')) not in (int, float)
                or not isinstance(result.get('usage'), dict)):
            raise ValueError('invalid agy summary')
        usage = result['usage']
        # agy にキャッシュ書き込みの欄はない。未知の欄を足して量を変えない。
        counts = token_counts(dict(usage, cache_write_tokens=0),
                              ('input_tokens', 'cache_read_tokens', 'cache_write_tokens', 'output_tokens'))
        thinking = usage.get('thinking_tokens', 0)
        if type(thinking) is not int or thinking < 0:
            raise ValueError('invalid thinking tokens')
        denied = result.get('denied_actions', [])
        fields.update(counts, thinking_tokens=thinking,
                      denied_actions=len(denied) if isinstance(denied, list) else None)
        turns, calls, transcript_detail = agy_transcript(result['conversation_id'], usage)
        fields.update(turns=turns, tool_calls=calls)
        if transcript_detail:
            details.append(transcript_detail)
        if fields['denied_actions']:
            details.append('denied_actions: %d' % fields['denied_actions'])
        fields['end'] = 'completed' if result['status'] == 'SUCCESS' and harness_exit == 0 else 'error'
        final = result['response']
    except (ValueError, TypeError, KeyError):
        fields = dict.fromkeys(fields)
        details.append('harness summary unreadable')
    errors = re.findall(r'(?m)^AGY_ERROR: ?(.*)$', err)
    if errors:
        try:
            value = json.loads(errors[-1])
            if not isinstance(value, dict):
                value = {}
            error = json.dumps({key: value[key] for key in ('status', 'code', 'retryable', 'error_id', 'short_error')
                                if key in value and type(value[key]) in (str, int, float, bool, type(None))},
                               ensure_ascii=False)
        except (ValueError, RecursionError):
            error = errors[-1]
        details.append('AGY_ERROR: ' + EMAIL.sub('<email>', error)[:200])
    if harness_exit == 3 or errors:
        fields['end'] = 'error'
        if not errors:
            details.append('agy exited with status 3')
    if AGY_PRINT_TIMEOUT.search(err):
        fields['end'] = 'timeout'
        details.append('agy print timeout')
    return fields, '; '.join(details), final


def external_output(output: str, out: str, harness_exit: int, err: str = '') -> Tuple[Dict[str, object], str, str]:
    """集計の欠損を課題の不合格へ変えず、測れなかった欄を空にする。"""
    try:
        if output == 'dsh-ndjson':
            return dsh_output(out, harness_exit)
        if output == 'claude-json':
            return claude_output(out, harness_exit)
        if output == 'agy-json':
            return agy_output(out, harness_exit, err)
    except (ValueError, KeyError, TypeError):
        pass
    fields = dict.fromkeys(TOKEN_FIELDS + ('turns', 'tool_calls', 'end'))
    return fields, 'harness summary unreadable', ''


def session_file(directory: str, relative: str) -> bytes:
    # 記録のパスを辿って、試行の外のファイルを読み出さない。
    path = os.path.realpath(os.path.join(directory, relative))
    if os.path.commonpath([os.path.realpath(directory), path]) != os.path.realpath(directory):
        raise ValueError('session file is outside the session directory')
    with open(path, 'rb') as f:
        return f.read()


class CompletionPromptMismatch(ValueError):
    """実行ファイルが送った促しと評価の集計に使う促しの版が一致しない。"""


def completion_check_prompt() -> str:
    """集計と見出しで、実行ファイルと同じ規則で包んだ促しを使う。"""
    with open(os.path.join(REPO, 'crates/benitoiteh/prompts/completion-check.md'), encoding='utf-8') as f:
        return '<completion_check>\n%s\n</completion_check>' % f.read().rstrip()


def benitoiteh_results(directory: str, metrics: Optional[Dict[str, object]] = None) -> Tuple[List[List[str]], bool]:
    """記録の結果を既存の診断の集計に渡す。退避した出力は安全の検査だけに使う。"""
    if metrics is not None:
        metrics.update(completion_prompts=0, edits_after_prompt=0,
                       repository_path_seen=False,
                       verification_calls_before=dict.fromkeys(PROCESS_TOOLS, 0),
                       verification_calls_after=dict.fromkeys(PROCESS_TOOLS, 0))
    prompt = completion_check_prompt()
    path = os.path.join(directory, 'session.jsonl')
    if not os.path.isfile(path):
        return [], False
    entries = []
    calls = {}
    shell_seen = False
    with open(path, encoding='utf-8', errors='replace') as f:
        for line in f:
            if not line.strip():
                continue
            value = json_object(line)
            if value is None:
                raise TypeError('session record must be an object')
            if value.get('type') == 'spilled':
                if not isinstance(value['parts'], list):
                    raise TypeError('parts must be a list')
                # UTF-8 の文字の途中で分かれた部分も、バイト列を繋いでから復元する。
                restored = b''.join(session_file(directory, part) for part in value['parts'])
                value = json_object(restored.decode('utf-8', 'replace'))
            if value is None:
                raise TypeError('restored session record must be an object')
            if metrics is not None and repository_path_seen(value):
                metrics['repository_path_seen'] = True
            if value.get('type') == 'compaction' and metrics is not None and 'compaction_records' in metrics:
                metrics['compaction_records'].append({key: value.get(key) for key in
                    ('reason', 'summarized_items', 'kept_items', 'kept_estimate', 'usage')})
            if value.get('type') == 'user' and value.get('kind') == 'environment':
                message = value.get('message')
                if not isinstance(message, dict) or not isinstance(message.get('parts'), list):
                    raise TypeError('environment message must contain a parts list')
                is_prompt = False
                for part in message['parts']:
                    if (not isinstance(part, dict) or not isinstance(part.get('kind'), str)
                            or not isinstance(part.get('text'), str)):
                        raise TypeError('environment parts must contain kind and text strings')
                    text = part['text']
                    if part['kind'] == 'environment' and text.startswith('<completion_check>\n'):
                        if text != prompt:
                            raise CompletionPromptMismatch('completion-check prompt version mismatch')
                        is_prompt = True
                if metrics is not None and is_prompt:
                    metrics['completion_prompts'] += 1
            if value.get('type') == 'tool_call':
                call = value['call']
                if not isinstance(call, dict) or not isinstance(call.get('arguments', {}), dict):
                    raise TypeError('invalid tool call')
                calls[call['id']] = call.get('arguments', {})
                if metrics is not None and value.get('validation_error') is None:
                    after = metrics['completion_prompts'] > 0
                    if call.get('name') in PROCESS_TOOLS:
                        counts = metrics['verification_calls_after' if after else 'verification_calls_before']
                        counts[call['name']] += 1
                    elif after and call.get('name') in ('write', 'edit'):
                        metrics['edits_after_prompt'] += 1
            if value.get('type') != 'tool_result':
                continue
            name = value.get('name')
            result = value['result']
            if not isinstance(result, dict):
                raise TypeError('invalid tool result')
            text = result['text']
            for key in ('spill', 'stderr_spill'):
                spill = value.get(key)
                if spill is not None:
                    if not isinstance(spill, dict):
                        raise TypeError('invalid spill')
                    saved = session_file(directory, spill['path']).decode('utf-8', 'replace')
                    if metrics is not None and repository_path_seen(saved):
                        metrics['repository_path_seen'] = True
                    if name in PROCESS_TOOLS:
                        shell_seen = bool(SHELL_OUTPUT.search(saved)) or shell_seen
            if name in PROCESS_TOOLS:
                arguments = calls.get(result['id'], {})
                entries.append(['', '', '', ','.join(codes_in(text)), PROCESS_TOOLS[name],
                                arguments.get('path', '')])
                shell_seen = shell_seen or bool(SHELL_OUTPUT.search(text))
            elif name in ('write', 'edit'):
                check = re.search(r'(?m)^Check of (.+) after this change:\n', text)
                if check:
                    entries.append(['', '', '', ','.join(codes_in(text[check.end():])), 'check', check.group(1)])
    return entries, shell_seen


def copy_tree(source: str, target: str) -> None:
    if not os.path.isdir(source):
        return
    for root, _dirs, files in os.walk(source):
        relative = os.path.relpath(root, source)
        os.makedirs(os.path.join(target, relative), exist_ok=True)
        for name in files:
            shutil.copyfile(os.path.join(root, name), os.path.join(target, relative, name))


class JudgeStopped(ValueError):
    """実行器の結果を信用できない場合は、後続の評価を止める。"""


def find_judge_runner(given: Optional[str] = None) -> str:
    if sys.platform != 'darwin':
        raise ValueError('judgment requires the macOS sandbox')
    target = os.path.abspath(os.environ.get('CARGO_TARGET_DIR', os.path.join(REPO, 'target')))
    candidates = ([given] if given else [os.path.join(target, profile, 'examples', 'skill_eval_judge')
                                        for profile in ('debug', 'release')])
    for path in candidates:
        if os.path.isfile(path) and os.access(path, os.X_OK):
            return os.path.realpath(path)
    raise ValueError('build --example skill_eval_judge or pass --judge-runner')


def loopback_port(task: Task, allowed: set, http_base: Optional[str]) -> Optional[int]:
    if task.name != 'http_client' or 'Http.Connect' not in allowed or http_base is None:
        return None
    match = re.fullmatch(r'http://127\.0\.0\.1:([0-9]{1,5})', http_base)
    if match is None or not 1 <= int(match.group(1)) <= 65535:
        raise ValueError('invalid judgment HTTP endpoint')
    return int(match.group(1))


def runner_execute(command: List[str], directory: str, programs: List[str],
                   read_roots: Optional[List[str]] = None, port: Optional[int] = None,
                   workspace: Optional[str] = None) -> dict:
    """信頼する検査を writable の外に置き、実行器自身には親の秘密の場所を知らせる。"""
    runner = find_judge_runner(JUDGE_RUNNER)
    helper = os.path.realpath(os.path.join(os.path.dirname(os.path.dirname(runner)), 'benitoiteh'))
    with tempfile.TemporaryDirectory(prefix='jt-', dir='/tmp') as temporary:
        temporary = os.path.realpath(temporary)
        directory = os.path.realpath(directory)
        JUDGE_ROOTS.extend([directory, temporary])
        args = [runner, '--helper', helper, '--cwd', directory, '--temp', temporary, '--timeout', '60',
                '--deny-read', os.path.realpath(REPO)]
        denied_workspace = workspace or JUDGE_WORKSPACE
        if denied_workspace and os.path.isdir(denied_workspace):
            args += ['--deny-read', os.path.realpath(denied_workspace)]
        roots = [directory, temporary] + (read_roots or [])
        # HOME 全体を読む許可は作らない。実行に必要な OS の木だけを列挙する。
        roots += [path for path in ('/System', '/Library', '/usr', '/bin', '/sbin', '/opt', '/private/etc', '/private/var/select', '/dev')
                  if os.path.isdir(path)]
        for root in sorted(set(os.path.realpath(path) for path in roots)):
            args += ['--read-root', root]
        for program in sorted(set(os.path.realpath(path) for path in programs)):
            args += ['--allow-program', program]
        if port is not None:
            args += ['--allow-loopback-port', str(port)]
        args += ['--'] + command
        env = {key: value for key, value in os.environ.items()
               if key in ('HOME', 'XDG_CONFIG_HOME', 'XDG_STATE_HOME', 'TMPDIR')}
        env['PATH'] = '/usr/bin:/bin:/usr/sbin:/sbin'
        try:
            result = subprocess.run(args, cwd='/', env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, timeout=100, close_fds=True)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise JudgeStopped('judge runner could not complete: ' + type(error).__name__) from None
        if result.returncode < 0:
            raise JudgeStopped('judge runner terminated by signal %d' % -result.returncode)
        report = json_object(result.stdout.decode('utf-8', 'replace'))
        bools = ('success', 'timed_out', 'possibly_remaining', 'stop_required', 'stdout_truncated', 'stderr_truncated')
        if (not report or type(report.get('schema_version')) is not int or report['schema_version'] != 1
                or any(type(report.get(key)) is not bool for key in bools)
                or any(not isinstance(report.get(key), str) for key in ('stdout', 'stderr', 'end'))
                or any(report.get(key) is not None and type(report[key]) is not int for key in ('signal', 'exit_code'))
                or any(key not in report for key in ('signal', 'exit_code', 'error'))
                or (report.get('error') is not None and not isinstance(report.get('error'), str))
                or report.get('end') not in ('exited', 'timed_out', 'cancelled', 'error')
                or result.returncode not in (0, 1) or (result.returncode == 0) != report['success']):
            raise JudgeStopped('unreadable or inconsistent judge runner JSON')
        if report['stop_required'] or report['possibly_remaining']:
            raise JudgeStopped('judge runner detected possible remaining processes; stop subsequent evaluation')
        if report['signal'] is not None:
            report['success'] = False
        return report


def check_judge_processes() -> Optional[str]:
    """false の残存の印を保証にせず、評価の終わりにも cwd を調べる。"""
    return check_process_cwds(JUDGE_ROOTS)


def lsof_path(path: str) -> str:
    """lsof の LC_ALL=C の表記へ直し、制御文字や非 ASCII の根も照合する。"""
    result = []
    escapes = {8: r'\b', 12: r'\f', 10: r'\n', 13: r'\r', 9: r'\t', 127: '^?'}
    for byte in os.fsencode(os.path.realpath(path)):
        result.append(escapes[byte] if byte in escapes else ('^' + chr(byte + 64)) if byte < 32
                      else chr(byte) if byte < 128 else '\\x%02x' % byte)
    return ''.join(result)


def check_process_cwds(roots: List[str]) -> Optional[str]:
    """実行器と同じ cwd の検出を、workspace の読み取りの前にも使う。"""
    if not roots:
        return None
    failure = 'could not inspect remaining evaluation processes'
    try:
        result = subprocess.run(['/usr/sbin/lsof', '-a', '-d', 'cwd', '-u', str(os.getuid()), '-Fpn'],
                                cwd='/', env={'LC_ALL': 'C', 'PATH': '/usr/bin:/bin'},
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10, close_fds=True)
        if result.returncode != 0:
            return failure
        roots = [lsof_path(root) for root in roots]
        process, seen_cwd = None, False
        for line in result.stdout.decode('ascii').splitlines():
            if line.startswith('p') and line[1:].isdigit() and int(line[1:]) > 0:
                if process is not None and not seen_cwd:
                    return failure
                process, seen_cwd = int(line[1:]), False
            elif line == 'fcwd' and process is not None:
                continue
            elif line.startswith('n/') and process is not None:
                path = line[1:]
                for alias in ('/tmp', '/var', '/etc'):
                    if path == alias or path.startswith(alias + '/'):
                        path = '/private' + path
                        break
                seen_cwd = True
                if process != os.getpid() and any(path == root or path.startswith(root + '/') for root in roots):
                    return 'evaluation processes may remain; inspect the evaluation work directories'
            else:
                return failure
        if not seen_cwd:
            return failure
    except (OSError, subprocess.TimeoutExpired, UnicodeError):
        return failure
    return None


def prepare_judgment(runner: Optional[str], tasks: List[Task]) -> None:
    global JUDGE_RUNNER, NORMAL_TOOLS
    try:
        JUDGE_RUNNER = find_judge_runner(runner)
        if tasks and tasks[0].kind == 'normal':
            NORMAL_TOOLS = prepare_normal_tools(tasks)
    except ValueError as error:
        raise SystemExit(str(error)) from None


def prepare_normal_tools(tasks: List[Task]) -> Dict[str, Dict[str, str]]:
    identities = {}
    tools = set(tool for task in tasks for tool in task.required_tools)
    if '/bin/sh' in tools:
        tools.add('/bin/bash')
    for tool in sorted(tools):
        path = shutil.which(tool, path='/usr/bin:/bin')
        if not path:
            raise ValueError('required tool not found: ' + tool)
        path = os.path.realpath(path)
        if tool == 'python3':
            path = '/usr/bin/python3' if os.path.isfile('/usr/bin/python3') else path
            result = subprocess.run([path, '-I', '-c', 'import os,sys; print(os.path.realpath(sys.executable))'],
                                    env={'PATH': '/usr/bin:/bin'}, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    timeout=10, close_fds=True)
            if result.returncode != 0:
                raise ValueError('cannot resolve python3 interpreter')
            path = os.path.realpath(result.stdout.decode().strip())
        identity = dict(path=path, sha256=file_sha256(path))
        if tool in ('/bin/sh', '/bin/bash'):
            identity['version'] = version_of([path, '--version'])
        if tool == 'python3':
            identity['version'] = version_of([path, '--version'])
            programs = [path]
            version_root = os.path.dirname(os.path.dirname(path))
            framework_root = os.path.dirname(os.path.dirname(version_root))
            if (os.path.basename(os.path.dirname(path)) == 'bin'
                    and os.path.basename(os.path.dirname(version_root)) == 'Versions'
                    and os.path.basename(framework_root).endswith('.framework')):
                # macOS の framework の shim は Python.app の実体を起動し直す。
                # 同じ版の固定の 1 ファイルだけを許し、ディレクトリ全体へ広げない。
                app = os.path.join(version_root, 'Resources', 'Python.app', 'Contents', 'MacOS', 'Python')
                if os.path.lexists(app):
                    app = os.path.realpath(app)
                    if (os.path.commonpath([app, version_root]) != version_root
                            or not os.path.isfile(app) or not os.access(app, os.X_OK)):
                        raise ValueError('cannot resolve python3 framework app executable')
                    programs.append(app)
                    identities['python3.app'] = dict(path=app, sha256=file_sha256(app))
            # verify.py と同じく、Python の子から sys.executable を起動できることも確かめる。
            probe = ('import subprocess,sys; subprocess.run('
                     '[sys.executable, "-I", "-B", "-c", \'print("ready")\'], check=True, close_fds=True)')
            with tempfile.TemporaryDirectory(prefix='jp-', dir='/tmp') as directory:
                directory = os.path.realpath(directory)
                report = runner_execute([path, '-I', '-B', '-c', probe], directory, programs)
                if not report['success'] or report['stdout'] != 'ready\n':
                    raise ValueError('python3 cannot run in the judgment sandbox')
        identities[tool] = identity
    return identities


def normal_snapshot(directory: str) -> dict:
    """ディレクトリの記述子から辿り、通常のファイルだけを開いてハッシュする。"""
    result = {}
    def visit(directory_fd: int, prefix: str) -> None:
        for name in os.listdir(directory_fd):
            relative = prefix + name
            info = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
            if stat.S_ISREG(info.st_mode):
                with open_regular_file(directory_fd, name) as stream:
                    actual = os.fstat(stream.fileno())
                    if (info.st_dev, info.st_ino) != (actual.st_dev, actual.st_ino):
                        raise ValueError('workspace entry changed during snapshot')
                    digest = hashlib.sha256()
                    remaining = actual.st_size
                    while remaining > 0:
                        chunk = stream.read(min(1024 * 1024, remaining))
                        if not chunk:
                            raise ValueError('workspace entry changed during snapshot')
                        digest.update(chunk)
                        remaining -= len(chunk)
                    result[relative] = ('file', digest.hexdigest())
            elif stat.S_ISDIR(info.st_mode):
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory_fd)
                try:
                    actual = os.fstat(child)
                    if (info.st_dev, info.st_ino) != (actual.st_dev, actual.st_ino):
                        raise ValueError('workspace entry changed during snapshot')
                    result[relative] = ('dir', '')
                    visit(child, relative + '/')
                finally:
                    os.close(child)
            else:
                result[relative] = ('special', '')
    descriptor = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        visit(descriptor, '')
    finally:
        os.close(descriptor)
    return result


def open_regular_file(directory: int, name: str):
    """開く間のリンク・FIFO への置換を snapshot と成果物の読み取りで同じく拒む。"""
    descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
    stream = os.fdopen(descriptor, 'rb')
    if not stat.S_ISREG(os.fstat(descriptor).st_mode):
        stream.close()
        raise ValueError('artifact is not a regular file')
    return stream


def artifact_bytes(root: str, relative: str) -> bytes:
    """残った子がパスを替えても、リンクや FIFO を開かない（実装プラン P5-34）。"""
    directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        parts = relative.split('/')
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=directory)
            os.close(directory)
            directory = child
        with open_regular_file(directory, parts[-1]) as stream:
            return stream.read()
    finally:
        os.close(directory)


def normal_changes(task: Task, workspace: str) -> Tuple[List[str], List[str]]:
    baseline = normal_snapshot(task.path('input'))
    baseline['task.md'] = ('file', file_sha256(task.path('task.md')))
    actual = normal_snapshot(workspace)
    changed = sorted(path for path in set(baseline) | set(actual) if baseline.get(path) != actual.get(path))
    problems = []
    for path in changed:
        kind = actual.get(path, ('missing', ''))[0]
        if path in task.writable_files and kind == 'file':
            continue
        generated = any(path == item.rstrip('/') or (item.endswith('/') and path.startswith(item))
                        for item in task.allowed_generated)
        if path not in baseline and generated and kind in ('file', 'dir'):
            continue
        problems.append('%s: %s' % (path, kind))
    return changed, problems


def judge_normal(task: Task, workspace: str) -> dict:
    workspace = os.path.realpath(workspace)
    problem = check_process_cwds([workspace])
    if problem:
        raise JudgeStopped('before workspace snapshot: ' + problem)
    changed, problems = normal_changes(task, workspace)
    artifacts = {}
    for path in task.artifacts:
        try:
            artifacts[path] = artifact_bytes(workspace, path)
        except (OSError, ValueError):
            problems.append('missing or non-regular artifact: ' + path)
    results = []
    if not problems:
        for name in ('verify', 'judge'):
            # 毎回原本へ戻し、前のコマンドに書き換えられたテストを再使用しない。
            with tempfile.TemporaryDirectory(prefix='nj-', dir='/tmp') as root:
                # command にも実のパスを使い、read-root の外の /tmp のリンクを辿らせない。
                root = os.path.realpath(root)
                directory, trusted = os.path.join(root, 'case'), os.path.join(root, 'trusted')
                prepare_inputs(task, directory)
                os.mkdir(trusted)
                copy_tree(task_path(task.dir, 'input/tests', True), os.path.join(trusted, 'tests'))
                copy_tree(task_path(task.dir, 'judge', True), os.path.join(trusted, 'judge'))
                for path, content in artifacts.items():
                    with open(os.path.join(directory, path), 'wb') as stream:
                        stream.write(content)
                python = os.path.realpath(NORMAL_TOOLS.get('python3', {}).get('path', sys.executable))
                # -I は PYTHONDONTWRITEBYTECODE を無視する。信頼した検査の場所へ
                # __pycache__ を書かないよう、環境だけに頼らず -B を明示する。
                command = ([python, '-B', '-m', 'unittest', 'discover', '-s', os.path.join(trusted, 'tests')]
                           if name == 'verify' else [python, '-I', '-B', os.path.join(trusted, 'judge/verify.py')])
                # 通常モードの課題は、インストール済みの道具を使う契約である。
                # 起動先の列挙は Benitoite の課題にだけ掛け、未記載の道具の制限を課さない（実装プラン P5-34）。
                report = runner_execute(command, directory, [], [trusted], workspace=workspace)
                cases = None
                if name == 'judge':
                    cases = json_object(report['stdout'])
                    cases = cases.get('cases') if cases else None
                    if (not isinstance(cases, list) or not cases or any(
                            not isinstance(case, dict) or not isinstance(case.get('name'), str)
                            or type(case.get('success')) is not bool or not isinstance(case.get('reason'), str)
                            for case in cases) or len({case['name'] for case in cases}) != len(cases)):
                        cases = None
                        report['success'] = False
                        report['error'] = 'invalid judge cases JSON'
                success = (report['success'] and report['end'] == 'exited' and report['exit_code'] == 0
                           and not report['timed_out'] and not report['stdout_truncated']
                           and not report['stderr_truncated'] and report['signal'] is None
                           and (name != 'judge' or (cases is not None and all(case['success'] for case in cases))))
                results.append(dict(name=name, success=success, cases=cases, runner=report))
    artifact_success = not problems and len(results) == 2 and all(result['success'] for result in results)
    return dict(artifact_success=artifact_success, changed_files=changed, integrity_errors=problems,
                judge_results=results, reason='ok' if artifact_success else '; '.join(problems) or 'artifact checks failed')


def bash_executions(out: str) -> List[int]:
    statuses = []
    for line in out.splitlines():
        event = json_object(line)
        if event and event.get('type') == 'tool_result' and event.get('name') == 'bash':
            screen = event.get('screen')
            if isinstance(screen, str) and re.fullmatch(r'exit -?[0-9]+', screen):
                statuses.append(int(screen[5:]))
    return statuses


def prepare_inputs(task: Task, directory: str) -> None:
    """課題の入力を作業ディレクトリに写す。解のスクリプト（reference.bnt）と期待する出力は写さない。"""
    os.makedirs(directory, exist_ok=True)
    copy_tree(task.path('input'), directory)


def run_command(command: List[str], cwd: str, env: Dict[str, str], timeout: int = RUN_TIMEOUT) -> Tuple[int, str, str]:
    """コマンドを新しいプロセスの群れで起動し、正常終了後も同じ群れの子孫を止める。

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
    finally:
        # 出力を閉じて背景に残る子も、判定の後に動かし続けない（設計書 07-02「評価の手順」）。
        # 別の群れへ移った子の検出は、この起動の口では行わない。
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    return process.returncode, out.decode('utf-8', 'replace'), err.decode('utf-8', 'replace')


def harness_environment() -> Tuple[Dict[str, str], List[str]]:
    """親の対話とキーを評価の対象に渡さない（設計書 07-02「評価の手順」）。"""
    keys = {'DEEPSEEK_API_KEY', 'GEMINI_API_KEY', 'GOOGLE_API_KEY', 'OPENAI_API_KEY'}
    removed = sorted(name for name in os.environ if name != 'CLAUDE_CONFIG_DIR'
                     and (name.startswith(('CLAUDE', 'ANTHROPIC')) or name in keys))
    return {name: value for name, value in os.environ.items() if name not in removed}, removed


@functools.lru_cache(maxsize=32)
def repository_markers(root: str, home: str) -> List[str]:
    """worktree と元の checkout を印にし、HOME と分けた綴りも検出する。"""
    roots = [os.path.realpath(root)]
    try:
        with open(os.path.join(root, '.git'), encoding='utf-8') as f:
            pointer = f.read().strip()
        if pointer.startswith('gitdir: '):
            gitdir = os.path.join(root, pointer[len('gitdir: '):])
            with open(os.path.join(gitdir, 'commondir'), encoding='utf-8') as f:
                common = os.path.realpath(os.path.join(gitdir, f.read().strip()))
            if os.path.basename(common) == '.git':
                roots.append(os.path.dirname(common))
    except (OSError, UnicodeError):
        pass
    markers = ['tools/skill-eval/tasks']
    for path in roots:
        markers.extend(path_prefixes(path))
        if home:
            relative = os.path.relpath(path, os.path.realpath(home))
            if relative != '.' and relative != '..' and not relative.startswith('../'):
                markers.append('/' + relative)
    return markers


def repository_path_seen(value: object) -> bool:
    """JSON のエスケープを戻した文字列から、評価の原本への参照を検出する。"""
    markers = repository_markers(REPO, os.environ.get('HOME', ''))
    pending = [value]
    while pending:
        item = pending.pop()
        if isinstance(item, str) and any(path in item for path in markers):
            return True
        if isinstance(item, dict):
            pending.extend(item.keys())
            pending.extend(item.values())
        elif isinstance(item, list):
            pending.extend(item)
    return False


def benitoiteh_environment(env: Dict[str, str]) -> Tuple[Dict[str, str], List[str]]:
    removed = sorted(name for name, value in env.items()
                     if name in ('PWD', 'OLDPWD') or repository_path_seen(value))
    return {name: value for name, value in env.items() if name not in removed}, removed


def dsh_environment() -> Tuple[Dict[str, str], List[str]]:
    """外からサンドボックスや送信先を変える指定を受け継がない（P5-04 補足 4）。"""
    env, removed = harness_environment()
    extra = [name for name in env if name.startswith(('DSH_', 'DEEPSEEK_', 'NODE_'))]
    for name in extra:
        del env[name]
    return env, sorted(removed + extra)


def agy_environment() -> Tuple[Dict[str, str], List[str]]:
    """起動の設定を親の環境から独立させる。更新を止める環境変数は実走で無効だった。"""
    env, removed = harness_environment()
    extra = [name for name in env if name.startswith(('AGY_', 'ANTIGRAVITY_', 'JETSKI_',
                                                     'GOOGLE_', 'GEMINI_', 'CLOUDSDK_'))]
    for name in extra:
        del env[name]
    env['AGY_CLI_DISABLE_AUTO_UPDATE'] = '1'
    return env, sorted(removed + extra)


def ancestors(directory: str) -> List[str]:
    chain = []
    current = os.path.realpath(directory)
    while True:
        chain.append(current)
        parent = os.path.dirname(current)
        if parent == current:
            return chain
        current = parent


def check_agy_config(workspace: str, work_root: Optional[str] = None) -> List[str]:
    """設定の鍵を許可の一覧で検査し、会話やトークンの領域は歩かない。"""
    home = os.path.expanduser('~')
    gemini = os.path.join(home, '.gemini')
    config = os.path.join(gemini, 'config')
    cli = os.path.join(gemini, 'antigravity-cli')
    unwanted = []
    def failed(error: OSError) -> None:
        raise ValueError('cannot inspect Antigravity directory: %s' % error.filename)
    directories = [home, gemini, cli]
    if os.path.lexists(config):
        for directory, dirs, _files in os.walk(config, onerror=failed):
            dirs[:] = [name for name in dirs if name != 'brain']
            directories.append(directory)
    for directory in directories:
        for name in ('GEMINI.md', 'AGENTS.md'):
            path = os.path.join(directory, name)
            if os.path.lexists(path):
                unwanted.append(path)
    for name in ('skills', 'plugins', 'rules', 'hooks.json', 'skills.json', 'plugins.json', 'agents',
                 'rules.json', 'agents.json', 'workflows', 'global_workflows', 'workflows.json', 'sidecars'):
        path = os.path.join(config, name)
        if os.path.lexists(path):
            unwanted.append(path)
    for path in (os.path.join(cli, 'hooks.json'), '/Library/Application Support/Antigravity/admin_settings.json'):
        if os.path.lexists(path):
            unwanted.append(path)
    for directory in (config, cli):
        mcp = os.path.join(directory, 'mcp_config.json')
        if not os.path.lexists(mcp):
            continue
        try:
            with open(mcp, encoding='utf-8') as f:
                text = f.read().strip()
            value = json.loads(text) if text else {}
            empty = ({},) if directory == cli else ({}, {'mcpServers': {}}, {'servers': {}})
            if value not in empty:
                unwanted.append(mcp)
        except (OSError, ValueError, RecursionError):
            raise ValueError('cannot inspect Antigravity mcp_config.json') from None
    settings = [(os.path.join(cli, 'settings.json'), {'model', 'trustedWorkspaces'}, 'settings'),
                (os.path.join(config, 'config.json'), {'userSettings'}, 'config')]
    projects = os.path.join(config, 'projects')
    if os.path.isdir(projects):
        settings += [(os.path.join(projects, name), {'id', 'name', 'projectResources'}, 'project')
                     for name in sorted(os.listdir(projects)) if name.endswith('.json')]
    for path, allowed, kind in settings:
        if not os.path.lexists(path):
            continue
        first_unwanted = len(unwanted)
        try:
            with open(path, encoding='utf-8') as f:
                value = json.load(f)
            if not isinstance(value, dict):
                raise ValueError('invalid settings')
            unwanted += [path + ': ' + key for key in sorted(set(value) - allowed)]
            if kind == 'config' and 'userSettings' in value:
                user = value['userSettings']
                if not isinstance(user, dict):
                    raise ValueError('invalid userSettings')
                unwanted += [path + ': userSettings.' + key for key in sorted(set(user) - {'remoteControlHostname'})]
            if kind == 'project':
                def keys(table: object, allowed: set, prefix: str) -> dict:
                    if not isinstance(table, dict):
                        raise ValueError('invalid project resource object')
                    unwanted.extend(path + ': ' + prefix + '.' + key for key in sorted(set(table) - allowed))
                    return table
                resources = keys(value.get('projectResources', {}), {'resources'}, 'projectResources')
                items = resources.get('resources', [])
                if not isinstance(items, list):
                    raise ValueError('invalid project resources list')
                for item in items:
                    resource = keys(item, {'folderUri', 'gitFolder'}, 'projectResources.resources')
                    folders = [resource] if 'folderUri' in resource else []
                    if 'gitFolder' in resource:
                        git = keys(resource['gitFolder'], {'folderUri', 'allowWrite'},
                                   'projectResources.resources.gitFolder')
                        if 'allowWrite' in git and type(git['allowWrite']) is not bool:
                            raise ValueError('invalid allowWrite')
                        folders.append(git)
                    for entry in folders:
                        uri = entry.get('folderUri')
                        if not isinstance(uri, str):
                            raise ValueError('invalid folderUri')
                        if not uri.startswith('file://'):
                            continue
                        folder = os.path.realpath(agy_gate.file_path(uri))
                        root = os.path.realpath(work_root or workspace)
                        if os.path.commonpath([root, folder]) in (root, folder):
                            unwanted.append(path + ': projectResources.resources.folderUri')
        except (OSError, ValueError, RecursionError):
            detail = '; ' + ', '.join(unwanted[first_unwanted:]) if len(unwanted) > first_unwanted else ''
            raise ValueError('cannot inspect Antigravity settings: %s%s' % (path, detail)) from None
    for index, directory in enumerate(ancestors(workspace)):
        for name in ('GEMINI.md', 'AGENTS.md'):
            path = os.path.join(directory, name)
            if os.path.lexists(path):
                unwanted.append(path)
            nested = os.path.join(directory, '.gemini', name)
            if index and os.path.lexists(nested):
                unwanted.append(nested)
        if index:
            for name in ('.agents', '.agent', '_agents', '_agent', '.gemini/settings.json'):
                path = os.path.join(directory, name)
                if os.path.lexists(path):
                    unwanted.append(path)
    if unwanted:
        raise ValueError('remove user Antigravity configuration before evaluation: ' + ', '.join(sorted(set(unwanted))))
    builtin = os.path.join(cli, 'builtin', 'skills')
    return sorted(os.listdir(builtin)) if os.path.isdir(builtin) else []


def prepare_agy(trial: str, workspace: str) -> None:
    """フックのコマンドを引用し、作業ディレクトリの外に門を置く。"""
    directory = os.path.join(trial, 'agy')
    os.makedirs(directory)
    gate = os.path.join(directory, 'agy_gate.py')
    shutil.copyfile(os.path.join(HERE, 'agy_gate.py'), gate)
    command = (shlex.quote(sys.executable) + ' -I ' + shlex.quote(os.path.abspath(gate))
               + ' ' + shlex.quote(os.path.realpath(workspace)))
    # 門はログを作らず、準備で作った通常ファイルにだけ追記する。
    with open(os.path.join(directory, 'gate.log'), 'w', encoding='utf-8'):
        pass
    hooks = {'evaluation-gate': {'PreToolUse': [{'matcher': '.*', 'hooks': [
        {'type': 'command', 'command': command, 'timeout': 30}]}]}}
    with open(os.path.join(workspace, '.agents', 'hooks.json'), 'w', encoding='utf-8') as f:
        f.write(json.dumps(hooks, ensure_ascii=False, indent=2) + '\n')


def agy_user_settings_state() -> Dict[str, object]:
    """許可の保存などを検知するため、指定した設定だけを比較する（P5-07 補足 2）。"""
    gemini = os.path.join(os.path.expanduser('~'), '.gemini')
    config = os.path.join(gemini, 'config')
    projects = os.path.join(config, 'projects')
    # リンクや特殊ファイルに替わった設定は開かず、事後なら比較不能として停止させる。
    for path in (gemini, config, projects, os.path.join(gemini, 'antigravity-cli')):
        if os.path.lexists(path) and (os.path.islink(path) or not os.path.isdir(path)):
            raise ValueError('cannot inspect Antigravity settings directory: %s' % path)
    try:
        state = {'config entries': sorted(os.listdir(config)) if os.path.isdir(config) else None}
        paths = ['config/config.json', 'config/mcp_config.json', 'antigravity-cli/settings.json']
        if os.path.isdir(projects):
            paths += ['config/projects/' + name for name in sorted(os.listdir(projects)) if name.endswith('.json')]
        for relative in paths:
            path = os.path.join(gemini, relative)
            if os.path.islink(path):
                raise ValueError('settings path is a link')
            state[relative] = file_sha256(path) if os.path.lexists(path) else None
        return state
    except (OSError, ValueError):
        raise ValueError('cannot inspect Antigravity user settings') from None


def agy_integrity(trial: str, workspace: str) -> Dict[str, object]:
    """実行の前後を比べる。リンクに替わったファイルの参照先は開かない。"""
    agents = os.path.join(workspace, '.agents')
    inventory = []
    def failed(error: OSError) -> None:
        raise error
    try:
        for root, dirs, files in os.walk(agents, followlinks=False, onerror=failed):
            for name in sorted(dirs + files):
                path = os.path.join(root, name)
                inventory.append((os.path.relpath(path, agents), stat.S_IFMT(os.lstat(path).st_mode)))
        inventory.sort()
        regular_tree = (os.path.isdir(agents) and not os.path.islink(agents)
                        and all(kind in (stat.S_IFDIR, stat.S_IFREG) for _name, kind in inventory))
    except OSError:
        inventory, regular_tree = None, False
    values = {'.agents entries': inventory}
    paths = {'hooks.json': os.path.join(agents, 'hooks.json'),
             'bin/benitoite': os.path.join(trial, 'bin', 'benitoite'),
             'agy/agy_gate.py': os.path.join(trial, 'agy', 'agy_gate.py')}
    for name, path in paths.items():
        try:
            if (os.path.islink(path) or os.path.islink(os.path.dirname(path))
                    or (name == 'hooks.json' and not regular_tree)):
                raise ValueError('integrity path is a link')
            if name == 'hooks.json':
                with open(path, 'rb') as f:
                    values[name] = f.read()
            else:
                values[name] = file_sha256(path)
        except (OSError, ValueError):
            values[name] = None
    try:
        skill = os.path.join(agents, 'skills', 'benitoite')
        values['benitoite Skill'] = skill_digest(skill) if regular_tree and os.path.isdir(skill) else None
    except OSError:
        values['benitoite Skill'] = None
    return values


def user_skill_state() -> Dict[str, object]:
    """v0.0.7 の skill/mod.rs の agent_parent の二か所を、内容を開かずに比べる。"""
    states = {}
    for relative in ('.claude/skills/benitoite', '.agents/skills/benitoite'):
        path = os.path.join(os.path.expanduser('~'), relative)
        entries = []
        def metadata(item: str) -> None:
            try:
                info = os.lstat(item)
                entries.append((os.path.relpath(item, path), stat.S_IFMT(info.st_mode), info.st_mtime_ns))
            except OSError as error:
                entries.append((os.path.relpath(item, path), 'unreadable', error.errno))
        if os.path.lexists(path):
            metadata(path)
            if os.path.isdir(path) and not os.path.islink(path):
                def failed(error: OSError) -> None:
                    entries.append((error.filename, 'unreadable', error.errno))
                for root, dirs, files in os.walk(path, followlinks=False, onerror=failed):
                    for name in sorted(dirs + files):
                        metadata(os.path.join(root, name))
        states[relative] = sorted(entries)
    return states


def agy_permissions(workspace: str, writable: bool) -> List[Dict[str, object]]:
    """開いた親ディレクトリを基準に権限を変え、消えた項目などの失敗は記録して続ける。"""
    errors = []
    def failed(path: str, error: Exception) -> None:
        errors.append({'path': path, 'stage': 'restore' if writable else 'read-only',
                       'error': type(error).__name__, 'errno': getattr(error, 'errno', None)})
    def change(name: str, parent: int, relative: str) -> None:
        try:
            info = os.stat(name, dir_fd=parent, follow_symlinks=False)
            directory = stat.S_ISDIR(info.st_mode)
            if not directory and not stat.S_ISREG(info.st_mode):
                return
            mode = (0o755 if writable else 0o555) if directory else (0o644 if writable else 0o444)
            if os.chmod in os.supports_follow_symlinks:
                try:
                    os.chmod(name, mode, dir_fd=parent, follow_symlinks=False)
                    return
                except (NotImplementedError, TypeError):
                    pass
            flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | (os.O_DIRECTORY if directory else 0)
            descriptor = os.open(name, flags, dir_fd=parent)
            try:
                opened = os.fstat(descriptor)
                if ((opened.st_dev, opened.st_ino) != (info.st_dev, info.st_ino)
                        or stat.S_IFMT(opened.st_mode) != stat.S_IFMT(info.st_mode)):
                    raise ValueError('permission target changed')
                os.fchmod(descriptor, mode)
            finally:
                os.close(descriptor)
        except (OSError, ValueError) as error:
            failed(relative, error)
    try:
        workspace_fd = os.open(workspace, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            if stat.S_ISDIR(os.stat('.agents', dir_fd=workspace_fd, follow_symlinks=False).st_mode):
                change('.agents', workspace_fd, '.agents')
                def walk_failed(error: OSError) -> None:
                    failed(error.filename or '.agents', error)
                for root, dirs, files, parent in os.fwalk('.agents', dir_fd=workspace_fd,
                                                         follow_symlinks=False, onerror=walk_failed):
                    for name in dirs + files:
                        change(name, parent, os.path.join(root, name))
        finally:
            os.close(workspace_fd)
    except (OSError, ValueError) as error:
        failed('.agents', error)
    if errors:
        try:
            log_dir = os.open(os.path.join(os.path.dirname(workspace), 'agy'),
                              os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            try:
                descriptor = os.open('permissions.log', os.O_WRONLY | os.O_APPEND | os.O_CREAT
                                     | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600, dir_fd=log_dir)
                with os.fdopen(descriptor, 'a', encoding='utf-8') as f:
                    if not stat.S_ISREG(os.fstat(f.fileno()).st_mode):
                        raise ValueError('permission log is not a regular file')
                    for error in errors:
                        f.write(json.dumps(error) + '\n')
            finally:
                os.close(log_dir)
        except (OSError, ValueError) as error:
            failed('permissions.log', error)
    return errors


def agy_protected_entries(workspace: str) -> set:
    entries = set()
    def failed(error: OSError) -> None:
        entries.add('<unreadable workspace entry>')
    for root, dirs, files in os.walk(workspace, followlinks=False, onerror=failed):
        for name in dirs + files:
            if name.lower() in agy_gate.PROTECTED_DIRS | agy_gate.PROTECTED_FILES:
                entries.add(os.path.relpath(os.path.join(root, name), workspace))
    return entries


def agy_gate_records(trial: str) -> Tuple[Dict[str, int], bool]:
    """道具ごとの決定数と、包みの起動に対応する門の許可を調べる。"""
    try:
        descriptor = os.open(os.path.join(trial, 'agy', 'gate.log'),
                             os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(descriptor, encoding='utf-8', errors='replace') as f:
            info = os.fstat(f.fileno())
            if not stat.S_ISREG(info.st_mode):
                return {}, False
            # 別の群れに残った子が追記していても、読み取りの開始時の量で打ち切る。
            values = [json_object(line) for line in f.read(info.st_size).splitlines()]
        records = [value for value in values if value is not None and value.get('decision') in ('allow', 'deny')]
        calls = {}
        for value in records:
            raw_name = value.get('tool')
            name = raw_name if isinstance(raw_name, str) and re.fullmatch(r'[a-z0-9_]{1,64}', raw_name) else '<invalid>'
            calls[name] = calls.get(name, 0) + 1
        return calls, any(value.get('tool') == 'run_command' and value.get('decision') == 'allow'
                          for value in records)
    except (OSError, ValueError):
        return {}, False


def dsh_overlay(model: str, effort: str) -> str:
    if not re.fullmatch(r'[A-Za-z0-9.-]+', model) or effort not in ('off', 'low', 'high', 'max'):
        raise ValueError('invalid DeepSeek model or effort')
    return ('- id: agent-default-model\n  config:\n    provider: deepseek-official\n'
            '    model: %s\n    reasoningEffort: %s\n'
            '- id: session-log-deepseek\n  config:\n    enabled: false\n'
            '- id: plugin-package-inventory-deepseek\n  disabled: true\n'
            '- id: tool-web\n  disabled: true\n'
            '- id: session-title-llm\n  disabled: true\n') % (json.dumps(model), json.dumps(effort))


def check_dsh_workspace(workspace: str) -> None:
    for directory in ancestors(workspace):
        if os.path.lexists(os.path.join(directory, '.git')):
            raise ValueError('DeepSeek workspace has a .git ancestor: %s' % directory)


def check_claude_config(workspace: str) -> List[str]:
    """普段のログインは保ち、利用者が加えた指示の名前だけを調べる。"""
    config = os.environ.get('CLAUDE_CONFIG_DIR') or os.path.expanduser('~/.claude')
    unwanted = []
    skills = os.path.join(config, 'skills')
    if os.path.isdir(skills):
        for name in sorted(os.listdir(skills)):
            path = os.path.join(skills, name)
            if name != 'synced' and (os.path.isdir(path) or os.path.islink(path)):
                unwanted.append('skills/' + name)
    for name in ('agents', 'commands', 'output-styles'):
        path = os.path.join(config, name)
        if os.path.isdir(path):
            unwanted += [name + '/' + child for child in sorted(os.listdir(path))]
        elif os.path.lexists(path):
            unwanted.append(name)
    if os.path.lexists(os.path.join(config, 'CLAUDE.md')):
        unwanted.append('CLAUDE.md')
    settings_path = os.path.join(config, 'settings.json')
    if os.path.exists(settings_path):
        try:
            with open(settings_path, encoding='utf-8') as f:
                settings = json.load(f)
            if (not isinstance(settings, dict) or not isinstance(settings.get('enabledPlugins', {}), dict)
                    or not isinstance(settings.get('permissions', {}), dict)):
                raise ValueError('invalid settings')
        except (OSError, ValueError):
            raise ValueError('cannot inspect Claude Code settings.json') from None
        if any(settings.get('enabledPlugins', {}).values()):
            unwanted.append('settings.json: enabledPlugins')
        unwanted += ['settings.json: ' + name for name in ('hooks', 'outputStyle', 'apiKeyHelper', 'env')
                     if name in settings]
        if 'allow' in settings.get('permissions', {}):
            unwanted.append('settings.json: permissions.allow')
    for directory in ancestors(workspace):
        for name in ('CLAUDE.md', 'CLAUDE.local.md'):
            if os.path.lexists(os.path.join(directory, name)):
                unwanted.append(os.path.join(directory, name))
    if unwanted:
        raise ValueError('remove user Claude Code configuration before evaluation: ' + ', '.join(unwanted))
    names = []
    synced = os.path.join(skills, 'synced')
    if os.path.isdir(synced):
        for child in sorted(os.listdir(synced)):
            manifest = os.path.join(synced, child, 'manifest.json')
            if not os.path.isfile(manifest):
                continue
            try:
                with open(manifest, encoding='utf-8') as f:
                    value = json.load(f)
                for skill in value['skills']:
                    name = skill['name']
                    if not isinstance(name, str):
                        raise ValueError('invalid skill name')
                    names.append(name)
            except (OSError, ValueError, KeyError, TypeError):
                raise ValueError('cannot inspect synced Skill manifest: %s' % child) from None
    return sorted(set(names))


def read_dsh_key(harness: Dict[str, object]) -> str:
    command = harness.get('api_key', {}).get('command')
    if not isinstance(command, list) or not command or not all(isinstance(part, str) and part for part in command):
        raise ValueError('DeepSeek api_key must reference a command')
    env, _removed = dsh_environment()
    code, out, _err = run_command(command, HERE, env)
    key = out.rstrip('\r\n')
    if code != 0 or not key:
        raise ValueError('cannot read the DeepSeek key from the keychain (exit %d)' % code)
    RECORD_SECRETS.append(key)
    return key


def redact_secrets(value: object) -> object:
    if isinstance(value, dict):
        return {redact_secrets(key): redact_secrets(item) for key, item in value.items()}
    if isinstance(value, list):
        return [redact_secrets(item) for item in value]
    if isinstance(value, str):
        for secret in RECORD_SECRETS:
            value = value.replace(secret, '<redacted-key>')
    return value


def dsh_key_leaks(trial_dir: str, key: str) -> Tuple[List[str], List[str]]:
    """試行全体を調べ、漏れと読めなかったパスを値を含めずに返す。"""
    needle = key.encode('utf-8')
    found = []
    unscannable = []
    def failed(error: OSError) -> None:
        unscannable.append(os.path.relpath(error.filename or trial_dir, trial_dir))
    for directory, dirs, files in os.walk(trial_dir, followlinks=False, onerror=failed):
        if directory == trial_dir:
            dirs[:] = [name for name in dirs if name not in ('bin', 'real')]
        for name in files:
            path = os.path.join(directory, name)
            relative = os.path.relpath(path, trial_dir)
            try:
                info = os.lstat(path)
                if not stat.S_ISREG(info.st_mode):
                    continue
                # lstat の後にリンクや FIFO に替えられても、辿ったり待ち続けたりしない。
                descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
                with os.fdopen(descriptor, 'rb') as f:
                    opened = os.fstat(f.fileno())
                    if not stat.S_ISREG(opened.st_mode):
                        continue
                    # 読んでいる間に追記されても、走査の開始時の大きさで打ち切る。
                    remaining = min(info.st_size, opened.st_size)
                    tail = b''
                    while remaining > 0:
                        chunk = f.read(min(65536, remaining))
                        if not chunk:
                            break
                        remaining -= len(chunk)
                        content = tail + chunk
                        if needle in content:
                            found.append(relative)
                            break
                        tail = content[-(len(needle) - 1):] if len(needle) > 1 else b''
            except OSError:
                unscannable.append(relative)
    return sorted(set(found)), sorted(set(unscannable))


def file_sha256(path: str) -> str:
    """処理系の写しを信用せず、判定の前に元の内容と比べるために使う。"""
    digest = hashlib.sha256()
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as f:
        info = os.fstat(f.fileno())
        if not stat.S_ISREG(info.st_mode):
            raise ValueError('processor must be a regular file')
        remaining = info.st_size
        while remaining > 0:
            chunk = f.read(min(1024 * 1024, remaining))
            if not chunk:
                break
            digest.update(chunk)
            remaining -= len(chunk)
    return digest.hexdigest()


def signal_group(process: subprocess.Popen, sig: int) -> None:
    try:
        os.killpg(process.pid, sig)
    except (ProcessLookupError, PermissionError):
        # macOS では、回収前のゾンビだけの群れも EPERM になる。
        pass


def stop_harness(process: subprocess.Popen, grace: float) -> None:
    signal_group(process, signal.SIGTERM)
    try:
        process.wait(timeout=grace)
    except subprocess.TimeoutExpired:
        pass
    finally:
        # 先頭が終わっていても、同じ群れに残った子を止める（P5-03 の補足 2）。
        signal_group(process, signal.SIGKILL)
        if process.poll() is None:
            try:
                process.kill()
            except ProcessLookupError:
                pass
        process.wait()


def run_harness(command: List[str], cwd: str, env: Dict[str, str], timeout: float,
                stdout_path: str, stderr_path: str, grace: float = TERM_GRACE,
                cleanup_group: bool = False) -> Tuple[int, bool, float]:
    """出力を直接残し、時間切れと中断では集計を書く猶予を与えて群れを止める。"""
    started = time.monotonic()
    with open(stdout_path, 'wb') as out, open(stderr_path, 'wb') as err:
        try:
            process = subprocess.Popen(command, cwd=cwd, env=env, stdout=out, stderr=err,
                                       start_new_session=True)
        except OSError as error:
            err.write(('cannot start %s: %s' % (command[0] if command else '', error)).encode('utf-8', 'replace'))
            return -2, False, 0.0
        timed_out = False
        pgid_path = os.path.join(os.path.dirname(stdout_path), 'harness.pgid')
        try:
            with open(pgid_path, 'w', encoding='utf-8') as f:
                f.write(str(process.pid) + '\n')
            try:
                process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                stop_harness(process, grace)
        except BaseException:
            # 別セッションの子には端末の Ctrl-C が届かないため、送り直す前に止める。
            stop_harness(process, grace)
            raise
        finally:
            if cleanup_group and not timed_out and process.poll() is not None:
                signal_group(process, signal.SIGKILL)
            if process.poll() is not None:
                try:
                    os.unlink(pgid_path)
                except FileNotFoundError:
                    pass
    return process.returncode, timed_out, round(time.monotonic() - started, 1)


def estimate_cost(row: Dict[str, object], harness: Dict[str, object]) -> Optional[float]:
    price = harness.get('price')
    keys = ('input_tokens', 'cache_read_tokens', 'cache_write_tokens', 'output_tokens')
    if not isinstance(price, dict) or any(row.get(key) is None for key in keys):
        return None
    uncached = max(0, row['input_tokens'] - row['cache_read_tokens'] - row['cache_write_tokens'])
    return (uncached * price['input'] + row['cache_read_tokens'] * price['cache_read']
            + row['cache_write_tokens'] * price['cache_write'] + row['output_tokens'] * price['output']) / 1000000


class CostBudget(object):
    """一回の道具の起動だけの累計を、項目をまたいで保つ。"""
    def __init__(self, limit: Optional[float]) -> None:
        self.limit = limit
        self.totals = {}  # type: Dict[str, float]
        self.message = ''

    def check(self, row: Dict[str, object]) -> str:
        """試行の途中の費用を、累計を書き換えずに判定する。"""
        billing = str(row.get('billing', 'none'))
        cost = row.get('cost_usd')
        if self.limit is not None and billing != 'none' and cost is None:
            return 'stopped: cost unknown for %s' % billing
        if (self.limit is not None and billing != 'none' and cost is not None
                and self.totals.get(billing, 0) + cost >= self.limit):
            return 'stopped: estimated cost reached %g USD for %s' % (self.limit, billing)
        return ''

    def add(self, row: Dict[str, object]) -> bool:
        self.message = self.check(row) or self.message
        billing, cost = str(row.get('billing', 'none')), row.get('cost_usd')
        if billing != 'none' and cost is not None:
            self.totals[billing] = self.totals.get(billing, 0) + cost
        return bool(self.message)


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
    def __init__(self, success: bool, reason: str, tests_passed: Optional[int] = None,
                 failure_kind: Optional[str] = None) -> None:
        self.success = success
        self.reason = reason
        self.tests_passed = tests_passed
        self.failure_kind = failure_kind
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
        # 本リポジトリには処理系のソースがないので、同じ版から取り出した一覧を使う。
        try:
            with open(os.path.join(HERE, 'warning-codes.txt'), encoding='utf-8') as f:
                return sorted(set(line.strip() for line in f if line.strip()))
        except OSError:
            return []
    return sorted(set(re.findall(r'^\s+(W[0-9]{4})\s', text, re.MULTILINE)))


def judge_environment(directory: str, http_base: Optional[str] = None) -> Dict[str, str]:
    """判定用の場所だけを設定し、親の環境や資格情報を引き継がない。"""
    directory = os.path.realpath(directory)
    env = {'PATH': '/usr/bin:/bin', 'PWD': directory}
    for variable, name in (('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'), ('XDG_STATE_HOME', 'state'),
                           ('XDG_CACHE_HOME', 'cache'), ('XDG_DATA_HOME', 'data'), ('TMPDIR', 'tmp')):
        env[variable] = os.path.join(directory, '.skill-eval-env', name)
        os.makedirs(env[variable], exist_ok=True)
    if http_base is not None:
        env[http_server.BASE_VARIABLE] = http_base
    return env


def judge_execute(task: Task, benitoite: str, directory: str, env: Dict[str, str],
                  allowed: set, command: List[str], arguments: Optional[List[str]] = None) -> Tuple[int, str, str]:
    """OS の保護が掛からなければ実行しない。直接の起動には同じエフェクトの許可も渡す。"""
    if sys.platform != 'darwin':
        raise ValueError('judgment requires the macOS sandbox')
    directory = os.path.realpath(directory)
    helpers = {'process_run': ['/usr/bin/sort'],
               'process_shell': ['/bin/sh', '/bin/bash', '/usr/bin/grep', '/usr/bin/cut', '/usr/bin/sort']}
    programs = [os.path.realpath(path) for path in
                [benitoite] + (helpers.get(task.name, []) if 'Process.Run' in allowed else [])]
    actual = [os.path.realpath(benitoite)] + command + ['--allow-effects=' + ','.join(sorted(allowed)), task.script]
    # 実行器はポートの例外から HTTP の URL を作り、親の環境は渡さない。
    actual += arguments or []
    port = loopback_port(task, allowed, env.get(http_server.BASE_VARIABLE))
    report = runner_execute(actual, directory, programs, port=port)
    if report['timed_out'] or report['stdout_truncated'] or report['stderr_truncated'] or report['signal'] is not None:
        raise ValueError(report.get('error') or 'judge command did not complete')
    return report['exit_code'] if report['exit_code'] is not None else -2, report['stdout'], report['stderr']


def judge_check(task: Task, benitoite: str, directory: str, env: Dict[str, str], allowed: set) -> Verdict:
    command = [benitoite, 'test', '--check'] if task.result_kind == 'test' else [benitoite, 'check']
    code, out, err = run_command(command + ['--report=json', task.script], directory, env)
    report = json_object(out) or {}
    diagnostics = report.get('diagnostics', [])
    if not isinstance(diagnostics, list):
        diagnostics = []
    codes = [item['code'] for item in diagnostics if isinstance(item, dict)
             and isinstance(item.get('code'), str)]
    messages = [item for item in diagnostics if isinstance(item, dict) and isinstance(item.get('message'), str)]
    errors = [item for item in messages if item.get('severity') == 'error']
    reason = (errors or messages)[0]['message'] if (errors or messages) else first_line(err)
    effects = declared_effects(code, out, 'testCheckReport' if task.result_kind == 'test' else 'checkReport')
    if code != 0:
        return with_codes(Verdict(False, 'check failed (exit %d): %s' % (code, judgment_excerpt(reason)),
                                  failure_kind='check'), codes)
    if effects is None:
        return with_codes(Verdict(False, 'invalid declared effects report', failure_kind='check'), codes)
    extra = effects - allowed
    if extra:
        return with_codes(Verdict(False, 'effects outside task allow: ' + ','.join(sorted(extra)),
                                  failure_kind='effects'), codes)
    return with_codes(Verdict(True, 'ok'), codes)


def judge(task: Task, script_text: str, benitoite: str, http_base: Optional[str],
          case_name: Optional[str] = None) -> Verdict:
    """書かれたスクリプトを、入力を写し直した新しいディレクトリで検査・実行し、期待と比べる。

    エージェントが作業ディレクトリの入力を書き換えていても判定が変わらないよう、作業ディレクトリでは実行しない。
    判定のための起動は包みのコマンドを通さないので、検査と修正の回数に数えない。
    """
    for text in task.require_text:
        if text not in script_text:
            return Verdict(False, 'the script does not contain `%s`' % text)
    try:
        allowed = set(filter(None, task_allow(task, benitoite).split(',')))
    except (OSError, ValueError) as error:
        return Verdict(False, 'task allow unavailable: %s' % error, failure_kind='effects')
    if task.kind == 'staged':
        cases = [case for case in task.judge_cases if case_name is None or case['name'] == case_name]
        if not cases:
            raise ValueError('unknown judge case: %s' % case_name)
        failures, codes, failure_kind = [], [], None
        for case in cases:
            with tempfile.TemporaryDirectory(prefix='skill-eval-case-') as directory:
                directory = os.path.realpath(directory)
                prepare_inputs(task, directory)
                copy_tree(task_path(task.dir, case['input'], True), directory)
                with open(os.path.join(directory, task.script), 'w', encoding='utf-8') as f:
                    f.write(script_text)
                env = judge_environment(directory, http_base)
                verdict = judge_check(task, benitoite, directory, env, allowed)
                codes.extend(verdict.final_codes)
                if verdict.success:
                    verdict = judge_run(task, benitoite, directory, env, allowed, case)
                if not verdict.success:
                    failures.append('%s: %s' % (case['name'], verdict.reason))
                    failure_kind = verdict.failure_kind
        return with_codes(Verdict(not failures, '; '.join(failures) if failures else 'ok',
                                  failure_kind=failure_kind), codes)
    directory = os.path.realpath(tempfile.mkdtemp(prefix='skill-eval-judge-'))
    try:
        prepare_inputs(task, directory)
        with open(os.path.join(directory, task.script), 'w', encoding='utf-8') as f:
            f.write(script_text)
        env = judge_environment(directory, http_base)
        checked = judge_check(task, benitoite, directory, env, allowed)
        if not checked.success:
            return checked
        if task.result_kind == 'test':
            verdict = judge_test(task, benitoite, directory, env, allowed)
            return with_codes(verdict, sorted(set(checked.final_codes + verdict.final_codes)))
        verdict = judge_run(task, benitoite, directory, env, allowed)
        return with_codes(verdict, checked.final_codes)
    finally:
        shutil.rmtree(directory, ignore_errors=True)


def with_codes(verdict: Verdict, codes: List[str]) -> Verdict:
    verdict.final_codes = codes
    return verdict


def judge_run(task: Task, benitoite: str, directory: str, env: Dict[str, str], allowed: set,
              case: Optional[Dict[str, object]] = None) -> Verdict:
    try:
        code, out, err = judge_execute(task, benitoite, directory, env, allowed, ['run'],
                                      case['args'] if case else [])
    except JudgeStopped:
        raise
    except (OSError, ValueError) as error:
        return Verdict(False, 'judgment sandbox unavailable: %s' % error, failure_kind='sandbox')
    if code != task.exit_code:
        return Verdict(False, 'run exited with %d, expected %d: %s' % (code, task.exit_code, judgment_excerpt(first_line(err))),
                       failure_kind='run')
    stdout = task_path(task.dir, case['expected_stdout']) if case else task.path('expected.stdout')
    with open(stdout, encoding='utf-8') as f:
        expected = f.read()
    if out != expected:
        return Verdict(False, 'standard output differs: %s' % first_difference(expected, out), failure_kind='stdout')
    expected_files = task_path(task.dir, case['expected_files'], True) if case else task.path('expected.files')
    if os.path.isdir(expected_files):
        for root, _dirs, files in os.walk(expected_files):
            for name in sorted(files):
                want = os.path.join(root, name)
                relative = os.path.relpath(want, expected_files)
                if not same_file(want, os.path.join(directory, relative)):
                    return Verdict(False, 'file %s differs or is missing' % relative, failure_kind='files')
    return Verdict(True, 'ok')


def judge_test(task: Task, benitoite: str, directory: str, env: Dict[str, str], allowed: set) -> Verdict:
    """テストを書く課題の判定。`benitoite test --diagnostics=json` の集計の行（06-04「結果の報告」）を読む。"""
    try:
        code, out, err = judge_execute(task, benitoite, directory, env, allowed, ['test', '--diagnostics=json'])
    except JudgeStopped:
        raise
    except (OSError, ValueError) as error:
        return Verdict(False, 'judgment sandbox unavailable: %s' % error, failure_kind='sandbox')
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
        return Verdict(False, 'no test summary (exit %d): %s' % (code, judgment_excerpt(first_line(err or out))))
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
            return 'line %d: expected "%s", got "%s"' % (index + 1, judgment_excerpt(a), judgment_excerpt(b))
    return 'trailing newline'


def judgment_excerpt(text: str) -> str:
    """判定の出力の抜粋を ASCII に逃がし、120文字以下にする（設計書 07-02「評価の手順」）。"""
    escaped = json.dumps(text[:120], ensure_ascii=True)[1:-1]
    return escaped[:117] + '...' if len(text) > 120 or len(escaped) > 120 else escaped


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


def valid_u64(value: object, positive: bool = False) -> bool:
    return type(value) is int and (1 if positive else 0) <= value <= 2**64 - 1


def load_harnesses() -> Dict[str, Dict[str, object]]:
    with open(HARNESSES_FILE, encoding='utf-8') as f:
        data = json.load(f)
    harnesses = {k: v for k, v in data.items() if not k.startswith('_')}
    for name, harness in harnesses.items():
        if 'mode' in harness and (harness.get('kind') != 'benitoiteh'
                                  or harness['mode'] not in ('restricted', 'normal')):
            raise ValueError('harness %s mode requires benitoiteh and restricted|normal' % name)
        if 'config' in harness:
            config = harness['config']
            allowed = {'restricted': {'after_write_check'}, 'read': {'line_numbers'},
                       'agent': {'completion_check'}, 'compaction': {'reserve_tokens', 'keep_tokens'}}
            if harness.get('kind') != 'benitoiteh' or not isinstance(config, dict):
                raise ValueError('harness %s config requires a benitoiteh table dictionary' % name)
            for table, values in config.items():
                if table not in allowed or not isinstance(values, dict):
                    raise ValueError('harness %s has an invalid config table' % name)
                if any(key not in allowed[table] or (not valid_u64(value) if table == 'compaction'
                                                     else type(value) is not bool)
                       for key, value in values.items()):
                    raise ValueError('harness %s has an invalid config key or value' % name)
        if harness.get('kind') == 'benitoiteh':
            provider = harness.get('provider', {})
            for key in ('context_window', 'max_output_tokens'):
                if key in provider and not valid_u64(provider[key], positive=True):
                    raise ValueError('harness %s provider.%s must be a positive u64' % (name, key))
        if ('price' in harness or (harness.get('kind') == 'benitoiteh' and not harness.get('mock'))):
            if not harness.get('billing'):
                raise ValueError('harness %s requires billing' % name)
    return harnesses


def render(template: List[str], values: Dict[str, str]) -> List[str]:
    return [part.format(**values) for part in template]


def version_of(command: List[str], env: Optional[Dict[str, str]] = None, cleanup_group: bool = False) -> str:
    if cleanup_group:
        # agy の背後の更新のプロセスが出力を持っていても、問い合わせを待ち続けない。
        with tempfile.TemporaryDirectory(prefix='agy-version-', dir=env.get('TMPDIR')) as directory:
            out, err = os.path.join(directory, 'stdout'), os.path.join(directory, 'stderr')
            code, timed_out, _wall = run_harness(command, directory, env, 60, out, err, cleanup_group=True)
            if code == -2 or timed_out:
                return 'unknown'
            with open(out, encoding='utf-8', errors='replace') as f:
                return first_line(f.read()) or 'unknown'
    try:
        completed = subprocess.run(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        return 'unknown'
    return first_line(completed.stdout.decode('utf-8', 'replace')) or 'unknown'


def judge_trial_script(task: Task, script_path: str, benitoite: str, http_base: Optional[str],
                       attempts: int) -> Verdict:
    if not os.path.isfile(script_path):
        return Verdict(False, '%s was not written' % task.script)
    if attempts > task.attempt_limit:
        return Verdict(False, '%d attempts exceed the limit %d' % (attempts, task.attempt_limit))
    with open(script_path, encoding='utf-8', errors='replace') as f:
        return judge(task, f.read(), benitoite, http_base)


def prepare_stage_prompt(task: Task, number: int) -> str:
    with open(task_path(task.dir, task.stages[number - 1]), encoding='utf-8') as f:
        return f.read()


def stage_materials(task: Task, number: int) -> List[str]:
    relative = 'materials/%d' % number
    directory = task_path(task.dir, relative, True)
    return sorted(os.path.relpath(os.path.join(root, name), task.dir)
                  for root, _dirs, files in os.walk(directory) for name in files)


def prepare_stage(task: Task, workspace: str, number: int) -> str:
    """後の段の資料は、前の段を終えてから写す。"""
    relative = 'materials/%d' % number
    task_path(task.dir, relative, True)
    target = workspace
    for part in relative.split('/'):
        target = os.path.join(target, part)
        os.makedirs(target, exist_ok=True)
        if not stat.S_ISDIR(os.lstat(target).st_mode):
            raise ValueError('workspace materials path is not a regular directory')
    for path in stage_materials(task, number):
        destination = os.path.join(workspace, path)
        parent = os.path.dirname(path)
        target = workspace
        for part in parent.split('/'):
            target = os.path.join(target, part)
            os.makedirs(target, exist_ok=True)
            if not stat.S_ISDIR(os.lstat(target).st_mode):
                raise ValueError('workspace materials path is not a regular directory')
        if os.path.lexists(destination) and not stat.S_ISREG(os.lstat(destination).st_mode):
            raise ValueError('workspace material is not a regular file')
        shutil.copyfile(task_path(task.dir, path), destination)
    body = prepare_stage_prompt(task, number)
    return (PROMPT.format(limit=task.attempt_limit) + '\n\n' if number == 1 else '') + body


def compaction_events(events: List[Dict[str, object]], stage: int) -> List[Dict[str, object]]:
    """超過は実行中の番、閾値・再開は次の番へ対応させる（P5-32 補足）。"""
    records, pending = [], []
    active = None
    for event in events:
        kind = event.get('type')
        if kind == 'turn_start':
            active = event.get('turn')
            for record in pending:
                record.update(turn=active, turn_reason=None)
            pending = []
        elif kind == 'turn_end':
            active = None
        elif kind == 'compaction':
            record = {key: event.get(key) for key in ('reason', 'summarized_items', 'kept_items',
                'kept_estimate', 'usage', 'estimate_before', 'estimate_after')}
            record.update(stage=stage, turn=None, turn_reason='no subsequent turn started')
            if record['reason'] == 'overflow':
                record.update(turn=active, turn_reason=None if active is not None else 'no active turn')
            else:
                pending.append(record)
            records.append(record)
    return records


def compaction_comparison(fields: Dict[str, object], harness: Dict[str, object], dry_run: bool) -> None:
    provider = harness.get('provider', {})
    window = provider.get('context_window', 200000)
    output = provider.get('max_output_tokens', 32000)
    config = harness.get('config', {}).get('compaction', {})
    reserve, keep = config.get('reserve_tokens', 16384), config.get('keep_tokens', 20000)
    effective = min(output + reserve, window // 2)
    fields.update(effective_reserve=effective, compaction_threshold=window - effective,
                  effective_keep=min(keep, window // 4))
    reasons = []
    events = fields['compaction_events']
    records = fields['compaction_records']
    keys = ('reason', 'summarized_items', 'kept_items', 'kept_estimate', 'usage')
    matches = [{key: event.get(key) for key in keys} for event in events] == records
    fields['compaction_records_match'] = matches and len(events) == fields.get('compactions')
    if dry_run:
        reasons.append('dry-run; no harness execution')
    if not fields['compaction_records_match']:
        reasons.append('compaction summary, events and session records differ')
    if not events:
        reasons.append('no compaction occurred')
    if reserve == 16384 and not any((event.get('kept_estimate') or 0) > 0 and (event.get('kept_items') or 0) > 0
               and event.get('stage', 0) > 1 for event in events):
        reasons.append('no nonzero retained range across requests')
    if min(output + 16384, window // 2) == min(output + 8192, window // 2):
        reasons.append('reserve settings produce the same effective reserve')
    if len(fields['stages']) != 3 or fields.get('end') != 'completed':
        reasons.append('three stages did not complete normally')
    if fields.get('repository_path_seen'):
        reasons.append('repository path seen in session')
    fields.update(comparison_valid=not reasons, comparison_reasons=reasons)


def run_stages(task: Task, trial_dir: str, workspace: str, executable: str,
               env: Dict[str, str], allow: str, harness: Dict[str, object],
               model: str, budget: Optional[CostBudget]) -> Tuple[Dict[str, object], str, str, int, bool, float]:
    """一つの試行を途中で切らず、同じ設定とセッションで三段を実行する。"""
    fields = dict.fromkeys(BH_FIELDS)
    counters = ('turns', 'attempts', 'after_write_checks', 'checks', 'fixes', 'policy_denied', 'compactions')
    fields.update({key: 0 for key in counters + TOKEN_FIELDS})
    fields.update(kind='benitoiteh', allow=allow, tool_calls={}, stages=[], exec_launches=0,
                  compaction_events=[], compaction_records=[], end='error', shell_output_seen=False)
    elapsed, final, reason, code, timed_out, identity = 0.0, '', '', 0, False, None
    # 実行器がより小さい上限を指定した場合も、試行全体に適用する。
    options = harness.get('options', {})
    max_turns = min(50, options.get('max_turns', 50))
    max_tokens = min(2000000, options.get('max_run_tokens', 2000000))
    max_time = min(1800, options.get('harness_timeout', 1800))
    identity_keys = ('session_id', 'provider', 'base_url', 'model', 'mode',
                     'system_prompt_sha256', 'tools_sha256', 'compaction', 'completion_check')
    config = harness.get('config', {})
    expected_compaction = dict(reserve_tokens=16384, keep_tokens=20000)
    expected_compaction.update(config.get('compaction', {}))
    budget = budget or CostBudget(None)
    for number in range(1, 4):
        cost = estimate_cost(fields, harness)
        billing = harness.get('billing', 'none')
        stop = budget.check(dict(fields, cost_usd=cost, billing=billing))
        if stop:
            fields.update(budget_stop=stop, end='cost_limit')
            reason = stop
            break
        turns, tokens, seconds = max_turns - fields['turns'], max_tokens - (fields['total_tokens'] or 0), max_time - elapsed
        if turns <= 0 or tokens <= 0 or seconds <= 0:
            fields['end'] = 'max_turns' if turns <= 0 else 'max_tokens' if tokens <= 0 else 'timeout'
            timed_out = seconds <= 0
            reason = 'trial limit reached before stage %d' % number
            break
        try:
            prompt = prepare_stage(task, workspace, number)
        except (OSError, ValueError) as error:
            fields['end'], reason = 'error', 'stage materials unavailable: %s' % type(error).__name__
            break
        command = [executable, 'exec']
        if number > 1:
            command += ['--resume', fields['session_id']]
        command += ['--mode', 'restricted', '--provider', 'eval']
        if allow:
            command += ['--allow', allow]
        command += ['--max-turns', str(turns), '--max-run-tokens', str(tokens), '--json', prompt]
        stdout = os.path.join(trial_dir, 'stage-%d.stdout' % number)
        stderr = os.path.join(trial_dir, 'stage-%d.stderr' % number)
        code, timed_out, wall = run_harness(command, workspace, env, seconds, stdout, stderr)
        elapsed += wall
        fields['exec_launches'] += 1
        with open(stdout, encoding='utf-8', errors='replace') as f:
            out = f.read()
        with open(stderr, encoding='utf-8', errors='replace') as f:
            stderr_first = first_line(f.read())
        try:
            stage, reason, final = benitoiteh_output(out, code)
        except (ValueError, TypeError, KeyError) as error:
            stage = dict.fromkeys(BH_FIELDS)
            stage.update(end='error', attempts=0, tool_calls={})
            reason, final = 'unreadable stage summary: %s' % type(error).__name__, ''
        stage.update(stage=number, harness_exit=code, timed_out=timed_out, wall_seconds=wall,
                     reason=reason,
                     limits=dict(turns=turns, tokens=tokens, seconds=seconds),
                     cost_usd=estimate_cost(stage, harness), stdout=os.path.basename(stdout), stderr=os.path.basename(stderr))
        fields['stages'].append(stage)
        events = [json_object(line) for line in out.splitlines() if line.strip()]
        valid_events = [event for event in events if event is not None]
        fields['compaction_events'].extend(compaction_events(valid_events, number))
        for key in counters + TOKEN_FIELDS:
            value = stage.get(key)
            if not valid_u64(value):
                fields[key] = None
            elif fields.get(key) is not None:
                fields[key] += value
        for key, count in (stage.get('tool_calls') or {}).items():
            fields['tool_calls'][key] = fields['tool_calls'].get(key, 0) + count
        fields['end'] = 'timeout' if timed_out else stage['end']
        starts = [event for event in valid_events if event.get('type') == 'session_start']
        current = {key: starts[0].get(key) for key in identity_keys} if starts else None
        stage['session_start'] = current
        if identity is None and current is not None:
            identity = current
            for key in ('session_id', 'benitoiteh_version', 'system_prompt_sha256', 'tools_sha256'):
                fields[key] = stage.get(key)
        problem = ''
        if code == -2:
            problem = stderr_first or 'harness could not be started'
        elif not starts or len(starts) != 1 or any(event is None for event in events):
            problem = 'invalid stage event stream or session_start'
        elif (not isinstance(current['session_id'], str)
              or not re.fullmatch(r'[A-Za-z0-9_-]+', current['session_id'])
              or starts[0].get('resumed', False) is not (number > 1) or current != identity):
            problem = 'session identity or resume mismatch'
        elif (current['provider'] != 'eval' or current['mode'] != 'restricted' or current['model'] != model
              or not all(isinstance(current[key], str) and current[key]
                         for key in ('system_prompt_sha256', 'tools_sha256'))):
            problem = 'unexpected destination, mode or missing prompt hashes'
        elif (current['compaction'] != expected_compaction
              or current['completion_check'] is not config.get('agent', {}).get('completion_check', False)):
            problem = 'effective compaction or completion-check settings differ'
        elif any(not valid_u64(stage.get(key)) for key in ('turns', 'total_tokens', 'compactions')):
            problem = 'stage summary has unknown limits'
        cost = estimate_cost(fields, harness)
        stop = ('stopped: cost unknown for %s' % billing if billing != 'none' and cost is None
                else budget.check(dict(fields, cost_usd=cost, billing=billing)))
        if stop:
            fields['budget_stop'] = stop
        stage.update(runner_error=problem or None, budget_stop=stop or None)
        if problem:
            fields['end'], reason = 'error', problem
        if problem or code != 0 or timed_out or fields['end'] != 'completed' or fields.get('budget_stop'):
            if code != 0 and fields['end'] == 'completed':
                fields['end'], reason = 'error', 'stage exited with %d' % code
            if fields.get('budget_stop') and fields['end'] == 'completed':
                fields['end'] = 'cost_limit'
                reason = fields['budget_stop']
            break
    return fields, reason, final, code, timed_out, elapsed


def normal_environment(workspace: str) -> Tuple[Dict[str, str], List[str]]:
    allowed = {'HOME', 'PATH', 'LANG', 'LC_ALL', 'TERM', 'SHELL', 'USER', 'LOGNAME', 'TZ'}
    env = {key: value for key, value in os.environ.items() if key in allowed}
    env['PATH'] = os.pathsep.join(path for path in env.get('PATH', '/usr/bin:/bin').split(os.pathsep)
                                  if path and not repository_path_seen(path) and os.path.realpath(path) != WRAPPER_DIR)
    env, removed = benitoiteh_environment(env)
    env['PWD'] = workspace
    return env, sorted(set(removed + [key for key in os.environ if key not in env]))


def run_normal_trial(task: Task, trial_dir: str, benitoite: str, harness_name: str, harness: dict,
                     model: str, dry_run: bool, benitoiteh: Optional[str]) -> dict:
    if not dry_run and (harness.get('kind') != 'benitoiteh' or harness.get('mode', 'restricted') != 'normal'):
        raise ValueError('normal tasks require a normal benitoiteh harness')
    workspace = os.path.join(trial_dir, 'workspace')
    prepare_inputs(task, workspace)
    shutil.copyfile(task.path('task.md'), os.path.join(workspace, 'task.md'))
    real_dir = os.path.join(trial_dir, 'real')
    os.mkdir(real_dir)
    processor = os.path.join(real_dir, 'benitoite')
    shutil.copy2(benitoite, processor)
    digest = file_sha256(processor)
    env, removed = normal_environment(workspace)
    for variable, name in (('XDG_CONFIG_HOME', 'bh/config'), ('XDG_STATE_HOME', 'bh/state'), ('TMPDIR', 'tmp')):
        env[variable] = os.path.join(trial_dir, name)
        os.makedirs(env[variable])
    server = None
    fields = dict.fromkeys(BH_FIELDS)
    fields.update(attempts=None, checks=None, fixes=None, after_write_checks=None)
    statuses, reason = [], ''
    try:
        if dry_run:
            for path in task.artifacts:
                shutil.copyfile(task_path(task.dir, 'reference/' + path), os.path.join(workspace, path))
            harness_exit, timed_out, wall_seconds = 0, False, None
            fields['end'] = 'dry-run'
        else:
            port = None
            if harness.get('mock'):
                artifact = task.artifacts[0]
                with open(task_path(task.dir, 'reference/' + artifact), encoding='utf-8') as stream:
                    server, _base = mock_provider.start('normal', artifact, stream.read(),
                                                       verify='python3 -m unittest discover -s tests')
                port = server.server_address[1]
                env['SKILL_EVAL_MOCK_KEY'] = 'skill-eval-synthetic-key'
            provider_key = harness.get('provider', {}).get('api_key', {}).get('env')
            if provider_key and provider_key in os.environ:
                env[provider_key] = os.environ[provider_key]
            config_dir = os.path.join(env['XDG_CONFIG_HOME'], 'benitoiteh')
            os.mkdir(config_dir)
            with open(os.path.join(config_dir, 'config.toml'), 'w', encoding='utf-8') as stream:
                stream.write(benitoiteh_config(harness, model, processor, port))
            options = harness['options']
            command = [benitoiteh or find_benitoiteh(None), 'exec', '--mode', 'normal', '--approval', 'never',
                       '--provider', 'eval', '--max-turns', str(options['max_turns']),
                       '--max-run-tokens', str(options['max_run_tokens']), '--json', NORMAL_PROMPT]
            stdout_path, stderr_path = os.path.join(trial_dir, 'harness.stdout'), os.path.join(trial_dir, 'harness.stderr')
            harness_exit, timed_out, wall_seconds = run_harness(
                command, workspace, env, options.get('harness_timeout', HARNESS_TIMEOUT), stdout_path, stderr_path)
            with open(stdout_path, encoding='utf-8', errors='replace') as stream:
                out = stream.read()
            fields, reason, final = benitoiteh_output(out, harness_exit)
            fields.update(attempts=None, checks=None, fixes=None, after_write_checks=None)
            statuses = bash_executions(out)
            with open(os.path.join(trial_dir, 'final.txt'), 'w', encoding='utf-8') as stream:
                stream.write(final)
            if timed_out:
                fields['end'] = 'timeout'
        verdict = judge_normal(task, workspace)
        missing = not dry_run and not statuses
        success = verdict['artifact_success'] and not missing
        if not dry_run and (fields['end'] != 'completed' or harness_exit != 0):
            success = False
            verdict['reason'] += '; ' + reason
        if file_sha256(processor) != digest:
            success = False
            verdict['reason'] += '; processor copy modified'
        if missing:
            verdict['reason'] += '; verification_missing'
    finally:
        if server is not None:
            server.shutdown()
            server.server_close()
    if server is not None and server.problems:
        success = False
        verdict['reason'] += '; mock provider: ' + '; '.join(server.problems)
    row = dict(task=task.name, suite='normal', category=task.category, harness=harness_name, model=model,
               success=success, verification_missing=missing, bash_calls=len(statuses), bash_exit_codes=statuses,
               harness_exit=harness_exit, timed_out=timed_out, wall_seconds=wall_seconds, trial_dir=trial_dir,
               skill=None, log_exists=False, diagnostics=diagnostic_stats([]), final_codes=[], tests_passed=None,
               billing=harness.get('billing', 'none'), cost_usd=None if dry_run else estimate_cost(fields, harness),
               environment_removed_paths=removed, tool_identities=NORMAL_TOOLS, dry_run=dry_run)
    row.update(verdict)
    row.update(fields)
    row['success'] = success
    return row


def run_trial(task: Task, trial_dir: str, benitoite: str, harness_name: str, harness: Dict[str, object],
              model: str, skill_dir: Optional[str], dry_run: bool,
              benitoiteh: Optional[str] = None, dsh_key: Optional[str] = None,
              work_root: Optional[str] = None, budget: Optional[CostBudget] = None) -> Dict[str, object]:
    """一回の試行。作業ディレクトリ（workspace）と、エージェントに見せない記録（benitoite.log など）を分けて置く。"""
    global JUDGE_WORKSPACE
    if task.kind == 'normal':
        return run_normal_trial(task, trial_dir, benitoite, harness_name, harness, model, dry_run, benitoiteh)
    workspace = os.path.join(trial_dir, 'workspace')
    JUDGE_WORKSPACE = workspace
    log = os.path.join(trial_dir, 'benitoite.log')
    is_dsh = harness.get('prepare') == 'dsh' and not dry_run
    is_agy = harness.get('prepare') == 'agy' and not dry_run
    output = harness.get('output') if not dry_run else None
    synced_skills = None
    builtin_skills = check_agy_config(workspace, work_root) if is_agy else None
    user_skills_before = user_skill_state()
    if is_dsh:
        check_dsh_workspace(workspace)
        overlay = dsh_overlay(model, harness.get('options', {}).get('effort', 'high'))
        if not dsh_key:
            raise ValueError('DeepSeek key must be read before starting trials')
    if output == 'claude-json':
        synced_skills = check_claude_config(workspace)
    # 包みのコマンドと本物の処理系は、試行のディレクトリ（リポジトリの外）に写して使う。リポジトリの中のパスを
    # PATH や環境変数に出すと、エージェントがそこから解のスクリプトや設計書に辿り着けるため（06-06）
    tools_dir = os.path.join(trial_dir, 'bin')
    real_dir = os.path.join(trial_dir, 'real')
    os.makedirs(tools_dir)
    os.makedirs(real_dir)
    wrapper_copy = os.path.join(tools_dir, 'benitoite')
    shutil.copy2(os.path.join(WRAPPER_DIR, 'benitoite'), wrapper_copy)
    real_copy = os.path.join(real_dir, 'benitoite')
    original_benitoite = benitoite
    original_digest = file_sha256(original_benitoite)
    shutil.copy2(benitoite, real_copy)
    benitoite = real_copy
    prepare_inputs(task, workspace)
    shutil.copyfile(task.path('task.md'), os.path.join(workspace, 'task.md'))
    skill_path = install_skill(benitoite, harness, workspace, skill_dir)
    for name, text in dict(harness.get('files', {})).items():  # type: ignore
        with open(os.path.join(workspace, name), 'w', encoding='utf-8') as f:
            f.write(str(text))
    env, _removed = (dsh_environment() if is_dsh else agy_environment() if is_agy else harness_environment())
    if is_agy:
        prepare_agy(trial_dir, workspace)
        env['TMPDIR'] = os.path.join(trial_dir, 'tmp')
        os.makedirs(env['TMPDIR'])
        agy_before = agy_integrity(trial_dir, workspace)
        protected_before = agy_protected_entries(workspace)
    if is_dsh:
        for variable, name in (('DSH_HOME', 'dsh-home'), ('DSH_AGENTS_HOME', 'agents-home'), ('TMPDIR', 'tmp')):
            env[variable] = os.path.join(trial_dir, name)
            os.makedirs(env[variable])
        env['DSH_TELEMETRY_DISABLED'] = '1'
        log = os.path.join(env['TMPDIR'], 'benitoite.log')
        with open(os.path.join(trial_dir, 'dsh-eval-overlay.yml'), 'w', encoding='utf-8') as f:
            f.write(overlay)
    env['PATH'] = tools_dir + os.pathsep + env.get('PATH', '')
    env['SKILL_EVAL_LOG'] = log
    env['SKILL_EVAL_REAL_BENITOITE'] = benitoite
    is_benitoiteh = harness.get('kind') == 'benitoiteh' and not dry_run
    staged = task.kind == 'staged'
    if staged and not dry_run and not is_benitoiteh:
        raise ValueError('staged tasks require a benitoiteh harness')
    fields = dict.fromkeys(BH_FIELDS)
    allow = task_allow(task, benitoite) if is_benitoiteh else None
    if is_benitoiteh:
        env, removed_paths = benitoiteh_environment(env)
        fields.update(kind='benitoiteh', allow=allow)
    server = None
    mock = None
    http_base = None
    permission_errors = []
    try:
        if task.http:
            server, http_base = http_server.start()
            env[http_server.BASE_VARIABLE] = http_base
        if is_benitoiteh:
            for variable, relative in (('XDG_CONFIG_HOME', 'bh/config'), ('XDG_STATE_HOME', 'bh/state'),
                                       ('TMPDIR', 'tmp')):
                env[variable] = os.path.join(trial_dir, relative)
                os.makedirs(env[variable])
            port = None
            if harness.get('mock'):
                with open(task.path('reference.bnt'), encoding='utf-8') as f:
                    mock, _base = mock_provider.start(
                        task.kind, task.script, f.read(),
                        completion_check=harness.get('config', {}).get('agent', {}).get('completion_check', False),
                        stages=[{'prompt': prepare_stage_prompt(task, number),
                                 'materials': stage_materials(task, number)} for number in range(1, 4)]
                        if staged else None,
                        input_tokens=harness.get('mock_input_tokens', 1000))
                port = mock.server_address[1]
                env['SKILL_EVAL_MOCK_KEY'] = 'skill-eval-synthetic-key'
            config = benitoiteh_config(harness, model, benitoite, port)
            config_dir = os.path.join(env['XDG_CONFIG_HOME'], 'benitoiteh')
            os.makedirs(config_dir)
            with open(os.path.join(config_dir, 'config.toml'), 'w', encoding='utf-8') as f:
                f.write(config)
        if dry_run:
            if staged:
                for number in range(1, 4):
                    prepare_stage(task, workspace, number)
            # ハーネスの代わりに解のスクリプトを置き、エージェントと同じく包みのコマンドで一度検査する
            shutil.copyfile(task.path('reference.bnt'), os.path.join(workspace, task.script))
            command = ['benitoite', 'test' if task.result_kind == 'test' else 'check', task.script]
            run_command(command, workspace, env)
            harness_exit = 0
            err = ''
            timed_out, wall_seconds = False, None
        elif staged:
            fields, end_reason, final, harness_exit, timed_out, wall_seconds = run_stages(
                task, trial_dir, workspace, benitoiteh or find_benitoiteh(None), env, allow, harness, model, budget)
            err = fields['stages'][-1].get('runner_error') or '' if fields['stages'] else ''
        else:
            if is_benitoiteh:
                options = harness['options']
                command = [benitoiteh or find_benitoiteh(None), 'exec', '--mode', 'restricted', '--provider', 'eval']
                if allow:
                    command += ['--allow', allow]
                command += ['--max-turns', str(options['max_turns']),
                            '--max-run-tokens', str(options['max_run_tokens']), '--json',
                            PROMPT.format(limit=task.attempt_limit)]
                cwd = workspace
            else:
                values = dict(harness.get('options', {}))  # type: ignore
                values.update(workspace=workspace, trial=trial_dir, model=model,
                              prompt=PROMPT.format(limit=task.attempt_limit))
                command = render(list(harness['start']), values)  # type: ignore
                cwd = str(harness.get('cwd', '{workspace}')).format(**values)
            stdout_path = os.path.join(trial_dir, 'harness.stdout')
            stderr_path = os.path.join(trial_dir, 'harness.stderr')
            if is_dsh:
                env['DEEPSEEK_API_KEY'] = dsh_key
            cleanup = {'cleanup_group': True} if is_agy else {}
            if is_agy:
                settings_before = agy_user_settings_state()
                harness_version_start = version_of(list(harness['version']), env, cleanup_group=True)
            try:
                if is_agy:
                    permission_errors.extend(agy_permissions(workspace, False))
                harness_exit, timed_out, wall_seconds = run_harness(
                    command, cwd, env, harness.get('options', {}).get('harness_timeout', HARNESS_TIMEOUT),
                    stdout_path, stderr_path, **cleanup)
            finally:
                if is_agy:
                    permission_errors.extend(agy_permissions(workspace, True))
                    harness_version_end = version_of(list(harness['version']), env, cleanup_group=True)
                    try:
                        settings_changed = settings_before != agy_user_settings_state()
                    except ValueError:
                        settings_changed = True
            if is_dsh:
                del env['DEEPSEEK_API_KEY']
        entries = read_log(log)
        attempts = count_attempts(entries)
        if not staged or dry_run:
            end_reason = ''
        no_summary = False
        unreadable = ''
        if not staged or dry_run:
            final = None
        try:
            if not dry_run and not staged:
                with open(stdout_path, encoding='utf-8', errors='replace') as f:
                    out = f.read()
                with open(stderr_path, encoding='utf-8', errors='replace') as f:
                    err = f.read()
            if is_benitoiteh:
                if not staged:
                    fields, end_reason, final = benitoiteh_output(out, harness_exit)
                fields['allow'] = allow
                attempts = fields['attempts']
                no_summary = end_reason.startswith('benitoiteh produced no summary ')
                session_id = fields['session_id']
                entries = []
                if session_id:
                    sessions = os.path.join(env['XDG_STATE_HOME'], 'benitoiteh', 'sessions')
                    directory = os.path.join(sessions, str(session_id))
                    if os.path.dirname(os.path.realpath(directory)) != os.path.realpath(sessions):
                        raise ValueError('invalid session_id')
                    entries, seen = benitoiteh_results(directory, fields)
                    fields['shell_output_seen'] = seen if task.name == 'process_shell' else False
            elif output:
                external, end_reason, final = external_output(output, out, harness_exit, err)
                fields.update(external)
        except (OSError, ValueError, KeyError, TypeError) as error:
            if output:
                fields.update(dict.fromkeys(TOKEN_FIELDS + ('end',)))
                end_reason, final = 'harness summary unreadable', ''
            else:
                unreadable = 'unreadable session record: %s' % type(error).__name__
        if (is_benitoiteh or output) and final is not None:
            with open(os.path.join(trial_dir, 'final.txt'), 'w', encoding='utf-8') as f:
                f.write(final)
        if timed_out:
            fields['end'] = 'timeout'
        if is_benitoiteh:
            fields['comparison_excluded'] = bool(fields.get('repository_path_seen'))
        if staged:
            if dry_run:
                fields.update(stages=[], exec_launches=0, compactions=0, compaction_events=[], compaction_records=[])
            compaction_comparison(fields, harness, dry_run)
            if mock is not None:
                fields['mock_summary_requests'] = mock.summary_requests
        script_path = os.path.join(workspace, task.script)
        try:
            processor_modified = file_sha256(real_copy) != original_digest
        except (OSError, ValueError):
            processor_modified = True
        if processor_modified:
            verdict = Verdict(False, 'SAFETY: the processor copy was modified')
        elif unreadable:
            verdict = Verdict(False, unreadable)
        elif no_summary:
            verdict = Verdict(False, end_reason)
        elif harness_exit == -2:
            verdict = Verdict(False, 'the harness could not be started: %s' % first_line(err))
        elif not is_benitoiteh and (not os.path.isfile(log) or attempts == 0):
            # 包みのコマンドを通らずに処理系を使った（迂回した、サンドボックスが記録を書けない）試行を、成功と記録しない
            verdict = Verdict(False, 'WARNING: no counted benitoite command in the wrapper log')
        else:
            verdict = judge_trial_script(task, script_path, original_benitoite, http_base, attempts)
        if staged:
            fields['artifact_success'] = verdict.success
            if not dry_run and (len(fields['stages']) != 3 or fields['end'] != 'completed'
                                or unreadable or not fields['compaction_records_match']):
                verdict.success = False
                verdict.reason += '; staged trial incomplete or records inconsistent'
        if is_benitoiteh and not unreadable and not no_summary and fields['end'] != 'completed':
            verdict.reason += '; %s: %s' % (fields['end'], end_reason)
        if output:
            if output == 'agy-json':
                if end_reason:
                    verdict.reason += '; ' + end_reason
                if fields['end'] not in (None, 'completed'):
                    verdict.reason += '; ' + fields['end']
            elif end_reason == 'harness summary unreadable':
                verdict.reason += '; harness summary unreadable'
            elif fields['end'] != 'completed':
                verdict.reason += '; %s: %s' % (fields['end'], end_reason)
            elif fields.get('subagent_calls'):
                verdict.reason += '; usage excludes subagents'
            fields['over_token_cap'] = (fields.get('total_tokens') or 0) > 2000000
            if fields['over_token_cap']:
                verdict.reason += '; over the 2M token cap'
        if is_agy:
            gate_calls, command_allowed = agy_gate_records(trial_dir)
            calls = fields.get('tool_calls')
            # 門の失敗時も道具が通るため、総数で相殺せず道具ごとの不足を足す（P5-07 補足 3）。
            changes = []
            if calls is None:
                changes.append('SAFETY: gate coverage unverifiable')
            else:
                missing = sum(max(0, count - gate_calls.get(name, 0)) for name, count in calls.items())
                if missing:
                    changes.append('SAFETY: tool calls without a gate decision (%d)' % missing)
            agy_after = agy_integrity(trial_dir, workspace)
            changes += ['SAFETY: %s was modified' % name for name, before in agy_before.items()
                        if before != agy_after.get(name)]
            created = {}
            for name in agy_protected_entries(workspace) - protected_before:
                marker = os.path.basename(name).lower()
                if marker in agy_gate.PROTECTED_FILES:
                    marker = marker.upper().replace('.MD', '.md')
                elif marker not in agy_gate.PROTECTED_DIRS:
                    changes.append('WARNING: workspace inspection incomplete')
                    continue
                created[marker] = created.get(marker, 0) + 1
            changes += ['SAFETY: %s was created (%d)' % (name, count) for name, count in sorted(created.items())]
            if (calls and not gate_calls) or (entries and not command_allowed):
                changes.append('SAFETY: hooks were not loaded')
            if permission_errors:
                changes.append('WARNING: agy permission changes failed: %d' % len(permission_errors))
            if changes:
                verdict.reason = '; '.join(changes) + '; ' + verdict.reason
        if is_dsh:
            leaks, unscannable = dsh_key_leaks(trial_dir, dsh_key)
            if leaks:
                verdict.reason = ('; '.join('SAFETY: DeepSeek key value found in ' + path for path in leaks)
                                  + '; ' + verdict.reason)
                print('SAFETY: DeepSeek key value found in trial files. Rotate the DeepSeek API key.', file=sys.stderr)
            if unscannable:
                verdict.reason += '; ' + '; '.join('SAFETY: unscannable ' + path for path in unscannable)
            verdict.reason = redact_secrets(verdict.reason)
        if fields['shell_output_seen']:
            verdict.reason = 'SAFETY: shell output seen; ' + verdict.reason
        if fields.get('repository_path_seen'):
            verdict.reason = 'WARNING: repository path seen in session; ' + verdict.reason
        if is_benitoiteh:
            fields['environment_removed_paths'] = removed_paths
        if user_skill_state() != user_skills_before:
            verdict.reason = 'SAFETY: user skill directory changed; ' + verdict.reason
        if is_agy:
            if settings_changed:
                verdict.reason = 'SAFETY: user Antigravity settings changed; ' + verdict.reason
            if harness_version_start != harness_version_end:
                verdict.reason += '; WARNING: harness version changed %s -> %s' % (
                    harness_version_start, harness_version_end)
    finally:
        if mock is not None:
            mock.shutdown()
            mock.server_close()
        if server is not None:
            server.shutdown()
            server.server_close()
    if mock is not None and mock.problems:
        verdict.success = False
        verdict.reason += '; mock provider: ' + '; '.join(mock.problems)
    row = {
        'task': task.name, 'success': verdict.success, 'reason': verdict.reason, 'attempts': attempts,
        'log_exists': os.path.isfile(log), 'skill': os.path.relpath(skill_path, workspace),
        'tests_passed': verdict.tests_passed, 'harness_exit': harness_exit, 'trial_dir': trial_dir,
        'diagnostics': diagnostic_stats(entries), 'final_codes': verdict.final_codes, 'harness': harness_name,
        'model': model, 'timed_out': timed_out, 'wall_seconds': wall_seconds,
        'cost_usd': estimate_cost(fields, harness) if (is_benitoiteh or output)
        and not fields.get('usage_missing') and not fields.get('subagent_calls') else None,
        'billing': harness.get('billing', 'none'),
    }
    row.update(fields)
    if synced_skills is not None:
        row.update(claude_config_checked=True, claude_synced_skills=synced_skills)
    if builtin_skills is not None:
        row.update(agy_config_checked=True, agy_builtin_skills=builtin_skills,
                   agy_settings_changed=settings_changed, harness_version_start=harness_version_start,
                   harness_version_end=harness_version_end)
    return row


def self_test(tasks: List[Task], benitoite: str) -> int:
    """解のスクリプトが期待する出力を出すか、受け入れテストと同じ入力か、包みのコマンドが働くかを確かめる。"""
    if tasks and all(task.kind == 'normal' for task in tasks):
        return self_test_normal(tasks)
    failures = 0
    for task in tasks:
        problems = []
        skipped = []
        for relative, source in sorted(task.acceptance.items()):
            source_path = os.path.join(REPO, source)
            # 写した課題の元は本リポジトリに含まれない（設計書 07-02「評価の課題」）。
            if not os.path.exists(source_path):
                skipped.append('acceptance skipped: %s' % source)
            elif not same_file(source_path, task.path(relative)):
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
        for failure in task.self_test_failures:
            with open(task_path(task.dir, failure['script']), encoding='utf-8') as f:
                negative = judge(task, f.read(), benitoite, http_base, failure['case'])
            if negative.success:
                problems.append('%s unexpectedly passed case %s' % (failure['script'], failure['case']))
            elif negative.failure_kind not in ('stdout', 'files'):
                problems.append('%s is not a valid negative control: %s' % (failure['script'], negative.reason))
        failures += 1 if problems else 0
        result = 'ok' if not problems else 'FAILED: ' + '; '.join(problems)
        if skipped:
            result += ' (%s)' % '; '.join(skipped)
        print('%-16s %s' % (task.name, result))
    wrapper_problem = check_wrapper(benitoite)
    print('%-16s %s' % ('(wrapper)', wrapper_problem or 'ok'))
    failures += 1 if wrapper_problem else 0
    print('self-test: %d of %d failed' % (failures, len(tasks) + 1))
    return 1 if failures else 0


def self_test_normal(tasks: List[Task]) -> int:
    failures = 0
    for task in tasks:
        problems = []
        with tempfile.TemporaryDirectory(prefix='ns-', dir='/tmp') as workspace:
            workspace = os.path.realpath(workspace)
            prepare_inputs(task, workspace)
            shutil.copyfile(task.path('task.md'), os.path.join(workspace, 'task.md'))
            initial = judge_normal(task, workspace)
            target = task.normal_self_test['initial_case']
            cases = [case for result in initial['judge_results'] for case in (result['cases'] or [])]
            if initial['artifact_success'] or not any(case['name'] == target and not case['success'] for case in cases):
                problems.append('initial input did not fail the intended case ' + target)
            for path in task.artifacts:
                shutil.copyfile(task_path(task.dir, 'reference/' + path), os.path.join(workspace, path))
            positive = judge_normal(task, workspace)
            if not positive['artifact_success']:
                problems.append('reference failed: ' + positive['reason'])
            path = os.path.join(workspace, task.artifacts[0])
            controls = [('fixed-output', task.normal_self_test['fixed_output'],
                         task.normal_self_test['fixed_output_cases'], False)]
            for control in task.normal_self_test['controls']:
                with open(task_path(task.dir, control['source']), encoding='utf-8') as stream:
                    controls.append((control['source'], stream.read(), control['cases'], True))
            for label, content, expected, require_mismatch in controls:
                with open(path, 'w', encoding='utf-8') as stream:
                    stream.write(content)
                negative = judge_normal(task, workspace)
                cases = [case for result in negative['judge_results'] for case in (result['cases'] or [])]
                failed = {case['name'] for case in cases if not case['success']}
                # 全ケースで落ちる実行不能な対照を、指定した条件の退行として採らない。
                if failed != set(expected):
                    problems.append('%s failure cases %s differ from %s' % (label, sorted(failed), sorted(expected)))
                if not any(case['success'] for case in cases):
                    problems.append(label + ' did not pass any case')
                if require_mismatch and any(not case['success']
                        and case['reason'] != 'output, file contents or exit status differs' for case in cases):
                    problems.append(label + ' failed without a behavioral mismatch')
        failures += bool(problems)
        print('%-24s %s' % (task.name, 'FAILED: ' + '; '.join(problems) if problems else 'ok'))
    print('self-test: %d of %d failed' % (failures, len(tasks)))
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
        lines += ['### %s（%s）' % (harness, model), '']
        if any(r.get('kind') == 'benitoiteh' for r in mine) and not any(r['diagnostics']['counts'] for r in mine):
            lines += ['no diagnostic codes were found in benitoiteh tool results', '']
        lines += code_table(mine) + ['']
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


def path_prefixes(root: str) -> List[str]:
    """macOS の別名も集め、出力側のパスの綴りに依存させない。"""
    paths = set()
    if root:
        for path in (os.path.abspath(root), os.path.realpath(root)):
            if path == os.path.sep:
                continue
            paths.add(path)
            for public in ('/var', '/tmp', '/etc'):
                private = '/private' + public
                if path == private or path.startswith(private + '/'):
                    paths.add(path[len('/private'):])
                elif path == public or path.startswith(public + '/'):
                    paths.add('/private' + path)
    return sorted(paths, key=len, reverse=True)


def redact_record(value: object, work_root: Optional[str]) -> object:
    """保存する写しだけを再帰して置き換え、試行後に使う実のパスを保つ。"""
    prefixes = {}
    home = os.environ.get('HOME', '')
    username = os.path.basename(os.path.normpath(home)) if home else ''
    for root, replacement in ((tempfile.gettempdir(), '<tmp>'), (home, '~'),
                              (REPO, '<repo>'), (work_root, '<work-root>')):
        if root:
            for path in path_prefixes(root):
                prefixes[path] = replacement
    def replace(item: object) -> object:
        if isinstance(item, str):
            for prefix in sorted(prefixes, key=len, reverse=True):
                pattern = re.escape(prefix) + (r'(?![A-Za-z0-9._-])' if prefixes[prefix] == '~' else '')
                item = re.sub(pattern, lambda match: prefixes[prefix], item)
            if username and username not in ('.', '..', os.path.sep):
                item = re.sub(r'(?<=/)' + re.escape(username) + r'(?![A-Za-z0-9._-])', '<user>', item)
            return item
        if isinstance(item, dict):
            return {replace(key): replace(part) for key, part in item.items()}
        if isinstance(item, (list, tuple)):
            return [replace(part) for part in item]
        return item
    return replace(value)


def record_value(value: object, pattern: str = '%s') -> str:
    return '-' if value is None else pattern % value


def write_record(path: str, header: Dict[str, object], rows: List[Dict[str, object]], tasks: List[Task],
                 work_root: Optional[str] = None) -> None:
    """公開する記録だけにパスの置換を掛け、生の対話は含めない。"""
    header = redact_secrets(redact_record(header, work_root))
    rows = redact_secrets(redact_record(rows, work_root))
    lines = ['# Skill の評価の記録', '']
    for row in rows:
        if row.get('shell_output_seen'):
            lines.append('SAFETY: shell output seen in %s trial %s' %
                         (row['task'], os.path.basename(str(row['trial_dir']))))
    for key, value in header.items():
        lines.append('- %s: %s' % (key, value))
    unknown = sum(1 for row in rows if row.get('billing', 'none') != 'none' and row.get('cost_usd') is None)
    if unknown:
        lines.append('- cost unknown: %d trials' % unknown)
    normal_rows = [row for row in rows if row.get('suite') == 'normal']
    if normal_rows:
        lines += ['', '## 通常モード', '', '| 種別 | 成功 / 試行 |', '|---|---|']
        for category in sorted(set(row['category'] for row in normal_rows)):
            mine = [row for row in normal_rows if row['category'] == category]
            lines.append('| %s | %d / %d |' % (category, sum(row['success'] for row in mine), len(mine)))
        lines += ['', '| 課題 | 成果物 | 確認不足 | bash の実行数 | 終了状態 | 変更したファイル | 判定 |',
                  '|---|---|---|---|---|---|---|']
        for row in normal_rows:
            cells = [row['task'], row['artifact_success'], row['verification_missing'], row['bash_calls'],
                     row['bash_exit_codes'], ', '.join(row['changed_files']),
                     ', '.join('%s=%s' % (result['name'], result['success']) for result in row['judge_results'])]
            lines.append('| ' + ' | '.join(str(cell).replace('|', '\\|') for cell in cells) + ' |')
    lines += ['', '| 課題 | 成功 / 試行 | 成功した試行の検査と修正の回数 | 時間の中央値 (秒) | トークンの中央値 | 上限で停止 |',
              '|---|---|---|---|---|---|']
    for task in tasks:
        mine = [r for r in rows if r['task'] == task.name]
        wins = [r for r in mine if r['success']]
        counts = ', '.join(str(r['attempts']) for r in wins if r.get('attempts') is not None) or '-'
        medians = []
        for key, pattern in (('wall_seconds', '%.1f'), ('total_tokens', '%g')):
            values = [r[key] for r in wins if r.get(key) is not None]
            medians.append(record_value(statistics.median(values) if values else None, pattern))
        limits = sum(1 for r in mine if r.get('end') in ('max_turns', 'max_tokens', 'timeout'))
        lines.append('| %s | %d / %d | %s | %s | %s | %d |' %
                     (task.name, len(wins), len(mine), counts, medians[0], medians[1], limits))
    costs = [r['cost_usd'] for r in rows if r.get('cost_usd') is not None]
    lines += ['', '費用の見積もりの合計: %s USD（費用を取得できた試行の合計）' %
              record_value(sum(costs) if costs else None, '%.6f'), '', '## 試行ごとの結果', '',
              '| 課題 | 試行 | 成功 | 番 | 検査と修正 (書いた後の検査) | トークン (キャッシュ読み取り) | 時間 (秒) | 終わり方 | 費用 (USD) | 理由 |',
              '|---|---|---|---|---|---|---|---|---|---|']
    for row in rows:
        attempts = record_value(row.get('attempts'))
        if row.get('kind') == 'benitoiteh':
            attempts += ' (%s)' % record_value(row.get('after_write_checks'))
        tokens = record_value(row.get('total_tokens'))
        if row.get('total_tokens') is not None:
            tokens += ' (%s)' % record_value(row.get('cache_read_tokens'))
        cells = [row['task'], os.path.basename(str(row['trial_dir'])), 'yes' if row['success'] else 'no',
                 record_value(row.get('turns')), attempts, tokens, record_value(row.get('wall_seconds'), '%.1f'),
                 record_value(row.get('end')), record_value(row.get('cost_usd'), '%.6f'), row['reason']]
        lines.append('| ' + ' | '.join(str(cell).replace('|', '\\|').replace('\n', ' ') for cell in cells) + ' |')
    staged_rows = [row for row in rows if 'stages' in row]
    if staged_rows:
        lines += ['', '## 圧縮の比較', '',
                  'exec の起動の合計: %d' % sum(row['exec_launches'] for row in staged_rows), '',
                  '| 課題 | 試行 | 成果物の合格 | 成功した圧縮 | 記録との一致 | 比較の成立 | 理由 |',
                  '|---|---|---|---|---|---|---|']
        for row in staged_rows:
            cells = [row['task'], os.path.basename(str(row['trial_dir'])), row.get('artifact_success'),
                     row.get('compactions'), row.get('compaction_records_match'), row.get('comparison_valid'),
                     '; '.join(row.get('comparison_reasons', []))]
            lines.append('| ' + ' | '.join(str(cell).replace('|', '\\|') for cell in cells) + ' |')
        lines += ['', '| 課題 | 試行 | 段 | 番 | トークン | 時間 (秒) | 費用 (USD) | 終わり方 |',
                  '|---|---|---|---|---|---|---|---|']
        for row in staged_rows:
            for stage in row['stages']:
                cells = [row['task'], os.path.basename(str(row['trial_dir'])), stage['stage'],
                         record_value(stage.get('turns')), record_value(stage.get('total_tokens')),
                         record_value(stage.get('wall_seconds'), '%.1f'), record_value(stage.get('cost_usd'), '%.6f'),
                         stage.get('end')]
                lines.append('| ' + ' | '.join(str(cell).replace('|', '\\|') for cell in cells) + ' |')
    lines += diagnostics_section(rows)
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, 'w', encoding='utf-8') as f:
        f.write(redact_secrets('\n'.join(lines)) + '\n')
    with open(os.path.splitext(path)[0] + '.jsonl', 'w', encoding='utf-8') as f:
        for row in rows:
            f.write(redact_secrets(json.dumps(row, ensure_ascii=False)) + '\n')


def interrupt_evaluation(signum: int, frame: object) -> None:
    """端末以外からの終了も、子を止めて記録を残す中断へ揃える。"""
    raise KeyboardInterrupt()


def main(argv: Optional[List[str]] = None) -> int:
    previous = {}
    try:
        for sig in (signal.SIGTERM, signal.SIGHUP):
            previous[sig] = signal.signal(sig, interrupt_evaluation)
        return evaluate_main(argv)
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)


def evaluate_main(argv: Optional[List[str]] = None) -> int:
    global JUDGE_RUNNER, JUDGE_WORKSPACE, NORMAL_TOOLS
    parser = argparse.ArgumentParser(description='Evaluate the bundled Benitoite Agent Skill.')
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--self-test', action='store_true', help='run the reference scripts (no LLM)')
    mode.add_argument('--dry-run', action='store_true', help='prepare and judge without a harness (no LLM)')
    mode.add_argument('--harness', help='harness names in harnesses.json, comma-separated, or `default` '
                      'for the default set (calls an LLM)')
    parser.add_argument('--task', action='append', default=[], help='task name (repeatable; default: all)')
    parser.add_argument('--suite', choices=('benitoite', 'compaction', 'normal'), default='benitoite',
                        help='task suite (default: the original 11 Benitoite tasks)')
    parser.add_argument('--interleave', action='store_true',
                        help='run all conditions per task/trial, rotating their order each trial')
    parser.add_argument('--trials', type=int, default=1, help='trials per task')
    parser.add_argument('--model', help='model passed to the harness (default: harnesses.json)')
    parser.add_argument('--agent', default='codex', help='harness whose Skill location --dry-run uses')
    parser.add_argument('--skill-dir', help='Skill directory to use instead of `benitoite skill install`')
    parser.add_argument('--benitoite', help='path of the real benitoite (default: target/release, target/debug, PATH)')
    parser.add_argument('--benitoiteh', help='path of benitoiteh (default: newest target/release or target/debug)')
    parser.add_argument('--judge-runner', help='path of the built skill_eval_judge example (macOS only)')
    parser.add_argument('--work-root', help='directory for workspaces and raw logs (default: a new temporary one)')
    parser.add_argument('--allow-unverified', action='store_true',
                        help='start a harness whose settings are not verified (harnesses.json "verified": false)')
    parser.add_argument('--record', help='record file (default: results/<date>-<harness>.md)')
    parser.add_argument('--stop-cost', type=float, help='stop after estimated USD per billing provider')
    args = parser.parse_args(argv)
    if args.stop_cost is not None and (not 0 <= args.stop_cost < float('inf')):
        parser.error('--stop-cost must be a finite nonnegative amount')
    ALL_ROWS.clear()
    RECORD_SECRETS.clear()
    JUDGE_ROOTS.clear()
    JUDGE_WORKSPACE = None
    NORMAL_TOOLS = {}

    try:
        tasks = load_tasks(args.task, args.suite)
    except (OSError, ValueError) as error:
        raise SystemExit('invalid task: %s' % error) from None
    benitoite = find_benitoite(args.benitoite)
    if args.self_test:
        prepare_judgment(args.judge_runner, tasks)
        try:
            return self_test(tasks, benitoite)
        finally:
            problem = check_judge_processes()
            if problem:
                raise SystemExit(problem)

    try:
        harnesses = load_harnesses()
    except ValueError as error:
        raise SystemExit(str(error))
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
            raise SystemExit('the settings of harness %s are not verified; check tools/skill-eval/'
                             'SOURCE.md with the designer and pass --allow-unverified' % name)
        if args.suite == 'compaction' and not args.dry_run and harnesses[name].get('kind') != 'benitoiteh':
            raise SystemExit('compaction suite requires benitoiteh harnesses')
        if args.suite == 'normal' and not args.dry_run and (
                harnesses[name].get('kind') != 'benitoiteh' or harnesses[name].get('mode', 'restricted') != 'normal'):
            raise SystemExit('normal tasks require a normal benitoiteh harness')
        if args.suite != 'normal' and harnesses[name].get('mode', 'restricted') == 'normal':
            raise SystemExit('normal harness requires --suite normal')
    if len(names) > 1 and (args.record or args.model):
        raise SystemExit('--record and --model need a single harness')
    if args.interleave and len(names) < 2:
        parser.error('--interleave requires multiple harnesses')
    skill_dir = os.path.abspath(args.skill_dir) if args.skill_dir else None
    if not args.dry_run and any(harnesses[name].get('kind') == 'benitoiteh' for name in names):
        args.benitoiteh = find_benitoiteh(args.benitoiteh)
    work_root = os.path.realpath(args.work_root or tempfile.mkdtemp(prefix='skill-eval-'))
    if any(os.path.commonpath([work_root, root]) == root for root in ('/tmp', '/private/tmp')):
        raise SystemExit('--work-root must not be under /tmp (harnesses can write there)')
    if os.path.commonpath([work_root, os.path.realpath(REPO)]) == os.path.realpath(REPO):
        raise SystemExit('--work-root must be outside the repository (raw logs are not kept in it)')
    prepare_judgment(args.judge_runner, tasks)
    # 利用者が指定した別名も保存時の置換に含める（macOS の /var と /private/var など）。
    args.work_root = os.path.abspath(args.work_root) if args.work_root else work_root
    budget = CostBudget(args.stop_cost)
    headers = {}
    launches = []
    status = 0
    try:
        status = evaluate(args, tasks, benitoite, names, harnesses, skill_dir, work_root, budget, headers, launches)
    finally:
        if len(names) > 1 and (ALL_ROWS or (args.interleave and launches)):
            # 中断でも、それまでに終わった項目を全体の記録に残す。
            directory = work_root if args.dry_run else RESULTS_DIR
            record = os.path.join(directory, '%s-overall.md' % datetime.datetime.now().strftime('%Y-%m-%d-%H%M'))
            header = {'ハーネス': ', '.join(names), '生の記録': work_root, '項目': headers}
            if args.interleave:
                header['起動順'] = launches
            if budget.message:
                header['停止'] = budget.message
            write_record(record, header, ALL_ROWS, tasks, args.work_root)
            print('overall record: %s' % record)
    return status


def evaluation_header(args: argparse.Namespace, benitoite: str, harness_name: str,
                      harness: Dict[str, object], skill_dir: Optional[str], work_root: str,
                      rows: List[Dict[str, object]]) -> Dict[str, object]:
    first = rows[0]
    first_trial = str(first['trial_dir'])
    first_skill = os.path.join(first_trial, 'workspace', str(harness['skill_parent']), 'benitoite')
    is_dsh = harness.get('prepare') == 'dsh'
    is_agy = harness.get('prepare') == 'agy'
    version_env, removed = (dsh_environment() if is_dsh else agy_environment() if is_agy else harness_environment())
    if args.dry_run:
        harness_version = 'not started (dry-run)'
    elif harness.get('kind') == 'benitoiteh':
        version_env, removed_paths = benitoiteh_environment(version_env)
        removed = sorted(set(removed + removed_paths))
        version_env.update(XDG_CONFIG_HOME=os.path.join(first_trial, 'bh/config'),
                           XDG_STATE_HOME=os.path.join(first_trial, 'bh/state'),
                           TMPDIR=os.path.join(first_trial, 'tmp'))
        # 版の問い合わせも、普段の設定に触れない環境で行う。
        for variable in ('XDG_CONFIG_HOME', 'XDG_STATE_HOME', 'TMPDIR'):
            os.makedirs(version_env[variable], exist_ok=True)
        harness_version = first.get('benitoiteh_version') or version_of([args.benitoiteh, '--version'], version_env)
    else:
        if is_dsh:
            version_env.update(DSH_HOME=os.path.join(first_trial, 'dsh-home'),
                               DSH_AGENTS_HOME=os.path.join(first_trial, 'agents-home'),
                               DSH_TELEMETRY_DISABLED='1', TMPDIR=os.path.join(first_trial, 'tmp'))
        if is_agy:
            # 更新後の版を見出しに使わず、最初の試行の開始時に得た版を使う。
            harness_version = first['harness_version_start']
        else:
            harness_version = version_of(list(harness['version']), version_env)
    with open(benitoite, 'rb') as f:
        processor_hash = hashlib.sha256(f.read()).hexdigest()
    options = harness.get('options', {})
    header = {
        '日時': datetime.datetime.now().strftime('%Y-%m-%d %H:%M'),
        '処理系': version_of([benitoite, '--version']), '処理系 SHA-256': processor_hash,
        'Skill': 'not installed (normal suite)' if first.get('suite') == 'normal' else (
            ('%s（--skill-dir %s）' % (skill_version(first_skill), args.skill_dir)) if skill_dir
            else skill_version(first_skill)),
        'ハーネス': '%s（%s）' % (harness_name, harness_version),
        '提供者': harness.get('provider', {}), 'モデル': args.model or str(harness['model']),
        'effort': harness.get('effort', options.get('effort', options.get('reasoning_effort', '-'))),
        'config': json.dumps(harness['config'], sort_keys=True) if 'config' in harness else '-',
        'max_turns': options.get('max_turns', '-'), 'max_run_tokens': options.get('max_run_tokens', '-'),
        'HARNESS_TIMEOUT': options.get('harness_timeout', HARNESS_TIMEOUT),
        '除いた環境変数': ', '.join(removed) or '-',
        'CLAUDE_CONFIG_DIR': 'retained' if 'CLAUDE_CONFIG_DIR' in version_env else 'not set',
        '試行の回数': str(args.trials), '生の記録': work_root + '（リポジトリの外）',
    }
    if harness.get('kind') == 'benitoiteh':
        header['completion_check_sha256'] = hashlib.sha256(completion_check_prompt().encode('utf-8')).hexdigest()
        for key in ('benitoiteh_version', 'system_prompt_sha256', 'tools_sha256'):
            header[key] = record_value(first.get(key))
    if is_dsh:
        header['使用量の範囲'] = 'parent session only; usage excludes subagents'
    if harness.get('output') == 'claude-json' and not args.dry_run:
        header['Claude Code の利用者設定の確認'] = 'passed' if first.get('claude_config_checked') else 'not checked'
        header['Claude Code の同期された Skill'] = first.get('claude_synced_skills', [])
    if is_agy and not args.dry_run:
        header['Antigravity の利用者設定の確認'] = 'passed' if first.get('agy_config_checked') else 'not checked'
        header['Antigravity の同梱の Skill'] = first.get('agy_builtin_skills', [])
    return header


def evaluate(args: argparse.Namespace, tasks: List[Task], benitoite: str, names: List[str],
             harnesses: Dict[str, Dict[str, object]], skill_dir: Optional[str], work_root: str,
             budget: CostBudget, headers: Dict[str, object], launches: List[Dict[str, object]]) -> int:
    """全条件で費用を共有し、中断時も条件ごとの完了済みの試行を保存する。"""
    rows_by_name = {name: [] for name in names}
    roots = {name: os.path.join(work_root, name) if len(names) > 1 else work_root for name in names}
    keys = {}
    if args.interleave:
        groups = ((task, trial) for task in tasks for trial in range(1, args.trials + 1))
        plan = ((name, task, trial, position) for group, (task, trial) in enumerate(groups)
                for position, name in enumerate(names[group % len(names):] + names[:group % len(names)], 1))
    else:
        plan = ((name, task, trial, position) for position, name in enumerate(names, 1)
                for task in tasks for trial in range(1, args.trials + 1))
    try:
        for name, task, trial, position in plan:
            harness_name, harness = name, harnesses[name]
            model = args.model or str(harness['model'])
            work_root = roots[name]
            rows = rows_by_name[name]
            if name not in keys:
                keys[name] = None
                if harness.get('prepare') == 'dsh' and not args.dry_run:
                    try:
                        check_dsh_workspace(work_root)
                        dsh_overlay(model, harness.get('options', {}).get('effort', 'high'))
                        keys[name] = read_dsh_key(harness)
                    except ValueError as error:
                        raise SystemExit(str(error)) from None
            dsh_key = keys[name]
            number = trial
            while os.path.exists(os.path.join(work_root, task.name, str(number))):
                number += args.trials
            trial_dir = os.path.join(work_root, task.name, str(number))
            os.makedirs(trial_dir)
            launch = dict(harness=name, task=task.name, repetition=trial,
                          condition_order=len(launches) + 1, condition_position=position, completed=False)
            launches.append(launch)
            try:
                extra = {'dsh_key': dsh_key} if dsh_key is not None else {}
                if harness.get('prepare') == 'agy':
                    extra['work_root'] = work_root
                if task.kind == 'staged':
                    extra['budget'] = budget
                row = run_trial(task, trial_dir, benitoite, harness_name, harness, model, skill_dir, args.dry_run,
                                args.benitoiteh, **extra)
            except ValueError as error:
                raise SystemExit(str(error))
            row['repetition'] = trial
            row['condition_order'] = launch['condition_order']
            row['condition_position'] = position
            launch['completed'] = True
            row['billing'] = harness.get('billing', 'none')
            rows.append(row)
            ALL_ROWS.append(row)
            print('%s %-16s trial %d: %s, attempts %s, %s' % (
                harness_name, task.name, trial, 'success' if row['success'] else 'failure', row['attempts'],
                judgment_excerpt(row['reason'])), flush=True)
            budget.add(row)
            if row.get('budget_stop'):
                budget.message = row['budget_stop']
            if harness.get('prepare') == 'agy' and not args.dry_run:
                stops = []
                if row['agy_settings_changed']:
                    stops.append('user Antigravity settings changed')
                if row['harness_version_start'] != row['harness_version_end']:
                    stops.append('harness version changed %s -> %s' % (
                        row['harness_version_start'], row['harness_version_end']))
                if stops:
                    # 費用の上限と同じく、後続の項目も止めて完了済みの記録を残す。
                    budget.message = 'stopped: ' + '; '.join(stops)
            if budget.message:
                print(budget.message, file=sys.stderr)
                return 3
            if len(rows) == len(tasks) * args.trials:
                wins = sum(1 for row in rows if row['success'])
                print('%s: %d of %d trials succeeded; workspaces in %s' % (name, wins, len(rows), roots[name]))
        if args.dry_run:
            bad = [r for r in ALL_ROWS if not (r['success'] and (r.get('suite') == 'normal'
                         or (r['log_exists'] and r['attempts'] >= 1)))]
            return 1 if bad else 0
        return 0
    finally:
        process_problem = check_judge_processes()
        if process_problem:
            budget.message = process_problem
        for name, rows in rows_by_name.items():
            if not rows:
                continue
            harness, root = harnesses[name], roots[name]
            header = evaluation_header(args, benitoite, name, harness, skill_dir, root, rows)
            header['条件の順'] = 'interleave: rotate per task/repetition group' if args.interleave else 'sequential'
            if args.dry_run:
                header['種類'] = 'dry-run'
            if budget.message:
                header['停止'] = budget.message
            headers[name] = header
            record = (os.path.join(root, 'record.md') if args.dry_run else args.record or os.path.join(
                RESULTS_DIR, '%s-%s.md' % (datetime.datetime.now().strftime('%Y-%m-%d-%H%M'), name)))
            write_record(record, header, rows, tasks, args.work_root)
            print('record: %s' % record)
        if process_problem:
            raise SystemExit(process_problem)


if __name__ == '__main__':
    sys.exit(main())
