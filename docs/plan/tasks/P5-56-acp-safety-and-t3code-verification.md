# P5-56 `benitoiteh acp` の安全の性質の統合テストと T3Code 実機検証（BH-055）

## 目的

本物の OS のサンドボックス（macOS の Seatbelt、Linux の bubblewrap + Landlock + seccomp）と本物の Benitoite の処理系（`v0.0.12`）を使って、`benitoiteh acp` の安全の性質（設計書 05-03、07-01「安全の性質のテスト」）を統合テストで確かめる。あわせて、実機の T3Code に `Local ACP command` として `benitoiteh acp` を登録し、`BH-055` の 4 項目を検証して記録する。

## 読む設計書

- [ACP（エディタとの連携）](../../design/05-interface/05-03-acp.md)
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)
- [テスト](../../design/07-quality/07-01-testing.md)の「安全の性質のテスト」、スキル `test-audit`
- [未決事項](../../design/open-issues.md)（`BH-054`〜`BH-057`）
- [P5-55](P5-55-acp-protocol-and-approver.md)、既存の `crates/benitoiteh/tests/{sandbox_exec_restricted.rs,sandbox_benitoite_run.rs,sandbox_bash.rs}`

## この作業が認める型の変更

なし。

## 作るもの

1. **本物のサンドボックスと処理系による ACP 安全統合テスト**（`crates/benitoiteh/tests/sandbox_acp.rs`）
   - `benitoiteh acp` を子プロセスとして本物のパイプ（`stdin`/`stdout`/`stderr`）で起動し、模擬の提供者と本物の処理系・本物の OS のサンドボックスで次を確かめる（ファイル名を `sandbox_acp.rs` とすることで、`scripts/check.sh` の `--inside-codex` と外の実行の区分にそのまま従う）。
     - **MCP 資格情報の隔離**: `benitoiteh acp` の起動環境に `T3_ACP_MCP_ENDPOINT`・`T3_ACP_MCP_AUTHORIZATION`・`T3_ACP_MCP_NODE`・`T3CODE_THREAD_ID` の囮の値を置き、制限モードの `benitoite_run`（`Process.Environment` を確認）と通常モードの `bash` で環境変数を出力させても、囮の変数名も値も一切現れないこと。また、`session/new` の `mcpServers` に囮のトークンを含む設定を渡しても、標準出力・標準エラー出力・`session.jsonl` のどこにもそのトークンが現れないこと。
     - **制限モードのエフェクト確認とサンドボックス**: `benitoiteh acp --mode restricted` で `File.Write` を持つスクリプトを実行したとき、`session/request_permission` が届き、`reject-once` で実行が阻止されてファイルが作られないこと、`allow-once` で実行されて 2 回目の実行では再び `session/request_permission` が届くこと、`/bin/sh` を起動しようとするスクリプトは `allow-once` で許可しても OS のサンドボックスに拒否されること。
     - **通常モードの通信の中継と終了競合**: `benitoiteh acp --mode normal`（`network = "proxy"`）で未許可ドメインへ接続する `bash` コマンドを実行したとき、`session/request_permission` が届き、コマンドが終了・取り消された後に遅れて `allow-session` を返しても次のコマンドでそのドメインが通らないこと。
     - **取り消しと EOF による子プロセスの後始末**: `benitoiteh acp` で長いコマンドやスクリプトを実行中に `session/cancel` または `stdin` の EOF を送ったとき、子プロセスが止まり、`stopReason: "cancelled"`（またはプロセス終了）となって子孫のプロセスが残らないこと。
2. **Linux の aarch64 コンテナでのスキップ一覧の更新**（`scripts/check.sh`）
   - `scripts/check.sh` の `linux` の段で、Landlock を持たない Apple の `container`（aarch64）で飛ばす `sandbox_*.rs` の一覧に `sandbox_acp.rs` を加え、Landlock を持つ x86_64 の Linux（`BENITOITEH_X86_64_HOST`）と macOS で走らせる。
3. **T3Code 実機検証と記録**（`docs/plan/results/P5-56-acp.md`、`docs/design/open-issues.md`）
   - オーケストレータ（または設計者）が、実機の T3Code の `Settings > Providers > Add provider > Show more > Local ACP command` に `benitoiteh acp` を登録し、`BH-055` の 4 項目を確かめて `docs/plan/results/P5-56-acp.md` に記録する（本物の LLM を呼ぶ場合は、事前に提供者・モデル・回数・見込みの費用を設計者に示して了承を得る）。

## 触るファイル

- `crates/benitoiteh/tests/sandbox_acp.rs`（新）
- `scripts/check.sh`
- `docs/plan/results/P5-56-acp.md`（新、実機検証の記録）
- `docs/design/05-interface/05-03-acp.md`、`docs/design/open-issues.md`（検証完了後の更新）

## テスト

`test-audit` の作成時の関門を通す。

- `crates/benitoiteh/tests/sandbox_acp.rs` で、本物の処理系（`v0.0.12`）と本物の OS のサンドボックス（macOS の Seatbelt、x86_64 Linux の bubblewrap + Landlock + seccomp）を通して、上記の 4 つの安全契約を確かめる。

## 確かめること

- macOS と x86_64 Linux（Landlock 有効）で `tests/sandbox_acp.rs` が通り、実機の T3Code で `BH-055` の 4 項目（版 1 への交渉、承認カードの全事実表示と Supervised 設定での非自動承認、`session/new`・`session/load`・`configOptions`、`session/cancel` と EOF の後始末）が成立することを確かめて `docs/plan/results/P5-56-acp.md` に記録する。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが外で `scripts/check.sh` の全体（macOS、aarch64 Linux、`BENITOITEH_X86_64_HOST` の x86_64 Linux）を通す。
- `docs/plan/results/P5-56-acp.md` に実機検証と安全テストの結果を記録する。
