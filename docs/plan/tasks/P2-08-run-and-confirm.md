# P2-08 実行の前の検査の報告、制限モードの方針、`benitoite_run`、対話の確認

## 目的

制限モードでスクリプトを実行する道具 `benitoite_run` を作る。実行の前に処理系の検査の報告（`check --report=json`）からエフェクトを得て、確認の要否を決め、対話の画面で確認を受け、そのエフェクトから作った方針の OS のサンドボックスの中で、同じ集合を `--allow-effects` に渡して実行する（設計書 04-01 の「実行の前の検査の報告」、01-03 の「benitoite_run（制限モード）」、03-02 の「制限モードの方針」、03-03 の「制限モードの確認」）。報告を読む部品、エフェクトの型、方針を作る関数、確認の判定、実行の結果の作り方は、`benitoite_test`（P2-12）と `benitoite_scratch`（P2-09）も使う。

この作業は対話の確認だけを作る。`benitoite_test`、非対話の `--allow`、続けての拒否での停止は P2-12 で作る。この作業の時点の `exec` の制限モードは、確認の要求をすべて断る（後述の 5）。対話の画面で示す事実のうち、エフェクトの説明、サンドボックスの範囲、許すプログラムは P2-10 で作る。この作業の時点の画面は、対象のパス、エフェクト、増えたエフェクト、シェルの参照を示す。

## 読む設計書

- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「処理系の起動」（`run` のコマンドライン、`--allow-effects` をいつも付けること、空の集合、標準入力、出力を終わりまで読むこと、16 MiB の上限、時間の上限と止め方）「終了状態と診断の受け渡し」（`run` の終了状態の読み方、`E0512` の英文、診断の文の形）「実行の前の検査の報告」（すべて。報告を二度得る規則を含む）
- [道具](../../design/01-agent/01-03-tools.md)の「benitoite_run（制限モード）」「出力の切り詰め」（1,000 行か 25 KiB、`spill/`）「道具の定義の共通の規則」「読まないファイル」「web_fetch」（確認を受けてから実行し直す形）「モードごとの道具の一覧」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「制限モードの方針」「制限モードで実行を許すプログラム」「子プロセスの環境」「子プロセスの後始末」「端末からの切り離し」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」「制限モードの確認」「非対話での扱い」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「保証しないこと」（検査と実行の間の、同じエフェクトの範囲での変更）
- [セッション](../../design/01-agent/01-05-sessions.md)の「セッションの記録」（実行の始まりの行、確認の記録）「再開」（確認したエフェクトの記録は再開しない）「巻き戻し」（記録する時点、戻せないものの表示）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「終了状態」「exec の出力」
- [テスト](../../design/07-quality/07-01-testing.md)の「安全の性質のテスト」
- 既存のもの: P2-05〜P2-07 の `benitoite` モジュール（`Processor::query`、`QueryLimits`・`StderrLimit`、JSON の読み方、`BenitoiteError`、`sandbox::restricted_policy`・`RestrictedAccess`、`diagnostics.rs` の読み方と文の形と `DiagnosticSources`、`benitoite/text.rs`）、`src/sandbox/run.rs`（`run`・`RunLimits`・`StopSignal::Interrupt`・`Leftovers`）、`src/sandbox/mod.rs`（`SandboxCommand`・`SandboxStdin`）、`src/config/environment.rs`（`child_environment`）、`src/tools/bash.rs`（出力の末尾の保ち方、`spill/` の書き方、`Leftovers` の示し方、`checkpoint_before` の呼び方）、`src/tools/truncate.rs`、`src/tools/paths.rs`（`open_file`）、`src/tools/mod.rs`（`ToolOutput`・`ToolOutput::needs_approval`・`ApprovalStatus`・`Invocation`）、`src/approval/`（`ApprovalKind`・`ApprovalFacts`・`decide`・`decide_in_mode`・`ApprovalRecord`・`ApprovalAnswer`・`Approver`・`NonInteractiveApprover`・`policy_denial_message`・`user_denial_message`・`text::with_user_message`）、`src/approval/interactive.rs`、`src/agent/execution.rs`（`approve`・`execute_approved`、`allowed_commands`・`session_domains` の持ち方、`Record::ToolStart` を書く箇所）、`src/agent/rewind.rs`（戻せないものの一覧）、`src/ui/tui/text.rs`（`approval_title`・`answer_keys`・事実の見出し）、`src/ui/tui/confirmation.rs`（事実の行）、`src/rewind/types.rs`（`CheckpointReason`）、`src/session/record.rs`（`Summary`）

## 前提（オーケストレータが確かめた事実）

2026-10-18 に、オーケストレータが手元の `v0.0.7` で次のことを確かめた。

報告の欄:

- `checkReport` と `testCheckReport` の最上位の欄は、`aggregateEffects`、`complete`、`diagnosticTargets`、`diagnostics`、`executed`、`kind`、`producer`、`requestedTargets`、`schemaVersion`、`sourceSets`、`sources`、`status`、`targets`、`targetsComplete` である。
- `aggregateEffects` は `{ "basis": "declaredUpperBound", "expanded": [...] }` の形であり、`complete` が偽のときは `null` である。
- `processShellReference` は、`check` の報告では `targets[].main.processShellReference` に、`test --check` の報告では `targets[].processShellReference` にある。`test --check` で検査の誤りがあった対象では `"unknown"` である。
- `sources` の要素は `{ "sourceId", "kind": "user" | "prelude", "displayPath", "contentHash": { "algorithm", "value" } }` の形である。

報告の状態と終了状態:

- `status` が `"checkError"` のときは、終了状態 2 であり、`targets[].main` は `null` である。
- `main.bnt` のないディレクトリを渡すと、`status` が `"usageError"` で、終了状態 2 である。`diagnostics` は `code` と `primary` が `null` の一件だけで、`targets` と `sources` は空である。
- ないファイルを渡すと、`status` が `"checkError"` で、`primary` が `null` の `E0101` の診断がある。
- `status` が `"ok"` の報告も、`diagnostics` に警告を持ちうる。同じスクリプトの `run` は、その警告を標準エラー出力にもう一度書く。

`run`:

- `--allow-effects` は 13 の名前をすべて受け付ける。`IO.All`、知らない名前、重なった名前を渡すと、JSON でない `error:` の一行を標準エラー出力に書いて、終了状態 2 で終わる。
- `E0512` は、終了状態 2 で、診断を一件書く（`kind` が `"check"`。`notes` に `effects not allowed: …` と `allowed effects: …`）。
- `die` の失敗は、終了状態 1 で、`{ "kind": "die", "code": null, … }` の報告を書く。
- 実行時の誤り（`R0101` など）の報告は、`kind` が `"runtime"` で、診断の欄に加えて `trace`、`traceOmitted`、`taskOrigins` の欄を持つ。
- `main` が `Result.Error` を返すと、JSON でない文を標準エラー出力に書いて、終了状態 1 で終わる。
- `Process.exit(7)` は、標準エラー出力に何も書かず、終了状態 7 で終わる。
- `SIGINT` を受けると、標準エラー出力に何も書かず、終了状態 130 で終わる。

`die` と `runtime` の報告が P2-07 の読み方（診断の欄をすべて正しい型で持つ行を診断とする）で診断として読めるかを確かめ、報告に書く。読めなければ止まって報告する。

## この作業が認める型の変更

00-common の「止まるとき」に当たる次の変更は、この作業で行ってよい。10-interfaces はオーケストレータが更新する。

- `ToolId` に `BenitoiteRun` を加える。並べて実行しない。
- `benitoite` に、エフェクトの型 `Effect`（13 の値と、`--allow-effects` の綴りとの変換）を加える。
- `ApprovalKind` に `Effects`（スクリプトの実行の前のエフェクトの確認。`benitoite_run` と、後の `benitoite_scratch`）を加える。テストの確認の種類は P2-12 で加える。
- `ApprovalFacts` に次の欄を `#[serde(default)]` で加える（P2-10 で、サンドボックスの範囲などの欄を足す）。
  - `targets: Vec<PathBuf>`: 実行の対象の絶対の実のパス。
  - `effects: Vec<String>`: 報告の集合から `Assert.Check` を除き、名前の順に並べたもの。
  - `added_effects: Vec<String>`: そのうち、その単位で前に確認した集合にないもの。初めての実行では空とする。
  - `shell_reference: bool`: 報告の `processShellReference` に `"present"` があり、集合に `Process.Run` を含むときに真。
  - `sources: Vec<ApprovalSource>`: 報告の `sources` のうち `kind` が `"user"` のもの。`ApprovalSource { display_path: String, content_hash: ... }` とし、`content_hash` は報告の `{ algorithm, value }` をそのまま保つ。記録と JSON Lines にだけ書き、画面には示さない。
- 確かめの単位 `ConfirmKey` を `enum ConfirmKey { Run(PathBuf), Test(Vec<PathBuf>) }` として `tools` に置く（`Test` は P2-12 で使う）。`Agent` が会話の外に `confirmed_effects: BTreeMap<ConfirmKey, BTreeSet<Effect>>` を持ち、`Invocation` に参照を渡す（`session_domains` と同じ形）。
- `ToolOutput` に `processor_launches: u64` を加える（LLM に送る文に含めない。後述の 7）。
- `policy_denial_message` と `user_denial_message` の `Effects` の枝は、事実（対象とエフェクト）から文を作るので、引数に `&ApprovalFacts` と、表示用のパスを作る手段（プロジェクトの実のパス）を足してよい。
- `CheckpointReason` に `BenitoiteRun` を加える。
- `benitoite` に、エフェクトの集合から P2-05 の `RestrictedAccess` を作る関数を加える。

## 作るもの

### 1. 報告を読む

- `src/benitoite/report.rs` に、`checkReport` を読む部品を作る。`testCheckReport` も同じ部品で読めるようにする（`kind` と `processShellReference` の位置を引数で分ける。P2-12 が使う）。欄は名前で読み、並びに頼らない。
- 04-01 の条件（終了状態 0、一つの完全な JSON、`schemaVersion` 1、`kind`、`status` が `"ok"`、`complete` が真、`executed` が偽、`aggregateEffects` が `null` でなく `basis` が `"declaredUpperBound"`、`expanded` の名前がすべて 13 の知る名前）を満たすときだけ、確定した結果を返す。確定した結果は、エフェクトの集合（`BTreeSet<Effect>`）、シェルの参照（`processShellReference` に `"present"` があるか）、`sources` のうち `kind` が `"user"` のものの表示用のパスと `contentHash` とする。知らない名前を一つでも含めば確定しない。
- `status` が `"ok"` の報告の `diagnostics`（警告）は示さない。理由: `run` が同じ警告を標準エラー出力にもう一度書き、実行の結果の診断として示される。
- `status` が `"checkError"` なら、報告の `diagnostics` を P2-07 の診断の文（読まないファイルの件数、重なりのまとめ、20 件の上限を含む）にして、誤りの印を付けて返す（実行しない）。
- ほかのときは、`Could not inspect <path> with the Benitoite processor: <reason>.` を、誤りの印を付けて返す（実行しない）。`<path>` は LLM が渡したパス、`<reason>` は一行の理由とする。`status` が `"usageError"` なら、最初の診断の `message` を理由とする。ほかの理由（終了状態、読めない JSON、`schemaVersion` や `kind` の違い、`complete` が偽、`executed` が真、`aggregateEffects` が `null`、`basis` の違い、知らない名前、時間切れ）の英文は実装で決め、報告に全文を書く。報告が 16 MiB を超えたときだけは、この形でなく、04-01 の英文 `The inspection report from benitoite exceeded 16 MiB and was not read. Pass fewer or narrower paths.` をそのまま返す。
- 報告の問い合わせは、P2-05 の `Processor::query` で起動する（サンドボックスの中、時間の上限 30 秒、標準出力の上限 16 MiB、標準エラー出力は `StderrLimit::KeepFirst(64 KiB)`、`Invocation.cancelled` を渡す）。
- 英文は `benitoite/text.rs`（か `tools/text.rs`）に置く。

### 2. 方針

- エフェクトの集合から `RestrictedAccess` を作り、`sandbox::restricted_policy` で、03-02 の「制限モードの方針」の表のとおりに方針を作る。
  - 書ける場所は、`File.Write` を含めばプロジェクトのディレクトリと一時ディレクトリ、含まなければ一時ディレクトリだけ。
  - IP の通信は、`Http.Connect` か `Http.Listen` を含むときだけ許す。
  - 起動できるプログラムは、`Process.Run` を含むときは処理系の実のパスと `restricted.allowed_programs`（P2-05 で確かめたもの）、含まないときは処理系の実のパスだけ。
  - 書ける場所での実行の拒否はいつも掛ける。
  - 読める範囲は既定（`ReadAccess::All` から読ませない場所を除く。通常モードの `normal_policy` と同じ候補を使う）。
- 子プロセスの環境は `child_environment(.., Some(("restricted", allow_ip)))` とする。`allow_ip` は、集合に `Http.Connect` か `Http.Listen` を含むときに真とする。
- 標準入力は、`stdin` が渡されたときだけ `SandboxStdin::Piped` にして書いてから閉じ、渡されないときは `SandboxStdin::Null` とする。理由: Linux の `Piped` は制御のソケットで記述子を送る形（P2-01）なので、要らないときは使わない。
- Linux では、P0-07 の `prepare` と P2-01 の `run` を通す。macOS は P4-07 の `__session` を通す。

### 3. `benitoite_run`

- 引数は `{ "path": "string", "args": ["string (optional)"], "stdin": "string (optional)", "timeout": "integer (seconds, optional, max 600)" }` とする（01-03 と同じ名前）。`path` は `ToolContext::open_file(path, false)`（`read` と同じ規則）で開き、開いた記述子の絶対の実のパスを処理系に渡す。拡張子が `.bnt` でなければ道具の誤りとする。
- **一度目の呼び出し**（`ApprovalStatus::Unapproved`）:
  1. 報告を得る（1）。確定しなければ、1 の結果を返す。
  2. 確かめの単位は `ConfirmKey::Run(実のパス)` とする。報告の集合から `Assert.Check` を除いたものが、`confirmed_effects` のその単位の集合に収まれば（単位があるときに限る）、確認を求めずに実行する（後述の「実行」）。
  3. 収まらなければ（その単位がまだない、または集合が増えた）、実行せずに `ToolOutput::needs_approval` で `ApprovalKind::Effects` の確認の要求を返す。事実は「この作業が認める型の変更」の欄で作る。`summary` は `approval/text.rs` の関数で作る（例: `Run <path> with effects: <list>`）。
- **二度目の呼び出し**（ループが確認を受けて、`ApprovalStatus::Approved` で呼び直したもの）:
  1. 報告をもう一度得る（04-01 の「実行の前の検査の報告」の、報告を二度得る規則）。確定しなければ、1 の結果を返す。
  2. その集合から `Assert.Check` を除いたものが、ループが確認の後に広げた `confirmed_effects` のその単位の集合に収まるときだけ実行する。収まらなければ、実行せずに 04-01 の英文 `The script changed after the effects were confirmed and now declares more effects. Call the tool again to inspect it and ask the user.` を、誤りの印を付けて返す。
  3. `--allow-effects` とサンドボックスの方針は、二度目の報告の集合から作る（一度目の集合も、確認した範囲も渡さない）。
- **実行**:
  1. 巻き戻しの時点を `CheckpointReason::BenitoiteRun` で記録する（`bash` と同じく `checkpoint_before`。確認の後、実行の直前。01-05）。
  2. `run --diagnostics=json --allow-effects=<報告の集合> <実のパス> [args...]` を、2 の方針のサンドボックスの中で起動する。集合が空なら `--allow-effects=` とする。`Assert.Check` は報告にあれば残す。
  3. 時間の上限は既定 120 秒、`timeout` で 600 秒まで（1 未満か 600 を超える値は、`bash` と同じく道具の誤り）。止める合図は `SIGINT` とし、10 秒の後に `SIGKILL` を送る（`StopSignal::Interrupt`）。取り消しも同じとする。
- 実行の始まりの行は、ループが一度目の呼び出しの前に書く既存の `Record::ToolStart` とし、新しい記録の種類を作らない。
- **結果**の形:
  - 次の順に並べ、空の部分は省く。`[stdout]` の行に続けて標準出力の末尾、`[stderr]` の行に続けて標準エラー出力のうち診断でない行の末尾、診断の文（P2-07 の読み方と文の形。`die` と `runtime` の報告を含む）、`[exit code: N]`。
  - 標準出力と標準エラー出力は、それぞれ末尾の 1,000 行か 25 KiB を返す（`bash` の末尾の保ち方を共有する形にする）。切り詰めたときだけ、その出力を `spill/` に書き、`bash` と同じ印を添える（`bash` と同じく、切り詰めなかった出力は書かずに捨てる）。二つの出力は別のファイルにする（例: 呼び出しの ID に `-stdout`・`-stderr` を付けた名前）。`ToolOutput.spill` の扱いは報告に書く。
  - 終了状態が 0 でないとき、時間切れのとき、取り消したときは、誤りの印を付ける。
  - 時間切れは `[The script timed out after <n> seconds and was stopped.]`、取り消しは `[The script was stopped because the user cancelled the tool call.]`、グループを抜けて残った可能性（`Leftovers::PossiblyRemaining`）は `[Processes started by the script may still be running.]` を添える。
  - `bash` のサンドボックスの拒否の案内（`escalate` を勧める文）は付けない。理由: 制限モードには `escalate` がない。
  - 処理系の失敗とスクリプトの終了を区別する手順（04-01）は、`E0512` の英文を添えるかを決めるためにだけ使う。終了状態が 2 で、`code` が `"E0512"` の診断があるときに、診断の後に 04-01 の英文を添える。ほかの終了状態の値は、区別せずにそのまま示す。
- 偽の診断: スクリプトが標準エラー出力に書いた診断の形の行も、P2-07 の部品で読む。位置のファイルを `read` が開かない診断は、文で示さず件数だけを示す（P2-07 と同じ。D1）。
- 道具の説明の英文を `prompts/tools/benitoite_run.md` に置く（1 行目が説明、残りが規則。規則は 3 行まで）。

  ```text
  Run a Benitoite script in the sandbox after benitoiteh inspects the effects of `main` and asks the user to confirm them.
  Check the script with benitoite_check first. You do not choose the allowed effects: benitoiteh passes `--allow-effects` from the inspection report.
  Pass script arguments in `args` and standard input in `stdin` (empty otherwise). `timeout` is in seconds (default 120, max 600).
  ```

### 4. 確認の判定（対話）

- 確認が要るのは、その単位をセッションで初めて実行するときと、報告の集合（`Assert.Check` を除く）が、その単位で前に確認した集合より増えたときである（03-03）。収まるときは、中身が変わっていても確認しない。
- ループの `approve` は、`ApprovalKind::Effects` を次のとおり扱う。
  - `decide_in_mode` が制限モードで `WebFetch` のほかを拒む今の守りは保ち、`Effects` だけを `decide` に通す。`decide` は、承認の方針が `on-request`（対話の制限モードの値）なら `Ask`、`never`（`exec` の値）なら `Deny` を返す。
  - `Ask` なら、確認の受け手（`InteractiveApprover`）に答えを求める。答えが `Approved` なら、ループは、その単位の `confirmed_effects` を、事実の `effects` との和にしてから（`ApprovalAnswer::remember_domain` と同じ形の関数で）、`ApprovalStatus::Approved` で道具を呼び直す。
  - `Denied` は `The user declined to run <path> with effects: <list>. Ask the user how to proceed, or change the script.`、`DeniedWithMessage` はこの文を `text::with_user_message` で包んだものを、誤りの印を付けて返す。`<path>` は診断と同じ表示用のパス（プロジェクトの下ならプロジェクトからの相対のパス、ほかは絶対のパス）、`<list>` は `effects`（`Assert.Check` を除き、名前の順に並べたもの）を `, ` で繋いだもの、空なら `none` とする。
  - 「このセッションの間許す」の答え（`a`・`s`）は `Effects` では受け付けない。確認した集合に入れることが、その単位のセッションの間の許しに当たる。
- 単位は、ループが `ApprovalKind` と `ApprovalFacts.targets` から作る（`Effects` は `ConfirmKey::Run(targets の一つ目)`）。道具の側も同じ関数で作る。
- 確認の要求と結果は、既存の `ApprovalRecord` として記録と JSON Lines に書く。`ApprovalFacts.sources` もそこに入る（04-01 の「実行の前の検査の報告」）。
- 確認したエフェクトの記録は再開しない（再開した記録から作り直さない）。`/new` と再開の後は空から始める。

### 5. この作業の時点の非対話

- `--allow` は、P2-12 まで今のとおり使えないオプション（`ArgsError::Unavailable`）のままとする。
- `exec` の制限モードでは、承認の方針が `never` なので、4 の `decide` が `Effects` の確認の要求に `Deny` を返し、ループは `policy_denial_message(Effects)` の `Tool call was denied by the never approval policy: running a Benitoite script requires confirming its effects. Stop and report.` を、誤りの印を付けて返す（実行しない）。集計の `approvals.policy_denied` に数える。エフェクトのないスクリプトも、初めての実行では確認の要求になるので、この作業の時点では断られる。理由: 03-03 の非対話の扱い（確認なしの範囲と `--allow` の和に収まれば実行する）は P2-12 で作る。それまでは、確認していない実行を一つも起こさない側に倒す。P2-12 の後は、この文は、承認の方針が `never` で非対話の許す範囲が渡されていない（設定の食い違い）ときの守りとして残る。

### 6. 対話の画面（P4-07 の確認の画面）

- `ui/tui/text.rs` の `approval_title` と `answer_keys` の網羅の `match` に `Effects` を加える。題は `╭─ Run Benitoite script?`、答えのキーは `[y] run  [n] decline  [m] decline with a message` とする。
- `ui/tui/confirmation.rs` の事実の行に、次の行をこの順で加える（見出しの英文は `text.rs` の定数）。
  - `Targets`: `targets` を一行に一つ。
  - `Effects`: `effects` を `, ` で繋いだもの。空なら `none`。
  - `Added effects`: `added_effects` を `, ` で繋いだもの。空なら行を出さない。
  - `Shell`: `shell_reference` が真のときだけ、`The script references Process.shell; the OS sandbox blocks starting a shell in restricted mode.`
- `sources` は画面に示さない。
- 03-03 の「確認の共通の規則」の、事実を答えのキーの行のすぐ上に置く規則と、題・すべての事実・答えのキーが収まらないときに許す答えを受け付けない規則は、P4-07 の実装をそのまま当てる。

### 7. 集計

- `Summary.checks` は、P2-07 の `ToolOutput.check_launch` が `Some` の数に、`ToolOutput.processor_launches` の和を足したものとする（07-02 の数え方）。`benitoite_run` は、報告の問い合わせ一回ごとと、`run` の起動一回ごとに 1 を数える（確認を経た呼び出しは、一度目の 1 と二度目の 2 の合わせて 3）。ループは、確認の要求を返した出力の `processor_launches` も、呼び直した出力のものも集計に足す。
- `benitoite_run` の `check` は `None` のままとし、検査と修正の回数（P2-07）を変えない。

### 8. 巻き戻しとの関係

- 戻せないものの一覧（01-05 の「戻せないものの表示」）に、`benitoite_run` の呼び出しを含める。`web_fetch` と同じく、`Record::ToolStart` の道具の名前で選ぶ（確認を断られて実行しなかった呼び出しも含まれうるが、少なく示すよりよい）。

### 9. 道具の一覧

- 制限モードの `ToolSet` に、01-03 の表の順（`benitoite_check` の後）で加える。通常モードと `ToolSet::without_benitoite` には加えない。期待のファイル（`tests/expected/tools-restricted.json`・`system-restricted.txt`）を作り直す。

### 残ったプロセスでの停止（03-02「子プロセスの後始末」）

- `sandbox::run` が、グループに `SIGKILL` を送った後もパイプが 2 秒のうちに閉じない（`Leftovers::PossiblyRemaining`）と知らせたら、`benitoite_run`（と P2-12 の `benitoite_test`、P2-09 の `benitoite_scratch`）は、道具の結果に残ったプロセスの英文を添え、`ToolOutput` に止める印（例: `stop_run: Option<String>`）を立てる。ループはこの印を見たら、その道具の結果を会話に入れた後、次の番へ進まずに `AgentError::LeftoverProcesses(String)`（`kind()` は `"limit"`。`Outcome` と `cli` は変えない）で一連の処理を止める。英文の案: `The run stopped because processes started by the script may still be running.`（`agent/text.rs`）。通常モードの `bash` では止めない（今の扱いのまま）。
- テスト: 許すプログラム（`sandbox-probe`）が、パイプを持ったまま新しいセッションに逃げる子を作る形（P2-01 の `sandboxed_groups_are_cleaned_and_escaped_pipe_holders_are_reported` と同じ作り）で、macOS の制限モードの一連の処理が止まること。

## 触るファイル

- `crates/benitoiteh/src/benitoite/`（`report.rs`（新）、`effects.rs`（新）、`text.rs`）
- `crates/benitoiteh/src/sandbox/`（制限モードの方針）
- `crates/benitoiteh/src/tools/`（`benitoite_run.rs`（新）、`ConfirmKey`、出力の部品を `bash.rs` と共有する形、`mod.rs`（`ToolId`・`ToolOutput`・`Invocation`）、`definitions.rs`、`text.rs`）
- `crates/benitoiteh/prompts/tools/benitoite_run.md`（新）
- `crates/benitoiteh/src/approval/`（`Effects` の種類、事実の欄、`decide`・`decide_in_mode`、拒否の文、`text.rs`、`interactive.rs` の答えのキー）
- `crates/benitoiteh/src/agent/`（`confirmed_effects`、`approve` の確認の後の集合の更新、集計、`rewind.rs` の戻せないもの）
- `crates/benitoiteh/src/ui/tui/`（`text.rs`、`confirmation.rs`）
- `crates/benitoiteh/src/rewind/types.rs`（`CheckpointReason`）
- `crates/benitoiteh/src/session/`（記録の欄、`spill/` の名前）
- `crates/benitoiteh/tests/expected/`、新しい `tests/sandbox_benitoite_run.rs` ほか

## テスト

テストを書く前に、スキル `test-audit` の作成時の関門に従う。サンドボックスを要するものは `tests/sandbox_*.rs` に置き、本物の `v0.0.7` の処理系（`BENITOITEH_TEST_BENITOITE`）で確かめる。`Process.Run` で起動を許すプログラムには `sandbox-probe` を使う（Linux では静的にリンクしたもの）。

- **報告の受け入れ**（純粋な関数にバイト列を渡す）: 条件のどれか一つを欠く報告（終了状態、二つの JSON、`schemaVersion` 2、`kind` の違い、`status` が `checkError`・`usageError`、`complete` が偽、`executed` が真、`aggregateEffects` が `null`、`basis` の違い、知らない名前、空の標準出力）で実行しないこと。空の集合として扱わないこと。`usageError` の理由が最初の診断の `message` であること。欄の並びを変えても読めること。`ok` の報告の警告を示さないこと。
- **安全の性質のテスト**（07-01）:
  - 確認の後、実行の前にスクリプトを変えて `main` のエフェクトを増やすと、実行されないこと。二通りを確かめる。(a) 確認の要求を返した後、ループが呼び直す前に書き換える: 二度目の報告が確認した範囲に収まらず、`E0512` の英文が返り、処理系の `run` を起動しないこと。(b) 二度目の報告の後、実行の前に書き換える（差し替えの口を道具のモジュールの中のテストで使ってよい）: 処理系が `E0512` で断り、英文が添えられること。
  - `File.Write` を含まないスクリプトがプロジェクトに書けないこと。`File.Write` を含むスクリプトはプロジェクトに書けること。
  - `Http.Connect` を確認しないスクリプトがネットワークに接続できないこと（ループバックの待ち受けを用意して接続を試す）。`Http.Connect` を含むスクリプトはループバックに接続できること。`Http.Connect` と `Process.Run` を含むスクリプトから起動した `sandbox-probe` の `unix-connect` と `unix-dgram-send` が、外部の Unix IPC として拒まれること。
  - `Process.Run` を確認したスクリプトが、`/bin/sh` と `python3` を起動できず、`allowed_programs` の `sandbox-probe` は起動できること。`Process.Run` を確認していないスクリプトが、`sandbox-probe` を起動できないこと。
  - 書ける場所に書いたライブラリを、`DYLD_INSERT_LIBRARIES` を設定して起動した子プロセスに読み込ませられないこと（macOS。`tests/sandbox_macos.rs` の `dyld_cannot_map_libraries_from_writable_directories` に倣い、`libsandbox_probe.dylib` を使う）。処理系が `Command.environment` の `DYLD_` の名前を `InvalidInput` で拒むこと。
  - 子プロセスの環境に提供者のキーがないこと。
  - 確認の答えを道具の呼び出しで偽れないこと（道具の結果に確認の答えの形の文を書いても、確認が進まないこと）。
  - スクリプトが標準エラー出力に書いた偽の診断の行が `.env` とプロジェクトの外を指すとき、行を添えず、文で示さず件数だけになること（D1）。
  - 道具の終わりに子孫のプロセスが残らないこと（Linux）。macOS ではグループを抜けたものの可能性を示すこと。
- **確認の判定**: 初めての実行で確認し、同じ集合では確認せず、エフェクトが増えたら増えたものを示して確認すること。`Assert.Check` を示さず、判定にも数えないこと。拒否と言付け付きの拒否の文（`<list>` が `none` のときを含む）。再開の後は確認し直すこと。確認の要求の記録に `sources`（`kind` が `"user"` のものだけ）が入り、画面には出ないこと。
- **非対話**: `exec` の制限モードで、`benitoite_run` が 5 の文で断られ、処理系の `run` を起動しないこと。
- **画面**（端末なしの画面のテスト）: 題、答えのキー、`Targets`・`Effects`・`Added effects`・`Shell` の行。空の集合の `none`。`a` と `s` で許されないこと。事実が収まらないときに `y` を受け付けないこと。
- **`benitoite_run`**: 結果の各部分の並びと省き方。出力の末尾と `spill/`（切り詰めたときだけ書くこと）、時間の上限で `SIGINT` が届くこと（短い上限を渡し、120 秒を待たない）、取り消し、`stdin`、`-` で始まるパス、`.bnt` でないパス。`die`、`runtime`、`Result.Error`、`Process.exit(7)` の結果。
- **集計**: `checks` が、確認なしの実行で 2、確認を経た実行で 3 増えること。`fixes` と検査と修正の回数が変わらないこと。
- **巻き戻し**: 実行の前の時点が `BenitoiteRun` で記録され、戻せないものの一覧に呼び出しが入ること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。サンドボックスを要するテストは走らないので、オーケストレータが外で確かめる。
- 完了の報告に、道具の説明の英文、足した英文の全文、`die` と `runtime` の報告が診断として読めたか、加えた公開の関数と型のシグネチャ、画面の例、実行ファイルの大きさの変化を書く。
