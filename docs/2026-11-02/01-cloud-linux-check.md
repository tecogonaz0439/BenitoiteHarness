# クラウドの x86_64 の Linux での検査

本書は、2026-11-02 に、Claude Code のクラウドのセッション（使い捨ての x86_64 の Linux）で、x86_64 の Linux のテストを走らせるために整えた環境と、検査の結果を記録する。オーケストレータ（Claude Code）が行った。設計上の決定を加えるものではない。まとめは [README](README.md) にある。

本物の LLM は呼んでいない。macOS で確かめる項目（Seatbelt、評価の道具の判定）と aarch64 の Linux（Apple の `container`）は、この環境にないので扱わず、別途とする。

## 準備の道具

`scripts/cloud-linux-setup.sh` を root で走らせると、次を用意し、最後に設定する環境変数を示す。

- ホストの bubblewrap と musl-tools（apt）、Rust の `x86_64-unknown-linux-musl` の対象
- Benitoite の処理系 `v0.0.12`（リリースのタグの `git archive` から release ビルドする。タグのコミットが設計書の記録と一致することを確かめる）
- `alpine:3.22` の rootfs（マニフェストの digest で固定する）と、非 root の試験の利用者 `bhtest`

`scripts/cloud-linux-run.sh` は cargo の runner であり、テストの実行ファイルを rootfs の中で、手元と同じパスに見せ、非 root で走らせる。使い方は次のとおりである（値は `cloud-linux-setup.sh` が最後に示す）。

```bash
export BENITOITEH_CLOUD_LINUX_DIR=/opt/benitoiteh-cloud-linux
export BENITOITEH_TEST_BENITOITE=/opt/benitoiteh-cloud-linux/benitoite-v0.0.12/target/x86_64-unknown-linux-musl/release/benitoite
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUNNER="$PWD/scripts/cloud-linux-run.sh"
cargo build -p sandbox-probe --target x86_64-unknown-linux-musl
cargo test --workspace --target x86_64-unknown-linux-musl
```

`sandbox-probe` は、`scripts/check.sh` と同じく、テストの前にビルドしておく。これがないと、`sandbox_*.rs` と `run.rs` などのテストが準備の段で失敗する。`.cargo/config.toml` の runner（Apple の `container` を使う `scripts/container-run.sh`）は、上の環境変数で置き換える。

## 環境

| 項目 | 値 |
|---|---|
| ホスト | Ubuntu 24.04（noble）、Linux 6.18.44-fc-v185 x86_64、Intel Xeon 2.80GHz の 4 コア、メモリー 15 GB。root で動く |
| Landlock | `effective_abi: V7, kernel_abi: None`（`sandbox_linux.rs::kernel_landlock_and_seccomp_are_available` の出力）。[P2 の結果](../plan/results/P2-safety.md)の X（V9）より古い |
| bubblewrap | 0.9.0（Ubuntu の `bubblewrap 0.9.0-1ubuntu0.3`）。setuid の印はない。[サンドボックス](../design/03-safety/03-02-sandbox.md)の要件（0.8.0 以上）を満たす |
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
| `cargo test --workspace --target x86_64-unknown-linux-musl`（Alpine、非 root） | 1 回目は 754 件が通り、失敗 0、無視 16（無視はすべて手で走らせる測定）。`sandbox_linux.rs` の 34 件を含む。2 回目は、[時々失敗するセッションのテスト](02-session-flaky-tests.md)の 1 件が失敗した |
| 同じテストを、Ubuntu のホストの root で直接走らせた場合 | 4 件が失敗した。`sandbox_linux.rs` の `dynamic_loader_must_be_denied_to_confine_execution` と `reading_excludes_secrets_and_confines_readable_roots` は Alpine を前提とする（`/lib/ld-musl-*`、`/etc/alpine-release`）。`target_cannot_inherit_or_open_control_descriptors_and_stdin_modes_work` と `setup_failures_do_not_execute_the_program_and_cloexec_descriptors_do_not_leak` は、能力（capability）を持たない利用者を前提とする。Alpine・非 root の形では通るので、試験の環境の違いであり、コードの誤りではない |
| `cargo deny check licenses advisories` | 通った |
| `tools/perf` の Python のテスト | 通った |
| `tools/skill-eval` の Python のテスト（`BENITOITEH_SKILL_EVAL_JUDGE_SANDBOX=0`、`BENITOITEH_SKILL_EVAL_STAGED_E2E=0`） | 118 件のうち、失敗 3、誤り 9、飛ばし 42。誤りのうち 8 件は `judgment requires the macOS sandbox`。残りの 4 件（`test_workspace_processes_stop_before_snapshot_and_judgment` の 3 件、`test_security_json_recursion_special_files_and_record_redaction` の 1 件）の原因は、2026-11-03 に確かめた（[09](09-linux-followups.md)。1 件は Python の版の違いでテストを直し、3 件は macOS を前提とする）。作業のディレクトリ（`TMPDIR`）を `/tmp` の下に置くと、評価の道具が `--work-root` を拒んで、さらに 13 件が誤りになる。macOS では `TMPDIR` が `/var/folders` の下なので起きない |
| release の `benitoiteh` の大きさ（`x86_64-unknown-linux-musl`） | 7,494,568 バイト |
