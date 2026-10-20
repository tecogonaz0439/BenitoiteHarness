# P2 の安全の性質とテストの対応

本書は、設計書 07-01「安全の性質のテスト」のうち、制限モードと子プロセスの境界に関わる項目を、テストのファイルと関数に対応させる。P2-11 の実装結果であり、設計上の決定を追加するものではない。主のプロセスの操作は [P4 の対応表](P4-safety.md)を参照する。

## 環境と検査の範囲

ファイルのパスは、特記した `src/` を除き、`crates/benitoiteh/tests/` からの相対パスである。`src/` は `crates/benitoiteh/src/` を指す。表の環境は、そのテストを走らせる対象であり、実行済みかは次の結果欄に記す。

| 記号 | 確かめる環境 | 今回の結果 |
|---|---|---|
| C | Codex の中の macOS | `scripts/check.sh --inside-codex` の実行結果は下記 |
| M | サンドボックスの外の手元の macOS | 通った（下記「M・A・X での実行結果」） |
| A | Apple の `container` の aarch64 Linux。Landlock も bubblewrap もない | 通った（同） |
| X | x86_64 の試験の機械の rootless podman の Linux | 通った（同） |

M は Seatbelt を要するテストを含み、X は Landlock を要するテストを含む。A では `sandbox_linux.rs`、`sandbox_benitoite.rs`、`sandbox_benitoite_run.rs`、`sandbox_benitoite_test.rs`、`sandbox_benitoite_scratch.rs`、`sandbox_exec_restricted.rs` を除く。したがって、本表で M/X とした処理系の実行のテストは A では走らない。Linux 向けの clippy とビルドの成功を、Linux での実行結果とは扱わない。

C の環境は `sw_vers` と `uname -srmp` で確認した。macOS 27.0.1（26A434）、Darwin 27.0.0、arm64 である。CPU の製品名は `sysctl -n machdep.cpu.brand_string` が `Operation not permitted` で失敗したため未確認である。Landlock ABI と bubblewrap の版は対象外である。

| 環境 | 処理系の情報 |
|---|---|
| C | `version=0.0.7`。`build={"commit":null,"dirty":null,"provenance":{"commit":"unknown","dirty":"notCollected"},"target":"aarch64-apple-darwin"}`。SHA-256: `ee8eaf0eb4e624be571b14198e28f1078fb6c6791c42b88ab77a015ea0e8f46e` |
| M | macOS 27.0.1（26A434）、Darwin 27.0.0、arm64、Apple M4。処理系は C と同じ（`version=0.0.7`、`target=aarch64-apple-darwin`、SHA-256 `ee8eaf0eb4e624be571b14198e28f1078fb6c6791c42b88ab77a015ea0e8f46e`）。Landlock と bubblewrap は対象外 |
| A | Apple `container` CLI 1.4.1、`alpine:3.22`（3.22.6）、Linux 6.18.15 aarch64（ホストは上の M）。LSM の一覧は空で Landlock なし。bubblewrap なし。処理系は `version=0.0.7`、静的リンクの aarch64 の ELF（`rust:1.98.1-alpine3.22` で `v0.0.7` のソースから作った）、SHA-256 `56f1cd36a86df65d0781effe8989020b4f91d3b0fdab546c208c7579921676ca`（`check.sh` の一行と手元の `shasum` で一致） |
| X | Omarchy（Arch 系）、Linux 7.2.9-arch1-Watanare-T2-3-t2 x86_64、Intel Core i3-8100B。podman 6.1.1（rootless）、イメージ `localhost/benitoiteh-test:alpine-3.22-bwrap-bash-python3`（Alpine 3.22.5、bubblewrap 0.12.0、Python 3.12.15）。Landlock は `effective_abi: V9, kernel_abi: Some(10)`（`sandbox_linux.rs::kernel_landlock_and_seccomp_are_available` の出力）。処理系は `version=0.0.7`、static-pie の x86_64 の ELF（同じく `v0.0.7` のソースから作った）、SHA-256 `b0c94bdde85350c30d5169f2c7874783c3d18bb72a7568498cc4df3a4fa4d6e9`（送り先のコンテナの `sha256sum` で、手元と一致） |

C の処理系には、指定された `<主の作業ツリー>/work/benitoite-v0.0.7/target/release/benitoite` を使う。`--version=json` の build.commit は null であるため、ソースのタグ `v0.0.7` のコミットを別に記す。手元のリポジトリで `git rev-parse 'v0.0.7^{commit}'` を実行した結果は `63de89a21cce4d0c2995011226db8f2f938598ec` である。実行ファイルの版とハッシュはそれぞれ `--version=json` と `shasum -a 256` で取得した。

## C での実行結果（2026-10-18）

`BENITOITEH_TEST_BENITOITE="<主の作業ツリー>/work/benitoite-v0.0.7/target/release/benitoite" scripts/check.sh --inside-codex`（パスは公開のため一般化した）を実行し、終了状態 0 と `All checks passed.` を確認した。cargo はスクリプトの既定の `CARGO_NET_OFFLINE=true` で動かした。整形、ホストと aarch64／x86_64 Linux の clippy、ホストのテストと doctest、ライセンス、3 対象のリリースビルドを通した。ライブラリ 101 件、agent_loop 60 件、tui 41 件を含む。今回狭めた CLI の起動拒否の判定も成功した。

ホストのテストの前に、次の一行が出た。

```text
processor aarch64-apple-darwin: version=0.0.7 sha256=ee8eaf0eb4e624be571b14198e28f1078fb6c6791c42b88ab77a015ea0e8f46e
```

| リリースの対象 | 実行ファイルの大きさ |
|---|---:|
| aarch64-apple-darwin | 5,933,648 bytes |
| aarch64-unknown-linux-musl | 6,469,056 bytes |
| x86_64-unknown-linux-musl | 8,024,016 bytes |

`--inside-codex` により `sandbox_benitoite.rs`、`sandbox_benitoite_run.rs`、`sandbox_benitoite_scratch.rs`、`sandbox_benitoite_test.rs`、`sandbox_exec_restricted.rs`、`sandbox_linux.rs`、`sandbox_macos.rs`、`sandbox_rewind.rs` と、両 Linux 対象のコンテナでの実行を除いた。新しい 2 件はこの中に含まれ、clippy によるコンパイルは通ったが、実行はオーケストレータが外で確かめる。既定で ignored の手動測定 4 件（`measure_checkpoint_latency_and_storage`、`measure_noop_overhead`、`measure_session_startup_overhead`、`measure_safe_search_traversal`）も実行していない。

前置きの検査は、C の処理系パスの未指定と空、全体の A のパスの未指定、X の接続先があるときの X のパスの空の 4 通りを試した。いずれも fmt より前に該当する変数の `is required: set it to a benitoite v0.0.7 executable.` を示し、終了状態 2 となった。共通の処理系の補助を使う context のテストと、CLI の起動拒否のテストの実行ファイルも、それぞれ未指定と空で起動し、理由の文を伴う失敗（終了状態 101）になることを確かめた。この失敗は、処理系なしに試験を飛ばさないことを確かめるための意図した実行であり、上記の検査の失敗ではない。

`bash -n scripts/check.sh scripts/container-run.sh scripts/remote-linux-run.sh` も成功した。確認の偽造の新規テストで使う 2 本のスクリプトは、本物の処理系の `check --report=json` に通した。これを、サンドボックスを含むテスト全体の実行済みとはしない。

## 07-01 の各項目

同じセルでファイルを省いた関数名は、直前のファイルに属する。M/X の項目は Linux では Landlock を要する。C/M/A/X の項目は OS のサンドボックスを要しない。例外は各行に記す。

| 項目 | 確かめるテスト（ファイルと関数） | 環境・未確認の範囲 |
|---|---|---|
| 制限モードの道具に bash がなく、write・edit が拡張子・範囲外・リンク・ハードリンク・守る場所を拒む（R1・R2） | `context.rs::llm_context_matches_reviewed_files_in_both_modes`、`tools_write.rs::restricted_writes_reject_extensions_escape_and_real_protected_paths`・`final_links_including_dangling_hooks_and_multiple_hardlinks_are_never_written` | C/M/A/X。見直した道具の一覧との比較と、実行器での拒否・変更なしを確かめる |
| 書ける場所のライブラリを DYLD で読み込ませず、スクリプトのローダ変数を InvalidInput にする（R1） | `sandbox_macos.rs::dyld_cannot_map_libraries_from_writable_directories`、`sandbox_benitoite_run.rs::processor_rejects_loader_environment_before_starting_an_allowed_program` | 前者 M。後者 M/X で DYLD_INSERT_LIBRARIES と LD_PRELOAD の両方を試す。Linux の動的リンクのローダは許可しない |
| 確認後のエフェクト増加を E0512 で拒む（R3） | `sandbox_benitoite_run.rs::effects_cannot_grow_between_approval_report_and_execution`・`effect_limit_notice_requires_the_actual_target_path`、`sandbox_benitoite_test.rs::entry_added_after_report_is_not_executed_and_gets_the_effect_change_notice` | M/X。確認後の変更、再報告の後の変更、ディレクトリへ加えたテストを含む。別のファイルを指す偽の E0512 で変更の通知を出さない |
| ネットワークを許さないときの Unix ソケット接続と io_uring の拒否（R4） | `sandbox_benitoite_run.rs::report_effects_control_project_writes_ip_programs_and_environment`。OS の試験と io_uring は P4（[P4-safety.md](P4-safety.md)） | 処理系からの Unix IPC は M/X。接続と宛先付き送信を Http.Connect がある場合にも拒む。IP を許さない場合と io_uring の BPF 判定は P4 の範囲 |
| 確認した Process.Run でも /bin/sh と /usr/bin/python3 を起動できず、許したプログラムは起動できる（R1） | `sandbox_benitoite_run.rs::report_effects_control_project_writes_ip_programs_and_environment` | M/X。両処理系の存在を必須とし、不在を拒否の成功として扱わない。許可した sandbox-probe の成功も対照にする |
| File.Write のないスクリプトはプロジェクトへ書けず、未確認の Http.Connect では IP に接続できない（R4） | `sandbox_benitoite_run.rs::report_effects_control_project_writes_ip_programs_and_environment` | M/X。同じ操作を、エフェクトなし・ありで試す。書き込みでは権限を戻した後も再び拒む |
| 道具の呼び出し・結果で確認の答えを偽れない（R3） | `agent_loop.rs::approval_answers_are_trusted_only_from_approver_and_write_permission_is_not_remembered`、`tools_benitoite_run.rs::inspection_precedes_confirmation_and_rejects_bad_paths_reports_and_changes`、`sandbox_benitoite_run.rs::processor_output_cannot_approve_a_later_effect_increase` | 前二つ C/M/A/X。最後は M/X。実際の stdout の approved・y・JSON の文を提供者へ返した後、同じスクリプトのエフェクト増加を拒む Approver に渡し、実行しない |
| 子プロセスへ提供者のキーを渡さない（R5） | `config.rs::child_process_environment_removes_all_provider_keys_and_replaces_inherited_markers`、`sandbox_benitoite_run.rs::report_effects_control_project_writes_ip_programs_and_environment` | 共通の環境の口は C/M/A/X。処理系とその許可された子の環境から OPENAI_API_KEY が消えることは M/X |
| 読まないファイルを read・glob・grep が返さない | P4（[P4-safety.md](P4-safety.md)） | P4 |
| 通常モード workspace-write の範囲外・.git・ネットワークの拒否 | P4（[P4-safety.md](P4-safety.md)） | P4 |
| 外部 Unix IPC の接続・宛先付き送信の拒否と、ソケットの対の許可 | P4（[P4-safety.md](P4-safety.md)） | P4。処理系からの経路は上記の Unix IPC の行 |
| 未確認の Process.Run と benitoite_test から許可プログラムを起動しない（R1・R3） | `sandbox_benitoite_run.rs::allowed_programs_still_require_process_run_in_the_report`、`sandbox_benitoite_test.rs::process_run_added_after_confirmation_cannot_launch_an_allowed_program` | M/X。前者はエフェクトから作った方針で、同じプログラムの起動の可否を直接試す。後者は確認後のエフェクト増加を再報告で拒み、印のファイルが作られないことを確かめる |
| i386・x32 によるソケット作成の迂回を拒む | P4（[P4-safety.md](P4-safety.md)） | P4 |
| bubblewrap・利用者の名前空間・Landlock ABI 3 を欠くと制限モードが起動しない（R1〜R5） | `src/sandbox/linux/availability.rs::tests::diagnostics_preserve_the_failed_stage`・`bubblewrap_requires_a_supported_release_without_setuid`、`sandbox_linux.rs::setup_failures_do_not_execute_the_program_and_cloexec_descriptors_do_not_leak`、`src/cli/exec/tests.rs::processor_startup_fails_closed_in_restricted_mode_and_disables_tools_in_normal_mode`・`startup_selects_policy_and_fails_closed_unless_explicitly_disabled` | 診断の分類は A/X、補助の失敗は X、起動時の誤りの差し替えは C/M/A/X。この三層の連鎖で確かめる。`src/sandbox/normal.rs::check_availability` が機能ごとの失敗を誤りへ変える部分を通すテストはない。A で bubblewrap がなく制限モードが起動しないことは、オーケストレータが外で一度確かめる |
| escalate でセッション内の同じコマンドの許可を選べない | P4（[P4-safety.md](P4-safety.md)） | P4 |
| 補助の失敗時に実行せず、親の記述子を漏らさない | P4（[P4-safety.md](P4-safety.md)） | P4。上記の起動拒否の行でも補助の失敗を参照する |
| Linux の読ませない場所・守る場所・マウント・後始末 | P4（[P4-safety.md](P4-safety.md)） | P4 |
| 道具の終了時の子孫の後始末と停止の合図 | `sandbox_benitoite_run.rs::escaped_pipe_holders_stop_the_restricted_agent_and_skip_remaining_calls`・`timeout_and_cancellation_interrupt_the_real_processor`、`sandbox_benitoite_test.rs::timeout_interrupts_tests_and_reports_interrupted_summary`、`src/benitoite/tests.rs::query_rejects_a_successful_child_with_possible_descendants`・`queries_bound_output_timeout_and_propagate_cancellation` | 処理系の run/test は M/X、問い合わせは C/M/A/X。macOS のグループを抜けてパイプを持ち続ける子孫は「残る可能性」としてエージェントを止める。パイプを閉じた子孫を検出する保証はない（BH-090）。Linux の名前空間と停止の合図の共通部分は P4 |
| 読まないファイルを取り込む run/test/scratch は対話で毎回確認し、囮でも隠せない（R3・R5） | `tools_benitoite_test.rs::excluded_imports_always_require_confirmation_for_both_scripts_and_tests`・`readable_decoy_never_hides_unread_imports_in_real_reports`、`tools_benitoite_scratch.rs::unread_dependencies_always_require_confirmation_even_with_automatic_effects`、`tui/interaction.rs::real_decoy_imports_are_visible_as_counts_in_interactive_confirmation`、`sandbox_benitoite_test.rs::interactively_confirmed_unread_imports_run_but_are_never_remembered_as_permission` | 最後の実行テスト以外 C/M/A/X、最後 M/X。自動のエフェクトでも外部依存と除外ソースの取り込みを確認する。非対話の拒否は次の行 |
| 読まないファイルのテスト名・値・出力・診断を結果と spill に残さない（R5） | `sandbox_benitoite_test.rs::excluded_test_names_values_output_and_diagnostics_never_reach_results_or_spill` | M/X。秘密の失敗は件数だけにし、表示の上限でも漏らさず、spill が空であることを確かめる |
| 非対話では --allow と確認なしの範囲の和を超える実行を拒み、その和を後の実行許可に残さない（R3） | `sandbox_exec_restricted.rs::exec_denies_effects_outside_the_noninteractive_allow_set`・`exec_allows_assert_check_without_permissions_and_stops_three_effect_denials`、`sandbox_benitoite_test.rs::noninteractive_execution_resets_denials_counts_launches_and_records_rewind_points`、`sandbox_benitoite_scratch.rs::noninteractive_union_confines_automatic_file_reads_and_does_not_authorize_later_runs` | M/X。Assert.Check の自動実行、3 回の連続拒否、成功による拒否回数のリセット、後の run の拒否を含む。読まない取り込みの非対話の拒否は `agent_loop/allow.rs::unread_imports_are_denied_even_with_allowed_effects_and_after_reinspection`・`noninteractive_tests_reject_real_unread_sources_behind_readable_decoys`・`scratch_unread_dependencies_are_denied_without_starting_a_refusal_sequence`（C/M/A/X） |
| 確認なしの一時スクリプトは読む範囲を限り、一時領域の外へ書かず、ネットワークを使わない（R2〜R4） | `sandbox_benitoite_scratch.rs::automatic_file_reads_are_confined_to_project_and_temporary_roots`・`automatic_runs_and_confirmed_retries_execute_the_original_script`・`automatic_runs_reset_denials_and_are_checkpointed_as_irreversible`、`config.rs::user_configuration_reads_the_design_example_and_rejects_invalid_fields` | 設定の制限は C/M/A/X、実行は M/X。範囲外の File.Read は本物の OS の拒否まで試す。書ける場所・IP・起動プログラムは方針の値で確かめており、この経路で実際の書き込み・接続を試した結果ではない |
| 一時スクリプトの名前は既存ファイル・リンクを上書きせず、承認後に同じパスを使う（R2・R3） | `tools_benitoite_scratch.rs::new_names_never_overwrite_or_follow_links_and_approved_calls_reuse_the_path`・`invalid_input_and_exhausted_names_do_not_write_scripts` | C/M/A/X。名前の衝突、消した名前、改変、既存の確認、不正入力、探索の上限を含む。承認後の実際の実行は前行の M/X |
| 確認画面のサンドボックスの範囲と許可プログラムを、掛かる方針より狭く示さない（R3） | `tools_benitoite_run.rs::restricted_facts_cover_current_and_previously_confirmed_effects`、`tools_benitoite_test.rs::test_restricted_facts_cover_current_and_previously_confirmed_effects`、`tui/interaction.rs::effect_confirmation_displays_all_facts_but_not_source_hashes_and_must_fit`・`security_followup_confirmation_paths_escape_newlines_and_tabs_before_joining`・`security_followup_previous_process_run_remains_visible_in_confirmation` | C/M/A/X。現在と以前のエフェクトの和、プロジェクトと一時領域、IP、処理系自身と許可プログラムを示す。改行・タブを含むパスと、情報が収まらないときの拒否も含む |
| 偽の診断の位置で、範囲外・除外ソースの行を添えない（R5） | `tools_check.rs::forged_diagnostics_cannot_read_outside_scope_secrets_pipes_or_record_a_read`・`existing_display_path_cannot_reveal_excluded_import_diagnostics`・`real_diagnostics_hide_secrets_and_deduplicate_imported_modules_before_counting`、`sandbox_benitoite_run.rs::forged_diagnostics_hide_secret_and_outside_paths` | 前三つ C/M/A/X、最後 M/X。秘密・範囲外・FIFO を読み込まず、読んだ版も記録しない。run の標準エラー出力からの診断でも隠す |
| ホーム・設定・データと重なる書ける場所を拒む | P4（[P4-safety.md](P4-safety.md)） | P4 |
| 制御列とファイル名が確認画面を書き換えず、表示前のキーで進まない | P4（[P4-safety.md](P4-safety.md)） | P4。エフェクト確認固有の範囲とパスの表示は上記の行 |
| 巻き戻しが範囲外のリンクを辿らず、記録に中身のないファイルを消さない | P4（[P4-safety.md](P4-safety.md)） | P4 |
| web_fetch の URL・リダイレクト先・解決後のアドレスの検査 | P4（[P4-safety.md](P4-safety.md)） | P4 |

## 処理系の選択と起動

| 項目 | 確かめるテスト（ファイルと関数） | 環境・未確認の範囲 |
|---|---|---|
| #!、書ける場所、Linux の動的リンクを処理系として拒む | `sandbox_exec_restricted.rs::invalid_processor_shapes_stop_before_conversation_and_provider_requests`、`src/benitoite/executable.rs::tests::elf_static_pie_is_accepted_but_interpreters_and_out_of_range_headers_are_rejected`、`src/benitoite/tests.rs::native_executables_must_be_outside_writable_roots_and_allowed_names`・`executable_fifo_is_rejected_without_waiting_for_a_writer` | exec の入口は M/X。単体の検査は C/M/A/X。入口では理由の文、セッションの記録に利用者の依頼と会話が書かれないこと、提供者への要求 0 件まで試す。処理系の問い合わせに使うセッションと一時領域の作成は許す。PT_INTERP は Linux で動的リンクの拒否を試し、macOS では ELF 自体が拒まれる |
| 許可プログラムの名前の一覧を実のパスに適用する（allowed_programs） | `src/benitoite/tests.rs::native_executables_must_be_outside_writable_roots_and_allowed_names`、`src/cli/exec/tests.rs::restricted_startup_rejects_interpreter_names_after_resolving_aliases` | C/M/A/X。名前の禁止は許可プログラムに適用し、別名のリンクでも実の名前を調べる。実行の一覧の適用は上表の Process.Run の行 |
| 設定を優先し、信頼しない PATH とプロジェクト設定を使わず、/new で検査済みの処理系を固定する | `src/benitoite/tests.rs::discovery_prioritizes_config_and_excludes_untrusted_path_entries`、`src/cli/exec/tests.rs::processor_startup_fails_closed_in_restricted_mode_and_disables_tools_in_normal_mode`・`new_pins_the_verified_processor_even_when_configuration_changes`・`changed_processor_is_reported_in_diagnostics_and_notice_without_rewriting_resume_header` | C/M/A/X。変更した処理系は通知するが、再開前のヘッダを書き換えない |

## M・A・X での実行結果（2026-10-18、オーケストレータ）

サンドボックスの外で、`BENITOITEH_TEST_BENITOITE`（M）、`BENITOITEH_TEST_BENITOITE_AARCH64`（A）、`BENITOITEH_TEST_BENITOITE_X86_64`（X）と `BENITOITEH_X86_64_HOST` に x86_64 の試験の機械の ssh の宛先を設定して `scripts/check.sh` を走らせ、終了状態 0 を確かめた。三つの環境のテストの合計は、成功 1,364 件、失敗 0 件、ignored 14 件（手動の測定など）である。A では上記の 6 ファイルを Landlock がないために飛ばし、そのことを検査の最後に示した。処理系の版とハッシュの一行は、三つの対象のすべてで出た（値は上の表と同じ）。P2-11 で加えた 2 件のテストは M と X で通った。

途中の一回目は、`invalid_processor_shapes_...` が「セッションを作らないこと」を確かめていたために M で失敗した。処理系の確かめはセッションの一時ディレクトリを使うサンドボックスの中の問い合わせなので、セッションを先に作るのは設計どおりであり、確かめを「会話の記録を書かないこと」に改めた（作業文書の補足の誤り）。

A で bubblewrap がないときに制限モードが起動しないことを、一度手で確かめた。`alpine:3.22` の中で、リリースの `benitoiteh`（aarch64）を `exec --mode restricted "hello"` で起動すると、`benitoiteh: Sandbox setup failed at availability: bubblewrap: No such file or directory (os error 2); user namespaces: not checked; ...` を示して終了状態 2 で終わった（模擬の提供者の宛先は使われる前に終わる）。

## P2-11 で加えたテストと検査の変更

新しいテストは `sandbox_exec_restricted.rs::invalid_processor_shapes_stop_before_conversation_and_provider_requests` と `sandbox_benitoite_run.rs::processor_output_cannot_approve_a_later_effect_increase` の 2 件である。いずれも既存の部品の単体テストでは届かない、exec の起動全体と本物の処理系の stdout の取り込みを試す。本番のコードへ差し込み口を増やしていない。

既存の `processor_rejects_loader_environment_before_starting_an_allowed_program` に LD_PRELOAD を加え、`report_effects_control_project_writes_ip_programs_and_environment` では /bin/sh と /usr/bin/python3 を必須にした。`processor_startup_fails_closed_in_restricted_mode_and_disables_tools_in_normal_mode` は、サンドボックスの失敗を StartupError::Sandbox だけで判定し、処理系の失敗との混同を避けた。

`scripts/check.sh` は、--inside-codex では BENITOITEH_TEST_BENITOITE、全体では加えて BENITOITEH_TEST_BENITOITE_AARCH64、x86_64 の接続先がある場合は BENITOITEH_TEST_BENITOITE_X86_64 を必須とする。未指定と空を失敗にし、ホストと対象ごとの実行の前に `processor <target>: version=<version> sha256=<sha256>` を出す。版は JSON をホストの python3 で読み、ハッシュはホスト・aarch64 で shasum、x86_64 で送り先のコンテナの sha256sum を使う。

Linux の処理系は aarch64 で読み取り専用のマウント、x86_64 で --sync による転送と読み取り専用のマウントで渡す。処理系自身を runner で起動するときも、同じディレクトリを重ねて書き込み可能でマウントしない。X のイメージには python3 を加え、タグを `localhost/benitoiteh-test:alpine-3.22-bwrap-bash-python3` にした。以前のイメージを消す処理は加えていない。Linux への実際の転送、版とハッシュの表示、テストからの利用は、上の「M・A・X での実行結果」のとおり確かめた。

## 残る確認

機能ごとの診断から `check_availability` の誤りへの変換を一続きに通すテストは足していない。既存の診断・補助・起動の三層の試験で確かめることと、その間に未試験の接続があることを上表に明記した。作業文書の補足の指定によるものである。A での制限モードの起動拒否、M/X の実際のサンドボックスの拒否と新しい 2 件の実行結果は、上の「M・A・X での実行結果」のとおり確かめた。

一時スクリプトの書き込みと IP は、方針の値を確認する範囲にとどまる。Linux の i386・x32・io_uring は P4 の BPF 判定の範囲であり、実際の syscall の実行済みとはしない。ほかの CPU とカーネルでの成功、利用者が許したプログラム自体の安全、macOS の検出できない子孫まで保証する結果ではない（設計書 03-01「保証しないこと」「Linux での保証の範囲」、BH-090）。

## 本物の提供者での課題の完了（2026-10-18、オーケストレータ）

設計者の了承（1 回）を得て、本物の提供者で Benitoite の課題を一つ完了できることを確かめた。生の対話の記録はリポジトリに置かない。

| 項目 | 内容 |
|---|---|
| 提供者とモデル | Anthropic、`claude-opus-5-5`（effort high、`anthropic-messages`）。キーは利用者のキーチェーンからコマンドの参照で得た |
| 課題 | Benitoite `v0.0.7` の `tools/skill-eval/tasks/json_summary`（`git archive v0.0.7` で取り出した）。`orders.json` を読み、種類ごとの件数と合計を示す `main.bnt` を書く |
| 走らせ方 | リポジトリの外の空のディレクトリに `orders.json` だけを置き、`benitoiteh exec --mode restricted --allow File.Read,Console.Write --max-turns 40 --max-run-tokens 600000 --json "<task.md の本文>"`（macOS、処理系 `v0.0.7`、SHA-256 `ee8eaf0e…f46e`） |
| 結果 | 成功。終了状態 0、`The run completed.`。6 番、道具の呼び出し 8 回（`benitoite_reference` 3、`glob` 1、`read` 1、`write` 1、`benitoite_run` 1、`benitoite_scratch` 1）、検査 6 回、修正 0 回、確認 0 回（範囲は `--allow` に収まった）、58 秒 |
| 判定 | 書かれた `main.bnt` は `check` を通り、エフェクトは `Console.Write`・`File.Read` だった。`run` の標準出力は課題の `expected.stdout` と一致した。配列でない入力とないファイルでは、理由を示して終了状態 1 で終わった |
| トークン | 入力 177,955（うちキャッシュの読み取り 142,761、書き込み 35,180）、出力 3,637 |
| 費用の概算 | 約 $0.28〜0.38（2026-10-18 の公式の価格表の単価: 入力 $4、キャッシュの書き込み $5（1 時間なら $8）、読み取り $0.20、出力 $20。いずれも 100 万トークンあたり） |

一回の成功であり、成功率や手数の比較の根拠にはしない（評価は P5 で行う）。

### DeepSeek と Gemini での試し（2026-10-18、オーケストレータ）

P5 の比較の相手を決めるために、設計者の了承（1 回ずつ）を得て、同じ課題と同じ走らせ方（`--max-run-tokens` だけを変えた）で、ほかの二つの提供者を試した。どの試行でも、書かれた `main.bnt` の `run` の標準出力は `expected.stdout` と一致した。

| 提供者とモデル | `--max-run-tokens` | 結果 | 番・道具の呼び出し・時間 | トークン（入力（うちキャッシュの読み取り）・出力） | 費用の概算 |
|---|---|---|---|---|---|
| DeepSeek、`deepseek-flash`（V4.1-Flash-1017、`anthropic-messages`、`https://api.deepseek.com/anthropic`、effort は既定の high） | 600,000 | 正しい `main.bnt` を書いた後、テストを書き足して確かめを続け、トークンの上限で止まった（終了状態 1）。テストが `State` を使い、`--allow` の外として 1 回拒んだ | 20 番・25 回・70 秒 | 596,152（565,888）・11,591 | 約 $0.01〜0.03 |
| 同上 | 5,000,000 | 成功（`The run completed.`） | 15 番・17 回・41 秒 | 381,000（364,000）・6,500 | 約 $0.01 |
| Google、`gemini-3.8-flash`（`openai-chat`、`https://generativelanguage.googleapis.com/v1beta/openai`、effort high） | 600,000 | 最初の要求で失敗した。応答の道具の呼び出しに `index` がなく、`benitoiteh` が読めない応答として扱った。P1-12 で直した | — | — | — |
| 同上（P1-12 の後） | 600,000 | 成功（`The run completed.`） | 16 番・15 回・147 秒 | 450,000（353,000）・4,700 | 約 $0.12 |

単価（100 万トークンあたり、2026-10-18 の公式の価格表）は、DeepSeek がキャッシュに当たった入力 $0.006、当たらない入力 $0.3、出力 $1.2（ピーク時。ピークでない時間は半額）、Gemini 3.8 Flash が入力 $0.75、キャッシュ $0.075、出力 $3.75（2026 年中）である。これらも一回ずつの試しであり、比較の根拠にはしない。
