# P4-08 `/` で始まるコマンドと、P4 の安全の性質のテストの一覧

## 目的

対話の画面の `/` で始まるコマンド（`/help`・`/status`・`/compact`・`/rewind`・`/new`・`/quit`）を作る（設計書 05-02 の「`/` で始まるコマンド」）。圧縮（P4-01）、巻き戻し（P4-03）、対話の画面（P4-06・P4-07）の口を使う。あわせて、07-01 の安全の性質のテストのすべてについて、どのテストが確かめるかの一覧を作る（ロードマップの P4 の完了条件の準備）。

## 読む設計書

- [対話の画面](../../design/05-interface/05-02-tui.md)の「`/` で始まるコマンド」「確認の画面」「出力の示し方」（2026-10-18 に加えた、目に見えない文字と幅の定まらない文字の変換）
- [セッション](../../design/01-agent/01-05-sessions.md)の「巻き戻し」（会話を戻せる時点、不完全な時点、戻せないものの表示）
- [文脈の管理](../../design/01-agent/01-04-context.md)の「圧縮」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」（示す前のキーと示した直後のキーを捨てる。答えのキーのすぐ上に `benitoiteh` の事実を置く規則（2026-10-17）と、事実が収まらないときは許す答えを受け付けない規則（2026-10-18））
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「モードを変える操作」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「端末からの切り離し」（3 の一覧のため）
- [テスト](../../design/07-quality/07-01-testing.md)の安全の性質のテスト
- [ロードマップ](../../design/00-overview/00-03-roadmap.md)の「段階」の P4 の完了条件
- 既存のもの（取り込み済みのもの）: `Agent::compact`・`CompactReport`・`CompactOutcome`（P4-01）、`Agent::checkpoints`・`CheckpointInfo.conversation_available`・`Agent::rewind(checkpoint, scope, allow_incomplete, events) -> Result<RewindReport>`・戻せないものの一覧（`bash` と `web_fetch` を含む。`agent/rewind.rs` の非公開の関数）（P4-03、P4-04）、`exec::prepare`・`Prepared`・働き手のスレッドの仕事の形・`Agent::context_estimate()`・`context_window()`・`Summary.tokens`・`compactions`・`add_usage`（P4-06）、`ui/tui/confirmation.rs` の `Confirmation`・`ConfirmationGuard`・`fits`、`ui/tui/unicode.rs` の `cells`、`SafeText::strict`、`RunControl` の二つの印、`InteractiveApprover::channel()`、入力の履歴（P4-07）。型と口は [10-interfaces.md](../10-interfaces.md) の P4-06・P4-07 の節を見る。

## この作業が認める型の変更

- 働き手のスレッドの仕事に、`Compact`・`Status`・`ListCheckpoints`・`PreviewRewind`・`Rewind`・`End` を加える（`Agent` を `Mutex` で包まない）。
- `Agent` の状態の写し（例: `AgentStatus`。セッションの ID、モード、提供者の設定の名前とモデル、ホスト、サンドボックスの方針、承認の方針、文脈の見積もりと上限、読んだプロジェクトの指示のファイルと読まなかったもの）を返す口。トークンの累計は `AgentStatus` に入れない（下の `/status`）。
- 戻さずに戻せないものの一覧を返す口（例: `Agent::rewind_preview(checkpoint, scope) -> RewindPreview`）。一覧は `RewindReport` と同じ非公開の関数で作る（`web_fetch` は既に含まれている）。
- P4-03 の時点の一覧の口を広げ、依頼の先頭（今の世代なら会話から、なければ記録の行から。取れなければ `(not available)`）と、道具の前の時点の道具の名前と引数を返す（`agent` は `ui` に依存しないので、一行にするのは画面の側で `ui::tui::text::tool_call` を使う）。
- `RewindError` に、戻す前の時点が不完全なことを表す変種（例: `IncompleteBefore { checkpoint: String, incomplete: Vec<String> }`）を加え、`restore_files` はこれを返す（今の `RewindError::Invalid(INCOMPLETE_BEFORE)` の代わり）。
- セッションを終える口（例: `Agent::end(reason)`。`session_end` を一行書く）。
- 画面の芯が、新しいセッションを作る関数を外から受け取る形（`/new` とテストのため）。関数は、`ui` に置く公開の型（例: `ui::tui::NewSession { agent, session, status, diagnostics }`）を返す。`Prepared` は `cli` の中に置いたままでよい。
- `ui/tui/confirmation.rs` の `Confirmation` に、`/rewind` の確かめの種類を加える。

## 作るもの

### 1. 入力の解釈

- コマンドとして解釈するのは、画面の入力欄で Enter を押して送った文だけとする。LLM の出力、道具の結果、`AgentInput`、記録、再開した会話の文は、コマンドとして解釈しない。
- 文の先頭が `/` のとき、`/` の後から最初の空白か改行までを名前とする。名前が `[a-z]+` ならコマンドとする（知る名前でも知らない名前でも）。そうでなければ（`/Help`、`/usr/bin/env` など）、依頼として送る。先頭の空白は除かず、空白で始まる文は依頼として送る。名前の後の空白の後ろの全文（改行を含む）を引数とする。`/` だけの文と知らない名前は、`/help` を案内して示す（LLM に送らない）。引数を取らないコマンド（`/help`・`/status`・`/rewind`・`/new`・`/quit`）に、空白だけでない引数を付けたら誤りとして示す（空白だけの引数は、ないものとして扱う）。
- `//` で始まる文は、先頭の `/` を一つ除いて、依頼・操縦・後続として送る（三つで同じ扱い）。
- 一連の処理の途中（圧縮と戻す処理の最中を含む）は、Enter で送った `/quit` だけを受け付け、ほかは「処理の途中では使えない」と示す。処理の途中の Alt+Enter（後続）で `/` で始まる文（`/quit` を含む）は拒む。
- 入力の履歴（P4-07）には、打ったままの文を入れる（知らない名前も入れる）。`/rewind` の選択と確かめのキーは入れない。

### 2. コマンド

- `/help`: コマンドと入力の操作の一覧（05-02 の表）を示す。
- `/status`: `AgentStatus` と、画面の芯が持つ値を、出力の流れ（履歴）に書く。セッションの ID、モード、提供者の設定の名前とモデル、ホストの名前（`base_url` は示さない）、処理系（この作業では `not checked`。英文は `text` に置き、処理系を探しに行かない。P2-05 で埋める）、サンドボックスの方針（`--no-sandbox` で外したかを含む）、承認の方針、使ったトークンの量、文脈の見積もりと上限、読んだプロジェクトの指示のファイルと読まなかったもの。トークンの量は、働き手のスレッドが、このプロセスで、このセッションの依頼ごとの `report.summary.tokens` と、手動の圧縮の `CompactReport.usage`（`Summary::add_usage`）を足したものとし、入力、キャッシュの読み、キャッシュの書き、出力を示す（再開の前の分は含めない。費用は示さない。BH-035）。文字列は `terminal_text(.., false)` で変えて示す。
- `/compact [指示]`: 働き手のスレッドの `Compact` の仕事で、`RunControl` を作って処理中の印を立て、`Agent::compact` に `cancel_step` の印を渡して呼ぶ（`Worker::cancel` は二つの印を両方立てるので、一度の ESC と Ctrl+C のどちらでも止まる）。処理中は状態の行に示す。結果（済んだ、圧縮するものがない、取り消した、失敗した）と、要約した項目の数と残した項目の数を一行で示す。手動の圧縮の後は、結果によらず `Record::Summary` を一行追記する（`compactions` は `Compacted` のときだけ 1 を足す）。
- `/rewind`:
  1. 時点の一覧（新しい順。番の番号、理由、時刻（UTC）、依頼の先頭か道具の一行、会話を戻せるか、不完全か）を選択の画面で示し、↑・↓ と Enter で選ぶ。一覧が下の部分の高さを超えたら、選択の窓を動かす。ESC でやめる。時点が一つもなければ、その旨を一行示して終える。
  2. 範囲（「files and conversation」「files only」「conversation only」）を選ぶ。時点の一覧の「会話を戻せるか」が偽の時点（道具の前の時点、戻す操作の前の時点、圧縮か会話の巻き戻しで世代が変わる前の時点）では、「files only」だけを選べる。
  3. `rewind_preview` で戻せないもの（選んだ時点の後の `bash` と `web_fetch` の呼び出しなど）を得て示す（多ければ最初の 20 件と `N more`）。`[y] rewind` `[n] cancel` で確かめる。`benitoiteh` が作る事実（選んだ時点、範囲、範囲がファイルを含むときは戻す前の状態も時点として記録すること、選んだ時点が不完全なら増えたファイルを消さないこと）を答えのキーのすぐ上に置き、戻せないものの一覧はその前に置く。
  4. `allow_incomplete = false` で `rewind` を呼ぶ。`RewindError::IncompleteBefore` が返れば、その `incomplete` から不完全の理由を示し（同時に届く `AgentEvent::Checkpoint` に頼らない）、`[f] rewind anyway` `[n] cancel` で確かめ、`[f]` なら `true` で呼び直す（戻す前の時点はもう一度記録される）。
  5. 結果（書き戻した数、消した数、作った数、戻せなかったものの一覧（多ければ最初の 20 件と `N more`））を履歴に示す。
  - 3 と 4 の確かめは、`Confirmation` の `/rewind` の種類として、P4-07 と同じ `ConfirmationGuard`（示す前に届いていたキーと貼り付けを捨てる、示した後 300 ミリ秒より前のキーを捨てる、ESC は拒否、端末の大きさが変わったら掛け直す）と `fits` を使う。題・事実・答えのキーが収まらなければ `[y]`・`[f]` を受け付けない（03-03）。
  - 選択の画面と確かめの画面（入力欄の下の部分）に示す文字列（依頼の先頭、道具の一行、戻せないものの一覧、不完全の理由）は、すべて `SafeText::strict` で変え、幅は `unicode::cells` で数える。履歴に書く結果の表示（戻せなかったもののパスと理由）は `terminal_text(.., false)` でよい。
  - 戻す処理は取り消せない。最中のキーの Ctrl+C・`/quit` と一度目の終える合図では、終わるまで待つことを状態の行に示す。二度目の外からの終える合図（SIGTERM・SIGHUP・外からの SIGINT）だけは、P4-06 の動きのとおりすぐに終える（終了状態 130）。そのときは、端末を戻した後に、戻す前の時点の ID が分かっていれば示す。
- `/new`: 新しいセッションを先に準備する。準備は画面のスレッドで止まって行うので、先に一行（例: `Starting a new session...`）示す。準備の関数は、起動の指定から `--resume`・`--continue` と起動のときの依頼（`Request::Text(String::new())` にする）を除き、今のセッションのモード（`--mode`）とプロジェクトのディレクトリ（`--project`、`Agent::project_dir()`）を明示して加えたもので、`exec::prepare(.., check_availability, &mut |_| false)` を呼ぶ。通常モードでは、今の承認の方針とサンドボックスの方針も明示して加える（制限モードでは `--sandbox`・`--approval` を加えない。通常モードだけの指定の誤りになるため）。設定のファイルは読み直すが、読み直して変わりうるのは、提供者とモデル、上限、`web_fetch` の許すドメイン、プロジェクトの指示のファイルだけとなる（モードとプロジェクトとサンドボックスと承認の方針は明示した値になる。03-01）。
  - 準備に失敗したら、今のセッションを続けて誤りを示す。
  - 成功したら、働き手のスレッドの `End` の仕事で今のセッションを終え（`session_end` を一行書く。`outcome` は `completed`、`reason` は `text` の定数（例: `New session started.`））、`Agent` を落とした（一時ディレクトリが消えた）ことを画面のスレッドに知らせてから、新しい `Agent` に替える。cli の側は、新しいセッションの `panic_record` で `panic::install` をし直し、確認の受け口（`InteractiveApprover::channel()` と `with_approver`・`with_approval_requests`）を新しい `Agent` ごとに作り直す。起動のときの表示（提供者・モデル・ホスト・送る内容・サンドボックス・診断）を出し直す。
  - このセッションで許したコマンドとドメインの集合と、読んだ版の記録は引き継がない。前の画面の出力は端末の履歴に残る。入力の履歴は同じプロジェクトのものを使い続ける。
- `/quit`: 終える（終了状態 0）。一連の処理の途中なら、P4-06 の「処理の途中で終えるときの流れ」を使う。
- モードを変えるコマンドと、提供者やモデルを変えるコマンドは設けない（05-02）。

### 3. 安全の性質のテストの一覧

- 07-01 の安全の性質のすべての項目について、確かめるテストの名前（ファイルと関数）を挙げた一覧を、`docs/plan/results/P4-safety.md`（新）に表で書く。項目ごとに、確かめる環境（Codex の中、手元の macOS、aarch64 のコンテナ、x86_64 の Linux）を記し、まだ確かめられないものは理由（「`v0.0.7` の処理系の連携で確かめる」「オーケストレータが手元で走らせる」など）を記す。制限モードの処理系に関わる項目は、P2-11 が作る `docs/plan/results/P2-safety.md` を参照する、と書いて分ける。
- P4 で加えた主のプロセスの操作として、次を必ず含める（07-01 の箇条書きにまだないものも含める）。足りないテストがあれば、この作業で加える。
  - 圧縮、再開、巻き戻し（`/rewind` の確かめを含む）、制限モードから通常モードへの再開の確認。
  - `web_fetch` の URL の検査と、名前の解決の後の検査。
  - `glob`・`grep`・`read` のリンクを辿らない読み方と上限（P4-09。07-01 の「読まないファイル」の項目も通し直す）。
  - 確認の画面の並び（LLM の説明は事実より前に 10 行まで置き、答えのキーのすぐ上には `benitoiteh` の事実を置く。03-03）。事実が収まらないときに `y`・`a`・`s` を受け付けないこと、`␠×N` の表示（03-03、2026-10-18）。
  - 端末の制御の列と、目に見えない文字と幅の定まらない文字の変換（U+3164 など。05-02）。
  - 入力の守り（示す前と示した直後のキー、貼り付け、端末の大きさを変えたときの掛け直し）。入力の履歴のファイルの扱い。
  - 道具の子プロセスが制御の端末を持たないこと（03-02「端末からの切り離し」、BH-097。`__session` の失敗、Linux の `normal_sandbox_has_no_controlling_terminal`、macOS で `/dev/tty` を開けず書けないこと）。
  - `/new` が、許したコマンドとドメインの集合とモードを引き継がないこと。LLM の文の `/` で始まる文が実行されないこと。
  - `escalate` で `[a]` を選べないことを、対話の画面でも確かめること。
- この作業の時点では、P4 の完了条件のうち、`create_project` と処理系の項目が残ることを一覧に記す。オーケストレータは、取り込んだ後に、すべてのテストを手元の macOS、aarch64 のコンテナ、x86_64 の Linux で走らせ、07-01 の求める環境の情報（OS と版、CPU、カーネル、Landlock の ABI の版、bubblewrap の版、処理系の版とハッシュ（連携の前なので「なし」））を一覧に添える。

## 触るファイル

- `crates/benitoiteh/src/ui/`（コマンドの解釈、`/rewind` の選択の画面、`/status` の表示、`confirmation.rs` の `/rewind` の種類、`NewSession`、`text.rs`）
- `crates/benitoiteh/src/agent/`（`AgentStatus`、`rewind_preview`、時点の一覧の欄、`end`）
- `crates/benitoiteh/src/rewind/`（`RewindError::IncompleteBefore`）
- `crates/benitoiteh/src/cli/`（`/new` のための新しいセッションを作る関数、`panic::install` のし直し、確認の受け口の作り直し）
- `crates/benitoiteh/tests/`
- `docs/plan/results/P4-safety.md`（新。この一か所だけ）

## テスト

端末なしで、画面の芯にキーのイベントの列と書き先を渡して確かめる。`/new` は、模擬の提供者と一時のディレクトリで、`Agent::new` から `NewSession` を作る関数を渡す。`/rewind` の状態は、サンドボックスを要さない通常モードの `write`・`edit` で作る。

- コマンドの解釈: 知らない名前、`/` だけ、大文字の名前（依頼として送る）、`/usr/bin/env` の形（依頼として送る）、先頭の空白、引数を取らないコマンドの引数（空白だけの引数を含む）、`//` の扱い（依頼・操縦・後続）、処理の途中で使えないこと、Alt+Enter の `/` の文（`/quit` を含む）の拒否、`/compact` の複数行の指示が要約の依頼に入ること（模擬の提供者）。
- 安全の性質（07-01）: LLM の応答の文に `/new`・`/rewind` などを含めても何も実行されない。`/rewind` の確かめの画面を示す前に打った `y` と、示した直後の `y` で戻らない。確かめの事実が収まらない端末の大きさで `[y]`・`[f]` を受け付けない。依頼の先頭に U+3164 などを含めても、選択の画面で目に見える形になる。
- `/status` の表示の内容（描画の純関数で）。トークンの量が、依頼ごとの集計と手動の圧縮の使用量の和と一致する。`base_url` が示されない。
- `/compact`: 取り消し（ESC、Ctrl+C）で止まり、結果によらず `Record::Summary` が一行増え、`compactions` は済んだときだけ増える。
- `/rewind`: 選択の画面のキーの操作、時点がないときの表示、会話を戻せない時点（道具の前の時点、会話を戻した後の古い時点、圧縮の前の時点）で範囲が「files only」に限られること、`[y]` でなければ戻さないこと、`IncompleteBefore` で `[f]` を求め理由を示すこと、戻せないものの一覧に `bash` と `web_fetch` が入ること、戻した結果の表示。ファイルと会話が選んだ時点に戻る。
- `/new`: 新しいセッションの ID になり、前のセッションの記録の最後の行が、`reason` が `New session started.` の `session_end` である。前の一時ディレクトリが消え、`[a]` で許したコマンドが引き継がれない。`--resume` で始めた制限モードのセッションで、利用者の設定の既定のモードが通常モードでも、`/new` の後のモードは制限モードで、プロジェクトは前のセッションのものである。準備に失敗したら今のセッションが続く。
- `/quit`: 芯が終了状態 0 を返し、働き手のスレッドが閉じる。処理の途中でも同じ。端末を戻す列は、`tests/tui/pty.rs` の形で、本物の擬似端末で `/quit` を打って確かめる。

## 手で確かめる項目（オーケストレータ）

完了の報告に手順を書く。本物の端末での `/rewind` の選択の操作と描き直し、`/new` の前後の出力、処理の途中の `/quit` の後の `stty -a`。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、各コマンドの出力の例（文字の写し）、足した英文の全文、`docs/plan/results/P4-safety.md` の要約を書く。
