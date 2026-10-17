# P3-04 通常モードの承認の方針

## 目的

通常モードの承認の方針（`untrusted`・`on-request`・`never`）を、`bash`、`write`・`edit`、`escalate` の要求に、設計書 03-03 の表のとおりに掛ける。確認の答えの形（許可する、拒否する、拒否して LLM に伝える文を書く、このセッションでは同じものを許可する）を、`Approver` の境界で表せるようにする。対話の画面（P4）はまだないので、答えは `Approver` を実装したテストの台本と、非対話の `exec` の `never` で確かめる。

## 読む設計書

- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」「通常モードの承認の方針」「非対話での扱い」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「通常モードの方針」（`danger-full-access` と `escalate`）
- [道具](../../design/01-agent/01-03-tools.md)の「bash（通常モード）」「書く道具のパスの規則」
- [セッション](../../design/01-agent/01-05-sessions.md)の記録（確認と答えを記録に書くこと）
- 既存のもの: `src/approval/`（`Approver`、`ApprovalRecord`、`ApprovalKind`、`NonInteractiveApprover`）、`src/agent/execution.rs`（確認を尋ねる箇所）、`src/tools/`（`ApprovalRequirement`、P3-02 の `bash`）、`src/session/`（`ApprovalOutcome` と記録）、`src/config/`（`Approval`、`Sandbox`）、`src/cli/exec.rs`

## 作るもの

1. **方針の判定を一か所に置く**: `approval` に、方針（`Approval` と `Sandbox`）と確認の種類から、「確認せずに許す」「尋ねる」「確認せずに拒否する」のどれかを返す関数を作る。表は 03-03 の「通常モードの承認の方針」のとおりとする。
   - `bash`（`escalate` でない）: `untrusted` は尋ねる。`on-request` と `never` は確認せずに許す（サンドボックスの中で動く）。
   - `write`・`edit` の書ける場所の外への書き込み: `untrusted` と `on-request` は尋ねる。`never` は確認せずに拒否する。`danger-full-access` のときは、どの承認の方針でも確認せずに許す。書ける場所の中は確認しない（今のとおり）。
   - `escalate` の要求: `untrusted` と `on-request` は尋ねる。`never` は確認せずに拒否する。
   - 制限モードの確認は、この作業では変えない。
2. **答えの形**: `ApprovalOutcome` に、拒否して LLM に伝える文を持つ答えと、このセッションでは同じものを許可する答えを加える（名前は任せる）。
   - 拒否して文を伝える答えでは、道具の結果を `approval::denial_message()` に続けて利用者の文（印を付けて、LLM の文と分かる形）とする。
   - このセッションでは同じものを許可する答えは、`untrusted` の `bash` の確認でだけ受け付ける。同じかは、コマンドの文字列の完全な一致で判定し、許可したコマンドの集合をセッションの間だけ保つ。`escalate` と書ける場所の外への書き込みでは、この答えを一回の許可として扱う（03-03 の【決定】。`escalate` の承認はその一回だけに効く）。
   - 記録の形（`session` の `ApprovalOutcome`）に新しい答えを加え、記録の形式の版を変えるかは、既存の記録の読み込みとの関係で判断して報告に書く。
3. **確認の要求の中身**: `ApprovalRecord` に、`benitoiteh` が作った事実（コマンドの全文、作業ディレクトリ、書く場所の絶対パス、サンドボックスの方針の名前）と、LLM が書いた説明の文（`justification`）を、別の欄で持たせる（03-03 の「確認の共通の規則」）。画面の示し方は P4 で作る。
4. **起動の入口**: `cli/exec.rs` は、通常モードの承認の方針を `never` に固定したまま（P3-02）、1 の判定を使う。`--sandbox danger-full-access` のときの書き込みの扱いも 1 に従う。
5. 確認と答えを、今と同じくセッションの記録とイベント（`approval_request`・`approval_result`）に残す。

## 触るファイル

- `crates/benitoiteh/src/approval/`
- `crates/benitoiteh/src/agent/execution.rs`、要れば `src/agent/events.rs`
- `crates/benitoiteh/src/session/`（答えの形と記録）
- `crates/benitoiteh/src/tools/`（`bash` と書く道具の確認の要求）
- `crates/benitoiteh/src/cli/exec.rs`
- `crates/benitoiteh/tests/`（エージェントのループと道具のテスト）

## テスト

模擬の提供者と、答えを台本で返す `Approver` で確かめる。

- 1 の表のすべての組（三つの承認の方針と、`danger-full-access` を含む二つのサンドボックスの方針、三つの種類）で、尋ねるか、確認せずに許すか、確認せずに拒否するかが表のとおりになる。尋ねなかったときに `Approver` が呼ばれない。
- `untrusted` で「このセッションでは同じものを許可する」と答えた後、同じ文字列のコマンドは尋ねずに動き、一字でも違うコマンドは再び尋ねる。
- `escalate` に「このセッションでは同じものを許可する」と答えても、次の同じ要求で再び尋ねる。
- 拒否して文を伝える答えで、道具の結果に拒否の文と利用者の文が入り、コマンドは動かない。
- 道具の結果や LLM の文に、確認の答えの形の文（`approved` など）を書いても、確認が進まない（安全の性質のテスト。07-01）。
- 確認の要求に、`benitoiteh` の事実と `justification` が別の欄で入る。
- 確認と答えがセッションの記録に残り、再開の読み込みが壊れない。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、方針の判定の関数と答えの形のシグネチャを書く。
