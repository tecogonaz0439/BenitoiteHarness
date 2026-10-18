#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Antigravity の全道具を PreToolUse の門で検査する（実装プラン P5-07）。"""
import datetime
import json
import os
import re
import stat
import sys
import unicodedata
from typing import List, Optional
from urllib.parse import unquote, urlsplit

# allow だけでは非対話の command 権限を得られない（P5-07 補足 2 の実走結果）。
PERMISSION_OVERRIDES = True
READ_TOOLS = {'view_file', 'list_dir', 'grep_search', 'find_by_name', 'view_file_outline', 'view_code_item'}
WRITE_TOOLS = {'write_to_file', 'replace_file_content', 'multi_replace_file_content'}
PROTECTED_DIRS = {'.agents', '_agents', '.agent', '_agent', '.gemini', '.git'}
PROTECTED_FILES = {'agents.md', 'gemini.md'}


def file_path(value: str) -> Optional[str]:
    def invalid_boundary(text: str) -> bool:
        return (not text or '\x00' in text or any(char.isspace() or unicodedata.category(char).startswith('C')
                                                 for char in (text[0], text[-1])))
    if invalid_boundary(value):
        raise ValueError('invalid path whitespace or control character')
    if value.startswith('file://'):
        uri = urlsplit(value)
        if uri.netloc not in ('', 'localhost') or uri.query or uri.fragment:
            raise ValueError('unsupported file URI')
        value = unquote(uri.path)
        if invalid_boundary(value) or not os.path.isabs(value):
            raise ValueError('file URI must be absolute')
    return value if value.startswith('/') else None


def path_arguments(value: object) -> List[str]:
    """本文や検索語と区別し、パスの鍵の値だけを検査する。鍵名の推測は実走で確かめる。"""
    paths = []
    if isinstance(value, dict):
        for key, child in value.items():
            if key.lower().endswith(('path', 'file', 'directory', 'uri')):
                # FullPath などの表示の指定は、パスの文字列として扱わない。
                if isinstance(child, (bool, int, float)):
                    continue
                values = [child] if isinstance(child, str) else child
                if not isinstance(values, list) or not values or any(not isinstance(item, str) for item in values):
                    raise ValueError('path value must be a string or a nonempty list of strings')
                for item in values:
                    path = file_path(item)
                    if path is None:
                        raise ValueError('path must be absolute')
                    paths.append(path)
            else:
                paths.extend(path_arguments(child))
    elif isinstance(value, list):
        for child in value:
            paths.extend(path_arguments(child))
    return paths


def argument_keys(value: object) -> List[str]:
    keys = set()
    if isinstance(value, dict):
        keys.update(value)
        for child in value.values():
            keys.update(argument_keys(child))
    elif isinstance(value, list):
        for child in value:
            keys.update(argument_keys(child))
    return sorted(keys)


def permitted(name: str, args: dict, workspace: str) -> bool:
    if not workspace:
        return False
    if name == 'run_command':
        command, cwd = args.get('CommandLine'), args.get('Cwd')
        if not (isinstance(command, str) and len(command) <= 4096
                and re.fullmatch(r'benitoite( [A-Za-z0-9_./=:,+-]+)*', command)
                and isinstance(cwd, str) and os.path.isabs(cwd)
                and os.path.commonpath([workspace, os.path.realpath(cwd)]) == workspace):
            return False
        arguments = command.split(' ')[1:]
        if not arguments or arguments[0] not in ('check', 'run', 'test'):
            return False
        return not any(arg.startswith('=') or re.search(r'[=,:]=', arg)
                       or any(part.startswith('/') or '..' in part.split('/') for part in re.split(r'[=,:]', arg))
                       for arg in arguments)
    if name not in READ_TOOLS | WRITE_TOOLS:
        return False
    paths = path_arguments(args)
    if not paths:
        return False
    for path in paths:
        real = os.path.realpath(path)
        if os.path.commonpath([workspace, real]) != workspace:
            return False
        if name in WRITE_TOOLS:
            for candidate in (path, real):
                parts = os.path.relpath(candidate, workspace).lower().split(os.path.sep)
                if set(parts) & PROTECTED_DIRS or parts[-1] in PROTECTED_FILES:
                    return False
    return True


def main() -> None:
    workspace = os.path.realpath(sys.argv[1]) if len(sys.argv) == 2 else ''
    decision, name, command, keys = 'deny', None, None, []
    reason = 'This tool is disabled for this evaluation.'
    try:
        value = json.load(sys.stdin)
        tool = value.get('toolCall', {}) if isinstance(value, dict) else {}
        name, args = tool.get('name'), tool.get('args', {})
        if isinstance(args, dict):
            command, keys = args.get('CommandLine'), argument_keys(args)
        if name == 'run_command':
            reason = ('Only "benitoite check|run|test <args>" commands with relative arguments are allowed, '
                      'with Cwd set to %s. Use the file tools to read and edit files.') % workspace
        elif isinstance(name, str) and name in READ_TOOLS | WRITE_TOOLS:
            reason = ('Use an absolute path inside %s. ' % workspace +
                      ('Provide an absolute file path and do not edit agent configuration or instruction files.'
                       if name in WRITE_TOOLS else 'Use the file tools to read files in this workspace.'))
        if isinstance(name, str) and isinstance(args, dict) and permitted(name, args, workspace):
            decision = 'allow'
    except (OSError, ValueError, TypeError, AttributeError, RecursionError):
        pass
    result = {'decision': decision, 'reason': reason}
    if decision == 'allow' and name == 'run_command' and PERMISSION_OVERRIDES:
        result['permissionOverrides'] = ['command(%s)' % command]
    # ログは準備で作った通常ファイルだけを開き、リンクや FIFO を辿らない。
    try:
        path = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'gate.log')
        descriptor = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(descriptor, 'a', encoding='utf-8') as f:
            if not stat.S_ISREG(os.fstat(f.fileno()).st_mode):
                raise ValueError('gate log is not a regular file')
            f.write(json.dumps({'time': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                                'decision': decision, 'tool': name if isinstance(name, str) else None,
                                'argument_keys': keys,
                                'command': command[:200] if isinstance(command, str) else None}) + '\n')
    except (OSError, ValueError):
        result = {'decision': 'deny', 'reason': reason}
    print(json.dumps(result))


if __name__ == '__main__':
    main()
