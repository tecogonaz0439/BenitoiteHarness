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
| [06-stage3.md](06-stage3.md) | キャッチアップの案の段 3（通信の制限）の設計と実装 |
| [07-stage4.md](07-stage4.md) | キャッチアップの案の段 4（使い勝手）の設計と実装（2026-11-03 まで） |
| [08-stage2.md](08-stage2.md) | キャッチアップの案の段 2（エージェントの性能）の 1〜3 を、評価用の切り替えとして作った記録（2026-11-03） |

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
| `crates/benitoiteh/src/config/environment.rs`、`src/config/mod.rs`、`src/config/text.rs` | 変更 | 子プロセスの環境から秘密に見える名前を除き、`[secrets] pass_env` で通す名前を選べるようにした | [05](05-stage1.md) |
| `crates/benitoiteh/src/tools/benitoite_process.rs` | 変更 | 制限モードの実行で、印の付け直しだけを `remark_child_environment` で行う | [05](05-stage1.md) |
| `crates/benitoiteh/tests/config.rs` ほか、`child_environment` の呼び出し元 | 変更 | 環境と設定のテストに行を足し、ほかの呼び出し元に引数を足した | [05](05-stage1.md) |
| `docs/design/03-safety/03-01-security-model.md`、`03-02-sandbox.md`、`04-benitoite/04-01-benitoite-integration.md`、`05-interface/05-01-cli.md` | 変更 | 子プロセスの環境の方針と `pass_env` を書いた | [05](05-stage1.md) |
| `crates/benitoiteh/src/cli/mod.rs` | 変更 | 読ませないホームの場所に、資格情報、保管庫、履歴、ブラウザの組を加えた | [05](05-stage1.md) |
| `crates/benitoiteh/tests/sandbox_exec_restricted.rs` | 変更 | 通常モードの `bash` が HOME の秘密を読めないことを確かめる行を足した | [05](05-stage1.md) |
| `docs/design/03-safety/03-02-sandbox.md` | 変更 | 「読ませないホームの場所」を加えた | [05](05-stage1.md) |
| `crates/benitoiteh/src/tools/bash.rs` | 変更 | `escalate` の一回の `PATH` から、書ける場所に届く項目を除く | [05](05-stage1.md) |
| `crates/benitoiteh/tests/tools_bash.rs`、`tests/support/bash.rs` | 変更 | 外で動かす一回の `PATH` のテストと、`PATH` を差し替える準備を加えた | [05](05-stage1.md) |
| `docs/design/03-safety/03-02-sandbox.md` | 変更 | 「通常モードの方針」に、外で動かす一回の `PATH` の扱いを加えた | [05](05-stage1.md) |
| `crates/benitoiteh/src/cli/mod.rs`、`src/cli/text.rs` | 変更 | Linux で、主のプロセスを起動の直後にダンプできない形にする | [05](05-stage1.md) |
| `crates/benitoiteh/tests/sandbox_exec_restricted.rs` | 変更 | 外で動く `bash` が主のプロセスの環境を読めないことを確かめるテストを加えた | [05](05-stage1.md) |
| `docs/design/03-safety/03-02-sandbox.md`、`docs/design/open-issues.md` | 変更 | 「主のプロセスの守り」を加え、macOS の扱いを BH-118 とした | [05](05-stage1.md) |
| `crates/benitoiteh/src/`（サンドボックス、道具、確認、ループ、CLI、画面） | 変更 | `.agents` を守る名前に加え、`write_agents` と `exec --allow-agents-write` で、一回ごとの確認の後にだけ書けるようにした | [05](05-stage1.md) |
| `crates/benitoiteh/tests/`（実行、テスト、書く道具、exec、Linux のサンドボックス、引数）と `tests/expected/` | 変更 | `.agents` の守りと確認のテストを加え、道具の定義の期待を作り直した | [05](05-stage1.md) |
| `docs/design/01-agent/01-03-tools.md`、`03-safety/03-02-sandbox.md`、`03-safety/03-03-approval.md`、`05-interface/05-01-cli.md` | 変更 | 「.agents の守り」と「.agents への書き込み」を加え、道具の引数とオプションを書いた | [05](05-stage1.md) |
| `crates/benitoiteh/src/sandbox/restricted.rs`、`src/tools/exclude.rs` | 変更 | 制限モードで、プロジェクトの中の秘密の名前を根から 3 段まで読ませない（BH-100） | [05](05-stage1.md) |
| `crates/benitoiteh/src/cli/mod.rs` | 変更 | 読ませないホームの場所に、`~/.zshenv` を除くシェルの起動の設定を加えた | [05](05-stage1.md) |
| `crates/benitoiteh/tests/sandbox_benitoite_run.rs`、`tests/sandbox_exec_restricted.rs` | 変更 | 上の二つのテストを加え、行を足した | [05](05-stage1.md) |
| `docs/design/01-agent/01-03-tools.md`、`03-safety/03-01-security-model.md`、`03-safety/03-02-sandbox.md`、`open-issues.md` | 変更 | BH-100 を閉じ、起動の設定と BH-118 の決定を書いた | [05](05-stage1.md) |
| `crates/benitoiteh/src/config/trust.rs`（追加）、`src/context/skills.rs`、`src/cli/exec.rs`、`src/cli/tui.rs`、`src/cli/args.rs`、`src/agent/`、`src/ui/tui/` | 変更 | プロジェクトの Skill・プラグイン・`model` を、信頼の確認の後にだけ使う。決定を根ごとに保存し、`exec --trust-project` を加えた | [05](05-stage1.md) |
| `crates/benitoiteh/tests/exec.rs`、`tests/exec/skills.rs`、`tests/args.rs` と単体テスト | 変更 | 信頼のテストを加え、プロジェクトの資源を読む既存のテストに `--trust-project` を付けた | [05](05-stage1.md) |
| `docs/design/03-safety/03-01-security-model.md`、`01-agent/01-04-context.md`、`05-interface/05-01-cli.md` | 変更 | 「プロジェクトの信頼」を加え、オプション、データの置き場所、Skill とプラグインの読む条件を書いた | [05](05-stage1.md) |
| `crates/benitoiteh/src/sandbox/proxy.rs`（追加）、`proxy/http.rs`（追加）ほか、`src/sandbox/`、`src/tools/bash.rs`、`src/config/`、`src/cli/exec.rs`、`src/agent/`、`src/approval/`、`src/ui/tui/`、`crates/sandbox-probe/` | 変更 | 通常モードの通信の中継（`network = "proxy"`） | [06](06-stage3.md) |
| `crates/benitoiteh/tests/`（Linux のサンドボックス、bash、exec、設定、方針、画面） | 変更 | 中継のテストを加えた | [06](06-stage3.md) |
| `docs/design/03-safety/03-02-sandbox.md`、`03-03-approval.md`、`04-benitoite/04-01-benitoite-integration.md`、`05-interface/05-01-cli.md`、`open-issues.md` | 変更 | 「通信の中継」「通信の確認」と処理系への要望の 7 を加え、BH-046 を閉じ、BH-040 に方針を書き、BH-119 を加えた | [06](06-stage3.md) |
| `crates/benitoiteh/src/`（セッション、提供者、Agent、CLI、文脈、道具、画面）と `prompts/init.md`（追加） | 変更 | 段 4 の使い勝手の項目（記録を残さないセッション、送る入力の表示、雛形と `/init`、通知、書き出し、外部のエディタ、`@` の補い、画像の添付、`/model`、出力の型） | [07](07-stage4.md) |
| `crates/benitoiteh/tests/`（セッション、exec、引数、CLI、設定、画面、提供者）と単体テスト | 変更 | 段 4 のテストを加えた | [07](07-stage4.md) |
| `docs/design/01-agent/01-04-context.md`、`01-05-sessions.md`、`02-providers/02-01-provider-layer.md`、`03-safety/03-01-security-model.md`、`05-interface/05-01-cli.md`、`05-02-tui.md`、`open-issues.md` | 変更 | 段 4 の各項目を書き、途中のモデルの切り替えの規則を改め、BH-120 と BH-121 を加えた | [07](07-stage4.md) |
| `crates/benitoiteh/src/`（道具、提供者の引数の型、圧縮、セッション、文脈、Agent、設定、画面）と `prompts/tools/edit-multiple.md`（追加） | 変更 | 段 2 の 1〜3 の評価用の切り替え（複数の置き換え、道具の結果の刈り込み、下のディレクトリの指示のファイル） | [08](08-stage2.md) |
| `crates/benitoiteh/tests/`（書く道具、提供者、設定、exec、ループ、セッション、文脈）、`tests/expected/`、`examples/update_context_expected.rs` | 変更 | 段 2 のテストを加え、複数の置き換えの形の文脈の期待値を加えた | [08](08-stage2.md) |
| `docs/design/01-agent/01-03-tools.md`、`01-04-context.md`、`01-05-sessions.md`、`02-providers/02-01-provider-layer.md`、`03-safety/03-01-security-model.md`、`05-interface/05-01-cli.md`、`open-issues.md` | 変更 | 段 2 の三つの切り替えを書き、BH-122 と BH-123 を加えた | [08](08-stage2.md) |
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
- 本物の提供者での、利用者の発言の中の画像（BH-120）と出力の型（BH-121）と `edit` の入れ子の Schema（BH-122）の確かめ。本物の LLM を呼ぶので、設計者の了承を得てから行う。
- 段 2 の四つの評価用の切り替え（計画の道具を含む）の効果の比べ（BH-123、BH-008）。本物の LLM を呼ぶので、課題・モデル・回数・費用を示して設計者の了承を得てから行う。
- 段 2 の 5（スクリプトから道具を呼ぶエフェクト。処理系の側の変更を待つ）。
- macOS で確かめる項目に加えたもの: 通信の中継（BH-119）、 主のプロセスの環境と記憶をほかのプロセスから読めるか（BH-118）、Seatbelt で `.agents` が守られること、制限モードのプロジェクトの秘密の名前をどの深さでも塞ぐ正規表現の規則にするか、`PATH` の判定のデバイスと inode の比べ、`~/Library/Keychains` を読ませないことの影響。
- 原因を確かめていない失敗 2 件（`tests/exec.rs::closed_event_output_still_finishes_the_session_record`、`tests/tui/commands.rs` の `accepted_rewind_finishes_before_quit_and_repeated_keyboard_interrupts`）。[03](03-other-test-files.md)の「ほかの失敗」に記した。
- 評価の道具の Python のテストのうち、macOS の判定を要しない 4 件の失敗の原因（[01](01-cloud-linux-check.md)の「結果」）。
