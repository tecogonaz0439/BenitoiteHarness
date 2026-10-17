# P2-05 処理系の見つけ方と確かめ、問い合わせの起動

## 目的

Benitoite の処理系（`benitoite`）を見つけ、`--version=json` で要る機能を持つことを確かめ、利用者のスクリプトを実行しない問い合わせ（版、Skill の一覧と文書、検査）を OS のサンドボックスの中で起動する部品を作る（設計書 04-01 の「処理系の見つけ方と版の確かめ」「処理系の起動」）。制限モードは、処理系を使えなければ起動を止め、通常モードは Benitoite の道具を外して続ける。処理系の実行ファイルの SHA-256 をセッションの記録のヘッダに書き、直前のセッションと違えば示す。

この作業は、group B（Benitoite `v0.0.7` の公開の後に始めた、処理系との連携の作業。2026-10-18 から）の最初の作業である。Benitoite の道具そのもの（`benitoite_*`）は作らない（P2-06〜P2-09）。

## 読む設計書

- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「前提」「処理系の見つけ方と版の確かめ」「処理系の起動」（表の「版と機能の確かめ」の行、環境変数、標準入力、16 MiB の上限、問い合わせの時間の上限 10 秒、絶対の実のパス、新しいプロセスグループ、書ける場所を一時ディレクトリだけにしてネットワークを使えない方針で起動すること）
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「制限モードの方針」（表と、利用者のスクリプトを実行しない起動の方針）「制限モードで実行を許すプログラム」（OS の実行ファイルの形式、`#!`、書ける場所の外、名前の一覧、Linux の静的リンク）「端末からの切り離し」「子プロセスの後始末」「サンドボックスを掛けられないとき」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「信頼の境界」の 2（処理系の言明として扱う出力）、「保証しないこと」（処理系の実行ファイルが本物であること）
- [セッション](../../design/01-agent/01-05-sessions.md)の「セッションの記録」（ヘッダの欄）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」の `benitoite.path`・`restricted.allowed_programs`、「プロジェクトの設定ファイル」（`benitoite.path` はプロジェクトの設定で書けない）
- [テスト](../../design/07-quality/07-01-testing.md)の「処理系の用意」
- 既存のもの: `src/config/mod.rs`（`BenitoiteConfig.path`、`RestrictedConfig.allowed_programs` と絶対パスの検査）、`src/config/writable.rs`（`WritableLocations`）、`src/config/environment.rs`（`child_environment`）、`src/sandbox/`（`PolicyConfig`・`SandboxPolicy`・`ReadAccess`・`ProgramAccess`、`SandboxCommand`、`run.rs` の `run`・`RunLimits`・`RunOutcome`、P4-07 で加えた `__session` の起動、Linux の `prepare`）、`src/cli/exec.rs`（`prepare` の流れと、通常モードの `check_sandbox` の差し込み）、`src/session/record.rs`（`Header`（`Record::Header`）・`RecordedBenitoite`）、`src/session/scan.rs`（P4-02 のセッションの一覧）、`src/agent/mod.rs`（`Agent::construct` がヘッダを書き、`ToolSet::new` を呼ぶ）、`src/tools/mod.rs`（`ToolContext`）、`src/cli/doctor/probe.rs`（doctor が持つ制限モードの方針。P4-10 で寄せる）、`src/context/mod.rs`（`BENITOITE_DOCUMENT_VERSION` は `"0.0.6"` の埋め込んだ写しの版で、P2-06 で除く。この作業の新しい定数と混同しない）、`src/tools/bash.rs` のテストの `busybox` などの名前の扱い

## 前提（オーケストレータが用意する）

- テストに使う処理系は、Benitoite の `v0.0.7` のタグのソースからビルドしたものとする。macOS の実行ファイルは `/Users/tecogonaz/src/BenitoiteHarness/work/benitoite-v0.0.7/target/release/benitoite` にある。
- Linux のテスト（aarch64 の `container` と x86_64 の機械）に使う、`x86_64-unknown-linux-musl` と `aarch64-unknown-linux-musl` の静的リンクの処理系は、オーケストレータがビルドして置いた（2026-10-18。`v0.0.7` のタグのソースを `rust:1.98.1-alpine3.22` のコンテナでビルドした）。パスは `/Users/tecogonaz/src/BenitoiteHarness/work/benitoite-linux-build/aarch64/release/benitoite`（`ET_EXEC`、`PT_INTERP` なし）と `/Users/tecogonaz/src/BenitoiteHarness/work/benitoite-linux-build/x86_64/release/benitoite`（static-pie。`ET_DYN` で `PT_DYNAMIC` を持つが `PT_INTERP` はない。静的リンクの判定は `PT_INTERP` がないことだけで行う）である。

## この作業が認める型の変更

00-common の「止まるとき」に当たる次の変更は、この作業で行ってよい。10-interfaces はオーケストレータが更新する。

- 新しいモジュール `benitoite`（`src/benitoite/`。設計書 06-01 の「モジュール」の表にある）を作る。持ち主はこの作業とする。依存の向き（06-01）を守り、`benitoite` は `sandbox`・`config` だけに依存し、`tools`・`session`・`agent` に依存しない。直前のセッションの記録を読むのは `cli` の側で行う。
- `session::record::Header` の `benitoite`（`RecordedBenitoite`）の欄を、04-01 と 01-05 の欄（実のパス、実行ファイルの SHA-256、`--version=json` の `version` と `build`、`SKILL.md` の `contentHash`（P2-06 が埋める。この作業では `None`））を持つ形に改める。既存の記録を読めるように、加える欄は `#[serde(default)]` とする。`skill_version` は P2-06 で除くので、この作業では残す。ヘッダは `agent/mod.rs` の `Agent::construct` で書くので、確かめた処理系は `ToolContext::with_benitoite(Option<Arc<Processor>>)` で渡し、`Agent` がヘッダに写す。
- 問い合わせの起動の文脈の型（例: `QueryContext { helper, scratch, placeholders, cwd, env, locations, denied_candidates, launcher: QueryLauncher, allowed_programs }`）。`launcher` は `pub(crate) enum QueryLauncher { Sandboxed, Unsandboxed, #[cfg(test)] TestDirect }` とする（後述の 2 と 4）。
- `ToolSet::new` は変えない。Benitoite の道具を一覧に加える形は P2-06 で決める。
- 起動の通知（`Environment` の起動の通知。今は `Option<&'static str>`）を `Vec<String>` に改め、版の警告と処理系が変わったことを `session_start` の `notice` に加えられる形にする。

## 作るもの

### 1. 処理系を探す

- 04-01 の順（利用者の設定の `benitoite.path`、なければ `PATH`）で探し、シンボリックリンクを解いた実のパスにする。`PATH` の要素のうち、相対のパスと、プロジェクトのディレクトリとその下を指すもの（実のパスで比べる）は飛ばす。
- 実のパスのファイルを、次の規則で確かめる。規則は、利用者の設定の `restricted.allowed_programs` にも同じ関数で当てる（03-02 の「制限モードで実行を許すプログラム」）。
  - 通常のファイルで、先頭のバイト列が、macOS では Mach-O（32・64 ビットとユニバーサル）の印、Linux では ELF の印であること（OS で分ける。ユニバーサルの印 `0xCAFEBABE` は Java の class ファイルと同じで、Linux では binfmt_misc で起動されうるので受けない）。`#!` で始まらないこと。
  - 書ける場所（プロジェクトのディレクトリ、セッションの一時ディレクトリ）の外にあること。
  - `benitoite.path` が相対のパスなら、設定の誤りとする（`allowed_programs` と同じ）。
  - Linux では、ELF のプログラムヘッダに `PT_INTERP` がないこと（静的リンク）。読むのは先頭の固定の長さ（例: 64 KiB）までとし、ヘッダが範囲の外を指せば満たさないとする。
  - `allowed_programs` の各項目は、実のパスのファイルの名前が 03-02 の名前の一覧に当たらないこと（`python*` のような `*` は前方一致）。当たれば、制限モードの起動を止める。
- 満たさないときの扱い: 制限モードは起動を止める。通常モードは Benitoite の道具を外して続ける（理由を診断に積む）。`allowed_programs` は制限モードのときだけ確かめる。

### 2. 問い合わせの起動

- `src/benitoite/` に、利用者のスクリプトを実行しない処理系の起動（`--version=json`、`skill list --format=json`、`skill show --format=json <名前>`、のちの `check`・`test --check`）を一つの関数にまとめる。例: `Processor::query(&self, args: &[OsString], limits: QueryLimits, cancelled: &AtomicBool) -> Result<QueryOutput, BenitoiteError>`。
- 方針は 03-02 の「制限モードの方針」の、利用者のスクリプトを実行しない起動のもの（読める範囲は既定、書ける場所は一時ディレクトリだけ、IP の通信なし、起動できるプログラムは処理系の実のパスだけ、書ける場所での実行の拒否）とする。方針を作る関数は、P2-08 が実行の方針を作る関数と同じ場所（例: `sandbox::restricted_policy(access: &RestrictedAccess, locations: &WritableLocations, denied_candidates: &[PathBuf]) -> Result<SandboxPolicy, SandboxError>`。`RestrictedAccess` はプロジェクトに書けるか、IP の通信の可否、起動できるプログラムの実のパスの一覧、読める範囲を限るか（P2-09）を持つ。エフェクトの名前から `RestrictedAccess` を作るのは `benitoite` の側で行い、`sandbox` は `benitoite` に依存しない）に置き、この作業ではエフェクトのない場合だけを使う。
- 起動は `sandbox::run` を通し、作業ディレクトリはプロジェクトのディレクトリ、環境は `child_environment`（提供者のキーを除く）、標準入力は空、時間の上限は問い合わせの既定 10 秒（検査は P2-07 で 30 秒を渡す）、止める合図は `SIGINT`、10 秒で終わらなければ `SIGKILL` とする。環境は `child_environment(.., Some(("restricted", false)))` とする。macOS とサンドボックスを掛けない起動は、P4-07 の `__session` を通る（03-02 の「端末からの切り離し」）。
- 起動の形は `QueryLauncher` で決める。`Sandboxed` は上の方針のサンドボックスの中で起動する（制限モード、および通常モードで OS のサンドボックスを使えるとき。通常モードではサンドボックスの方針（`danger-full-access` を含む）によらない。04-01「処理系の起動」の 2026-10-18 の【方針】）。`Unsandboxed` は、通常モードで `--no-sandbox` のときだけ使い、`__session` を通してサンドボックスを掛けずに起動する。`#[cfg(test)] TestDirect` は、モジュールの中のテストだけが使い、方針を作って検証までしたうえで、`Command::new(処理系)` に `process_group(0)` を付けて直接起動する（テストの実行ファイルは `__session` を持たないため）。本番の呼び出し元（`exec::run`、`tui.rs`）は `TestDirect` を作れない。方針の作成と検証は、どの形でも通る。
- 標準出力は 16 MiB を上限に受け取り、超えたら読むのをやめて子プロセスを止め（`sandbox::run` は変えない。問い合わせが自分の `Arc<AtomicBool>` を作って `run` の `cancelled` に渡し、受け取り先が 16 MiB を超えたら受け取りをやめてこの印を立てる。`std::thread::scope` の見張りで、呼び出し元の `cancelled` もこの印へ写す。`RunEnd::Cancelled` で超過の印があれば、超過の誤りとして返す）、04-01 の英文（`The inspection report from benitoite exceeded 16 MiB and was not read. Pass fewer or narrower paths.`）の誤りを返す。英文は `benitoite/text.rs` に置く。標準エラー出力は先頭の 64 KiB を保ち、残りは捨てる（超えても誤りにせず、止めない）。
- JSON の読み方は、標準出力の全体を一つの JSON のオブジェクトとして読み（前後の空白は許し、二つ目のオブジェクトと末尾のほかの文字は誤り）、欄を名前で読む部品として共有する。`schemaVersion` と `kind` の照合もこの部品で行う。
- 処理系の誤り（終了状態が 0 でない、読めない、時間切れ、上限超え）は、モジュールの誤りの型 `BenitoiteError` で表す。

### 3. 版と機能の確かめ、ハッシュ

- 処理系が見つからないとき、使えないとき、制限モードでサンドボックスを掛けられないときの `exec` の終了状態は 2 とする（05-01 の「終了状態」）。
- 起動のときに一度だけ、`--version=json` を問い合わせ、04-01 の条件（終了状態 0、一つの完全な JSON、`schemaVersion` 1、`kind` が `"version"`、`formats` の 7 つのキーの配列がどれも 1 を含む）を確かめる。結果は `Processor`（実のパス、`version`、`build` の JSON の値、実行ファイルの SHA-256）として保つ。
- `version` が `0.0.7`（システムプロンプトの対応の説明を確かめた版。定数 `benitoite::PROMPT_VERIFIED_VERSION` として置く）と違えば、警告を診断に積み、`session_start` の `notice` にも書く（英文は `benitoite/text.rs`）。
- 実行ファイルの SHA-256 は、確かめた実のパスのファイルを開いた記述子から読んで求める（読む間にパスが差し替えられても、確かめたファイルの値になるように、一度開いた記述子で形の確かめとハッシュを行う）。
- 同じプロジェクトのディレクトリの直前のセッションの記録のヘッダの値と違えば、前と今の版とともに、変わったことを診断と `session_start` の `notice` に積んで続ける（英文は `benitoite/text.rs`）。直前の記録がないか、処理系の欄がないときは何も示さない。直前のセッションは、`continue_session`（錠を取る）を使わず、今のセッションの ID を除いて、ヘッダの `benitoite` だけを読む関数を `session/scan.rs` に新しく置いて探す（`SessionEntry` に欄を足さない）。再開のときは、再開したセッションのヘッダと比べ、ヘッダは書き直さない（01-05）。
- ヘッダに、実のパス、SHA-256、`version`、`build` を書く。

### 4. 起動の流れへの組み込み

- `cli::exec::prepare` で、書ける場所を決めた後、セッションの一時ディレクトリを作った後に処理系を確かめる（問い合わせの方針は一時ディレクトリを要する）。Linux の起動に要る `scratch`（セッションの `sandbox/`）と `placeholders` は、今は通常モードの枝だけで作っているので、モードで分ける前に作り、制限モードの `check_sandbox` と問い合わせにも使う。
- 制限モードでは、OS のサンドボックスを掛けられることを、通常モードと同じ `check_sandbox` で確かめ、掛けられなければ起動を止める（制限モードには `--no-sandbox` がない）。
- 通常モードでも、`--no-sandbox` でなければ `check_sandbox` を呼び（`danger-full-access` のときも）、掛けられれば問い合わせを `Sandboxed` で起動する。掛けられなければ Benitoite の道具を外して続け、理由を診断に積む。`--no-sandbox` のときは `Unsandboxed` で起動する。
- 問い合わせの起動は、`QueryLauncher` で差し替えられる形にする。制限モードの `check_sandbox` の差し替えとは別の口にする。テスト（`src/cli/exec/tests.rs` などのモジュールの中のテスト）だけが `TestDirect` を渡せる。本番の経路に、制限モードでサンドボックスを掛けずに処理系を起動する設定やオプションを作らない。
- 既定のモードは制限モードなので、`exec` や対話の画面を既定のモードで起動する既存の統合テスト（`tests/exec.rs`、`tests/exec/resume.rs`、`tests/tui/pty.rs`、`tests/cli.rs` など約 30。P4-08 が加えたものを含む）は、処理系とサンドボックスを要するようになる。制限モードであることを確かめないテストは、`--mode normal --no-sandbox` に改める（`--sandbox danger-full-access` では問い合わせがサンドボックスの中で動き、Codex の中で重ねて掛けられないため）。制限モードを要するもの（道具の一覧、モードの記録、モードの変更の確認など）は、`tests/sandbox_exec_restricted.rs` などの `tests/sandbox_*.rs` へ移す。`scripts/check.sh` の aarch64 の段（Landlock がない）では、移したファイルと `tests/sandbox_benitoite.rs` を除き、理由を示す。移したものと改めたものの一覧を報告に書く。
- 統合テストの準備では、`PATH` か `benitoite.path` を固定し、利用者が入れた `benitoite` を拾わない。通常モードの統合テストで新しく出る「処理系がない」の診断の行は、期待値を直す。
- 対話の画面の `/status` の処理系の欄（P4-08 では `Interpreter: not checked`。`ui/tui/text.rs`、`Status` は `ui/tui/mod.rs`）に、見つけた処理系のパスと版を示す（05-02「`/` で始まるコマンド」）。処理系がなく Benitoite の道具を外したときは、その旨を示す。

### 5. テストの処理系の用意

- `scripts/check.sh` は、環境変数 `BENITOITEH_TEST_BENITOITE`（手元の macOS の処理系）を受け取り、テストに渡す。Linux の段には、`BENITOITEH_TEST_BENITOITE_X86_64` と `BENITOITEH_TEST_BENITOITE_AARCH64`（静的リンクの処理系）を渡し、テストの中では `BENITOITEH_TEST_BENITOITE` として見せる。aarch64 の段は、`.cargo/config.toml` の runner の `scripts/container-run.sh` が `-e BENITOITEH_TEST_BENITOITE=$BENITOITEH_TEST_BENITOITE_AARCH64` を付け、処理系の親のディレクトリを読み取りだけで同じパスに見せる。x86_64 の段は、`scripts/remote-linux-run.sh` が `--sync` で処理系のファイルだけを `rsync -aR` で同じパスに送り、`podman run` に `-e BENITOITEH_TEST_BENITOITE=$BENITOITEH_TEST_BENITOITE_X86_64` を付ける。`--sync` は `--exclude=/work/` を加え、`work/` の全体（数 GB）を送らない。
- 処理系を要するテストは、環境変数がなければ、理由を示して失敗する（飛ばさない）。テストの共通の部品（`tests/support/`）に、処理系のパスを得る関数を一つ置く。
- Codex の中では、`BENITOITEH_TEST_BENITOITE=/Users/tecogonaz/src/BenitoiteHarness/work/benitoite-v0.0.7/target/release/benitoite` を設定して `scripts/check.sh --inside-codex` を走らせる。

- `/new`（P4-08）は、安全に関わる値を今のセッションの値に固定する（`SessionOverrides`）。この作業で `benitoite.path` を使い始めるので、`/new` では、確かめた処理系の実のパスを固定し、読み直した設定の `benitoite.path` で置き換えない（新しいセッションでも形の確かめとハッシュは行い直す）。テストを加える。

## 触るファイル

- `crates/benitoiteh/src/benitoite/`（新。`mod.rs`、`text.rs`、版の確かめ、問い合わせ、実行ファイルの形の確かめ）
- `crates/benitoiteh/src/lib.rs`（モジュールの宣言）
- `crates/benitoiteh/src/sandbox/`（制限モードの方針を作る関数）
- `crates/benitoiteh/src/config/`（`allowed_programs` の確かめを呼ぶ箇所）
- `crates/benitoiteh/src/cli/exec.rs` と `src/cli/exec/tests.rs`、`src/cli/tui.rs`（`prepare` の引数）
- `crates/benitoiteh/src/session/`（ヘッダの欄、直前のセッションの処理系の欄を読む口）
- `crates/benitoiteh/src/agent/mod.rs`（ヘッダへの写し）、`crates/benitoiteh/src/tools/mod.rs`（`ToolContext::with_benitoite`）、`crates/benitoiteh/src/context/`（起動の通知の `Vec<String>`）
- `crates/benitoiteh/src/ui/tui/`（`/status` の処理系の欄）
- `crates/benitoiteh/tests/`（`support/`、移す統合テスト、新しい `tests/sandbox_benitoite.rs`）
- `scripts/check.sh`、`scripts/remote-linux-run.sh`、`scripts/container-run.sh`（処理系のパスの受け渡し）

## テスト

- 探し方: `benitoite.path` が優先されること。`PATH` の相対の要素、プロジェクトの下を指す要素（シンボリックリンクで指すものを含む）が飛ばされること。見つからないときの扱い（制限モードは止まり、通常モードは続く）。
- 実行ファイルの形: `#!` のスクリプト、テキストのファイル、ディレクトリ、書ける場所の中のファイルを拒むこと。Mach-O と ELF の印を受けること。Linux で、`PT_INTERP` を持つ ELF（動的リンク）を拒み、静的リンクを受けること（ELF は小さなバイト列のファイルを作って確かめてよい。本物の処理系でも確かめる）。`allowed_programs` の名前の一覧に当たる名前（`python3.12`、`bash`、シンボリックリンクで名前を変えたものは実のパスの名前で判定）で起動が止まること（安全の性質のテスト。07-01）。
- 版の確かめ: 本物の `v0.0.7` で通ること。`formats` のキーが欠けたもの、1 を含まないもの、`kind` が違うもの、JSON が二つ続くもの、末尾に文字があるもの、終了状態が 0 でないもの、16 MiB を超える出力、10 秒を超えるものを、使えないと判定すること。JSON の照合は、純粋な関数にバイト列を渡して確かめる。時間切れは `QueryLimits` に短い上限を渡して確かめ、10 秒と猶予を待たない。出力の形の偽物は、`sandbox-probe` に決まった出力を返す操作を足して使ってよい。`v0.0.6` 相当は、`sandbox-probe` に `--version=json` を渡せば知らない操作として 0 でない状態で終わるので、それで確かめる。
- 版の数が `0.0.7` でないときに警告が出て、続くこと。
- ハッシュ: 直前のセッションと違う処理系で起動すると、変わったことが示されること。同じなら示されないこと。
- 問い合わせの方針（`tests/sandbox_benitoite.rs`。macOS と Linux）: 本物の処理系は書く・つなぐ・起動するを試みないので、`sandbox-probe` を起動するプログラムとし、同じ `restricted_policy`（エフェクトなし）で、プロジェクトへの書き込み、TCP の接続、`/bin/sh` の起動、一時ディレクトリに写したファイルの起動が拒まれることを確かめる。あわせて、本物の処理系の `--version=json` がサンドボックスの中で通ることと、子プロセスの環境に提供者のキーがないことを確かめる。
- 安全の性質（07-01）: プロジェクトの設定に書いた `[benitoite] path` が使われないこと。`PATH` の、一時ディレクトリを指す要素の処理系を拒むこと。
- 通常モードの `--no-sandbox` で、問い合わせが `__session` を通ってサンドボックスなしに起動すること。通常モードでサンドボックスを掛けられないとき（差し替えで失敗させる）に、Benitoite の道具を外して続けること。
- `/status` の処理系の欄。
- 制限モードで、サンドボックスを掛けられない（差し替えで失敗させる）ときに起動が止まること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、加えた公開の関数と型のシグネチャ、`benitoite/text.rs` に足した英文の全文、移した統合テストの一覧、ヘッダの例（一行）、実行ファイルの大きさの変化を書く。
