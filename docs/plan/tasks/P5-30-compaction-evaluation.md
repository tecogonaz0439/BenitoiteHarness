# P5-30 圧縮の設定・境と残す量・記録とイベント

## 目的

BH-014 の予備と残す量を利用者の設定から変え、圧縮の量を評価の道具へ渡せるようにする。P5-29 の後に行う。Rust の設定、圧縮、記録とイベント、ループのテストを担当する。評価の道具は P5-32、長い 2 課題は P5-33 に分ける。本物の LLM を呼ばず、BH-014 は閉じない。

## 読む設計書

- [文脈の管理](../../design/01-agent/01-04-context.md)の「文脈の大きさの見積もり」「圧縮」「圧縮の値を比べる設定」「推論の内容の扱い」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「上限」「やり直し」、[セッション](../../design/01-agent/01-05-sessions.md)の「再開」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」「プロジェクトの設定ファイル」
- [評価](../../design/07-quality/07-02-evaluation.md)の「M の段の共通の条件」「M の段の圧縮」「M の段の残りの規模と費用」
- [テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」、スキル `test-audit`
- [P5-29](P5-29-completion-check.md)、既存の `crates/benitoiteh/src/agent/compaction/{mod.rs,conversation.rs}`、`src/config/mod.rs`、`src/session/record.rs`、`tools/skill-eval/{skill_eval.py,mock_provider.py,harnesses.json,SOURCE.md}`、`tasks/json_summary/`・`tasks/dir_walk/`

## この作業が認める型の変更

- `CompactionConfig { reserve_tokens: u64, keep_tokens: u64 }` を加え、`UserConfig.compaction`・`ResolvedExec.compaction`・`AgentConfig.compaction` に渡す。既定は 16,384 と 20,000。`ProjectConfig` と提供者の機能の型は変えない。
- `Header` に省略可能な `compaction` の実効値を足してよい。現行値なら省略できる。古い記録を読めるようにし、再開では今の利用者の値を使う。評価では全段で同じ設定を保つ。
- `AgentEvent::Compaction` と対応する記録に、省略可能な `usage` と `kept_estimate` を足す。`usage` はその圧縮の要約の要求とやり直しの和、`kept_estimate` は残すと選んだ範囲を再構築前に既存の `item_bytes` で数え、バイト数を 3 で切り上げて割った値とする。既存の記録には値がなくてもよい。既存の集計へ二重に使用量を足さない。

## 作るもの

- `threshold_reached` の 16,384 と、`perform_compaction` の残す量の 20,000 を設定へ置き換える。出力の上限、文脈の上限の半分と 4 分の 1、上限が分からない場合と超過した場合の扱いは、設計書の式のとおりとする。加算は飽和させ、0 も受け付ける。
- `tail_start` の道具の呼び出しと結果を分けない境界、Anthropic の道具が続く途中の残す範囲なし、送り先が変わった再開の残す範囲なしは保つ。設定でこれらを上書きしない。
- `CompactReport.usage` は受け取りの合計と混ぜず、その圧縮だけの量をイベントへ渡す。圧縮の理由、番と段、前後の見積もり、要約した項目と残した項目の数と量を評価の行へ取り込めるようにする。

## 触るファイル

- `crates/benitoiteh/src/config/mod.rs`、`src/agent/mod.rs`、`src/cli/exec.rs` と設定の型を組み立てる既存の呼び出し元
- `crates/benitoiteh/src/agent/compaction/mod.rs`・`conversation.rs`（境と残す量の計算、`kept_estimate`）
- `crates/benitoiteh/src/session/record.rs`、`src/agent/events.rs`、`src/ui/mod.rs`、`src/ui/tui/mod.rs` と、型の組み立て・再生・表示の既存の箇所
- `crates/benitoiteh/tests/agent_loop/compaction.rs` と、設定・exec・セッション・巻き戻しの既存のテスト。内部の既存のテストも、型の変更に要る範囲で直す

## テスト

`test-audit` の作成時の関門を通し、既存の設定とループの表を拡張する。

- 設定の境界で既定、0、整数の範囲、真偽と負数の拒否、プロジェクトからの変更の拒否を確認する。
- ループの境界で、模擬の提供者の固定の使用量を使い、現行値では圧縮し、予備を減らすとまだ圧縮しない見積もりを作る。次の要求の実際の会話で違いを確認する。別のケースで残す量だけを変え、残った発言と道具の呼び出しと結果の対応を確認する。
- Anthropic の道具が続く途中と送り先の変化の既存のケースを拡張し、`keep_tokens` が大きくても残さないことを確認する。依頼の境では残せることも表に加える。
- 記録とイベントの固有の契約として、省略された旧欄の読み取り、設定のヘッダへの反映、圧縮だけの使用量と `kept_estimate` が失われないことを確かめる。既存の使用量へ二重に足さない。テストのためだけに内部の関数を公開しない。

## 完了条件

- `scripts/check.sh --inside-codex` が通り、既定の値に依る既存の期待が保たれる。
- 報告に 3 条件の境と残す量、記録とイベントの欄の例、実行できなかった検査を示す。評価の道具と課題、有料の走行はこの作業の完了条件に含めない。

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。以下の推奨を、明記した修正を除いてこの作業の指示として採る。コードの行番号は、点検報告が参照した main の `d67e1af` のものである。

### 触る箇所

- **定数（`agent/compaction/mod.rs`）:**
  - `threshold_reached` は 173–187。出力の上限の既定 32,000 は 182、予備の 16,384 は 183、`window / 2` は 184。
  - 残す量は `perform_compaction` の 212–215 で、`window / 4` と 20,000 の小さい方をとる。
- **使う所:**
  - `threshold_reached` は `agent/mod.rs:828` だけから呼ぶ。
  - `perform_compaction` は、再開の 738（`keep_recent: false`）、閾値の 829、超過の 852 と、`compact`・`compact_with_options`（71–118、TUI の `/compact`）から呼ぶ。
  - 超過のときは、上限の代わりに `overflow_window`（189–195）の値を使う。
- **境（`compaction/conversation.rs`）:**
  - `tail_start` は 74–104。`budget` との比較は 88–92 で、項目 0 は残さない。
  - `tool_sequence`（106–127）が真のとき、Anthropic では残さない。
  - `item_bytes` は 13–46。`rebuild` は 310–363 で、`SameDestination` では残した番の推論を除く（352–358）。
- **使用量:**
  - `receive_summary`（421–492）が、通常の集計（459）と引数の `usage`（460）の両方に足す。
  - 呼び出し元は毎回新しい `Usage::default()` を渡す（`agent/mod.rs:748,834,857`、`compaction/mod.rs:91`）。
  - したがって `report.usage`（318）は既にその圧縮だけの量である。
- **記録とイベント:**
  - `Record::Compaction` は 332–338 で、イベント（346–355）より先に追記する。`AgentEvent::Compaction.record()` は `None` を返す（`events.rs:274`）。
  - 定義は `record.rs:384–395` と `events.rs:129–135`。
  - 字面の組み立ては `tests/agent_loop.rs:737`、`tests/rewind.rs:1196`、`tests/agent_loop/compaction.rs:583`、`tests/session.rs:430,1550,1871,2252`。
  - 表示は `ui/mod.rs:137` と `ui/tui/mod.rs:325`。
  - `docs/plan/10-interfaces.md:147–148` もこれらの欄を列挙している。
- **見積もり:** `context/mod.rs:176–181`、`compaction/mod.rs:137–171`。再開と枝の移動では `last_usage = None` にする（`agent/mod.rs:529`）。

### 残す量と記録の意味

1. **DeepSeek の口で残す範囲ができる時点:** `anthropic-messages` では、道具が続く途中は残さない（`tool_sequence`）。`kept_items > 0` になるのは、末尾が新しい依頼である段の境だけである。設計書どおりだが、テストの表に明記する。

2. **`kept_estimate` の数え方**
   - 推奨: rebuild の前の `items[end..]` の `item_bytes` の和を 3 で切り上げて割る。`tail_start` の比較と同じ量なので、`min(keep, window/4)` を超えない。
   - `SameDestination` では rebuild が推論を除くので、実際に送る量より少し多くなる。この意味を型の文書に書く。

3. **記録の欄**
   - 記録はイベントより先に書くので、記録には追記の前に分かる `usage` と `kept_estimate` だけを、`serde(default, skip_serializing_if)` で足す。
   - `estimate_before`・`estimate_after` はイベントだけに残す。
   - 番は P5-32 がイベントの順から取る。閾値と再開による圧縮は次の `turn_start`、超過による圧縮は実行中の番に対応させる（下記の修正理由）。
   - `Header.compaction` は `after_write_check` と同じく、既定と違うときだけ書く。

4. **10-interfaces との食い違い:** 作業の文書が型の変更を認めているので進め、10-interfaces.md への反映は報告の「残したこと」に書く。

### テストと後続の作業

- 圧縮の表は `tests/agent_loop/compaction.rs` の既存の型（162）に足す。予備の値に依る既存の期待（679 など）が既定の値で通ることを確かめる。
- `kept_estimate` は再構築前に選んだ範囲の量である。再構築後の送信量とは区別する。`CompactReport` にもこの値を加え、型の文書に数える時点と推論の除去による差を書く。
- P5-32 がイベントと記録を読み、段と番を対応させて評価の行へ取り込む。P5-31 の判定の実行器は、この作業と並べて進められる。

### 点検報告から修正した点

報告の「番は、イベントの直後の `turn_start` から取る」は、超過による圧縮には適用しない。理由: `agent/mod.rs:846–873` では `turn_start` を出した後に超過の圧縮を行い、同じ番で要求を送り直す。P5-32 が超過の圧縮を実行中の番に対応させ、閾値と再開による圧縮には次の番を使う。圧縮の後で上限などにより次の番を始めなければ、番は `null` として理由を残す。
