#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""評価の配線を、本物の LLM を呼ばずに確かめる Messages API の模擬（実装プラン P5-02）。"""
import json
import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import List, Optional, Tuple


class MockServer(ThreadingHTTPServer):
    def __init__(self, kind: str, script: str, content: str, completion_check: bool,
                 stages: Optional[List[dict]], input_tokens: int, verify: Optional[str] = None) -> None:
        super().__init__(('127.0.0.1', 0), Handler)
        self.completion_check = completion_check
        self.kind = kind
        self.script = script
        self.content = content
        self.problems = []  # type: List[str]
        self.requests = 0
        self.summary_requests = 0
        self.stages = stages
        self.stage = 0
        self.step = 0
        self.after_summary = False
        self.input_tokens = input_tokens
        self.verify = verify
        prompt = os.path.join(os.path.dirname(__file__), '../../crates/benitoiteh/prompts/compaction.md')
        with open(prompt, encoding='utf-8') as f:
            self.summary_prompt = f.read().strip()
        self.request_lock = threading.Lock()
        self.daemon_threads = True


class Handler(BaseHTTPRequestHandler):
    timeout = 10

    def send_body(self, status: int, body: bytes, content_type: str) -> None:
        self.send_response(status)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def problem(self, status: int, message: str) -> None:
        self.server.problems.append(message)
        self.send_body(status, message.encode('utf-8'), 'text/plain; charset=utf-8')

    def do_POST(self) -> None:
        server = self.server
        with server.request_lock:
            server.requests += 1
            number = server.requests
        if not server.stages and number > (4 if server.kind == 'normal' else 5 if server.completion_check else 3):
            self.problem(500, 'unexpected request %d after the scenario ended' % number)
            return
        if self.path != '/v1/messages':
            self.problem(400, 'expected POST /v1/messages')
            return
        try:
            length = int(self.headers.get('Content-Length', ''))
            if length < 0 or length > 8 * 1024 * 1024:
                raise ValueError('invalid Content-Length')
        except ValueError:
            self.problem(400, 'Content-Length must be between 0 and 8 MiB')
            return
        try:
            request = json.loads(self.rfile.read(length))
        except (ValueError, UnicodeDecodeError):
            self.problem(400, 'request is not JSON')
            return
        messages = request.get('messages') if isinstance(request, dict) else None
        if server.stages:
            self.staged_response(messages, number)
            return
        if server.kind == 'normal':
            if number > 1:
                last = messages[-1] if isinstance(messages, list) and messages else {}
                blocks = last.get('content', []) if isinstance(last, dict) else []
                if not isinstance(blocks, list) or not any(
                        isinstance(block, dict) and block.get('type') == 'tool_result' for block in blocks):
                    self.problem(400, 'normal request has no final tool_result')
                    return
            blocks = [
                {'type': 'tool_use', 'name': 'read', 'input': {'path': server.script}},
                {'type': 'tool_use', 'name': 'write', 'input': {'path': server.script, 'content': server.content}},
                {'type': 'tool_use', 'name': 'bash', 'input': {'command': server.verify}},
                {'type': 'text', 'text': 'Done.'},
            ]
            block = blocks[number - 1]
            if block['type'] == 'tool_use':
                block['id'] = 'mock-normal-%d' % number
            self.respond(block, number, 1000)
            return
        if number > 1 and number != 4:
            last = messages[-1] if isinstance(messages, list) and messages else {}
            blocks = last.get('content') if isinstance(last, dict) else None
            if not (isinstance(last, dict) and last.get('role') == 'user' and isinstance(blocks, list)
                    and any(isinstance(block, dict) and block.get('type') == 'tool_result' for block in blocks)):
                self.problem(400, 'request %d has no final user tool_result' % number)
                return
        if number == 4:
            last = messages[-1] if isinstance(messages, list) and messages else {}
            blocks = last.get('content', []) if isinstance(last, dict) else []
            if not (isinstance(last, dict) and last.get('role') == 'user' and isinstance(blocks, list)
                    and any(block.get('type') == 'text' and isinstance(block.get('text'), str)
                            and block['text'].startswith('<completion_check>\n')
                            for block in blocks if isinstance(block, dict))):
                self.problem(400, 'request 4 has no completion prompt')
                return
        if number == 1:
            block = {'type': 'tool_use', 'id': 'mock-write', 'name': 'write',
                     'input': {'path': server.script, 'content': server.content}}
        elif number in (2, 4):
            block = {'type': 'tool_use', 'id': 'mock-execute',
                     'name': 'benitoite_test' if server.kind == 'test' else 'benitoite_run',
                     'input': {'paths': [server.script]} if server.kind == 'test' else {'path': server.script}}
        else:
            block = {'type': 'text', 'text': 'Done.'}
        self.respond(block, number, 1000)

    def staged_response(self, messages: object, number: int) -> None:
        server = self.server
        last = messages[-1] if isinstance(messages, list) and messages else {}
        blocks = last.get('content', []) if isinstance(last, dict) else []
        if isinstance(blocks, str):
            blocks = [{'type': 'text', 'text': blocks}]
        texts = [block.get('text', '') for block in blocks if isinstance(block, dict) and block.get('type') == 'text']
        if any(server.summary_prompt in text for text in texts):
            server.summary_requests += 1
            server.after_summary = True
            self.respond({'type': 'text', 'text': '## Goal\nContinue the staged task.\n'
                          '## Constraints & Preferences\nKeep all prior requirements.\n'
                          '## Progress\n### Done\nRead the supplied materials.\n'
                          '### In Progress\nUpdate the script.\n### Blocked\nNone.\n'
                          '## Key Decisions\nUse Benitoite.\n## Errors and Fixes\nNone.\n'
                          '## Next Steps\nFinish the current request.\n## Critical Context\n'
                          'The script is %s.\n' % server.script}, number, 1000)
            return
        if server.stage >= len(server.stages):
            self.problem(500, 'unexpected request after all stages ended')
            return
        stage = server.stages[server.stage]
        if server.step == 0:
            if not any(text.endswith(stage['prompt']) for text in texts):
                self.problem(400, 'stage %d has no expected request' % (server.stage + 1))
                return
        elif not server.after_summary and not any(
                isinstance(block, dict) and block.get('type') == 'tool_result' for block in blocks):
            self.problem(400, 'stage request has no final tool_result')
            return
        server.after_summary = False
        # 再開では読んだ版を戻さないため、既存のスクリプトを置換する前に読み直す。
        paths = stage['materials'] + ([server.script] if server.stage > 0 else [])
        if server.step < len(paths):
            block = {'type': 'tool_use', 'name': 'read', 'input': {'path': paths[server.step]}}
        elif server.step == len(paths):
            block = {'type': 'tool_use', 'name': 'write', 'input': {'path': server.script, 'content': server.content}}
        elif server.step == len(paths) + 1:
            block = {'type': 'tool_use', 'name': 'benitoite_run', 'input': {'path': server.script}}
        else:
            block = {'type': 'text', 'text': 'Done with stage %d.' % (server.stage + 1)}
        if block['type'] == 'tool_use':
            block['id'] = 'mock-stage-%d-step-%d' % (server.stage + 1, server.step)
        server.step += 1
        if block['type'] == 'text':
            server.stage += 1
            server.step = 0
        self.respond(block, number, server.input_tokens)

    def respond(self, block: dict, number: int, input_tokens: int) -> None:
        # tests/support/exec_fixture.rs の Protocol::Anthropic と同じく、input 全体を start に入れる。
        events = [
            {'type': 'message_start', 'message': {'id': 'mock-%d' % number, 'usage': {
                'input_tokens': input_tokens, 'cache_read_input_tokens': 0, 'cache_creation_input_tokens': 0,
                'output_tokens': 0}}},
            {'type': 'content_block_start', 'index': 0, 'content_block': block},
            {'type': 'content_block_stop', 'index': 0},
            {'type': 'message_delta', 'delta': {'stop_reason': 'end_turn' if block['type'] == 'text' else 'tool_use'},
             'usage': {'output_tokens': 50}},
            {'type': 'message_stop'},
        ]
        body = ''.join('event: %s\ndata: %s\n\n' % (event['type'], json.dumps(event)) for event in events)
        self.send_body(200, body.encode('utf-8'), 'text/event-stream')

    def log_message(self, format: str, *args: object) -> None:
        # 通常の評価の出力に HTTP の記録を混ぜない。
        pass


def start(kind: str, script: str, content: str, completion_check: bool = False,
          stages: Optional[List[dict]] = None, input_tokens: int = 1000,
          verify: Optional[str] = None) -> Tuple[MockServer, str]:
    """試行ごとの筋書きで起動する。呼び出し側が shutdown と server_close を行う。"""
    if kind not in ('run', 'test', 'staged', 'normal'):
        raise ValueError('unknown task kind: %s' % kind)
    if kind == 'staged' and (not stages or completion_check):
        raise ValueError('staged mock requires stages and completion_check false')
    if kind == 'normal' and (not verify or completion_check or stages):
        raise ValueError('normal mock requires verify, no stages and completion_check false')
    server = MockServer(kind, script, content, completion_check, stages, input_tokens, verify)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, 'http://127.0.0.1:%d' % server.server_address[1]
