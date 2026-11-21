# P5-53 子プロセスの環境からの ACP クライアントの MCP 資格情報の強制除外と、通信の中継の終了と確認応答の直列化

## 目的

`benitoiteh acp` を加える前段として、既存の環境の規則（`src/config/environment.rs`）と通常モードの通信の中継（`src/sandbox/proxy.rs`）にある二つの安全の境界を固める（設計書 03-02「通信の中継」「規則の後に必ず当てる変更」、05-01「設定ファイル」、05-03「信頼の境界と安全の原則」「通信の中継と秘密の入力の扱い」）。

1. T3Code などの ACP クライアントがエージェントの起動環境に渡す MCP 接続・制御用の資格情報と設定（`T3_ACP_MCP_*` と `T3CODE_*`）を、提供者のキーと同じく、環境の規則の後に必ず子プロセスの環境から除き、`set` や `*` を含まない `allow` で指定することも設定の誤りとして拒む。
2. 通常モードの `[normal] network = "proxy"` の `ProxyRun` で、コマンドの終了（`ProxyRun::finish`）と確認の応答の受領・許可の更新が重なったときに、終わったコマンドの確認応答からセッションのドメイン許可（`NetworkGate.session`）が増える競合を塞ぐ。

## 読む設計書

- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「通信の中継」「環境の規則」「規則の後に必ず当てる変更」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」「doctor」
- [ACP（エディタとの連携）](../../design/05-interface/05-03-acp.md)の「信頼の境界と安全の原則」「通信の中継と秘密の入力の扱い」
- [テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」「安全の性質のテスト」、スキル `test-audit`
- [P5-49](P5-49-environment-rules.md)、既存の `crates/benitoiteh/src/config/environment.rs`、`src/config/text.rs`、`src/sandbox/proxy.rs`

## この作業が認める型の変更

なし（`child_environment`・`remark_child_environment`・`EnvironmentRules::parse`・`ProxyRun` の公開シグネチャは変えず、内部の状態管理と規則の検査を更新する）。

## 作るもの

1. **ACP クライアントの MCP 用変数の強制除外**（`crates/benitoiteh/src/config/environment.rs`、`src/config/text.rs`）
   - 強制除外する接頭辞として `T3_ACP_MCP_` と `T3CODE_`（ASCII の大文字と小文字を区別しない）を定義する。
   - `parse_line` で：
     - `"allow"` のパターンに `*` が含まれず、`T3_ACP_MCP_` または `T3CODE_`（ASCII の大小文字を区別しない）で始まるときは、`provider_key` と同じく設定の誤りとして拒む。
     - `"set"` の `name` が `T3_ACP_MCP_` または `T3CODE_`（ASCII の大小文字を区別しない）で始まるときは、`FIXED_NAMES` や `BENITOITEH_` と同じく `text::RULE_SET_FIXED` で拒む。
   - `remark_child_environment` で、`PROVIDER_KEY_ENV_VARS` の除外とあわせて、`T3_ACP_MCP_` または `T3CODE_`（ASCII の大小文字を区別しない）で始まる変数をすべて `environment` から除く。これにより、`child_environment`・`remark_child_environment`・`ineffective_rules`（および `doctor` の表示）のすべてで、`allow *` や `allow T3_*` があっても子プロセスには渡らない。
2. **`ProxyRun` の終了と確認応答の直列化**（`crates/benitoiteh/src/sandbox/proxy.rs`）
   - `ProxyRun` の終了状態（`finished`）とコマンド内のドメイン許可・拒否の状態（`allowed`・`declined`）の更新、および `NetworkGate.session` へのドメイン追加を、終了の確定と同じ錠（`Mutex`）のもとで直列化する。
   - **錠の規則**: 錠を持ったまま確認の返事（`response.recv_timeout`）、ネットワークの I/O、スレッドの `join` を待たない。複数の錠を続けて取る必要があるとき（`ProxyRun` の状態の錠と `NetworkGate.session` の錠）は、常に `ProxyRun` の状態の錠 → `NetworkGate.session` の錠の順に取る。
   - `ProxyRun::finish(&self)` は、状態の錠を取って `finished = true` にする。
   - `ProxyRun::decide(&self, host: &str, port: u16)` で確認の応答 `answer` を受け取ったときは、状態の錠を取り、`finished` が真であれば応答を捨てて `Err(Denied::NotAllowed)`（または `UserDeclined`）を返し、`self.allowed` も `self.gate.session` も `self.declined` も一切更新しない。`finished` が偽であるときだけ、同じ錠を保持したまま `Approved` なら `allowed` に、`ApprovedForSession` なら `gate.session` に、`Denied` 等なら `declined` に記録する。

## 触るファイル

- `crates/benitoiteh/src/config/environment.rs`、`crates/benitoiteh/src/config/text.rs`
- `crates/benitoiteh/src/sandbox/proxy.rs`、`crates/benitoiteh/src/sandbox/proxy/tests.rs`
- `crates/benitoiteh/tests/config.rs`

## テスト

`test-audit` の作成時の関門を通し、既存の設定と通信中継のテストを拡張する。

- 設定と環境の境界（`crates/benitoiteh/tests/config.rs` または `src/config/environment.rs`）で、親の環境に `T3_ACP_MCP_ENDPOINT`・`T3_ACP_MCP_AUTHORIZATION`・`T3_ACP_MCP_NODE`・`T3CODE_THREAD_ID`（および小文字の綴り）があるとき、既定の `allow *` でも `allow T3_ACP_MCP_*` でも `child_environment` の結果に含まれないこと、`allow T3_ACP_MCP_AUTHORIZATION` や `set T3_ACP_MCP_ENDPOINT=...`・`set T3CODE_THREAD_ID=...` が設定の誤りとして拒まれることを確かめる。
- 通信中継の境界（`crates/benitoiteh/src/sandbox/proxy/tests.rs`）で、通信の確認（`ApprovalPrompt`）が届いた後に `ProxyRun::finish()` を呼び、その直後に `reply.send(ApprovalAnswer { outcome: ApprovalOutcome::ApprovedForSession, message: None })` を返したとき、その呼び出しが拒否されるだけでなく、同じ `NetworkGate` を使う次の `ProxyRun` でもそのドメインが `gate.session` に許可されておらず再び確認（または拒否）になることを確かめる。

## 確かめること

- `remark_child_environment` から `T3_ACP_MCP_*`・`T3CODE_*` の除外を外した壊した版と、`ProxyRun::decide` で応答受領時の直列化検査を外した壊した版で、それぞれ加えたテストが失敗することを確かめて報告する。

## 完了条件

- `T3_ACP_MCP_*`・`T3CODE_*` が子プロセスの環境から強制除外され、規則での個別指定が拒まれる。
- `ProxyRun::finish` の後に届いた確認応答がセッションのドメイン許可を変更しない。
- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが外で `scripts/check.sh`（macOS、aarch64 Linux、`BENITOITEH_X86_64_HOST` の x86_64 Linux）を通す。
