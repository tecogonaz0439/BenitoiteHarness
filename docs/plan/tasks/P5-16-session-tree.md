# P5-16 セッションの木（記録と、枝を移る操作）

## 目的

設計書 01-05「セッションの木」に 2026-10-19 に決めた【決定】のうち、記録の中に木を保つ部分を実装する（BH-016 を閉じた決定）。

- 会話を作る記録に節点の番号を付け、枝を移る記録（`branch`）を加える。
- 記録から、今の先端までの道で会話を組み立てる（再開も同じ）。
- 会話を戻す操作（`/rewind` の「会話を戻す」）を、新しい枝を始める操作に改め、圧縮より前の時点にも戻れるようにする。
- 時点（巻き戻しの記録）を、会話の項目の数と世代の代わりに、節点に結び付ける。
- エージェントに、どの枝の依頼の直前と枝の先端にも移れる操作を設ける（画面から使うのは次の作業）。

対話の画面の `/tree` は次の作業（P5-17）で、新しいセッションに写す `/fork` はその次（P5-19）で行う。枝の要約と、節点の名前は作らない。この作業で本物の LLM の提供者を呼ばない。

## 読む設計書

- [セッション](../../design/01-agent/01-05-sessions.md)の「セッションの記録」「再開」「巻き戻し」（とくに「会話を戻すとき」）「セッションの木」
- [対話の画面](../../design/05-interface/05-02-tui.md)の「`/` で始まるコマンド」「確認の画面」
- [文脈の管理](../../design/01-agent/01-04-context.md)の「圧縮」「推論の内容の扱い」
- 既存のもの: `crates/benitoiteh/src/session/record.rs`（`Record`、`FORMAT_VERSION`）、`src/session/replay.rs`（`replay_records`、`fill_missing` の扱い）、`src/session/resume.rs`（`ResumeState`）、`src/rewind/`（`Checkpoint`、`CheckpointInfo`、`checkpoints`、`advance_generation`、世代）、`src/agent/rewind.rs`（`rewind_preview`、`rewind`）、`src/agent/mod.rs`（会話への追加 `push_item`、記録の書き込み）、`src/agent/compaction/`（圧縮の記録）、`src/ui/tui/`（`commands.rs` の `/rewind`、`confirmation.rs` の巻き戻しの確認）、`src/cli/exec.rs`（再開）

## この作業が認める型の変更

- 記録の形式の版（`FORMAT_VERSION`）を 2 に上げる。版 1 の記録は再開できない（設計書のとおり、試作の間は前の版の読み込みを保証しない）。`sessions` の一覧は、版 1 の記録を、読めない理由とともに示す（今の扱い）。
- 会話を作る記録（`user`、`assistant`、`tool_result`、`compaction`、`images_removed`）に、節点の番号の欄（例: `node: u64`。セッションの中で 1 から増える）を加える。
- `Record` に、枝を移る記録（例: `Branch { to: Option<u64>, from: Option<u64>, checkpoint: Option<String> }`。`to` は移る先の節点（会話の始めなら `None`）、`from` は離れる枝の先端、`checkpoint` は移る前に記録した時点の ID）を加え、今の `Rewind { items, checkpoint }` を置き換える。
- `Checkpoint` の `items` と `generation` を、記録したときの今の先端の節点（例: `node: Option<u64>`）に置き換える。

## 作るもの

### 1. 節点と枝の記録

- 会話を作る記録を書くとき、セッションの中で一つずつ増える番号を付ける。再開したセッションでは、記録の最大の番号の続きから数える。
- 各節点の親は、記録の上で直前の今の枝の節点とする（記録に書かない）。`branch` の記録の後は、その `to` が親になる。
- 道の会話には、今と同じく、利用者の依頼・応答・道具の結果・圧縮・画像を外した記録を、記録の順に当てる。

### 2. 記録の検査と会話の組み立て（`replay_records`）

- 記録を一度読み、節点の番号から、その親と記録の位置への表を作る。`branch` の `to` と `from` が、その `branch` より前に現れた節点でないとき（未知の番号、後の番号）と、節点の番号が増え続けていないときは、記録の形の誤りとして読み込みを断る（`SessionError::Invalid`。文言は今の形に合わせる）。
- 今の先端は、最後の `branch` の `to`（その後に節点があれば最後の節点）とする。`branch` がなければ最後の節点とする。
- 先端から親を辿って道を作り、道の上の最後の `compaction` の会話から始め、その後の道の上の節点を順に当てる。ほかの枝の `compaction` と `images_removed` は使わない。
- 結果のない道具の呼び出しの補い（今の `fill_missing`）は、組み立てた道に対して行う。補うのは、先端の枝の最後の応答の呼び出しだけで、ほかの枝の呼び出しには補わない。
- `ResumeState` が持つ値（番の最大の番号、推論の区間、ファイルの記録など）は、今のまま記録の全体から求めてよいが、会話と推論の区間は道に合わせる（道にない応答の推論の区間を使わない）。

### 3. 枝を移る操作（`agent`）

- 移るのは、処理をしていないときだけとする（今の `rewind` と同じ条件）。
- 移る先は、(a) 今の枝を含むどの枝でも、利用者の依頼（`user` の `Request`）の節点の直前（その親の節点。会話の始めなら `None`）と、(b) 枝の先端（子のない節点）とする。(a) では、その依頼の文を画面の入力の欄に戻す。
- 移る前に、今の状態を時点（今の `BeforeRewind` に当たる理由）として記録し、`branch` の記録の `checkpoint` に書く。
- 「会話とファイル」を選んだときは、移る先に対応する時点のファイルを戻す。対応する時点は、(a) ではその依頼を受けたときの `Request` の時点、(b) ではその枝を離れたときに記録した時点（その枝の先端を `from` とする `branch` の `checkpoint`）とし、なければ「会話だけ」だけを示す。ファイルの戻し方、戻せないものの表示、不完全な時点の扱いは今の巻き戻しの規則のまま。
- 会話だけを移したときも、ファイルを戻したときも、読んだ版の記録をすべて消す。
- 移った後は、`previous_last_item`（キャッシュの区切り）と圧縮の数え（`added_bytes` など）を、組み立て直した会話に合わせて直す。推論の内容の送り返しは、今の規則（送り先とハッシュの一致）のまま。

### 4. 時点と `/rewind`

- 時点は、記録したときの今の先端の節点に結び付ける。会話を戻す先に使えるのは `Request` の時点だけで、今の枝の道の上にあるものとする（`checkpoints` が `conversation_available` を求める規則を、世代の比べ方から、道の上かの判定に改める）。圧縮をまたいでも戻れる。
- `/rewind` の「会話を戻す」と「会話とファイル」は、3. の操作で、その `Request` の時点の依頼の直前へ移ることとする。「ファイルだけ」は今のまま（枝を移らない）。
- 世代（`generation`）と `advance_generation` はなくす。

### 5. 非対話の実行

- `exec --resume` は、記録の今の先端から続ける。`exec` に枝を移る指定は設けない。

## 触るファイル

- `crates/benitoiteh/src/session/record.rs`、`src/session/replay.rs`、`src/session/resume.rs`、`src/session/scan.rs`（要れば）
- `crates/benitoiteh/src/rewind/`（`types.rs`、`mod.rs`）
- `crates/benitoiteh/src/agent/rewind.rs`、`src/agent/mod.rs`、`src/agent/execution.rs`（節点の番号を付けて書く所）、`src/agent/compaction/`（圧縮の記録）、`src/agent/events.rs`（要れば）
- `crates/benitoiteh/src/ui/tui/`（`commands.rs`、`confirmation.rs`、`/tree` の画面）
- テスト: `src/agent/resume_tests.rs`、`tests/session.rs`、`tests/rewind.rs`、`tests/sandbox_rewind.rs`、`tests/agent_loop/`、`src/ui/tui/tests.rs`（それぞれ既存の置き場所に合わせる）

## テスト

- 記録: 会話を作る記録に、増え続ける節点の番号が付くこと。再開した後も続きから数えること。
- 組み立て: 二つの枝を持つ記録から、今の先端の道だけの会話ができること。ほかの枝の圧縮と画像を外した記録を使わないこと。`branch` の `to` が未知の番号か後の番号の記録、節点の番号が戻る記録は、読み込みを断ること。
- 移る操作: 依頼の直前へ移ると、その依頼の文が入力の欄に戻り、次の依頼が新しい枝になり、もとの枝が `/tree` に残ること。先端へ移ると、その枝の会話で続けられること。移った後の要求の本文が、その道の会話だけを含むこと（模擬の提供者で確かめる）。
- 圧縮をまたぐ: 圧縮の後に、圧縮より前の依頼の直前へ移ると、圧縮の前の元の会話（要約ではなく）で次の要求が送られること。
- ファイル: 「会話とファイル」で移ると、移る先の時点のファイルに戻り、移る前の状態が時点として記録され、その時点に戻ってもとの枝の先端のファイルを取り戻せること。読んだ版が消えていること。
- 再開: 枝を移った後に再開すると、移った先の枝から続くこと。
- 既存の巻き戻しのテストは、世代の代わりに道の判定で同じ結果になるよう改める（「圧縮の後は会話を戻せない」を確かめるテストは、戻れることを確かめる形に改める）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、二つの枝を持つ記録の例（数行）を書く。

## 補足（2026-10-19、始める前の点検）

本節は、上の各節に優先する。`/tree` の画面は P5-17 に分けた（上の各節から除いた）。行の番号は `main`（`117d8dc`）のもの。

1. **節点を付けて書く所**: `src/agent/mod.rs` の 674 行（依頼の `User`）、708 行（環境の変化の `User`。種類は `Request` だが `Request` の部分を持たない）、772 行（`Assistant`）、882 行（操縦の `User`、種類 `Steer`）、`src/agent/execution.rs` の 527 行（`ToolResult`。取り消しや抑えたものを含む）、`src/agent/compaction/mod.rs` の 332 行（`Compaction`。すべての理由）、`src/agent/mod.rs` の 535〜537 行と `src/agent/rewind.rs` の 140〜142 行（再開で補う結果。`src/session/resume.rs` の 182 行で作る）、`src/agent/retry.rs` の 48 行（`ImagesRemoved`。今は `AgentEvent::record()`、`src/agent/events.rs` の 238 行を通して書く）。`ImagesRemoved` は節点を付けて直に書き、`record()` はこの事象に `None` を返す。これらをすべて一つの関数（例: `append_node`）で書き、番号を付け、今の先端を進め、時点の記録の側（`CheckpointRecorder::set_node`。`set_turn` と同じ形）に今の節点を知らせる。番号は増え続ければよく（飛んでよい）、記録の最大の番号の続きは `checked_add` で求め、溢れる記録（`u64::MAX` など）は記録の形の誤りとして断る。補う結果は、書くときに番号を付ける。`src/agent/events.rs` と `src/agent/retry.rs` は必ず触る。
2. **依頼の節点**: 種類が `Request` で、`UserPartKind::Request` の部分を持つ `user` の記録とする（`src/agent/rewind.rs` の 44〜55 行、`src/session/scan.rs` の 139〜147 行の見方と同じ）。操縦の節点と、環境の変化だけの節点は、移る先にしない。
3. **結果のない呼び出しの補い**: 作るもの 2 の「先端の枝の最後の応答だけ」を取り消す。今の `src/session/replay.rs` の 159〜226 行と同じく、道の全体に掛ける（ほかの枝の節点は道にないので補われない）。結果のない呼び出しを持つ先端へ移ったときは、再開と同じく、補う結果を次の実行で書くものとして持つ（`resume_results`）。`branch` の記録を書く前に、まだ書いていない補う結果を、今の先端の下に書く（今の `src/agent/rewind.rs` の 138〜142 行と同じ）。
4. **`Record::Rewind` を置き換える所**: `src/session/record.rs:331`、`src/session/replay.rs:136-139`、`src/session/resume.rs:106`、`src/rewind/mod.rs:104-106`、`src/agent/rewind.rs:143-148`。テストは `tests/tui/commands.rs:620`・`:908`、`tests/tui/pty.rs:483`。
5. **時点の `items` と世代を使う所**: `src/rewind/types.rs:122-123`、`src/rewind/mod.rs`（64、93、100、104〜106、135、173、192〜193、205〜210、229、286〜291 行）、`src/agent/compaction/mod.rs:338-340`、`src/agent/rewind.rs`（27〜33、107〜115、135、148 行）、`src/agent/mod.rs:648-653`（`Some(conversation.items.len())`）、`src/tools/mod.rs:622`（`None`）、`src/agent/execution.rs:672`（テスト）、`src/ui/tui/tests.rs:453-467`、`tests/rewind.rs`（539、620〜625、689 行）。`capture` の `items` の引数は、時点の記録の側が持つ今の節点（`set_node`）に替える。プロジェクトを替えるときの `prepare_restart`（`src/rewind/mod.rs:159-183`）も、今の節点を引き継ぐ。`src/tools/mod.rs` を触るファイルに加える。
6. **移る前の時点は一度だけ**: 今の `BeforeRewind` の時点は、ファイルを戻すときだけ `prepare_restore_files`（`src/rewind/restore.rs:27-37`）で記録している。ファイルを戻す移動では、その時点（`PreparedRestore.before`）をそのまま `branch` の `checkpoint` に使い、会話だけの移動では、移る前に一度だけ記録する（二度記録しない。時点の ID がずれて `tests/tui/pty.rs:443-447`・`:479` の `cp-2` と `tests/rewind.rs:1076-1083` の期待が変わるため）。会話だけの移動で時点の記録に失敗したら、`checkpoint: None` のまま移ってよい。移る前の時点が不完全なときに止めるのは、ファイルを戻す移動だけとする。どちらでも `RewindReport.before_rewind` を設定する。
7. **`/rewind` で会話を戻せる時点**: `Request` の時点の `node` は依頼の親なので、同じ親から別の枝に出した二つの依頼で同じ値になる。会話を戻せる時点は、その時点の後に記録された最初の節点が依頼の節点で、その親が時点の `node` と等しく、その依頼の節点が今の道の上にあるものとする。作るもの 3 (a) の「その依頼を受けたときの `Request` の時点」も、同じ対応で求める。依頼の文を求める処理（`src/agent/rewind.rs:27-59`、今は `conversation.items[index]`）は、記録から求める形に改める。
8. **移った後の状態は記録から組み立て直す**: 今の `src/agent/rewind.rs:134-169` は会話を切り詰めるだけだが、ほかの枝へ移るには、`construct()`（`src/agent/mod.rs:420-473`）と同じく組み立て直す。画像の中身の読み直し（`session.restore_images`）、応答ごとの推論の区間と、ハッシュが違うときの `strip_reasoning` と、`resume_compaction`・`resume_has_reasoning` の求め直し、道の上の環境の情報からの `environment_changes` の求め直し（`changes_since_rendered`）、`started = !path.is_empty()`、`previous_last_item = None`・`last_usage = None`・`added_bytes = 0`・`largest_input = None`・`repeated = None`（今の 149〜153 行と同じ）。`turn` はセッションの最大のまま（呼び出しの ID と `spill/` の名前を重ねない）。`FileHistory` と承認はセッション全体のまま。読んだ版をすべて消す関数は今ないので、`ToolContext` に加える（今は `clear_read_version(path)`、`src/tools/mod.rs:603` だけ）。記録から道の状態（会話、応答ごとの `ReasoningContext`、補う結果）を作る関数を一つにし、`ResumeState::from_records` と枝の移動の両方から使う。`construct()` の後の処理も関数に分けて共有する。
9. **再開の状態の作り方**: 推論の区間の並びは、道の上の `AssistantTurn` と一対一に揃える（揃わなければ `construct` が `InvalidConfiguration` になる。`src/agent/mod.rs:431`）。区間は記録の順に一度で求め（`SessionStart` が区間を替える。`src/session/resume.rs:65-93`）、応答の節点ごとに一つ、圧縮の節点ごとにその会話の応答の数だけ（その圧縮を記録したときの区間。`resume.rs:138-144`）持つ。`images_removed` は項目を増やさず、補う結果は区間を増やさない。`max_turn` と呼び出しの表は全体のまま。`replay_records` は `Conversation` を返すので、検査の誤りは `ResumeState::from_records` から `Session::open`（`src/session/mod.rs:151`）と `replay()`（`replay.rs:102-105`）へ伝える。親を辿る処理は再帰でなく繰り返しで書く。
10. **依頼の文を入力の欄に戻す**: 今の画面は戻していない（`selection.rs:157` で知らせを出すだけ。`CheckpointInfo.request` は一行目だけ）。依頼の全文を `RewindReport` の新しい欄（`src/rewind/types.rs:166`）で返し、画面は `screen.restore_input`（`src/ui/tui/mod.rs:438`）で戻す。先頭の `/` は `//` にして戻す（`/` で始まる依頼を出し直すとコマンドになるため。`commands.rs:64`）。これで `tests/tui/commands.rs:559-616`（戻った文に貼り付けがつながる）が変わるので、テストを改める。
11. **形式の版を 2 にして変わるテスト**: `tests/session.rs:391`（版 2 を「対応しない版」に使っている。3 にする）、`tests/exec.rs:50`・`:800`（`format == 1`）、`tests/session.rs:345-360`（番号のない `compaction` の行を読めることを確かめている。番号を必須にするので改める。番号の欄に serde の既定を付けない）、`tests/agent_loop/images.rs:206`（`images_removed` の行の JSON の全体）、手で記録を書くテスト（`tests/session.rs` の 186〜276、332〜339、455 行、`tests/rewind.rs:1052`）は番号を増え続ける形にする。`src/session/record.rs:1` の文書の「形式の版 1」を改める。
12. **向きの変わるテスト**: `tests/rewind.rs:620-625`（cp-2 へ戻した後の cp-1 は、依頼が今の道の上にあるので戻せるようになる）と `:650-691`（圧縮の後も戻せる）。`tests/sandbox_rewind.rs` は変えなくてよい（触るファイルから除く）。
13. **プロジェクトの切り替え**: `ProjectDirChanged` は時点を消す（`src/rewind/mod.rs:103`）。プロジェクトを替える前の節点へ移っても、今のプロジェクトのままとし（切り替えは戻さない）、ファイルを戻せるのは最後の切り替えより後の時点だけとする。プロンプトの側は、組み立て直した `environment_changes` で伝わる。
14. **その他**: `exec --resume` は `Session::open` を通るので、`exec.rs` を変えずに先端から続く。`--continue` は形式の版 1 のセッションを黙って飛ばし、`sessions` は「Unsupported session format version.」と示す（今の扱い）。`AgentEvent::Checkpoint` は `exec` の JSON に時点の全体を出すので、`items`・`generation` を `node` に替えると形が変わる（BH-012 は形を約束していない）。評価の道具は `node` の欄を見ない。設計書 01-05 の「形式の版 1」の記述は、取り込むときにオーケストレータが直す。
