# P5-55 `benitoiteh acp` のサブコマンド、ACP 版 1 の JSON-RPC 層、`AcpApprover`、終了手順

## 目的

設計書 05-03「ACP（エディタとの連携）」に従い、`benitoiteh acp [オプション]` サブコマンド、標準入出力の JSON-RPC 2.0（ACP 版 1）の通信層、`AcpApprover` による `session/request_permission` の確認の受け渡し、および取り消し・EOF・出力障害の 5 段の終了手順を実装する。

## 読む設計書

- [ACP（エディタとの連携）](../../design/05-interface/05-03-acp.md)
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「コマンドラインの形」「オプション」「終了状態」
- [対話の画面](../../design/05-interface/05-02-tui.md)の「出力の示し方」「確認の画面」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」「制限モードの確認」「.agents への書き込み」
- [テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」「安全の性質のテスト」、スキル `test-audit`
- [P5-54](P5-54-prepare-policies-and-one-shot-effects.md)、既存の `crates/benitoiteh/src/cli/{mod.rs,args.rs,exec.rs,tui.rs}`、`src/ui/tui/{worker.rs,confirmation.rs}`、`src/approval/{mod.rs,interactive.rs}`

## この作業が認める型の変更

- `cli`: `acp` のサブコマンドのモジュール（`src/cli/acp.rs`、必要に応じて `src/ui/acp/`）を加え、`cli::run` から `"acp"` を振り分ける。
- `ui::tui::confirmation`（または `approval`）: 確認の事実（`ApprovalFacts`）から、制御文字と 8 文字以上の空白を可視化した事実の行を組み立てる処理を、`tui` と `acp` で共有できるように関数を切り出してよい。

## 作るもの

1. **CLI の入口**（`crates/benitoiteh/src/cli/mod.rs`、`src/cli/args.rs`、`src/cli/acp.rs`）
   - `benitoiteh acp [オプション]` を `cli::run` に加える。`--help` は使い方を標準出力に書いて終了状態 0 で終わる。
   - 標準出力には JSON-RPC 2.0 の行だけを書き、起動時の診断や JSON-RPC になる前の誤りは `ExecOutput::diagnostic`（制御の文字を可視化）で標準エラー出力に書く。
2. **スレッド構成と非ブロッキング出力キュー**（`crates/benitoiteh/src/cli/acp.rs` または `src/ui/acp/`）
   - **入力の読み取りのスレッド**: `stdin` から 1 行ずつ（上限 16 MiB）読み、JSON-RPC の要求（`id` と `method` がある）、通知（`method` があり `id` がない）、応答（`id` があり `method` がない）に分け、主のスレッドのチャネルへ送る。EOF に達したら主のスレッドへ EOF を知らせて終わる。
   - **出力の書き込みのスレッド**: 容量 256 行の `sync_channel` から JSON の行を受け取り、`stdout` に書いて 1 行ごとに `flush` する。書き込みまたは `flush` が失敗したら、`output_failed: Arc<AtomicBool>` を立てて主のスレッドを起こし、残りのキューを読み捨てて終わる。主のスレッドからは `try_send` で送り、`Full` または `Disconnected` のときは `output_failed` を立てて出力の障害の終了手順へ移る（主のスレッドが出力待ちで止まらないようにする）。
3. **ACP 版 1 のメソッド処理**:
   - `initialize`: `protocolVersion` が `2` でも `1` でも、`result.protocolVersion = 1`、`agentInfo: { name: "benitoiteh", version }`、`agentCapabilities: { loadSession: true, promptCapabilities: { image, embeddedContext: true } }`、`authMethods` を返す。未知の欄は無視する。
   - `authenticate`: 選んだ提供者の認証（環境変数の API キー、または `chatgpt.json` のトークン）を確かめ、通れば `{}`、失敗すれば JSON-RPC の誤りを返す。
   - `session/new`: `params.cwd`（必須、絶対パス。欠落や相対パスは `-32602`）と `params.mcpServers`（必須、配列）を受ける。非空の `mcpServers` は接続せず、資格情報（`env`、ヘッダ、URL、コマンド）を伏せて無視した件数だけを標準エラー出力に一度診断として出す。P5-54 の `Acp` の準備方針で `exec::prepare` を呼び、アクティブなセッションを作り、`{ sessionId, configOptions }`（および `context_window` があれば初期の `usage_update`）を返す。
   - `session/load`: `params.sessionId`・`params.cwd`（絶対パス）・`params.mcpServers` を受ける。既存セッションを開いて再開の準備を行い（記録のプロジェクトの実のパスと `cwd` の実のパスが違えば拒む）、現在の枝の会話を `user_message_chunk`・`agent_thought_chunk`・`agent_message_chunk`・`tool_call`・`tool_call_update` として順に `session/update` で送ってから、`{ configOptions }` を返す。過去の記録から承認済み状態を復元しない。
   - `session/set_config_option`: アクティブな `sessionId` を確かめ、`configId` が `"model"` または `"effort"` のとき `Agent::switch_model` を呼び、更新後の `{ configOptions }` を返す。`session/set_mode` は `-32601`（`Method not found`）とする。
   - `session/prompt`: アクティブな `sessionId` を確かめ、`prompt` 配列の `ContentBlock`（`text`、`resource_link`、`resource` の `text`、`image` の base64）を設計書 05-03「依頼の受け取りと終わりの理由」の規則に従って検証・変換する。非対応の `type`・`audio`・`resource.blob`・上限超過は `-32602` で拒み、提供者へ送らない。エージェントのスレッドで `run_controlled` を走らせ、イベントを `session/update` に変えて送り、残イベントをすべて回収した後に、`RunReport.stop_reason` とその実行の取消状態から一か所で一度だけ `result: { stopReason }`（または JSON-RPC の `error`）を返す。`AgentEvent::Error` は標準エラー出力への診断として扱い、そこで `session/prompt` の応答を送らない。
   - `session/cancel`: 正規の `id` なし通知で非アクティブまたは未知の `sessionId` なら応答せず無視する。`id` 付き要求で非アクティブな `sessionId` なら `-32602` を返す。アクティブなセッションで実行中なら、終了手順の 1〜4 に従って取り消しを発動する。
4. **`AcpApprover` と確認の台帳**:
   - 確認の要求ごとにプロセス内で単調増加する一意の JSON-RPC 要求 ID（`"bh-perm-<n>"`）を振り、`sessionId`、一連の処理の世代、提示した `options` の一覧、返事の `SyncSender<ApprovalAnswer>` を主のスレッドの台帳に登録する。
   - `toolCall` には、設計書 05-03「実行の前の確認（`session/request_permission`）」のとおり、`benitoiteh` が組み立てた事実の全文（題、対象パス、エフェクトと説明、`unread_imports`、サンドボックスの範囲、`.agents` の警告、空白の連なり `␠×N` と制御文字を可視化したコマンド全文）を `rawInput.command` と `content` の両方に入れ、LLM の `justification` は入れない。`toolCall.kind` は `Effects`・`TestEffects`・`Command`・`Escalate` が `"execute"`、`WriteScope`・`CreateProject` が `"edit"`、`WebFetch`・`Network` が `"fetch"` とし、`"read"`・`"search"`・`"think"` は使わない。
   - 出す `options` と選ばれたときの内部の扱いは、設計書 05-03 の対応表のとおりとする。保留中の ID に未提示の `optionId` や不正な形が返ったらその要求を `Denied` で完了させ、すでに保留のない ID への重複・遅れた応答は無視して確定済みの結果も許可集合も変えない。
5. **5 段の終了手順**:
   - 設計書 05-03「スレッドの構成と終了の手順」の 1〜5 をそのまま実装する。特に、取消・EOF・出力障害のときに保留中の全確認（通常確認と通信確認）を `Cancelled` で即時解除し、`AgentEvent` チャネルを完了まで抜き続け、終了時は開いたままの `stdin` スレッドや停止した `stdout` スレッドを無期限に `join` しない。

## 触るファイル

- `crates/benitoiteh/src/cli/mod.rs`、`crates/benitoiteh/src/cli/args.rs`、`crates/benitoiteh/src/cli/text.rs`
- `crates/benitoiteh/src/cli/acp.rs`（新）、必要に応じて `crates/benitoiteh/src/ui/acp/`（新）および `crates/benitoiteh/src/ui/tui/confirmation.rs`
- `crates/benitoiteh/tests/acp.rs`（新）

## テスト

`test-audit` の作成時の関門を通す。模擬の提供者とインメモリの入出力（またはパイプ）で、`crates/benitoiteh/tests/acp.rs` に次を確かめる統合テストを置く。

- **版 1 の交渉と基本の往復**: `protocolVersion: 2`（および未知の欄）を含む `initialize` に `protocolVersion: 1` を返すこと。`session/new`（相対パス `cwd` の `-32602` 拒否と、非空 `mcpServers` の資格情報を伏せた無視を含む）→ `session/prompt` → `session/update`（`agent_thought_chunk`・`agent_message_chunk`・`tool_call` の `pending` から `in_progress`、`tool_call_update`、`usage_update`）→ `stopReason: "end_turn"`。`AgentEvent::Error`（`storage_limit` など）が出ても正常完了の応答が二重になったり失敗に変わったりしないこと。
- **セッションの再開と切り替え**: `session/load` で会話の履歴が再生され、過去の承認記録から許可が復元されないこと。新しい `session/new` でセッションを切り替えたとき、前のセッションの許可や保留中の確認が漏れないこと。非アクティブな `sessionId` への通知 `session/cancel` は無応答で無視され、`id` 付き要求は `-32602` になること。
- **承認の範囲と選択肢**:
  - 通常の `Effects`・`TestEffects` で `allow-once` と `allow-session` と `reject-once` が出ること。`allow-once` では 1 回目が実行され 2 回目に再確認が求められること。`allow-session` では 2 回目が確認なしで動くこと。
  - 一回限りの確認（`Escalate`、`WriteScope`、`CreateProject`、`agents_write`、`unread_imports > 0`）では `allow-once` と `reject-once` だけが出ること。未提示の `"allow-session"` を返すと `Denied` になること。完了済みの要求 ID に遅れて `"allow-session"` を送っても無視され、許可集合が変わらないこと。
  - `session/request_permission` の `toolCall.rawInput.command` と `content` に必須の事実がすべて含まれ、制御文字と 8 文字以上の空白が可視化され、LLM の `justification` が含まれず、`toolCall.kind` が `execute`・`edit`・`fetch` のいずれかであること。
- **取り消し・EOF・出力の停止**:
  - 通常の確認待ち中、通信の中継（`NetworkGate`）の確認待ち中、および `AgentEvent` チャネルが満杯の状態で、`session/cancel` を送ると、デッドロックせずに最後の `session/update` の後に一度だけ `stopReason: "cancelled"` が返ること。
  - クライアントが標準出力を閉じた状態（Broken pipe）、および標準出力の読み取り口を開いたまま一切読まずに出力キューを満杯にして標準入力だけを閉じた状態（EOF）で、`benitoiteh acp` がデッドロックせずに後始末を終えて終了すること。

## 確かめること

- リリースのビルド（`cargo build --release -p benitoiteh`）の実行ファイルの大きさを測り、P5-52 からの増分と、8 MiB の目標に余裕を持って収まることを確かめて報告する。

## 完了条件

- `benitoiteh acp` が ACP 版 1 のハンドシェイク、セッション管理、イベント通知、確認、5 段の終了手順を満たす。
- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが外で `scripts/check.sh`（macOS、aarch64 Linux、`BENITOITEH_X86_64_HOST` の x86_64 Linux）を通す。
