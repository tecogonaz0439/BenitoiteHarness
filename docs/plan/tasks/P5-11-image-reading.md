# P5-11 画像のファイルを読む

## 目的

設計書 01-03「画像のファイルの読み取り」、02-01「画像の受け渡し」、01-05 の画像の記録の段落（「記録を残す期間」の節の先頭）に 2026-10-19 に加えた方針を実装する。`read` が PNG・JPEG・GIF・WebP の画像を、使う提供者が画像を受け付けるときだけ、画像として LLM に渡す。画像は縮めない。

この作業で本物の LLM の提供者を呼ばない（DeepSeek と Gemini で受け付けるかの確かめ（BH-109）は、取り込んだ後に設計者の了承を得て行う）。BH-108 の判断のための、縮めるクレートを加えたときの実行ファイルの大きさの測定は、オーケストレータが別に行い、この作業には含めない。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「read」「画像のファイルの読み取り」「読んでから直す規則」
- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「共通の形」「画像の受け渡し」「提供者ごとの機能の表」
- [セッション](../../design/01-agent/01-05-sessions.md)の「記録を残す期間」の先頭の段落（画像を `objects/` に置く）と「再開」
- [文脈の管理](../../design/01-agent/01-04-context.md)の「文脈の大きさの見積もり」「圧縮」
- 既存のもの: `crates/benitoiteh/src/tools/read.rs`（`is_image`、`check_text_file`、`execute`）、`src/tools/mod.rs`（`ToolOutput`、`ToolContext`）、`src/tools/text.rs`、`src/provider/types.rs`（`ToolResult`、`Capabilities`、`official_capabilities`）、`src/config/mod.rs`（`CapabilityOverrides`、`resolved_capabilities`）、`src/agent/execution.rs`（510〜545 行あたりの `ToolResult` の組み立て）、`src/agent/mod.rs`（`construct`）、`src/agent/compaction/conversation.rs`（`item_bytes`）、`src/session/objects.rs`（`store_object`、`read_object`）、`src/provider/anthropic/request.rs`、`src/provider/openai_chat/request.rs`、`src/provider/openai_responses/request.rs`、`tests/tools_read.rs`、`tests/provider_*.rs`、`tests/config.rs`、`src/agent/resume_tests.rs`

## この作業が認める型の変更

- `provider::types` に `ToolImage` を加え、`ToolResult` に `images: Vec<ToolImage>`（`#[serde(default, skip_serializing_if = "Vec::is_empty")]`）を加える。`ToolImage` は MIME の型（`image/png` など）、SHA-256 の値（16 進の小文字 64 字）、幅、高さ、バイト数と、記録に書かない中身（`#[serde(skip)]`、`Option<Arc<[u8]>>` など）を持つ。記録の形式の版（`FORMAT_VERSION`）は変えない（欄が無い古い記録はそのまま読める）。
- `Capabilities` に `image_input: bool`、`CapabilityOverrides` に `image_input: Option<bool>` を加える。`official_capabilities` では `AnthropicMessages` と `OpenAiResponses` で真、`OpenAiChat` で偽とする（設計書 02-01 の機能の表のとおり）。
- `ToolOutput` に `images: Vec<ToolImage>`、`ToolContext` に `image_input: bool`（既定は偽）を加える。
- 既存の `ToolResult { .. }` の式（src と tests）に `images: Vec::new()` を足す変更を認める。

## 作るもの

### 1. 画像の判定と見出しの読み取り（`read`）

- `read` は、開いたファイルの先頭のバイトで画像を判定する。PNG（`\x89PNG\r\n\x1a\n`）、JPEG（`FF D8 FF`）、GIF（`GIF87a`・`GIF89a`）、WebP（`RIFF` ＋ 4 バイト ＋ `WEBP`）を画像とする。判定に使った先頭のバイトは、画像でなければテキストの処理にそのまま渡す（読み落とさない）。
- 画像と判定しなかったファイルは今までどおり扱う。拡張子が今の `is_image` の一覧に当たるファイル（BMP、SVG、壊れた PNG など）は、今までどおり `NotText` の誤りとする。
- 画像と判定したファイルは、次の順で確かめ、どれかに当たれば誤りを返す。
  1. `offset` か `limit` が指定されている。
  2. `ToolContext.image_input` が偽（`The current provider does not accept images; this file cannot be shown.` の形）。
  3. ファイルの大きさが 3,932,160 バイト（3.75 MiB）を超える（大きさと上限を示す）。
  4. 見出しから幅と高さを読めない（壊れた画像）、動く PNG（`IDAT` より前に `acTL` がある）、幅か高さが 0 か 2,000 画素を超える（寸法と上限を示す）。
- 寸法は画像を解かずに見出しから読む。PNG は `IHDR`、JPEG は SOF の印（`C0`〜`CF` のうち `C4`・`C8`・`CC` を除く）まで印を辿る、GIF は論理画面の幅と高さ、WebP は `VP8 `・`VP8L`・`VP8X` の塊から読む。読むときは添字を使わず、長さを確かめて読む（壊れたファイルで panic しない）。
- ファイルは上限 ＋ 1 バイトまでで読み、読む前と後で版（`FileVersion`）が変わっていないことを確かめる（テキストと同じ）。読んだ版を `record_read_version` で記録する。
- セッションがあるときは、中身を `store_object` で `objects/` に保存し、その値を `ToolImage` に入れる。容量の上限で保存できない（`Ok(None)`）か保存に失敗したら、画像を返さず誤りを返す（記録した画像が必ず `objects/` にあるようにするため）。セッションがないとき（道具だけのテスト）は、中身の SHA-256 を計算して入れる。
- 結果の文は `Read image file <MIME の型> (<幅>x<高さ>, <バイト数> bytes)` の一行とする。画面の一行の要約（`screen`）も同じ趣旨にする。文言は `tools/text.rs` に定数か関数として置く。
- `benitoite_check` などが使う `content_hash` の振る舞いは変えない（画像を受け付けない）。

### 2. 結果への受け渡し

- `agent/execution.rs` で `ToolOutput.images` を `ToolResult.images` に移す。中身の文を切り詰める処理（`cap_middle`）は文だけに掛ける。誤りの結果（`is_error`）には画像を入れない。
- `Agent::construct` で、`tool_context.image_input` を `config.capabilities.image_input` から設定する（再開して別の提供者に変えたときも、その提供者の値になる）。

### 3. アダプタ（設計書 02-01「画像の受け渡し」）

画像を送るのは、`self.capabilities.image_input` が真で、結果が誤りでなく、画像の中身がある（`data` が `Some`）ときだけとする。送れない画像があるときは、文の後に `(Image omitted: the current provider does not accept images.)` か `(Image omitted: the image data is not available.)` の形の一文を足して文だけを送る（文言はアダプタの共通の `provider/text.rs` に置く）。

- `anthropic-messages`: `tool_result` の `content` を、`{"type":"text","text":...}` と `{"type":"image","source":{"type":"base64","media_type":...,"data":...}}` の並びにする。画像がない結果は、今のまま文字列で送る（既存のテストの期待を変えない）。キャッシュの区切りの付け方は変えない。
- `openai-responses`: `function_call_output` の `output` を、`{"type":"input_text","text":...}` と `{"type":"input_image","image_url":"data:<MIME>;base64,<中身>","detail":"auto"}` の並びにする。画像がない結果は、今のまま文字列で送る。
- `openai-chat`: `role: "tool"` の `content` は文字列のままとし、画像を送る結果には、文の後に改行と `(The image is attached in the next user message.)` を足す。連続する道具の結果の並びが終わったところ（次の項目が道具の結果でないか、会話の終わり）で、`{"role":"user","content":[{"type":"text","text":"Attached image(s) from tool results:"},{"type":"image_url","image_url":{"url":"data:<MIME>;base64,<中身>"}}, ...]}` を一つ加える。
- base64 の符号化は、依存に既にあるクレートで行う（無ければ標準の範囲で小さく書く。依存を加えるなら大きさの変化を測って完了の報告に書く）。

### 4. 記録と再開（設計書 01-05）

- 記録の `ToolResult` には `images` の欄（中身なし）だけが書かれる。中身は 1. で `objects/` に保存済みである。
- 再開のとき（`Agent::construct` で `state` が `Some`）、会話の各 `ToolResult` の各画像について、`Session::read_object(<値>, 3,932,160)` で読み、値を確かめて `data` に入れる。読めないか値が合わないときは、その画像を `images` から除き、結果の文に `(An image from this result could not be restored from the session and was omitted.)` を足す。この処理は `session` の側に関数として置いてよい。
- 巻き戻し（`/rewind`）、圧縮の記録、`exec` の出力の形は変えない。

### 5. 文脈の見積もりと圧縮（設計書 02-01「画像の受け渡し」、01-04）

- `agent/compaction/conversation.rs` の `item_bytes` で、`ToolResult` の画像ごとに、トークンの見積もり `min(⌈幅/28⌉ × ⌈高さ/28⌉, 1568)` を、見積もりの関数の換算（`estimate_tokens` は 3 バイトで 1 トークン）に合わせたバイト数（× 3）にして足す。`image_input` が偽のときも同じに数えてよい（見積もりを多めにする側）。
- 圧縮の要求に渡す文（`[Tool result ...]`）は、結果の文だけを使い、画像を送らない（今の形のまま。結果の文の一行目が画像を読んだことを示す）。圧縮で残した側の結果の画像は、そのまま送る。

### 6. 設定

- 設定ファイルの `providers.<名前>.capabilities.image_input` を受け付ける。本家の Anthropic と Responses の送り先で既定が真、互換の口と本家の Chat Completions で既定が偽になること。

## 触るファイル

- `crates/benitoiteh/src/tools/read.rs`、`src/tools/mod.rs`、`src/tools/text.rs`
- `crates/benitoiteh/src/provider/types.rs`、`src/provider/text.rs`、`src/provider/anthropic/request.rs`、`src/provider/openai_chat/request.rs`、`src/provider/openai_responses/request.rs`
- `crates/benitoiteh/src/config/mod.rs`
- `crates/benitoiteh/src/agent/execution.rs`、`src/agent/mod.rs`、`src/agent/compaction/conversation.rs`
- `crates/benitoiteh/src/session/objects.rs`（要れば `src/session/mod.rs`）
- 既存の `ToolResult { .. }` の式があるファイル（`images: Vec::new()` を足すだけ）
- テスト: `crates/benitoiteh/tests/tools_read.rs`、`tests/provider_anthropic*.rs`・`tests/provider_openai_chat*.rs`・`tests/provider_openai_responses*.rs`（既存の置き場所に合わせる）、`tests/config.rs`、`src/agent/resume_tests.rs`、`src/agent/compaction/tests.rs`

## テスト

画像のテストの資料は、テストの中でバイト列として組み立てる（見出しだけの小さな PNG・JPEG・GIF・WebP。画像を解かないので、中身の画素は要らない）。

- `read`:
  - 4 つの形式で、寸法と MIME の型が正しく読めること。拡張子がない名前や `.txt` の名前でも画像として扱うこと。
  - `image_input` が偽なら誤りになること。`offset`・`limit` の指定、3,932,161 バイト、2,001 画素、幅 0、動く PNG、途中で切れた見出しが、それぞれ誤りになり、panic しないこと。
  - 既存の `picture.PNG`（中身が文字）が今までどおり `binary or image` の誤りになること。PNG の印で始まらない普通のテキストが今までどおり読めること。
  - セッションがあるとき、`objects/` に中身が値の名前で置かれ、結果の `images` に値が入ること。読んだ版が記録され、続く `edit` が読み直しを求めないこと。
- アダプタ: 3 つのアダプタで、画像のある結果の要求の本文が上の形になること。`image_input` が偽のときと `data` が `None` のときに画像を送らず断りの一文が付くこと。画像のない結果の本文が今と同じであること。`openai-chat` で、二つの道具の結果の両方に画像があるとき、二つの `role: "tool"` の後に `role: "user"` が一つ、画像が二つ入ること。
- 記録と再開: 画像を読んだセッションの記録の行に画像の中身（base64 も含めて）が書かれていないこと。再開すると `data` が戻ること。`objects/` の中身を書き換えると、画像が除かれ断りの文が付くこと。
- 見積もり: 1,000×1,000 の画像（36 × 36 = 1,296 トークン）と 2,000×2,000（上限の 1,568）で、`item_bytes` が文の分にそれぞれ 3,888 と 4,704 を足すこと。
- 設定: `image_input` の既定（本家の Anthropic と Responses で真、ほかで偽）と、上書きを受け付けること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、3 つのアダプタで画像のある結果を送る要求の本文の例（base64 は短く切って）と、依存を加えたならその大きさの変化を書く。

## 補足（2026-10-19、始める前の点検）

本節は、上の各節に優先する。

1. **`read` の処理の順**: `execute`（`read.rs` の 46 行）は、今は読む前に `check_text_file`（拡張子で拒む）を呼ぶので、その前に判定を置く。(a) `open_file` と `metadata`。(b) 先頭の 12 バイトを、12 バイトか終わりまで繰り返し読む（一回の `read` は短く返しうる）。(c) 画像の印に当たれば画像の処理へ進み、`check_text_file` を呼ばない。(d) 当たらなければ `check_text_file` を呼び（拡張子で `NotText`、64 MiB の上限）、先頭のバイトを最初の断片として `check_text_chunk(prefix, 0)` と `pending`・`decode_prefix` に渡し、今の繰り返しを続ける。`content_hash`（13 行）と `diagnostic_line`（339 行）は、今のまま `check_text_file` を呼ぶ。
2. **壊れた画像の二つの場合**: 印があり見出しが読めないものは、作るもの 1 の 4. の画像の誤りとする。印がなく拡張子が `is_image` に当たるもの（中身が文字の `picture.PNG` など）は、今までどおり `NotText` とする。
3. **`image_input` の置き場所**: `ToolContext` に `pub(crate) image_input: bool`（`new` で偽）と、`pub fn with_image_input(mut self, enabled: bool) -> Self` を加える。`Agent::construct` は、`tool_context.noninteractive = ...`（`agent/mod.rs` の 305 行あたり）の隣で欄を直に設定する。結合のテストは `with_image_input(true)` を使う。
4. **`ToolImage` の形と比べ方**: 記録の欄の名前を `{"media_type": "image/png", "sha256": "<16 進 64 字>", "width": <u32>, "height": <u32>, "bytes": <u64>}` に固定し、中身は `#[serde(skip)] data: Option<Arc<[u8]>>` とする。`PartialEq` と `Debug` は手で書き、中身を除いた欄だけを比べ・示す（`assert_eq!(session::replay(dir)?, *agent.conversation())` の形の既存のテスト（`agent/resume_tests.rs` の 168・172 行、`agent/compaction/tests.rs` の 172 行）が、中身の有無で落ちないため。失敗の表示に中身を出さないため）。中身が戻ることのテストは `data` を直に確かめる。`ToolImage` を `provider/mod.rs` の 13〜17 行の `pub use types::{...}` に加える。
5. **base64**: 依存を加えず、`provider` の下に小さな符号化の関数（標準の字の並び、`=` の詰め物あり）を手で書き、既知の値（RFC 4648 の試験の値）でテストする。理由: 直の依存を加えると、設計書 06-01 の依存の表を直す必要が生じる。
6. **`ToolOutput` の式**: `ToolOutput::success`（`tools/mod.rs` の 696 行）と `ToolOutput::needs_approval`（715 行あたり）に `images: Vec::new()` を足す。ほかの式は `..ToolOutput::success/error(...)` を使うので直さない。
7. **`ToolResult { .. }` の式（12 箇所）**: src の `agent/execution.rs:516`、`session/replay.rs:207`、`agent/compaction/tests.rs:122`、tests の `session.rs:129`、`provider_common.rs:308`、`provider_anthropic.rs:270`・`458`、`provider_openai_chat.rs:273`・`537`、`provider_openai_responses.rs:374`・`574`。
8. **テストの置き場所**: 提供者は `tests/provider_anthropic.rs`、`tests/provider_openai_chat.rs`、`tests/provider_openai_responses.rs`（`build_request` は三つとも公開）。再開は `src/agent/resume_tests.rs`、見積もりは `src/agent/compaction/tests.rs`（`super::conversation::item_bytes` を呼べる）、記録の行は `tests/session.rs` か `resume_tests.rs`。再開と記録のテストの `AgentConfig` は `capabilities.image_input = true` にする（今は `Capabilities::default()` で偽なので、`read` が拒む）。
9. **読んだ版のテスト**: 画像に `edit` は使えない（UTF-8 でないので `NotText`）。読んだ版は `context.read_version(&real_path)?` が `Some` であることで確かめる。
10. **上限の定数**: `pub const MAX_IMAGE_BYTES: u64 = 3_932_160;` と寸法の上限 `2_000` を `tools/mod.rs`（か `provider::types`）に置き、`read` と再開の両方で使う。
11. **誤りの形**: 画像の誤りは `ToolError::InvalidArguments(<tools/text.rs の文言>)` とする。`offset`・`limit` は、鍵があれば（`arguments.get("offset").is_some()`）指定とみなす（`offset: 1` も誤り）。
12. **共通の文言の参照**: アダプタの中の `text::` は各アダプタの `text` を指す。共通の断りの文は `crate::provider::text::...` で参照する。
13. **保存の上限**: `store_object` が `Ok(None)` を返したら、`crate::session::text::STORAGE_LIMIT` を含む誤りを返す（画面の知らせは `reserve` が既に出す）。同じ画像を読み直したときは `(値, false)` が返り、容量を重ねて数えない。
14. **動く WebP**: 設計書は動く PNG だけを除くので、動く WebP（`VP8X` の動きの印か `ANIM` の塊）は受け付け、寸法は `VP8X` の画面の大きさとする。
15. **見出しの読み方**:
    - PNG: `IHDR` は 8 バイト目からの最初の塊（長さ 13）で、幅と高さは 16・20 バイト目の大端の u32。動く PNG は、塊（長さ ＋ 型 ＋ 中身 ＋ CRC）を `IDAT` まで辿り、先に `acTL` があれば拒む。
    - JPEG: `0xFF` の詰め物を飛ばす。`D0`〜`D7`・`01`・`D8` は長さを持たない。SOF の区切りの +3 に高さ、+5 に幅（大端の u16）。
    - GIF: 6・8 バイト目の小端の u16。
    - WebP の `VP8 `: 中身の +3 に `9d 01 2a`、幅は +6 の小端の u16 の下位 14 ビット、高さは +8 の同じ。`VP8L`: 中身の 0 バイト目が `0x2f`、+1 の小端の u32 の 0〜13 ビットが幅 − 1、14〜27 ビットが高さ − 1。`VP8X`: 中身の +4 の小端の 24 ビットが幅 − 1、+7 が高さ − 1。
16. **道具の説明は変えない**: `prompts/tools/read.md`・`read-plain.md` と、`tests/expected/` の写しは変えない（設計書 01-03 の BH-110）。
17. **会話に残る画像の数**: 枚数や合計の大きさの上限は、この作業では設けない（設計書 02-01 の BH-111）。
