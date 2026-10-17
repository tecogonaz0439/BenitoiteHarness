# P4 の安全の性質とテストの対応

本書は、設計書 07-01「安全の性質のテスト」の各項目と、P4 で加えた主のプロセスの操作を、テストのファイルと関数に対応させる。P4-08 の実装結果であり、設計上の決定を追加するものではない。

## 環境と検査の範囲

表の環境の記号は次の意味である。ファイルのパスは、特記した `src/` を除き、`crates/benitoiteh/tests/` からの相対パスである。`src/` は `crates/benitoiteh/src/` を指す。

| 記号 | 確かめる環境 | この作業での扱い |
|---|---|---|
| C | Codex の中の macOS | `scripts/check.sh --inside-codex` で、OS のサンドボックスを要しないテストを実行する。擬似端末のテストも含む |
| M | サンドボックスの外の手元の macOS | オーケストレータが `scripts/check.sh` で実行する。Seatbelt を重ねて掛けるテストを含む |
| A | aarch64 Linux のコンテナ | オーケストレータが実行する。既定のカーネルに Landlock がないため、`sandbox_linux.rs` は除く（00-common.md「検査」） |
| X | x86_64 Linux の機械の podman | オーケストレータが実行する。Landlock を要するテストを含む |
| P2 | 本物の Benitoite v0.0.7 と連携する環境 | 処理系の連携の作業後に `docs/plan/results/P2-safety.md` を参照する。この作業の時点では同ファイルはまだない |

C の環境は `sw_vers` と `uname -srmp` で確認した。macOS 27.0.1（26A434）、Darwin 27.0.0、arm64 である。CPU の製品名は `sysctl -n machdep.cpu.brand_string` が拒否されたため未確認。Landlock と bubblewrap は対象外、Benitoite の版とハッシュは「なし」（連携前）である。M・A・X の実行後は、オーケストレータが OS と版、CPU、カーネル、Landlock ABI、bubblewrap の版、処理系の版とハッシュを添える。過去の作業での実行結果を、今回の実行済みの根拠にはしない。

2026-10-18 に C で `CARGO_NET_OFFLINE=true scripts/check.sh --inside-codex` を実行し、終了状態0と `All checks passed.` を確認した。整形、3対象向けの clippy、ホストのテスト、ライセンス、3対象のリリースビルドを通した。ライブラリの59件、対話画面の36件、巻き戻しの21件も成功した。手動の性能測定4件は既定の `ignored` のまま実行していない。

| リリースの対象 | 実行ファイルの大きさ |
|---|---:|
| aarch64-apple-darwin | 5,553,248 bytes |
| aarch64-unknown-linux-musl | 6,075,096 bytes |
| x86_64-unknown-linux-musl | 7,511,920 bytes |

`--inside-codex` によって除いたものは、`sandbox_linux.rs`、`sandbox_macos.rs`、`sandbox_rewind.rs` と、aarch64／x86_64 Linux のコンテナでのテスト実行である。Linux 向けのビルドの成功を、これらのテストの実行結果とは扱わない。

## 07-01 の各項目

| 項目 | 確かめるテスト（ファイルと関数） | 環境・未確認の範囲 |
|---|---|---|
| 制限モードの道具、拡張子、範囲外、リンク、ハードリンク、守る場所への書き込み | `tools_write.rs::restricted_writes_reject_extensions_escape_and_real_protected_paths`、`final_links_including_dangling_hooks_and_multiple_hardlinks_are_never_written`。道具の一覧は `context.rs` の `llm_context_matches_reviewed_files_in_both_modes` | C/M/A/X。処理系の道具は P2 |
| 書ける場所のライブラリを DYLD に読み込ませない | `sandbox_macos.rs::dyld_cannot_map_libraries_from_writable_directories` | M。`Process.Command.environment` の `InvalidInput` は P2 |
| 確認後のエフェクト増加を E0512 で拒む | 処理系の連携後の `P2-safety.md` を参照する | P2。`--allow-effects` の受け渡しをまだ実装していない |
| ネットワークを許さないときの Unix 接続と io_uring の拒否 | `sandbox_macos.rs::external_unix_ipc_is_denied_with_and_without_ip_permission`、`sandbox_linux.rs::external_unix_ipc_is_denied_but_stream_socketpairs_work`。`src/sandbox/linux/filter/tests.rs::compiled_filter_rejects_bypass_syscalls_and_foreign_architectures` | Unix IPC は M/X。io_uring は A/X で BPF の判定を実行する。生の io_uring syscall を発行するテストではない |
| 制限モードの Process.Run の制限と許可プログラムの起動 | `sandbox_macos.rs::execution_allowlist_applies_to_descendants_and_rejects_writable_executables`、`sandbox_linux.rs::executable_allowlist_is_inherited_and_writable_programs_are_denied` | OS の境界は M/X。Benitoite のスクリプトによる確認を含む一連の処理は P2 |
| File.Write のないスクリプトの書き込みと、未確認の Http.Connect の拒否 | OS の境界は `sandbox_macos.rs::writing_is_confined_and_protected_names_cannot_be_created_or_replaced`、`ip_connections_follow_policy`、`sandbox_linux.rs::writing_creates_removes_and_atomically_replaces_files_only_in_writable_roots`、`ip_connections_follow_policy` | M/X。エフェクトの集合から方針を作る連携は P2 |
| 道具の呼び出し・結果による確認の偽造を拒む | `agent_loop.rs::approval_answers_are_trusted_only_from_approver_and_write_permission_is_not_remembered`、`web_fetch.rs::web_fetch_is_serial_in_both_modes_and_untrusted_json_cannot_approve_it`、`tui/interaction.rs::approval_gate_and_key_table_have_no_implicit_or_modified_answer` | C/M/A/X。処理系の確認は P2 |
| 子プロセスへ提供者のキーを渡さない | `config.rs::child_process_environment_removes_all_provider_keys_and_replaces_inherited_markers`、`tools_bash.rs::bash_clears_keys_and_removes_sandbox_markers_when_escalated` | C/M/A/X。処理系の道具も同じ環境の口を使うことを P2 で確認する |
| 読まないファイルを read・glob・grep が返さない | `tools_read.rs::exclusions_apply_to_real_paths_and_both_modes_and_settings_only_add_rules`、`searches_honor_local_gitignore_include_hidden_files_and_do_not_follow_links` | C/M/A/X。P4-09 の安全な読み方を含めて通し直す |
| 通常モード workspace-write の範囲外・.git・ネットワークの拒否 | `sandbox_macos.rs::writing_is_confined_and_protected_names_cannot_be_created_or_replaced`、`ip_connections_follow_policy`、`sandbox_linux.rs::writing_creates_removes_and_atomically_replaces_files_only_in_writable_roots`、`root_protection_blocks_creation_writing_removal_and_replacement`、`ip_connections_follow_policy` | M/X |
| 外部 Unix IPC の接続・宛先付き送信を拒み、ソケットの対を許す | `sandbox_macos.rs::external_unix_ipc_is_denied_with_and_without_ip_permission`、`sandbox_linux.rs::external_unix_ipc_is_denied_but_stream_socketpairs_work` | M/X。各テストが IP の可否を両方試す |
| 未確認の Process.Run と benitoite_test から起動させない | 処理系の連携後の `P2-safety.md` を参照する | P2。OS の起動の一覧だけでは、確認したエフェクトとの対応を証明しない |
| i386・x32 によるソケット作成の迂回を拒む | `src/sandbox/linux/filter/tests.rs::compiled_filter_rejects_bypass_syscalls_and_foreign_architectures`、`compiled_socket_rules_limit_domains_types_and_ip_operations` | X。BPF に CPU・番号・引数を渡す試験であり、互換 ABI の syscall の実行済みとはしない |
| bubblewrap・利用者の名前空間・Landlock ABI 3 を欠く環境で起動しない | `src/sandbox/linux/availability.rs::tests::diagnostics_preserve_the_failed_stage`、`sandbox_linux.rs::setup_failures_do_not_execute_the_program_and_cloexec_descriptors_do_not_leak`、`doctor_verifies_restricted_and_normal_sandboxes` | X。`sandbox_linux.rs` は aarch64 では実行しない。availability の分類の単体テストだけは A/X。処理系を含む制限モードの起動は P2 |
| escalate でセッション内の同じコマンドの許可を選べない | `agent_loop.rs::bash_approval_uses_actual_shell_policy_and_never_remembers_escalation`、`tui/interaction.rs::approval_gate_and_key_table_have_no_implicit_or_modified_answer` | C/M/A/X。後者は Escalate の画面に `a` を送り、返事が出ないことを確認する |
| 補助の失敗時に実行せず、親の記述子を漏らさない | `sandbox_linux.rs::setup_failures_do_not_execute_the_program_and_cloexec_descriptors_do_not_leak`、`target_cannot_inherit_or_open_control_descriptors_and_stdin_modes_work`、`malformed_control_input_fails_before_target_execution`。macOS の限界は `sandbox_macos.rs::inherited_descriptors_require_close_on_exec_on_macos` | X/M。`__session` の失敗は下の P4 の表も参照する |
| Linux の読ませない場所・守る場所・マウント・後始末 | `sandbox_linux.rs::reading_excludes_secrets_and_confines_readable_roots`、`root_protection_blocks_creation_writing_removal_and_replacement`、`gitdir_targets_and_existing_nested_metadata_are_read_only`、`protected_and_denied_mounts_cannot_be_removed_or_overmounted`、`writing_creates_removes_and_atomically_replaces_files_only_in_writable_roots`、`cleanup_waits_for_exit_and_preserves_preexisting_empty_directories` | X。未作成の入れ子と大文字の限界は `uncreated_nested_and_deep_metadata_and_uncreated_uppercase_root_are_outside_guarantee` |
| 道具の終了時の子孫の後始末と停止の合図 | `sandbox_linux.rs::namespace_exit_leaves_no_descendants_even_after_setsid`、`namespace_stop_signals_and_exit_statuses_reach_the_caller`、`sandbox_macos.rs::sandboxed_groups_are_cleaned_and_escaped_pipe_holders_are_reported`、`sandboxed_timeout_delivers_the_requested_signal`、`run.rs::timeout_forwards_each_stop_signal_and_escalates_when_ignored` | M/X、サンドボックスなしの run は C/M/A/X。グループを抜けてパイプを閉じた macOS の子孫の限界は BH-090 の扱い |
| スクリプトの偽の診断に範囲外のファイルの行を添えない | 処理系の連携後の `P2-safety.md` を参照する | P2。診断の受け渡しをまだ実装していない |
| ホーム・設定・データと重なる書ける場所を拒む | `config.rs::writable_locations_reject_each_rule_and_resolve_aliases_before_checking`、`writable_locations_reject_the_real_executable_in_any_root` | C/M/A/X。/new の再検査も下の表で確認する |
| 制御列とファイル名が確認画面を書き換えず、表示前のキーで進まない | `tui.rs::every_untrusted_surface_emits_only_harness_controls`、`tui/interaction.rs::confirmation_facts_cannot_forge_answer_rows_and_messages_are_not_history`、`approval_gate_and_key_table_have_no_implicit_or_modified_answer` | C/M/A/X |
| 巻き戻しが範囲外のリンクを辿らず、記録に中身のないファイルを消さない | `rewind.rs::untrusted_manifests_cannot_write_outside_or_protected_paths`、`corrupt_objects_hardlinks_and_replaced_parents_never_modify_outside`、`complete_checkpoints_never_delete_excluded_paths`、`object_links_are_rejected_even_when_the_target_has_the_right_hash` | C/M/A/X。OS の境界は `sandbox_rewind.rs::sandboxed_child_cannot_change_checkpoint_objects_or_session`（M/X） |
| web_fetch の URL、リダイレクト先、解決後のアドレスの検査 | `src/tools/web_fetch/tests.rs::urls_reject_ambiguous_authorities_and_canonicalize_before_approval`、`resolution_waits_for_approval_and_rejects_any_forbidden_answer`、`redirects_resolve_relative_references_recheck_domains_and_enforce_five_hops`、`redirect_to_forbidden_dns_is_not_connected_and_target_needs_no_premature_lookup` | C/M/A/X。手元の TLS サーバーと名前解決の台本を使い、禁止アドレスの表も `address_table_rejects_special_ranges_and_checks_embedded_ipv4` で確認する |

## P4 で加えた操作

| 項目 | 確かめるテスト（ファイルと関数） | 環境・備考 |
|---|---|---|
| 圧縮・超過時の圧縮・取消し・会話と使用量 | `agent_loop/compaction.rs::threshold_compacts_once_preserves_prefix_and_accounts_only_real_turns`、`overflow_uses_largest_input_shortens_utf8_drops_old_items_and_retries_same_turn_once`、`separate_requests_omit_tools_and_neutralize_untrusted_content_and_file_names`、`cancellation_during_summary_preserves_the_conversation_for_manual_and_run` | C/M/A/X。/compact の接続は次の行 |
| /compact の指示、ESC と Ctrl+C、結果ごとの Summary | `tui/commands.rs::manual_compaction_records_each_attempt_and_status_adds_run_and_compaction_usage`、`manual_compaction_cancels_with_escape_or_interrupt_and_failed_attempts_are_recorded`、`cancellation_restores_literal_slashes_for_compaction_followups_and_run_inputs` | C/M/A/X。取消しは模擬提供者が HTTP 要求を受信してから送る。未送信の後続・操縦は `//new` の原文で入力欄へ戻す |
| 再開、壊れた末尾、モード変更の確認 | `exec/resume.rs::resume_appends_to_same_record_preserves_history_and_mode_and_repairs_tail`、`resume_errors_preserve_records_and_never_escalate_or_follow_links`、`tui/pty.rs::mode_confirmation_signals_exit_130_and_decline_is_printed_after_terminal_restore` | C/M/A/X。後者は本物の擬似端末で TERM/HUP/INT と拒否を確認する |
| 巻き戻し、世代、読んだ版、戻らない操作の予告 | `rewind.rs::agent_rewind_replay_resume_and_read_versions_agree`、`hardlink_events_only_report_new_paths_and_compaction_invalidates_conversation_rewind`、`web_fetch_calls_are_reported_as_irreversible_after_the_selected_checkpoint` | C/M/A/X。予告と結果の一覧の一致を bash と web_fetch で確認する |
| /rewind の選択・会話不可の範囲・確認と復元 | `src/ui/tui/tests.rs::checkpoint_selection_scrolls_esc_cancels_and_unavailable_conversation_has_only_files`、`rewind_confirmations_reject_queued_and_early_keys_paste_and_hidden_facts`、`tui/commands.rs::rewind_restores_files_and_conversation_after_explicit_selection_and_confirmation`、`incomplete_before_rewind_requires_f_and_reports_the_error_reasons`、`src/ui/tui/tests.rs::rewind_confirmation_keeps_irreversible_count_when_details_do_not_fit`、`rewind_confirmation_facts_cannot_forge_answer_rows_and_selection_flattens_newlines` | C/M/A/X。戻せない操作の件数を事実に含めて y/f の収まりを調べる。一覧は内容の幅で折り返し、全行に接頭辞を付ける。選択の行の改行は単一行へ変換する |
| 巻き戻し中のキーの終了操作は完了を待ち、外からの二度目の合図で中断する | `tui/commands.rs::accepted_rewind_finishes_before_quit_and_repeated_keyboard_interrupts`、`tui/pty.rs::second_signal_prints_checkpoint_after_terminal_restore_only_during_rewind` | C/M/A/X。擬似端末で実際の復元中に TERM/HUP を送り、終了状態130、termios の復元、端末を戻した後の時点 ID の表示、復元が未完了であることを確認する。巻き戻し完了後の同じ合図では ID を表示しない |
| glob・grep・read のリンクを辿らない読み方と上限（P4-09） | `tools_read.rs::searches_skip_hostile_gitignores_without_waiting`、`hostile_file_inputs_are_rejected_or_skipped_without_waiting`、`traversal_limits_return_partial_results_and_cancellation`、`searches_honor_local_gitignore_include_hidden_files_and_do_not_follow_links`、`exclusions_apply_to_real_paths_and_both_modes_and_settings_only_add_rules` | C/M/A/X。FIFO・装置・リンク・64 MiB・走査数・時間・取消しを含む |
| 確認画面の説明は事実より前、10行まで。事実の不足時は y/a/s を拒む。長い空白は ␠×N | `tui/interaction.rs::long_justification_keeps_facts_next_to_keys_and_rewraps_on_resize`、`oversized_facts_disable_approval_keep_decline_and_never_overflow_the_screen` | C/M/A/X |
| 不可視文字・幅の定まらない文字・制御列の変換 | `tui/interaction.rs::invisible_and_ambiguous_characters_cannot_hide_command_suffixes`、`tui.rs::every_untrusted_surface_emits_only_harness_controls`、`src/ui/tui/tests.rs::rewind_confirmations_reject_queued_and_early_keys_paste_and_hidden_facts` | C/M/A/X。U+3164、U+FFA0、異体字セレクタ、方向の制御、曖昧幅と未割り当てを含む |
| 入力の守りと端末の大きさの変更 | `tui/interaction.rs::mode_confirmation_flushes_then_discards_input_and_obeys_read_timestamps_without_sleeping`、`resizing_keeps_confirmation_bounded_and_rearms_the_guard_before_answers`、`discarding_pending_input_keeps_resize_and_interrupt_only`、`src/ui/tui/tests.rs::rewind_confirmations_reject_queued_and_early_keys_paste_and_hidden_facts` | C/M/A/X。表示前、300 ms 未満、貼り付け、Resize、Ctrl+C を区別する |
| 入力履歴の権限・プロジェクト分離・上限・リンク拒否 | `tui/interaction.rs::project_history_is_private_bounded_shared_and_never_follows_links`、`history_rotation_keeps_encoded_file_within_the_read_limit`、`confirmation_facts_cannot_forge_answer_rows_and_messages_are_not_history`、`tui/commands.rs::command_grammar_busy_rules_and_literal_slashes_apply_only_at_submission` | C/M/A/X。// は入力した原文を残し、確認のキーは残さない |
| 子プロセスが制御端末を持たず、__session が失敗時に実行しない | `cli.rs::session_helper_executes_only_after_setsid_and_failures_exit_125`、`tui/pty.rs::unsandboxed_session_launch_has_no_controlling_terminal`、`sandbox_macos.rs::sandboxes_detach_the_controlling_terminal_for_both_modes`、`sandbox_linux.rs::normal_sandbox_has_no_controlling_terminal` | 前二つ C/M/A/X。Seatbelt は M、Landlock は X。対照で端末を開けることも確認する |
| /new が許可と読んだ版を引き継がず、安全設定を固定する | `tui/commands.rs::new_does_not_carry_allowed_commands_domains_or_read_versions`、`new_session_prepares_first_ends_and_drops_old_agent_and_preserves_session_on_failure`、`src/cli/exec/tests.rs::new_after_restricted_resume_preserves_mode_and_project_and_normal_preserves_policies`、`new_preserves_explicitly_disabled_sandbox_without_reprobing` | C/M/A/X。モード・プロジェクト・sandbox・approval・network・解決済み writable_roots・allowed_programs を固定し、モデル変更は反映する。ドメインの許可を記憶させる準備では、返事を渡す前に取り消し、外の名前解決へ進ませない |
| /new の固定したパスをセッション作成前に再検査する | `src/cli/exec/tests.rs::new_after_restricted_resume_preserves_mode_and_project_and_normal_preserves_policies`、`new_rejects_changed_allowed_program_path_before_creating_a_session` | C/M/A/X。writable_roots を別の許されるディレクトリへのリンクへ替え、allowed_programs を別の実行ファイルへのリンクへ替える。いずれも `SESSION_PATH_CHANGED` を確認し、許可プログラムの検査失敗で新しいセッションが作られないことも確認する |
| LLM の / コマンドを実行しない | `tui/commands.rs::llm_slash_text_never_executes_commands_and_empty_rewind_is_reported` | C/M/A/X。テストが確かめるのは LLM の応答の文である。AgentInput と再開の会話をコマンドにしないことは、`command_action` を `Screen::input_at` だけが呼ぶコードの構造による保証である |
| /new の End 待ちの間の終了では新しい働き手を起動しない | `tui/commands.rs::new_session_is_ended_without_activation_when_quit_or_signals_arrive_before_old_end` | C/M/A/X。/quit、一度目・二度目の終える合図で起動用の処理を呼ばず、準備したセッションに SessionEnd を書き、一時ディレクトリを片付ける |
| /quit の終了状態と端末の復元 | `tui/pty.rs::slash_quit_exits_zero_and_restores_terminal_while_idle_and_processing` | C/M/A/X。本物の擬似端末で /quit を送り、終了状態0と termios の復元を確認する |

## P4 の完了条件として残るもの

`create_project` と、処理系 v0.0.7 の連携を要する項目は残る。処理系の項目は P2 の作業後の `docs/plan/results/P2-safety.md` を参照する。`restricted.allowed_programs` は P4-08 では解決済みの一覧を保持して /new に渡し、設定の読み直しで変更しない。処理系の実行にその一覧を適用することは P2 で確認する。

M・A・X の実行と環境情報の追記は、取り込み時にオーケストレータが行う。C の検査だけをもって「安全の性質をすべて通し直した」とはしない。互換 ABI と io_uring は上表の BPF の試験の範囲を記した。処理系からの実行を含む試験の範囲と区別して引き継ぐ。

## 本物の端末での確認手順

本物の LLM を呼ぶ場合は、00-common.md と AGENTS.md に従い、提供者・モデル・回数・費用について設計者の了承を得てから行う。模擬提供者を使う場合は、追加の利用料は生じない。

1. ファイルを一つ含む試験用プロジェクトで対話を始め、write/edit で変更を作る。`/rewind` で ↑・↓・Enter・ESC を使い、時点の選択、会話不可の時点で files only だけになること、窓が選択に追従することを確かめる。
2. 戻せない操作があるときは `Cannot be undone: N operations` の件数が承認に必要な事実として出ること、一覧が空なら見出しが出ないことを確かめる。確認画面で n を押した場合にファイルが変わらないこと、再度選んで y を押した場合に戻ることを確かめる。確認の前に y を連打し、端末を縮め、貼り付けも試す。事実の全文が収まらない間は y/f が受け付けられないことを確かめる。
3. `/status`、`/new`、`/status` を順に送り、ID が替わり、起動の案内が再表示され、前の出力が端末の履歴に残ることを確かめる。同じプロジェクトの入力履歴を ↑・↓ で辿る。
4. 終了前に `stty -a` を記録する。応答を待つ処理の途中で `/quit` を送り、終了状態が0で、終了後の `stty -a` が記録と一致することを確かめる。待機中の `/quit` も同じ形で確かめる。

5. `/compact` の最中に `//new` を Alt+Enter で後続に積み、ESC か Ctrl+C で取り消す。入力欄に `//new` が戻り、Enter で送り直しても `/new` を実行しないことを確かめる。通常の依頼の途中の操縦と後続も同様に確かめる。
6. 多数のファイルを戻す `/rewind` で、復元前の時点の記録が表示された後に、外から TERM と HUP を送る。終了状態130となり、端末の復元後に `Checkpoint before rewind: ID` が出ることを確かめる。復元完了後に同じ操作をした場合は、その行が出ないことも確かめる。これは上表の擬似端末のテストでも実行する。
