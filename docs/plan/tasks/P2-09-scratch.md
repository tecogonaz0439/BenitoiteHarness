# P2-09 一時的なスクリプト `benitoite_scratch`

## 目的

制限モードの道具 `benitoite_scratch` を作る。LLM が渡したコードを一時ディレクトリに新しい名前のファイルとして書き、実行の前の検査の報告を得て、エフェクトが確認なしの範囲に収まれば、読める範囲を限った OS のサンドボックスの中で確認なしで実行する。範囲を超えれば `benitoite_run` と同じく確認を受ける（設計書 01-03 の「benitoite_scratch（制限モード）」、03-03 の「一時的なスクリプト」）。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「benitoite_scratch（制限モード）」「benitoite_run（制限モード）」「出力の切り詰め」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「一時的なスクリプト」（既定の範囲、利用者の設定で加えられるものと加えられないもの、プロジェクトの設定では変えられないこと、確認なしで実行するときの読める範囲と書ける場所）「制限モードの確認」「非対話での扱い」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「制限モードの方針」「方針の要素」
- [セッション](../../design/01-agent/01-05-sessions.md)の「一時ディレクトリ」「巻き戻し」（記録する時点）
- 既存のもの: P2-08 の `benitoite_run`（報告、方針、実行、結果、`ApprovalKind::Effects` の確認の判定、`ConfirmKey`）、P2-12 の非対話の判定（ループの `approve`）と続けての拒否の数、`src/config/mod.rs`（`RestrictedConfig.scratch_auto_effects` と名前の検査）、`src/sandbox/policy.rs`（`ReadAccess::Roots`）、`src/session/`（一時ディレクトリ）

## この作業が認める型の変更

- `ToolId` に `BenitoiteScratch` を加える。並べて実行しない。
- P2-12 の非対話の判定に、確認なしの範囲を渡す口を加える（今は `benitoite_run`・`benitoite_test` の空の範囲だけ）。
- 巻き戻しの時点の理由は、`CheckpointReason` に `BenitoiteScratch` を加えるか、P2-08 の `BenitoiteRun` を使うかをこの作業で決め、報告に書く。
- P2-12 の続けての拒否の停止の英文を、`The run stopped because benitoite_run, benitoite_test or benitoite_scratch was refused 3 times in a row: the effects were outside --allow.` に改める。

## 作るもの

- 引数は `{ "code": "string", "args": ["string (optional)"], "stdin": "string (optional)" }`。
- `code` を、セッションの一時ディレクトリに `scratch-<n>.bnt`（`n` はセッションの中で 1 から増やす。すでにある名前には書かず、次の番号にする）として、リンクを辿らずに新しく作って書く。書いたファイルのパスを結果に含める。
- 報告（`check --report=json`）を得て、エフェクト（`Assert.Check` を除く）が確認なしの範囲に収まるかを決める。範囲は、既定（エフェクトなし、`Console.Write`、`State`、`Process.Exit`）と、利用者の設定の `restricted.scratch_auto_effects` の和とする。プロジェクトの設定では変えられない（今の `ProjectConfig` が `restricted` を持たないことをテストで確かめる）。
- 収まるとき: 確認なしで実行する。方針は、03-03 のとおり、読める範囲をプロジェクトのディレクトリ、一時ディレクトリ、処理系の実行ファイル、処理系の動作に要る OS の場所に限り（`ReadAccess::Roots`）、書ける場所は一時ディレクトリだけとする。ネットワークとプログラムの起動は、P2-08 の方針の規則のとおり（範囲の中では `Http.*` と `Process.Run` は含まれないので、どちらも許さない）。処理系の動作に要る OS の場所は、macOS と Linux のそれぞれで、処理系の `run` が動く最小の組を試して決め、報告に一覧と理由を書く（例: macOS の `/usr/lib`、`/System/Library` の一部。Linux の静的リンクの処理系では要らない見込み）。
- 収まらないとき: `benitoite_run` と同じ確認（`ApprovalKind::Effects`。確かめの単位は書いたファイルの実のパスの `ConfirmKey::Run`）を受け、許されたら、P2-08 のとおり報告を得直して確認した範囲と照らし、P2-08 の方針（読める範囲は既定）で実行する。非対話では、P2-12 の判定で、確認なしの範囲と `--allow` の和に収まれば実行し、超えれば P2-12 の英文を返して、続けての拒否の数に入れる。
- 実行は P2-08 の `benitoite_run` の部品を使う（`--allow-effects` に報告の集合、出力の扱い、時間の上限 120 秒、止め方、結果の形、`processor_launches` の数え方）。巻き戻しの時点を実行の直前に記録する。戻せないものの一覧に `benitoite_scratch` の呼び出しを、`benitoite_run` と同じ選び方で含める。
- 一時的なスクリプトの根は一時ディレクトリなので、プロジェクトのモジュールを取り込めないことを、道具の説明に書く。道具の説明の英文を `prompts/tools/benitoite_scratch.md` に置く（案を報告に書き、オーケストレータが取り込む前に見直す）。
- 制限モードの `ToolSet` に、01-03 の表の順（`benitoite_test` の後）で加える。期待のファイルを作り直す。

- `/new`（P4-08）は、安全に関わる値を今のセッションの値に固定する（`SessionOverrides`）。この作業で `restricted.scratch_auto_effects` を使い始めるので、`/new` ではこの値も固定し、読み直した設定で置き換えない。テストを加える。

## 触るファイル

- `crates/benitoiteh/src/tools/`（`benitoite_scratch.rs`（新）、`mod.rs`、`definitions.rs`、`text.rs`）
- `crates/benitoiteh/prompts/tools/benitoite_scratch.md`（新）
- `crates/benitoiteh/src/sandbox/`（読める範囲を限る方針）
- `crates/benitoiteh/src/approval/`、`src/agent/`（範囲の受け渡し、停止の英文、`rewind.rs`）
- `crates/benitoiteh/src/rewind/types.rs`（`CheckpointReason` を加えるとき）
- `crates/benitoiteh/tests/expected/`、新しいテスト（サンドボックスを要するものは `tests/sandbox_*.rs`）

## テスト

- 範囲: `Console.Write` だけのスクリプトが確認なしで動くこと。`File.Read` は既定で確認を求め、利用者の設定で加えると確認なしになること。`File.Write`・`Process.Run`・`Http.Connect` を `scratch_auto_effects` に書くと設定の誤りになること（今の検査の確かめ）。
- 読める範囲（安全の性質のテスト。07-01 の「読まないファイル」の考えに当たる）: 確認なしで動く `File.Read` のスクリプト（設定で範囲に加えたもの）が、プロジェクトのディレクトリと一時ディレクトリの外（ホームディレクトリの下に置いたファイル）を読めないこと。プロジェクトに書けないこと。macOS と Linux で。
- 名前: 二度呼ぶと `scratch-1.bnt` と `scratch-2.bnt` になること。一時ディレクトリにあらかじめ `scratch-1.bnt` へのシンボリックリンクを置いても、リンクの先に書かないこと。
- 範囲を超えたときの確認と、非対話での扱い。
- プロジェクトのモジュールを取り込むコードが、検査の誤りになること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、道具の説明の英文、足した英文の全文、OS ごとの読める範囲の一覧と決めた理由、実行ファイルの大きさの変化を書く。

## P2-08・P2-12 の実装の後の補足（2026-10-18）

P2-08 と P2-12 を取り込んだ後に、この文書を実装と `v0.0.7` に突き合わせた。上の記述と食い違うところは、この節を正とする。

### 既存のものの名前と形

- 非対話の印は `ToolContext.noninteractive`（`AgentConfig.allow_effects.is_some()` から立てる）である。
- `benitoite::RunContext.scratch` は、Linux の覆いのファイルを置くセッションの `sandbox/` であり、一時的なスクリプトの置き場所ではない。一時的なスクリプトは `ToolContext.temporary`（セッションの一時ディレクトリを開いた `ScopeRoot`。`run.locations.temporary` と同じディレクトリ）に書く。
- 実行の本体（`tools/benitoite_run.rs` の非公開の `execute_report`）を `tools/benitoite_process.rs` へ移して共有する。巻き戻しの理由と読める範囲を引数で受け取る形にする。`benitoite_run` の振る舞いは変えない。
- 方針は `benitoite_process::prepare` の中で `effects::restricted_access(...)` が作り、`readable` は常に `ReadAccess::All` である。確認なしの経路のために、作った `RestrictedAccess` の `readable` を差し替える口を加える。
- 判定の部品は P2-08・P2-12 と同じものを使う: `report::parse(.., ReportKind::Run)`、`benitoite::diagnostics::unread_imports`、`approval::effects_summary`・`allow_denied`・`unread_imports_denied`、`ConfirmKey::from_facts(ApprovalKind::Effects, ..)`。
- `ScopeRoot::write`・`write_from` は既存の名前を置き換えるので使わない。`ScopeRoot` に、親の記述子から `O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC`（権限 0o600）で作るだけの関数を加える（`TemporaryFile::create` と同じ開き方）。すでにある名前（シンボリックリンクを含む）は `SafeFsError::AlreadyExists` になるので、次の番号を試す。試すのは 128 回までとし、超えたら道具の誤りとする。

### 一時的なスクリプトの名前と、確認の後の呼び直し

- 番号は `ToolContext` が持つ単調に増える数で振り、同じ `ToolContext` の中で同じ番号を二度使わない（消されたファイルの番号も使い直さない）。`/new` と再開の後は新しい `ToolContext` と一時ディレクトリになるので、1 から振り直してよい。
- 確認を受けた後の呼び直し（`ApprovalStatus::Approved`）では、新しく書かない。初回（`Unapproved`）に書いたファイルのパスを、`call_id` を鍵として `ToolContext` に覚える（`scratch` は並べて実行しないので、待っている一件だけを保てばよい）。呼び直しでは、そのパスを `open_file(.., false)` で開き直して使う。見つからないときは道具の誤り（`ToolError::State` に当たるもの）とする。理由: ループは確認の後に同じ呼び出しを実行し直し、確認したエフェクトを、書いたファイルの実のパスの `ConfirmKey::Run` に記録する（`agent/execution.rs` の `approve` と `execute_approved`）。呼び直しで別の名前に書くと単位がずれ、`EFFECTS_CHANGED` になる。非対話の `--allow` の経路でも同じである。
- 処理系には、書いたファイルの実のパス（`temporary.real_path().join(name)`）を渡す。書いた後のどの結果（検査の誤り、確認の拒否の後の結果、実行の結果）にも、そのパスを最初の行で示す。
- `code` が先頭の 8 KiB に NUL を含むか、64 MiB を超えるときは、書かずに道具の誤りとする。理由: `read` が読み返せず、内容のハッシュを照合できないので、読まないファイルに数えられて毎回確認になる。

### 範囲の判定（読まないファイルを取り込む実行を含む）

- 初回（`Unapproved`）は、報告の集合（`Assert.Check` を除く）が確認なしの範囲に収まり、かつ `unread_imports == 0` のときだけ確認なしで実行する。ほかは `ApprovalKind::Effects` の確認を求める。初回は `invocation.confirmed_effects` の前の確認を見ない。理由: LLM は `write` で一時ディレクトリに `.bnt` を書き、`benitoite_run` で確認させられるので、前の確認を見ると、別の中身への確認が引き継がれる道ができる。
- 呼び直し（`Approved`）は `benitoite_run` と同じとする。二度目の報告が確認した集合に収まらなければ `EFFECTS_CHANGED` を返し、非対話で `unread_imports > 0` なら `unread_imports_denied` を返す。
- 読まないファイルの規則（設計書 04-01「実行の前の検査の報告」）を当てる理由: 2026-10-18 に `v0.0.7` で、スクリプトの先頭の `dependency Ext / path "<外>" / end dependency` で外のディレクトリのモジュールを取り込め、その源が `kind:"user"` の絶対の表示のパスで報告に出ること、取り込んだモジュールの文字列のリテラルは実行で読み取りを許さなくても出力に出ることを確かめた。狭い読み取りの範囲では防げない。
- 一時的なスクリプト自身は読まないファイルに数えない（特別な扱いは要らない）。報告の表示のパスは渡した絶対の実のパスのままになり（`v0.0.7` で確かめた）、`ToolContext::resolve` がプロジェクトの外として扱い、`open_file` が `temporary` の根で開き、ハッシュが一致する。利用者の `read.exclude` が一時ディレクトリに当たるとき、上の NUL と大きさの規則に当たるときは、安全の側（毎回の確認）に倒れる。

### 確認なしの範囲の値の持ち方と `/new`

- `benitoite::RunContext` に `scratch_auto_effects: BTreeSet<Effect>` を加える。値は既定（`Console.Write`・`State`・`Process.Exit`）と、利用者の設定の `restricted.scratch_auto_effects` を `Effect::from_name` で読んだものの和である。`cli/exec.rs` の `prepare` で、制限モードのときだけ作る（`tests/support/benitoite_run.rs` の `RunContext` を作る所も直す）。
- `agent::SessionSafety` に、利用者の設定の値を固定する欄（例: `scratch_auto_effects: Vec<String>`。通常モードでは空）を加える。`prepare` は、`overrides` があるとき、`user.restricted.scratch_auto_effects` を固定した値で置き換える（`allowed_programs` と同じ）。`with_session_safety` にも渡す。
- プロジェクトの設定（`ProjectConfig`、`deny_unknown_fields`）は `restricted` を持たない。プロジェクトの設定で範囲を変えられないことは、`tests/safe_fs.rs` の拒否の表に `"[restricted]\nscratch_auto_effects=['File.Read']"` を足して確かめる。

### 非対話と続けての拒否

- `agent/execution.rs` の `approve` の `allow_effects` の枝で、`ToolId::BenitoiteScratch` のときは、許す集合を「`--allow` と `run.scratch_auto_effects` の和」とし、収まるかの判定、`confirmed_effects` の広げ方、`allow_denied` の `<list>` のすべてに使う。理由: 和で判定しないと `{Console.Write, File.Read}` を `--allow File.Read` で動かせず、和で広げないと呼び直しで `EFFECTS_CHANGED` になる。
- 続けての拒否の数を 0 に戻す条件に `ToolId::BenitoiteScratch` を加える。
- 範囲の中でも `unread_imports > 0` なら、道具は確認を求め、非対話では `approve` の既存の `unread_imports_denied` が断る（続けての拒否の数には入れない。`benitoite_run` と同じ）。
- 停止の英文（`agent/text.rs` の `EFFECTS_NOT_ALLOWED`）を、上の「この作業が認める型の変更」の文に改め、`tests/agent_loop/allow.rs` の期待も直す。

### 確認なしで実行するときの方針（読める範囲）

- `restricted_access` で作り、`readable` だけを差し替える。書ける場所は一時ディレクトリだけ、IP なし、起動できるのは処理系だけであり、範囲に `File.Write`・`Http.*`・`Process.Run` は入りえないので、`restricted_access` の値と一致する。確認を経た実行（呼び直し）は P2-08 と同じ方針（読める範囲を限らない）とする（設計書 03-03「一時的なスクリプト」）。
- `ReadAccess::Roots` はディレクトリだけを受け付け、Linux は根ごとに `ReadFile|ReadDir` を与える。配下を含まない一つのパス（処理系の実行ファイル、macOS の `/`）と、macOS でメタデータだけを許すパスを表す要素を、`PolicyConfig`（補助のプロセスへ JSON で渡す。新しい欄は `serde(default)`）に加えてよい。
- macOS の組（2026-10-18 に、macOS 27.0.1・arm64 で `v0.0.7` に `sandbox-exec -p` で方針を直接渡して確かめた）:
  - `file-read* file-map-executable`: プロジェクトと一時ディレクトリ（`subpath`）、処理系（`literal`）、`/usr/lib`、`/System/Library`、`/System/Volumes/Preboot/Cryptexes`。
  - `file-read*`: `(literal "/")`、`/private/etc/localtime`（`literal`）、`/private/var/db/timezone`（`subpath`）。
  - `file-read-metadata`（`literal`）: プロジェクト、一時ディレクトリ、処理系、時間帯の情報のそれぞれの上位のディレクトリ（`/etc`・`/var` のリンクも含む）。全体には許さない（ファイルの有無、大きさ、時刻が見えるため）。
  - 欠けたときの振る舞い: `/usr/lib`・`/System` の側か `/` が欠けると SIGABRT（134）。上位のディレクトリのメタデータか、作業ディレクトリの読み取りが欠けると、処理系は `internal compiler error ... run stage`（終了状態 3）。時間帯の情報がないと `Clock.localOffsetMinutes()` が 0 を返す。
- Linux の組は実測していない。Landlock は `stat` と経路の解決を限らないので、上位のディレクトリのメタデータは要らない見込みである。処理系は静的リンクで、実行の権利は `programs` の規則で与える。処理系のファイルの読み取りが要るかと、`/etc/localtime` を含めるかを、x86_64 の機械で確かめる（オーケストレータがサンドボックスの外の検査で確かめるので、Codex はテストを用意する）。
- 組は、利用者が範囲に加えられる `File.Read`・`Clock.Time`・`Random.Generate`・`Process.Environment` のそれぞれを使うスクリプトでも確かめ、完了の報告に OS ごとの一覧と理由を書く。

### 巻き戻し・道具の定義・プロンプト

- `CheckpointReason::BenitoiteScratch` を加え（P2-12 の `BenitoiteTest` と同じ形）、`ui/tui/text.rs` の網羅の `match` に `"benitoite_scratch"` を加える。戻せないものの一覧（`agent/rewind.rs` の `matches!`）にも、確認の有無によらず `"benitoite_scratch"` を加える。
- `ToolId::BenitoiteScratch`（名前 `"benitoite_scratch"`）を加え、`definitions.rs` の `benitoite_test` の後に置く。引数は `code`（必須の文字列）、`args`（`text::RUN_ARGS`）、`stdin`（`text::RUN_STDIN`）とし、`parallel` は偽とする。`timeout` は持たず、上限は 120 秒に固定する。`ToolSet::without_benitoite` の `matches!` にも加える。
- 説明は `prompts/tools/benitoite_scratch.md` に置き、`tools/text.rs` から `prompt_parts(include_str!(..))` で読む。案（取り込む前に見直す）:

  ```
  Write Benitoite code to a new temporary script in the session temporary directory, check it, and run it in the sandbox.
  benitoiteh runs it without asking when its effects stay within the automatic set (no effects, Console.Write, State, Process.Exit, and any the user added); otherwise the user confirms them as for benitoite_run. You do not choose the allowed effects.
  The script's root is the session temporary directory, so `import` cannot load project modules. The result shows the script's path; read it and write it into the project to keep it.
  Pass script arguments in `args` and standard input in `stdin` (empty otherwise). The time limit is 120 seconds.
  ```
- `prompts/benitoite-mapping.md` の「benitoite_run and benitoite_test obtain the inspection report ...」を「benitoite_run, benitoite_test and benitoite_scratch obtain ...」に改める（設計書 01-04 も同じ）。期待のファイル（`tests/expected/tools-restricted.json`・`system-restricted.txt`）を作り直す。

### テスト

- 道具の段の判定（名前、リンク、範囲、読まないファイル、取り込めないモジュール）は、本物の処理系の報告で `tests/tools_benitoite_scratch.rs`（新）に置く（`--inside-codex` で動く）。サンドボックスの中の実行は `tests/sandbox_benitoite_scratch.rs`（新）に置き、`tests/support/benitoite_run.rs` の `Fixture` を使う（`call` は道具の名前を引数に取る形に広げてよい）。非対話の和と停止は `tests/agent_loop/allow.rs` に加える。`/new` の固定は、`src/cli/exec/tests.rs` の `new_after_restricted_resume_preserves_mode_and_project_and_normal_preserves_policies` を広げる。
- `scripts/check.sh` の、aarch64 のコンテナで飛ばす sandbox のテストの名前の列挙に `sandbox_benitoite_scratch.rs` を加える。
- 加えるテスト:
  - 読まないファイル: `dependency` で `Fixture.root` の下の外のディレクトリ（`package.bnt` を置く）を取り込むスクリプト、プロジェクトの `read.exclude` で除いたディレクトリを取り込むスクリプト、BH-029 の囮（`tests/support/import_decoy.rs`）を取り込むスクリプトのそれぞれで、範囲の中のエフェクトだけでも、対話では確認（`unread_imports` が 1 以上）を求め、非対話では `unread_imports_denied` の文で断る。
  - `Console.Write` だけのスクリプトは確認なしで動き、`unread_imports` が 0 になる。
  - 読める範囲: 利用者の範囲に `File.Read` を加えたスクリプトが、プロジェクトと一時ディレクトリのファイルを読め、`Fixture.root` の下のその外のファイル（本物のホームディレクトリは使わない）を読めない（macOS と Linux）。
  - 「プロジェクトに書けないこと」は、範囲に `File.Write` も `Process.Run` も入らないので処理系を通しては起こせない。確認なしの経路が作る方針の値（書ける場所が一時ディレクトリだけ、IP なし、処理系だけ）を確かめるテストに置き換える。
  - 呼び直し: 範囲を超えるスクリプトを確認して呼び直し、`scratch-1.bnt` だけができて `scratch-2.bnt` ができないことと、実行されることを確かめる。
  - 名前: 単調に増えることと、あらかじめ置いたリンクを辿らないこと。
  - 設定の誤り: `tests/config.rs` の表に `Process.Run`・`Http.Connect`・`Console.Read`・`Http.Listen` を足す。

### 触るファイルに加える

`src/safe_fs/mod.rs`、`src/sandbox/policy.rs`・`macos.rs`・`linux/filesystem.rs`・`restricted.rs`、`src/benitoite/mod.rs`・`effects.rs`、`src/tools/benitoite_run.rs`・`benitoite_process.rs`、`src/agent/status.rs`・`execution.rs`・`text.rs`・`rewind.rs`、`src/cli/exec.rs`・`exec/tests.rs`、`src/ui/tui/text.rs`、`prompts/benitoite-mapping.md`、`scripts/check.sh`、`tests/support/benitoite_run.rs`、`tests/agent_loop/allow.rs`、`tests/safe_fs.rs`、`tests/config.rs`。
