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
