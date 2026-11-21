# P5-53 子プロセスの環境からの ACP クライアントの MCP 資格情報の強制除外と、通信の中継の終了と確認応答の直列化

- 状態: 未着手
- 依存: P5-52

## 目的

`benitoiteh acp` を加える前段として、既存の環境の規則（`src/config/environment.rs`）と通常モードの通信の中継（`src/sandbox/proxy.rs`）にある二つの安全の境界を固める（設計書 03-02「通信の中継」「規則の後に必ず当てる変更」、05-01「設定ファイル」、05-03「信頼の境界と安全の原則」「通信の中継と秘密の入力の扱い」）。

1. T3Code などの ACP クライアントがエージェントの起動環境に渡す MCP 接続・制御用の資格情報と設定（`T3_ACP_MCP_*` と `T3CODE_*`）を、提供者のキーと同じく、環境の規則の後に必ず子プロセスの環境から除き、`set` や `*` を含まない `allow` で指定することも設定の誤りとして拒む。
2. 通常モードの `[normal] network = "proxy"` の `ProxyRun` で、コマンドの終了（`ProxyRun::finish`）と確認の応答の受領・許可の更新が重なったときに、終わったコマンドの確認応答からセッションのドメイン許可（`NetworkGate.session`）が増える競合を塞ぐ。

## 読む文書

- `AGENTS.md`、[作業の進め方](../00-common.md)、[作業の間の約束](../10-interfaces.md)。
- 設計書 03-02「通信の中継」「環境の規則」「規則の後に必ず当てる変更」、05-01「設定ファイル」、05-03「信頼の境界と安全の原則」「通信の中継と秘密の入力の扱い」、07-01「安全の性質のテスト」。
- `.claude/skills/test-audit/SKILL.md`。

## 作るもの

1. **ACP クライアントの MCP 用変数の強制除外**（`crates/benitoiteh/src/config/environment.rs`、`src/config/text.rs`）
   - 強制除外する接頭辞として `T3_ACP_MCP_` と `T3CODE_`（ASCII の大文字と小文字を区別しない）を定義する（関数 `is_acp_client_mcp_var(name: &str) -> bool`、および `OsStr` の先頭バイト列を ASCII の大小文字を区別せずに比べる判定）。
   - `parse_line` で：
     - `"allow"` のパターンに `*` が含まれず、`is_acp_client_mcp_var(&pattern)` が真のときは、`provider_key` と同じく設定の誤りとして拒む。
     - `"set"` の `name` で `is_acp_client_mcp_var(name)` が真のときは、`FIXED_NAMES` や `BENITOITEH_` と同じく `text::RULE_SET_FIXED` で拒む。
   - `remark_child_environment` で、`PROVIDER_KEY_ENV_VARS` の除外とあわせて、`T3_ACP_MCP_` または `T3CODE_`（ASCII の大小文字を区別しない）で始まる変数をすべて `environment` から除く。これにより、`child_environment`・`remark_child_environment`・`ineffective_rules`（および `doctor` の表示）のすべてで、`allow *` や `allow T3_*` があっても子プロセスには渡らない。
2. **`ProxyRun` の終了と確認応答の直列化**（`crates/benitoiteh/src/sandbox/proxy.rs`）
   - `ProxyRun` の終了状態とコマンド内のドメイン許可・拒否の状態を、一つの `Mutex<RunState>`（`finished: bool`、`allowed: BTreeSet<String>`、`declined: BTreeSet<String>`、`abort_waiters: Vec<SyncSender<()>>` など）にまとめるか、終了の確定と応答の採用・許可集合の更新を同じ錠のもとで直列化する。
   - **錠の規則**: 錠を持ったまま確認の返事（`response.recv_timeout`）、ネットワークの I/O、スレッドの `join` を待たない。複数の錠を取る必要があるとき（`ProxyRun` の状態の錠と `NetworkGate.session` の錠）は、常に `ProxyRun` の状態の錠 → `NetworkGate.session` の錠の順に取る。
   - `ProxyRun::finish(&self)` は、状態の錠を取って `finished = true` にし、確認の返事を待っているスレッドがあれば即時に起こす（またはチャネルで知らせる）。
   - `ProxyRun::decide(&self, host: &str, port: u16)` で確認の応答 `answer` を受け取ったときは、状態の錠を取り、`finished` が真であれば応答を捨てて `Err(Denied::NotAllowed)`（または `UserDeclined`）を返し、`self.allowed` も `self.gate.session` も `self.declined` も一切更新しない。`finished` が偽であるときだけ、同じ錠を保持したまま `Approved` なら `allowed` に、`ApprovedForSession` なら `gate.session` に、`Denied` 等なら `declined` に記録する。

## テスト

`test-audit` の作成時の関門に従う。

- `crates/benitoiteh/tests/config.rs`（または `src/config/environment.rs` の単体テスト）に、親の環境に `T3_ACP_MCP_ENDPOINT`・`T3_ACP_MCP_AUTHORIZATION`・`T3_ACP_MCP_NODE`・`T3CODE_THREAD_ID`（および小文字の綴り）があるとき、既定の `allow *` でも `allow T3_ACP_MCP_*` でも `child_environment` の結果に含まれないこと、`allow T3_ACP_MCP_AUTHORIZATION` や `set T3_ACP_MCP_ENDPOINT=...`・`set T3CODE_THREAD_ID=...` が設定の誤りとして拒まれることを確かめるテストを加える。
- `crates/benitoiteh/src/sandbox/proxy/tests.rs` に、通信の確認（`ApprovalPrompt`）が届いた後に `ProxyRun::finish()` を呼び、その直後に `reply.send(ApprovalAnswer { outcome: ApprovalOutcome::ApprovedForSession, message: None })` を返したとき、その呼び出しが拒否されるだけでなく、同じ `NetworkGate` を使う次の `ProxyRun` でもそのドメインが `gate.session` に許可されておらず再び確認（または拒否）になることを確かめるテストを加える。

## 完了の条件

- `scripts/check.sh --inside-codex` が通る。
- 壊した版（`remark_child_environment` から `T3_ACP_MCP_*` の除外を外した版、`ProxyRun::decide` で応答受領時の直列化検査を外した版）で、加えたテストが失敗することを確かめる。
- 本書の「作ったもの」と「確かめた結果」を記録する。
