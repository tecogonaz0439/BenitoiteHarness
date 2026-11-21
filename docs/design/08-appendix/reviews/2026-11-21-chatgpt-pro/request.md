# ChatGPT Pro へのレビュー依頼の文面（ACP 対応）

関数型スクリプト言語 Benitoite 専用のエージェントハーネスの試作（実行ファイル名は仮に `benitoiteh`）において、T3Code（`pingdotgg/t3code`）などの統合画面・エディタと連携するため、**ACP（Agent Client Protocol）** に対応するサブコマンド `benitoiteh acp` の設計書の草稿を作りました。実装プランと実装に進む前に、この設計のレビューをお願いします。

### レビューの対象

- リポジトリ: https://github.com/tecogonaz0439/BenitoiteHarness （公開）
- 対象のブランチ: `design/acp-support`
- 対象のコミット: `f04e5d9c2985c84da747491e17b14c548bef252e`
  （ブラウズするときは https://github.com/tecogonaz0439/BenitoiteHarness/tree/f04e5d9c2985c84da747491e17b14c548bef252e を使ってください）
- 新しく追加した章:
  - `docs/design/05-interface/05-03-acp.md`（ACP（エディタとの連携））
- 合わせて更新した章と未決事項:
  - `docs/design/open-issues.md`（`BH-054`〜`BH-057` を追加）
  - `docs/design/README.md`（目次）
  - `docs/design/00-overview/00-01-goals.md`、`00-02-architecture.md`、`00-04-glossary.md`
  - `docs/design/03-safety/03-03-approval.md`
  - `docs/design/05-interface/05-01-cli.md`
  - `docs/design/06-impl/06-01-crate-structure.md`
  - `docs/design/07-quality/07-01-testing.md`
- 関連する既存の章と実装:
  - `AGENTS.md`（プロジェクトの規則と「レビューの観点」）
  - `docs/design/03-safety/03-01-security-model.md`、`03-02-sandbox.md`、`03-04-secrets.md`
  - `docs/design/01-agent/01-01-agent-loop.md`、`01-05-sessions.md`
  - `crates/benitoiteh/src/agent/mod.rs`、`src/approval/mod.rs`、`src/cli/exec.rs`、`src/ui/tui/worker.rs`

### 背景

- `benitoiteh` は P5（評価とキャッチアップの案の段 1〜4）まで実装が進んでおり、制限モード（Benitoite 以外のスクリプト言語を起動させない）と通常モード、OS のサンドボックス（macOS の Seatbelt、Linux の bubblewrap + Landlock + seccomp）、実行の前の確認、セッションの木と巻き戻し、TUI と非対話（`exec`）が動いています。
- 今回、T3Code（https://github.com/pingdotgg/t3code ）の `Local ACP command` として `benitoiteh` を登録して動かせるよう、標準入出力の JSON-RPC 2.0（NDJSON）で話す `benitoiteh acp` を加えたいと考えています。
- `benitoiteh` は設計書 06-01 のとおり、非同期の実行時（`tokio` など）を使わず OS のスレッドと `std::sync::mpsc` で組んでおり、外部の依存クレートを最小限に保っています。そのため、公式の `agent-client-protocol` クレート（`tokio` に依存）は使わず、`serde` と `serde_json` の上で ACP のメッセージを自前で扱う方針です。

### 特に見てほしい点

優先度の高い順に挙げます。

1. **安全の性質と信頼の境界**（`05-03-acp.md`・`03-01`・`03-02`・`03-03`）
   - ACP を通すことで、LLM の出力やプロジェクト内の信頼しない内容だけで、次のことができる道筋が生じていないか確認してください。
     - 実行の前の確認（`session/request_permission`）を飛ばす、または一回限りの確認（`escalate`、`.agents` への書き込み、`read` の開かないファイルを取り込む実行、`create_project`、範囲外の書き込み）をセッション全体の許可に広げる。
     - クライアント側の `fs/*` や `terminal/*`、`mcpServers` を通じて、`benitoiteh` の `safe_fs`（読まないファイル、`.bnt` の制限、ハードリンクの検査）や OS のサンドボックスの外に出る。
     - セッションの途中で制限モードから通常モードへ切り替える。
     - LLM が書いた `justification` で、`benitoiteh` が確かめた確認の事実（`ApprovalFacts`）を偽装する。
2. **ACP と T3Code の仕様との適合**（`05-03-acp.md`、`BH-055`）
   - ACP（https://agentclientprotocol.com/ 、https://github.com/agentclientprotocol/agent-client-protocol ）の版 1（および版 2 preview）と、T3Code（https://github.com/pingdotgg/t3code の `packages/provider-acp/` や `AcpSessionRuntime.ts`）が期待する JSON-RPC のメソッド名・引数の欄・応答の欄・`session/update` の `sessionUpdate` の種類・`session/request_permission` の要求と応答の形に、食い違いや不足がないかを一次資料に基づいて確認してください。
   - 特に、`session/new`・`session/load` の返り値、`session/set_config_option`（または `session/set_mode`）、`session/cancel` が通知（`id` なし）か要求（`id` あり）か、`session/request_permission` の `toolCall` と `options` / `outcome` の正確な JSON の形について、一次資料で確認できる事実があれば出典つきで教えてください。
3. **未決事項（`BH-054`〜`BH-057`）の判断**
   - `BH-054`: `session/new`・`session/load` の時点での確認（プロジェクトの信頼、制限モードでの処理系の SHA-256 の変化、モードを変えての再開）を、案 A（非対話 `exec` と同じく保存済みの決定と `--trust-project` に従い、処理系の変化とモード変更は拒む）とするか、案 B（最初の `session/prompt` の直前に `session/request_permission` で尋ねる）とするか。
   - `BH-056`: 1 プロセスにつき同時に持つアクティブなセッションを 1 つに限る形（案 A）で、T3Code のスレッド管理・切り替えと問題なく動くか、それとも 1 プロセス内で複数の `sessionId` を並行に保持できる必要があるか。
   - `BH-057`: 通常モードの通信の中継（`normal.network = "proxy"`）のドメイン確認を `session/request_permission` に流し、`secrets.askpass` を無効とする扱いが妥当か。
4. **スレッド構成・終了処理・実装の十分さ**（`05-03-acp.md`・`06-01`）
   - 標準入力を読むスレッド、主のスレッド、エージェントのスレッド（および `AcpApprover`）の間で、確認の待ち状態のときに `session/cancel` や EOF が届いた場合のデッドロックや競合、子プロセスの後始末の漏れが起きないかを見てください。
   - 既存の `crates/benitoiteh/src/` の作り（`Agent`、`Approver`、`exec::prepare`、`Worker`）に照らして、実装プランを作るのに十分な粒度で書かれているかを見てください。

### 報告の形

- 日本語でお願いします。
- 指摘は、重いものから順に番号を付けて並べてください。各指摘には次のことを書いてください。
  - 場所（ファイルのパスと見出し）
  - 問題点
  - 重さ（実装の前に直すべき／実装の中で直せばよい／好みの範囲）
  - 修正案
- 外部の事実（ACP のスキーマ、T3Code の実装、JSON-RPC の仕様など）を挙げるときは、出典の URL（リポジトリのファイルパスやコミットを含む）を添えてください。確かめられなかった事実は、そう明記してください。
- 未決事項（`BH-054`〜`BH-057`）について意見があれば、番号を添えてください。
- 最後に、全体の所見（実装プランの作成に進めるか、先に決めるべき点は何か）を短くまとめてください。
