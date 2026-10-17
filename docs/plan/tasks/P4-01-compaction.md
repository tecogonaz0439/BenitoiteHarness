# P4-01 圧縮

## 目的

会話の文脈が大きくなったときの圧縮を作る（設計書 01-04 の「圧縮」）。番の前の見積もりによる圧縮、文脈の超過の誤りを受けたときの圧縮、利用者が求める圧縮（`/compact` の中身。コマンドの画面は P4-08 で作る）の三つを、模擬の提供者で確かめる。

## 読む設計書

- [文脈の管理](../../design/01-agent/01-04-context.md)の「文脈の構成」「環境の情報」「文脈の大きさの見積もり」「圧縮」「推論の内容の扱い」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「やり直し」（文脈の超過は一度だけ圧縮してやり直す）、「取り消し」
- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「提供者ごとの機能の表」（`tool_choice_none`、`reasoning_replay`、`context_window`・`max_output_tokens`）と、各アダプタの `tool_choice` の節
- [セッション](../../design/01-agent/01-05-sessions.md)の「セッションの記録」
- 既存のもの: `src/agent/`（`mod.rs` の `run_inner`、`retry.rs`、`transport.rs` の `Adapter::request`、`text.rs` の `CONTEXT_OVERFLOW`、`events.rs`）、`src/context/mod.rs`（`estimate_tokens`、システムプロンプトの組み立て）、`src/provider/`（各アダプタの `build_request` と `RequestOptions` の `disable_tools`、`Capabilities`、`ReasoningReplay`、推論の項目を除く処理）、`src/session/`（`Record::Compaction`、`replay`、`Summary`）、`prompts/`、`tests/agent_loop.rs`、`tests/support/mock_provider.rs`

## 作るもの

### 1. 見積もりと時機

- `Agent` に、最後に提供者が報告した使用量（`Usage`）と、その応答の後に会話へ加えた内容の UTF-8 のバイトの数を持たせる。見積もりは `context::estimate_tokens` で行う。報告がないときの「すべて」は、システムプロンプト、道具の定義の JSON、会話の文（各部分の文、道具の呼び出しの引数、道具の結果）のバイトの数の和とする。
- 番の前（`run_inner` で要求を作る前）に、見積もりが「`context_window` − 予備」を超えたら圧縮する。予備は `max_output_tokens`（設定がなければ 32,000）に 16,384 を足した値とし、`context_window` の半分を超えない値に抑える。`context_window` が設定されていなければ、この判定では圧縮しない（01-04 の「上限が分からないとき」）。
- 道具の呼び出しに結果がまだ対応していない時点では圧縮しない（番の前にだけ判定するので当たらないことを、コメントに書く）。

### 2. 残す範囲

- 新しい側から、約 20,000 トークンと `context_window` の 4 分の 1 の小さい方（`context_window` がなければ 20,000）の量の項目を残す。項目の量は、その項目の文のバイトの数を 3 で割って数える。
- 残す範囲の先頭を、道具の結果（`Item::ToolResult`）にしない。計算した先頭が道具の結果なら、後ろへずらして、利用者の発言か応答の番から始める。残すものがなくなってもよい。
- `reasoning_replay` が `same-destination` で、アダプタが `anthropic-messages` で、会話の最後の項目が道具の結果のとき（道具の呼び出しが続いている途中）は、残す範囲を設けない（01-04 の BH-036 の扱い）。

### 3. 要約の作り方

- **会話に続けて求める形**（`tool_choice_none` が真のとき）: 今の会話をそのまま送り、末尾に要約を求める利用者の発言を加え、`disable_tools: true` で要求する。システムプロンプトと道具の定義は今のものと同じにする（キャッシュを効かせる）。
- **別の要求の形**（`tool_choice_none` が偽のときと、超過したときの圧縮）: Pi の作り方とする。システムプロンプトを要約用の文（`prompts/compaction-system.md`）、道具を空とし、要約する側の会話を文字にして `<conversation>` と `</conversation>` で囲んだものと、要約の依頼を一つの利用者の発言として送る。`Adapter` に、システムプロンプトと道具の一覧を指定して要求を作る口を加える（例: `Adapter::request_with(system: &str, tools: &[ToolSpec], conversation, ...)`）。会話を文字にする形（役割の印、道具の呼び出しと結果の書き方）は任せるが、推論の内容は入れない。
- 要約の依頼の文は `prompts/compaction.md` に置く。01-04 の「要約の形」の見出し（`## Goal`、`## Constraints & Preferences`、`## Progress`（`### Done`・`### In Progress`・`### Blocked`）、`## Key Decisions`、`## Errors and Fixes`、`## Next Steps`、`## Critical Context`）をそのまま並べ、ファイルのパス、関数の名前、診断のコード、誤りの文を正確に残すこと、会話の先頭に前の要約（`<compacted_summary>`）があれば一つにまとめることを指示する。利用者の指示（`/compact` の引数）があれば、依頼の末尾に `Additional instructions from the user:` に続けて加える。超過したときの圧縮で項目を除いたときは、除いたことを依頼に書く。英文はオーケストレータが取り込む前に見直す。
- 要約は、応答の文の部分をつないだものとする。応答が道具を呼んだら呼び出しは捨てて文だけを使い、文が空なら圧縮の失敗とする。
- 要約の要求は、通常の要求と同じやり直し（`receive_with_retry`）と取り消しの印を使う。要約の要求の使用量も集計に足す。

### 4. ファイルの一覧

- `Agent` は、成功した `read`・`write`・`edit` の呼び出しの `path` の引数を、読んだファイルと変えたファイルの集合（`BTreeSet<String>`）に覚える（`bash` は数えない）。
- 要約の後に、`<read-files>` と `<modified-files>` で囲んだ一覧を、一行に一つのパスで加える（変えたファイルは読んだファイルの一覧から除く）。空の一覧は書かない。
- 記録（`Record::ToolCall` と `Record::ToolResult`）から二つの集合を作り直す関数を `session` か `agent` に置く（再開の P4-02 で使う）。

### 5. 圧縮の後の会話

- 新しい会話は、先頭の利用者の発言（`Item::UserMessage`）に、環境の情報の部分（今の会話の最初の発言にあるもの）と要約の部分（`<compacted_summary>` と `</compacted_summary>` で囲んだ要約とファイルの一覧）をこの順に置き、その後に残した項目を並べる。残した範囲の先頭が利用者の発言なら、その発言の部分の前に二つの部分を入れて、一つの発言にする。
- `UserPartKind` に要約の種類（例: `Summary`）を加える（アダプタは今までどおり文だけを送る）。
- 推論の内容: `reasoning_replay` が `same-destination` なら、残した番の推論の内容を以後の要求に含めない（Anthropic の `thinking`・`redacted_thinking`、Responses の `reasoning` の項目）。文、道具の呼び出し、道具の結果は含める。`all-turns` なら、残した番の推論の内容を含める。`none` は今までどおり含めない。実現の仕方（残した番の `AssistantTurn` の推論の部分と元の項目を除く、印を付けてアダプタに除かせる、など）は任せる。
- 圧縮の後に、`Record::Compaction` に新しい会話の全体を書く（`replay` はこの行で会話を置き換える）。`Record::Compaction` に、理由（`threshold`・`manual`・`overflow`）、要約した項目の数、残した項目の数を、`#[serde(default)]` の欄で加えてよい。記録の形式の版は 1 のままとする。
- 見積もりのための最後の使用量は、圧縮の後は「報告なし」に戻す（次の番は全体をバイトの数で見積もる）。

### 6. 超過したときの圧縮

- 番の要求が `ProviderError::ContextOverflow` で終わったら、その番の中で一度だけ、超過したときの圧縮を行い、同じ番の要求を作り直して送る。圧縮の後の要求も超過したら、一連の処理を止める。
- 要約する側の会話を文字にしたものの見積もりが、文脈の上限の半分に収まるまで、大きい道具の結果を先頭と末尾だけに縮め（中ほどを除いたことを示す）、それでも収まらなければ古い側の項目から除く。文脈の上限が設定されていなければ、01-04 の「上限が分からないとき」の代わりの値を使う。それでも収まらないときは、一連の処理を止める。
- 止めたときの理由の文（`agent/text.rs` の `CONTEXT_OVERFLOW` を改める。今の「Context compaction is not implemented in P1.」を除く）と、`AgentEvent::Error` の `kind` は `context_overflow` のままとする。

### 7. 利用者が求める圧縮

- `Agent` に、一連の処理の外から呼ぶ口を加える（例: `pub fn compact(&mut self, instructions: Option<String>, cancelled: Arc<AtomicBool>, events: SyncSender<AgentEvent>) -> Result<CompactReport, AgentError>`）。会話が空なら何もしないで、そのことを返す。P4-08 の `/compact` がこれを呼ぶ。

### 8. イベントと集計

- `AgentEvent::Compaction { reason, summarized_items, kept_items, estimate_before, estimate_after }` を加え、`exec` の JSON の行の出力にも出す（形は既存のイベントに倣う）。
- 集計（`Summary`）に圧縮の回数を `#[serde(default)]` の欄で加える。

## 触るファイル

- `crates/benitoiteh/src/agent/`（`mod.rs`、`retry.rs` は要れば、`transport.rs`、`events.rs`、`text.rs`、圧縮のための新しいモジュール）
- `crates/benitoiteh/src/context/`（見積もり、会話を文字にする関数を置くなら）
- `crates/benitoiteh/src/provider/`（`UserPartKind`、推論の内容を除く処理）
- `crates/benitoiteh/src/session/`（`Record::Compaction` の欄、集計、記録からファイルの一覧を作る関数）
- `crates/benitoiteh/src/cli/exec.rs`（イベントの出力）
- `crates/benitoiteh/prompts/compaction.md`、`prompts/compaction-system.md`（新）
- `crates/benitoiteh/tests/agent_loop.rs`（新しいテストは別のファイル `tests/compaction.rs` にしてよい）、`tests/session.rs`、`tests/exec.rs`

## テスト

模擬の提供者（`tests/support/mock_provider.rs`）で、送った要求の本文を確かめる。

- 番の前の圧縮: `context_window` を小さく設定し、道具の呼び出しを何度か重ねると、要約の要求が送られる。その要求は、前の要求の会話を先頭にそのまま含み、末尾に要約の依頼があり、道具を呼ばせない指定（Chat の `tool_choice: "none"` など）を持つ。次の番の要求は、環境の情報と `<compacted_summary>` を含む最初の発言と、残した項目だけを持つ。
- `context_window` を設定しないときは、番の前の判定で圧縮しない。
- 残す範囲の先頭が道具の結果にならない（道具の呼び出しと結果の組が分かれない）。
- `tool_choice_none` が偽の提供者では、道具を持たない別の要求（`<conversation>` を含む）で要約する。
- 超過したときの圧縮: 最初の要求に文脈の超過の誤りを返すと、道具を持たない別の要求で要約し、同じ番をやり直して完了する。やり直しの要求も超過すると、一連の処理が止まり、理由が `context_overflow` になる。大きい道具の結果が縮められて、要約の要求の見積もりが上限の半分に収まる。
- 推論の内容: Anthropic（`same-destination`）では、圧縮の後の要求の残した番に `thinking` のブロックがない。Responses（`same-destination`）では `reasoning` の項目がない。`all-turns` を設定した Chat では、残した番の `reasoning_content` がある。
- Anthropic で道具の呼び出しが続いている途中に圧縮すると、残す範囲を設けない。
- ファイルの一覧: `read` と `write` を呼んだ後の要約の後に、`<read-files>` と `<modified-files>` が正しく入る。記録から作り直した集合が、ループが持つ集合と一致する。
- 二度目の圧縮の要約の要求が、前の要約を含み、まとめる指示を持つ。
- 利用者が求める圧縮: 指示の文が要約の依頼の末尾に入る。空の会話では要求を送らない。
- 要約の要求の途中の取り消しで、会話が変わらずに止まる。要約の文が空なら、圧縮が失敗し、会話が変わらない。
- 記録: 圧縮の後に `replay(セッションのディレクトリ)` が `agent.conversation()` と一致する（既存のテストの形に倣う）。前の形の `compaction` の行を読める。集計の圧縮の回数が合う。
- 既存の `authentication_and_context_overflow_stop_without_retry` の期待を、新しい扱い（超過したら一度圧縮してやり直す）に合わせて直す。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、`prompts/compaction.md`・`prompts/compaction-system.md` と、`agent/text.rs` に足した英文の全文、会話を文字にする形の例、加えた公開の関数と型のシグネチャを書く。
