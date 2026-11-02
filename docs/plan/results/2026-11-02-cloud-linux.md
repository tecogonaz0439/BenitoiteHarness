# クラウドの x86_64 の Linux での検査と、時々失敗するセッションのテストの調べ

本書は、2026-11-02 に、Claude Code のクラウドのセッション（使い捨ての x86_64 の Linux）で x86_64 の Linux のテストを走らせた結果と、その環境で再現した時々失敗するテスト（[P5-37](../tasks/P5-37-post-release-followups.md) の「時々失敗したテスト」の一行目）の調べの結果を記録する。オーケストレータ（Claude Code）が行った。設計上の決定を加えるものではなく、時々失敗するテストの直し方は、設計者の決定（2026-11-02）に従った。

本物の LLM は呼んでいない。macOS で確かめる項目（Seatbelt、評価の道具の判定）と aarch64 の Linux（Apple の `container`）は、この環境にないので扱わず、別途とする。

## 環境

| 項目 | 値 |
|---|---|
| ホスト | Ubuntu 24.04（noble）、Linux 6.18.44-fc-v185 x86_64、Intel Xeon 2.80GHz の 4 コア、メモリー 15 GB。root で動く |
| Landlock | `effective_abi: V7, kernel_abi: None`（`sandbox_linux.rs::kernel_landlock_and_seccomp_are_available` の出力）。[P2 の結果](P2-safety.md)の X（V9）より古い |
| bubblewrap | 0.9.0（Ubuntu の `bubblewrap 0.9.0-1ubuntu0.3`）。setuid の印はない。[サンドボックス](../../design/03-safety/03-02-sandbox.md)の要件（0.8.0 以上）を満たす |
| テストの rootfs | `alpine:3.22`（3.22.7）。linux/amd64 のマニフェスト `sha256:dc92f017beea8e7f43263073d717f7bc011228c95d5312e440deb4d035bb4eec`（Docker Hub と mirror.gcr.io の索引が同じ値を示した） |
| 処理系 | Benitoite `v0.0.12`（コミット `d0678ddb7e82da77715ea2e01ef974d06d41814b`）を、タグの `git archive` から `x86_64-unknown-linux-musl` で release ビルドした static-pie の ELF。`--version=json` の `version` は `0.0.12` |
| Rust | 1.98.1（`rust-toolchain.toml`） |

準備は `scripts/cloud-linux-setup.sh`、テストの runner は `scripts/cloud-linux-run.sh` で行う。設計者の試験の機械の podman（`scripts/remote-linux-run.sh`）との違いは次のとおりである。

| 点 | 理由 |
|---|---|
| apk で入れる代わりに、ホストの bubblewrap・bash・python3（3.13）・setpriv と、その共有ライブラリを rootfs に写した | Alpine のパッケージの取得先（`dl-cdn.alpinelinux.org` とミラー）が、この環境の proxy で拒否された |
| podman の代わりに、新しいマウントと PID の名前空間で rootfs に `pivot_root` し、`setpriv` で非 root の利用者（`bhtest`）に変えて走らせた | docker のデーモンがない。chroot の中では非特権の user namespace を作れず、bubblewrap が動かない |

処理系の実行ファイルは、ビルドしたディレクトリが違うと SHA-256 が変わった（`/home/user/work-benitoite/v0.0.12` で `fc654200…`、`/opt/benitoiteh-cloud-linux/benitoite-v0.0.12` で `1e0321f9…`）。ビルドのパスが実行ファイルに入るためと見られるが、確かめていない。

## 結果

| 段 | 結果 |
|---|---|
| `cargo fmt --all --check`、`cargo clippy --workspace --all-targets`（ホストと `x86_64-unknown-linux-musl`） | 通った |
| `cargo test --workspace --target x86_64-unknown-linux-musl`（Alpine、非 root） | 1 回目は 754 件が通り、失敗 0、無視 16（無視はすべて手で走らせる測定）。`sandbox_linux.rs` の 34 件を含む。2 回目は、下の時々失敗するテストの 1 件が失敗した |
| 同じテストを、Ubuntu のホストの root で直接走らせた場合 | 4 件が失敗した。`sandbox_linux.rs` の `dynamic_loader_must_be_denied_to_confine_execution` と `reading_excludes_secrets_and_confines_readable_roots` は Alpine を前提とする（`/lib/ld-musl-*`、`/etc/alpine-release`）。`target_cannot_inherit_or_open_control_descriptors_and_stdin_modes_work` と `setup_failures_do_not_execute_the_program_and_cloexec_descriptors_do_not_leak` は、能力（capability）を持たない利用者を前提とする。上の Alpine・非 root の形では通るので、試験の環境の違いであり、コードの誤りではない |
| `cargo deny check licenses advisories` | 通った |
| `tools/perf` の Python のテスト | 通った |
| `tools/skill-eval` の Python のテスト（`BENITOITEH_SKILL_EVAL_JUDGE_SANDBOX=0`、`BENITOITEH_SKILL_EVAL_STAGED_E2E=0`） | 118 件のうち、失敗 3、誤り 9、飛ばし 42。誤りのうち 8 件は `judgment requires the macOS sandbox`。残りの 4 件（`test_workspace_processes_stop_before_snapshot_and_judgment` の 3 件、`test_security_json_recursion_special_files_and_record_redaction` の 1 件）の原因は調べていない。作業のディレクトリ（`TMPDIR`）を `/tmp` の下に置くと、評価の道具が `--work-root` を拒んで、さらに 13 件が誤りになる。macOS では `TMPDIR` が `/var/folders` の下なので起きない |
| release の `benitoiteh` の大きさ（`x86_64-unknown-linux-musl`） | 7,494,568 バイト |

## 時々失敗するセッションのテスト

### 再現

`tests/session.rs` を、条件を変えて繰り返し走らせた。

| 条件 | 失敗した回 |
|---|---|
| Alpine、非 root、テストを並べて走らせる（既定） | 20 回のうち 7 回 |
| Alpine、非 root、`--test-threads=1` | 10 回のうち 0 回 |
| Ubuntu のホストの root で直接 | 10 回のうち 0 回 |

失敗したテストは次の 4 件である（20 回のうちの延べの回数）。いずれも後始末（`cleanup`）が消したものの数か、残ったものの数を確かめる箇所で失敗した。

- `cleanup_reports_failures_continues_and_retries_without_losing_locks`（2 回）
- `expired_session_tombstone_outlives_its_unremovable_temporary_directory`（2 回）
- `changed_temporary_directories_do_not_stop_resume_or_cleanup`（2 回。P5-37 の表にない）
- `cleanup_removes_only_inactive_matching_directories_and_expired_sessions`（1 回。P5-37 の表にない）

root で失敗しないのは、少なくとも `cleanup_reports_failures_continues_and_retries_without_losing_locks` が root では先頭で飛ばされる（`geteuid().is_root()` の分岐）ためである。

### 推定する原因

後始末は、セッションと一時ディレクトリの `lock` を `flock` の非待機の排他で取れないとき、使用中として何も言わずに飛ばす（`src/session/mod.rs` の `try_lock` の `SessionError::Locked` で `Ok(None)` を返す分岐）。`flock` の錠は開いたファイルの記述に結び付き、`fork(2)` などで複製した記述子も同じ錠を指す（man-pages 6.7 の [flock(2)](https://man7.org/linux/man-pages/man2/flock.2.html) の DESCRIPTION。Ubuntu の `manpages-dev 6.7-2` で確かめた）。記述子に `O_CLOEXEC` を付けていても、子は `exec` するまで複製を持つ。

テストのプロセスの中で、別のテストのスレッドが子プロセスを起こした瞬間に、そのテストが錠を持つ `Session` を落とすと、子が `exec` するまで錠が残る。その直後の後始末は、そのディレクトリを使用中と見て消さないため、数が合わなくなる、という推定である。

### 確かめ

`cleanup_reports_failures_continues_and_retries_without_losing_locks` だけを、手元で一時的に書き換えて走らせた（書き換えはコミットしていない）。書き換えは、テストの先頭で、同じプロセスの別のスレッドに処理を続けさせるものである。テストは一つだけを走らせた（`--exact`）。

| 別のスレッドの処理 | 失敗した回 |
|---|---|
| なし | 20 回のうち 0 回 |
| CPU だけを使う計算を繰り返す（子プロセスを起こさない） | 20 回のうち 0 回 |
| `benitoiteh --version` を子プロセスとして起こし続ける（1 スレッド） | 30 回のうち 3 回 |
| 同じ（4 スレッド） | 20 回のうち 7 回 |

失敗の箇所は、後始末が消したセッションの数（`sessions_removed`）、失敗の数（`failures.len()`）、残った一時ディレクトリの数で、元の失敗と同じ種類の箇所だった。子プロセスを起こすときだけ失敗し、起こす量を増やすと失敗が増えたので、推定と矛盾しない。ただし、錠を持っていた子プロセスを直接に観察してはいない。また、macOS で同じ原因かは確かめていない（P5-37 の記録は主に macOS の検査で出たもの）。P5-37 の「TMPDIR の共有が原因という説」については、各テストは自分の一時の根（`Fixture` の `tmp`）を使うので、根の共有よりも、この錠の複製の方が今回の観察に合う。

### 直し方

直し方の候補は二つあった。

- テストの側: 子プロセスを起こすテストを、セッションの錠を扱うほかのテストと同じプロセスで重ねない。
- コードの側: 使用中と見た項目を、短い間を置いて取り直す。ただし、子の `exec` の遅れに上限はないので、確率を下げるだけである。利用者にとっての影響は、次の起動の後始末まで消すのが遅れることにとどまる。

2026-11-02 に、設計者がテストの側で直すと決めた。`tests/session.rs` に、ファイルの中で共有する `RwLock` を置いた。

- `Fixture::new()` は読みの錠（共有）を取り、`Fixture` が落ちるまで持つ。セッションの錠を扱うテストは、すべて `Fixture` を使う。
- 子プロセスを起こす 4 件（`cleanup_reports_failures_continues_and_retries_without_losing_locks`、`deep_cleanup_and_session_drop_do_not_depend_on_thread_stack_depth`、`pinned_temporary_directory_uses_an_alternate_name_on_resume`（macOS のみ）、`fork_copies_more_references_than_the_descriptor_limit`）は、`Fixture::spawning()` で書きの錠（排他）を取る。
- 一つのテストで二つの `Fixture` を使う `shared_temporary_cleanup_preserves_foreign_live_entries_and_unrecorded_helpers` は、二つ目を `Fixture::sibling()` で作り、錠を重ねて取らない。同じスレッドで読みの錠を重ねて取ると、待っている書きの錠と互いに待ち合うことがあるためである。

本番のコードは変えていない。

### 直した後の確かめ

同じ条件（Alpine、非 root、`tests/session.rs` を既定の並べた形）で走らせた。

| コード | 失敗した回 |
|---|---|
| 直す前（上の再現と、直した後の確かめと同じ時点の再実行の合計） | 40 回のうち 9 回（7/20、2/20） |
| 直した後 | 40 回のうち 0 回 |

直した後に、`cargo test --workspace --target x86_64-unknown-linux-musl`（754 件が通り、失敗 0）、`cargo fmt --all --check`、`cargo clippy -p benitoiteh --all-targets`（ホスト、`x86_64-unknown-linux-musl`、`aarch64-apple-darwin`）が通った。`aarch64-apple-darwin` は Linux からの検査だけで、macOS での実行は別途とする。

## ほかのテストのファイルの同じ競合

`tests/session.rs` を直した後に、ほかのテストのファイルと `src/` の単体テストに、同じ競合（同じテストのプロセスの別のスレッドが起こした子が、`exec` するまで記述子の複製を持つこと）で結果が変わる箇所がないかを調べた。見つかった 2 件の直し方は、この節の最後の「直し方と確かめ」に記す。

### 調べ方

1. 本番のコードで `flock` を使う場所を探し、複製が結果を変えうるものを選んだ。
2. テストで、錠を持つ記述子を落とした直後に、別の記述子で非待機の錠を取る処理（後始末、回収、再開）の結果を確かめる箇所を探した。
3. 候補のテストに、上の「確かめ」と同じ差し込み（`benitoiteh --version` を起こし続ける 4 スレッド。`src/` の単体テストでは `/bin/true`）を、別の git の worktree で一時的に加え、一つだけ（`--exact`）を 15 回ずつ走らせた。差し込みのない同じ回数を対照とした。書き換えはコミットしていない。
4. 差し込みなしの全体のテスト（`cargo test --workspace --target x86_64-unknown-linux-musl`、Alpine、非 root）を 20 回繰り返し、自然に起きる失敗を数えた。この 20 回は、上の直し（`tests/session.rs`）を含むコードで走らせた。

### 本番のコードの flock

| 場所 | 錠の取り方 | 複製の影響 |
|---|---|---|
| `src/session/mod.rs` | 非待機の排他。取れなければ使用中と見る | 結果が変わる（上の節） |
| `src/sandbox/linux/mounts/placeholders.rs` の回収（`reap`） | 非待機の排他。取れなければ、その記録と塞ぐ場所を回収しない | 結果が変わる |
| `src/auth/store.rs`、`src/ui/tui/history.rs` | 待機する排他（`auth/store.rs` の `lock_cancellable` は 20 ms ごとに取り直す） | 子が `exec` するまで待つだけで、結果は変わらない |

### すでに取り直しで扱っている箇所

次の箇所は、この競合を注記したうえで、`Locked` のときだけ取り直している。今回の調べの対象から外した。

- `tests/support/session.rs` の `open_created_session`（10 ms ごとに最大 100 回）
- `src/agent/resume_tests.rs`（最大 2 秒）
- `src/cli/exec/tests.rs` の再開の準備（最大 2 秒）

`tests/rewind.rs` と `tests/tui.rs` の再開は、持ったままの `Session` を `Agent::resume` に渡し、錠を取り直さないので、影響しない。

### 候補と結果

| テスト | 錠を落とした後の処理 | 差し込みあり | 対照 | 差し込みなしの全体の 20 回 |
|---|---|---|---|---|
| `tests/sandbox_linux.rs::stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths` | `drop(lock)` の直後に次のサンドボックスを準備し、回収で `!locked.exists()` を確かめる | 15 回のうち 3 回（すべて `!locked.exists()`） | 15 回のうち 0 回 | 20 回のうち 1 回（同じ箇所） |
| `src/cli/exec/tests.rs::fork_retains_copy_when_current_provider_was_removed` | 失敗したフォークが残した写しを、取り直しなしの `Session::open` で開く | 15 回のうち 2 回（`Error: Locked`） | 15 回のうち 0 回 | 0 回 |
| `tests/sandbox_exec_restricted.rs::continue_selects_latest_matching_readable_inactive_session` | `saved` で作って落としたセッションを、子の `--continue` が選ぶ | 15 回のうち 1 回（`Kind(TimedOut)`。錠の症状ではなく、4 コアを差し込みが占めたための遅れと見る） | 調べていない | 0 回 |
| `tests/sandbox_exec_restricted.rs::resume_errors_preserve_records_and_never_escalate_or_follow_links` | `drop(lock)` の後に子の `--resume` が形式の誤りを報告する | 15 回のうち 0 回 | — | 0 回 |
| `tests/sandbox_exec_restricted.rs::sessions_lists_last_mode_filters_projects_and_survives_unreadable_records` | `saved` で作って落としたセッションを、子の `sessions` が一覧にする | 15 回のうち 0 回 | — | 0 回 |
| `src/sandbox/linux/mounts/tests.rs::placeholder_records_preserve_path_bytes_and_validate_record_identity` | 回収を続けて呼び、記録が消えたことを確かめる | 15 回のうち 0 回 | — | 0 回 |

`stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths` は、P5-37 の「時々失敗したテスト」の 5 行目（P5-04 の x86_64 の全体の検査で、錠のあるパスの存在の確かめが一度失敗した）に当たる。差し込みで同じ箇所が失敗し、差し込みなしでも同じ箇所が 20 回のうち 1 回失敗したので、`tests/session.rs` と同じ原因と見る。

`fork_retains_copy_when_current_provider_was_removed` は、差し込みで `Locked` になった。差し込みなしの 20 回では失敗しなかったが、同じ形の箇所で取り直しを入れたテスト（上の「すでに取り直しで扱っている箇所」）と並ぶ、取り直しのない箇所である。

錠の症状が出なかった候補は、錠を落としてから錠を取り直すまでに子プロセスの起動などがはさまり、窓が狭いためと見るが、確かめていない。

### ほかの失敗（同じ競合と確かめていないもの）

差し込みなしの全体の 20 回で、ほかに次の 2 件が 1 回ずつ失敗した。

| テスト | 症状 | 見立て |
|---|---|---|
| `tests/exec.rs::closed_event_output_still_finishes_the_session_record` | `Error: Kind(UnexpectedEof)`（P5-37 の記録では `BrokenPipe`） | テストが子の標準出力の読み口を閉じ、子が書き込みの失敗を受けることを前提とする。別のスレッドの子が読み口の複製を持つと閉じたと見えない、という道筋はありうるが、`?` で返った誤りの場所を特定していない |
| `tests/tui/commands.rs` の `accepted_rewind_finishes_before_quit_and_repeated_keyboard_interrupts` | 画面の出力に `Waiting for rewind to finish` が現れなかった | 時刻に依る表示の確かめであり、上の差し込みの実験と同時に走った回で起きた。負荷による遅れと見るが、確かめていない |

`tests/session.rs` は、直した後のコードで 20 回とも通った。

### 直し方と確かめ

2026-11-02 に、設計者が次の 2 件を直すと決めた。どちらも、このリポジトリで同じ競合に使ってきた形（`Locked` や回収の残りだけを短時間待ち、錠を放さない退行は 2 秒で失敗にする）に揃えた。読み書きの錠（`tests/session.rs` の形）にしなかったのは、確かめる箇所が一つであり、`tests/sandbox_linux.rs` の 36 件のテストのすべてに共有の側を持たせる変更が要るためである。本番のコードは変えていない。

- `stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths`: `drop(lock)` の後の準備（回収を含む）を、`locked` が消えるまで 20 ms ごとに最大 2 秒やり直す。
- `fork_retains_copy_when_current_provider_was_removed`: `src/cli/exec/tests.rs` の再開の後の取り直し（`declined_normal_resume_preserves_record_bytes_and_releases_lock` の中にあった）を `open_released` にまとめ、このテストの `Session::open` にも使う。

| テスト | 直す前の差し込みあり | 直した後の差し込みあり |
|---|---|---|
| `stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths` | 15 回のうち 3 回 | 15 回のうち 0 回 |
| `fork_retains_copy_when_current_provider_was_removed` | 15 回のうち 2 回 | 15 回のうち 0 回 |

直した後に、`cargo test --workspace --target x86_64-unknown-linux-musl`（754 件が通り、失敗 0）、`cargo fmt --all --check`、`cargo clippy -p benitoiteh --all-targets`（ホスト、`x86_64-unknown-linux-musl`、`aarch64-apple-darwin`）が通った。
