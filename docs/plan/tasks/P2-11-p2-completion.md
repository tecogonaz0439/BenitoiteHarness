# P2-11 P2 の完了の確かめ（制限モードの安全の性質のテストの一覧と結果）

## 目的

ロードマップの P2 の完了条件のうち、テストで確かめられるもの（[テスト](../../design/07-quality/07-01-testing.md)の「安全の性質のテスト」の制限モードの項目が、macOS と Linux で通ること）を確かめ、どのテストがどの項目を確かめるかの一覧と、走らせた環境ごとの結果を残す。足りない項目のテストを足す。

本物の LLM の提供者で Benitoite の課題を一つ完了する確かめ（P2 の完了条件の残り）は、利用料がかかるので設計者の了承を得てオーケストレータが行う。この作業では行わない。

## 読む設計書

- [ロードマップ](../../design/00-overview/00-03-roadmap.md)の「段階」の P2 の内容と完了条件
- [テスト](../../design/07-quality/07-01-testing.md)の「安全の性質のテスト」（全体と、結果に添える環境の情報）「処理系の用意」「完了の前に通す検査」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の R1〜R5 と「保証しないこと」「Linux での保証の範囲」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「サンドボックスを掛けられないとき」
- 既存のもの: `tests/sandbox_*.rs`、P2-05〜P2-10 と P2-12 で加えたテスト、`scripts/check.sh`、`scripts/remote-linux-run.sh`

## この作業が認める型の変更

なし（テストと結果の文書だけを足す）。本番のコードを変える必要が生じたら止まって報告する。

## 作るもの

### 1. 一覧

- 07-01 の「安全の性質のテスト」の各項目のうち、制限モードと子プロセスの境界に関わるもの（P2 の対象）を一つずつ挙げ、確かめるテスト（ファイルと関数の名前）、走る OS（macOS、aarch64 の Linux、x86_64 の Linux）、Landlock を要するかを表にする。P4 の対象（巻き戻し、`web_fetch`、画面、`create_project`）は P4-08 の一覧に任せ、この表では「P4」と書くだけにする。
- テストのない項目は、テストを足す。足せない項目（環境がない、`unsafe` が要るなど）は、理由を表に書く。少なくとも次を確かめる。
  - 制限モードの道具の一覧に `bash` がないこと。`write`・`edit` が `.bnt` の外とプロジェクトの外を拒むこと（既存のテストの確かめ）。
  - `DYLD_INSERT_LIBRARIES` の項目、`E0512` の項目、`Process.Run` で `/bin/sh` と `python3` を拒む項目、`File.Write`・`Http.Connect` を確認しないスクリプトの項目、確認の答えを偽れない項目、提供者のキーの項目、偽の診断の位置の項目、子孫のプロセスの項目（P2-07・P2-08・P2-12 で加えたもの）。
  - bubblewrap がない、利用者の名前空間を作れない、Landlock の ABI の版 3 を使えない Linux で、制限モードが起動しないこと（差し替えで条件を作る）。
  - 処理系の実行ファイルの形の規則（`#!`、書ける場所の中、動的リンク、名前の一覧）で、制限モードが起動しないこと（P2-05）。

### 2. 結果の文書

- `docs/plan/results/P2-safety.md` に、1 の表と、走らせた環境ごとの結果を書く。結果には、OS とその版、CPU の種類、カーネルの版、Landlock の ABI の版、bubblewrap の版、処理系の版（`--version=json` の `version` と `build`）と実行ファイルの SHA-256 を添える（07-01）。
- Codex の中では、サンドボックスを要するテストと Linux のテストが走らないので、Codex は表と、`--inside-codex` で走ったものの結果だけを書き、残りの欄は「オーケストレータが外で確かめる」と書く。オーケストレータが外で `scripts/check.sh`（すべて）を走らせ、結果の欄を埋めて取り込む。

### 3. 検査の仕組み

- `scripts/check.sh` が、処理系を要するテストを、処理系のパスがないときに失敗させること（飛ばさないこと）を、もう一度確かめる。Linux の段で、静的リンクの処理系が送り先に置かれ、使われていることを、テストの出力（処理系の版とハッシュの一行）で示す。

## 触るファイル

- `crates/benitoiteh/tests/`（足すテスト）
- `docs/plan/results/P2-safety.md`（新）
- `scripts/check.sh`（処理系の版とハッシュを示す一行を加えるとき）

## テスト

1 で足すテストそのもの。足すときは、スキル `test-audit` の作成時の関門に従い、既存のテストと重ねない。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- `docs/plan/results/P2-safety.md` に、表と、Codex の中で走った結果が書いてある。
- 完了の報告に、足したテストの一覧と、テストを足せなかった項目とその理由を書く。

## P2-05〜P2-12 の実装の後の補足（2026-10-18）

P2-05〜P2-10 と P2-12 を取り込んだ後に、この文書を実装に突き合わせた。上の記述と食い違うところは、この節を正とする。

### 結果の文書の形

- `docs/plan/results/P2-safety.md` の形は `docs/plan/results/P4-safety.md` に揃える。環境の記号は P4 と同じく、C（Codex の中の macOS）、M（サンドボックスの外の macOS）、A（Apple の `container` の aarch64 Linux。Landlock も bubblewrap もない）、X（intelmacmini の rootless podman の x86_64 Linux）とする。表の列は「項目｜確かめるテスト（ファイルと関数）｜環境・未確認の範囲」とする。
- P4-safety.md が「P2」と書いて先送りした項目（DYLD の `InvalidInput`、`E0512`、スクリプトによる `Process.Run`・`File.Write`・`Http.Connect`、処理系の道具の確認と提供者のキー、未確認の `Process.Run` と `benitoite_test`、偽の診断、処理系を含む起動の拒否、`allowed_programs` の適用）には、すべて答える。
- 07-01「安全の性質のテスト」に、P2-09・P2-10・P2-12 で加わった性質（読まないファイルの取り込み、非対話の `--allow` と和、一時的なスクリプトの確認なしの経路、確認の画面の範囲）を加えた。これらも表に載せる。
- A では `scripts/check.sh` の列挙のとおり `sandbox_linux.rs`・`sandbox_benitoite*.rs`・`sandbox_exec_restricted.rs` を走らせない。これらを Linux で走らせるのは X だけである（Landlock を要する）。表の環境の欄はこれに合わせる。ロードマップ 00-03 の P2 の完了条件も、この分け方に改めた。

### 対応表の下書き

事前点検で、各項目を確かめる既存のテストを次のとおり見つけた。Codex は、これを関数の名前と中身で確かめ直してから表にする。

- 道具の一覧と `write`・`edit`: `tests/context.rs::llm_context_matches_reviewed_files_in_both_modes`、`tests/tools_write.rs::restricted_writes_reject_extensions_escape_and_real_protected_paths`・`final_links_including_dangling_hooks_and_multiple_hardlinks_are_never_written`。
- DYLD: `tests/sandbox_macos.rs::dyld_cannot_map_libraries_from_writable_directories`、`tests/sandbox_benitoite_run.rs::processor_rejects_loader_environment_before_starting_an_allowed_program`。
- `E0512`: `sandbox_benitoite_run.rs::effects_cannot_grow_between_approval_report_and_execution`・`effect_limit_notice_requires_the_actual_target_path`、`sandbox_benitoite_test.rs::entry_added_after_report_is_not_executed_and_gets_the_effect_change_notice`。
- Unix IPC、`Process.Run` の `/bin/sh`・`python3`、`File.Write`・`Http.Connect`、提供者のキー（run の環境）: `sandbox_benitoite_run.rs::report_effects_control_project_writes_ip_programs_and_environment`。提供者のキー（共通）: `tests/config.rs::child_process_environment_removes_all_provider_keys_and_replaces_inherited_markers`。
- 確認の答え: `tests/agent_loop.rs::approval_answers_are_trusted_only_from_approver_and_write_permission_is_not_remembered`、`tests/tools_benitoite_run.rs::inspection_precedes_confirmation_and_rejects_bad_paths_reports_and_changes`。
- 未確認の `Process.Run`: `sandbox_benitoite_run.rs::allowed_programs_still_require_process_run_in_the_report`、`sandbox_benitoite_test.rs::process_run_added_after_confirmation_cannot_launch_an_allowed_program`。
- bubblewrap・名前空間・Landlock ABI 3: `src/sandbox/linux/availability.rs` の `diagnostics_preserve_the_failed_stage`・`bubblewrap_requires_a_supported_release_without_setuid`、`sandbox_linux.rs::setup_failures_do_not_execute_the_program_and_cloexec_descriptors_do_not_leak`、`src/cli/exec/tests.rs::processor_startup_fails_closed_in_restricted_mode_and_disables_tools_in_normal_mode`・`startup_selects_policy_and_fails_closed_unless_explicitly_disabled`。表には、この三層の連鎖で確かめていることと、`src/sandbox/normal.rs` の `check_availability` が各項目の失敗を誤りに変える部分を通すテストがないことを書く。A で bubblewrap がないために制限モードが起動しないことは、オーケストレータが外で一度確かめて結果の欄に書く。
- 子孫のプロセス: `sandbox_benitoite_run.rs::escaped_pipe_holders_stop_the_restricted_agent_and_skip_remaining_calls`・`timeout_and_cancellation_interrupt_the_real_processor`、`sandbox_benitoite_test.rs::timeout_interrupts_tests_and_reports_interrupted_summary`、`src/benitoite/tests.rs::query_rejects_a_successful_child_with_possible_descendants`・`queries_bound_output_timeout_and_propagate_cancellation`。
- 偽の診断: `tests/tools_check.rs::forged_diagnostics_cannot_read_outside_scope_secrets_pipes_or_record_a_read`・`existing_display_path_cannot_reveal_excluded_import_diagnostics`・`real_diagnostics_hide_secrets_and_deduplicate_imported_modules_before_counting`、`sandbox_benitoite_run.rs::forged_diagnostics_hide_secret_and_outside_paths`。
- 処理系の実行ファイルの形: `src/benitoite/executable.rs` の `elf_static_pie_is_accepted_but_interpreters_and_out_of_range_headers_are_rejected`、`src/benitoite/tests.rs::native_executables_must_be_outside_writable_roots_and_allowed_names`・`discovery_prioritizes_config_and_excludes_untrusted_path_entries`・`executable_fifo_is_rejected_without_waiting_for_a_writer`、`src/cli/exec/tests.rs::restricted_startup_rejects_interpreter_names_after_resolving_aliases`。
- 読まないファイルの取り込み: `tests/tools_benitoite_test.rs::excluded_imports_always_require_confirmation_for_both_scripts_and_tests`・`readable_decoy_never_hides_unread_imports_in_real_reports`、`tests/tools_benitoite_scratch.rs::unread_dependencies_always_require_confirmation_even_with_automatic_effects`、`tests/tui/interaction.rs::real_decoy_imports_are_visible_as_counts_in_interactive_confirmation`、`sandbox_benitoite_test.rs::interactively_confirmed_unread_imports_run_but_are_never_remembered_as_permission`・`excluded_test_names_values_output_and_diagnostics_never_reach_results_or_spill`。
- 非対話の `--allow`: `tests/sandbox_exec_restricted.rs::exec_denies_effects_outside_the_noninteractive_allow_set`・`exec_allows_assert_check_without_permissions_and_stops_three_effect_denials`、`sandbox_benitoite_test.rs::noninteractive_execution_resets_denials_counts_launches_and_records_rewind_points`、`tests/sandbox_benitoite_scratch.rs::noninteractive_union_confines_automatic_file_reads_and_does_not_authorize_later_runs`。
- 一時的なスクリプトの確認なしの経路: `sandbox_benitoite_scratch.rs::automatic_file_reads_are_confined_to_project_and_temporary_roots`・`automatic_runs_and_confirmed_retries_execute_the_original_script`・`automatic_runs_reset_denials_and_are_checkpointed_as_irreversible`、`tools_benitoite_scratch.rs::new_names_never_overwrite_or_follow_links_and_approved_calls_reuse_the_path`・`invalid_input_and_exhausted_names_do_not_write_scripts`、`tests/config.rs::user_configuration_reads_the_design_example_and_rejects_invalid_fields`。書き込みとネットワークは、方針の値で確かめていることを表に書く。
- 確認の画面の範囲: `tools_benitoite_run.rs::restricted_facts_cover_current_and_previously_confirmed_effects`、`tools_benitoite_test.rs::test_restricted_facts_cover_current_and_previously_confirmed_effects`、`tui/interaction.rs::effect_confirmation_displays_all_facts_but_not_source_hashes_and_must_fit`・`security_followup_confirmation_paths_escape_newlines_and_tabs_before_joining`・`security_followup_previous_process_run_remains_visible_in_confirmation`。
- 処理系の選び方（P2-05）: `src/cli/exec/tests.rs::new_pins_the_verified_processor_even_when_configuration_changes`・`changed_processor_is_reported_in_diagnostics_and_notice_without_rewriting_resume_header`、`src/benitoite/tests.rs::discovery_prioritizes_config_and_excludes_untrusted_path_entries`。
- P4 の対象（読まないファイルの `read`・`glob`・`grep`、通常モード、外部の Unix IPC、i386・x32、`escalate`、補助の失敗と記述子、Linux のファイルシステムの見え方、書ける場所の検査、巻き戻し、`web_fetch`）は「P4（P4-safety.md）」と書く。

### 足すテストと直し（この節で触るファイルを広げる）

1. **処理系の形の誤りで起動しないこと**: `tests/sandbox_exec_restricted.rs` に足す。`benitoite.path` に、`#!/bin/sh` のスクリプト、プロジェクトの中に写した本物の処理系、`PT_INTERP` を持つ小さな ELF（`executable.rs` のテストと同じ作り方）を順に設定し、`benitoiteh exec --mode restricted` が `The program must be a native executable outside writable locations (statically linked on Linux).` で終わること、セッションを作らないこと、模擬の提供者への要求が 0 件であることを確かめる。
2. **処理系の出力が確認を進めないこと**: `tests/sandbox_benitoite_run.rs` に足す。`benitoite_run` の標準出力に確認の答えの形の文（`approved`、`y` など）を書かせ、続けてエフェクトを増やした呼び出しが、拒む Approver に届き、実行されないことを確かめる。
3. **`LD_PRELOAD` も拒むこと**: `processor_rejects_loader_environment_before_starting_an_allowed_program` に足す（`v0.0.7` の `--version=json` の `loaderEnvironment.deniedPrefixes` は `"DYLD_"` と `"LD_"` を持つ）。
4. **X の `python3`**: `scripts/remote-linux-run.sh` のイメージに `python3` を加え、タグを `localhost/benitoiteh-test:alpine-3.22-bwrap-bash-python3` に改める（前のタグのイメージは消さない。送り先の機械のほかのイメージとコンテナに触らない）。`report_effects_control_project_writes_ip_programs_and_environment` の `if !program.exists() { continue; }` をやめ、`/bin/sh` と `python3` の両方を必須にする（macOS は `/usr/bin/python3` を、Linux は `/usr/bin/python3` を使う。macOS で `python3` がないときは理由を示して失敗させる）。
5. **起動の拒否のテストの判定を狭める**: `src/cli/exec/tests.rs` の `processor_startup_fails_closed_in_restricted_mode_and_disables_tools_in_normal_mode` で、サンドボックスの失敗の場合に `StartupError::Benitoite` も受け入れている判定を、`StartupError::Sandbox` だけを受けるように狭める（テストのコードだけを変える）。
6. **`scripts/check.sh` の前置きの検査**: 要る変数が空なら、理由（例: `BENITOITEH_TEST_BENITOITE is required: set it to a benitoite v0.0.7 executable.`）を示して止める。`--inside-codex` では `BENITOITEH_TEST_BENITOITE` だけ、すべてを走らせるときは加えて `BENITOITEH_TEST_BENITOITE_AARCH64`、`BENITOITEH_X86_64_HOST` があるときは `BENITOITEH_TEST_BENITOITE_X86_64` も。`tests/support/processor.rs` と `src/cli/exec/tests.rs` の処理系を得る部分は、空の文字列を「ない」と扱い、理由の文で失敗させる（テストのコードだけを変える）。
7. **処理系の版とハッシュの一行**: `scripts/check.sh` の test の段の前（ホスト）と、linux の段の各対象の前に、`processor <target>: version=<version> sha256=<sha256>` の形の一行を出す。aarch64 は `scripts/container-run.sh "$BENITOITEH_TEST_BENITOITE_AARCH64" --version=json` と手元の `shasum -a 256`、x86_64 は `--sync` の後に `scripts/remote-linux-run.sh "$BENITOITEH_TEST_BENITOITE_X86_64" --version=json` と送り先の `sha256sum` で得る。Codex の中では Linux の段は動かないので、オーケストレータが外で確かめる。

### 結果の文書の環境の欄

- Codex は、C の結果と、M・A・X の欄を「オーケストレータが外で確かめる」として書く。オーケストレータが外で `scripts/check.sh` を走らせた後に、M・A・X の結果と環境の情報（OS と版、CPU、カーネル、Landlock の ABI、bubblewrap の版、処理系の `version`・`build` と SHA-256）を書き足して取り込む。
- 処理系の `build.commit` は null なので、ソースのタグ `v0.0.7` のコミットを別に書く。
- 本物の LLM の提供者で課題を一つ完了する確かめは、この作業では行わない（設計者の了承の後にオーケストレータが行う）。結果の文書には「設計者の了承待ち」と書く。
