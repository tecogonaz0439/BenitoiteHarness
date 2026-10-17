# P4-01 圧縮

## 目的

会話の文脈が大きくなったときの圧縮を作る（設計書 01-04 の「圧縮」）。番の前の見積もりによる圧縮、文脈の超過の誤りを受けたときの圧縮、利用者が求める圧縮（`/compact` の中身。コマンドの画面は P4-08 で作る）の三つを、模擬の提供者で確かめる。

## 読む設計書

- [文脈の管理](../../design/01-agent/01-04-context.md)の「文脈の構成」「環境の情報」「文脈の大きさの見積もり」「圧縮」（2026-10-17 に「上限が分からないとき」、BH-036 を確かめるまでの扱い、Anthropic の `tool_choice` とキャッシュを加えた）「推論の内容の扱い」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「やり直し」（文脈の超過は一度だけ圧縮してやり直す）、「取り消し」
- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「提供者ごとの機能の表」（`tool_choice_none`、`reasoning_replay`、`context_window`・`max_output_tokens`）と、各アダプタの `tool_choice` の節
- [セッション](../../design/01-agent/01-05-sessions.md)の「セッションの記録」
- 既存のもの: `src/agent/`（`mod.rs` の `run_inner`・`receive`・`previous_last_item`・`token_limit_reached`、`retry.rs`、`transport.rs` の `Adapter::request`、`text.rs` の `CONTEXT_OVERFLOW`、`events.rs` の `AgentEvent::Compaction`）、`src/context/mod.rs`（`estimate_tokens`、システムプロンプトの組み立て）、`src/provider/`（各アダプタの `build_request` と `RequestOptions` の `disable_tools`、`raw_items` から同じ送り先の番を作る処理、`Capabilities`、`ReasoningReplay`）、`src/session/`（`Record::Compaction`、`replay`、`Summary::add_turn`）、`src/ui/mod.rs`（イベントの出力）、`prompts/`、`tests/agent_loop.rs`、`tests/support/mock_provider.rs`

## この作業が認める型の変更

00-common の「止まるとき」の「10-interfaces の型を変える必要がある」に当たる次の変更は、この作業で行ってよい。10-interfaces はオーケストレータが更新する。

- `UserPartKind` に要約の種類（`Summary`）を加える。
- `Record::Compaction` に `#[serde(default)]` の欄を加える。
- `AgentEvent::Compaction`（今は欄がない）に欄を加える。
- `Summary` に `#[serde(default)]` の欄を加え、使用量だけを足す関数を加える。
- 道具が空の要求で、`tools` などの鍵を送らないように各アダプタの要求の組み立てを変える。

`AssistantTurn` などの会話の型に、推論を除いた印の欄を加えることはしない（5 の除き方を使う）。

## 作るもの

### 1. 見積もりと時機

- `Agent` の欄に、最後に提供者が報告した使用量（`Option<Usage>`）と、その報告より後に会話へ加えた内容のバイトの数を持つ。足すバイトは、報告を受けた応答そのもの（文、道具の呼び出しの引数、送り返す推論の文）と、その後に加えたすべての項目（道具の結果、操縦と後続の発言、注意の文）とする。見積もりは `context::estimate_tokens` で行う。会話の中の `AssistantTurn.usage` から最後の使用量を求めない（圧縮で残した番は、圧縮の前の大きな会話の使用量を持つ）。
- 報告がないときの「すべて」は、システムプロンプト、道具の定義の JSON、会話の項目のバイトの数の和とする。項目のバイトの数は、文の部分、道具の呼び出しの引数、道具の結果、送り返す推論の文の UTF-8 のバイトの数の和とする（2 の残す範囲の計算も同じ数え方を使う）。
- `Agent` の欄に、このセッションで報告された入力の全体の最も大きい値を持つ（01-04 の「上限が分からないとき」の代わりの値に使う）。
- 番の前（`run_inner` で `TurnStart` を出す前）に、見積もりが「`context_window` − 予備」を超えたら圧縮する。予備は `max_output_tokens`（設定がなければ 32,000）に 16,384 を足した値とし、`context_window` の半分を超えない値に抑える。`context_window` が設定されていなければ、この判定では圧縮しない。
- 番の前の圧縮は、一つの番で一度だけとする。圧縮の後も値を超えるときは、そのまま要求を送り、超過は 6 に任せる。要約する側が空（3 の「要約する側」）なら圧縮しない。理由: システムプロンプトと道具の定義だけで値を超える設定では、毎番の圧縮を繰り返す。
- 道具の呼び出しに結果がまだ対応していない時点では圧縮しない（番の前にだけ判定するので当たらないことを、コメントに書く）。

### 2. 残す範囲と要約する側

- 会話の最初の発言（環境の情報を持つ `Item::UserMessage`）は、残す範囲にも要約する側にも含めない。前の圧縮の要約は最初の発言の中にあり、要約する側の先頭として扱う（3 の会話を文字にする形で `<compacted_summary>` として示す）。
- 新しい側から、約 20,000 トークンと文脈の上限の 4 分の 1 の小さい方の量の項目を残す。文脈の上限は `context_window` とし、設定がなければ、超過したときの圧縮では 01-04 の「上限が分からないとき」の代わりの値、ほかの圧縮では 20,000 だけを使う。項目の量は、1 のバイトの数を 3 で割って数える。
- 残す範囲の先頭を、道具の結果（`Item::ToolResult`）にしない。計算した先頭が道具の結果なら、後ろへずらして、利用者の発言か応答の番から始める。残すものがなくなってもよい。
- 要約する側は、最初の発言の後から残す範囲の前までの項目と、最初の発言の中の前の要約とする。要約する側に項目が一つもないときは圧縮しない（番の前は何もしない。利用者が求める圧縮は「圧縮するものがない」と返す。超過したときは一連の処理を止め、`context_overflow` とする）。
- Anthropic のアダプタで、会話の最後の `AssistantTurn` が道具を呼んでいて、その後に利用者の依頼（`UserPartKind::Request` の部分を持つ発言）がないとき（道具の呼び出しが続いている途中）は、残す範囲を設けない（01-04 の BH-036 の扱い）。
- 残す範囲を設けない圧縮を、理由によらず呼べる形にしておく（再開の P4-02 が、DeepSeek で送り先が記録と違うときに使う。01-04 の「推論の内容の扱い」）。

### 3. 要約の作り方

- **会話に続けて求める形**: 今の会話をそのまま送り、末尾に要約を求める利用者の発言を加える。システムプロンプトと道具の定義は今のものと同じにし、`previous_last_item` は今の値を渡す。
  - `tool_choice_none` が真で、アダプタが Anthropic でないときは、`disable_tools: true` で要求する。
  - Anthropic のときは、`tool_choice` を変えずに要求する（01-04 の「要約の作り方」。`tool_choice` を変えると会話の部分のキャッシュを失う）。
  - `tool_choice_none` が偽の提供者では、この形を使わず、別の要求の形を使う。
- **別の要求の形**（`tool_choice_none` が偽のとき、超過したときの圧縮、会話に続けて求める形が使えなかったとき）: Pi の作り方とする。システムプロンプトを要約用の文（`prompts/compaction-system.md`）、道具を空とし、要約する側を文字にして `<conversation>` と `</conversation>` で囲んだものと、要約の依頼を一つの利用者の発言として送る。`previous_last_item` は `None` を渡す。`Adapter` に、システムプロンプトと道具の一覧を指定して要求を作る口を加える（例: `Adapter::request_with(system: &str, tools: &[ToolSpec], conversation, ...)`）。
- 道具が空の要求では、各アダプタは `tools`・`tool_choice`・`parallel_tool_calls` の鍵を送らない。
- 会話を文字にする形: 項目ごとに、`[User]`・`[Assistant]`・`[Tool call <名前>] <引数の JSON>`・`[Tool result <名前>]`（誤りなら `[Tool error <名前>]`）の印の行に続けて中身を書く。推論の内容と環境の情報は入れない。前の要約は `<compacted_summary>` で囲んで先頭に置く。超過したときの圧縮では、4 KiB を超える道具の結果を、先頭の 2 KiB と末尾の 2 KiB（UTF-8 の文字の途中で切らない）に縮め、間に除いたバイトの数を示す行を置く。
- 要約の依頼の文は `prompts/compaction.md` に置く。
  - 01-04 の「要約の形」の見出し（`## Goal`、`## Constraints & Preferences`、`## Progress`（`### Done`・`### In Progress`・`### Blocked`）、`## Key Decisions`、`## Errors and Fixes`、`## Next Steps`、`## Critical Context`）をそのまま並べる。
  - ファイルのパス、関数の名前、診断のコード、誤りの文を正確に残すこと、前の要約（`<compacted_summary>`）があれば一つにまとめることを指示する。
  - 会話に続けて求める形では、会話の最も新しい部分は要約の後に原文のまま残ることを添える。
  - 末尾に `Do not call any tools while writing this summary; respond with text only.` を置く。
  - 利用者の指示（`/compact` の引数）があれば、依頼の末尾に `Additional instructions from the user:` に続けて加える。超過したときの圧縮で項目を除いたときは、除いたことを依頼に書く。
  - 英文はオーケストレータが取り込む前に見直す。
- 要約は、応答の文の部分をつないだものとする。応答が道具を呼んだら、呼び出しは実行せずに捨てて文だけを使う。文が空なら、会話に続けて求める形では別の要求の形で一度だけ作り直し、別の要求の形では圧縮の失敗とする。
- 要約の要求は、`receive_with_retry` のやり直しと取り消しの印を使う。差分のイベント（`TextDelta`・`ReasoningDelta`）を出さない（`receive` に差分を出すかの引数を加えるなど）。`TurnStart`・`TurnEnd`・`Record::Assistant` を出さず、番の番号を進めない。`Retry` のイベントは出してよい。
- 要約の要求の使用量は、集計の番の数に数えず、トークンの量だけを足す（`Summary` に使用量だけを足す関数を加える）。`max_run_tokens` の判定には含める。
- 会話に続けて求める形の要約の要求そのものが文脈の超過で終わったら、別の要求の形で一度だけ作り直す。
- 圧縮が失敗したとき（やり直せない誤り、別の要求の形の空の文、別の要求の形の超過）は、会話を変えずに一連の処理を止める。`AgentError` に圧縮の失敗の種類を加え、`AgentEvent::Error` の `kind` は `compaction` とする。番の前の圧縮が失敗したときも止める（そのまま送っても超過する見込みが大きい）。

### 4. ファイルの一覧

- `Agent` は、成功した（`is_error` が偽の）`read`・`write`・`edit` の呼び出しの `path` の引数を、読んだファイルと変えたファイルの集合（`BTreeSet<String>`）に覚える（`bash` は数えない）。集合は圧縮で空にせず、セッションの全体で積み上げる。
- 記録から二つの集合を作り直す関数を `session` か `agent` に置く（再開の P4-02 で使う）。`Record::ToolCall` と `Record::ToolResult` を呼び出しの ID で組にし、結果の `is_error` が偽のものだけを数える。`Record::Compaction` で集合を空にしない。
- 要約の後に、`<read-files>` と `<modified-files>` で囲んだ一覧を、一行に一つのパスで加える（変えたファイルは読んだファイルの一覧から除く）。空の一覧は書かない。制御の文字か `<`・`>` を含むパスは、JSON の文字列の形（`"..."`）で書く。

### 5. 圧縮の後の会話

- 新しい会話は、先頭の利用者の発言に、今の最初の発言の `Environment` の部分のすべてを順に置き、続けて要約の部分（`UserPartKind::Summary`。`<compacted_summary>` と `</compacted_summary>` で囲んだ要約とファイルの一覧）を置き、その後に残した項目を並べる。残した範囲の先頭が利用者の発言なら、その発言の部分の前に、これらの部分を入れて一つの発言にする。
- 信頼しない文（要約の文、会話を文字にしたものの中の道具の結果とファイルの中身）の中の `</compacted_summary>`・`</conversation>`・`</read-files>`・`</modified-files>`（ASCII の大文字と小文字を区別しない）は、`<` を `&lt;` に変えて無害化する。理由: `benitoiteh` が書いた枠の外に、環境の情報や利用者の発言を装った文を置かせない。
- 要約の部分は、利用者の依頼として扱わない（後の画面と再開で、利用者の発言として示さない。2 の「利用者の依頼」の判定にも数えない）。
- 推論の内容: `reasoning_replay` が `same-destination` なら、残した `AssistantTurn` から次のものを除く。除いた会話を以後の要求に使い、`Record::Compaction` にも書く。
  - `parts` の `Reasoning`
  - Anthropic の `raw_items` の `thinking`・`redacted_thinking` のブロック
  - Responses の `raw_items` の `type: "reasoning"` の項目と、同じ番の残る項目の `id`（対応する推論の項目のない `id` を送らない）
  - Chat の `raw_items` の一つの項目から、`reasoning_content`・`reasoning` の鍵
  - `all-turns` なら、残した番の推論の内容を除かない。`none` は今までどおり（アダプタが送らない）。
- 圧縮の後に、`Record::Compaction` に新しい会話の全体を書く（`replay` はこの行で会話を置き換える）。欄に、理由（`CompactionReason` の `threshold`・`manual`・`overflow`。snake_case で直列化し、イベントと共有する）、要約した項目の数、残した項目の数を `#[serde(default)]` で加える。記録の形式の版は 1 のままとする。
- 圧縮の後は、最後の使用量を「報告なし」に戻し、`previous_last_item` を `None` に戻す。

### 6. 超過したときの圧縮

- 番の要求が `ProviderError::ContextOverflow` で終わったら、`run_inner` の受け取りの直後で捕まえる（`retry.rs` は変えない）。その番の中で一度だけ、超過したときの圧縮（別の要求の形）を行い、同じ番の番号のまま、`TurnStart` を出し直さずに要求を作り直して送る。圧縮の後の要求も超過したら、一連の処理を止める。
- 要約する側を文字にしたものと依頼の見積もり（バイトの数を 3 で割る）が、文脈の上限（2 のとおり）の半分に収まるまで、3 の縮め方を使い、それでも収まらなければ古い側の項目から除く。それでも収まらないときは、一連の処理を止める。
- 止めたときの理由の文（`agent/text.rs` の `CONTEXT_OVERFLOW` を改める。今の「Context compaction is not implemented in P1.」を除く）と、`AgentEvent::Error` の `kind` は `context_overflow` のままとする。

### 7. 利用者が求める圧縮

- `Agent` に、一連の処理の外から呼ぶ口を加える（例: `pub fn compact(&mut self, instructions: Option<String>, cancelled: Arc<AtomicBool>, events: SyncSender<AgentEvent>) -> Result<CompactReport, AgentError>`）。P4-08 の `/compact` がこれを呼ぶ。
- `CompactReport` は、結果の種類（済んだ、圧縮するものがない、取り消した、失敗した）、要約の要求の使用量、見積もりの前と後、要約した項目の数と残した項目の数を持つ。一連の処理の外なので、使用量はこの値で返し、セッションの記録には `Record::Compaction` を書く。

### 8. 取り消し

- 圧縮は `TurnStart` の前（6 では番の中）に行う。要約の要求の途中で取り消されたら、会話を変えずに止める（一連の処理は `Outcome::Cancelled`、`compact` は取り消した）。

### 9. イベントと集計

- `AgentEvent::Compaction` を、`{ reason: CompactionReason, summarized_items, kept_items, estimate_before, estimate_after }` の欄を持つ形に改める。`record()` は今までどおり `None` を返し、`Record::Compaction` はループが別に書く。
- `src/ui/mod.rs` の出力: `--json` では他のイベントと同じく一行の JSON にする。JSON でないときは、標準エラー出力に一行（例: `Compacted the conversation (threshold): 12 items summarized, 4 kept.`。英文は `ui/text.rs` に置く）を示す。
- 集計（`Summary`）に圧縮の回数を `#[serde(default)]` の欄で加える。

## 触るファイル

- `crates/benitoiteh/src/agent/`（`mod.rs`、`transport.rs`、`events.rs`、`text.rs`、圧縮のための新しいモジュール。`retry.rs` は変えない）
- `crates/benitoiteh/src/context/`（見積もり、会話を文字にする関数を置くなら）
- `crates/benitoiteh/src/provider/`（`UserPartKind`、道具が空の要求の鍵）
- `crates/benitoiteh/src/session/`（`Record::Compaction` の欄、集計、記録からファイルの一覧を作る関数）
- `crates/benitoiteh/src/ui/`（`mod.rs`、`text.rs`）
- `crates/benitoiteh/prompts/compaction.md`、`prompts/compaction-system.md`（新）
- `crates/benitoiteh/tests/`（新しいテストは `tests/compaction.rs` にしてよい）、`tests/agent_loop.rs`、`tests/session.rs`、`tests/exec.rs`

## テスト

模擬の提供者（`tests/support/mock_provider.rs`）で、送った要求の本文を確かめる。模擬の提供者は API に依らないバイト列を返すので、各アダプタの SSE（Anthropic の thinking のブロックと署名、Responses の reasoning の項目を含む）を手で作る。用意した応答が尽きるとやり直しが走るので、応答の数を合わせる。番の前の圧縮のテストは、通常モードで、模擬の応答の使用量（`input_tokens` など）で見積もりを決める（制限モードはシステムプロンプトが大きい）。

- 番の前の圧縮:
  - `context_window` を小さく設定し、道具の呼び出しを重ねると、要約の要求が送られる。その要求は、前の要求の会話を先頭にそのまま含み、末尾に要約の依頼がある。Chat と Responses では道具を呼ばせない指定を持ち、Anthropic では `tool_choice` が前の要求と同じである。
  - 次の番の要求は、環境の情報と `<compacted_summary>` を含む最初の発言と、残した項目だけを持つ。
  - 要約の要求の間に `text_delta` が出ず、集計の番の数が増えない。
  - `context_window` が小さく、圧縮の後も値を超えても、一つの番で圧縮は一度だけである。
- `context_window` を設定しないときは、番の前の判定で圧縮しない。
- 残す範囲の先頭が道具の結果にならない。最初の発言は二重にならない。
- `tool_choice_none` が偽の提供者では、道具を持たない別の要求（`<conversation>` を含む）で要約し、その要求の本文に `tools`・`tool_choice`・`parallel_tool_calls` の鍵がない。
- 要約の応答が道具を呼んでも実行されず、文だけが要約になる。文が空なら別の要求の形で作り直す。
- 超過したときの圧縮:
  - 番の要求に文脈の超過の誤りを返すと、道具を持たない別の要求で要約し、同じ番をやり直して完了する。やり直しの要求も超過すると、一連の処理が止まり、理由が `context_overflow` になる。
  - 大きい道具の結果が先頭と末尾に縮められる。
  - `context_window` を設定せずに超過したとき、報告された入力の量の最大の値から、残す量と縮める目標が決まる。
  - 最初の要求で超過したとき（要約する側が空）は、要約の要求を送らずに止まる（既存の `authentication_and_context_overflow_stop_without_retry` の期待を、この扱いに直す）。
  - 会話に続けて求める形の要約の要求が超過すると、別の要求の形で作り直す。
- 推論の内容:
  - Anthropic（`same-destination`）では、圧縮の後の要求の残した番に `thinking` のブロックがない。`cache_control` を有効にしても、圧縮の直後の要求が組み立てられ、区切りが新しい会話の中の位置に付く。
  - Responses（`same-destination`）では、残した番に `reasoning` の項目と、その番の項目の `id` がない。
  - Chat で `same-destination` のときは、残した番に `reasoning_content` がない。`all-turns` を設定した Chat では、残した番の `reasoning_content` がある。
- Anthropic で道具の呼び出しが続いている途中に圧縮すると、残す範囲を設けない。
- ファイルの一覧: `read` と `write` を呼んだ後の要約の後に、`<read-files>` と `<modified-files>` が正しく入る。記録から作り直した集合が、ループが持つ集合と一致する。改行と `</compacted_summary>` を含む名前のパスと要約の文が無害化される。
- 二度目の圧縮の要約の要求が、前の要約を含み、まとめる指示を持つ。
- 利用者が求める圧縮: 指示の文が要約の依頼の末尾に入る。要約する側が空なら要求を送らず「圧縮するものがない」を返す。
- 要約の要求の途中の取り消しで、会話が変わらずに止まる。圧縮の失敗で、会話が変わらずに止まり、`kind` が `compaction` になる。
- 記録: 圧縮の後に `replay(セッションのディレクトリ)` が `agent.conversation()` と一致する（既存のテストの形に倣う）。前の形の `compaction` の行を読める。集計の圧縮の回数とトークンの量が合う。
- `exec` の `--json` の出力に圧縮のイベントが出る。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、`prompts/compaction.md`・`prompts/compaction-system.md` と、`agent/text.rs`・`ui/text.rs` に足した英文の全文、会話を文字にした例、加えた公開の関数と型のシグネチャを書く。
