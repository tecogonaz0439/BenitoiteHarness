# P4-02 再開、セッションの一覧、記録の期間と大きさの上限

## 目的

記録したセッションを再開する仕組み（`--resume <ID>`・`--continue`）と、セッションの一覧（`benitoiteh sessions`）と、記録を残す期間と大きさの上限を作る（設計書 01-05 の「再開」「記録を残す期間」、05-01 の「sessions」）。対話の画面はまだないので、再開は非対話の `exec` で確かめる。対話の画面からの再開（と、制限モードのセッションを通常モードで再開するときの確認の画面）は P4-07 で作る。

## 読む設計書

- [セッション](../../design/01-agent/01-05-sessions.md)の「セッションの記録」「再開」（2026-10-17 に、同じファイルへの追記、動いているセッション、プロジェクトのディレクトリ、提供者とモデルの既定、番の番号、戻さないもの、`exec` での再開、環境の情報の変化を加えた）「一時ディレクトリ」「記録を残す期間」（同日に上限の掛け方を加えた）
- [文脈の管理](../../design/01-agent/01-04-context.md)の「環境の情報」「推論の内容の扱い」「圧縮」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「モードを変える操作」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「コマンドラインの形」「オプション」「終了状態」「設定ファイル」の `[sessions]`、「データの置き場所」「sessions」
- 既存のもの: `src/cli/args.rs`（`DeferredOptions`、`ArgsError::Unavailable`）、`src/cli/exec.rs`（`prepare` の順序）、`src/config/mod.rs`（`resolve_exec`）、`src/session/`（`Session::create`、ロックのファイル、`cleanup`、`storage.rs` の `LogState`・`SessionWriter`・`SpillWriter`、`replay.rs` の `read_records`・`replay`・`fill_missing`、`id.rs` の `SessionId`、`Record::Header`・`SessionStart`、P4-01 の `FileHistory`）、`src/agent/`（`Agent::new`、`events.rs` の `AgentEvent::SessionStart` と `record()`、呼び出しの ID の作り方、P4-01 の `compact_with_options`・`CompactOptions`・`compaction/conversation.rs` の推論を除く処理）、`src/context/environment.rs`（`Environment::changes_since`）、`src/ui/mod.rs`（`terminal_text`）、`tests/session.rs`（`cleanup` のテストと注釈）、`tests/args.rs`

## この作業が認める型の変更

- `Record::SessionStart` と `AgentEvent::SessionStart` に、`resumed: bool`、`base_url`、`system_prompt_sha256`、`tools_sha256` を `Option` か `#[serde(default)]` で加える（`exec --json` の出力も変わる）。新しいセッションの `session_start` にも同じ欄を書く。
- `Session` に、すでにあるセッションを開く関数と、`spill/`・`objects/` の書ける量の口を加える。
- `Agent` に、記録から作る口（例: `Agent::resume(...)`）を加える。
- `CompactOptions` に、別の要求の形を強いる欄（例: `separate_request: bool`）を加え、`CompactionReason` に `Resume` を加える。
- 推論の内容を除く処理を、圧縮と再開で共有する関数（例: `strip_reasoning(&mut AssistantTurn)`。Responses の生の項目の `id` を除く処理を含む）に切り出す。
- 引数の型: `resume` と `continue_session` を `DeferredOptions` から `exec` の引数へ移す。

## 作るもの

### 1. 引数

- `--resume <ID>` と `--continue` を `exec` で使えるようにする（`ArgsError::Unavailable` から外す）。二つを同時に指定したら使い方の誤りとする。`--allow` と `--project-parent` は今までどおり使えない。`tests/args.rs` の `--resume` を `Unavailable` とする期待を改める。
- 対話の画面（引数のない起動）はまだないので、対話の形での再開は今までどおり使えない。

### 2. 起動の順序

再開では、プロジェクト、モード、提供者を記録から決めるので、今の `prepare` の順序（プロジェクトの解決、設定の解決、書ける場所の検査、`cleanup`、`Session::create`）を、再開のときは次の順にする。

1. 開くセッションの ID を決める（`--resume` の値、または `--continue` の走査。4）。
2. `Session::open` で錠を取る（3）。
3. 記録を読み、最後のモード、提供者の設定の名前、モデル、プロジェクトのディレクトリ、番の番号の最大を得る（5）。
4. 記録の値を既定にして、設定を解決する（6）。
5. 書ける場所を検査する（記録のプロジェクトのディレクトリも、`ScopeRoot::open` と `validate_writable_with_executable` を同じく通す）。
6. `cleanup` を行う（自分が錠を持つので、開いたセッションは「動いている」とみなされて消されない）。
7. 書ける量を数える（9）。

標準エラー出力に、再開するセッションの ID とプロジェクトのディレクトリを一行で示す（英文は `text` に置く）。

### 3. セッションを開く

- `Session::open(paths: &DataPaths, id: &SessionId) -> Result<Session, SessionError>` を加える。ID の検査は既存の `SessionId::from_str` を使う（33 文字の厳しい形。パスの要素に使えない形は通らない）。
- `sessions/<ID>` をシンボリックリンクを辿らずに開き、ロックのファイルの錠を取る。取れなければ「動いている」誤りとする。錠を取った後、ロックのファイルを `fstat` して、リンクの数が 0（削除の途中で消された）なら「ない」とする。
- 失敗したとき（錠が取れない、一時ディレクトリを作れない、記録が読めないなど）は、セッションのディレクトリとその中身に触れない（`Session::create` の失敗のときの消し方を写さない）。
- 記録のファイル（`session.jsonl`）は `O_WRONLY|O_APPEND|O_NOFOLLOW` で開き、権限が 0600 でリンクの数が 1 であることを確かめる。最後の行が改行で終わっていなければ（異常な終わり）、追記の前に、最後の改行の位置まで切り詰める（`ftruncate`）。理由: そのまま追記すると、切れた行と新しい行がつながり、次の読み込みが JSON の誤りで止まる。
- 書き手（`SessionWriter`）は、記録にヘッダの行があることを確かめてから、ヘッダを書いた後の状態で作る（今の `LogState::new` はヘッダより前の行を拒む）。
- 一時ディレクトリ: `benitoiteh-<ID>` がすでにあれば、ディレクトリで、自分の所有で、権限が 0700 で、シンボリックリンクでないときだけ、シンボリックリンクを辿らずに消してから作り直す。満たさなければ誤りとし、消さない。
- 記録の形式の版: ヘッダの `format` を先に読み、今の版と違えば、そのことを示す専用の文言で誤りとする。

### 4. `--continue` の走査

- セッションのディレクトリを ID の降順に見て、記録の最後のプロジェクトのディレクトリ（`Record::ProjectDirChanged` があればその最後、なければヘッダ）の実のパスが、今のプロジェクトのディレクトリの実のパスと同じもののうち、最初に錠が取れるものを開く。ないときは誤りとする。
- 読めない記録と、形式の版が違う記録は飛ばす。
- 走査には、記録の最後まで読む軽い読み手を使う（各行を `type` と要る欄だけの構造に読み、`Spilled` の中身は辿らない）。6 の一覧も同じ読み手を使う。
- 走査の間の短い錠で、同時に走る `--resume` が「動いている」誤りになりうることは受け入れる。

### 5. 記録を読む

- 記録から、会話（`replay` の論理）、結果の記録のない道具の呼び出しに補う結果、番の番号の最大、最後のモード・提供者の設定の名前・モデル・ベースの URL・プロジェクトのディレクトリ、推論の区間（7）、`FileHistory` を持つ値（例: `ResumeState`）を `session` で作る。`replay` と `fill_missing` の論理を二重に書かない。
- 番の番号の最大は、`TurnStart`・`Assistant`・`ToolCall`・`ToolResult`・`TurnEnd` のすべてから取る（応答の前に落ちた番も数える）。
- 最後のモードは、最後の `session_start`、なければヘッダのものとする。提供者の設定の名前とモデルも同じ。

### 6. 起動の値

- モード: `--mode` があればそれ、なければ記録のモードとする。設定の `default_mode` は使わない（`resolve_exec` の `args.mode.unwrap_or(self.default_mode)` を、再開のときに記録のモードに変える）。理由: `default_mode = "normal"` の利用者が、制限モードのセッションを `--mode` なしで再開したとき、確認なしで通常モードになることを防ぐ（03-01 の「モードを変える操作」）。
- 制限モードのセッションを通常モードで再開するとき（`--mode normal` を明示したとき）は、`exec` では行わず、使い方の誤り（終了状態 2）とする。通常モードのセッションを制限モードで再開するのは許す。
- 提供者とモデル: `--provider` が記録の名前と違えば、記録のモデルを使わない（その提供者の既定に従う）。`--provider` が同じか指定がなければ、提供者は記録の名前とし、モデルは `--model` ＞ 記録のモデル ＞ プロジェクトの設定 ＞ 提供者の設定の順とする。記録の名前の提供者が今の設定になければ、誤り（終了状態 2）とする。
- 推論の深さ、サンドボックスと承認の方針、書ける場所、上限は、今の起動の指定と設定による。
- `--resume` に `--project` を添えたときは、記録の最後のプロジェクトのディレクトリと実のパスで比べ、違えば使い方の誤りとする。

### 7. 推論の内容

- 記録を組み立て直すときに、各 `Assistant` の番を、その前の最も近いヘッダか `session_start` の（システムプロンプトと道具の定義のハッシュ、送り先）と結び付ける。`session_start` にハッシュがなければ（前の形）、ヘッダの値を使う。`Record::Compaction` の会話の項目は、その行の時点の区間に属するとみなす。
- 今のハッシュと違う区間の番からだけ、推論の内容を除く（共有の関数を使う）。送り先が違う番は、今までどおりアダプタが推論を送らない（`raw_items.destination` の比べ方）。理由: 区間を見ずに「最後の `session_start`」だけと比べると、二度目の再開で、一度目の再開より前の推論を送ってしまう（Anthropic は 400 を返す）。
- `reasoning_replay` が `all-turns` で、送り先かハッシュが今と違う番が会話に残るときは、最初の要求の前に、残す範囲を設けず、別の要求の形を強いた圧縮（`CompactOptions { reason: Resume, keep_recent: false, separate_request: true, .. }`）を行う（01-04 の「推論の内容の扱い」）。圧縮が `Failed`・`Cancelled` か、推論があるのに `NothingToCompact` のときは、再開を止めて示し、`exec` は失敗の終了状態で終わる。

### 8. 再開の手順

- `Agent::resume(...)` は、ヘッダを書かず、始めたものとして扱い（`started = true`）、このセッションで許したコマンドの集合と読んだファイルの版を空のまま始め、`FileHistory` を記録から作ったものにする。番の番号と呼び出しの ID（`turn-N-call-i`）は、記録の最大の番の番号の続きから数える。
- 記録に書く順: 切れた行の切り詰め（3）→ 補う結果の `Record::ToolResult`（`turn` は対応する `Assistant` の番、`name` は呼び出しの名前、`spill: None`）→ 再開の `session_start` → 要れば 7 の圧縮 → 新しい依頼。補った結果は実行し直さない。
- 環境の情報: 記録には描いた文だけが残るので、今の環境の情報の欄ごとの描いた形（`<name>value</name>`）と、プロジェクトの指示のファイルの描いた形（`<project_instructions ...>...</project_instructions>`）が、今の会話のすべての `Environment` の部分のどこにも含まれていなければ、変化として加える関数（例: `Environment::changes_since_rendered(&[&str]) -> Option<String>`）を作る。変化は、新しい依頼の発言の先頭に `Environment` の部分として置く（`Agent` にこれを渡す口を加える）。消えた欄（読まなくなった指示のファイルなど）は伝わらないことを、コメントに書く。

### 9. 記録を残す期間と大きさの上限

- 期間: 起動のとき（`exec`、`sessions`）に、`[sessions] retention_days`（既定 30）を過ぎたセッションのディレクトリを消す。過ぎたかは、記録のファイルの更新の時刻（ディレクトリの記述子から `fstatat` でシンボリックリンクを辿らずに取る。記録のファイルがなければディレクトリの更新の時刻）で判定する。P1-05 の判定（ID の時刻）を、この判定に置き換える。理由: 再開したセッションは追記で更新の時刻が新しくなり、続けて使っている記録を消さない。期間を過ぎたセッションでも、明示すれば再開できる。`tests/session.rs` の、更新の時刻で判定することを退行とみなすテストと注釈を、新しい判定に合わせて改める。動いているもの（錠が取れないもの）は消さない。消すときはシンボリックリンクを辿らない。
- 大きさ: 起動のときに、すべてのセッションのディレクトリの中の通常のファイルの大きさ（`len()`。シンボリックリンクを辿らず、数える間に消えたものは無視する）の合計を数える。範囲は、セッションのディレクトリの中のすべて（`session.jsonl`、`spill/`、`objects/`、`checkpoints/` を含む）とする。このセッションの `spill/` と `objects/` に書ける量を、1 GiB から自分の今の `spill/` と `objects/` の量を引いた値と、「10 GiB − ほかのセッションの合計」（0 で止める）の小さい方とする。
- 書ける量は、`Session` の内部の状態に `AtomicU64` で持たせ、`Session::reserve(bytes) -> bool` と `Session::release(bytes)` の口で、`SpillWriter` と P4-03 の `objects/` の書き手が共有する。`SpillWriter::discard` は予約を返す。超える書き込みは誤りにし、`bash` の結果では今の「保存できなかった」の印になる（P3-05）。上限に達したら、標準エラー出力に一度だけ一行で示す（01-03 の「出力の切り詰め」の「結果と画面に示す」）。記録の分割（`Record::Spilled` の部品）と `session.jsonl` は止めない。
- 合計が 10 GiB を超えているときは、起動のときに、そのことと、期間を短くする方法を一行で示す。
- 起動のたびにすべてのファイルを辿る時間を測り、完了の報告に書く（1,000 のセッションで）。

### 10. セッションの一覧

- `benitoiteh sessions [--project <dir>] [--json]` を加える（05-01 の「sessions」）。4 の軽い読み手を使う。動いているかは、錠を取れるかで調べ、調べた後に錠を放す。
- 並びは ID の降順。モードは最後の `session_start`（なければヘッダ）のもの。`--project` は実のパスで比べる。
- 最初の依頼は、最初の `Record::User` の `Request` の部分から取り、Unicode の文字で 60 文字に切ってから `ui::terminal_text` で制御の文字を目に見える形にする。記録が `Spilled` に分けられていて読めないときは `(not available)` と示す。プロジェクトのパスと読めない理由の文にも `terminal_text` を使う。
- JSON の欄は、`id`、`started_at`、`mode`、`project_dir`、`first_request`、`active`、`error`（読めないときだけ）とする。

## 触るファイル

- `crates/benitoiteh/src/cli/`（`args.rs`、`exec.rs`、`sessions` のための新しいモジュール、`mod.rs` の振り分け、`text.rs`）
- `crates/benitoiteh/src/config/`（再開のときの値の解決）
- `crates/benitoiteh/src/session/`（開く関数、切り詰め、軽い読み手、`ResumeState`、期間と上限、`SpillWriter` の予約）
- `crates/benitoiteh/src/agent/`（`Agent::resume`、`SessionStart` の欄、番の番号、推論の内容の区間、`CompactOptions`、共有の推論を除く関数）
- `crates/benitoiteh/src/context/`（環境の情報と指示のファイルの変化）
- `crates/benitoiteh/src/ui/`（`sessions` の表示）
- `crates/benitoiteh/tests/`（`exec.rs`、`session.rs`、`args.rs`、新しい `tests/resume.rs` と `tests/sessions_cmd.rs` にしてよい）

## テスト

模擬の提供者と、一時の `XDG_STATE_HOME` で確かめる。`#[cfg(test)]` の差し替えは `tests/` から見えないので、統合テストでは実際のファイルで状態を作る（大きさは `set_len` の疎なファイル、時刻は `File::set_modified`、ハッシュの違いはヘッダの書き換えか、通常モードのセッションを制限モードで再開して作る）。

- `exec --resume <ID>`: 前の `exec` のセッションの会話に続けて、新しい依頼を送る。送った要求が、前の会話と新しい依頼を持つ。記録は同じファイルに追記され、再開の `session_start`（`resumed: true`）が一行加わる。`replay` が `agent.conversation()` と一致する。
- `exec --continue`: 同じプロジェクトの最も新しいセッションを選ぶ。別のプロジェクトのセッション、動いているセッション（テストの中で錠を持つ）、読めない記録は選ばない。
- 誤り: `--resume` と `--continue` の同時指定、`--resume` と違う `--project`、ない ID、形の違う ID（`../x` など）、`sessions/<ID>` がシンボリックリンク、形式の版が違う記録。どの誤りでも、セッションのディレクトリと中身が残る。
- 安全の性質（07-01）:
  - 制限モードのセッションを `--mode normal` で `exec` で再開すると、使い方の誤りになる。
  - `default_mode = "normal"` の設定で、制限モードのセッションを `--mode` なしで再開すると、制限モードになる。
  - 一時ディレクトリに残ったシンボリックリンクの先を消さない。期間で消すときもシンボリックリンクの先を消さない。
- 切れた最後の行: 改行で終わらない最後の行を作ってから再開すると、切り詰めてから追記し、もう一度 `replay` と再開ができる。
- 結果の記録のない道具の呼び出し（実行を始めた記録あり、なし）に、決まった結果が補われ、記録に書かれ、実行し直されない。
- 番の番号と呼び出しの ID が記録の続きから数えられ、前の `spill/` のファイルを上書きしない。
- 推論の内容: ハッシュが記録と違うと、前の区間の推論の内容を送らない。同じなら送る（Anthropic の thinking のブロック）。ハッシュを変えて再開した後、同じハッシュでもう一度再開しても、最初の区間の推論を送らない。`all-turns` で送り先が違うと、最初の要求の前に、道具を持たず `<conversation>` を含む要約の要求が送られる。
- 提供者とモデル: 明示しないと記録の名前とモデルを使う。`--provider` で別の名前を示すと記録のモデルを使わない。記録の名前が設定になければ誤りになる。
- 環境の情報: 日付を変えて再開すると、変化として新しい依頼の前に加わる。変わらなければ加わらない。プロジェクトの指示のファイルを変えると加わる。
- `sessions`: 降順に並び、`--project` で絞れ、`--json` で一行一件になる。最初の依頼の制御の文字が目に見える形になる。読めない記録は ID と理由だけを示して続ける。
- 期間: 更新の時刻が期間を過ぎたセッションが起動のときに消え、動いているもの、期間内のもの、今開いたものは残る。期間を過ぎたセッションを `--resume` で明示すると再開できる。
- 上限: ほかのセッションに疎なファイルを置いて合計を上限の近くにすると、`spill/` への書き込みが上限で止まり、`bash` の結果が「保存できなかった」の印になり、一度だけ一行が出る。合計が 10 GiB を超えているときに、起動のときの一行が出る。
- このセッションで許したコマンドを戻さないことは、`exec`（`never` だけ）では確かめられないので、`Agent::resume` の単体のテストで、集合が空であることを確かめる。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、加えた英文の全文、`sessions` の出力の例（文字と JSON）、加えた公開の関数と型のシグネチャ、1,000 のセッションでの起動の走査の時間を書く。
