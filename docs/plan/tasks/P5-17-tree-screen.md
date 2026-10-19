# P5-17 `/tree` の画面

## 目的

設計書 01-05「セッションの木」と 05-02「`/` で始まるコマンド」の `/tree` を、対話の画面に加える。P5-16 で作った、記録の木と、枝を移る操作（依頼の直前と枝の先端へ移る。ファイルも戻せる）を使う。この作業で本物の LLM の提供者を呼ばない。P5-16 を取り込んだ `main` から始める。

## 読む設計書

- [セッション](../../design/01-agent/01-05-sessions.md)の「セッションの木」「巻き戻し」
- [対話の画面](../../design/05-interface/05-02-tui.md)の「出力の示し方」「確認の画面」「`/` で始まるコマンド」
- 既存のもの: P5-16 で作った木の型と枝を移る操作、`crates/benitoiteh/src/ui/tui/commands.rs`（コマンドの一覧と引数の検査）、`selection.rs`（`/rewind` の選択の画面）、`confirmation.rs`（巻き戻しの確認）、`worker.rs`・`runtime.rs`（仕事の受け渡しと、巻き戻しの間の Ctrl-C の扱い）、`text.rs`（`HELP` と行の文言）、`mod.rs`（`restore_input`）

## 作るもの

### 1. 木の一覧の型（`agent` か `session` の側）

- 記録から、画面に示す行の並びを作る関数を公開する。行は、会話の始め（根）、利用者の依頼の節点（依頼の文の先頭の 60 文字。制御の文字は今と同じく見える形にする）、枝の先端、今の位置の四種とする。依頼の節点が同時に枝の先端でもあるとき（依頼を記録した直後に止まったなど）は一行とし、両方の印を付ける。今の位置が依頼でも先端でもない節点（依頼の直前へ移った後など）のときは、その位置に「今の位置」の行を加える。
- 字下げは、子を二つ以上持つ節点の下で一段深くする。枝は記録の順に並べる。親を辿る処理は再帰で書かない。

### 2. `/tree` の画面

- `/tree` で一覧を開く。上下で選び、Enter で確定し、ESC で閉じる（今の `/rewind` の選択の画面と同じ操作）。今の位置の行に印を付ける。
- 依頼の行を選ぶとその直前へ、先端の行を選ぶとその先端へ移る。今の位置を選んだときは何もしない。
- 移る前に、`/rewind` と同じ確認の画面で、「会話とファイル」「会話だけ」の選択（移る先に時点がなければ「会話だけ」だけ）と、戻せないものの一覧（今の巻き戻しと同じく、時点より後の記録の中の実行を、枝をまたいで多めに示す）を示す。
- ファイルを戻す移動の間は、今の巻き戻しと同じく、仕事中の印（`worker.rewinding`）を立て、Ctrl-C と「戻す処理を待っている」の扱いを同じにする。
- 依頼の直前へ移ったら、その依頼の文を入力の欄に戻す（P5-16 と同じ戻し方）。移った後は、移ったこと（と、戻したファイルの数）を知らせの一行で示す。

### 3. コマンドの一覧

- `commands.rs` のコマンドの名前の一覧と引数の検査に `tree` を加える（引数は取らない）。`HELP` の文に加える。

## 触るファイル

- P5-16 の木の型の所（行の並びを作る関数）
- `crates/benitoiteh/src/ui/tui/commands.rs`、`selection.rs`、`confirmation.rs`、`worker.rs`、`runtime.rs`、`mod.rs`、`text.rs`
- テスト: `src/ui/tui/tests.rs`、`tests/tui/commands.rs`（コマンドの一覧を数えるテスト: 44、61、75、83 行あたり）、`tests/tui/pty.rs`（要れば）

## テスト

- 行の並び: 二つの枝を持つ記録から、根、依頼、先端、今の位置の行が、字下げと印とともに正しく並ぶこと。依頼と先端を兼ねる節点が一行になること。
- 画面: `/tree` で一覧が開き、依頼を選ぶと確認を経てその直前へ移り、入力の欄に依頼の文が戻ること。先端を選ぶとその枝で続けられること。今の位置を選ぶと何も起きないこと。ESC で閉じること。
- `HELP` と、コマンドの一覧のテストが `tree` を含むこと。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、`/tree` の画面の例（二つの枝の記録で）を書く。

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。行の番号は `main`（`3447151`）のもの。

1. **P5-16 が用意したもの**: `SessionTree`（`src/session/replay.rs:109-257`）の `tip`・`from_records`・`path`・`entry`・`leaves()`・`request_checkpoint`・`branch_checkpoint`・`path_nodes()` と、`Agent::branch_targets()`（`src/agent/rewind.rs:124-170`。依頼ごとの `BeforeRequest` と葉ごとの `Tip` を平たく返す）、`Agent::switch_branch()`。足りないのは、子の列挙、行の型、行を作る関数である。
   - `SessionTree` に、節点と親と記録の位置を列挙する口（例: `pub(crate) fn nodes(&self)`）を加える。
   - 行を作る関数は、記録と木を受ける純粋な関数（例: 新しい `src/session/tree.rs` の `tree_rows`）とし、`Agent` の側の口（例: `Agent::branch_tree()`）が、一度の `read_records` の中で、各行に移る先（`BranchTarget`）と時点の有無を付ける。行の型は公開する（P5-19 の `/fork` が使い回す）。
   - 今の位置は、記録から作った木の `tip` を使う（`Agent::current_node` と一致する）。
   - 木は保持しない。一覧、確かめ、実行の各段で、働き手の仕事の中で一度ずつ読む（キーを押すたびには読まない）。
2. **行の並べ方**: 兄弟は節点の番号の順に並べ、明示のスタックで前順に辿る（再帰しない）。字下げの深さは、子を二つ以上持つ祖先（根を含む）の数とし、一段を 2 空白とする。今の位置が依頼でも先端でもない節点のときは、「今の位置」の行を、前順のその節点の位置（子の行より前）に置く。一覧は上から記録の順に並べ、始めのカーソルは今の位置の行に置く。
3. **行の文言**（`text.rs` の定数）: 根は `(conversation start)`。依頼の行は依頼の文の先頭 60 文字（`char` で数え、改行を含む全文から切り、60 を超えたら `...` を付ける）。印は行末に ` [tip]`・` [current]` と付ける。今の位置だけの行は `[current]`。切った後で `SafeText::strict(.., false)`（`selection.rs:49` と同じ）を通す。
4. **Enter で何をするか**:
   - 依頼の行: その依頼の直前（`BeforeRequest`）へ移る。依頼と先端を兼ねる行も `BeforeRequest` とする（応答のない依頼の先端に移るより、文を戻して出し直すほうが役に立つため）。
   - 先端の行: その先端（`Tip`）へ移る。
   - 今の位置の行、根の行、今の先端を兼ねる行: 仕事を送らずに閉じる（何もしない）。`switch_branch(Tip(今の先端), ..)` は何もせず既定の報告を返すので、画面から送らない。今の位置の印を付けた依頼の行（今の位置が依頼の節点で、葉でないとき）は、通常の `BeforeRequest` の移動とする。
   - 節点が一つもないときは一覧を開かず、`The session tree is empty.` を示す。
5. **確認までの段**: `/rewind` と同じく、一覧 → 範囲の選択の段 → 確認の画面とする（上の「移る前に、確認の画面で選択」は、この順に読み替える）。範囲は、移る先に時点があれば「会話とファイル」「会話だけ」、なければ「会話だけ」だけとし、選択肢が一つでも段を示す（`/rewind` の `scopes()` と同じ）。
   - `RewindPreview` と `rewind_preview()`（`rewind.rs:57-77`）は、時点の一覧にない ID と、会話を戻せない時点を拒むので使い回さない。新しく `Agent::branch_preview(target, scope)` を作り、移る先の情報、時点（`CheckpointInfo`。不完全の警告に使う）、範囲、戻せないものの一覧を返す。戻せないものの一覧は `irreversible`（`rewind.rs:337-421`）をそのまま使う（時点から記録の末尾までを枝を問わず走査するので、「枝をまたいで多めに示す」を満たす）。時点がない移る先では空とする。
   - 画面の保留の状態は、`rewind_preview` の一つから、`/rewind` と `/tree` の二つを持てる形（例: `enum PendingMove { Rewind(..), Branch(..) }`）に広げる。`IncompleteBefore` を受けたときの `[f]` の開き直し（`worker.rs:365-372`）も、両方で働くようにする。
   - 確認の画面は、`open_rewind` の署名を変えずに、枝の移動の口を足す（`src/ui/tui/tests.rs` の既存の確認のテストを直さずに済ませるため）。題 `╭─ Move to branch?`、キー `[y] move  [n] cancel`、取り消し `Branch move cancelled.`、処理中 `Moving...`、結果 `Moved to the selected branch.`（ファイルを戻したときは、戻した・消した・作った数を添える）。`REQUEST_RESTORED`、`WAIT_REWIND`、戻せない操作の示し方は使い回す。
6. **仕事中の印**: 枝の移動の仕事は、範囲によらず `rewinding` を立てる（会話だけでも、移る前の時点と `branch` の記録を書くため）。上の「ファイルを戻す移動の間は」は、この形に読み替える。判定は `matches!(job, Job::Rewind{..} | Job::SwitchBranch{..})` のように一つにまとめ、Ctrl-C・ESC・`/quit`・終了の合図の扱い（`runtime.rs` の 100-113、166、242、264-267、344-361 行）を `/rewind` と同じにする。`runtime.rs:304-335` の `match` に新しい変種の腕を加える。
7. **型の名前の例**: `Command::Tree`、`Command::PreviewBranch(BranchTarget, RewindScope)`、`Job::{ListBranches, PreviewBranch, SwitchBranch}`、`Selection::{Branches { rows, index }, BranchScope { row, index }}`。
8. **直すテスト**: `tests/tui/commands.rs` の 44 行（表に `/tree`）、61 行（引数を拒む配列）、71-79 行（処理中と後続の配列）、83 行（処理中の Enter）。`HELP` が `tree` を含むことは新しいテストで確かめる（212 行は 1 行目だけを見ている）。`/tree` の画面のテストは、`tests/tui/commands.rs:530-620` の `/rewind` の流れを写し、二つの枝の記録は `tests/rewind.rs:1319` と 1602 行を参考に作る。行の並びのテストは `tests/session.rs` か `tests/rewind.rs` に置く。pty のテストは加えなくてよい。
