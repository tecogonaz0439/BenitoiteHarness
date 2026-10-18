#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""HTTP の課題（tasks/http_client）の接続先のサーバ（設計書 06-06「Skill の評価」）。

同じ機械のループバック（127.0.0.1）で、決まった応答だけを返す。待ち受けのポートは OS に空いているものを
選ばせ（ポート 0）、接続先の URL を環境変数 SKILL_EVAL_HTTP_BASE（例 http://127.0.0.1:54321）で
スクリプトに渡す。Python の標準ライブラリだけを使い、開発機の python3（3.9）で動く書き方にする。

単独で起動すると、接続先の URL を標準出力に一行書き、止められるまで応答する:

    python3 tools/skill-eval/http_server.py
"""
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Tuple

# スクリプトに接続先を渡す環境変数の名前（課題の task.md にも同じ名前を書く）
BASE_VARIABLE = 'SKILL_EVAL_HTTP_BASE'

INVENTORY = [
    {'name': 'pen', 'stock': 12, 'price': 120},
    {'name': 'eraser', 'stock': 3, 'price': 80},
    {'name': 'stapler', 'stock': 0, 'price': 900},
    {'name': 'notebook', 'stock': 5, 'price': 202},
]


class Handler(BaseHTTPRequestHandler):
    # 応答ごとに接続を閉じる（HTTP/1.0 の既定）。Content-Length は必ず付ける
    def _send(self, status: int, body: bytes, content_type: str) -> None:
        self.send_response(status)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _text(self, status: int, text: str) -> None:
        self._send(status, text.encode('utf-8'), 'text/plain; charset=utf-8')

    def do_GET(self) -> None:
        if self.path == '/status':
            self._text(200, 'ok')
        elif self.path == '/inventory':
            self._send(200, json.dumps(INVENTORY).encode('utf-8'), 'application/json')
        else:
            self._text(404, 'not found')

    def do_POST(self) -> None:
        length = int(self.headers.get('Content-Length') or '0')
        body = self.rfile.read(length) if length > 0 else b''
        if self.path != '/report':
            self._text(404, 'not found')
            return
        try:
            value = json.loads(body.decode('utf-8'))
        except (UnicodeDecodeError, ValueError):
            self._text(400, 'the body is not JSON')
            return
        if not isinstance(value, list):
            self._text(400, 'the body is not a JSON array')
            return
        self._text(201, 'received %d items' % len(value))

    def log_message(self, format: str, *args: object) -> None:
        # 評価の出力を汚さないよう、要求の記録を書かない
        pass


def start() -> Tuple[ThreadingHTTPServer, str]:
    """別のスレッドでサーバを起動し、サーバと接続先の URL を返す。止めるのは呼び出し側（shutdown）。"""
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    base = 'http://127.0.0.1:%d' % server.server_address[1]
    return server, base


def main() -> int:
    server, base = start()
    print(base, flush=True)
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        pass
    finally:
        server.shutdown()
        server.server_close()
    return 0


if __name__ == '__main__':
    sys.exit(main())
