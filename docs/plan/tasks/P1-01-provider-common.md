# P1-01 提供者の共通の形と SSE の読み取り

## 目的

提供者の層（[提供者の層](../../design/02-providers/02-01-provider-layer.md)）のうち、アダプタに依らない部分を作る。会話の共通の形、提供者の元の項目、ストリーミングのイベント、使ったトークンの量、終わりの理由、道具の定義の内部の型と JSON Schema への変換、道具の呼び出しの ID の対応、Server-Sent Events の読み取りである。P1-02・P1-03 のアダプタと、P1-05 のセッションの記録、P1-09 のループがこの型を使う。

## 読む設計書

- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「層の分け方」「共通の形」「提供者ごとの機能の表」「道具の定義の変換」「HTTP の通信」（SSE の部分）
- [文脈の管理](../../design/01-agent/01-04-context.md)の「推論の内容の扱い」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「番の流れ」の 4（終わりの理由の分け方）
- [作業の間の約束](../10-interfaces.md)

## 作るもの

`crates/benitoiteh/src/provider/`（`mod.rs`、`text.rs`、`types.rs`、`sse.rs`、`tool_schema.rs`、`ids.rs` など。分け方は任せる）。

1. 会話の共通の形（`types.rs`）。
   - `Destination`: 提供者の設定の名前、ベースの URL、モデル。
   - `AdapterKind`: `AnthropicMessages`・`OpenAiChat`・`OpenAiResponses`（`OpenAiResponses` は P3 で作るが、型には置く）。
   - `Conversation`（会話の項目の並び）と、会話の項目 `Item`: 利用者の発言 `UserMessage`、応答 `AssistantTurn`、道具の結果 `ToolResult`。`UserMessage` は、一つの発言の中に複数の文の部分を持てる形にし、部分ごとに種類（環境の情報、依頼、操縦）を持つ。理由: 環境の情報は、最初の依頼と同じ発言の先頭に置く（01-04 の「文脈の構成」）。アダプタは部分を順に、提供者の形の文の部分（Anthropic の `text` の block など）に変える。
   - `AssistantTurn`: 共通の形の内容の並び（文、推論、道具の呼び出しを、応答の中の順に持つ `Vec<AssistantPart>`）、提供者の元の項目 `RawItems`、終わりの理由、使ったトークンの量。
   - `RawItems`: 作った `Destination` と `AdapterKind` と、提供者が返した項目の並び（`Vec<serde_json::Value>`）。順序、識別子、署名、暗号化した内容を変えずに持つ。
   - `Reasoning`: 表示用の文（なければ空）と、提供者が送り返しを求める不透明なデータ（`serde_json::Value`）。
   - `ToolCall`: 内部の ID（`benitoiteh` が付ける）、提供者の ID、名前、引数（`serde_json::Value`）、受け取った引数の文字列（検証の誤りを示すため）。Responses のために、項目の `id` と `call_id` を別々に持てる形にする（欄を `Option` にしてよい）。
   - `ToolResult`: 内部の ID、提供者の ID、結果の文、誤りの印。
   - `Usage`: 入力の全体、キャッシュから読んだ量、キャッシュに書いた量、出力、そのうちの推論を、それぞれ `Option<u64>`（報告なしは `None`）で持ち、提供者が返した値そのもの（`serde_json::Value`）も持つ。
   - `FinishReason`: 終わった、道具の呼び出し、出力の上限、拒否（分類の文字列を持つ）、誤り、取り消し、切断。
   - どの型も `serde` で JSON にでき、読み戻せる（セッションの記録に書くため）。
2. ストリーミングのイベント `StreamEvent`: 文の差分、推論の文の差分、道具の呼び出しの始まり（応答の中の番号、提供者の ID、名前）、引数の差分、終わり、使ったトークンの量、終わり（終わりの理由）。アダプタは、提供者のイベントをこの列に変え、受け取り終えたら `AssistantTurn` を組み立てる。
3. 誤りの分類 `ProviderError`: やり直せる（`Retry-After` の秒を持てる）、文脈の超過、やり直せない（認証、残高と利用の上限、要求の形、拒否された thinking の検査）、ストリームの途中の切断、応答が空。01-01 の「やり直し」の表に揃える。
4. 提供者ごとの機能の表 `Capabilities`（02-01 の「提供者ごとの機能の表」の欄）と、アダプタごとの既定の値（使わない側）。本家の口（P1-04）のときに有効にする値も、アダプタごとに定める: `anthropic-messages` は `cache_control`・`strict_tools`・`eager_input_streaming`・`tool_choice_none`・`parallel_tool_calls` を有効、`reasoning_replay` を `SameDestination` とする。`openai-chat` の本家の口は `strict_tools`・`tool_choice_none`・`parallel_tool_calls` を有効、`reasoning_replay` を `None` とする。`reasoning_replay` は `None`・`SameDestination`・`AllTurns`。
5. 道具の定義の内部の型 `ToolSpec`（`tool_schema.rs`）: 名前、説明、引数の並び（名前、型、必須か、説明）。引数の型は、文字列、整数、真偽、文字列の配列を持てば足りる。JSON Schema への変換を三つの形で持つ。
   - 普通の形（`required` に必須の引数だけ）。
   - Anthropic の strict の形（Anthropic の文書の要求する形。分からない点は、普通の形に `additionalProperties: false` を足す形にして、報告に書く）。
   - OpenAI の strict の形（02-01 の「道具の定義の変換」のとおり、すべての引数を必須にし、省略できる引数は型に `null` を加え、`additionalProperties: false`）。受け取った `null` を省いたものとして扱う関数も置く。
6. 道具の呼び出しの ID の対応（`ids.rs`）: 別の送り先へ送るときに、その提供者の形（Anthropic は `^[a-zA-Z0-9_-]+$`、長さは 64 字までとする）の ID を作る関数。元の ID のハッシュ（`sha2`）を含め、別の元の ID が同じ ID にならないようにする（02-01 の「共通の形」）。
7. SSE の読み取り（`sse.rs`）: バイト列を受け取り、行に分け、`event:` と `data:` の行（複数の `data:` 行は改行でつなぐ）、空行でのイベントの区切り、`:` で始まる行（注釈）の無視、`\r\n` と `\n` の両方を扱う。一つのイベントの大きさの上限を 16 MiB とし、超えたら誤りとして止める。入力は `P0-03` の `http` のストリーミングの要求が渡すバイト列の塊であり、塊の境目が行やイベントの途中にあってもよい。

## 触るファイル

- `crates/benitoiteh/src/lib.rs`（`pub mod provider;`）
- `crates/benitoiteh/Cargo.toml`（`sha2` を加える）

## テスト

- SSE: 塊の境目をずらした同じ入力が同じイベントの列になる。複数の `data:` 行、注釈の行、`\r\n`、上限を超えるイベント。
- 道具の定義の三つの形の JSON（期待の JSON と比べる）。OpenAI の strict の形で受け取った `null` が省いたものとして扱われる。
- ID の対応: 字を削ると同じになる二つの元の ID から、違う ID ができる。作った ID が提供者の形に合う。
- 共通の形の JSON の読み書き（`AssistantTurn` を書いて読み戻すと同じ値になる）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告の「決めたこと」に、公開した型と関数の一覧（シグネチャ）を書く。オーケストレータが 10-interfaces に写す。
