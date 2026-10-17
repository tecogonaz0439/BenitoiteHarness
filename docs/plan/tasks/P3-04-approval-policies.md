# P3-04 通常モードの承認の方針

## 目的

通常モードの承認の方針（`untrusted`・`on-request`・`never`）を、`bash`、`write`・`edit`、`escalate` の要求に、設計書 03-03 の表のとおりに掛ける仕組みを作る。確認の答えの形（許可する、このセッションでは同じものを許可する、拒否する、拒否して文を伝える）を、`Approver` の境界で表せるようにする。

道具 `bash` は P3-05 で作る。この作業では、`bash` の確認の種類（`Command`・`Escalate`）と判定を作り、判定の関数のユニットテストで確かめる。`write`・`edit` は、エージェントのループを通して確かめる。対話の画面（P4）はまだないので、答えは台本の `Approver` と、非対話の `exec` の `never` で確かめる。

あわせて、今の `write`・`edit` の確認の不具合を直す。今は、書ける場所をプロジェクトと一時ディレクトリに決め打ちしているので、`read-only` でもプロジェクトに確認なしで書け（`never` でも書ける）、設定で足した書ける場所への書き込みは確認を受ける。守る場所（`.git`・`.benitoiteh`）への書き込みの確認もない。

## 読む設計書

- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」「通常モードの承認の方針」「非対話での扱い」（2026-10-17 に、守る場所、`--no-sandbox`、サンドボックスのないときの `escalate` の扱いを加えた）
- [道具](../../design/01-agent/01-03-tools.md)の「書く道具のパスの規則」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「通常モードの方針」
- [セッション](../../design/01-agent/01-05-sessions.md)の記録と「再開」
- 既存のもの: `src/approval/`、`src/agent/`（`mod.rs` の `Agent::new`、`execution.rs` の確認の流れ、`events.rs`）、`src/tools/`（`mod.rs` の `ShellContext`・`ApprovalRequest`・`ApprovalStatus`、`write_path.rs` の `WriteTarget::approval`、`definitions.rs` の `ApprovalRequirement`）、`src/session/`（`ApprovalOutcome`、`Record::Approval`、集計）、`src/ui/`（`text.rs` の `approval`）、`src/cli/exec.rs`、`tests/agent_loop.rs`、`tests/exec.rs`、`tests/session.rs`

## 作るもの

### 1. 型と置き場所

- `approval` に、確認の種類 `ApprovalKind`（`WriteScope`、`Command`、`Escalate`）、確認の事実 `ApprovalFacts`（`command`、`cwd`、`path`、`sandbox_policy`。どれも省略できる）、承認の方針の値 `ApprovalPolicy { approval: Approval, sandbox: Sandbox, sandboxed: bool }` を置く。`tools` は `approval` の型を使い、逆に依存しない。
- `tools::ApprovalRequest` を `{ kind, facts, justification: Option<String>, summary }` にする（`summary` は事実の要約として残す）。
- `ShellContext` に、選んだサンドボックスの方針（`sandbox: Sandbox`）と、実際に掛けているか（`sandboxed: bool`。`--no-sandbox` で動かすときは偽）を加える。
- `Agent` は、`Approver` と `ApprovalPolicy` を外から受け取る（例: `Agent::with_approvals(self, Box<dyn Approver>, ApprovalPolicy)`）。受け取らないときは、`NonInteractiveApprover` と、モードに応じた既定（通常モードは `never` と `workspace-write`）を使う。`cli/exec.rs` は `ShellContext` の値から `ApprovalPolicy` を作って渡す。

### 2. 判定の関数

`approval::decide(policy: &ApprovalPolicy, kind: ApprovalKind, facts: &ApprovalFacts, allowed: &BTreeSet<String>) -> Decision`（`Decision` は `Allow`・`Ask`・`Deny`）を作る。表は 03-03 のとおりとし、次の組をすべて定める。サンドボックスの場合は、`read-only`、`workspace-write`、`danger-full-access`、`--no-sandbox` で掛けていない場合（`sandboxed` が偽）の四つとする。

- `WriteScope`（書ける場所の外か、書ける場所の中の守る場所への `write`・`edit`）: `danger-full-access` なら `Allow`。ほかは `untrusted`・`on-request` で `Ask`、`never` で `Deny`。`--no-sandbox` でも、選んだ方針の表を使う。
- `Command`（`escalate` でない `bash`）: `untrusted` で、`facts.command` が `allowed` にあれば `Allow`、なければ `Ask`。`on-request`・`never` は `Allow`。
- `Escalate`（`escalate` が真の `bash`）: サンドボックスを掛けていないとき（`danger-full-access` か、`sandboxed` が偽）は、`Command` と同じに判定する。掛けているときは、`untrusted`・`on-request` で `Ask`、`never` で `Deny`。

### 3. 書く道具の書ける場所

- `write`・`edit` が書ける場所の外かを決めるとき、プロジェクトと一時ディレクトリの決め打ちをやめる。選んだサンドボックスの方針の書ける場所（`read-only` は一時ディレクトリだけ、`workspace-write` はプロジェクトと一時ディレクトリと設定の `writable_roots`）を使う。この一覧は `ShellContext` かそれに当たる値から渡す。
- 書ける場所の中でも、実のパスの途中の要素に `.git`・`.benitoiteh`（ASCII の大文字と小文字を区別しない）があれば、`WriteScope` の確認の対象とする（01-03 の「書く道具のパスの規則」）。

### 4. 確認の流れ（`agent/execution.rs`）

- 今の流れ（最初の実行は何もせずに確認が要ることを返し、ループが答えを得てから `ApprovalStatus::Approved` で実行し直す）を保つ。決め打ちの `ApprovalKind::WriteScope` をやめ、道具が返す `ApprovalRequest` の種類を使う。
- ループは、`decide` の結果で分ける。`Allow` はイベントを出さずに `Approved` で実行し直す。`Deny` は確認のイベントを出さずに、種類ごとの方針による拒否の文を道具の結果にする。`Ask` は `Approver` に尋ねる。
- このセッションで許したコマンドの集合（`BTreeSet<String>`）は `Agent` が持つ（道具からは触れない）。再開では戻さない（01-05 の「再開」）。
- 確認を要しうる道具（`write`・`edit`、P3-05 の `bash`）は、並べて実行しない（今の並べて実行する経路は確認を扱わない）。そのことをコメントに書く。使われていない `ApprovalRequirement` は、除くか、この規則を表す形に改める。

### 5. 答えの形

- `Approver` は `ApprovalAnswer { outcome: ApprovalOutcome, message: Option<String> }` を返す。`ApprovalOutcome` は値を持たない列挙のまま（`Copy` のまま）とし、`Approved`・`ApprovedForSession`・`Denied`・`DeniedWithMessage`・`Cancelled` にする。
- `ApprovedForSession` は、`Command` の確認でだけ、集合に `facts.command` を加える。`Escalate` と `WriteScope` では一回の許可として扱う（03-03 の【決定】）。
- `DeniedWithMessage` の道具の結果は、利用者による拒否の文の後に、利用者の文であることを示す形で利用者の文を続ける（例: `{denial}\nMessage from the user: {text}`。`approval/text.rs` に置く）。
- 文は `approval/text.rs` に、方針による拒否（種類ごと。`never` の方針によることを示す）と、利用者による拒否（種類ごと）を分けて置く。今の `DENIED`（"non-interactive run" と書く）は、P4 で対話の利用者が `never` を選べるので、方針による拒否の文に改める。オーケストレータが取り込む前に文面を見直す。

### 6. 記録と集計

- 記録の形式の版は 1 のままとする。列挙に値を足す（`ApprovalKind` と `ApprovalOutcome`）ほかは、`Record::Approval` に新しい欄（`kind`、事実、`justification`、`user_message`）を `Option` か `#[serde(default)]` で足す。前の形の `approval` の行を読めることを、再開のテストで確かめる。
- 集計は、`ApprovedForSession` を許可、`DeniedWithMessage` を拒否として数える。方針による拒否（`Deny`）は、新しい欄（例: `#[serde(default)] policy_denied`）で別に数える。`tests/session.rs` の集計の JSON の期待を合わせて直す。
- 確認のイベント（`approval_request`・`approval_result`）にも、種類、事実、`justification`、利用者の文を、別々の欄で入れる。いつも `None` の `ApprovalRequest.decision` は除いてよい。

## 触るファイル

- `crates/benitoiteh/src/approval/`
- `crates/benitoiteh/src/agent/`（`mod.rs`、`execution.rs`、`events.rs`）
- `crates/benitoiteh/src/session/`（答えと記録と集計）
- `crates/benitoiteh/src/tools/`（`mod.rs`、`write_path.rs`、`definitions.rs`、書く道具）
- `crates/benitoiteh/src/ui/`（`text.rs` の答えの表示）
- `crates/benitoiteh/src/cli/exec.rs`
- `crates/benitoiteh/tests/agent_loop.rs`、`tests/exec.rs`、`tests/session.rs`、`tests/tools_write.rs`

## テスト

- 判定の関数のユニットテスト: 三つの種類、三つの承認の方針、四つのサンドボックスの場合のすべての組が、2 のとおりになる。
- このセッションでの許可: `Command` で `ApprovedForSession` と答えた後、同じ文字列は `Allow`、一字でも違う文字列は `Ask` になる。`Escalate` と `WriteScope` では、次も `Ask` になる。
- `write`・`edit`（エージェントのループと台本の `Approver` で）:
  - `read-only` では、プロジェクトへの書き込みが `on-request` で確認を受け、`never` で拒否され、ファイルができない（今の不具合の直しの確かめ。安全の性質）。
  - `workspace-write` では、プロジェクトの中と設定で足した書ける場所は確認なしで書け、プロジェクトの外は確認を受ける。
  - 書ける場所の中の `.git/x`・`.GIT/x`・`.benitoiteh/x` は、`on-request` で確認を受け、`never` で拒否される。`danger-full-access` では確認なしで書ける。
  - 尋ねなかったときに `Approver` が呼ばれない（台本の `Approver` の呼び出しの記録で確かめる）。
  - `DeniedWithMessage` で、道具の結果に拒否の文と利用者の文が入り、書かない。
  - 道具の結果や LLM の文に、確認の答えの形の文（`approved` など）を書いても、確認が進まない（安全の性質のテスト。07-01）。
- 確認のイベントと記録に、事実と `justification` が別の欄で入る。前の形の記録を読める。
- 既存のテストの直し:
  - `tests/exec.rs` の `invalid_project_configuration_is_reported_and_noninteractive_confirmation_is_denied` は、`--sandbox danger-full-access` で `.benitoiteh/` への書き込みの拒否を確かめているが、新しい表では許される。`--sandbox workspace-write --no-sandbox`（サンドボックスを掛けられないことにする差し替えを使う）の形に直す。
  - `tests/agent_loop.rs` の `non_interactive_outside_write_emits_denial_…` は、`never` の方針による拒否になり、確認のイベントを出さない形に期待を直す。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、`decide`・`ApprovalPolicy`・`ApprovalFacts`・`ApprovalAnswer` のシグネチャと、拒否の文の全文を書く。
