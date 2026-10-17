# P2-08 実行の前の検査の報告、制限モードの方針、`benitoite_run`・`benitoite_test`

## 目的

制限モードでスクリプトとテストを実行する道具 `benitoite_run` と `benitoite_test` を作る。実行の前に処理系の検査の報告（`check --report=json`・`test --check --report=json`）からエフェクトを得て、確認の要否を決め、そのエフェクトから作った方針の OS のサンドボックスの中で、同じ集合を `--allow-effects` に渡して実行する（設計書 04-01 の「実行の前の検査の報告」、01-03 の「benitoite_run（制限モード）」「benitoite_test（制限モード）」、03-02 の「制限モードの方針」、03-03 の「制限モードの確認」「非対話での扱い」）。

確認は、この作業で、判定（初めての実行か、エフェクトが増えたか）、非対話の `--allow`、確認の要求の記録を作る。対話の画面で示す事実の全体（エフェクトの説明、サンドボックスの範囲、許すプログラム、シェルの参照）は P2-10 で作る。この作業の時点の対話の画面は、P4-07 の確認の画面に、パスとエフェクトの一覧を事実として示す。

## 読む設計書

- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「処理系の起動」（`run`・`test` のコマンドライン、`--allow-effects` をいつも付けること、空の集合、標準入力、出力を終わりまで読むこと、時間の上限と止め方）「終了状態と診断の受け渡し」（`E0512` の英文、`test` の結果の作り方、テストが通ったと示す条件）「実行の前の検査の報告」（すべて）
- [道具](../../design/01-agent/01-03-tools.md)の「benitoite_run（制限モード）」「benitoite_test（制限モード）」「出力の切り詰め」（1,000 行か 25 KiB、テストごとに 100 行か 4 KiB、`spill/`）「モードごとの道具の一覧」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「制限モードの方針」「制限モードで実行を許すプログラム」「子プロセスの環境」「子プロセスの後始末」「端末からの切り離し」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」「制限モードの確認」「非対話での扱い」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「保証しないこと」（検査と実行の間の、同じエフェクトの範囲での変更）
- [セッション](../../design/01-agent/01-05-sessions.md)の「セッションの記録」（実行の始まりの行）「再開」（確認したエフェクトの記録は再開しない）「巻き戻し」（記録する時点、戻せないものの表示）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の `--allow`、「終了状態」
- 既存のもの: P2-05〜P2-07 の `benitoite` モジュール（問い合わせ、方針を作る関数、JSON の読み方、診断の文）、`src/sandbox/run.rs`（`run`・`RunLimits`・`StopSignal::Interrupt`）、`src/tools/bash.rs`（出力の末尾の保ち方と `spill/` の書き方、`Leftovers` の示し方）、`src/tools/truncate.rs`、`src/approval/`（`ApprovalKind`・`ApprovalFacts`・`decide`・`decide_in_mode`（P4-07）・`ApprovalRecord`・`Approver`・`InteractiveApprover`）、`src/agent/execution.rs`（確認の流れ、`allowed_commands`・`session_domains` の持ち方）、`src/cli/args.rs`（`--allow` は読むだけで使っていない）、`src/config/`（`RestrictedConfig`）、`src/rewind/`（`CheckpointReason`）、`src/agent/rewind.rs`（戻せないものの一覧）

## この作業が認める型の変更

- `ToolId` に `BenitoiteRun` と `BenitoiteTest` を加える。どちらも並べて実行しない。
- `ApprovalKind` に `Effects` を加える。`ApprovalFacts` に、実行の対象（`targets: Vec<PathBuf>`）、エフェクトの集合（`effects: Vec<String>`）、前に確認したときより増えたもの（`added_effects: Vec<String>`）を `#[serde(default)]` で加える（P2-10 で、サンドボックスの範囲などの欄を足す）。
- 確認したエフェクトの記録を `Agent` が会話の外に持つ（例: `confirmed_effects: BTreeMap<ConfirmKey, BTreeSet<Effect>>`）。`Invocation` に参照を渡す（`session_domains` と同じ形）。
- 非対話で許すエフェクトの集合（`--allow`）を、`Agent` の設定（`AgentConfig` か `ToolContext`）として渡す。
- `CheckpointReason` に、Benitoite の実行の前の時点（例: `BenitoiteRun`）を加える。
- `benitoite` に、エフェクトの集合から P2-05 の `RestrictedAccess` を作る関数を加える。

## 作るもの

### 1. 報告を読む

- `src/benitoite/report.rs` に、`checkReport` と `testCheckReport` を読み、04-01 の条件（終了状態 0、一つの完全な JSON、`schemaVersion` 1、`kind`、`status` が `"ok"`、`complete` が真、`executed` が偽、`aggregateEffects` が `null` でなく `basis` が `"declaredUpperBound"`、`expanded` の名前がすべて 13 の知る名前）を満たすときだけ、確定した結果（エフェクトの集合、`processShellReference`、`sources` のうち `kind` が `"user"` の表示用のパスと `contentHash`）を返す部品を作る。欄は名前で読み、並びに頼らない。欄の名前と形は、`v0.0.7` の処理系の出力と、同梱の Skill の `references/inspection.md` で確かめ、報告に書く。
- エフェクトの名前は型（例: `enum Effect` の 13 の値と、`--allow-effects` の綴りへの変換）で表す。知らない名前を一つでも含めば確定しない。
- `status` が `"checkError"` なら、報告の `diagnostics` を P2-07 の診断の文にして返す（実行しない）。ほかのとき（使い方の誤り、処理系の不具合、中断、読めない報告、知らない名前、16 MiB を超える報告）は、処理系の誤りとして理由を一行で返す（実行しない）。英文は `benitoite/text.rs`。
- 報告の問い合わせは、P2-05 の問い合わせの起動（サンドボックスの中、時間の上限 30 秒）で行う。

### 2. 方針

- エフェクトの集合から `RestrictedAccess` を作り、`sandbox::restricted_policy` で、03-02 の「制限モードの方針」の表のとおりに方針を作る。書ける場所は `File.Write` を含めばプロジェクトのディレクトリと一時ディレクトリ、含まなければ一時ディレクトリだけ。IP の通信は `Http.Connect` か `Http.Listen` を含むときだけ。起動できるプログラムは、`Process.Run` を含むときは処理系の実のパスと `restricted.allowed_programs`（P2-05 で確かめたもの）、含まないときは処理系の実のパスだけ。書ける場所での実行の拒否はいつも掛ける。読める範囲は既定（`ReadAccess::All` から読ませない場所を除く。通常モードの `normal_policy` と同じ候補を使う）。
- Linux では、P0-07 の `prepare` と P2-01 の `run` を通す。

### 3. `benitoite_run`

- 引数は `{ "path": "string", "args": ["string (optional)"], "stdin": "string (optional)", "timeout": "integer (optional, max 600)" }`。`path` は `read` と同じ規則で解き、絶対の実のパスで渡す（`.bnt` でなければ誤り）。
- 手順は 01-03 のとおり。報告を得る → 確認（後述の 5） → 巻き戻しの時点を記録する（確認の後、実行の直前。01-05） → 実行の始まりの行を記録に書く → `run --diagnostics=json --allow-effects=<報告の集合> <path> [args...]` を、2 の方針のサンドボックスの中で起動する。`--allow-effects` には、前に確認した範囲でなく、今回の報告の集合を渡す（空なら `--allow-effects=`）。`Assert.Check` は報告にあれば渡す集合に残す。
- 時間の上限は既定 120 秒、`timeout` で 600 秒まで。止める合図は `SIGINT`、10 秒の後に `SIGKILL`。取り消しも同じ。
- 結果: 終了状態、標準出力、標準エラー出力（診断を除く）、診断（P2-07 の文）。出力はそれぞれ末尾の 1,000 行か 25 KiB を返し、`spill/` に書く（`bash` と同じ部品と印）。処理系の失敗とスクリプトの終了の区別は、04-01 の「終了状態と診断の受け渡し」のとおり、標準エラー出力の処理系の形式の JSON の報告の有無で行う。`E0512` のときは、04-01 の英文（`The script changed after the effects were confirmed and now declares more effects. Call the tool again to inspect it and ask the user.`）を添える。グループを抜けて残った可能性（`Leftovers::PossiblyRemaining`）は、`bash` と同じく示す。
- 道具の説明の英文を `prompts/tools/benitoite_run.md` に置く（案を報告に書き、オーケストレータが取り込む前に見直す。確認は `benitoiteh` が行うこと、`--allow-effects` を渡さなくてよいことを含める）。

### 4. `benitoite_test`

- 引数は `{ "paths": ["string"] }`。各要素は `.bnt` のファイルかディレクトリとし、`read` と同じ規則で解いて実のパスにし、重なりを除く（並びは指定の順を保つ）。
- 報告は `test --check --report=json <paths>`、実行は `test --diagnostics=json --allow-effects=<報告の集合> <paths>`。どれかの対象に検査の誤りがあれば、報告は完全でないので、どのテストも実行せずに診断を返す。
- 結果は 04-01 の「終了状態と診断の受け渡し」の `test` の結果の作り方とする。テストが通ったと示すのは、集計の行があり、終了状態が 0 で、`filesNotRun` が 0、`interrupted` が偽のときだけとする。失敗したテストの `stdout`・`stderr` の欄は、テストごとに末尾の 100 行か 4 KiB を添える。JSON として読めない行は出力として示す。標準出力と標準エラー出力の全体は `spill/` に書く。
- 道具の説明の英文を `prompts/tools/benitoite_test.md` に置く（案を報告に書く）。

### 5. 確認の判定

- 確かめの単位（`ConfirmKey`）は、`benitoite_run` ではスクリプトの実のパス、`benitoite_test` では実のパスの組（並べ替えて重なりを除いたもの）とする（03-03 の「指定したファイルとディレクトリの組」）。
- 確認が要るのは、その単位をセッションで初めて実行するときと、報告のエフェクト（`Assert.Check` を除く）が、その単位で前に確認した集合より増えたときである。収まるときは確認しない（中身が変わっていても）。
- 対話（`InteractiveApprover`）では、要るときに `ApprovalKind::Effects` の確認を求める。事実は、対象のパス、エフェクト（`Assert.Check` を除く）、増えたもの。`summary` は `approval/text.rs` の関数で作る。許されたら、その単位の確認した集合を、今回の集合（`Assert.Check` を除く）との和にする。拒否されたら、01-03 の英文（`The user declined to run <path> with effects: <list>. Ask the user how to proceed, or change the script.`）を返す。「このセッションの間許す」の答えは作らない（確認した集合に入れることが、その単位のセッションの間の許しに当たる）。
- 非対話（`exec`）では、03-03 の「非対話での扱い」のとおりにする。`--allow` の名前は 13 の知る名前のうち `Assert.Check` を除いたもの（`IO.All` は受け付けない）とし、起動のときに確かめて、知らない名前は使い方の誤り（終了状態 2）とする。許す範囲は、確認なしの範囲（`benitoite_run`・`benitoite_test` では空。`benitoite_scratch` の範囲は P2-09）と `--allow` の和とし、`Assert.Check` はいつも収まるとする。範囲を超えれば実行せず、03-03 の英文（`This non-interactive run allows only these effects: <list>. <path> uses: <list>. Narrow the uses of main, or stop and report.`）を返す。同じ一連の処理で 3 回続いたら、処理を止めて終了状態 1 で終える。
- `decide_in_mode` が制限モードで `WebFetch` のほかを拒む今の守りは保ち、`Effects` だけを上の判定に通す。
- 確認の要求と結果を記録に書くときは、報告の `sources` のうち `kind` が `"user"` のものの表示用のパスと `contentHash` を添える（04-01）。
- 確認したエフェクトの記録は再開しない（再開した記録から作り直さない）。

### 6. 巻き戻しとの関係

- 実行の前の時点を、確認の後、実行の直前に記録する（01-05 の「記録する時点」）。
- 戻せないものの一覧（01-05 の「戻せないものの表示」）に、`benitoite_run` と `benitoite_test` の呼び出しを含める（`web_fetch` と同じく、記録の道具の名前で選ぶ）。

### 7. 道具の一覧

- 制限モードの `ToolSet` に、01-03 の表の順（`benitoite_check` の後）で加える。通常モードには加えない。期待のファイルを作り直す。

## 触るファイル

- `crates/benitoiteh/src/benitoite/`（`report.rs`（新）、`effects.rs`（新）、`text.rs`）
- `crates/benitoiteh/src/sandbox/`（制限モードの方針）
- `crates/benitoiteh/src/tools/`（`benitoite_run.rs`（新）、`benitoite_test.rs`（新）、出力の部品を `bash.rs` と共有する形、`mod.rs`、`definitions.rs`、`text.rs`）
- `crates/benitoiteh/prompts/tools/benitoite_run.md`・`benitoite_test.md`（新）
- `crates/benitoiteh/src/approval/`（`Effects` の種類、事実の欄、判定、拒否の文、`text.rs`）
- `crates/benitoiteh/src/agent/`（確認したエフェクトの記録、非対話の続けての拒否の数、`rewind.rs` の戻せないもの）
- `crates/benitoiteh/src/rewind/types.rs`（`CheckpointReason`）
- `crates/benitoiteh/src/cli/`（`--allow` の確かめと受け渡し）
- `crates/benitoiteh/src/session/`（記録の欄）
- `crates/benitoiteh/tests/expected/`、新しい `tests/sandbox_benitoite_run.rs` ほか

## テスト

サンドボックスを要するものは `tests/sandbox_*.rs` に置き、本物の `v0.0.7` の処理系で確かめる。

- 報告の受け入れ: 条件のどれか一つを欠く報告（終了状態、二つの JSON、`schemaVersion` 2、`kind` の違い、`status` が `checkError`、`complete` が偽、`executed` が真、`aggregateEffects` が `null`、`basis` の違い、知らない名前、空の標準出力）で実行しないこと（決まった出力を返す差し替えで）。空の集合として扱わないこと。
- 安全の性質のテスト（07-01）:
  - 確認の後、実行の前にスクリプトを変えて `main` のエフェクトを増やすと、処理系が `E0512` で実行を断り、英文が添えられること（報告と実行の間に書き換える差し替えの口を、道具のモジュールの中のテストで使ってよい）。
  - `File.Write` を含まないスクリプトがプロジェクトに書けないこと。`Http.Connect` を確認しないスクリプトがネットワークに接続できないこと（ループバックの待ち受けを用意して接続を試す）。
  - `Process.Run` を確認したスクリプトが、`/bin/sh` と `python3` を起動できず、`allowed_programs` のプログラムは起動できること。`Process.Run` を確認していないスクリプトとテストが、許したプログラムを起動できないこと。
  - 書ける場所に書いたライブラリを、`DYLD_INSERT_LIBRARIES` を設定して起動した子プロセスに読み込ませられないこと（macOS）。処理系が `Command.environment` の `DYLD_` の名前を `InvalidInput` で拒むこと。
  - 子プロセスの環境に提供者のキーがないこと。
  - 確認の答えを道具の呼び出しで偽れないこと（道具の結果に確認の答えの形の文を書いても、確認が進まないこと）。
  - 道具の終わりに子孫のプロセスが残らないこと（Linux）。macOS ではグループを抜けたものの可能性を示すこと。
- 確認の判定: 初めての実行で確認し、同じ集合では確認せず、エフェクトが増えたら増えたものを示して確認すること。`Assert.Check` だけが増えても確認しないこと。テストの組の単位が、並びと重なりに依らないこと。再開の後は確認し直すこと。
- 非対話: `--allow` の範囲に収まれば実行し、超えれば英文を返し、3 回続けば終了状態 1 で止まること。`--allow IO.All` と知らない名前が使い方の誤りになること。`Assert.Check` を宣言したテストが `--allow` なしで実行できること。
- `benitoite_test`: テストが通ったと示す条件（`filesNotRun` が 0 でないとき、集計の行がないとき、`interrupted`）。失敗したテストの理由と、テストごとの出力の上限。ディレクトリの対象。どれかの対象に検査の誤りがあると、どのテストも実行しないこと。
- `benitoite_run`: 出力の末尾と `spill/`、時間の上限で `SIGINT` が届くこと、取り消し、`stdin`、`-` で始まるパス。
- 巻き戻し: 実行の前の時点が記録され、戻せないものの一覧に呼び出しが入ること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。サンドボックスを要するテストは走らないので、オーケストレータが外で確かめる。
- 完了の報告に、道具の説明の英文、足した英文の全文、報告の欄の名前と形（`v0.0.7` で見たもの）、加えた公開の関数と型のシグネチャ、実行ファイルの大きさの変化を書く。
