# BH-054〜057 ACP（Agent Client Protocol）対応の実装プラン

本書は、[README.md](README.md)、[decisions.md](decisions.md)、および設計書 [05-03「ACP（エディタとの連携）」](../design/05-interface/05-03-acp.md) に基づく実装プランである。全体の実装プラン（[docs/plan/README.md](../plan/README.md)）の作業番号 P5-53〜P5-56 に対応する。

今回は設計とプランの作成までであり、設計者が本ディレクトリの文書を読んで了承した後に P5-53 から順に実装を始める。

## 前提と依存

- **開始する版**: `gamou` ブランチの `f051cb6`（P5-52 完了時点。`design/acp-support` ブランチはこの上に設計書と本プランを重ねている）。
- **処理系の版**: Benitoite `v0.0.12`（既存の `P5-40` で取り込み済み。処理系側の変更は一切要らない）。
- **新しい外部クレート**: 追加しない（`BH-056` の決定）。既存の `serde`・`serde_json` と標準ライブラリだけで実装する。
- **実装する者**: 作業ごとに Codex CLI（推論の度合い high）。作業の進め方と共通の規則は [docs/plan/00-common.md](../plan/00-common.md) と [implementation-request.md](implementation-request.md) に従う。

## 共通の関門

すべての作業（P5-53〜P5-56）に次の関門を適用する。

1. **テストを弱めない**: 既存のテストを削除・変更・`#[ignore]` 化しない。型の欄の追加（P5-54 の `Invocation.call_approved_effects` や `RunReport.stop_reason` など）に伴って既存のテストの構築箇所を更新するときも、既存の検証条件と期待値を一切緩めない。
2. **`test-audit` の作成時の関門を通す**: 新しいテストを加えるときは、`.claude/skills/test-audit/SKILL.md` の作成時の関門（守る観測可能な契約、現実的な退行、既存のテストで捕まらない理由、テスト専用の本番 API を足していないこと、および実装の該当箇所を壊したときに新しいテストが失敗すること）を確かめて報告する。
3. **コーディング規約を守る**: [00-common.md](../plan/00-common.md) のとおり、本番のコードに `unwrap`・`expect`・添字アクセス `[]`・`unsafe`（`sandbox/` と `process.rs` の既存の箇所以外）を書かない。利用者に示す文言は各モジュールの `text.rs` に集める。
4. **個人の接続先・秘密を書かない**: 追跡するファイルに個人のホスト名・鍵ファイル名・トークンなどを一切書かない。

## 段とレビューの区切り

4 つの作業を 2 つの段に分ける。各作業はそれぞれ完了した状態でコンパイルと検査が通り、単独でコミットできる単位とする。

| 段 | 作業 | 焦点 | 段の終わりのレビューと検査 |
|---|---|---|---|
| 段 1 | P5-53、P5-54 | 子プロセスの環境からの `T3_ACP_MCP_*`・`T3CODE_*` 除外、`ProxyRun` の終了と確認応答の直列化、準備方針の 5 分離、一回限りのエフェクト許可 `Invocation.call_approved_effects`、終了理由 `StopReason`、画像バイト列の縮小入口 | レビュー G1、および外の環境（macOS、aarch64 Linux、x86_64 Linux）での `scripts/check.sh` 全体 |
| 段 2 | P5-55、P5-56 | `benitoiteh acp` の CLI と JSON-RPC 2.0（ACP 版 1）通信層、`AcpApprover`、5 段の終了手順、本物のサンドボックスと処理系による `sandbox_acp.rs` 安全統合テスト、リリースバイナリの大きさ測定、T3Code 実機検証（BH-055） | レビュー G2、外の環境での `scripts/check.sh` 全体、リリースバイナリ測定、T3Code 実機検証の記録（`docs/plan/results/P5-56-acp.md`） |

---

## 段 1: 安全境界の補強とエージェント・承認の基盤

### P5-53 子プロセスの環境からの ACP クライアントの MCP 資格情報の強制除外と、通信の中継の終了と確認応答の直列化

- **状態**: 完了
- **依存**: P5-52

#### 目的

`benitoiteh acp` を加える前段として、既存の環境の規則（`src/config/environment.rs`）と通常モードの通信の中継（`src/sandbox/proxy.rs`）にある二つの安全の境界を固める（設計書 03-02「通信の中継」「規則の後に必ず当てる変更」、05-01「設定ファイル」、05-03「信頼の境界と安全の原則」「通信の中継と秘密の入力の扱い」、[decisions.md](decisions.md) の D1・D6）。

1. T3Code などの ACP クライアントがエージェントの起動環境に渡す MCP 接続・制御用の資格情報と設定（`T3_ACP_MCP_*` と `T3CODE_*`）を、提供者のキーと同じく、環境の規則の後に必ず子プロセスの環境から除き、`set` や `*` を含まない `allow` で指定することも設定の誤りとして拒む。
2. 通常モードの `[normal] network = "proxy"` の `ProxyRun` で、コマンドの終了（`ProxyRun::finish`）と確認の応答の受領・許可の更新が重なったときに、終わったコマンドの確認応答からセッションのドメイン許可（`NetworkGate.session`）が増える競合を塞ぐ。

#### 読む設計書

- [OS のサンドボックス](../design/03-safety/03-02-sandbox.md)の「通信の中継」「環境の規則」「規則の後に必ず当てる変更」
- [CLI と設定ファイル](../design/05-interface/05-01-cli.md)の「設定ファイル」「doctor」
- [ACP（エディタとの連携）](../design/05-interface/05-03-acp.md)の「信頼の境界と安全の原則」「通信の中継と秘密の入力の扱い」
- [テスト](../design/07-quality/07-01-testing.md)の「持ち主の境界」「安全の性質のテスト」、スキル `test-audit`
- [P5-49](../plan/tasks/P5-49-environment-rules.md)、既存の `crates/benitoiteh/src/config/environment.rs`、`src/config/text.rs`、`src/sandbox/proxy.rs`

#### この作業が認める型の変更

なし（`child_environment`・`remark_child_environment`・`EnvironmentRules::parse`・`ProxyRun` の公開シグネチャは変えず、内部の状態管理と規則の検査を更新する）。

#### 作るもの

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

#### 触るファイル

- `crates/benitoiteh/src/config/environment.rs`、`crates/benitoiteh/src/config/text.rs`
- `crates/benitoiteh/src/sandbox/proxy.rs`、`crates/benitoiteh/src/sandbox/proxy/tests.rs`
- `crates/benitoiteh/tests/config.rs`

#### テスト

`test-audit` の作成時の関門を通し、既存の設定と通信中継のテストを拡張する。

- 設定と環境の境界（`crates/benitoiteh/tests/config.rs` または `src/config/environment.rs`）で、親の環境に `T3_ACP_MCP_ENDPOINT`・`T3_ACP_MCP_AUTHORIZATION`・`T3_ACP_MCP_NODE`・`T3CODE_THREAD_ID`（および小文字の綴り）があるとき、既定の `allow *` でも `allow T3_ACP_MCP_*` でも `child_environment` の結果に含まれないこと、`allow T3_ACP_MCP_AUTHORIZATION` や `set T3_ACP_MCP_ENDPOINT=...`・`set T3CODE_THREAD_ID=...` が設定の誤りとして拒まれることを確かめる。
- 通信中継の境界（`crates/benitoiteh/src/sandbox/proxy/tests.rs`）で、通信の確認（`ApprovalPrompt`）が届いた後に `ProxyRun::finish()` を呼び、その直後に `reply.send(ApprovalAnswer { outcome: ApprovalOutcome::ApprovedForSession, message: None })` を返したとき、その呼び出しが拒否されるだけでなく、同じ `NetworkGate` を使う次の `ProxyRun` でもそのドメインが `gate.session` に許可されておらず再び確認（または拒否）になることを確かめる。

#### 確かめること

- `remark_child_environment` から `T3_ACP_MCP_*`・`T3CODE_*` の除外を外した壊した版と、`ProxyRun::decide` で応答受領時の直列化検査を外した壊した版で、それぞれ加えたテストが失敗することを確かめて報告する。

#### 完了条件

- `T3_ACP_MCP_*`・`T3CODE_*` が子プロセスの環境から強制除外され、規則での個別指定が拒まれる。
- `ProxyRun::finish` の後に届いた確認応答がセッションのドメイン許可を変更しない。
- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが外で `scripts/check.sh`（macOS、aarch64 Linux、`BENITOITEH_X86_64_HOST` の x86_64 Linux）を通す。

---

### P5-54 起動・準備の方針の 5 分離、呼び出し単位の一回限りのエフェクト許可、終了理由の分類と画像バイト列の縮小入口

- **状態**: 完了
- **依存**: P5-53

#### 目的

`benitoiteh acp`（P5-55）が既存の `exec::prepare`・`Agent`・`Approver`・`tools` を安全に共用できるよう、設計書 05-03（および [decisions.md](decisions.md) の D4・D5・D8）で定めた四つの基盤を整備する。

1. `ExecArgs.interactive` の一つの真偽値で切り替えていた準備の方針を、五つの独立した方針（起動時の確認、実行時の承認の方針、事前許可のオプション、通信の中継の確認、`askpass` の可否）に分離し、`NetworkGate` への確認チャネル接続も `InteractiveApprover` 以外の確認の受け手から使えるようにする。
2. 制限モードのエフェクト確認（`Effects`・`TestEffects`）で、セッションに記憶する許可（TUI の `[y]` と ACP の `allow_always`）と、その呼び出し 1 回だけに効きセッションの `confirmed_effects` を更新しない許可（ACP の `allow_once`）を分け、1 回目の承認後の再実行（直前の再検査）にその呼び出し限りの承認済みエフェクト集合を渡せるようにする。
3. `Agent::run_controlled` が返す `RunReport` に、英語の診断の文を解析せずに `stopReason`（完了、取り消し、番の数の上限、トークンの量の上限、そのほかの失敗）を判別できる終了の分類を持たせる。
4. `read` の画像縮小の補助（`benitoiteh __image`）に、ローカルのファイルのパスだけでなく、メモリ上のバイト列（ACP の `image` ブロックの base64 復号後のデータ）を渡して検証・縮小・`objects/` への保存を行える入口を加える。

#### 読む設計書

- [画像のファイルの読み取り](../design/01-agent/01-03-tools.md#画像のファイルの読み取り)
- [利用者の確認](../design/03-safety/03-03-approval.md)の「確認の共通の規則」「制限モードの確認」「.agents への書き込み」
- [CLI と設定ファイル](../design/05-interface/05-01-cli.md)の「オプション」
- [ACP（エディタとの連携）](../design/05-interface/05-03-acp.md)の「起動とセッションの準備の方針の分離」「依頼の受け取りと終わりの理由」「実行の前の確認（`session/request_permission`）」
- [テスト](../design/07-quality/07-01-testing.md)の「持ち主の境界」「安全の性質のテスト」、スキル `test-audit`
- [P5-53](#p5-53-子プロセスの環境からの-acp-クライアントの-mcp-資格情報の強制除外と通信の中継の終了と確認応答の直列化)、既存の `crates/benitoiteh/src/cli/{args.rs,exec.rs}`、`src/approval/mod.rs`、`src/agent/{mod.rs,execution.rs,events.rs}`、`src/tools/{mod.rs,benitoite_run.rs,benitoite_test.rs}`、`src/tools/read/`

#### この作業が認める型の変更

- `cli::args` / `cli::exec`: 入口の種類（`Tui`・`Exec`・`Acp`）または 5 項目の準備方針を表す型を導入し、`ExecArgs::parse_acp` を加える。
- `agent::Agent`: `connect_network_prompts(&self, prompts: Sender<ApprovalPrompt>, wake: Option<WakeCallback>)` を加え、`AgentConfig` に事前許可（`--allow` 等）を超える実行を確認の受け手に尋ねるか拒むかの区分を持たせる。
- `approval::ApprovalAnswer`: エフェクト確認のセッション記憶の有無（既定は `outcome == ApprovalOutcome::Approved`、一回限りの許可では `false`）を表す欄または構築関数（`ApprovalAnswer::approved_once()` など）を加える。`session.jsonl` の `ApprovalOutcome` の直列化の綴りは変えない。
- `tools::Invocation<'a>`: その呼び出し 1 回限りに承認されたエフェクト集合 `pub call_approved_effects: Option<&'a BTreeSet<crate::benitoite::Effect>>` を加える。
- `agent::RunReport`: 終了理由の分類 `pub stop_reason: StopReason`（`Completed`・`Cancelled`・`MaxTurns`・`MaxRunTokens`・`Failed { kind: &'static str, message: String }`）を加える。`RunControl` に検証済みの添付画像（またはバイト列と MIME 型の組）を渡す欄を加えてよい。
- `tools::read`: メモリ上の画像バイト列と MIME 型を検証・縮小（`benitoiteh __image` 補助経由）して返す関数を公開（または `pub(crate)`）する。

#### 作るもの

1. **起動・準備の方針の 5 分離**（`crates/benitoiteh/src/cli/args.rs`、`src/cli/exec.rs`、`src/agent/mod.rs`）
   - 設計書 05-03「起動とセッションの準備の方針の分離」の表の 5 項目をそれぞれ独立に判定する。
     - 起動・再開時の確認: `Tui` は画面で確認、`Exec` と `Acp` は保存済みの信頼または `--trust-project` に従い、制限モードの処理系の SHA-256 変化と制限モードから通常モードへの再開は拒む。
     - 実行時の承認の方針: `Tui` は制限モードが `OnRequest`・通常モードが `--approval` または設定値、`Exec` は `Never`、`Acp` は制限モードが `--approval`（`OnRequest` または `Never`、既定 `OnRequest`、制限モードでの `Untrusted` は引数の誤り）・通常モードが `--approval` または設定値（既定 `OnRequest`）。
     - `--allow`・`--allow-agents-write`・`--project-parent`・`--ephemeral`: `Exec` と `Acp` で受け付け、`Tui` では使い方の誤りとする。
     - 通信の中継（`normal.network = "proxy"`）の未許可ドメインの確認: `Tui` と `Acp` で承認の方針が `Never` でないときに有効にする。
     - 秘密の入力（`secrets.askpass`）: `Tui` のみ設定に従い、`Exec` と `Acp` では無効とする。
2. **呼び出し単位の一回限りのエフェクト許可**（`crates/benitoiteh/src/approval/mod.rs`、`src/agent/execution.rs`、`src/tools/mod.rs`、`src/tools/benitoite_run.rs`、`src/tools/benitoite_test.rs`）
   - `Agent::approve` で：
     - `--allow` が指定されているとき、宣言したエフェクトが `--allow`（一時スクリプトでは確認なしの範囲との和）に収まれば確認なしに実行する。収まらないとき、承認の方針が `Approval::Never`（`exec` または `acp --approval never`）なら `allow_denied` で断り（3 回続けば `EffectsNotAllowed`）、`OnRequest`（`acp` の既定）なら `Approver` による確認へ進む。`unread_imports > 0` や `agents_write` で事前許可がないときも同じ規則で、`Never` なら断り、`OnRequest` なら確認へ進む。
     - `Approver` が許可を返したとき、セッションに記憶する許可なら今までどおり `self.confirmed_effects` を広げ、一回限りの許可なら `self.confirmed_effects` を変えずに、その確認の `record.facts.effects` から得た集合を `execute_approved` の `Invocation.call_approved_effects` にだけ渡す。
   - `benitoite_run::execute_script`（`benitoite_scratch` も通る）と `benitoite_test::execute` で、`approval == ApprovalStatus::Approved` の再実行のとき、再検査した `confirmation` が「セッションの `confirmed_effects`（または `retry_automatic`）に収まる」か「`invocation.call_approved_effects` に収まる」ことを確かめ、収まらなければ `EFFECTS_CHANGED` で拒む。
3. **終了理由の分類**（`crates/benitoiteh/src/agent/events.rs`、`src/agent/mod.rs`）
   - `Agent::run_controlled` が返す `RunReport` に `stop_reason: StopReason` を設定する。既存の `RunReport.outcome` と `exec` の終了状態は変えない。
4. **画像バイト列の検証・縮小・保存の入口**（`crates/benitoiteh/src/tools/read/`、`src/agent/mod.rs`）
   - メモリ上のバイト列と MIME 型を受け取り、寸法と形式の検証、上限を超える PNG/JPEG の `benitoiteh __image` サンドボックス補助による縮小、セッションの `objects/` への保存を行う関数を加える。

#### 触るファイル

- `crates/benitoiteh/src/cli/args.rs`、`crates/benitoiteh/src/cli/exec.rs`
- `crates/benitoiteh/src/approval/mod.rs`
- `crates/benitoiteh/src/agent/mod.rs`、`crates/benitoiteh/src/agent/execution.rs`、`crates/benitoiteh/src/agent/events.rs`
- `crates/benitoiteh/src/tools/mod.rs`、`crates/benitoiteh/src/tools/benitoite_run.rs`、`crates/benitoiteh/src/tools/benitoite_test.rs`、`crates/benitoiteh/src/tools/read/`
- `crates/benitoiteh/tests/args.rs`、`crates/benitoiteh/tests/agent_loop.rs`、`crates/benitoiteh/tests/tools_read_image.rs`（および `Invocation` を使う既存のテスト）

#### テスト

`test-audit` の作成時の関門を通す。

- **一回限りのエフェクト確認のテスト**（`tests/agent_loop.rs`）:
  - 1 回目の `benitoite_run`（および `benitoite_test`）で一回限りの許可を返したとき、1 回目の実行が再検査を通って成功すること。
  - 続けて同じスクリプトを 2 回目に呼んだとき、`confirmed_effects` に記憶されておらず再び `Approver` に確認が求められること。
  - 既存の `confirmed_effects`（例: `File.Read`）がある状態で、`File.Read, File.Write` のスクリプトを一回限りで許可したとき、その 1 回は実行でき、次の呼び出しでは既存の `File.Read` の記憶だけが保たれている（消えも広がりもしない）こと。
  - 確認の直後、再実行の再検査の前にスクリプトのエフェクトが増えたときは、一回限りの許可でも `EFFECTS_CHANGED` で拒まれること。
- **準備の方針と `--allow` + 確認のテスト**（`tests/args.rs`、`tests/agent_loop.rs`）:
  - `ExecArgs::parse_acp` で `--allow`・`--allow-agents-write`・`--project-parent`・`--ephemeral`・`--approval on-request|never`（制限モードでは `untrusted` を拒む）が設計書 05-01・05-03 のとおりに判定されること。
  - `--allow File.Read` かつ承認の方針が `OnRequest` のとき、`File.Read` だけのスクリプトは確認なしで動き、`File.Write` を含むスクリプトは `Approver` に確認を求め、`Never` のときは確認せずに `allow_denied` で拒むこと。
- **終了理由と画像バイト列のテスト**:
  - `max_turns` と `max_run_tokens` に達したときの `RunReport.stop_reason` がそれぞれ `MaxTurns`・`MaxRunTokens` になること。
  - PNG/JPEG のバイト列を渡した画像検証・縮小が `objects/` に保存され、壊れたバイト列や非対応の形式が拒まれること。

#### 確かめること

- `Invocation.call_approved_effects` を再実行に渡さない壊した版で 1 回目の一回限り許可が `EFFECTS_CHANGED` で失敗すること、および `call_approved_effects` の上限照合を外した壊した版でエフェクト増加時の拒否テストが失敗することを確かめて報告する。

#### 完了条件

- 5 項目の準備方針の分離、一回限りのエフェクト許可、終了理由の分類、画像バイト列の検証・縮小入口が揃う。
- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが外で `scripts/check.sh`（macOS、aarch64 Linux、`BENITOITEH_X86_64_HOST` の x86_64 Linux）を通す。

---

## 段 2: ACP 本体と安全の統合検証・実機検証

### P5-55 `benitoiteh acp` のサブコマンド、ACP 版 1 の JSON-RPC 層、`AcpApprover`、終了手順

- **状態**: 完了
- **依存**: P5-54

#### 目的

設計書 05-03「ACP（エディタとの連携）」および [decisions.md](decisions.md) の D1〜D8 に従い、`benitoiteh acp [オプション]` サブコマンド、標準入出力の JSON-RPC 2.0（ACP 版 1）の通信層、`AcpApprover` による `session/request_permission` の確認の受け渡し、および取り消し・EOF・出力障害の 5 段の終了手順を実装する。

#### 読む設計書

- [ACP（エディタとの連携）](../design/05-interface/05-03-acp.md)
- [CLI と設定ファイル](../design/05-interface/05-01-cli.md)の「コマンドラインの形」「オプション」「終了状態」
- [対話の画面](../design/05-interface/05-02-tui.md)の「出力の示し方」「確認の画面」
- [利用者の確認](../design/03-safety/03-03-approval.md)の「確認の共通の規則」「制限モードの確認」「.agents への書き込み」
- [テスト](../design/07-quality/07-01-testing.md)の「持ち主の境界」「安全の性質のテスト」、スキル `test-audit`
- [P5-54](#p5-54-起動準備の方針の-5-分離呼び出し単位の一回限りのエフェクト許可終了理由の分類と画像バイト列の縮小入口)、既存の `crates/benitoiteh/src/cli/{mod.rs,args.rs,exec.rs,tui.rs}`、`src/ui/tui/{worker.rs,confirmation.rs}`、`src/approval/{mod.rs,interactive.rs}`

#### この作業が認める型の変更

- `cli`: `acp` のサブコマンドのモジュール（`src/cli/acp.rs`、必要に応じて `src/ui/acp/`）を加え、`cli::run` から `"acp"` を振り分ける。
- `ui::tui::confirmation`（または `approval`）: 確認の事実（`ApprovalFacts`）から、制御文字と 8 文字以上の空白を可視化した事実の行を組み立てる処理を、`tui` と `acp` で共有できるように関数を切り出してよい。

#### 作るもの

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

#### 触るファイル

- `crates/benitoiteh/src/cli/mod.rs`、`crates/benitoiteh/src/cli/args.rs`、`crates/benitoiteh/src/cli/text.rs`
- `crates/benitoiteh/src/cli/acp.rs`（新）、必要に応じて `crates/benitoiteh/src/ui/acp/`（新）および `crates/benitoiteh/src/ui/tui/confirmation.rs`
- `crates/benitoiteh/tests/acp.rs`（新）

#### テスト

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

#### 確かめること

- リリースのビルド（`cargo build --release -p benitoiteh`）の実行ファイルの大きさを測り、P5-52 からの増分と、8 MiB の目標に余裕を持って収まることを確かめて報告する。

#### 完了条件

- `benitoiteh acp` が ACP 版 1 のハンドシェイク、セッション管理、イベント通知、確認、5 段の終了手順を満たす。
- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが外で `scripts/check.sh`（macOS、aarch64 Linux、`BENITOITEH_X86_64_HOST` の x86_64 Linux）を通す。

---

### P5-56 `benitoiteh acp` の安全の性質の統合テストと T3Code 実機検証（BH-055）

- **状態**: 未着手
- **依存**: P5-55

#### 目的

本物の OS のサンドボックス（macOS の Seatbelt、Linux の bubblewrap + Landlock + seccomp）と本物の Benitoite の処理系（`v0.0.12`）を使って、`benitoiteh acp` の安全の性質（設計書 05-03、07-01「安全の性質のテスト」）を統合テストで確かめる。あわせて、実機の T3Code に `Local ACP command` として `benitoiteh acp` を登録し、`BH-055` の 4 項目を検証して記録する。

#### 読む設計書

- [ACP（エディタとの連携）](../design/05-interface/05-03-acp.md)
- [セキュリティモデル](../design/03-safety/03-01-security-model.md)
- [OS のサンドボックス](../design/03-safety/03-02-sandbox.md)
- [テスト](../design/07-quality/07-01-testing.md)の「安全の性質のテスト」、スキル `test-audit`
- [未決事項](../design/open-issues.md)（`BH-054`〜`BH-057`）
- [P5-55](#p5-55-benitoiteh-acp-のサブコマンドacp-版-1-の-json-rpc-層acpapprover終了手順)、既存の `crates/benitoiteh/tests/{sandbox_exec_restricted.rs,sandbox_benitoite_run.rs,sandbox_bash.rs}`

#### この作業が認める型の変更

なし。

#### 作るもの

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

#### 触るファイル

- `crates/benitoiteh/tests/sandbox_acp.rs`（新）
- `scripts/check.sh`
- `docs/plan/results/P5-56-acp.md`（新、実機検証の記録）
- `docs/design/05-interface/05-03-acp.md`、`docs/design/open-issues.md`（検証完了後の更新）

#### テスト

`test-audit` の作成時の関門を通す。

- `crates/benitoiteh/tests/sandbox_acp.rs` で、本物の処理系（`v0.0.12`）と本物の OS のサンドボックス（macOS の Seatbelt、x86_64 Linux の bubblewrap + Landlock + seccomp）を通して、上記の 4 つの安全契約を確かめる。

#### 確かめること

- macOS と x86_64 Linux（Landlock 有効）で `tests/sandbox_acp.rs` が通り、実機の T3Code で `BH-055` の 4 項目（版 1 への交渉、承認カードの全事実表示と Supervised 設定での非自動承認、`session/new`・`session/load`・`configOptions`、`session/cancel` と EOF の後始末）が成立することを確かめて `docs/plan/results/P5-56-acp.md` に記録する。

#### 完了条件

- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが外で `scripts/check.sh` の全体（macOS、aarch64 Linux、`BENITOITEH_X86_64_HOST` の x86_64 Linux）を通す。
- `docs/plan/results/P5-56-acp.md` に実機検証と安全テストの結果を記録する。

---

## 検査の割り当て

| 検査 | 走る場所 | 対象 |
|---|---|---|
| `scripts/check.sh --inside-codex` | 各作業の Codex CLI の中 | `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、依存の勧告の検査、Seatbelt と Linux コンテナを使わない全単体・統合テスト（`tests/config.rs`、`tests/args.rs`、`tests/agent_loop.rs`、`tests/acp.rs` など）、Python 道具のテスト |
| `scripts/check.sh`（全体） | 各作業の完了後にオーケストレータが外で実行 | 上記に加え、macOS の Seatbelt を使う `tests/sandbox_*.rs`（P5-56 の `sandbox_acp.rs` を含む）、Apple の `container`（aarch64 Linux）、および `BENITOITEH_X86_64_HOST` が指す x86_64 Linux（Landlock + bubblewrap + seccomp）での全テスト |
| リリースバイナリの大きさ測定 | P5-55 の完了時 | `cargo build --release -p benitoiteh` の実行ファイルの大きさを測り、8 MiB の上限（06-02）に対する余裕を記録する |
| T3Code 実機検証（`BH-055`） | P5-56 の完了時 | T3Code（`d5f8a7573a3817d8c20f7491e7672443355c4083` 以降）の `Local ACP command` に `benitoiteh acp` を登録し、`BH-055` の 4 項目を検証して `docs/plan/results/P5-56-acp.md` に記録する |

## 結果とレビュー修正の管理

- 段 1（P5-53・P5-54）と段 2（P5-55・P5-56）の各作業が完了するごとに、[docs/plan/README.md](../plan/README.md) と本書の作業状態を更新し、レビュー（G1・G2）で修正が必要になった場合は該当する作業（または追加の修正コミット）で処置して再検査を通す。
- P5-56 の検証結果は `docs/plan/results/P5-56-acp.md` に記録し、[docs/design/open-issues.md](../design/open-issues.md) の `BH-055` を更新する。
