# P5-24 プラグインの Skill の上限の残り

## 目的

P5-23 の 2 回目の再レビュー（2026-10-20）で「公開の後でよい」とした所見を直す。設計書 01-04「Agent Plugins のプラグイン」の「長さと数の上限」に加えた、示す文の大きさの上限を実装し、上限のテストの不足を補う。この作業で本物の LLM の提供者を呼ばない。

## 読む設計書

- [文脈の管理](../../design/01-agent/01-04-context.md)の「利用者の Agent Skill」「Agent Plugins のプラグイン」（「長さと数の上限」「記録と表示」）
- [P5-23](P5-23-plugin-skills.md)の補足 13 と 14
- 既存のもの: `crates/benitoiteh/src/context/skills.rs`（集める流れ、`Collector::precut`、段ごとの枠）、`src/context/plugins.rs`、`src/context/plugins/text.rs`、`src/ui/text.rs`（表示の行）、`tests/user_skills.rs`、`tests/user_skills/plugins.rs`、`tests/exec/skills.rs`

## 作るもの

### 1. 示す文の大きさの上限（再レビューの低-A）

- 一つのプラグインの通知（知らない欄、`extensions`、`version`、`mcp.json`）を、一行にまとめる。今は `context/skills.rs` の 681-691 行あたりで、プラグインごとに最大 4 件を `report` を通さずに直に入れている。まとめた行は、通知の文を `; ` でつなぐなどして、一つのパスと、その後の通知の並びにする。
- 段（利用者・プロジェクト）ごとに、通知の行と飛ばしたものの行（`skipped_skills`・`skipped_plugins`）を、合わせて 64 KiB までを一件ずつ示して記録に書く。数えるのは、示す文の UTF-8 のバイト数とする（JSON の逃がしの前）。今の 100 件の枠と両方を掛け、どちらかに達したら、残りは数だけを示す。数を示す今の一行（`N more skipped skills and plugins for this source are not listed; ...`）に、通知の数も含めるか、通知の数の行を別に一行置く。
- 数でまとめた行（プラグインの数の上限、候補の数の上限、示さなかった数）は、今と同じく枠に入れず、必ず示す。これらの行もパスを含むので、段ごとに最大 3 行とする今の形を保つ。

### 2. 候補の上限で開かなかった名前を覚える（情報-G）

- 段ごとの 1,000 個の候補の上限で開かなかった候補の名前も、`Collector::precut` と同じく、後の候補との名前の重なりの判定に使う（`seen_names` に入れる）。理由: 開かずに飛ばした候補が、開いた場合と違う優先順位を生まないようにする。今は precut の飛ばしでは覚え、候補の上限の飛ばしでは覚えないので、利用者のプラグインの Skill が 1,000 を超えたとき、超えた分と同名のプロジェクトの Skill が読まれる。

### 3. テスト

- (a) 候補の上限（再レビューの低-B）: `tests/exec/skills.rs` の `many_bad_or_shadowed_plugin_skill_candidates_keep_output_and_records_small` で、`Skipped 100 plugin skill entries beyond the first 1000 for this source without opening them.` が出ることを、文と数で確かめる。加えて、最後のプラグインに名前の重ならない有効な Skill を置き、それが読まれないことを確かめる。候補の上限の処理を消すと落ちるテストにする。
- (b) 読む前に断った項目の数え方（低-C）: `.agents/plugins` に範囲の外を指すリンクを 100 個置き、その後ろ（名前の順）に有効なプラグインを 1 個置く。有効なプラグインが読まれず、`Skipped 1 entries beyond the 100-plugin limit for this source.` が出ることを確かめる。`counted` の扱いを消すと落ちるテストにする。
- (c) `a_user_skill_cut_without_opening_still_shadows_a_project_skill` の強化（低-D）: 対照の場合を先に確かめる。利用者の `z019` だけを置かない同じ配置で、プロジェクトの `z019` が読まれることを確かめてから、本題を確かめる。テストの一時の根の長さ（TMPDIR）によって、打ち切りの後の残りのバイト数が変わり、対照なしでは退行を見逃す場合がある（再レビューの計算では、根のパスの長さが 15〜21 など）。
- (d) 大きさの上限: 長いパス（1,000 バイト前後の実のパス）の下にプラグインを多数置き、各プラグインに知らない欄・長すぎる版・`mcp.json` を置く。標準エラー出力と開始の記録が段ごとの上限の範囲（例えば 2 段で 512 KiB 以下）に収まり、開始の記録が分けて置かれないことを確かめる。macOS の PATH_MAX の範囲で作れる長さとし、重いテストにしすぎない。
- (e) 名前を覚える（2）: 利用者のプラグインの Skill の候補を 1,000 を超えて置き、超えた分の候補と同名の Skill をプロジェクトに置く。プロジェクトの Skill が読まれないことを確かめる。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、まとめた通知の行と、数を示す行の例を書く。
