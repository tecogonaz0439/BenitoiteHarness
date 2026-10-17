# P3-03 `openai-responses` のアダプタ

## 目的

OpenAI の Responses API の形のアダプタを作る。P1-02（`anthropic-messages`）と P1-03（`openai-chat`）のアダプタと同じ形（公開の関数のシグネチャ）に揃え、エージェントのループから使えるようにする。要求と応答の形は、2026-10-17 に公式の文書と SDK で確かめた内容（設計書 02-01 の「OpenAI の Responses API」、BH-030）による。本物の API は呼ばない（模擬の提供者で確かめる）。

## 読む設計書

- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「共通の形」「OpenAI の Responses API」「提供者ごとの機能の表」「道具の定義の変換」「HTTP の通信」
- [文脈の管理](../../design/01-agent/01-04-context.md)の「推論の内容の扱い」「圧縮」（道具を呼ばせない要求）
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「やり直し」
- 既存のもの: `src/provider/`（`types.rs` の共通の形と `AdapterKind`・`Capabilities`、`sse.rs`、`ids.rs`、`tool_schema.rs`、`anthropic.rs`・`anthropic/`、`openai_chat.rs`・`openai_chat/`）、`src/agent/transport.rs`（アダプタの振り分け）、`tests/provider_openai_chat.rs`、`tests/support/`（模擬の提供者）

## 作るもの

`crates/benitoiteh/src/provider/openai_responses.rs` と `openai_responses/`（`request.rs`・`assembly.rs`・`text.rs`。`openai_chat` と同じ分け方）。

### 要求

- `POST {base_url}/responses`、`Authorization: Bearer <キー>`（キーがなければ付けない）。
- 本文: `model`、`instructions`（システムプロンプト）、`input`（会話の項目の並び）、`tools`、`tool_choice: "auto"`（圧縮で道具を呼ばせないときは `"none"`。今の `openai_chat` の `disable_tools` に当たる指定）、`parallel_tool_calls`（機能の表で有効なときだけ `true`）、`store: false`、`stream: true`、`include: ["reasoning.encrypted_content"]`、`reasoning: {effort: <設定の値>, summary: "auto"}`、`prompt_cache_key`（セッションの ID）。`max_output_tokens` は `Capabilities` の `max_output_tokens` があるときだけ付ける。
- 道具の定義は平らな形 `{type: "function", name, description, parameters, strict}`。`strict_tools` が有効なら `strict: true` と厳密な形の `parameters`（`tool_schema.rs` の OpenAI の形）、無効なら `strict: false`。
- セッションの ID をアダプタに渡す方法（`prompt_cache_key` のため）は、既存の作り（`AgentConfig` など）に合わせて決め、報告に書く。

### 会話の変換

- 利用者の発言は `{type: "message", role: "user", content: [{type: "input_text", text}]}`。
- 同じ送り先の応答は、`RawItems` に持った `output` の項目を、順序と欄（`id`、`phase`、`encrypted_content`、`status` など）を変えずに並べる。ただし、`id` は、Codex CLI に倣い、接頭辞の付いたもの（`rs_`・`fc_`・`msg_` のように英小文字と `_` で始まるもの）だけを残す（設計書 02-01）。接頭辞の判定の関数は一つにまとめ、ユニットテストする。
- 道具の結果は `{type: "function_call_output", call_id, output: <文字列>}`。`call_id` は関数の呼び出しの項目の `call_id` を使う（`id` ではない）。
- `reasoning_replay` が `SameDestination` 以外なら、同じ送り先の応答でも `reasoning` の項目を送らない。
- 違う送り先の応答は、P1-02・P1-03 と同じく共通の形から作る。文は `{type: "message", role: "assistant", content: [{type: "output_text", text, annotations: []}]}` にし、推論は送らない。道具の呼び出しは `{type: "function_call", call_id, name, arguments}` とし、`call_id` は P1-01 の対応の関数で作る（Responses の `call_id` の形の上限は、確かめた資料にないので、英数字と `_`・`-` の 64 字までとする）。項目の `id` は付けない。

### ストリームの変換

- SSE の行は `sse.rs` で読み、`:` で始まる注釈の行を飛ばす。`data:` の JSON の `type` で振り分ける（`event:` の行の名前は使わなくてよい）。`[DONE]` の行は来ない前提とし、来ても誤りにしない。
- `response.output_text.delta` は文の差分、`response.reasoning_summary_text.delta` は推論の文の差分、`response.function_call_arguments.delta` は道具の呼び出しの引数の差分とする。呼び出しの始まりは `response.output_item.added` の `function_call` の項目（`call_id`・`name`）で知り、`output_index` で対応を取る。
- `response.output_item.done` の項目を、そのまま `RawItems` の一つとして持つ。推論の項目と `encrypted_content` は、`added` ではなく `done` の項目から取る（設計書 02-01）。
- 終わり: `response.completed` は終わった（道具の呼び出しの項目があれば道具の呼び出し）、`response.incomplete` は `incomplete_details.reason` が `max_output_tokens` なら出力の上限、`content_filter` なら拒否、ほかは誤りとする。`response.failed` と `error` のイベントは誤りとする（`error.code` と `message` を保つ）。応答の文の部分に `refusal` があれば拒否とする（`response.refusal.delta` を受けたときも）。
- 終わりのイベントを受け取る前に切れたら切断とする。
- 知らない種類のイベントは捨てる（`response.queued`、`response.output_text.annotation.added` など）。

### 使ったトークンの量

終わりのイベントの `response.usage` から、`input_tokens` を入力の全体、`input_tokens_details.cached_tokens` をキャッシュから読んだ量（入力の内数）、`input_tokens_details.cache_write_tokens` をキャッシュに書いた量（なければ報告なし）、`output_tokens` を出力、`output_tokens_details.reasoning_tokens` を推論とする。

### 誤り

- HTTP の 500・503 はやり直せる。429 は、本文の `error.code` が利用の上限や支払いに関わるもの（OpenAI の[誤りの符号の文書](https://developers.openai.com/api/docs/guides/error-codes)の、支払い・使う額・割り当ての 429。`credit_balance_exhausted` など。実装する者が文書で符号の一覧を確かめ、定数にまとめ、確かめた一覧を報告に書く）ならやり直せない誤り、ほかはやり直せる誤りとする。`Retry-After` は既存の扱いのまま保つ。
- 401・403 は認証、400 は要求の形の誤りとする。文脈の超過（`error.code` が `context_length_exceeded`）を分ける。

### 登録

- `AdapterKind::OpenAiResponses` の `official_capabilities` を、`strict_tools`・`tool_choice_none`・`parallel_tool_calls` を真、`reasoning_replay` を `SameDestination` とし、ほかを既定とする。
- `agent/transport.rs` の振り分けに加え、今の `invalid()` を除く。

## 触るファイル

- `crates/benitoiteh/src/provider/`（新しいファイル、`mod.rs`、`types.rs`、要れば `sse.rs`・`ids.rs`）
- `crates/benitoiteh/src/agent/transport.rs`、要れば `src/agent/mod.rs`（セッションの ID を渡すとき）
- `crates/benitoiteh/tests/provider_openai_responses.rs`（新）、`tests/support/`

## テスト

`tests/provider_openai_responses.rs` で、模擬の提供者を通して確かめる。SSE の台本は、OpenAPI の定義の例の形（`event:` の行と `data:` の行、`sequence_number`）で書く。

- 要求の本文: `instructions`、`store: false`、`include`、`reasoning`、`prompt_cache_key`、平らな道具の定義と `strict`、`tool_choice`、`max_output_tokens` の有無。キーがないときに `Authorization` を送らない。
- 推論と道具の呼び出しを含む応答の後の要求で、`output` の項目が順序と欄を変えずに並び、その後に `function_call_output`（`call_id` を使う）が続く。`added` の項目の `encrypted_content` が欠けていても、`done` の項目のものが使われる。
- 接頭辞のない `id` が除かれ、接頭辞のあるものが残る。
- `reasoning_replay` が `None` のとき、`reasoning` の項目を送らない。
- 違う送り先の応答の変換（推論を送らない、`call_id` を作る、`id` を付けない）。
- 複数の道具の呼び出しが、引数の差分を交互に受け取っても、`output_index` ごとに正しく組み立てられる。
- 終わりの分類: `completed`、`incomplete` の `max_output_tokens` と `content_filter`、`refusal`、`failed`、`error` のイベント、終わりの前の切断、`:` の注釈の行。
- 使ったトークンの量の対応（`cache_write_tokens` があるときとないとき）。
- 誤りの分類: 500、503、429 のやり直せるものとやり直せないもの、401、`context_length_exceeded`。
- `exec` の端から端までのテスト（既存の `openai-chat` のものに倣い、道具を一度呼んで終わる一連の処理）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、作った公開の関数のシグネチャと、セッションの ID の渡し方を書く。
