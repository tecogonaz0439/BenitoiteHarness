# P4-10 `doctor` の処理系の項目

## 目的

P4-05 で「確かめていない」とした `doctor` の処理系の項目を作る（設計書 05-01 の「doctor」）。処理系が見つかり、`--version=json` が読め、要る出力の形式を持つこと、版とビルドの情報、版がシステムプロンプトの対応の説明を確かめた版と一致するか、`skill show` で `SKILL.md` を得られることを示す。制限モードの規則の確かめを、`benitoiteh` 自身の `--version` の代わりに、処理系の `--version` を許して `/bin/sh` を拒む形に改め、Linux では処理系が静的にリンクされていることを示す。

## 読む設計書

- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「doctor」（項目、四つの状態、終了状態、`--json`、`--project`、処理系の項目を連携まで「確かめていない」とした段落）
- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「処理系の見つけ方と版の確かめ」「取り込む文書と版」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「制限モードで実行を許すプログラム」（静的リンク、実行ファイルの形）
- 既存のもの: `src/cli/doctor/`（`mod.rs` の `Item`・`Status`、処理系の項目の仮の文 `text::BENITOITE`、`probe.rs` の `rule`（`benitoiteh` 自身の `--version` を許す方針）、Linux の `DYNAMIC` の判定）、P2-05 の `benitoite` モジュール（探し方、実行ファイルの形の確かめ、`--version=json` の確かめ、問い合わせ）、P2-06 の Skill の問い合わせ

## この作業が認める型の変更

なし。P2-05・P2-06 の公開の口を使う。足りない口があれば、小さな公開の関数を `benitoite` に足してよい（報告に書く）。

## 作るもの

- 処理系の項目（`benitoite`）を、次の小さな項目に分けて示す。どれも、確かめた・使えない・確かめていない・注意のどれかとする。
  - 見つけ方: どこで見つけたか（設定の `benitoite.path` か `PATH`）と実のパス。見つからなければ使えない。
  - 実行ファイルの形: OS の実行ファイルの形式であること、`#!` でないこと、`--project` のプロジェクトのディレクトリの外にあること、Linux では静的リンクであること。満たさなければ使えない。
  - 版と機能: `--version=json` の確かめ（P2-05 と同じ関数）。`version`、`build` の `commit`・`dirty`・`target` を示す。`formats` の要るキーが欠ければ使えない。`version` が対応の説明を確かめた版（`0.0.7`）と違えば注意。
  - Skill: `skill show --format=json SKILL.md` が P2-06 の条件を満たすこと。`contentHash` の値を示す。
  - 実行ファイルの SHA-256 を示す。
- 制限モードの規則の確かめ（今の `rule`）を、処理系が見つかり形の確かめを通ったときは、処理系の実のパスだけを許す方針で `benitoite --version` を起動して成功し、`/bin/sh` の起動が拒まれる形に改める。処理系がないときは、今の `benitoiteh` 自身を使う確かめを残し、処理系の確かめではないことを文に書く。
- 問い合わせは、P2-05 の問い合わせの起動（サンドボックスの中）を使う。サンドボックスを掛けられない環境では、処理系の項目を確かめていないとし、理由を示す。
- `--json` の各行に、加えた項目を同じ形で出す。秘密を示さない今の規則は変えない。
- 英文は `cli/doctor/text.rs` に置き、`text::BENITOITE` の仮の文を除く。
- 05-01 の「処理系の項目は、Benitoite の処理系との連携を作るまで『確かめていない』とし」の段落は、この作業の後にオーケストレータが直す。

## 触るファイル

- `crates/benitoiteh/src/cli/doctor/`（`mod.rs`、`probe.rs`、`text.rs`）
- `crates/benitoiteh/src/benitoite/`（足りない口があれば）
- 関係するテスト（サンドボックスを要するものは `tests/sandbox_*.rs`）

## テスト

- 本物の `v0.0.7` で、処理系の項目がすべて確かめたになり、終了状態 0 になること（サンドボックスの中で）。
- 処理系がない、`#!` のスクリプトを `benitoite.path` に書いた、`formats` の欠けた処理系（決まった出力を返す差し替え）で、それぞれ使えないになり、終了状態 1 になること。版が `0.0.7` でない処理系で注意になること。
- 処理系を許す方針で `/bin/sh` が拒まれること。
- `--json` の行の形。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、`doctor` の出力の例（文と `--json`）、足した英文の全文を書く。

## P2-05〜P2-12 の実装の後の補足（2026-10-18）

P2-05〜P2-12 を取り込んだ後に、この文書を実装に突き合わせた。上の記述と食い違うところは、この節を正とする。設計書 05-01「doctor」も同じ日に改めた。

- 使う既存の口: `benitoite::discover`・`validate_executable`（`pub`）、`Processor::inspect`・`QueryContext`・`QueryLauncher`（`pub(crate)`）、`Processor` の `path`・`version`・`build`・`sha256`、`PROMPT_VERIFIED_VERSION`、`skill::SkillDocuments::from_processor`・`document`・`skill_parts`・`SkillDocument::content_hash`・`query_reason`、`config::child_environment`、`UserConfig::referenced_env_vars`、`sandbox::restricted_policy`、`benitoite::display::one_line`。`version_report` は非公開のままとし、`formats` の欠けは `Processor::inspect` の `Invalid(text::UNSUPPORTED)` として受ける（このとき版と build は示さない）。
- 項目の名前は `benitoite.discovery`・`benitoite.executable`・`benitoite.version`・`benitoite.skill` とし、元の `benitoite` の一項目と `text::BENITOITE` を除く。SHA-256 は `benitoite.executable` の説明に `sha256=<64 字>` として含める。JSON の行の形（`type`・`name`・`status`・`detail`）は変えず、欄を足さない。
- 見つけ方: 出どころは `config.benitoite.path` の有無で決める（`discover` は出どころを返さない）。利用者の設定が読めないときは、四つの項目をすべて `unchecked`（`text::CONFIG_REQUIRED`）とする。`Probe` を作れないときも、四つを `unchecked` としてその理由を示す。見つからないときは `unavailable`。設定のパスを説明に添える。
- 制限モードの書ける場所は、`validate_writable_with_executable(paths, project, &probe.write, &[], &probe.helper)`（`additional` は空。`cli/exec.rs` の制限モードと同じ）で作り、`discover`・`validate_executable`・`QueryContext` に使う。通常モードの `normal()` の検査は今のまま残す。
- 実行ファイルの形: `validate_executable(.., false)` の誤りの文（`benitoite::text::INVALID_PROGRAM`）をそのまま示す。条件ごとに分ける新しい口は作らない。
- 制限モードの規則の確かめ（`probe.rs` の `rule`）: 処理系が見つかり形の確かめを通ったときは、方針を `sandbox::restricted_policy(&RestrictedAccess { write_project: false, allow_ip: false, programs: vec![処理系の実のパス], readable: ReadAccess::All }, &制限の locations, &denied_candidates(paths))` で作る（問い合わせと同じ方針）。正の確かめは、処理系の `--version=json` が終了状態 0 で終わり、残ったプロセスがないこととする。負の確かめ（`/bin/sh`）と判定は今のままとする。処理系がないか形が通らないときは、今の `benitoiteh` 自身を使う確かめを残し、説明に処理系の確かめでないことを書く。Linux の `text::DYNAMIC` は、この後者の場合にだけ使う。
- 問い合わせ: 規則の確かめが `ok` のときだけ、`Processor::inspect` を `QueryLauncher::Sandboxed` で呼ぶ。`QueryContext` は、`helper`・`scratch`・`placeholders` に `Probe` の場所（`Probe` に取り出す `pub(super)` の口を足す）、`env` に `child_environment(std::env::vars_os(), &config.referenced_env_vars(), &probe.write, Some(("restricted", false)))`（今の `child_environment` の引数に合わせる）、`denied_candidates` に `super::denied_candidates(paths)` を渡して作る。規則の確かめが `ok` でなければ、`benitoite.version` と `benitoite.skill` を `unchecked` とし、制限モードのサンドボックスが要ることを示す。`Unsandboxed` は使わない（04-01「処理系の起動」。doctor には `--no-sandbox` がない）。
- 版: `version`、`build` の `commit`・`dirty`・`target` を示す。`null` と、欠けた欄や文字列でない欄は `unknown` と示し、`dirty` の `null` を「変更なし」と示さない（04-01）。処理系が申告した文字列は `display::one_line` を通す。版が `PROMPT_VERIFIED_VERSION` と違えば `warning` とする（説明は `version_warning()` の文から先頭の `Warning: ` を除いたものでよい）。
- Skill: `SkillDocuments::from_processor` の `document("SKILL.md")`（一覧と本文の二回の問い合わせ）の後に `skill_parts` を確かめ、`cli/exec.rs` の `initialize_skill` と同じ条件にする。失敗の理由は `skill::query_reason` で一行にする。成功したら `contentHash` を示す。
- 要約: `RESTRICTED_OK`・`RESTRICTED_UNAVAILABLE` から "(Benitoite not checked)" を除く。`Summary.restricted_sandbox` の意味は変えない。
- 項目を組み立てる部分は、得た結果（見つけ方と形の `Result`、`inspect` の `Result` か版・build・ハッシュ、Skill の `Result`）を受け取る関数に分け、`src/cli/doctor/` の単体テストで、`Err(Invalid(UNSUPPORTED))` で `unavailable`、版 `0.0.8` で `warning`、規則が通らないときに `unchecked` を確かめる。`formats` のキーごとの欠けと版の判定は `src/benitoite/tests.rs` が確かめているので繰り返さない。決まった出力を返す差し替えの実行ファイルは作らない。
- `tests/doctor.rs`（Codex の中でも走る）: `support/processor.rs` を取り込み、処理系がない（`PATH` を空にする）、`#!` のスクリプト、プロジェクトの中の処理系（設定と `PATH` の両方）で使えないになり終了状態 1 になること、本物の処理系で見つけ方と形が `ok` になること、`benitoite.version` と `benitoite.skill` は要約の `restricted_sandbox` が偽なら `unchecked` になることを確かめる。`json_contract_and_project_selection_leave_no_files_behind` の `benitoite` を `unchecked` とする確かめを、新しい項目に合わせて改める。
- `tests/sandbox_macos.rs` と `tests/sandbox_linux.rs` の `doctor_verifies_restricted_and_normal_sandboxes` は、設定に `[benitoite] path` を `support/processor.rs` の `benitoite()` から書き、`benitoite.*` を含むすべての項目が `ok`（Linux の `sandbox.linux_scope` だけは `warning`）、終了状態 0、プロジェクトと状態と一時の場所に何も残らないことを確かめるよう改める。Codex の中では走らないので、オーケストレータが外で確かめる。
- 範囲外: `restricted.allowed_programs` の検査、サンドボックスの外での問い合わせ、通常モードの書ける場所の中にある処理系の注意。
- 触るファイルに、`tests/doctor.rs`、`tests/sandbox_macos.rs`、`tests/sandbox_linux.rs` を加える。
