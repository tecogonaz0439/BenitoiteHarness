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

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。行の番号は `main`（`c7e24ee`）の `crates/benitoiteh/` からの相対である。

1. **今の箇所**（`src/context/skills.rs`）:
   - プラグインごとの通知を直に入れる箇所は、`Collector::search_plugins`（599-716）の中の 681-684（`manifest.notices`）と 686-691（`mcp.json`）。その後の 692 で `search_skills` を呼ぶ。通知の文の元は `src/context/plugins.rs` の `parse`（82-102）。
   - 100 件の枠は、定数 `SKIPPED_REPORT_LIMIT`（28）、欄 `reported`（335）、判定 `Collector::report`（353-362）。呼ぶのは `skip`（339-343）と `skip_plugin`（345-349）だけ。数を示す行は `report_hidden`（364-372。文は `skills/text.rs:46-50` の `skipped_hidden`）。枠に入れない数の行は `report_plugin_limits`（442-458）。
   - 候補の上限は、定数 `PLUGIN_CANDIDATE_LIMIT`（26）、欄 `plugin_candidates`（333）、判定 `search_skills` の 540-553（超えた候補は 547-551 で `continue`）。
   - `Collector::precut` は 417-440。名前を覚えるのは 574。`seen_names` は 321、同名の判定は `consider` の 375。
   - `counted` は 645、668-670、708-712。数の上限の判定は 637-642。
2. **通知も枠に数える**: まとめた通知の一件も `report` を通し、100 件と 64 KiB の両方に数える（設計書 01-04 の 100 件の文に「プラグインの通知」を加えた）。`report` は、作った `SkippedInstruction` と、通知かどうかを受け取る形にする。`skip`・`skip_plugin` は、先に `skipped(path, reason)` で項目を作ってから渡す。
3. **まとめた通知の一行**: 681-691 を、通知の文を集めて、空でなければ一件の `SkippedInstruction` にまとめる形に改め、`report` を通してから `plugin_notices` に入れる。入れる時は、`mcp.json` を確かめた後、`search_skills`（692）の前とする。文は `" "` でつなぐ（どの文も `.` で終わる。`src/agent/mod.rs:681-684` の開始の通知の結合と同じ形）。並びは、決まった文（extensions、version、mcp）を先に、知らない欄の文を最後にする。理由: 知らない欄の名前はリポジトリが選べるので、後ろに置けば、ほかの文を装いにくい。記録の型（`plugin_notices: Vec<SkippedInstruction>`）と表示のコードは変えない。
4. **64 KiB の数え方**: 定数 `SKIPPED_REPORT_BYTES = 64 * 1024` を 28 の隣に置く。数えるのは、記録の項目の `path` の UTF-8 のバイト数と `reason` のバイト数の和とし、表示の前置き（`Plugin notice: ` など）は数えない（`skipped()`（854-860）がパスを lossy な UTF-8 にした後の値を数える）。`reported` の値は、示した件数・示したバイト数・隠した飛ばしの数・隠した通知の数・最初に隠したもののパスを持つ小さな構造体にする。いったん枠を越えたら、その段の残りは、短い行でも示さず数だけにする（見つけた順の先頭を示す今の件数の枠と同じ振る舞い）。
5. **数を示す行**: 隠した通知の数も、今の一行（`report_hidden`）に入れ、段ごとに最大 3 行の形を保つ。文の例: `{n} more skipped skills and plugins and {m} plugin notices for this source are not listed; only the first 100 entries within 64 KiB are listed.`（`m` が 0 のときは今の形の文でもよい）。この文を確かめる既存のテストはない。
6. **枠に入れる数の行**: 作業の文書が「枠に入れない」とした 3 種（`report_plugin_limits` の 2 種と `report_hidden`）のほかの、ディレクトリごとの数の行（`entry_limit`（527-533・614-620）と precut のまとめ（586-591））は、ディレクトリごとに出て数に上限がないので、今どおり `skip` を通して枠に入れる。
7. **候補の上限で飛ばした名前**（作業 2）: 548-550 の `continue` の直前で、`entry`（536 で束縛済み）を、stat せずにそのまま `seen_names` に入れる。Skill の名前の規則（64 バイトまで、`[a-z0-9-]`）に合わない名前は採られることがないので、覚えずに捨てる（判定は 895-902 から関数に切り出して使う）。これで、段あたり覚える名前は最大 10 万個・一つ 64 バイトまでとなる。
8. **テスト**:
   - (a) 今の配置（プラグイン 100 個 × 11 個）では、`p090` の `s09` が 1,000 個目で、超える分は 100 個、数の行のパスは `project/.agents/plugins/p090/skills` である。`p099` の `s10` を、名前の重ならない有効な Skill（例 `t-unique`）に置き換え、合計 1,100 個を保つ。期待の文は `Skipped 100 plugin skill entries beyond the first 1000 for this source without opening them.` のまま、`t-unique` が読まれないことを確かめる。
   - (b) `tests/user_skills/plugins.rs` の `collect` の段で書く。`home/.agents/plugins/a000`〜`a099` を `root/outside` へのリンクにし（手本は 340-341）、有効な `z` を置く。`z` が読まれず、`skipped_plugins` が範囲の外の 100 件と `Skipped 1 entries beyond the 100-plugin limit for this source.` であることを確かめる。
   - (c) 今の配置では、利用者の Skill 一つの索引の大きさは `1070 + R` バイト（R は `f.root.as_os_str().len()`）で、Linux のコンテナ（`/tmp`、R=21）では対照の側が落ちる。説明の長さを `986 - R` 文字に決め、15 個がちょうど入って残りが 526 バイトになるようにする（526 バイトは `z015` には足りず、小さいプロジェクトの `z019` は入る）。そのうえで、対照（利用者の `z019` を置かない同じ配置でプロジェクトの `z019` が読まれる）と本題を確かめる。残りのバイト数の前提も assert する。
   - (d) 「512 KiB 以下」だけでは、64 KiB の枠を実装し忘れても通る（読んだプラグインの行は枠の外で約 210 KB、100 件の枠だけでも約 450 KB）。知らない欄の名前を 64 文字のもの 10 個にして、まとめた行を約 1.9 KB にする。加えて、段ごとに、示した項目の `path` と `reason` のバイト数の和が 64 KiB 以下であることと、数の行に通知の数が出ることを直に確かめる。開始の記録の一行は約 350〜400 KB になるので、`tests/exec/skills.rs:377-381` の「一行 256 KiB 以下」は使えない。開始の記録が分けて置かれないことと、一行が 512 KiB 以下であることを確かめる。
   - (e) 利用者のプラグイン `a` の `skills/` に空のディレクトリを 1,000 個置き、`b` に有効な `dup` を、プロジェクトの `.agents/skills/dup` に小さな `dup` を置く。プロジェクトの `dup` が、同名の理由（`DUPLICATE_NAME`）で飛ばされることを確かめる。空のディレクトリは警告の行を出さず、100 個の上限にも触れない。
   - まとめた一行で期待が変わるテスト: `tests/user_skills/plugins.rs` の 296（件数 3 が 1）、298-299（`ends_with`）、307-311（等値の比較）、`tests/exec/skills.rs:105-111`（`plugin_notices` の件数 2 が 1）。
9. **長いパス**: macOS の PATH_MAX は 1024（NUL を含む）で、絶対パスは 1023 バイトまで、要素一つは 255 バイトまでである。いちばん長い `<プラグインの根>/skills/<名前>/SKILL.md` も 1023 バイトに収める。プラグインの根の実のパスを 900〜950 バイトに揃え、足す長さを実行時に `目標 − 根の長さ` で計算し、200 バイト以下の要素に分ける。長い実のディレクトリを範囲の中に置き、`.agents/plugins/pNNN` をそこへのリンクにする（最後の要素を `pNNN` にして名前の一致を満たす。範囲の中のリンクの手本は `tests/user_skills/plugins.rs:395-406`）。Linux のコンテナには TMPDIR が渡らず `/tmp` になる。コンテナは root で走るので、飛ばす候補は権限でなく、範囲の外へのリンクか書式の誤りで作る。
10. **補助**: `tests/user_skills.rs` の `Fixture`（21-125。`home`、`project`、`write`、`skill`、`collect`）、`tests/user_skills/plugins.rs` の `Fixture::plugin`・`manifest`・`with_plugins`（9-36）、1,000 個を超えるディレクトリの作り方（557-559）、exec の入口の `tests/support/exec_fixture.rs`（`Fixture::new`、`command`、`records`、`lines`、`Running`）、`list_sessions` の使い方（`tests/exec/skills.rs:392-404`）。
