# P5-56 `benitoiteh acp` の安全の性質の統合テストと T3Code 実機検証（BH-055）

- 状態: 未着手
- 依存: P5-55

## 目的

本物の OS のサンドボックス（macOS の Seatbelt、Linux の bubblewrap + Landlock + seccomp）と本物の Benitoite の処理系（`v0.0.12`）を使って、`benitoiteh acp` の安全の性質（設計書 05-03、07-01「安全の性質のテスト」）を統合テストで確かめる。あわせて、実機の T3Code に `Local ACP command` として `benitoiteh acp` を登録し、`BH-055` の 4 項目を検証して記録する。

## 読む文書

- `AGENTS.md`、[作業の進め方](../00-common.md)、[作業の間の約束](../10-interfaces.md)。
- 設計書 05-03「ACP（エディタとの連携）」、03-01「セキュリティモデル」、03-02「OS のサンドボックス」、07-01「安全の性質のテスト」、`open-issues.md`（`BH-054`〜`BH-057`）。
- `.claude/skills/test-audit/SKILL.md`。

## 作るもの

1. **本物のサンドボックスと処理系による ACP 安全統合テスト**（`crates/benitoiteh/tests/sandbox_acp.rs`）
   - `benitoiteh acp` を子プロセスとして本物のパイプ（`stdin`/`stdout`/`stderr`）で起動し、模擬の提供者と本物の処理系・本物の OS のサンドボックスで次を確かめる（ファイル名を `sandbox_acp.rs` とすることで、`scripts/check.sh` の `--inside-codex` と外の実行の区分にそのまま従う）。
     - **MCP 資格情報の隔離**: `benitoiteh acp` の起動環境に `T3_ACP_MCP_ENDPOINT`・`T3_ACP_MCP_AUTHORIZATION`・`T3_ACP_MCP_NODE`・`T3CODE_THREAD_ID` の囮の値を置き、制限モードの `benitoite_run`（`Process.Environment` を確認）と通常モードの `bash` で環境変数を出力させても、囮の変数名も値も一切現れないこと。また、`session/new` の `mcpServers` に囮のトークンを含む設定を渡しても、標準出力・標準エラー出力・`session.jsonl` のどこにもそのトークンが現れないこと。
     - **制限モードのエフェクト確認とサンドボックス**: `benitoiteh acp --mode restricted` で `File.Write` を持つスクリプトを実行したとき、`session/request_permission` が届き、`reject-once` で実行が阻止されてファイルが作られないこと、`allow-once` で実行されて 2 回目の実行では再び `session/request_permission` が届くこと、`/bin/sh` を起動しようとするスクリプトは `allow-once` で許可しても OS のサンドボックスに拒否されること。
     - **通常モードの通信の中継と終了競合**: `benitoiteh acp --mode normal`（`network = "proxy"`）で未許可ドメインへ接続する `bash` コマンドを実行したとき、`session/request_permission` が届き、コマンドが終了・取り消された後に遅れて `allow-session` を返しても次のコマンドでそのドメインが通らないこと。
     - **取り消しと EOF による子プロセスの後始末**: `benitoiteh acp` で長いコマンドやスクリプトを実行中に `session/cancel` または `stdin` の EOF を送ったとき、子プロセスが止まり、`stopReason: "cancelled"`（またはプロセス終了）となって子孫のプロセスが残らないこと。
2. **T3Code 実機検証と記録**（`docs/plan/results/P5-56-acp.md`、`docs/design/open-issues.md`）
   - オーケストレータ（または設計者）が、実機の T3Code の `Settings > Providers > Add provider > Show more > Local ACP command` に `benitoiteh acp` を登録し、`BH-055` の 4 項目を確かめて `docs/plan/results/P5-56-acp.md` に記録する（本物の LLM を呼ぶ場合は、事前に提供者・モデル・回数・見込みの費用を設計者に示して了承を得る）。
     1. T3Code の `protocolVersion: 2` の初期化要求から `1` へ交渉して接続できること。
     2. 制限モードと通常モードの確認要求で、T3Code の承認カードに `benitoiteh` の全事実（対象、エフェクト、サンドボックス範囲、`.agents` 警告、コマンド全文）が欠けずに表示され、Supervised 設定で自動承認されず、`Approve`（1 回限り）と `Always allow this session`（セッション記憶、一回限りの要求では 1 回限りとして動作）と `Decline` が意図どおりに働くこと。
     3. 新規スレッド（`session/new`）、既存スレッドの再開（`session/load`）と履歴表示、モデルと `effort` の切り替え（`session/set_config_option`）が動くこと。
     4. 実行中のキャンセル（`session/cancel`）とスレッド終了・再接続で、処理と子プロセスが残らないこと。
   - 確かめた結果を受けて、設計書の `05-03-acp.md` と `open-issues.md` の `BH-055`（および設計者の承認を得た `BH-054`・`BH-056`・`BH-057`）を更新する。

## 完了の条件

- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが外で `scripts/check.sh` の全体（macOS、aarch64 Linux、`BENITOITEH_X86_64_HOST` の x86_64 Linux）を通す。
- `docs/plan/results/P5-56-acp.md` に検証結果を記録する。
