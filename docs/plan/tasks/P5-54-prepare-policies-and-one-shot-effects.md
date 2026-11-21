# P5-54 起動・準備の方針の 5 分離、呼び出し単位の一回限りのエフェクト許可、終了理由の分類と画像バイト列の縮小入口

## 目的

`benitoiteh acp`（P5-55）が既存の `exec::prepare`・`Agent`・`Approver`・`tools` を安全に共用できるよう、設計書 05-03（および第 2 回外部レビューの R01・R02・R04）で定めた四つの基盤を整備する。

1. `ExecArgs.interactive` の一つの真偽値で切り替えていた準備の方針を、五つの独立した方針（起動時の確認、実行時の承認の方針、事前許可のオプション、通信の中継の確認、`askpass` の可否）に分離し、`NetworkGate` への確認チャネル接続も `InteractiveApprover` 以外の確認の受け手から使えるようにする。
2. 制限モードのエフェクト確認（`Effects`・`TestEffects`）で、セッションに記憶する許可（TUI の `[y]` と ACP の `allow_always`）と、その呼び出し 1 回だけに効きセッションの `confirmed_effects` を更新しない許可（ACP の `allow_once`）を分け、1 回目の承認後の再実行（直前の再検査）にその呼び出し限りの承認済みエフェクト集合を渡せるようにする。
3. `Agent::run_controlled` が返す `RunReport` に、英語の診断の文を解析せずに `stopReason`（完了、取り消し、番の数の上限、トークンの量の上限、そのほかの失敗）を判別できる終了の分類を持たせる。
4. `read` の画像縮小の補助（`benitoiteh __image`）に、ローカルのファイルのパスだけでなく、メモリ上のバイト列（ACP の `image` ブロックの base64 復号後のデータ）を渡して検証・縮小・`objects/` への保存を行える入口を加える。

## 読む設計書

- [画像のファイルの読み取り](../../design/01-agent/01-03-tools.md#画像のファイルの読み取り)
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」「制限モードの確認」「.agents への書き込み」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「オプション」
- [ACP（エディタとの連携）](../../design/05-interface/05-03-acp.md)の「起動とセッションの準備の方針の分離」「依頼の受け取りと終わりの理由」「実行の前の確認（`session/request_permission`）」
- [テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」「安全の性質のテスト」、スキル `test-audit`
- [P5-53](P5-53-acp-env-and-proxy-serialization.md)、既存の `crates/benitoiteh/src/cli/{args.rs,exec.rs}`、`src/approval/mod.rs`、`src/agent/{mod.rs,execution.rs,events.rs}`、`src/tools/{mod.rs,benitoite_run.rs,benitoite_test.rs}`、`src/tools/read/`

## この作業が認める型の変更

- `cli::args` / `cli::exec`: 入口の種類（`Tui`・`Exec`・`Acp`）または 5 項目の準備方針を表す型を導入し、`ExecArgs::parse_acp` を加える。
- `agent::Agent`: `connect_network_prompts(&self, prompts: Sender<ApprovalPrompt>, wake: Option<WakeCallback>)` を加え、`AgentConfig` に事前許可（`--allow` 等）を超える実行を確認の受け手に尋ねるか拒むかの区分を持たせる。
- `approval::ApprovalAnswer`: エフェクト確認のセッション記憶の有無（既定は `outcome == ApprovalOutcome::Approved`、一回限りの許可では `false`）を表す欄または構築関数（`ApprovalAnswer::approved_once()` など）を加える。`session.jsonl` の `ApprovalOutcome` の直列化の綴りは変えない。
- `tools::Invocation<'a>`: その呼び出し 1 回限りに承認されたエフェクト集合 `pub call_approved_effects: Option<&'a BTreeSet<crate::benitoite::Effect>>` を加える。
- `agent::RunReport`: 終了理由の分類 `pub stop_reason: StopReason`（`Completed`・`Cancelled`・`MaxTurns`・`MaxRunTokens`・`Failed { kind: &'static str, message: String }`）を加える。`RunControl` に検証済みの添付画像（またはバイト列と MIME 型の組）を渡す欄を加えてよい。
- `tools::read`: メモリ上の画像バイト列と MIME 型を検証・縮小（`benitoiteh __image` 補助経由）して返す関数を公開（または `pub(crate)`）する。

## 作るもの

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

## 触るファイル

- `crates/benitoiteh/src/cli/args.rs`、`crates/benitoiteh/src/cli/exec.rs`
- `crates/benitoiteh/src/approval/mod.rs`
- `crates/benitoiteh/src/agent/mod.rs`、`crates/benitoiteh/src/agent/execution.rs`、`crates/benitoiteh/src/agent/events.rs`
- `crates/benitoiteh/src/tools/mod.rs`、`crates/benitoiteh/src/tools/benitoite_run.rs`、`crates/benitoiteh/src/tools/benitoite_test.rs`、`crates/benitoiteh/src/tools/read/`
- `crates/benitoiteh/tests/args.rs`、`crates/benitoiteh/tests/agent_loop.rs`、`crates/benitoiteh/tests/tools_read_image.rs`（および `Invocation` を使う既存のテスト）

## テスト

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

## 確かめること

- `Invocation.call_approved_effects` を再実行に渡さない壊した版で 1 回目の一回限り許可が `EFFECTS_CHANGED` で失敗すること、および `call_approved_effects` の上限照合を外した壊した版でエフェクト増加時の拒否テストが失敗することを確かめて報告する。

## 完了条件

- 5 項目の準備方針の分離、一回限りのエフェクト許可、終了理由の分類、画像バイト列の検証・縮小入口が揃う。
- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが外で `scripts/check.sh`（macOS、aarch64 Linux、`BENITOITEH_X86_64_HOST` の x86_64 Linux）を通す。
