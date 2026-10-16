# P1-02 模擬の提供者と `anthropic-messages` のアダプタ

## 目的

テストが使う模擬の提供者（HTTP のサーバ）と、Anthropic の Messages API のアダプタを作る。アダプタは、共通の形（P1-01）から要求を作り、ストリームを共通の形のイベントと `AssistantTurn` に変え、誤りを分類する。

## 読む設計書

- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「共通の形」「Anthropic の Messages API」「提供者ごとの機能の表」「道具の定義の変換」「認証の情報」「HTTP の通信」
- [文脈の管理](../../design/01-agent/01-04-context.md)の「文脈の構成」「推論の内容の扱い」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「やり直し」
- [テスト](../../design/07-quality/07-01-testing.md)の「模擬の提供者」
- P1-01 で作った `provider` の型、P0-03 で作った `http`

## 作るもの

1. 模擬の提供者 `crates/benitoiteh/tests/support/mock_provider.rs`（統合テストが `mod support;` で使う）。
   - 手元の空いたポート（`127.0.0.1:0`）で待ち受ける HTTP/1.1 のサーバ。スレッドで動き、テストの終わりに止まる。
   - 台本: 要求の番号ごとに、HTTP の状態、ヘッダ、本文のバイト列（SSE か JSON）、送り方（一度に送る、塊に分けて間を置いて送る、途中で接続を切る、ヘッダの後で止まる）を与える。
   - 受け取った要求（パス、ヘッダ、本文の JSON）を記録し、テストが読めるようにする。API キーのヘッダも記録するが、テストの外に出さない。
   - Anthropic、OpenAI の Chat Completions、Responses のどの形の台本も書けるように、形に依らない作りにする。
   - 台本の SSE は、Anthropic の文書の形に従って手で書く（`message_start`、`content_block_start`、`content_block_delta` の `text_delta`・`thinking_delta`・`signature_delta`・`input_json_delta`、`content_block_stop`、`message_delta`（`stop_reason` と `usage`）、`message_stop`、`ping`、`error`）。台本を作る補助の関数を置いてよい。
2. `crates/benitoiteh/src/provider/anthropic.rs`。
   - 要求: `POST {base_url}/v1/messages`、ヘッダ `x-api-key`、`anthropic-version: 2023-06-01`、`content-type: application/json`。本文は `model`、`max_tokens`、`system`、`tools`、`messages`、`stream: true`、`thinking: {type: "adaptive"}`、`output_config: {effort: <値>}`。`max_tokens` は、`Capabilities` の `max_output_tokens` があればその値、なければ 32,000 とする。
   - 道具の定義: `ToolSpec` を Anthropic の形に変え、`strict_tools` があれば `strict: true`、`eager_input_streaming` があれば `eager_input_streaming: true` を付ける。`tool_choice` は、圧縮のための `none` のほかは付けない（02-01）。
   - `cache_control`（機能の表で有効なとき）: システムプロンプトの最後、前の要求の会話の最後の項目、今の要求の会話の最後の項目の三か所に `{"type": "ephemeral"}` を付ける（02-01）。前の要求の最後の項目がどれかは、呼び出す側が与える（会話の中の位置）。
   - 会話の変換: 同じ送り先の応答は `RawItems` の content block をそのまま送る（02-01 の「共通の形」）。違う送り先の応答は、共通の形から作り、推論の内容は送らず、道具の呼び出しの ID を P1-01 の対応の関数で作る。`reasoning_replay` が `None` のときも推論を送らない。道具の結果は、利用者の発言の中の `tool_result` の block（`tool_use_id`、`content`、`is_error`）にする。同じ応答の道具の結果は一つの利用者の発言にまとめる。
   - ストリームの変換: SSE のイベントを `StreamEvent` に変え、受け取り終えたら、content block を受け取った順に並べた `RawItems` と `AssistantTurn` を作る。content block の元の形（`thinking` の `signature`、`redacted_thinking`、`tool_use` の `id`・`name`・`input`）を、受け取った値から組み立てて保つ。`tool_use` の `input` は、`input_json_delta` の断片をつないで JSON として読む（読めなければ、その呼び出しを引数の誤りとして `AssistantTurn` に残し、P1-09 が結果を誤りにする）。
   - 終わりの理由: `stop_reason` の `end_turn`・`stop_sequence` は終わった、`tool_use` は道具の呼び出し、`max_tokens` は出力の上限、`refusal` は拒否（`stop_details.category` を持つ）、`pause_turn` は終わった（P1 では続きを求めない。報告に書く）。`message_stop` を受け取る前にストリームが切れたら切断とする。
   - 使ったトークンの量: `message_start` と `message_delta` の `usage` から、入力の全体を `input_tokens + cache_read_input_tokens + cache_creation_input_tokens` とし（02-01）、報告のない項目は `None` とする。
   - 誤り: HTTP の 429・500〜599・529 と、ストリームの中の `error` のイベントの `overloaded_error` はやり直せる。401・403 は認証、400 は要求の形（本文の `error.type` と `message` を保つ）、文脈の超過（400 で、文書の示す超過の誤り。分からなければ文言 `prompt is too long` を含むもの）を分ける。`Retry-After` を保つ。
3. 認証の情報の受け取り: アダプタは API キーの文字列を受け取るだけとする（参照の解決は P1-04）。キーを誤りの文、`Debug` の出力、記録に含めない（`Debug` を手で書き、キーを伏せる）。

## 触るファイル

- `crates/benitoiteh/src/provider/mod.rs`（アダプタの登録）
- `crates/benitoiteh/tests/support/`（新）

## テスト

模擬の提供者を通し、`http` の経路も含めて確かめる（`tests/provider_anthropic.rs`）。

- 文と道具の呼び出しと thinking が交互に並ぶ応答から、順序と ID と署名を変えない `RawItems` と、同じ順の `AssistantTurn` ができる。
- 同じ送り先へ次の要求を作ると、前の応答の content block が順序と ID と署名を変えずに送られる（模擬の提供者が記録した本文で確かめる）。違う送り先では、推論が送られず、ID が Anthropic の形になる。
- 要求の本文の `cache_control` の位置（三か所）、`thinking`・`output_config.effort`・`strict`・`eager_input_streaming` の有無（機能の表で切ったときに付かない）。
- `max_tokens` で終わった応答、`refusal`、`message_stop` の前の切断、`error` のイベント、429（`Retry-After`）、529、401、400 の分類。
- `usage` の入力の全体の足し方と、報告のない項目が `None` になること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、アダプタの公開の関数のシグネチャ（要求を作る、イベントを読む、組み立てる）を書く。P1-03 と P1-09 が同じ形を使う。
