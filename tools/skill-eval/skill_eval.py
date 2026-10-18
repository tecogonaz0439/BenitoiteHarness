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
             'cache_read_tokens', 'cache_write_tokens', 'output_tokens', 'total_tokens', 'end', 'shell_output_seen')
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
    provider.update(model=model, effort=harness['effort'], context_window=200000)
    # 一度全体を検証し、途中まで書かれた設定を起動に使わない。
    toml_value(provider)
    lines = ['default_mode = "restricted"', 'default_provider = "eval"', '', '[providers.eval]']
    lines += ['%s = %s' % (key, toml_value(value)) for key, value in provider.items()]
    lines += ['', '[benitoite]', 'path = ' + toml_value(processor), '', '[restricted]',
              'allowed_programs = ["/usr/bin/sort"]']
    config = harness.get('config', {})
    lines += ['%s = %s' % (key, toml_value(value))
              for key, value in sorted(config.get('restricted', {}).items())]
    if 'read' in config:
        lines += ['', '[read]']
        lines += ['%s = %s' % (key, toml_value(value))
                  for key, value in sorted(config['read'].items())]
    lines.append('')
    return '\n'.join(lines)


def task_allow(task: Task, benitoite: str) -> str:
    """解の報告が確定しているときだけ、非対話の許可の集合を取り出す。"""
    with tempfile.TemporaryDirectory(prefix='skill-eval-effects-') as directory:
        shutil.copyfile(task.path('reference.bnt'), os.path.join(directory, task.script))
        command = [benitoite, 'test', '--check'] if task.kind == 'test' else [benitoite, 'check']
        code, out, err = run_command(command + ['--report=json', task.script], directory, dict(os.environ))
    try:
        report = json.loads(out)
    except ValueError:
        report = None
    if isinstance(report, dict):
        aggregate = report.get('aggregateEffects')
        if (code == 0 and type(report.get('schemaVersion')) is int and report['schemaVersion'] == 1
                and report.get('status') == 'ok' and report.get('complete') is True
                and isinstance(aggregate, dict) and aggregate.get('basis') == 'declaredUpperBound'):
            expanded = aggregate.get('expanded')
            if isinstance(expanded, list) and all(isinstance(effect, str) and effect for effect in expanded):
                return ','.join(sorted(expanded))
    raise ValueError('cannot determine --allow for %s (exit %d): %s' % (task.name, code, first_line(err or out)))


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


def benitoiteh_results(directory: str) -> Tuple[List[List[str]], bool]:
    """記録の結果を既存の診断の集計に渡す。退避した出力は安全の検査だけに使う。"""
    path = os.path.join(directory, 'session.jsonl')
    if not os.path.isfile(path):
        return [], False
    entries = []
    calls = {}
    shell_seen = False
    with open(path, encoding='utf-8', errors='replace') as f:
        for line in f:
            value = json_object(line)
            if value is not None and value.get('type') == 'spilled':
                if not isinstance(value['parts'], list):
                    raise TypeError('parts must be a list')
                # UTF-8 の文字の途中で分かれた部分も、バイト列を繋いでから復元する。
                restored = b''.join(session_file(directory, part) for part in value['parts'])
                value = json_object(restored.decode('utf-8', 'replace'))
            if value is None:
                continue
            if value.get('type') == 'tool_call':
                call = value['call']
                if not isinstance(call, dict) or not isinstance(call.get('arguments', {}), dict):
                    raise TypeError('invalid tool call')
                calls[call['id']] = call.get('arguments', {})
            if value.get('type') != 'tool_result':
                continue
            name = value.get('name')
            result = value['result']
            if not isinstance(result, dict):
                raise TypeError('invalid tool result')
            text = result['text']
            if name in PROCESS_TOOLS:
                arguments = calls.get(result['id'], {})
                entries.append(['', '', '', ','.join(codes_in(text)), PROCESS_TOOLS[name],
                                arguments.get('path', '')])
                shell_seen = shell_seen or bool(SHELL_OUTPUT.search(text))
                for key in ('spill', 'stderr_spill'):
                    spill = value.get(key)
                    if spill is not None:
                        if not isinstance(spill, dict):
                            raise TypeError('invalid spill')
                        saved = session_file(directory, spill['path']).decode('utf-8', 'replace')
                        shell_seen = bool(SHELL_OUTPUT.search(saved)) or shell_seen
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


def harness_environment() -> Tuple[Dict[str, str], List[str]]:
    """親の対話とキーを評価の対象に渡さない（設計書 07-02「評価の手順」）。"""
    keys = {'DEEPSEEK_API_KEY', 'GEMINI_API_KEY', 'GOOGLE_API_KEY', 'OPENAI_API_KEY'}
    removed = sorted(name for name in os.environ if name != 'CLAUDE_CONFIG_DIR'
                     and (name.startswith(('CLAUDE', 'ANTHROPIC')) or name in keys))
    return {name: value for name, value in os.environ.items() if name not in removed}, removed


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
        {'type': 'command', 'command': command, 'timeout': 10}]}]}}
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


def agy_gate_records(trial: str) -> Tuple[bool, bool]:
    """会話の記録が欠けても、包みの起動に対応する門の許可があるかを調べる。"""
    try:
        descriptor = os.open(os.path.join(trial, 'agy', 'gate.log'),
                             os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(descriptor, encoding='utf-8', errors='replace') as f:
            info = os.fstat(f.fileno())
            if not stat.S_ISREG(info.st_mode):
                return False, False
            # 別の群れに残った子が追記していても、読み取りの開始時の量で打ち切る。
            values = [json_object(line) for line in f.read(info.st_size).splitlines()]
        records = [value for value in values if value is not None and value.get('decision') in ('allow', 'deny')]
        return bool(records), any(value.get('tool') == 'run_command' and value.get('decision') == 'allow'
                                  for value in records)
    except (OSError, ValueError):
        return False, False


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

    def add(self, row: Dict[str, object]) -> bool:
        billing = str(row.get('billing', 'none'))
        cost = row.get('cost_usd')
        if self.limit is not None and billing != 'none' and cost is None:
            self.message = 'stopped: cost unknown for %s' % billing
        if billing != 'none' and cost is not None:
            self.totals[billing] = self.totals.get(billing, 0) + cost
            if self.limit is not None and self.totals[billing] >= self.limit:
                self.message = 'stopped: estimated cost reached %g USD for %s' % (self.limit, billing)
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
        # 本リポジトリには処理系のソースがないので、同じ版から取り出した一覧を使う。
        try:
            with open(os.path.join(HERE, 'warning-codes.txt'), encoding='utf-8') as f:
                return sorted(set(line.strip() for line in f if line.strip()))
        except OSError:
            return []
    return sorted(set(re.findall(r'^\s+(W[0-9]{4})\s', text, re.MULTILINE)))


def judge(task: Task, script_text: str, benitoite: str, http_base: Optional[str]) -> Verdict:
    """書かれたスクリプトを、入力を写し直した新しいディレクトリで検査・実行し、期待と比べる。

    エージェントが作業ディレクトリの入力を書き換えていても判定が変わらないよう、作業ディレクトリでは実行しない。
    判定のための起動は包みのコマンドを通さないので、検査と修正の回数に数えない。
    """
    env, _removed = harness_environment()
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
    harnesses = {k: v for k, v in data.items() if not k.startswith('_')}
    for name, harness in harnesses.items():
        if 'config' in harness:
            config = harness['config']
            allowed = {'restricted': {'after_write_check'}, 'read': {'line_numbers'}}
            if harness.get('kind') != 'benitoiteh' or not isinstance(config, dict):
                raise ValueError('harness %s config requires a benitoiteh table dictionary' % name)
            for table, values in config.items():
                if table not in allowed or not isinstance(values, dict):
                    raise ValueError('harness %s has an invalid config table' % name)
                if any(key not in allowed[table] or type(value) is not bool
                       for key, value in values.items()):
                    raise ValueError('harness %s has an invalid config key or non-boolean value' % name)
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


def run_trial(task: Task, trial_dir: str, benitoite: str, harness_name: str, harness: Dict[str, object],
              model: str, skill_dir: Optional[str], dry_run: bool,
              benitoiteh: Optional[str] = None, dsh_key: Optional[str] = None,
              work_root: Optional[str] = None) -> Dict[str, object]:
    """一回の試行。作業ディレクトリ（workspace）と、エージェントに見せない記録（benitoite.log など）を分けて置く。"""
    workspace = os.path.join(trial_dir, 'workspace')
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
    fields = dict.fromkeys(BH_FIELDS)
    allow = task_allow(task, benitoite) if is_benitoiteh else None
    if is_benitoiteh:
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
                    mock, _base = mock_provider.start(task.kind, task.script, f.read())
                port = mock.server_address[1]
                env['SKILL_EVAL_MOCK_KEY'] = 'skill-eval-synthetic-key'
            config = benitoiteh_config(harness, model, benitoite, port)
            config_dir = os.path.join(env['XDG_CONFIG_HOME'], 'benitoiteh')
            os.makedirs(config_dir)
            with open(os.path.join(config_dir, 'config.toml'), 'w', encoding='utf-8') as f:
                f.write(config)
        if dry_run:
            # ハーネスの代わりに解のスクリプトを置き、エージェントと同じく包みのコマンドで一度検査する
            shutil.copyfile(task.path('reference.bnt'), os.path.join(workspace, task.script))
            command = ['benitoite', 'test' if task.kind == 'test' else 'check', task.script]
            run_command(command, workspace, env)
            harness_exit = 0
            err = ''
            timed_out, wall_seconds = False, None
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
        end_reason = ''
        no_summary = False
        unreadable = ''
        final = None
        try:
            if not dry_run:
                with open(stdout_path, encoding='utf-8', errors='replace') as f:
                    out = f.read()
                with open(stderr_path, encoding='utf-8', errors='replace') as f:
                    err = f.read()
            if is_benitoiteh:
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
                    entries, seen = benitoiteh_results(directory)
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
            agy_after = agy_integrity(trial_dir, workspace)
            changes = ['SAFETY: %s was modified' % name for name, before in agy_before.items()
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
            log_loaded, command_allowed = agy_gate_records(trial_dir)
            if (fields.get('tool_calls') and not log_loaded) or (entries and not command_allowed):
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
    lines += ['', '| 課題 | 成功 / 試行 | 成功した試行の検査と修正の回数 | 時間の中央値 (秒) | トークンの中央値 | 上限で停止 |',
              '|---|---|---|---|---|---|']
    for task in tasks:
        mine = [r for r in rows if r['task'] == task.name]
        wins = [r for r in mine if r['success']]
        counts = ', '.join(str(r['attempts']) for r in wins) or '-'
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
    parser.add_argument('--benitoiteh', help='path of benitoiteh (default: newest target/release or target/debug)')
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

    tasks = load_tasks(args.task)
    benitoite = find_benitoite(args.benitoite)
    if args.self_test:
        return self_test(tasks, benitoite)

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
    if len(names) > 1 and (args.record or args.model):
        raise SystemExit('--record and --model need a single harness')
    skill_dir = os.path.abspath(args.skill_dir) if args.skill_dir else None
    if not args.dry_run and any(harnesses[name].get('kind') == 'benitoiteh' for name in names):
        args.benitoiteh = find_benitoiteh(args.benitoiteh)
    work_root = os.path.realpath(args.work_root or tempfile.mkdtemp(prefix='skill-eval-'))
    if any(os.path.commonpath([work_root, root]) == root for root in ('/tmp', '/private/tmp')):
        raise SystemExit('--work-root must not be under /tmp (harnesses can write there)')
    if os.path.commonpath([work_root, os.path.realpath(REPO)]) == os.path.realpath(REPO):
        raise SystemExit('--work-root must be outside the repository (raw logs are not kept in it)')
    # 利用者が指定した別名も保存時の置換に含める（macOS の /var と /private/var など）。
    args.work_root = os.path.abspath(args.work_root) if args.work_root else work_root
    budget = CostBudget(args.stop_cost)
    headers = {}
    status = 0
    try:
        for name in names:
            root = os.path.join(work_root, name) if len(names) > 1 else work_root
            status = max(status, evaluate(args, tasks, benitoite, name, harnesses[name], skill_dir, root,
                                          budget, headers))
            if budget.message:
                break
    finally:
        if len(names) > 1 and ALL_ROWS:
            # 中断でも、それまでに終わった項目を全体の記録に残す。
            directory = work_root if args.dry_run else RESULTS_DIR
            record = os.path.join(directory, '%s-overall.md' % datetime.datetime.now().strftime('%Y-%m-%d-%H%M'))
            header = {'ハーネス': ', '.join(names), '生の記録': work_root, '項目': headers}
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
        'Skill': ('%s（--skill-dir %s）' % (skill_version(first_skill), args.skill_dir)) if skill_dir
        else skill_version(first_skill),
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


def evaluate(args: argparse.Namespace, tasks: List[Task], benitoite: str, harness_name: str,
             harness: Dict[str, object], skill_dir: Optional[str], work_root: str,
             budget: CostBudget, headers: Dict[str, object]) -> int:
    """試行が終わるたびに累計し、途中で止まっても完了した試行を記録する。"""
    model = args.model or str(harness['model'])
    rows = []
    dsh_key = None
    if harness.get('prepare') == 'dsh' and not args.dry_run:
        try:
            check_dsh_workspace(work_root)
            dsh_overlay(model, harness.get('options', {}).get('effort', 'high'))
            dsh_key = read_dsh_key(harness)
        except ValueError as error:
            raise SystemExit(str(error)) from None
    try:
        for task in tasks:
            for trial in range(1, args.trials + 1):
                number = trial
                while os.path.exists(os.path.join(work_root, task.name, str(number))):
                    number += args.trials
                trial_dir = os.path.join(work_root, task.name, str(number))
                os.makedirs(trial_dir)
                try:
                    extra = {'dsh_key': dsh_key} if dsh_key is not None else {}
                    if harness.get('prepare') == 'agy':
                        extra['work_root'] = work_root
                    row = run_trial(task, trial_dir, benitoite, harness_name, harness, model, skill_dir, args.dry_run,
                                    args.benitoiteh, **extra)
                except ValueError as error:
                    raise SystemExit(str(error))
                row['billing'] = harness.get('billing', 'none')
                rows.append(row)
                ALL_ROWS.append(row)
                print('%s %-16s trial %d: %s, attempts %d, %s' % (
                    harness_name, task.name, trial, 'success' if row['success'] else 'failure', row['attempts'],
                    row['reason']), flush=True)
                budget.add(row)
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
        wins = sum(1 for r in rows if r['success'])
        print('%s: %d of %d trials succeeded; workspaces in %s' % (harness_name, wins, len(rows), work_root))
        if args.dry_run:
            bad = [r for r in rows if not (r['success'] and r['log_exists'] and r['attempts'] >= 1)]
            return 1 if bad else 0
        return 0
    finally:
        if rows:
            header = evaluation_header(args, benitoite, harness_name, harness, skill_dir, work_root, rows)
            if args.dry_run:
                header['種類'] = 'dry-run'
            if budget.message:
                header['停止'] = budget.message
            headers[harness_name] = header
            record = (os.path.join(work_root, 'record.md') if args.dry_run else args.record or os.path.join(
                RESULTS_DIR, '%s-%s.md' % (datetime.datetime.now().strftime('%Y-%m-%d-%H%M'), harness_name)))
            write_record(record, header, rows, tasks, args.work_root)
            print('record: %s' % record)


if __name__ == '__main__':
    sys.exit(main())
