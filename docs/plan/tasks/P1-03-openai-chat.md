# P1-03 `openai-chat` のアダプタ

## 目的

OpenAI の Chat Completions API の形のアダプタを作る。手元の推論サーバと中継のサービスと、DeepSeek の Chat Completions の口がこの形を使う。P1-02 のアダプタと同じ形（公開の関数のシグネチャ）に揃える。

## 読む設計書

- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「共通の形」「OpenAI の Chat Completions API」「提供者ごとの機能の表」「道具の定義の変換」
- [文脈の管理](../../design/01-agent/01-04-context.md)の「推論の内容の扱い」
- P1-01 の型、P1-02 のアダプタと模擬の提供者

## 作るもの

`crates/benitoiteh/src/provider/openai_chat.rs`。

- 要求: `POST {base_url}/chat/completions`、`Authorization: Bearer <キー>`（キーがなければ付けない。手元の推論サーバのため）。本文は `model`、`messages`（最初に `role: "system"` のシステムプロンプト）、`tools`（`type: "function"` で包む。`strict_tools` があれば strict の形）、`stream: true`、`stream_options: {include_usage: true}`。`max_tokens` は `Capabilities` の `max_output_tokens` があるときだけ付ける。`parallel_tool_calls` は機能の表で有効なときだけ `true` を付ける。
- 会話の変換: 応答は `role: "assistant"` の発言（`content` と `tool_calls`）、道具の結果は `role: "tool"` の発言（`tool_call_id`）にする。同じ送り先では、`RawItems` に持った元の発言をそのまま送る。推論の文（`reasoning_content`）は、`reasoning_replay` が `AllTurns` なら、すべての番の応答に付けて送る。`SameDestination` なら、同じ送り先の応答にだけ付ける。`None` なら付けない。
- ストリームの変換: 差分（`choices[0].delta`）の `content`、`reasoning_content`（`reasoning` の欄を返す口もある。両方を読む）、`tool_calls`（`index` ごとに `id`・`function.name`・`function.arguments` の断片をつなぐ）を `StreamEvent` に変える。`finish_reason` の `stop` は終わった、`tool_calls` は道具の呼び出し、`length` は出力の上限、`content_filter` は拒否とする。`data: [DONE]` を受け取る前に切れたら切断とする。`finish_reason` を受け取った後、`usage` だけを持つ塊（`choices` が空）が続くことを扱う。
- 使ったトークンの量: `usage.prompt_tokens` を入力の全体、`usage.prompt_tokens_details.cached_tokens` をキャッシュから読んだ量（入力の内数、02-01）、`completion_tokens` を出力、`completion_tokens_details.reasoning_tokens` を推論とする。キャッシュに書いた量は `None` とする。
- 誤り: HTTP の 429・500〜599 はやり直せる。401・403 は認証、400 は要求の形、文脈の超過（本文の `error.code` が `context_length_exceeded` のもの）を分ける。
- 元の発言の `RawItems`: 受け取った差分から組み立てた `assistant` の発言（`content`、`tool_calls`、`reasoning_content`）を一つの項目として持つ。

## 触るファイル

- `crates/benitoiteh/src/provider/mod.rs`（アダプタの登録）
- `crates/benitoiteh/tests/support/`（台本の補助を足すとき）

## テスト

`tests/provider_openai_chat.rs` で、模擬の提供者を通して確かめる。

- 複数の道具の呼び出しが `index` ごとに断片で届く応答から、正しい呼び出しの並びができる。
- `reasoning_replay` の三つの値で、次の要求の本文に `reasoning_content` が付く番が変わる。
- `length`、`content_filter`、`[DONE]` の前の切断、`usage` だけの塊、429、`context_length_exceeded` の分類。
- キーがないときに `Authorization` のヘッダを送らない。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
