# 2026-11-02 の作業のまとめ

本ディレクトリは、2026-11-02 に、Claude Code のクラウドのセッション（使い捨ての x86_64 の Linux）で行った作業と、その結果をまとめる。作業はオーケストレータ（Claude Code）が、設計者の指示を受けて行った。ブランチは `claude/stoic-ride-otqoj5` であり、`gamou` にはまだ取り込んでいない。

## 作業の条件

設計者が次の条件を決めた。

- macOS が要る項目は、Linux での確かめだけを行う。macOS での確かめは別途とする。
- 本物の LLM を呼ぶ項目は行わない。テストは模擬の提供者だけを使う。
- 時々失敗するテストは、テストの側で直し、本番のコードを変えない。
- 段 1（[05](05-stage1.md)）は、安全の穴を塞ぐために本番のコードを変える。

## 文書

| 文書 | 内容 |
|---|---|
| [01-cloud-linux-check.md](01-cloud-linux-check.md) | クラウドの x86_64 の Linux で検査を走らせる環境（準備の道具、podman の試験の機械との違い）と、検査の各段の結果 |
| [02-session-flaky-tests.md](02-session-flaky-tests.md) | `tests/session.rs` の時々失敗するテストの再現、原因の推定と確かめ、テストの側の修正 |
| [03-other-test-files.md](03-other-test-files.md) | 同じ競合が、ほかのテストのファイルと `src/` の単体テストにないかの調べと、見つかった 2 件の修正 |
| [04-oss-feature-review.md](04-oss-feature-review.md) | Codex CLI、Pi Coding Agent、Hermes Agent、DeepSeek Harness、nono、sandbox-runtime のソースコードとの機能の比べと、キャッチアップの案 |
| [05-stage1.md](05-stage1.md) | キャッチアップの案の段 1（安全の穴を塞ぐ）の確かめと修正 |

## 変更したファイル

| ファイル | 種類 | 内容 | 文書 |
|---|---|---|---|
| `scripts/cloud-linux-setup.sh` | 追加 | クラウドの Linux の準備（bubblewrap、処理系 `v0.0.12` のビルド、`alpine:3.22` の rootfs、試験の利用者） | [01](01-cloud-linux-check.md) |
| `scripts/cloud-linux-run.sh` | 追加 | rootfs へ `pivot_root` し、非 root でテストを走らせる cargo の runner | [01](01-cloud-linux-check.md) |
| `crates/benitoiteh/tests/session.rs` | 変更 | ファイルで共有する `RwLock`。`Fixture::new()` は共有の側、子プロセスを起こす 4 件は `Fixture::spawning()` で排他の側を持つ。二つ目の準備に `Fixture::sibling()` を加えた | [02](02-session-flaky-tests.md) |
| `crates/benitoiteh/tests/sandbox_linux.rs` | 変更 | `stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths` で、錠を手放した後の回収を最大 2 秒やり直す | [03](03-other-test-files.md) |
| `crates/benitoiteh/src/cli/exec/tests.rs` | 変更 | `Locked` の取り直しを `open_released` にまとめ、`fork_retains_copy_when_current_provider_was_removed` でも使う | [03](03-other-test-files.md) |
| `crates/benitoiteh/src/sandbox/linux/mounts.rs` | 変更 | 入れ子の守る場所と gitdir の指す先の祖先を、書ける形で自分自身へ重ねる | [05](05-stage1.md) |
| `crates/benitoiteh/src/sandbox/linux/mounts/tests.rs` | 変更 | 引数列の期待値に祖先の留めを加えた | [05](05-stage1.md) |
| `crates/benitoiteh/tests/sandbox_linux.rs` | 変更 | 否定のテスト `existing_nested_metadata_ancestors_cannot_be_renamed_away` を加えた | [05](05-stage1.md) |
| `docs/design/03-safety/03-02-sandbox.md` | 変更 | 「Linux のファイルシステムの見え方」に「守る場所の祖先」を加えた | [05](05-stage1.md) |
| `docs/2026-11-02/` | 追加 | 本ディレクトリ | — |
| `docs/README.md` | 変更 | 本ディレクトリへの案内を加えた | — |

段 1 より前は、本番のコード（`crates/benitoiteh/src/` のうちテストでないもの）を変えていない。結果の記録は、はじめ `docs/plan/results/2026-11-02-cloud-linux.md` に書いたが、設計者の指示で本ディレクトリへ移し、元のファイルを消した。

## コミット

| コミット | 内容 |
|---|---|
| `22e83e8` | クラウドの Linux でテストを走らせるスクリプトを追加 |
| `c00ec83` | クラウドの Linux での検査と、時々失敗するセッションの後始末のテストを記録 |
| `2e23f8d` | 子プロセスを起こすセッションのテストを、錠に依るテストと重ねない |
| `197dee2` | ほかのテストのファイルの同じ flock の競合を記録 |
| `0cfacd0` | 継承された錠を短時間待つ形を、さらに 2 件のテストに加える |
| `bb0c9e2` | ほかのテストのファイルの節の古い注記を直す |

この後に、記録を本ディレクトリへ移すコミットが続く。

## 確かめた検査

すべて x86_64 の Linux（Alpine 3.22.7 の rootfs、非 root）で行った。

| 検査 | 結果 |
|---|---|
| `cargo test --workspace --target x86_64-unknown-linux-musl` | 最後の修正の後に 754 件が通り、失敗 0、無視 16 |
| `cargo fmt --all --check` | 通った |
| `cargo clippy -p benitoiteh --all-targets`（ホスト、`x86_64-unknown-linux-musl`、`aarch64-apple-darwin`） | 通った。`aarch64-apple-darwin` は Linux からの検査だけである |
| `cargo deny check licenses advisories`、`tools/perf` の Python のテスト | 通った |
| `tests/session.rs` を並べて 40 回 | 直す前は 9 回失敗し、直した後は 0 回 |
| 子プロセスを起こし続けるスレッドを差し込んだ 15 回（`stale_placeholder…` と `fork_retains_copy…`） | 直す前は 3 回と 2 回失敗し、直した後は 0 回と 0 回 |

## 残っていること

- macOS での実行。`aarch64-apple-darwin` の確かめは Linux からのコンパイルと lint だけである。macOS だけで走る `pinned_temporary_directory_uses_an_alternate_name_on_resume` も、`Fixture::spawning()` に変えた。
- aarch64 の Linux（Apple の `container`）での実行。
- 原因を確かめていない失敗 2 件（`tests/exec.rs::closed_event_output_still_finishes_the_session_record`、`tests/tui/commands.rs` の `accepted_rewind_finishes_before_quit_and_repeated_keyboard_interrupts`）。[03](03-other-test-files.md)の「ほかの失敗」に記した。
- 評価の道具の Python のテストのうち、macOS の判定を要しない 4 件の失敗の原因（[01](01-cloud-linux-check.md)の「結果」）。
