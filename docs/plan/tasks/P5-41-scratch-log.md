# P5-41 一時的なスクリプトの確認なしの既定に Log を加える

- 状態: 完了
- 依存: P5-40

## 目的

`benitoite_scratch` の確認なしの範囲の既定に `Log` を加える。2026-10-27 に設計者が、[P5-40](P5-40-processor-v0.0.12.md)の「設計者に判断を求める候補」を認めたことによる変更である。P5-40 は、処理系の版の更新では既定を広げず、候補を設計者の判断に残した。本作業でその決定を実装と現在の設計書へ反映する。P5-40 の作業文書は当時の記録として変更しない。

理由: 未処理の `Log` の既定の出力先は標準エラー出力だけである。ファイルなどに出す受け手の `File.Write` などは別のエフェクトとして残り、その確認と OS の制限を別に掛ける。この根拠を設計者が認め、確認なしの既定に加えると決めた。処理系の仕様の出典は、Benitoite の `v0.0.12`（コミット `d0678ddb7e82da77715ea2e01ef974d06d41814b`）の [Skill「Checked values and local builders」](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.12/crates/benitoite/skill/SKILL.md)と [「File and JSON log sinks」](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.12/crates/benitoite/skill/references/idioms.md#file-and-json-log-sinks)である。

## 読む文書

- `AGENTS.md`、[作業の進め方](../00-common.md)、[作業の間の約束](../10-interfaces.md)の「実行の前の検査とスクリプトの実行」「テストの実行と非対話の `--allow`」「一時的なスクリプト」。
- [P5-40](P5-40-processor-v0.0.12.md)の「Log の確認と許可の案」「設計者に判断を求める候補」。
- [道具](../../design/01-agent/01-03-tools.md)の `benitoite_run`・`benitoite_test`・`benitoite_scratch` の各節、[文脈の管理](../../design/01-agent/01-04-context.md)の「システムプロンプトの構成」「Benitoite の Skill の置き方」。
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「制限モードの確認」「一時的なスクリプト」「非対話での扱い」、[OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「制限モードの方針」、[Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「実行の前の検査の報告」。
- [テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」「LLM に見せる文の固定」「安全の性質のテスト」「処理系の用意」「完了の前に通す検査」と、`.claude/skills/test-audit/SKILL.md`。
- 後述の「触るファイル」に挙げるソース、プロンプト、既存のテスト。処理系の仕様は、`assets/benitoite/v0.0.12/skill/` の上記の写しでも確かめる。

## 作るものと手順

1. `crates/benitoiteh/src/benitoite/effects.rs` の `scratch_auto_effects` の既定に `Effect::Log` を加える。既定は、エフェクトなし、`Console.Write`、`Log`、`State`、`Process.Exit` とする。「エフェクトなし」は空集合が範囲に収まることを指し、新しいエフェクトの名前を作らない。利用者の設定との和を `BTreeSet` で作る処理は保ち、設定に `Log` が書かれていても集合として扱う。
2. `crates/benitoiteh/prompts/tools/benitoite_scratch.md` の既定の一覧に `Log` を加え、P5-40 で加えた `Log is not automatic by default ...` の文を改める。`Log` は既定で確認なしになり、受け手の `File.Write` などには別の許可が要ることを英語で示す。LLM が許すエフェクトを選ばないという説明を保つ。
3. 設計書 03-03「一時的なスクリプト」の既定の一覧を更新し、P5-40 で加えた `Log` を既定に含めない【方針】の段落を、2026-10-27 の設計者の【決定】と理由に改める。利用者の `restricted.scratch_auto_effects` に `Log` を引き続き書けることと、既定と重なっても集合として扱うことを明記する。同じ節にある `State`・`Process.Exit` の経緯は残す。
4. `git grep` で同じ既定や旧方針の記述を探し、現在の仕様を揃える。たとえば次を変更の前後に走らせる。

   ```sh
   git grep -n -E 'scratch_auto_effects|Log.*既定|既定.*Log|Console[.]Write.*State|automatic set|Log is not automatic' -- crates/benitoiteh docs/design docs/plan
   ```

   01-03「benitoite_scratch（制限モード）」、03-02「制限モードの方針」、01-04 のプロンプトの対応、04-01「実行の前の検査の報告」、08-02 の ADR 0345 の対応の行を点検する。既定に含めないという文と、現在の既定を列挙した箇所を改める。03-02 と 04-01 は、`Log` の出力と受け手のエフェクトの区別を保ち、確認なしの既定との関係が分かるようにする。01-04 など、一致する記述がなく現在の説明と矛盾しない箇所は変更せず、その理由を結果に記録する。P5-40 以前の作業文書、過去の測定・評価・外部のレビューの記録は置換しない。検索の一致が 0 件になることを完了条件にしない。
5. `docs/plan/10-interfaces.md` の「一時的なスクリプト」に、`scratch_auto_effects` の新しい既定を明記する。型とシグネチャは変えない。
6. 次節の方針で既存のテストを直す。`tests/agent_loop/allow.rs` の拒否の文に固定した既定の一覧も更新する。表示の順は `effects::names` の綴りの順に揃え、`Console.Write, Log, Process.Exit, State` とする。
7. `BENITOITEH_TEST_BENITOITE` に v0.0.12 の macOS の処理系を指定し、`cargo run --offline -p benitoiteh --example update_context_expected` でシステムプロンプトの固定の期待値を更新する。`examples/update_context_expected.rs` の既存の仕組みを使い、生成の処理自体は変えない。生成後に差分と変更された本文を読み、今回の道具の説明だけが変わったことを確かめる。Skill の本文、道具の引数と JSON の定義は変えず、生成し直して差分がなければ変更に含めない。期待値の生成だけを正しさの根拠にしない。
8. 「確かめること」と内側の検査を実施し、本作業文書の「確かめた結果」に、実際に行った検査と外へ残した検査を記録する。コミットしない。

### 保つ規則

- `benitoite_run` と `benitoite_test` は、一時スクリプトの確認なしの範囲を引き継がない。非対話では `--allow` の範囲だけ、対話ではその対象について確認した範囲だけを許す。テストの `Assert.Check` の特例も保つ。
- 確認なしの実行の狭い読み取り範囲と、OS のサンドボックスの方針を保つ。`Log` は書ける場所、IP の通信、起動できるプログラムを広げない。受け手に残る `File.Write` などを `Log` と一緒に自動で許さない。
- `restricted.scratch_auto_effects` に `Log` を書けることと、プロジェクトの設定から範囲を変えられないことを保つ。設定の既定値 `[]` は利用者の追加分を表すため、そのままとする。
- 03-03「非対話での扱い」の【決定】を保つ。確認なしの範囲と `--allow` の和はその一回だけに使い、一時スクリプトのパスについて許した範囲として残さない。残すのは `--allow` で許した分だけである。

## 触るファイル

以下は、点検後の実装で変更する範囲である。作業文書の執筆時点では、本作業文書と `docs/plan/README.md` の P5-41 の行だけを変更する。

- `crates/benitoiteh/src/benitoite/effects.rs`。
- `crates/benitoiteh/prompts/tools/benitoite_scratch.md`。
- `crates/benitoiteh/tests/sandbox_benitoite_scratch.rs`、`crates/benitoiteh/tests/agent_loop/allow.rs`。
- `crates/benitoiteh/tests/expected/system-restricted.txt`、`crates/benitoiteh/tests/expected/system-restricted-read-plain.txt`。既存の example で更新する。
- `docs/design/03-safety/03-03-approval.md`、`docs/design/01-agent/01-03-tools.md`、`docs/design/03-safety/03-02-sandbox.md`、`docs/design/01-agent/01-04-context.md`、`docs/design/04-benitoite/04-01-benitoite-integration.md`、`docs/design/08-appendix/08-02-pre-study-mapping.md`。上の決定に対応する箇所だけを扱い、点検で変更が不要なものは結果に記録する。
- `docs/plan/10-interfaces.md` と、本作業文書の「確かめた結果」。本作業と一覧の状態の更新はオーケストレータが行う。

依存、Skill の写し、処理系のファイル、P5-40 以前の作業文書と過去の結果は変更しない。本物の LLM を呼ばず、利用者の設定・状態、キーチェーン、トークン、`~/.ssh` を読まない。

## テスト

既存のテストの入力と期待値を改める。同じ契約を層ごとに重ねて加えず、既定の集合の値だけを写した単体テストは作らない。

| 対象 | 変更と守る契約 |
|---|---|
| `tests/sandbox_benitoite_scratch.rs` の `configured_log_does_not_authorize_later_scripts_or_tests` | 名前と関門のコメントを新しい既定に合わせる。利用者の追加がない場合と、設定に `Log` を明記した場合の両方で、一時スクリプトの実際のログ出力を確認なしで得る。その直後、同じ一時ファイルの `benitoite_run` と Log を使う `benitoite_test` が、空の `--allow` ではともに拒否されることを確かめる。従来の「設定なしなら最初も拒否」の期待を除き、拒否の件数と一連の処理の結果を揃える。模擬の提供者、本物の処理系、本物の OS のサンドボックスを使う |
| `tests/agent_loop/allow.rs` の `named_allow_set_is_honored_without_asking_the_approver` | `benitoite_run`・`benitoite_test` の Log の行は、`--allow Log` の有無で許否が変わる契約を保つ。`benitoite_scratch` の行は、`Log` と `File.Write` を宣言し、書き込みを含む入力へ改め、必要な明示の許可を `File.Write` とする。空の `--allow` では File.Write のため拒否され、`--allow File.Write` があれば二度目の検査まで進み、確認を求めないことを確かめる。この fixture の `execution context is unavailable` を、実行が成功した結果と扱わない |
| 同じ `allow.rs` の拒否の文の期待値 | `third_denial_stops_after_turn_end_and_records_all_calls_without_approval_records` と `scratch_noninteractive_allow_unions_automatic_effects_and_reports_the_union_on_denial` の一覧に Log を加える。拒否の連続で止まる条件と、和を判定・再検査に渡す条件は変えない |
| システムプロンプトの固定の期待値 | 既存の文脈のテストで、道具の説明と固定した本文の一致を確かめる。生成後の差分を読んで採用する |

受け手に残るエフェクトの実行時の扱いと OS の書き込みの拒否は、既存の `tests/sandbox_benitoite_run.rs` の `log_file_sinks_require_their_own_effects_and_respect_os_write_scope` が守る。本作業では同じ内容のテストを加えず、外の全体の検査でこれも通す。狭い読み取り範囲と、`--allow` で許した分だけをパスに残す既存のテストも保つ。

### 作成時の関門

| 問い | 答え |
|---|---|
| 守る観察可能な振る舞い・契約 | 既定の Log の一時スクリプトが確認なしで実行され、設定に Log を重ねても同じになること。その許可を後の run・test に引き継がず、File.Write は別の許可を要すること。拒否の文と LLM に見せる説明が現在の既定と一致すること |
| 捕まえる現実的な退行 | 既定に Log を加え忘れて最初の実行を拒む、自動の Log をパスの許可へ残して後の run・test を通す、Log とともに File.Write も許す、File.Write を明示しても自動の Log との和を渡し忘れて再検査で止まる退行 |
| 既存のテストで足りない理由と主となる境界 | P5-40 の二つのテストは、設定なしの Log を拒む前提である。新しい既定の実行と非継承は既存の sandbox の通しのテストを改めて守る。ループの表の scratch の行は明示の File.Write と自動の Log の和を扱い、許可の判定と再検査の受け渡しを守る。実行時の OS の拒否は既存の sink のテストに任せ、別の層へ同じ実行のテストを加えない |
| 本番にテスト専用の口が要るか | 要らない。既存の Fixture、道具、ループ、模擬の提供者と本物の処理系を使う。公開の範囲、フラグ、注入の引数を増やさない |

変更するテストは、実装前に本文と重なるテストを読み直す。旧既定で新しい期待が失敗することは、本作業のうち「既定の Log を自動で実行する」という仕様変更の差として扱う。OS のサンドボックスを要する実行を、Codex 内で直接の起動に差し替えて通したことにしない。

## 確かめること

- 既定がエフェクトなし、`Console.Write`、`Log`、`State`、`Process.Exit` に揃い、利用者の追加がなくても Log の一時スクリプトが確認なしで動くこと。設定に Log を書いた場合も同じであること。
- `benitoite_run`・`benitoite_test` への非継承、File.Write の独立した許可、狭い読み取り範囲、OS の方針、プロジェクトの設定からの変更の禁止、非対話の和をパスの許可に残さない規則を保ったこと。
- 設計書 03-03 に決定の日付と理由があり、プロンプト・設計書の関連箇所・10-interfaces・固定の期待値が一致すること。`git grep` の一致を現在の仕様と過去の記録に分け、P5-40 を変更していないこと。
- `BENITOITEH_TEST_BENITOITE` に指定された v0.0.12 の macOS の処理系を設定して、`scripts/check.sh --inside-codex` が通ること。最後の `Skipped with --inside-codex:` に出た範囲をそのまま報告し、sandbox のテストを内側で通した結果に含めないこと。

## 完了条件

- 新しい既定と設計者の決定が実装、道具の説明、現在の設計書、10-interfaces、固定の期待値に反映され、上記の規則を保っている。
- 上記の方針で既存のテストを直し、同じ契約を重ねた新しいテストや、テスト専用の本番のコードを加えていない。
- `BENITOITEH_TEST_BENITOITE` に v0.0.12 の macOS の処理系を設定して、`scripts/check.sh --inside-codex` が通る。
- 本作業文書の「確かめた結果」に、変更・維持した文書と理由、テストの変更、実施した検査、外へ残した検査が記録されている。

## 取り込みの条件

OS のサンドボックスを要するテストは Codex の中では走らない。オーケストレータは、サンドボックスの外で v0.0.12 の処理系を使い、`scripts/check.sh` の全体を走らせる。macOS は `BENITOITEH_TEST_BENITOITE`、Linux の二対象は `BENITOITEH_TEST_BENITOITE_AARCH64` と `BENITOITEH_TEST_BENITOITE_X86_64` に指定する。macOS の OS のサンドボックス、aarch64 のコンテナ、`BENITOITEH_X86_64_HOST` に指定した x86_64 の機械での実行を含め、Landlock を要するテストは既存の扱いどおり x86_64 の試験環境で確かめる。

外の全体の検査で、Log の一時スクリプトの実行と非継承、受け手の File.Write の独立した許可と OS の制限、狭い読み取り範囲を確かめる。失敗したら結果を返して修正を依頼する。全体が通り、外で確かめた結果を記録してから `gamou` に取り込む。内側の完了だけで取り込みの条件を省かない。

## 確かめた結果

2026-10-27 に、点検済みの本作業文書に従って実装した。確認なしの既定に `Effect::Log` を加え、利用者の設定との和を作る既存の `BTreeSet` の処理を保った。本作業と一覧の状態はオーケストレータが更新するため、未着手の表記を保っている。

### 文書と固定の期待値

| 対象 | 結果と理由 |
|---|---|
| 道具の説明と 01-03 | Log を確認なしの既定に含め、File.Write などの受け手のエフェクトは別の許可を要すると明記した。01-03 には設定との集合の和と、run・test への非継承も示した |
| 03-03「一時的なスクリプト」 | 既定の一覧に Log を加え、P5-40 の【方針】を 2026-10-27 の設計者の【決定】に改めた。理由を標準エラー出力と受け手のエフェクトの区別として記載し、Log の設定を引き続き受け付けることを明記した。State・Process.Exit の経緯、狭い読み取り範囲、非対話の和をパスに残さない【決定】は保った |
| 03-02・04-01 | 一時的なスクリプトの確認なしの既定との関係を示し、OS の範囲と、受け手のエフェクトに別の制限を掛ける説明を保った |
| 08-02・10-interfaces | ADR 0345 との対応の行に Log を加え、scratch_auto_effects の新しい既定と集合の和を明記した。型とシグネチャは変更していない |
| 01-04 | 変更なし。「システムプロンプトの構成」「Benitoite の Skill の置き方」には旧既定の一覧がなく、報告・許可を道具が扱う説明と、固定の期待値を更新する規則は今回も一致するため、本文を保った |
| 固定の期待値 | 指定された v0.0.12 の macOS の処理系を使い、`cargo run --offline -p benitoiteh --example update_context_expected` で更新した。差分と変更された本文を読み、制限モード二種類のシステムプロンプトの scratch の説明だけが変わったことを確認した。通常モード、Skill の本文、道具の定義の JSON、生成の example に差分はない |

上の `git grep` と、旧既定の一覧・`Log is not automatic` を探す検索を変更の前後に実施した。現在の仕様と拒否の文の期待値を更新し、P2-09・P5-40 などの作業文書に残る旧既定は当時の記録として維持した。依存、Skill の写し、過去の結果は変更していない。設定の既定値 `[]`、Log を受け付ける設定の検証、プロジェクトの設定の拒否も既存のままである。

### テストの変更と作成時の関門

作成時の関門の四つの答えに従い、新しいテストを加えず、既存のテストを修正した。守る契約は、既定の Log の実行と後続の道具への非継承、Log と File.Write の独立した許可である。旧テストは「設定なしの Log を拒む」という前提を持つため、その入力と期待を改めた。本番の公開の範囲、フラグ、注入の引数は増やしていない。

`named_allow_set_is_honored_without_asking_the_approver` の scratch の行を、実際のログと書き込みを含む `Log, File.Write` の入力、必要な明示の許可を File.Write とする行に改めた。既定の変更前には、File.Write を明示した場合も拒否が 1 件となり、期待する 0 件との比較で失敗した。これは自動の Log が旧既定にないことによる仕様変更の差である。変更後は `cargo test --offline -p benitoiteh --test agent_loop allow::` の 9 件が通った。空の許可で File.Write を拒み、File.Write だけの明示で自動の Log と合わせて再検査まで進むことを確かめた。fixture に実行の文脈がないという結果は実行の成功として扱っていない。拒否の文に固定した既定の一覧も更新した。

sandbox の既存テストは `automatic_log_does_not_authorize_later_scripts_or_tests` に改名した。設定なしと Log の明示の両方で、最初の一時スクリプトがログを出し、確認を求めず、その後の同じパスの run と Log を使う test が空の許可で拒否される期待に揃えた。この実行は OS のサンドボックスを要するため外へ残した。受け手の File.Write の実行時の許可と OS の拒否は既存の `log_file_sinks_require_their_own_effects_and_respect_os_write_scope` に任せ、同じテストを別の層に追加していない。

最初の全体の内側の検査では、`tools_benitoite_scratch::automatic_effects_and_import_roots_use_the_real_inspection_report` も旧既定を前提にしていることが分かった。Log だけの一時スクリプトに確認の要求があると期待していたが、既定の変更後は確認を求めず実行の文脈の不足まで進むため、`test` の段が終了状態 101 で止まった。対象の道具の既存テストとして、このファイルの旧前提の Log の確認部分を取り除き、後の import の入力が作るパスの番号を合わせた。Log の新しい自動実行は上の sandbox のテストに任せ、一時ファイルと import の既存の契約を保った。テスト関数の追加・削除や、別の層での Log の実行の重複はない。

本番のコードは 9 行追加・4 行削除、テストのコードは 28 行追加・49 行削除である（固定の期待値の置換は別に 4 行追加・4 行削除）。本番のコードの簡素化は行っていない。

### 内側の検査と外へ残す検査

指定された v0.0.12 の macOS の処理系を `BENITOITEH_TEST_BENITOITE` に設定し、`CARGO_NET_OFFLINE=true scripts/check.sh --inside-codex` を再実行した。終了状態 0、`All checks passed.` で通った。`processors`・`fmt`・macOS と Linux 二対象の `clippy`、内側の `test`、`skill-eval`（118 件中 42 件を所定の条件で除外）、`perf-tools`（6 件）、Linux 二対象のテストのビルドとリンク、`deny` のライセンス、三対象の release のビルドを含む。修正した道具のテスト 4 件と、システムプロンプトの固定の照合も通った。処理系の版は `0.0.12`、SHA-256 は `6b26491a8014d9ca88e92caa93c2fee8cc8201098331212071dfdcced8d47e7d` であった。

成功した検査の記録は `work/P5-41-check-inside-codex.log`、初回の失敗の記録は `work/P5-41-check-inside-codex-first-failed.log` に置いた。`git diff --check` と変更した Markdown の相対リンクの確認も通った。依存の版と機能は変更していない。

最後の除外の一覧は次のとおりである。

```text
Skipped with --inside-codex:
  deny: advisories (requires advisory database updates outside Codex)
  crates/benitoiteh/tests/sandbox_benitoite.rs
  crates/benitoiteh/tests/sandbox_benitoite_run.rs
  crates/benitoiteh/tests/sandbox_benitoite_scratch.rs
  crates/benitoiteh/tests/sandbox_benitoite_test.rs
  crates/benitoiteh/tests/sandbox_exec_restricted.rs
  crates/benitoiteh/tests/sandbox_linux.rs
  crates/benitoiteh/tests/sandbox_macos.rs
  crates/benitoiteh/tests/sandbox_read_image.rs
  crates/benitoiteh/tests/sandbox_rewind.rs
  crates/benitoiteh/tests/sandbox_skill_eval_judge.rs
  linux: container tests for aarch64-unknown-linux-musl
  linux: container tests for x86_64-unknown-linux-musl
  skill-eval: requires_sandbox tests (individual names above), test_judge_sandbox.RuntimeTests OS tests, all Benitoite/compaction self-test and dry-run passes (needs sandbox-exec outside Codex)
  skill-eval: benitoiteh-mock, benitoiteh-mock-completion-check and benitoiteh-mock-compact passes (needs loopback and the benitoiteh sandbox outside Codex)
  skill-eval: --self-test --suite normal, --dry-run --suite normal, benitoiteh-mock-normal; NormalTests.test_self_test_and_dry_run_normal_tasks, NormalTests.test_self_test_rejects_broken_controls, NormalTests.test_mock_normal_read_write_bash_for_all_tasks (needs the macOS sandbox outside Codex)
```

Log の一時スクリプトの実際の実行と後の run・test への非継承、受け手の File.Write の実行時の許可と OS の拒否、狭い読み取り範囲の実行は内側の結果に含めない。オーケストレータが指定の三つの v0.0.12 の処理系と x86_64 の試験の機械を使い、サンドボックスの外で `scripts/check.sh` 全体を通してから `gamou` に取り込む。コミットと本物の LLM の呼び出しは行っていない。
