# P4-07 対話の操作と確認の画面

## 目的

対話の画面に、一連の処理の途中の入力（操縦と後続）、ESC による取り消し、確認の画面、入力の履歴の保存、対話の形での再開を加える（設計書 05-02 の「入力の操作」「確認の画面」、01-01 の「利用者の割り込み」「取り消し」、03-03）。P4-06 の土台の上に作る。あわせて、BH-097（macOS の Seatbelt の中の子プロセスが端末を操作できないか）を確かめるサンドボックスのテストを加える。

## 読む設計書

- [対話の画面](../../design/05-interface/05-02-tui.md)の「入力の操作」（入力の履歴はプロジェクトごと。BH-018）「確認の画面」（英語だけ。BH-053）
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「利用者の割り込み」「取り消し」（2026-10-17 に一度の ESC の扱いを加えた）
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」（示す前のキーと示した直後 300 ミリ秒のキーを捨てる）「通常モードの承認の方針」「そのほかの確認」（`web_fetch` の答え。BH-047）
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「モードを変える操作」「保証しないこと」（サンドボックスの外で動くコマンドからの端末の操作。BH-097）
- [セッション](../../design/01-agent/01-05-sessions.md)の「再開」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「オプション」（対話の承認の方針の既定は `on-request`）「データの置き場所」（`history/`）
- 既存のもの: P4-06 の画面の芯と `prepare`・`Prepared`・働き手のスレッドの仕事の形、`src/agent/`（`AgentInput`、`run`・`run_inner`、`execution.rs` の確認の流れと残りの呼び出しを取り消す処理、`mod.rs` の取り消しの判定）、`src/approval/`（`Approver: Send`、`ApprovalRecord`、`ApprovalAnswer`、`decide`）、`src/sandbox/run.rs`（取り消しの印）、`src/cli/`（`exec.rs` の `ShellContext.approval` の固定と `with_approvals` の呼び方、`signals.rs`）、`src/config/`（`resolve_exec` の `--approval` の判定、`resolve_resume` の `RESUME_MODE`、`paths.rs` の `DataPaths`）、P4-04 の `ApprovalKind::WebFetch` とドメインの集合、`tests/support/` の `sandbox-probe`

## この作業が認める型の変更

- `Agent` の一連の処理の口を、二つの取り消しの印と操縦の受け口を持つ形にする（例: `RunControl { cancel_step: Arc<AtomicBool>, cancel_run: Arc<AtomicBool>, steering: Receiver<String> }`）。今の `run(request, cancelled, events)` は、これを包む関数として残してよい（テストの書き換えを減らす）。
- `sandbox::run::run` と `tools::Invocation` の取り消しの印を、二つの印を見る型（例: `CancelToken`）に変えてよい（2 の不変条件を守れるなら、印の受け渡しで済ませてもよい）。
- 確認の要求を画面へ送る `Approver` の実装を `approval` に置く（`agent` から `ui` への依存を作らない）。
- `config::resolve_resume` に、モードの変更を許すかの引数を加え、`prepare` に、モードの変更を確かめる口（例: `confirm_mode_change: &mut dyn FnMut(&ModeChange) -> bool`）を加える。
- `DataPaths` に履歴の置き場所（`history_dir`）を加える。
- P4-06 の画面の芯が受け取る入力のイベントに、読んだ時刻（`Instant`）を添える（`(Event, Instant)` か、時計を渡す形）。

## 作るもの

### 1. 操縦と後続

- 一連の処理の途中の Enter は操縦とし、入力欄の文を `RunControl.steering` へ送る。ループは、`execute_calls` と `TurnEnd` の後、番の先頭の圧縮の判定より前に受け口を空にし、届いた操縦を一つの `UserMessage` の複数の `UserPartKind::Steering` の部分として会話に加え、`Record::User { kind: Steer }` を一行書く。一連の処理が最後の応答（道具を呼ばない応答）で終わる直前にも一度だけ受け口を空にし、操縦があれば終わらずに次の要求を送る。`AgentInput::FollowUp`・`Request` はループでは受けない（後続は画面が持つ）。`exec` では受け口が切れているので、「操縦なし」として扱う。
- 操縦を会話に加えたことは、イベント（例: `AgentEvent::Steered { text }`）で画面に示す（送った時点ではなく、加えた時点で示す）。
- 取り消し、誤り、拒否、上限で終わったときに加えられずに残った操縦は、`RunReport` の欄で画面へ返し、画面はそれを入力欄に戻す。
- 三つのアダプタの要求の形（Anthropic は道具の結果の後の利用者の発言を別のメッセージにする）で、操縦が正しく送られることをテストで確かめる。
- Alt+Enter は後続とし、画面の列に入れる（入れた時点で入力の履歴に入れる）。一連の処理が終わった後に、新しい依頼として順に送る。待っている間（処理の途中でない）の Alt+Enter は Enter と同じに扱う。一連の処理を取り消したら、後続は送らずに入力欄へ戻す。終えるときは捨てる。後続の件数は状態の行に示す。

### 2. 取り消しの二つの段階

不変条件を次のとおりとする。

- 一連の処理を止めるとき（二度の ESC、処理中の Ctrl+C、終える印）は、`cancel_run` と `cancel_step` の両方を立てる。
- ループは `cancel_run` を戻さない。`cancel_step` は、道具の段階（一つの応答の道具の呼び出しの実行）の終わりに `swap(false)` で消費し、まだ終わっていない残りの呼び出しを取り消した結果（01-01 の英文）にして続ける。そのときの `TurnEnd` の `finish_reason` は応答の値（`ToolCalls`）のままとする。
- 道具の段階の外（応答の受け取り、自動の圧縮、やり直しの待ち、切り離したスレッドの待ち）で `cancel_step` が立っていれば、一連の処理を止める（01-01 の「応答の途中なら一連の処理を止める」）。
- `bash` の子プロセス（`run::run`）は、どちらの印でも止まる。
- `exec` は、`cancel_step` と `cancel_run` に別々の `Arc` を作り、SIGINT・SIGTERM の印を両方に登録する（同じ `Arc` を両方に渡さない）。
- 確認の画面の ESC では `cancel_step` を立てない（立てると、同じ応答の残りの呼び出しまで取り消される）。

キーの割り当て:

- ESC（一度）: 処理の途中なら `cancel_step` を立てる。確認の画面では `Denied`。待っている間は何もしない。
- ESC（1 秒以内に二度。確認の画面の ESC の後の二度目を含む）と処理中の Ctrl+C: 一連の処理を止める。kitty の拡張がない端末と tmux で ESC ESC が `Esc` と ALT の一つのイベントで届くときも、二度押しとして扱う。
- 止めたときは、道具の実行の途中（`ToolCall` があって `ToolResult` がまだない）だった場合だけ、止める前に子プロセスが行った操作は戻らないことを一行で示す（01-01）。

### 3. 確認の画面

- 受け渡し: `Approver` の実装は、要求ごとに返事のチャネル（`sync_channel(1)`）を作り、要求と一緒に画面のスレッドへ送り、答えを待つ（時間切れは設けない）。画面は、その時点までの `AgentEvent` を受け終えてから画面を開く。受け手が閉じたら `Cancelled` とする。Ctrl+D と終える印のときは `Cancelled` を返してから、ループが戻るのを待つ。P4-08 の `/new` で作り直せる形にする。
- 描き方: 題は種類から作る（`summary` を題に使わない。コマンドを含むため）。`ApprovalFacts` の欄（コマンド、作業ディレクトリ、パス、サンドボックスの方針、URL とドメイン）は切らずに折り返して示し、改行を含むコマンドは各行に字下げを付ける（枠の中に答えのキーの行を偽らせない）。行数が多いときは行数も示す。題・すべての事実・答えのキーが画面に収まらないときは、許す答えを受け付けず、断る答えだけを受け付ける（設計書 03-03、2026-10-18 に加えた）。長い空白の連なりは印にまとめ、目に見えない文字と幅の定まらない文字も目に見える形にする（設計書 05-02「出力の示し方」）。`justification` は `Reason given by the model:` の見出しを付けて事実と分けて示す。すべて `terminal_text` で制御の文字を目に見える形にする。枠と事実は履歴に書き、答えのキーの行と `[m]` の入力欄は下の部分に描く。打ちかけの入力欄の中身は保ち、答えた後に戻す。
- 答えのキー（英語。`ui` の `text` に置く）:
  - `Command`: `[y] run` `[a] allow this exact command for this session` `[n] decline` `[m] decline with a message`
  - `Escalate`: `[y] run outside the sandbox once` `[n] decline` `[m] decline with a message`
  - `WriteScope`: `[y] write` `[n] decline` `[m] decline with a message`
  - `WebFetch`: `[y] fetch once` `[s] allow this domain for this session` `[n] decline` `[m] decline with a message`
- 答えにするのは、`KeyEventKind::Press` で、修飾キーのない（大文字を受けるなら Shift だけの）文字のキーだけとする。Enter、Space、ほかのキー、Alt・Ctrl の付いたキーは答えにしない。既定の答えはない。貼り付けは答えにしない。
- `[m]` は、下の部分に一行の入力欄を開き、Enter で `DeniedWithMessage` とその文を返す。貼り付けは文として受け、改行は空白に変える。ESC で入力をやめて選択に戻る。`[m]` の文は入力の履歴に入れない。
- 入力の守り: 画面の `write_all` と `flush` が返った時点を「示した」時刻とする。その直後に、標準入力の端末の入力の待ち行列を `rustix::termios::tcflush`（`IFlush`）で捨て（要る `rustix` の機能を確かめる）、続けて `crossterm` の中と画面の側に溜まったイベントを捨てる。示した時刻から 300 ミリ秒より前に読んだキーは捨てる（ESC も捨てる。Ctrl+C は常に受ける）。この守りは共通の関数にし、P4-08 の `/rewind` の確かめと、5 のモードの変更の確かめでも使う。
- 確認の画面の Ctrl+C と終える印: `Cancelled` を返し、一連の処理も止める。
- 承認の方針の配線: 対話の方針は `--approval` → 利用者の設定の `normal.approval` → `on-request` の順で決め、その値を `ShellContext.approval` と `ApprovalPolicy` の両方に入れる。P4-06 で加えた「`never` でなければ誤り」の判定と、確認が要る操作は拒否されることの起動の一行を除く。`exec` の `resolve_exec` の判定（`never` だけ）は今のまま残す。制限モードでも、対話の形では、画面の `Approver` と `ApprovalPolicy { approval: OnRequest, .. }` を渡す（`web_fetch` の確認に答えられるように）。多重の守りとして、制限モードの `decide` は `WebFetch` 以外の種類を `Deny` にする。

### 4. 入力の履歴

- 置き場所: `DataPaths::history_dir`（データのディレクトリの `history/`。05-01）の下の、プロジェクトのディレクトリの実のパスのバイト列の SHA-256 を小文字の 16 進 64 文字で書いた名前のファイル。ディレクトリは `exec.rs` の `private_directory` と同じ検査（0700、自分の所有、シンボリックリンクでない）で作る。
- 書き込み: 同じ名前の `.lock` のファイルに `flock` で排他の錠を掛け、追記ごとに `O_APPEND|O_NOFOLLOW|O_CLOEXEC` で開き、`fstat` で通常のファイル・自分の所有・0600 を確かめる（違えば履歴を止めて一行で示す）。一行一件の JSON の文字列で書く。錠の中で件数を数え、500 件を超えたら、同じディレクトリに `O_EXCL|O_NOFOLLOW` の 0600 の一時ファイルを作って新しい 500 件を書き、名前を付け替える（同じプロジェクトで二つのセッションが動いていても記録を失わない）。
- 一件は 64 KiB まで（超える入力は入れない）、読むファイルは 64 MiB までとする。
- 起動のときに新しい 500 件を読む。↑・↓ は、入力欄が一行のとき、この履歴を辿る。
- 履歴の失敗は一行で示して続け、起動を止めない。
- 入れるもの: 送った依頼、操縦、後続（P4-08 で `/` のコマンド）。入れないもの: 確認の画面の `[m]` の文。

### 5. 対話の形での再開

- `benitoiteh --resume <ID>` と `benitoiteh --continue` を対話の形で使えるようにする（P4-02 の選び方と組み立て直しを使う）。
- 制限モードのセッションを `--mode normal` で再開するときは、`resolve_resume` の直後、`cleanup` と `Agent` の生成より前に、`prepare` の確かめの口で、前の文脈に信頼しない内容が含まれうることを示す確認の画面を出す（3 の入力の守りを掛ける）。`[y]` のときだけ再開する。拒んだら、錠を解き、記録のファイルをバイト単位で変えずに、終了状態 0 で終える。`exec` は今のまま拒否する（03-01 の「非対話では行わない」）。
- 再開したら、前の会話の最後の数件（利用者の発言（`Request`・`Steering` の部分）と応答の文の最後の 3 組。`Environment` と `Summary` は除き、圧縮があれば一行で示す）を、`Agent::conversation()` から作って示す（`terminal_text` に通す）。

### 6. BH-097 の確かめ

- `tests/support/` の `sandbox-probe` に、`with-ctty <pts> -- <cmd...>`（`setsid`、`pts` を開く、`rustix::process::ioctl_tiocsctty` の順に行ってから子を起動する）、`tty-write`（`/dev/tty` を書くために開いて書く）、`tty-ioctl`（`/dev/tty` を読み取りで開き、`rustix::termios::tcgetattr` を呼ぶ）の操作を加える（`unsafe` を使わない。`TIOCSTI` は `unsafe` なしで呼べないので、同じ種類の ioctl が拒まれることで代わりに確かめる）。
- `tests/sandbox_macos.rs`・`tests/sandbox_linux.rs` に、テストが `rustix::pty` で擬似端末を作り（`rustix` の `pty` の機能を加えてよい）、通常モードのサンドボックスの中の子に `tty-write` と `tty-ioctl` をさせるテストを加える。期待: macOS では、書き込みが失敗し、ioctl が拒まれ、擬似端末のマスタに何も届かない。Linux（`--new-session`）では、`/dev/tty` を開くと失敗する。対照として、同じ手順をサンドボックスなしで走らせ、書き込みが届くことを確かめる（テストが空振りしないように）。
- このテストは Codex の中では走らないので、オーケストレータが手元で走らせ、結果で BH-097 を閉じるか決める。

## 触るファイル

- `crates/benitoiteh/src/ui/`（対話のモジュール、確認の画面、入力の守り、履歴、`text.rs`）
- `crates/benitoiteh/src/agent/`（操縦、取り消しの二つの段階、`RunControl`、イベント、`RunReport` の残った操縦）
- `crates/benitoiteh/src/approval/`（対話の `Approver`、制限モードの `decide`）
- `crates/benitoiteh/src/sandbox/run.rs`、`src/tools/mod.rs`（取り消しの印の型を変えるなら）
- `crates/benitoiteh/src/cli/`（対話の形の承認の方針、再開と確かめの口、`signals.rs` の二つの印）
- `crates/benitoiteh/src/config/`（`resolve_resume` の引数、`DataPaths::history_dir`）
- `crates/benitoiteh/Cargo.toml`（`rustix` の `termios`・`pty` の機能が要れば）
- `crates/benitoiteh/tests/`（`tests/support/` の `sandbox-probe`、`tests/sandbox_macos.rs`、`tests/sandbox_linux.rs`、新しい対話のテスト）

## テスト

端末なしで、画面の芯に時刻付きのイベントの列と書き先を渡して確かめる（眠らずに時刻を進める）。

- 操縦: 模擬の提供者で、道具を呼ぶ応答の途中に操縦を送ると、道具の結果の後、次の要求の前に利用者の発言として入る（三つのアダプタの要求の形）。最後の応答の直前に届いた操縦で、一連の処理が続く。記録に `steer` の行が入る。取り消しで残った操縦が入力欄に戻る。
- 後続: 一連の処理の後に、新しい依頼として順に送られる。取り消すと送られずに入力欄へ戻る。
- 取り消し: 一度の ESC（`cancel_step`）で、`bash` の長いコマンド（`--sandbox danger-full-access` の `sleep`）が止まり、残りの呼び出しが取り消した結果になり、一連の処理が続く。応答の受け取りの途中の一度の ESC で一連の処理が止まる。二度の ESC と Ctrl+C で止まる。`exec` の SIGINT で `bash` が止まる（退行の防止）。確認の画面の ESC で、同じ応答の残りの呼び出しが取り消されない。
- 確認の画面:
  - 種類ごとのキーが答えに対応する。`Escalate` と `WriteScope` に `[a]`・`[s]` がない。
  - 安全の性質（07-01）: 画面を示す前に届いたキー、貼り付け、示した直後 300 ミリ秒より前のキー、`Alt+y`・`Ctrl+Y`・Enter・Space で答えにならない。ESC が拒否になる。
  - `[m]` の文（貼り付けと改行を含む）が拒否の結果に入る。
  - 安全の性質: 改行と枠の文字を含むコマンドで、答えのキーの行を偽った表示が作れない。事実と `justification` が分けて示され、制御の文字が目に見える形になる。
  - 古い要求への答えが次の要求に使われない（要求ごとの返事のチャネル）。
- 承認の方針: 対話の既定が `on-request` になる。制限モードの対話で `web_fetch` の確認が出る。制限モードの `decide` が `WebFetch` 以外を `Deny` にする。
- 履歴: 権限が 0600 で、500 件を超えると古い側が除かれ、プロジェクトごとに分かれる。シンボリックリンクに置き換えた履歴のファイルを辿らない。`[m]` の文が入らない。二つの書き手が交互に書いても記録を失わない。
- 再開: 制限モードのセッションを通常モードで再開するとき、確かめの関数が偽を返せば再開せず、記録のファイルがバイト単位で変わらず、錠が解かれる（安全の性質のテスト）。
- BH-097: 6 のサンドボックスのテスト。

## 手で確かめる項目（オーケストレータ）

完了の報告に手順を書く。確認の画面の直前に `y` を打ち続けても答えにならないこと（端末の待ち行列の破棄）、Terminal.app の Alt+Enter（既定では Option が Meta にならず届かない）、tmux の `escape-time` での二度の ESC、確認の画面を開いたまま端末の大きさを変える、`danger-full-access` で `/dev/tty` に偽の画面を書けてしまうこと（BH-097 の残る危険の確認）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、確認の画面の例（種類ごと。文字の写し）、足した英文の全文、`10-interfaces.md` に書く新しいシグネチャ（`RunControl`、`run` の形、`Approver` の要求の型、`prepare` の確かめの口、`DataPaths::history_dir`）を書く。
