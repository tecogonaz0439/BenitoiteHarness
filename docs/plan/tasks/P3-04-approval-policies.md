# P3-04 通常モードの承認の方針

## 目的

通常モードの承認の方針（`untrusted`・`on-request`・`never`）を、`bash`、`write`・`edit`、`escalate` の要求に、設計書 03-03 の表のとおりに掛ける仕組みを作る。確認の答えの形（許可する、拒否する、拒否して LLM に伝える文を書く、このセッションでは同じものを許可する）を、`Approver` の境界で表せるようにする。道具 `bash` は P3-05 で作るので、この作業では `bash` の確認の種類（`Command`・`Escalate`）と判定を作り、判定の関数のユニットテストで確かめる。`write`・`edit` は、エージェントのループを通して確かめる。対話の画面（P4）はまだないので、答えは `Approver` を実装したテストの台本と、非対話の `exec` の `never` で確かめる。

## 読む設計書

- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」「通常モードの承認の方針」「非対話での扱い」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「通常モードの方針」（`danger-full-access` と `escalate`）
- [道具](../../design/01-agent/01-03-tools.md)の「bash（通常モード）」「書く道具のパスの規則」
- [セッション](../../design/01-agent/01-05-sessions.md)の記録（確認と答えを記録に書くこと）
- 既存のもの: `src/approval/`（`Approver`、`ApprovalRecord`、`ApprovalKind`、`NonInteractiveApprover`）、`src/agent/execution.rs`（確認を尋ねる箇所）、`src/tools/`（`ApprovalRequirement`、P3-02 の `bash`）、`src/session/`（`ApprovalOutcome` と記録）、`src/config/`（`Approval`、`Sandbox`）、`src/cli/exec.rs`

## 作るもの

0. **確認の仕組みの土台**（P3-02 の点検で見つかった欠け）:
   - `Agent` が持つ `Approver` を差し替えられるようにする（例: `Box<dyn Approver>` を受け取る関数）。`exec` は `NonInteractiveApprover` を渡す。
   - `agent/execution.rs` が `ApprovalKind::WriteScope` を決め打ちしている箇所を、道具が返す確認の種類を使う形にする。流れは今と同じく、最初の実行は何もせずに確認が要ることを返し、ループが尋ねてから `ApprovalStatus::Approved` で実行し直す。
   - 判定が「確認せずに拒否する」のときは、確認のイベントを出さずに、種類ごとの拒否の文を道具の結果にする。
   - 拒否の文（今の `denial_message()` は書き込みに合わせた文である）を、確認の種類ごとに置く（英語。`approval/text.rs`。オーケストレータが取り込む前に文面を見直す）。
1. **方針の判定を一か所に置く**: `approval` に、方針（`Approval` と `Sandbox`）と確認の種類から、「確認せずに許す」「尋ねる」「確認せずに拒否する」のどれかを返す関数を作る。表は 03-03 の「通常モードの承認の方針」のとおりとする。
   - `bash`（`escalate` でない）: `untrusted` は尋ねる。`on-request` と `never` は確認せずに許す（サンドボックスの中で動く）。
   - `write`・`edit` の書ける場所の外への書き込み: `untrusted` と `on-request` は尋ねる。`never` は確認せずに拒否する。`danger-full-access` のときは、どの承認の方針でも確認せずに許す。書ける場所の中は確認しない（今のとおり）。
   - `escalate` の要求: `untrusted` と `on-request` は尋ねる。`never` は確認せずに拒否する。
   - 制限モードの確認は、この作業では変えない。
2. **答えの形**: `ApprovalOutcome` に、拒否して LLM に伝える文を持つ答えと、このセッションでは同じものを許可する答えを加える（名前は任せる）。
   - 拒否して文を伝える答えでは、道具の結果を `approval::denial_message()` に続けて利用者の文（印を付けて、LLM の文と分かる形）とする。
   - このセッションでは同じものを許可する答えは、`untrusted` の `bash` の確認でだけ受け付ける。同じかは、コマンドの文字列の完全な一致で判定し、許可したコマンドの集合をセッションの間だけ保つ。`escalate` と書ける場所の外への書き込みでは、この答えを一回の許可として扱う（03-03 の【決定】。`escalate` の承認はその一回だけに効く）。
   - 記録の形（`session` の `ApprovalOutcome`）に新しい答えを加え、記録の形式の版を変えるかは、既存の記録の読み込みとの関係で判断して報告に書く。
3. **確認の要求の中身**: `ApprovalRecord`、`AgentEvent` の確認のイベント、`session` の確認の記録に、確認の種類、`benitoiteh` が作った事実（コマンドの全文、作業ディレクトリ、書く場所の絶対パス、サンドボックスの方針の名前）、LLM が書いた説明の文（`justification: Option<String>`）を、別の欄で持たせる（03-03 の「確認の共通の規則」）。今の一つの `summary` の欄は、事実の要約として残してよい。画面の示し方は P4 で作る。
4. **起動の入口**: `cli/exec.rs` は、通常モードの承認の方針を `never` に固定したまま（P3-02）、1 の判定を使う。承認の方針とサンドボックスの方針は、P3-02 の `ShellContext` から読む。`--sandbox danger-full-access` のときの書き込みの扱いも 1 に従う。
5. 確認と答えを、今と同じくセッションの記録とイベント（`approval_request`・`approval_result`）に残す。

## 触るファイル

- `crates/benitoiteh/src/approval/`
- `crates/benitoiteh/src/agent/`（`mod.rs`、`execution.rs`、`events.rs`）
- `crates/benitoiteh/src/session/`（答えの形と記録）
- `crates/benitoiteh/src/tools/`（`bash` と書く道具の確認の要求）
- `crates/benitoiteh/src/cli/exec.rs`
- `crates/benitoiteh/tests/`（エージェントのループと道具のテスト）

## テスト

模擬の提供者と、答えを台本で返す `Approver` で確かめる。

- 1 の表のすべての組（三つの承認の方針と、`danger-full-access` を含む二つのサンドボックスの方針、三つの種類）で、尋ねるか、確認せずに許すか、確認せずに拒否するかが表のとおりになる（判定の関数のユニットテスト）。`write`・`edit` については、エージェントのループを通して、尋ねなかったときに `Approver` が呼ばれないことも確かめる。
- このセッションでの許可の集合: `Command` の種類で「このセッションでは同じものを許可する」と答えた後、同じ文字列は尋ねない判定になり、一字でも違う文字列は尋ねる判定になる。`Escalate` と書き込みでは、この答えを一回の許可として扱い、次も尋ねる判定になる（集合と判定のユニットテスト。`bash` を通した確かめは P3-05 で行う）。
- 拒否して文を伝える答えで、道具の結果に拒否の文と利用者の文が入り、コマンドは動かない。
- 道具の結果や LLM の文に、確認の答えの形の文（`approved` など）を書いても、確認が進まない（安全の性質のテスト。07-01）。
- 確認の要求に、`benitoiteh` の事実と `justification` が別の欄で入る。
- 確認と答えがセッションの記録に残り、再開の読み込みが壊れない。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、方針の判定の関数と答えの形のシグネチャを書く。
