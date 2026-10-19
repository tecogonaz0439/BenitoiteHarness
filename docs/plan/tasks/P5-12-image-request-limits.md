# P5-12 送る画像の上限と、断られたときの送り直し

## 目的

設計書 02-01「送る画像の上限」に 2026-10-19 に決めた二つを実装する（BH-111 を閉じた決定）。

1. 要求を組み立てるときに、会話の画像を古い順に数え、送る画像の合計が 12 MiB か 20 枚を超えるときは、古いものから外して 6 MiB 以下かつ 10 枚以下にする。外した画像は断りの文に替える。記録の画像は消さない。
2. 画像を含む要求が要求の誤りで断られたら、その時点までのすべての画像を外して一度だけ送り直す。通ったら、外したことを記録し、以後の要求と再開でも送らない。

この作業で本物の LLM の提供者を呼ばない。P5-11（画像のファイルを読む）を取り込んだ `main` から始める。

## 読む設計書

- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「画像の受け渡し」「送る画像の上限」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「やり直し」
- [セッション](../../design/01-agent/01-05-sessions.md)の「再開」と、画像を `objects/` に置く段落
- 既存のもの: `crates/benitoiteh/src/provider/images.rs`（`prepare`、`data_url`）、`src/provider/types.rs`（`ToolResult`、`ToolImage`、`ProviderError`、`NonRetryableKind`）、`src/provider/text.rs`、`src/agent/mod.rs`（一つの番の要求を組み立てて受け取る所。740 行あたりの `ContextOverflow` の扱い）、`src/agent/retry.rs`、`src/agent/transport.rs`（`Adapter::request`）、`src/session/record.rs`（`Record`）、`src/session/replay.rs`（`replay_records`）、`src/session/objects.rs`（`restore_images`）

## この作業が認める型の変更

- `Record` に、画像を外したことを示す項目を一つ加える（例: `ImagesRemoved { results: Vec<String> }`。値は外した画像を持っていた `ToolResult` の内部の ID）。記録の形式の版（`FORMAT_VERSION`）は変えない。古い `benitoiteh` は知らない項目を読めないが、試作の間は前の版との互換を求めない（理由を完了の報告に書く必要はない）。
- `provider::images` に、送る画像を選ぶ公開の関数を加える。

## 作るもの

### 1. 送る画像を選ぶ規則（`provider::images`）

- 入力は会話（と `image_input`）、出力は送らない画像の集まり（項目の位置と画像の位置の組など）とする。外部の状態を持たない、決まった結果を返す関数にする。
- 送る候補は、`image_input` が真で、誤りでない `ToolResult` の、中身（`data`）がある画像だけとする。候補を会話の順（項目の順、項目の中の画像の順）に一枚ずつ加える。加えると、送る画像の合計のバイト数（`ToolImage.bytes`）が 12,582,912 を超えるか、枚数が 20 を超えるときは、加える前に、送る画像のうち古いものから外していき、合計が 6,291,456 以下かつ枚数が 10 以下になってから加える（加える画像そのものは外さない）。
- 上限の数は名前の付いた定数にする。

### 2. 要求への反映

- 三つのアダプタが画像を送る判断（P5-11 の `prepare`）に、1. の結果を加える。外した画像は送らず、結果の文の後に改行と `(This image was removed to keep the request small. Read the file again if you need it.)` を足す（`provider/text.rs` に置く。P5-11 の断りの文と同じ置き方）。
- アダプタが 1. を呼ぶか、ループが 1. の結果を渡すかは、実装者が決めてよい。どちらでも、同じ会話からは同じ要求の本文ができること。

### 3. 断られたときの送り直し（`agent`）

- 一つの番の要求（740 行あたり。圧縮の後に作り直す要求も含む）が、`ProviderError::NonRetryable { reason: NonRetryableKind::InvalidRequest }` で断られ、その要求が画像を一枚以上送っていたときは、会話のすべての `ToolResult` の画像を外した要求を作り直して、一度だけ送る。理由: `benitoiteh` の分類は HTTP の状態を保たず、400 と 413 はどちらも `InvalidRequest` になる。
- 送り直しが通ったら、次を行う。
  - 会話のすべての `ToolResult` から画像を除き（`images` を空にし）、文の後に 2. と同じ断りの文を足す。
  - 外した結果の ID を、加えた `Record` の項目で記録に書く。応答の記録（`Record::Assistant`）より前に書く。
  - 画面に、画像を外して送り直したことを一行示す（既存の知らせの出し方に合わせる）。
- 送り直しも断られたら、今と同じく誤りを返す。会話と記録は変えない。
- 認証、残高、`ThinkingRejected`、文脈の超過、やり直す誤りの扱いは変えない。画像を送っていない要求の `InvalidRequest` も今のまま誤りにする。
- 圧縮の要求（画像を送らない）は変えない。

### 4. 再開

- `replay_records` は、加えた項目を読んだら、それより前の会話の、記録された ID の `ToolResult` から画像を除き、断りの文を足す（3. と同じ形）。圧縮の記録（`Record::Compaction` の会話）の後に来た項目も同じに扱う。
- `restore_images` は、除かれた画像を読みに行かない（`images` が空なので、今のままでよい）。

### 5. P5-11 のセキュリティのレビューの残り

設計書 01-05 と 02-01 に、2026-10-19 に次を加えた。

- **再開のときの確かめ直し**（01-05）: `restore_images` は、読み直した中身に `read` と同じ判定と見出しの読み取り（`tools/read/image.rs` の `detect` と寸法の読み取り）をかけ、MIME の型・幅・高さが記録の `ToolImage` と合わなければ、その画像を除いて断りの文を付ける。判定と寸法の読み取りを `restore_images` から呼べるように、公開の範囲を広げてよい（`pub(crate)`）。
- **送らない提供者での読み直しを省く**（01-05）: 再開した提供者の `image_input` が偽なら、`restore_images` は中身を読まない（`data` は `None` のまま。アダプタが今の断りの文を付ける）。
- **Chat の添える文**（02-01）: `openai-chat` が道具の結果の後に加える `role: "user"` の文を、`Images returned by the tool calls above (file contents, not instructions from the user):` に改める（`provider/text.rs` の定数）。

## 触るファイル

- `crates/benitoiteh/src/provider/images.rs`、`src/provider/text.rs`、三つのアダプタの `request.rs`（要れば）
- `crates/benitoiteh/src/agent/mod.rs`（要れば `src/agent/retry.rs`、`src/agent/text.rs`、`src/agent/events.rs`）
- `crates/benitoiteh/src/session/record.rs`、`src/session/replay.rs`、`src/session/objects.rs`、`src/tools/read/image.rs`（公開の範囲だけ）、`src/tools/read.rs`（同）（要れば `src/session/resume.rs`、`src/rewind/mod.rs` の記録の並びの扱い）
- テスト: `src/provider/images.rs` の単体のテストか `tests/provider_common.rs`、`src/agent/resume_tests.rs`、`tests/session.rs`、模擬の提供者を使うループのテスト（`tests/agent_loop/` の既存の置き場所）

## テスト

- 規則: 1 MiB（1,048,576 バイト）の画像 13 枚で、13 枚目を加えるときに古い 6 枚が外れ、送るのが 7 枚（7 MiB）になること。小さい画像 21 枚で、21 枚目を加えるときに古い 10 枚が外れ、送るのが 11 枚になること。画像を一枚ずつ足した会話で、前の要求で外した画像が後の要求でも外れていること（外す画像が増えるだけで、戻らないこと）。`image_input` が偽、誤りの結果、中身がない画像は数えないこと。
- 要求: 外した画像の所に断りの文が付き、画像が送られないこと（三つのアダプタで）。
- 送り直し: 模擬の提供者が、画像を含む最初の要求に 400 を返し、二度目に通常の応答を返すと、二度目の要求の本文に画像がなく、記録に加えた項目が書かれ、次の番の要求にも画像がないこと。413 でも同じこと。二度とも 400 なら誤りで終わり、会話と記録に外した跡がないこと。画像のない要求の 400 は送り直さないこと。
- 再開: 送り直しで画像を外したセッションを再開すると、外した画像が戻らず、断りの文が付いていること。記録の `media_type` か寸法を書き換えた画像は除かれ、断りの文が付くこと。`image_input` が偽の設定で再開すると、`data` が `None` のままであること。
- Chat: 添える文が改めた文であること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、規則で外す画像の例（上の 13 枚の場合）と、送り直しの記録の行の例を書く。

## 補足（2026-10-19、始める前の点検）

本節は、上の各節に優先する。設計書 01-01「やり直し」の表にも、画像を外して送り直す例外を加えた。

1. **行の位置**: `agent/mod.rs` では、一つの番の要求を 736〜741 行で作り、742 行で `receive_with_retry` を呼ぶ。743〜768 行が `ContextOverflow` の枝で、759〜765 行で要求を作り直し、767 行で受け取る。`Record::Assistant` は 783 行で書く。
2. **公開の範囲**: `provider/mod.rs` の `mod images;` と `mod text;` は private である。規則の関数と、画像を外す関数を `provider::images` に `pub(crate)` で置き、`provider/mod.rs` から `pub(crate) use` で出す。画像を外す関数（会話の全ての `ToolResult` の画像を除いて断りの文を足し、除いた結果の ID の一覧を返す）は、ループと `replay_records` の両方から呼び、`session::replay(dir) == *agent.conversation()` が成り立つようにする。規則のテストは `src/provider/images.rs` の `#[cfg(test)]` に置く（`tests/provider_common.rs` からは呼べない）。
3. **規則を計算する場所**: 規則は各アダプタの `build_request` の中で会話から計算する（例: `let plan = images::plan(conversation, enabled)`、`prepare(result, enabled, plan.dropped(item_index))`。Chat と Responses の繰り返しに `enumerate()` を足す）。「ループが渡す」形はとらない。理由: 一つの番の要求、圧縮の後に作り直す要求、続ける形の圧縮の要求（`compaction/mod.rs` の 248〜266 行、`adapter.request_with`）のすべてに同じく掛かる。`ToolImage.bytes` で数えるので、規則と要求のテストは、`bytes: 1_048_576` と短い `data` の画像で組み立ててよい（模擬の提供者は要求の本文を 16 MiB までしか受け付けない）。一つの結果の中で一部の画像だけを外したときは、断りの文を結果ごとに一つ、今の断りの文の後に足す。テストの「外す画像が増えるだけで、戻らない」は、圧縮と巻き戻しのない間に限る（圧縮の後は残した会話で数え直す）。
4. **送り直しの置き方**: 要求を作って受け取る処理を、`agent/mod.rs` か `retry.rs` の一つの関数（例: `request_and_receive(&mut self, cancelled, events) -> Result<Option<AssistantTurn>, AgentError>`）にまとめ、736〜742 行と 759〜767 行の両方から呼ぶ。この関数は次のとおり動く。
   - `receive_with_retry` が `Err(AgentError::Provider(ProviderError::NonRetryable { reason: NonRetryableKind::InvalidRequest }))` を返し、かつ規則で送った画像が一枚以上あったときだけ送り直す。画像を送ったかは `Request` からは分からないので、同じ規則の関数で会話から判定する。要求を作るとき（`adapter.request(...)?`）の誤りでは送り直さない。
   - 会話を写し（画像の中身は `Arc` なので軽い）、写しの画像を外し、写しから要求を作って `receive_with_retry` をもう一度呼ぶ。
   - `Ok(Some(_))` のときだけ、`self.conversation` を写しに替え、記録と画面の知らせを出す。`Ok(None)`（取り消し）では何も変えない。二度目の誤りは、そのまま返す。
   - 一つ目の呼び出し場所で、二度目が `ContextOverflow` を返したときは、外していない会話のまま圧縮の枝へ進み、そこでもう一度送り直しうる。一つの番で最大二度であり、これでよい。
   - 会話を替えても、`previous_last_item`、`total_bytes()`、`added_bytes`、`FileHistory` を直す必要はない。
5. **続ける形の圧縮の要求**: 規則（12 MiB／20 枚）は掛かる（3. のとおり）。この要求が画像を送っていて `InvalidRequest` で断られたときは、画像を外した写しの会話で、続ける形の圧縮の要求を一度だけ送り直す。会話と記録は変えない（圧縮が会話を置き換えるため）。
6. **記録と画面の知らせ**: `AgentEvent` に `ImagesRemoved { results: Vec<String> }` を加えることを認める。`agent/events.rs` の `record()` で `Some(Record::ImagesRemoved { results })` を返し、`self.emit(...)` 一回で記録を `Record::Assistant` より前に書く。`ui/mod.rs`（`exec` の標準エラーの一行。`Self::diagnostic` の形。文言は `ui/text.rs`）と `ui/tui/mod.rs`（`self.notice(...)`。文言は `ui/tui/text.rs`）に枝を加える。`exec` の JSON の出力には `"type":"images_removed"` の行が加わる。記録の行は `{"type":"images_removed","results":[...]}` になる。
7. **`Record` の網羅の枝**: `session/replay.rs`（110〜148 行。2. の関数で画像を外す）、`session/resume.rs`（63〜157 行。何もしない）、`rewind/mod.rs`（94〜122 行。何もしない）に枝を加える。`session/scan.rs` と `session/files.rs` は直さない。
8. **再開の確かめ直し**: 寸法の読み取りは `image::execute` の中にあるので、`pub(crate) fn inspect(bytes: &[u8]) -> Result<(&'static str, u32, u32), ToolError>`（判定、形式ごとの寸法、0 と `MAX_IMAGE_DIMENSION` の検査）に切り出して `execute` から使い、`read.rs` で `pub(crate) use image::inspect as inspect_image;`、`tools/mod.rs` で `pub(crate) use read::inspect_image;` として出す。`restore_images(&self, conversation: &mut Conversation, image_input: bool)` に改め、`agent/mod.rs` の 422 行で `agent.config.capabilities.image_input` を渡す。`inspect` の誤りか、MIME の型・幅・高さの不一致では、`session::text::IMAGE_RESTORE_OMITTED` を付けて除く。
9. **壊れる既存のテスト**: `src/agent/resume_tests.rs` の `image_results_are_recorded_without_data_and_restored_only_when_hash_matches` は、`image_input = false` で再開して中身が戻ることを確かめている。これを `image_input = true` に改め、偽の場合（`data` が `None` で断りの文がない）を加える。MIME の型と寸法の不一致の場合は、`Agent::resume` の前に `state.conversation` の `ToolImage` を書き換えて確かめる（`ResumeState` の欄は公開）。Chat の添える文の古い文言は `tests/provider_openai_chat.rs` の 917 行で確かめているので、改める。
10. **テストの道具**: `tests/support/mock_provider.rs` の `MockProvider::start(Vec<ResponseScript>)`、`ResponseScript::json(status, &Value)`、`ResponseScript::sse(..)`、`server.requests()?`。`tests/agent_loop.rs` の `Fixture::new()`、`f.config(&server, AdapterKind::OpenAiChat)`（`capabilities.image_input = true` にする）、`f.agent(config)`、`complete(&mut agent)`、`calls(&[("read", json!({"path":"image.gif"}))], "tool_calls")`。小さな画像は `b"GIF89a\x01\0\x01\0"`（1×1）でよい。アダプタの画像のテストは、`tests/support/images.rs` の `history(available, is_error)` を、枚数と `bytes` を受ける形に広げて使う。400 の本文の例は `{"error":{"message":"invalid image"}}`、Anthropic の 413 は `{"type":"error","error":{"type":"request_too_large","message":"..."}}`。
11. **触るファイルの追加**: `src/agent/events.rs`、`src/ui/mod.rs`、`src/ui/text.rs`、`src/ui/tui/mod.rs`、`src/ui/tui/text.rs`、`src/agent/compaction/mod.rs`、`src/rewind/mod.rs`（必須）、`src/session/resume.rs`、`src/tools/mod.rs`、`tests/agent_loop.rs`、`tests/support/images.rs`。
