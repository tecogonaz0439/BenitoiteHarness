# 2026-11-02〜03 の作業のまとめ

本ディレクトリは、2026-11-02 から 2026-11-03 に、Claude Code のクラウドのセッション（使い捨ての x86_64 の Linux）で行った作業と、その結果をまとめる。作業はオーケストレータ（Claude Code）が、設計者の指示を受けて行った。ブランチは `claude/stoic-ride-otqoj5` であり、`gamou` にはまだ取り込んでいない。

この README は、作業の一覧（何を、どの順に行ったか）と、残っていること（これから行うこと、判断や了承を待つこと）の入口である。各作業の詳しい経過、決定、確かめは、一覧の「記録」の文書に書く。

## 作業の条件

設計者が次の条件を決めた。

- macOS が要る項目は、Linux での確かめだけを行う。macOS での確かめは別途とする。
- 本物の LLM を呼ぶ項目は行わない。テストは模擬の提供者だけを使う。
- 時々失敗するテストは、テストの側で直し、本番のコードを変えない。
- 段 1（[05](05-stage1.md)）からは、キャッチアップの案（[04](04-oss-feature-review.md)）に従って本番のコードを変える。
- 問い合わせが要る選択は、推奨の方法で進める（段 2 から）。選んだ形は、各記録と設計書に【方針】として書く。
- push は作業用のブランチ `claude/stoic-ride-otqoj5` にだけ行う。

## 作業の一覧

行った順に並べる。

| # | 日付 | 作業 | 結果 | 記録 | コミット |
|---|---|---|---|---|---|
| 1 | 11-02 | クラウドの Linux で検査を走らせる環境を作り、全体の検査を走らせた | Alpine の rootfs と非 root の利用者で、全体のテストが通る環境を作った | [01](01-cloud-linux-check.md) | `22e83e8`、`c00ec83` |
| 2 | 11-02 | `tests/session.rs` の時々失敗するテストを調べて直した | 子プロセスを起こすテストと錠に依るテストを重ねない形にした | [02](02-session-flaky-tests.md) | `2e23f8d` |
| 3 | 11-02 | ほかのテストのファイルに同じ競合がないかを調べた | 2 件を直した。原因を確かめていない失敗が 2 件残る | [03](03-other-test-files.md) | `197dee2`、`0cfacd0`、`bb0c9e2` |
| 4 | 11-02 | 記録を本ディレクトリへ移した | — | 本書 | `599d873`、`688bfb8` |
| 5 | 11-02 | ほかのハーネスとサンドボックスの OSS と機能を比べ、キャッチアップの案（段 1〜5）を作った | 案を作った | [04](04-oss-feature-review.md) | `e1b6a0b` |
| 6 | 11-02 | 段 1: 安全の穴を塞ぐ | 7 項目を済ませた（判断待ちの 3 件は設計者が決めた） | [05](05-stage1.md) | `c386ebf`〜`1732c4e` |
| 7 | 11-02 | 段 3: 通信の制限 | 通常モードの中継を作った。制限モードは設計と処理系への要望まで | [06](06-stage3.md) | `3f04528`〜`cfb9fba` |
| 8 | 11-02〜03 | 段 4: 使い勝手 | 全項目を作った（画像の添付と出力の型を含む） | [07](07-stage4.md) | `60ed18c`〜`5c5b091` |
| 9 | 11-03 | 段 2 の 1〜3: 複数の置き換え、道具の結果の刈り込み、下のディレクトリの指示のファイル | 既定で切った評価用の切り替えとして作った。評価はまだ | [08](08-stage2.md) | `53b95c1`〜`333a203` |
| 10 | 11-03 | BH-011（完了の前の確かめ）を閉じた | `agent.completion_check` を既定 `false` のまま残し、段 2 の 4 の確かめの促しは加えない | [08](08-stage2.md) | `237306c` |
| 11 | 11-03 | 段 2 の 4: 計画を書く道具 `update_plan` | 既定で切った評価用の切り替えとして作った。評価はまだ | [08](08-stage2.md) | `32c0fee` |
| 12 | 11-03 | 本書の見直し（作業の一覧と残っていることの整理） | — | 本書 | `42374a0`、`d97df39` |
| 13 | 11-03 | 時々失敗するテスト 2 件の原因を確かめて直した | 負荷をかけた再現で原因を確かめ、テストの側で直した | [09](09-linux-followups.md) | `cd43ecc` |
| 14 | 11-03 | 評価の道具の Python のテストの 4 件の失敗を調べた | 1 件は Python の版の違いで、テストを直した。3 件は macOS を前提とする | [09](09-linux-followups.md) | `1680f63` |
| 15 | 11-03 | 端末を通した通しの試し | 確認、通知、外部のエディタを擬似端末で通すテストを加えた | [09](09-linux-followups.md) | `ef037fe` |
| 16 | 11-03 | 04 の未確認のます目を確かめた | 11 のます目に判定と出典を書き、`benitoiteh` の列を改めた | [09](09-linux-followups.md) | `413a491` |
| 17 | 11-03 | 記録の更新 | — | 本書、[09](09-linux-followups.md) | `30c87c9` |
| 18 | 11-03 | Benitoite の処理系への要望をまとめた | R1〜R5。v0.0.12 で R2 の不具合を再現し、R3 の検査の時間を測った | [10](10-benitoite-requests.md) | `26202b0` |
| 19 | 11-03 | BH-003 を閉じ、`gamou` への取り込みを保留とした | `read` の行の番号は既定（付ける）のまま。BH-080 に再構成の検討の手がかりを書いた | 本書、設計書 | `92125e5` |

キャッチアップの案の段ごとの状態は次のとおりである。

| 段 | 状態 |
|---|---|
| 段 1: 安全の穴を塞ぐ | 済み（macOS の主のプロセスは BH-118 で実機を待つ） |
| 段 2: エージェントの性能 | 1〜4 は評価用の切り替えとして作った（4 の確かめの促しは BH-011 を閉じたので加えない）。評価（BH-123）は了承待ち。5 は処理系の側を待つ |
| 段 3: 通信の制限 | 通常モードは済み。制限モードは処理系がプロキシに従うのを待つ（要望の R1）。3 の macOS の拒否の記録から理由を返すことは未着手 |
| 段 4: 使い勝手 | 済み |
| 段 5: 連携 | 未着手。試作の後の判断（BH-080）とあわせて決める |

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
| [08-stage2.md](08-stage2.md) | キャッチアップの案の段 2（エージェントの性能）の 1〜4 を、評価用の切り替えとして作った記録と、BH-011 を閉じた決定（2026-11-03） |
| [09-linux-followups.md](09-linux-followups.md) | Linux で進められる残件（時々失敗するテスト、Python のテストの失敗、04 の未確認、端末の通しの試し）の記録（2026-11-03） |
| [10-benitoite-requests.md](10-benitoite-requests.md) | Benitoite の処理系への要望（R1〜R5）。処理系の側のエージェントが単独で読める形にまとめた（2026-11-03） |

## 残っていること

### 設計者の了承や判断が要るもの

| 事項 | 内容 | 関わる番号 |
|---|---|---|
| 本物の提供者での確かめ | 利用者の発言の中の画像、`exec --output-schema` の出力の型、`edit` の `edits` の入れ子の Schema を、三つの口の本物の提供者が受け付けるか。本物の LLM を呼ぶので、提供者・モデル・回数・費用を示して了承を得てから行う | BH-120、BH-121、BH-122 |
| 段 2 の評価 | 四つの評価用の切り替え（`edit.multiple`、`compaction.prune_tool_results`、`agent.nested_instructions`、`agent.plan_tool`）を、既定の形と同じ課題・同じモデルで比べ、採るかを決める。課題と条件はまだ決めていない | BH-123、BH-008 |
| 処理系への要望（R1〜R5） | プロキシへの対応（R1）、表示のパスの不具合（R2）、取り込みの根の指定（R3）、秘密の入力（R4）、道具を呼ぶエフェクトの相談（R5）を、[10](10-benitoite-requests.md)にまとめた。設計者は次の次の版での実現を検討している。まだ依頼していない | BH-040、BH-029、BH-028、BH-105、BH-002 |
| `gamou` への取り込み | 作業用のブランチを `gamou` に `git merge --no-ff` で取り込む。2026-11-03 に、設計者が待つよう指示した | — |
| 段 5: 連携 | MCP、ACP、RPC、OS のキーストア、偽のトークンを差し替えるプロキシ。規模と実行ファイルの大きさへの影響が大きいので、試作の後の判断とあわせて決める | BH-116、BH-033、BH-080 |
| 試作の後の判断（BH-080） | 処理系にマージするか、独立したままとするか。設計者の記憶では、ディレクトリとリポジトリの再構成を別に検討しており、Benitoite の非公開の docs/todo にあるかもしれない（このリポジトリと処理系の公開の文書には見当たらなかった） | BH-080 |

### ほかを待つもの

- 段 2 の 5（Benitoite のスクリプトから読む道具を呼ぶエフェクト）。処理系の側と形を詰める（BH-002）。
- 段 3 の制限モード。処理系 `v0.0.12` の HTTP のクライアントはプロキシを使わない（[06](06-stage3.md)の「設計者の決定」）。処理系への要望の R1（[10](10-benitoite-requests.md)）が入るまで今のままとする。

### まだ行っていない作業

- 段 3 の 3 のうち、macOS の方針の拒否の記録から拒否の理由を道具の結果に返すこと（[06](06-stage3.md)の「限り」）。macOS の実機で確かめる。

04 の未確認のます目と、端末を通した通しの試し（擬似端末まで）は、2026-11-03 に済ませた（[09](09-linux-followups.md)）。

### macOS と aarch64 の Linux での確かめ

`aarch64-apple-darwin` は、Linux からのコンパイルと lint だけを確かめた。macOS で確かめる項目は次のとおりである。

- 全体のテスト。macOS だけで走る `pinned_temporary_directory_uses_an_alternate_name_on_resume` も、`Fixture::spawning()` に変えた（[02](02-session-flaky-tests.md)）。
- 通信の中継（BH-119。[06](06-stage3.md)）。
- 主のプロセスの環境と記憶をほかのプロセスから読めるか（BH-118。[05](05-stage1.md)）。
- Seatbelt で `.agents` が守られること、`PATH` の判定のデバイスと inode の比べ、`~/Library/Keychains` を読ませないことの影響（[05](05-stage1.md)）。
- 制限モードのプロジェクトの秘密の名前を、どの深さでも塞ぐ正規表現の規則にするか（[05](05-stage1.md)）。
- 段 4 の通知と外部のエディタの、macOS の端末での動きと、実際の端末のエミュレータでの OSC 9 の表示（[07](07-stage4.md)、[09](09-linux-followups.md)）。
- 評価の道具の Python のテストのうち、macOS を前提とするもの（判定のサンドボックスを要する 8 件と、`test_workspace_processes_stop_before_snapshot_and_judgment`）（[09](09-linux-followups.md)）。

aarch64 の Linux（Apple の `container`）での実行も、まだ行っていない。

### 原因を確かめていない失敗

ない。時々失敗するテスト 2 件と、評価の道具の Python のテストの 4 件の失敗は、2026-11-03 に原因を確かめた（[09](09-linux-followups.md)）。前者は 2 件とも、後者は 1 件をテストの側で直し、残りの 3 件は macOS を前提とする（上の「macOS と aarch64 の Linux での確かめ」）。

## 確かめた検査

すべて x86_64 の Linux（Alpine 3.22.7 の rootfs、非 root）で行った。各段の確かめは、それぞれの記録に書く。

| 時点 | `cargo test --workspace --target x86_64-unknown-linux-musl` | 記録 |
|---|---|---|
| 作業の初め（本番のコードを変える前） | 754 件が通り、失敗 0、無視 16 | [01](01-cloud-linux-check.md) |
| 段 4 の後 | 805 件が通り、失敗 0、無視 16 | [07](07-stage4.md) |
| 段 2 の 1〜3 の後 | 810 件が通り、失敗 0、無視 16 | [08](08-stage2.md) |
| 段 2 の 4 の後（`32c0fee`） | 811 件が通り、失敗 0、無視 16（`work/cloud-check/stage2-4.log`） | [08](08-stage2.md) |
| Linux で進められる残件の後（最新、`413a491`） | 812 件が通り、失敗 0、無視 16（`work/cloud-check/remaining-linux.log`） | [09](09-linux-followups.md) |

lint は、各段の後に `cargo clippy --workspace --all-targets`（`x86_64-unknown-linux-musl`、`aarch64-apple-darwin`）と `cargo fmt --check` が通ることを確かめた。作業の初めには、`cargo deny check licenses advisories` と `tools/perf` の Python のテストも通った。`tests/session.rs` の直し方の確かめ（直す前は 40 回のうち 9 回失敗し、直した後は 0 回）は [02](02-session-flaky-tests.md) に書く。

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
| `crates/benitoiteh/src/`（道具、提供者の引数の型、圧縮、セッション、文脈、Agent、設定、画面）と `prompts/tools/edit-multiple.md`・`prompts/tools/update_plan.md`・`src/tools/plan.rs`（追加） | 変更 | 段 2 の 1〜4 の評価用の切り替え（複数の置き換え、道具の結果の刈り込み、下のディレクトリの指示のファイル、計画を書く道具） | [08](08-stage2.md) |
| `crates/benitoiteh/tests/`（書く道具、提供者、設定、exec、ループ、セッション、文脈）、`tests/expected/`、`examples/update_context_expected.rs` | 変更 | 段 2 のテストを加え、複数の置き換えの形と計画の道具の形の文脈の期待値を加えた | [08](08-stage2.md) |
| `docs/design/01-agent/01-03-tools.md`、`01-04-context.md`、`01-05-sessions.md`、`02-providers/02-01-provider-layer.md`、`03-safety/03-01-security-model.md`、`05-interface/05-01-cli.md`、`open-issues.md` | 変更 | 段 2 の四つの切り替えを書き、BH-122 と BH-123 を加え、BH-011 を閉じた（`01-01-agent-loop.md`、`07-quality/07-02-evaluation.md` も変えた） | [08](08-stage2.md) |
| `crates/benitoiteh/tests/exec.rs`、`tests/tui/commands.rs` | 変更 | 時々失敗するテスト 2 件を、テストの側で直した | [09](09-linux-followups.md) |
| `crates/benitoiteh/tests/tui/pty.rs` | 変更 | 確認、通知、外部のエディタを擬似端末で通すテストを加えた | [09](09-linux-followups.md) |
| `tools/skill-eval/test_skill_eval.py` | 変更 | 深い JSON のテストの入れ子を、Python の版に依らない深さにした | [09](09-linux-followups.md) |
| `docs/design/04-benitoite/04-01-benitoite-integration.md` | 変更 | 「処理系への要望」から、要望の文書（R1〜R5）を指した | [10](10-benitoite-requests.md) |
| `docs/2026-11-02/` | 追加 | 本ディレクトリ | — |
| `docs/README.md` | 変更 | 本ディレクトリへの案内を加えた | — |

段 1 より前は、本番のコード（`crates/benitoiteh/src/` のうちテストでないもの）を変えていない。結果の記録は、はじめ `docs/plan/results/2026-11-02-cloud-linux.md` に書いたが、設計者の指示で本ディレクトリへ移し、元のファイルを消した。

## コミット

`gamou` からの作業用のブランチのコミットを、作業の一覧の順に並べる。

| 作業 | コミット | 内容 |
|---|---|---|
| 1 | `22e83e8` | クラウドの Linux でテストを走らせるスクリプトを追加 |
| 1 | `c00ec83` | クラウドの Linux での検査と、時々失敗するセッションの後始末のテストを記録 |
| 2 | `2e23f8d` | 子プロセスを起こすセッションのテストを、錠に依るテストと重ねない |
| 3 | `197dee2` | ほかのテストのファイルの同じ flock の競合を記録 |
| 3 | `0cfacd0` | 継承された錠を短時間待つ形を、さらに 2 件のテストに加える |
| 3 | `bb0c9e2` | ほかのテストのファイルの節の古い注記を直す |
| 4 | `599d873` | 2026-11-02 の作業を本ディレクトリに集める |
| 4 | `688bfb8` | まとめから取り込みの判断を除く |
| 5 | `e1b6a0b` | ほかのハーネスとサンドボックスの機能を比べ、キャッチアップの案を作る |
| 6 | `c386ebf` | Linux のサンドボックスで、入れ子の守る場所の祖先を留める |
| 6 | `3334df0` | 子プロセスの環境から秘密に見える変数を除く |
| 6 | `98b0b62` | HOME の下の資格情報、キーチェーン、履歴、ブラウザの場所を読ませない |
| 6 | `833119f` | 外で動かす `bash` の `PATH` から、書ける場所に届く項目を除く |
| 6 | `a6fdda8` | Linux で主のプロセスをダンプできない形にする |
| 6 | `f8068df` | `.agents` を守り、一回ごとの確認の後にだけ書けるようにする |
| 6 | `6e8df47` | プロジェクトの秘密、シェルの起動の設定、BH-118 の設計者の決定を当てる |
| 6 | `1732c4e` | プロジェクトの Skill・プラグイン・`model` を、信頼の確認の後にだけ使う |
| 7 | `3f04528` | ドメインを確かめる HTTP のプロキシの核を加える |
| 7 | `333c4d6` | Linux で、サンドボックスの通信を中継に通す |
| 7 | `9dfc767` | `normal.network` の `proxy` の設定を加え、`bash` を中継に通す |
| 7 | `cfb9fba` | 通常モードの中継と、制限モードの方針を設計書に書く |
| 8 | `60ed18c` | 記録を残さない `exec --ephemeral` |
| 8 | `00b2891` | 最初の要求を送らずに示す `debug prompt-input` |
| 8 | `05c4f05` | 利用者の定義するコマンドと `/init` |
| 8 | `d61a204` | 端末が前面にないときの通知 |
| 8 | `f2f2b7e` | 今の枝の会話の Markdown への書き出し |
| 8 | `b3c904e` | 外部のエディタと `@` のパスの補い |
| 8 | `6bd00f8` | 端末の準備と復元のバイト列のテストの期待値に、前面の報告を加える |
| 8 | `17176b3` | 貼った画像のパスの添付 |
| 8 | `2061be4` | 同じ提供者の中でモデルと推論の度合いを替える `/model` |
| 8 | `2c68e2f` | 最後の応答の形を JSON Schema で決める `exec --output-schema` |
| 8 | `5c5b091` | 段 4 の記録 |
| 9 | `53b95c1` | 複数の置き換えを受ける `edit` |
| 9 | `5d56185` | 圧縮の前の道具の結果の刈り込み |
| 9 | `a9ac200` | 下のディレクトリの指示のファイルを道具の結果に添える |
| 9 | `333a203` | 段 2 の評価用の切り替えの記録 |
| 10 | `237306c` | BH-011 を閉じ、完了の前の確かめを既定で切ったままにする |
| 11 | `32c0fee` | 計画を書く道具 `update_plan` |
| 12 | `42374a0` | まとめを、作業の一覧と残っていることを中心に組み直す |
| 12 | `d97df39` | 作業の一覧に、まとめのコミットを書く |
| 13 | `cd43ecc` | 時々失敗するテスト 2 件を、テストの側で直す |
| 14 | `1680f63` | 深い JSON のテストを Python の版に依らないようにする |
| 15 | `ef037fe` | 確認、通知、外部のエディタを、本物の端末で通して確かめる |
| 16 | `413a491` | 機能の比べの未確認のます目を確かめる |
| 17 | `30c87c9` | Linux で進められる残件を記録する |
| 18 | `26202b0` | Benitoite の処理系への要望を、単独で読める文書にする |
| 19 | `92125e5` | BH-003 を閉じ、gamou への取り込みを保留とし、BH-080 に手がかりを書く |
