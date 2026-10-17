# P2-12 `benitoite_test` と、非対話の `--allow`

## 目的

制限モードでテストを実行する道具 `benitoite_test` を作り、非対話（`exec`）の制限モードで許すエフェクトを指定する `--allow` と、範囲の外の実行が 3 回続いたときの停止を作る（設計書 01-03 の「benitoite_test（制限モード）」、04-01 の「終了状態と診断の受け渡し」の `test` の結果、03-03 の「非対話での扱い」、05-01 の `--allow`）。報告、方針、確認の判定、実行と結果の部品は P2-08 のものを使う。

## 読む設計書

- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「処理系の起動」（`test` のコマンドライン、時間の上限と止め方）「終了状態と診断の受け渡し」（`test` の結果の作り方、読まないファイルのテストを件数だけにする規則、テストが通ったと示す条件、`E0512` の英文）「実行の前の検査の報告」（すべて。ディレクトリの対象と重なりのまとめ方）
- [道具](../../design/01-agent/01-03-tools.md)の「benitoite_test（制限モード）」「benitoite_check」（ディレクトリの解き方）「出力の切り詰め」（テストごとに 100 行か 4 KiB）「モードごとの道具の一覧」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「制限モードの確認」「非対話での扱い」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「上限」（止め方の形）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「オプション」の `--allow`、「終了状態」
- [セッション](../../design/01-agent/01-05-sessions.md)の「巻き戻し」（記録する時点、戻せないものの表示）
- 既存のもの: P2-08 の全体（`report.rs`、`Effect`、方針を作る関数、`benitoite_run.rs` の流れと結果の部品、`ConfirmKey`、`confirmed_effects`、`ApprovalKind::Effects` の扱い、`ToolOutput.processor_launches`、画面の事実の行）、P2-07 の `diagnostics.rs` と `benitoite_check.rs`（ディレクトリを `search_path` の判定で解く形）、`src/cli/args.rs`（`DeferredOptions`・`ExecArgs`・`ArgsError::Unavailable`。`--allow` は今は使えないオプション）、`src/cli/exec.rs`（`ApprovalPolicy` と `AgentConfig` を作る箇所）、`src/agent/mod.rs`（`AgentConfig`・`AgentError` と `kind()`、P2-07 の `CheckFixLimit` の止め方）、`src/agent/execution.rs`（`approve`）

## 前提（オーケストレータが確かめた事実）

2026-10-18 に、オーケストレータが手元の `v0.0.7` で次のことを確かめた。報告の欄と状態は P2-08 の前提にある。

- `test` の標準出力の各行は、テストごとの `{ "kind": "test", "file", "name", "function", "location", "outcome", "failure", "stdout", "stderr" }` と、最後の `{ "kind": "testSummary", "passed", "failed", "filesNotRun", "interrupted" }` である。
- `failure.reason` は次のどれかである。
  - `"assert"`: `message`、`primary`、`left`、`right` を持つ。
  - `"exit"`: `exitCode` を持ち、`die` の欄を持つことがある。
  - `"error"`: `message` を持つ。
  - `"runtime"`: 診断の欄を持つ。
- `test` の `E0512` は、終了状態 2 で、標準出力に `testSummary`（`passed` 0、`failed` 0、`filesNotRun` が入口の数、`interrupted` が偽）を、標準エラー出力に `E0512` の診断を書く。
- `SIGINT` を受けると、終了状態 130 で終わり、集計の `interrupted` が真である。
- 空のディレクトリを渡すと、報告は `status` が `"ok"` で集合が空であり、`test` は終了状態 0 で、集計の数はすべて 0 である。
- `test --check` の報告の `processShellReference` は `targets[].processShellReference` にあり、検査の誤りがあった対象では `"unknown"` である。

`outcome` の値、`location` と `primary` の形、`left`・`right` の型（文字列か `null` か）は、実装の始めに `v0.0.7` の出力と同梱の Skill の `references/inspection.md` で確かめ、報告に書く。

## この作業が認める型の変更

00-common の「止まるとき」に当たる次の変更は、この作業で行ってよい。10-interfaces はオーケストレータが更新する。

- `ToolId` に `BenitoiteTest` を加える。並べて実行しない。
- `ApprovalKind` に `TestEffects`（テストの実行の前のエフェクトの確認）を加える。単位は `ConfirmKey::Test`、題・答えのキー・拒否の文は後述の 3。`decide_in_mode` は `Effects` と同じく `TestEffects` を `decide` に通す。
- `CheckpointReason` に `BenitoiteTest` を加える。
- `AgentError` に `EffectsNotAllowed(String)` を加え、`kind()` は `"limit"` とする。`Outcome` と `cli` は変えない（既存の `AgentError::Limit` と同じ経路で、終了状態 1 で終わる）。
- `cli::args::DeferredOptions` から `allow` を除き、`ExecArgs` に `allow: Option<Vec<String>>` を加える（後述の 4）。`--project-parent` は今のとおり使えないオプションのままとする。
- `AgentConfig` に `allow_effects: Option<BTreeSet<Effect>>` を加える。`Some` は非対話（`--allow` を省いたときは空の集合）、`None` は対話を表す。

## 作るもの

### 1. `benitoite_test`

- 引数は `{ "paths": ["string"] }` とする（01-03 と同じ。`timeout` の引数は持たない）。
- 各要素を、そのモードの `read` と同じ規則で解く。ディレクトリ（リンクを辿った種類）は、P2-07 の `benitoite_check` と同じく `search_path` の判定（プロジェクトの下に限り、読まないファイルの規則を当てる）を通した実のパスとする。ほかは `ToolContext::open_file(path, false)` で開き、開いた記述子の実のパスとし、拡張子が `.bnt` でなければ道具の誤りとする。実のパスの重なりを除き、並びは指定の順を保つ。空の配列は道具の誤りとする。
- 報告は `test --check --report=json <実のパス>...`、実行は `test --diagnostics=json --allow-effects=<報告の集合> <実のパス>...` とする。P2-08 の報告の部品（`testCheckReport`）で読む。どれかの対象に検査の誤りがあれば、報告は `checkError` なので、どのテストも実行せずに診断を返す。報告を得られないときの文は P2-08 の `Could not inspect <path> with the Benitoite processor: <reason>.` とし、`<path>` は渡したパスを `, ` で繋いだものとする。
- 一度目の呼び出し、確認の要求、二度目の呼び出しで報告を得直して確認した範囲と照らすこと、巻き戻しの時点（`CheckpointReason::BenitoiteTest`）、`--allow-effects` とサンドボックスの方針を二度目の報告の集合から作ることは、P2-08 の `benitoite_run` と同じとする。二度目の報告が収まらないときは、04-01 の `E0512` の英文を返す。
- 確かめの単位は `ConfirmKey::Test(実のパスを並べ替えて重なりを除いたもの)` とする（03-03 の「指定したファイルとディレクトリの組」）。
- `Assert.Check` は、確認の判定にも画面にも数えないが、`--allow-effects` に渡す集合には残す。
- 時間の上限は 120 秒とする。止める合図は `SIGINT` とし、10 秒の後に `SIGKILL` を送る。取り消しも同じとする。
- 方針は P2-08 の 2 と同じ規則で、選んだテストの関数のエフェクトの和から作る。
- 道具の説明の英文を `prompts/tools/benitoite_test.md` に置く（1 行目が説明、残りが規則）。

  ```text
  Run Benitoite tests in .bnt files or directories in the sandbox after benitoiteh inspects their effects and asks the user to confirm them.
  Check them with benitoite_check with `tests` set to true first. You do not choose the allowed effects: benitoiteh passes the union of the selected tests' effects, including Assert.Check.
  A directory runs every .bnt file under it except package.bnt and nested packages.
  ```

### 2. 結果

- 標準出力の各行のうち、JSON のオブジェクトとして読めて `kind` が `"test"` か `"testSummary"` の行を結果とし、ほかの行を出力とする。標準エラー出力は、P2-07 の部品で診断と出力の行に分ける。
- 失敗したテスト（`outcome` が失敗のもの）ごとに、次の行を出た順に並べる。
  - 見出しの行: `FAILED <file>:<line>:<column> "<name>": <reason>: <message>`。
    - 位置は、`reason` が `"assert"` で `primary` が `null` でないときは `primary` のファイルと始まり、ほかのときは行の `file` と `location` とする。パスは診断と同じ表示用のパス（プロジェクトの下ならプロジェクトからの相対のパス、ほかは絶対のパス）とする。
    - `<reason>` は `failure.reason` の値とする。`<message>` は、`"assert"`・`"error"`・`"runtime"` では `failure.message`、`"exit"` では、`die` の欄があればその `message`、なければ `exit code <exitCode>` とする。
  - `"assert"`: `  left: <left>` と `  right: <right>` の行（値が `null` の側は省く）。
  - `"exit"` で `die` の欄があるとき: その報告を P2-07 の診断の文にした行。
  - `"runtime"`: `failure` を一件の診断として P2-07 の文にした行。
  - そのテストの `stdout`・`stderr` の欄が空でなければ、`[stdout]`・`[stderr]` の行に続けて、それぞれ末尾の 100 行か 4 KiB。
- 読まないファイルのテスト（04-01）: 行の `file` か、`"assert"` の `primary` の `file` を、`read` が開かない（P2-07 の `DiagnosticSources` で開けない）失敗したテストは、上の行を示さず、数だけを `<n> failed tests in files that benitoiteh does not read are not shown.`（一件は `1 failed test in a file that benitoiteh does not read is not shown.`）で示す。成功と失敗の数には含める。
- 失敗したテストの後に、次のものをこの順に置き、空の部分は省く。
  1. JSON として読めない標準出力の行（`[stdout]` の行に続けて）と、診断でない標準エラー出力の行（`[stderr]` の行に続けて）。それぞれ末尾の 1,000 行か 25 KiB とする（01-03 の表にない部分なので、`benitoite_run` と同じ上限を当てる）。
  2. 標準エラー出力の診断の文（P2-07 の形）。終了状態が 2 で `code` が `"E0512"` の診断があれば、その後に 04-01 の英文を添える（`benitoite_run` と同じ文）。
  3. 集計の行。集計の行があれば `Tests: <p> passed, <f> failed.`。テストが通ったと示す条件（集計の行があり、終了状態が 0 で、`filesNotRun` が 0、`interrupted` が偽）を満たせば `All tests passed.`。集計の行がなければ `No test summary from benitoite; the run may have stopped early.`。`filesNotRun` が 0 でなければ `<n> test files were not run.`（一件は `1 test file was not run.`）。`interrupted` が真なら `The test run was interrupted.`。
  4. 時間切れは `[The tests timed out after <n> seconds and were stopped.]`、取り消しは `[The tests were stopped because the user cancelled the tool call.]`、グループを抜けて残った可能性は `[Processes started by the tests may still be running.]`。
  5. `[exit code: N]`。
- テストが通ったと示す条件を満たさないときは、誤りの印を付ける。
- 切り詰めた部分があれば、標準出力と標準エラー出力の全体をそれぞれ `spill/` に書き、`bash` と同じ印を添える（切り詰めなかったときは書かない）。
- 英文は `tools/text.rs` に置く。件数の行は、単数と複数の対を一つの関数で選ぶ。

### 3. 対話の確認（テスト）

- `ui/tui/text.rs` の `approval_title` と `answer_keys` に `TestEffects` を加える。題は `╭─ Run Benitoite tests?`、答えのキーは `[y] run  [n] decline  [m] decline with a message` とする。事実の行は P2-08 の `Effects` と同じ（`Targets` には指定の順の実のパスを一行に一つ）とする。
- 拒否の文は `The user declined to run tests in <paths> with effects: <list>. Ask the user how to proceed, or change the tests.` とし、言付け付きの拒否は `text::with_user_message` で包む。`<paths>` は表示用のパスを `, ` で繋いだもの、`<list>` は P2-08 と同じ（`Assert.Check` を除き、名前の順に `, ` で繋ぎ、空なら `none`）とする。

### 4. 非対話の `--allow`

- `--allow` を、`cli/args.rs` の使えないオプション（今は `ArgsError::Unavailable`。244 行付近）から外し、`ExecArgs.allow` として受け付ける。受け付けるのは `exec` の制限モードだけとし、対話の画面と、解決したモードが通常モードのとき（`--mode normal`、設定の `default_mode`、再開した記録のモード）は、使い方の誤り（終了状態 2）とする。
- 名前は `--allow-effects` と同じ 13 の名前とし、起動のときに確かめる。`IO.All`、知らない名前、重なった名前、空の要素は、使い方の誤りとする。`Assert.Check` は受け付けて、何もしない（いつも収まるものとして扱う。03-03）。理由: 重なった名前を誤りにするのは、処理系の `--allow-effects` と同じ規則に揃えるためである。
- `exec` は、制限モードで `AgentConfig.allow_effects` を `Some(許す集合)`（`--allow` を省いたときは空の集合）とし、対話の画面と通常モードでは `None` とする。
- ループの `approve` は、`Effects` と `TestEffects` の確認の要求を、`allow_effects` が `Some` なら次のとおり扱い、確認の受け手に渡さない。
  - 事実の `effects`（`Assert.Check` を除いたもの）が、許す範囲に収まれば、許す集合をその単位の `confirmed_effects` に加えてから、`ApprovalStatus::Approved` で道具を呼び直す。許す範囲は、確認なしの範囲（`benitoite_run`・`benitoite_test` では空。`benitoite_scratch` の範囲は P2-09）と `--allow` の和とする。呼び直した道具は、P2-08 のとおり報告を得直し、加えた集合に収まるときだけ実行する。
  - 収まらなければ、実行せずに 03-03 の英文 `This non-interactive run allows only these effects: <list>. <path> uses: <list>. Narrow the uses of main, or stop and report.` を、誤りの印を付けて返す。一つ目の `<list>` は許す集合（`Assert.Check` を除く。空なら `none`）、`<path>` は表示用のパス（テストでは `, ` で繋いだもの）、二つ目の `<list>` は `effects`（空なら `none`）とする。集計の `approvals.policy_denied` に数える。
  - `allow_effects` が `None` のときは、P2-08 のとおり `decide` で決める（対話の `on-request` は確認の受け手に渡し、`never` は P2-08 の守りの文で断る）。
- 確認の要求と結果の記録は書かない（非対話で許したもの、断ったものとも）。道具の結果と集計に残る。

### 5. 続けての拒否での停止

- ループは、4 の範囲の外の英文を返した回数を、一連の処理ごとに会話の外で数える。一連の処理の始めに 0 から数える。
- 範囲に収まって実行したときに 0 に戻す。ここで実行したときとは、ループが 4 で範囲に収まると判定したときと、`benitoite_run`・`benitoite_test`（後の `benitoite_scratch`）が確認の要求を返さずに実行まで進んだとき（その出力の `processor_launches` が、報告と実行の 2 であるとき）である。報告を得られずに実行しなかった呼び出しと、ほかの道具は、回数を変えない。
- 3 回続いたら、その番の道具の結果をすべて記録した後、次の要求を送る前に一連の処理を止め、`AgentError::EffectsNotAllowed` を返す。文は `The run stopped because benitoite_run or benitoite_test was refused 3 times in a row: the effects were outside --allow.` とし、`agent/text.rs` に置く。止めたことは、P2-07 の `CheckFixLimit` と同じ経路で、記録と JSON Lines に出し、`exec` は終了状態 1 で終わる。

### 6. 巻き戻し、集計、道具の一覧

- 戻せないものの一覧に、`benitoite_test` の呼び出しを、P2-08 の `benitoite_run` と同じ選び方で含める。
- `processor_launches` は P2-08 と同じく、報告の問い合わせ一回ごとと `test` の起動一回ごとに 1 を数える。`check` は `None` のままとする。
- 制限モードの `ToolSet` に、01-03 の表の順（`benitoite_run` の後）で加える。期待のファイルを作り直す。

## 触るファイル

- `crates/benitoiteh/src/tools/`（`benitoite_test.rs`（新）、`mod.rs`、`definitions.rs`、`text.rs`）
- `crates/benitoiteh/prompts/tools/benitoite_test.md`（新）
- `crates/benitoiteh/src/benitoite/`（`testCheckReport` を読む箇所、`test` の結果の行を読む部品）
- `crates/benitoiteh/src/approval/`（`TestEffects`、拒否の文、`text.rs`）
- `crates/benitoiteh/src/agent/`（`approve` の非対話の扱い、続けての拒否の数、`AgentError`、`text.rs`、`rewind.rs`）
- `crates/benitoiteh/src/ui/tui/`（`text.rs`、`confirmation.rs`）
- `crates/benitoiteh/src/cli/`（`args.rs` の `--allow`、`exec.rs` の `AgentConfig.allow_effects`）
- `crates/benitoiteh/src/rewind/types.rs`（`CheckpointReason`）
- `crates/benitoiteh/tests/expected/`、新しい `tests/sandbox_benitoite_test.rs` ほか

## テスト

テストを書く前に、スキル `test-audit` の作成時の関門に従う。サンドボックスを要するものは `tests/sandbox_*.rs` に置き、本物の `v0.0.7` の処理系で確かめる。

- **結果の行**（純粋な関数にバイト列を渡す）: `assert`（位置が `primary` であること、`left`・`right`、`null` の側を省くこと）、`exit`（`die` の欄の有無）、`error`、`runtime` の各形。テストごとの出力の上限。JSON として読めない行。集計の各場合（通ったとき、`filesNotRun` が 0 でないとき、集計の行がないとき、`interrupted`、空のディレクトリ）と誤りの印。単数と複数の件数の行。
- **読まないファイルのテスト**（安全の性質のテスト。本物の処理系）: `.env.bnt` に失敗するテスト（識別子と文字列を含む表明）を置いたディレクトリを渡すと、そのテストの名前、文、`left`・`right`、出力が結果のどこにも現れず、件数の行だけが出ること。
- **`benitoite_test`**（本物の処理系）: ファイルとディレクトリの対象、重なりのまとめ、単位が並びと重なりに依らないこと。どれかの対象に検査の誤りがあると、どのテストも実行しないこと。`Assert.Check` を宣言したテストが実行できること。`Process.Run` を確認していないテストが `sandbox-probe` を起動できないこと（07-01）。報告と実行の間に入口のファイルを加えて `E0512` になり、英文が添えられること。時間の上限で `SIGINT` が届き `interrupted` が示されること（短い上限を渡す）。
- **確認**: 初めての実行で確認し、同じ組では確認しないこと。題、答えのキー、拒否の文。確認の要求を返した後に対象のエフェクトを増やすと、呼び直しで実行されないこと。
- **非対話**: `--allow` の範囲に収まれば実行し、超えれば英文を返すこと。3 回続けば `EffectsNotAllowed` の文で終了状態 1 で止まること。間に範囲に収まる実行を挟むと 0 に戻ること。ほかの道具と、報告を得られなかった呼び出しが回数を変えないこと。一連の処理ごとに 0 から数えること。`Assert.Check` を宣言したテストが `--allow` なしで実行できること。`--allow Assert.Check` を受け付けること。`--allow IO.All`、知らない名前、重なった名前が使い方の誤りになること。対話の画面と `--mode normal` の `--allow` が使い方の誤りになること。`--project-parent` が使えないオプションのままであること。
- **巻き戻しと集計**: 実行の前の時点が `BenitoiteTest` で記録され、戻せないものの一覧に呼び出しが入ること。`checks` の数え方。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。サンドボックスを要するテストは走らないので、オーケストレータが外で確かめる。
- 完了の報告に、道具の説明の英文、足した英文の全文、`test` の結果の行の形（`outcome` の値、`location`・`primary`・`left`・`right` の形）、結果の例（失敗の各形と集計の各場合）、加えた公開の関数と型のシグネチャ、実行ファイルの大きさの変化を書く。
