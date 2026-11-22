# P5-56 `benitoiteh acp` の安全の性質の統合テストと実機検証の記録

## 実施日と環境

- 実施日: 2026-11-22
- 対象ブランチ: `impl/P5-56-acp-safety-and-t3code-verification`（P5-55 取り込み済みの `gamou` `5ee3d4f` の上）
- ホスト環境: `aarch64-apple-darwin`（macOS、Seatbelt 有効）
- 処理系: `benitoite` `v0.0.12`
  - `aarch64-apple-darwin`: sha256 `6b26491a8014d9ca88e92caa93c2fee8cc8201098331212071dfdcced8d47e7d`
  - `aarch64-unknown-linux-musl`: コンテナ内で非 Landlock テストを実行

## 1. 安全の性質の統合テスト（`crates/benitoiteh/tests/sandbox_acp.rs`）

本物の `benitoiteh acp` 子プロセス（本物の `stdin`/`stdout`/`stderr` パイプ）、模擬提供者（`MockProvider`）、本物の処理系（`v0.0.12`）、本物の OS サンドボックス（macOS Seatbelt）を通して、次の 4 テストすべてが合格した（1.44s）。

| テスト関数 | 確かめた安全契約 | macOS（Seatbelt） | x86_64 Linux（Landlock） |
|---|---|---|---|
| `acp_isolates_mcp_env_and_redacts_mcp_servers` | 起動環境の `T3_ACP_MCP_*`・`T3CODE_*` が制限モード `benitoite_run` と通常モード `bash` の子プロセス環境へ渡らないこと（対照の通常変数は届くこと）、および `session/new` の `mcpServers` の資格情報・URL・コマンド・`env` が `stdout`・`stderr`・`session.jsonl`・提供者要求へ漏れず `stderr` に無視件数のみが出ること | 合格 | ビルド・リンクのみ確認（未実行） |
| `acp_restricted_effect_approval_once_and_sandbox_denies_shell` | 制限モードで `File.Write` スクリプトへの `reject-once` がファイル作成を阻止し、`allow-once` は 1 回目のみ実行して 2 回目に再確認となり、`/bin/sh` を起動するスクリプトは `allow-once` で許可しても OS サンドボックスに拒否されること | 合格 | ビルド・リンクのみ確認（未実行） |
| `acp_normal_proxy_late_session_approval_does_not_allow_next_command` | 通常モード `network = "proxy"` で未許可ドメイン接続に `kind: "fetch"` の確認が上がり、コマンド自然終了後および `session/cancel` 後に遅れて `allow-session` を返しても次のコマンドでセッション許可されず再確認となること | 合格 | ビルド・リンクのみ確認（未実行） |
| `acp_cancel_and_eof_stop_child_processes_without_leftovers` | 長時間動く通常モード `bash`（子 `sh` と孫 `sleep`）および制限モード `benitoite_run`（`Clock.sleep`）の実行中に `session/cancel` または `stdin` EOF を送ったとき、子孫プロセスが残らず停止すること | 合格 | ビルド・リンクのみ確認（未実行） |

## 2. 全体検査（`scripts/check.sh`）とリリース実行ファイルの大きさ

- `scripts/check.sh`（`BENITOITEH_X86_64_HOST` なし）: **全段合格**（終了状態 0、`All checks passed.`）
  - `fmt`、`clippy`（ホスト + `aarch64-unknown-linux-musl` + `x86_64-unknown-linux-musl`）
  - `test`（macOS 単体・統合・`sandbox_acp.rs` を含む全 `sandbox_*.rs`）
  - `skill-eval`、`perf-tools`
  - `linux`（`aarch64-unknown-linux-musl` コンテナ全件〔Landlock 不在のため `sandbox_acp.rs` を含む `sandbox_*.rs` をスキップ〕、`x86_64-unknown-linux-musl` 全テストのビルドとリンク）
  - `deny`（`cargo deny check licenses advisories`: `advisories ok, licenses ok`）
- リリース実行ファイルの大きさ（目標 8 MiB 以下、新規外部クレート追加なし）:
  - `aarch64-apple-darwin`: **6,087,520 bytes**（5.81 MiB。8 MiB まで余裕 2.19 MiB）
  - `aarch64-unknown-linux-musl`: **6,610,304 bytes**（6.30 MiB）
  - `x86_64-unknown-linux-musl`: **7,940,208 bytes**（7.57 MiB）

## 3. 未実施・残件

1. **x86_64 Linux（Landlock + bubblewrap + seccomp）での `sandbox_acp.rs` 実実行**:
   - クロスビルドとリンクは確認済みだが、試験環境の podman が `exit 139` を返したため、設計者の判断により x86_64 Linux での実実行は別途環境復旧後に行うこととした。
2. **T3Code 実機検証（`BH-055` の 4 項目）**:
   - 手元の環境に T3Code が未導入であるため未実施。`docs/design/open-issues.md` の `BH-055` は `要検証` のまま維持する。
   - T3Code を導入・起動できる環境で、`Settings > Providers > Add provider > Show more > Local ACP command` に `target/release/benitoiteh acp`（例: 制限モードなら `benitoiteh acp --mode restricted`）を登録し、`Permission / access` を **`Supervised`** に設定して次の 4 項目を確かめた後に本書へ追記し、`BH-055` を閉じる:
     1. T3Code の `protocolVersion: 2` の初期化要求に対して `1` を返して版 1 として交渉できること。
     2. `session/request_permission` の承認カードに、`benitoiteh` が組み立てたすべての事実と選択肢が欠けずに表示され、Supervised 設定で自動承認されないこと。
     3. `session/new`・`session/load`、`configOptions`（モデルと `effort`）の切り替えと復元、履歴の再送が動くこと。
     4. `session/cancel`・EOF・再接続で処理と子プロセスが残らないこと。
