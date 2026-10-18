#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""評価の配線を、本物の LLM を呼ばずに確かめる Messages API の模擬（実装プラン P5-02）。"""
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import List, Tuple


class MockServer(ThreadingHTTPServer):
    def __init__(self, kind: str, script: str, content: str) -> None:
        super().__init__(('127.0.0.1', 0), Handler)
        self.kind = kind
        self.script = script
        self.content = content
        self.problems = []  # type: List[str]
        self.requests = 0
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
        if number > 3:
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
        if number > 1:
            last = messages[-1] if isinstance(messages, list) and messages else {}
            blocks = last.get('content') if isinstance(last, dict) else None
            if not (isinstance(last, dict) and last.get('role') == 'user' and isinstance(blocks, list)
                    and any(isinstance(block, dict) and block.get('type') == 'tool_result' for block in blocks)):
                self.problem(400, 'request %d has no final user tool_result' % number)
                return
        if number == 1:
            block = {'type': 'tool_use', 'id': 'mock-write', 'name': 'write',
                     'input': {'path': server.script, 'content': server.content}}
        elif number == 2:
            block = {'type': 'tool_use', 'id': 'mock-execute',
                     'name': 'benitoite_test' if server.kind == 'test' else 'benitoite_run',
                     'input': {'paths': [server.script]} if server.kind == 'test' else {'path': server.script}}
        else:
            block = {'type': 'text', 'text': 'Done.'}
        # tests/support/exec_fixture.rs の Protocol::Anthropic と同じく、input 全体を start に入れる。
        events = [
            {'type': 'message_start', 'message': {'id': 'mock-%d' % number, 'usage': {
                'input_tokens': 1000, 'cache_read_input_tokens': 0, 'cache_creation_input_tokens': 0,
                'output_tokens': 0}}},
            {'type': 'content_block_start', 'index': 0, 'content_block': block},
            {'type': 'content_block_stop', 'index': 0},
            {'type': 'message_delta', 'delta': {'stop_reason': 'end_turn' if number == 3 else 'tool_use'},
             'usage': {'output_tokens': 50}},
            {'type': 'message_stop'},
        ]
        body = ''.join('event: %s\ndata: %s\n\n' % (event['type'], json.dumps(event)) for event in events)
        self.send_body(200, body.encode('utf-8'), 'text/event-stream')

    def log_message(self, format: str, *args: object) -> None:
        # 通常の評価の出力に HTTP の記録を混ぜない。
        pass


def start(kind: str, script: str, content: str) -> Tuple[MockServer, str]:
    """試行ごとの筋書きで起動する。呼び出し側が shutdown と server_close を行う。"""
    if kind not in ('run', 'test'):
        raise ValueError('unknown task kind: %s' % kind)
    server = MockServer(kind, script, content)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, 'http://127.0.0.1:%d' % server.server_address[1]
