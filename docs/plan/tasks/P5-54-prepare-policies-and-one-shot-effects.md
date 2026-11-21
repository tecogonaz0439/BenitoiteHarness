# P5-54 起動・準備の方針の 5 分離、呼び出し単位の一回限りのエフェクト許可、終了理由の分類と画像バイト列の縮小入口

- 状態: 未着手
- 依存: P5-53

## 目的

`benitoiteh acp`（P5-55）が既存の `exec::prepare`・`Agent`・`Approver`・`tools` を安全に共用できるよう、設計書 05-03（および第 2 回外部レビューの R01・R02・R04）で定めた四つの基盤を整備する。

1. `ExecArgs.interactive` の一つの真偽値で切り替えていた準備の方針を、五つの独立した方針（起動時の確認、実行時の承認の方針、事前許可のオプション、通信の中継の確認、`askpass` の可否）に分離し、`NetworkGate` への確認チャネル接続も `InteractiveApprover` 以外の確認の受け手から使えるようにする。
2. 制限モードのエフェクト確認（`Effects`・`TestEffects`）で、セッションに記憶する許可（TUI の `[y]` と ACP の `allow_always`）と、その呼び出し 1 回だけに効きセッションの `confirmed_effects` を更新しない許可（ACP の `allow_once`）を分け、1 回目の承認後の再実行（直前の再検査）にその呼び出し限りの承認済みエフェクト集合を渡せるようにする。
3. `Agent::run_controlled` が返す `RunReport` に、英語の診断の文を解析せずに `stopReason`（完了、取り消し、番の数の上限、トークンの量の上限、そのほかの失敗）を判別できる終了の分類を持たせる。
4. `read` の画像縮小の補助（`benitoiteh __image`）に、ローカルのファイルのパスだけでなく、メモリ上のバイト列（ACP の `image` ブロックの base64 復号後のデータ）を渡して検証・縮小・`objects/` への保存を行える入口を加える。

## 読む文書

- `AGENTS.md`、[作業の進め方](../00-common.md)、[作業の間の約束](../10-interfaces.md)。
- 設計書 01-03「画像のファイルの読み取り」、03-03「確認の共通の規則」「制限モードの確認」「.agents への書き込み」、05-01「オプション」、05-03「起動とセッションの準備の方針の分離」「依頼の受け取りと終わりの理由」「実行の前の確認（`session/request_permission`）」。
- `.claude/skills/test-audit/SKILL.md`。

## 作るもの

1. **起動・準備の方針の 5 分離**（`crates/benitoiteh/src/cli/args.rs`、`src/cli/exec.rs`、`src/agent/mod.rs`）
   - 入口の種類または準備の方針を表す型（例: `EntryMode { Tui, Exec, Acp }` または `PreparePolicy`）を導入し、設計書 05-03「起動とセッションの準備の方針の分離」の表の 5 項目をそれぞれ判定する。
     - 起動・再開時の確認（プロジェクトの信頼、制限モードでの処理系の SHA-256 変化、制限モードから通常モードへの再開）: `Tui` は画面で確認、`Exec` と `Acp` は保存済みの信頼または `--trust-project` に従い、制限モードの処理系の変化とモード変更の再開は拒む。
     - 実行時の承認の方針: `Tui` は制限モードが `OnRequest`・通常モードが `--approval` または設定値、`Exec` は `Never`、`Acp` は制限モードが `--approval`（`OnRequest` または `Never`、既定 `OnRequest`、制限モードでの `Untrusted` は引数の誤り）・通常モードが `--approval` または設定値（既定 `OnRequest`）。
     - `--allow`・`--allow-agents-write`・`--project-parent`・`--ephemeral`: `Exec` と `Acp` で受け付け、`Tui` では使い方の誤りとする。`AgentConfig` と `ToolContext` で「事前許可の範囲を超える実行を拒むか（`Exec` および `--approval never` の `Acp`）、それとも `Approver` に尋ねるか（`--approval on-request` の `Acp`）」を区別する。
     - 通信の中継（`normal.network = "proxy"`）の未許可ドメインの確認: `Tui` と `Acp` で承認の方針が `Never` でないときに有効にする。`Agent::connect_network_prompts(&self, prompts: Sender<ApprovalPrompt>, wake: Option<WakeCallback>)` を公開（または `pub(crate)`）し、`Tui` と `Acp` の両方から確認の口をつなげるようにする。
     - 秘密の入力（`secrets.askpass`）: `Tui` のみ設定に従い、`Exec` と `Acp` では無効（`Broker` を作らない）とする。
2. **呼び出し単位の一回限りのエフェクト許可**（`crates/benitoiteh/src/approval/mod.rs`、`src/agent/execution.rs`、`src/tools/mod.rs`、`src/tools/benitoite_run.rs`、`src/tools/benitoite_test.rs`）
   - `ApprovalAnswer`（または `ApprovalOutcome`）で、エフェクトの確認について「セッションに記憶する許可（従来の `ApprovalOutcome::Approved`。TUI の `[y]` と ACP の `allow_always`）」と「この呼び出しの 1 回だけ許し、セッションの `confirmed_effects` を更新しない許可（例: `ApprovalAnswer { outcome: ApprovalOutcome::Approved, remember_effects: false, .. }` または専用のメソッド `ApprovalAnswer::approved_once()`）」を区別する。既存の `Record::Approval`（`session.jsonl`）の直列化の互換性を崩さないよう、`ApprovalOutcome` の JSON の綴りは変えないか、`ApprovalAnswer.remember_effects: bool`（既定は `outcome == ApprovalOutcome::Approved`）で制御する。
   - `Invocation<'a>` に、その呼び出し 1 回だけに承認されたエフェクト集合 `pub call_approved_effects: Option<&'a BTreeSet<crate::benitoite::Effect>>`（既定は `None`）を加える。
   - `Agent::approve`（`src/agent/execution.rs`）で：
     - `--allow` が指定されているとき、宣言したエフェクトが `--allow`（一時スクリプトでは確認なしの範囲との和）に収まれば、今までどおり確認なしに実行する。収まらないとき、`approval_policy.approval == Approval::Never`（`exec` または `acp --approval never`）なら今までどおり `allow_denied` で断り（3 回続けば `EffectsNotAllowed`）、`OnRequest`（`acp` の既定）なら `Approver` による確認へ進む。`unread_imports > 0` や `agents_write` で事前許可がないときも同じ規則で、`Never` なら断り、`OnRequest` なら確認へ進む。
     - `Approver` が許可を返したとき、`remember_effects` が真なら今までどおり `self.confirmed_effects` を広げ、偽（一回限り）なら `self.confirmed_effects` を変えずに、その確認の `record.facts.effects` から得た集合を `execute_approved` の `Invocation.call_approved_effects` にだけ渡す。
   - `benitoite_run::execute_script`（`benitoite_scratch` も通る）と `benitoite_test::execute` で、`approval == ApprovalStatus::Approved` の再実行のとき、再検査した `confirmation` が「セッションの `confirmed_effects`（または `retry_automatic`）に収まる」か「`invocation.call_approved_effects` に収まる」ことを確かめ、収まらなければ `EFFECTS_CHANGED` で拒む。再検査そのものと、`--allow-effects` への報告の集合の受け渡しは一切弱めない。
3. **終了理由の分類**（`crates/benitoiteh/src/agent/events.rs`、`src/agent/mod.rs`）
   - `RunReport` に、終了の詳細な分類 `pub stop_reason: StopReason`（例: `StopReason { Completed, Cancelled, MaxTurns, MaxRunTokens, Failed { kind: &'static str, message: String } }`）を持たせる。`Agent::run_controlled` が `AgentError` を捕まえて `Outcome::Failed` に変える箇所で、`AgentError::Limit(text::MAX_TURNS)` は `StopReason::MaxTurns`、`AgentError::Limit(text::MAX_RUN_TOKENS)` は `StopReason::MaxRunTokens`、取り消しは `StopReason::Cancelled`、正常完了は `StopReason::Completed`、そのほかは `StopReason::Failed` として記録する。既存の `RunReport.outcome` と `exec` の終了状態の意味は変えない。
4. **画像バイト列の検証・縮小・保存の入口**（`crates/benitoiteh/src/tools/read/`、`src/agent/mod.rs`）
   - `read` の画像処理（`src/tools/read/image.rs`・`image_helper.rs`）に、ファイルパスではなくメモリ上のバイト列（`&[u8]`）と期待する MIME 型を受け取り、寸法と形式の検証、上限を超える PNG/JPEG の `benitoiteh __image` サンドボックス補助による縮小、セッションの `objects/` への保存を行って `UserPart`（または添付の画像データ）を返す関数を加える。`RunControl`（または `Agent::run_controlled` の入力）に、パスの添付 `attachments: Vec<PathBuf>` に加えて検証用の画像バイト列（または検証済みの画像）を渡せるようにする。

## テスト

`test-audit` の作成時の関門に従う。

- **一回限りのエフェクト確認のテスト**（`tests/agent_loop.rs` または `tests/tools_benitoite_run.rs`）:
  - 1 回目の `benitoite_run`（および `benitoite_test`）で一回限りの許可（`remember_effects: false`）を返したとき、1 回目の実行が再検査を通って成功すること。
  - 続けて同じスクリプトを 2 回目に呼んだとき、`confirmed_effects` に記憶されておらず再び `Approver` に確認が求められること。
  - 既存の `confirmed_effects`（例: `File.Read`）がある状態で、`File.Read, File.Write` のスクリプトを一回限りで許可したとき、その 1 回は実行でき、次の呼び出しでは既存の `File.Read` の記憶だけが保たれている（消えも広がりもしない）こと。
  - 確認の直後、再実行の再検査の前にスクリプトのエフェクトが増えたときは、一回限りの許可でも `EFFECTS_CHANGED` で拒まれること。
- **準備の方針と `--allow` + 確認のテスト**（`tests/args.rs`、`tests/agent_loop.rs`）:
  - `acp` の引数解析で `--allow`・`--allow-agents-write`・`--project-parent`・`--ephemeral`・`--approval on-request|never`（制限モードでは `untrusted` を拒む）が設計書 05-01・05-03 のとおりに判定されること。
  - `--allow File.Read` かつ承認の方針が `OnRequest` のとき、`File.Read` だけのスクリプトは確認なしで動き、`File.Write` を含むスクリプトは `Approver` に確認を求め、`Never` のときは確認せずに `allow_denied` で拒むこと。
- **終了理由と画像バイト列のテスト**:
  - `max_turns` と `max_run_tokens` に達したときの `RunReport.stop_reason` がそれぞれ `MaxTurns`・`MaxRunTokens` になること。
  - PNG/JPEG のバイト列を渡した画像検証・縮小が `benitoiteh __image` を通って `objects/` に保存され、壊れたバイト列や非対応の形式が拒まれること。

## 完了の条件

- `scripts/check.sh --inside-codex` が通る。
- [`docs/plan/10-interfaces.md`](../10-interfaces.md) に本作業で加えた型と関数のシグネチャを追記する。
- 本書の「作ったもの」と「確かめた結果」を記録する。
