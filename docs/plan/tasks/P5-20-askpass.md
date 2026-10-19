# P5-20 秘密の入力の段 2（サンドボックスの外のコマンドの askpass の仲介）

## 目的

設計書 03-04「段 2: `benitoiteh` が秘密の入力を仲介する」に 2026-10-19 に決めた最初の版（BH-104 を閉じた決定）を実装する。通常モードで、範囲の外の実行の要求（`escalate`）を利用者が承認してサンドボックスの外で起動したコマンドと、`danger-full-access` で起動したコマンドの、`SSH_ASKPASS`・`GIT_ASKPASS`・`SUDO_ASKPASS` の求めを、対話の画面の入力の欄で仲介する。この作業で本物の LLM の提供者を呼ばない。

## 読む設計書

- [秘密の入力](../../design/03-safety/03-04-secrets.md)の「秘密を LLM に通さない」「段 1」「段 2」
- [対話の画面](../../design/05-interface/05-02-tui.md)の「確認の画面」「秘密の入力の欄」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」「通常モードの承認の方針」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「子プロセスの環境」「端末からの切り離し」
- 既存のもの: `crates/benitoiteh/src/tools/bash.rs`（`escalate`、子プロセスの環境）、`src/sandbox/`（`__session`、環境の組み立て）、`src/cli/mod.rs`（補助の入口の分け方）、`src/ui/tui/`（確認の画面、入力、知らせ）、`src/agent/`（道具の実行と取り消し）

## この作業が認める依存

- `zeroize`（秘密を消すため）。版を `=` で固定し、`docs/plan/10-interfaces.md` の依存の表への追加はオーケストレータが行う。加えた前と後の実行ファイルの大きさを完了の報告に書く。

## 作るもの

### 1. 補助（`benitoiteh-ssh-askpass`・`benitoiteh-git-askpass`・`benitoiteh-sudo-askpass`）

- セッションごとに、利用者だけが読み書きできる短いパスのディレクトリ（Unix のソケットの名前の上限、macOS で 104 バイトに収まること）を作り、その中に三つの名前で `benitoiteh` の実行ファイルへのシンボリックリンクと、窓口のソケットを置く。セッションが終われば消す（P5-18 の再帰しない削除）。
- `benitoiteh` の入口は、`argv[0]` の最後の要素がこれらの名前のとき、補助として動く（panic の hook と引数の解釈より前に分ける）。補助は標準入力を読まない。
- 合言葉の環境変数か窓口の環境変数がなければ、設計書の文（`Secret input is only available for commands approved to run outside the sandbox in interactive sessions.` の形）を標準エラー出力に一行書き、1 で終わる。
- 窓口があれば、ソケットにつなぎ、合言葉、求めの種類（起動の名前と `SSH_ASKPASS_PROMPT`）、求めの文（一つ目の引数。4 KiB まで、壊れた UTF-8 は置き換える）、自分と親のプロセスの番号を、長さを決めた一つの要求として送り、答えを受ける。答えが秘密なら、秘密と改行を標準出力に書いて 0 で、確かめの許しなら何も書かずに 0 で、それ以外は 1 で終わる。タッチの知らせ（`SSH_ASKPASS_PROMPT=none`）は、知らせを送った後、合図で止められるまで待つ。

### 2. 子プロセスの環境（`bash`）

- 通常モードの `bash` の子プロセスには、いつも `SSH_ASKPASS`・`GIT_ASKPASS`・`SUDO_ASKPASS` を補助に向け、`SSH_ASKPASS_REQUIRE=force` と `GIT_TERMINAL_PROMPT=0` を加える。
- 仲介する呼び出し（対話の実行で、`escalate` を承認してサンドボックスの外で起動したものと、`danger-full-access` で起動したもの）にだけ、窓口のパスと、その呼び出しに限る 32 バイトの乱数の合言葉（16 進）を加える。合言葉は、呼び出しが終わるか、取り消されるか、時間の上限に達したら無効にする。
- 利用者の設定（例: `[secrets] askpass = false`。既定は真）で、補助に向けないようにできる（利用者の `SSH_ASKPASS` を使う）。プロジェクトの設定ファイルでは受け付けない。

### 3. 窓口（主のプロセス）

- 別のスレッドでソケットを受け、要求を読む時間（例: 5 秒）と大きさ（例: 8 KiB）に上限を掛ける。知らない合言葉と無効にした合言葉は断る。Linux では、つないできたプロセスの利用者の ID が自分と同じことを確かめる。
- 求めは一度に一つずつ画面に渡し、一つの呼び出しで 3 回を超える求めは断る。答えを覚えて使い回さない。利用者の答えを待つ時間は、その `bash` の時間の上限に数えない。

### 4. 入力の欄（対話の画面）

- 設計書 03-04 の「入力の欄」のとおりに示す。`benitoiteh` の事実（サンドボックスの外のコマンドが秘密を求めていること、道具の呼び出しの番号、コマンドの全体、モード、作業のディレクトリ）と、補助の報告（プロセスの番号、求めの種類。「確かめていない」と示す）と、信頼しない求めの文（枠で分け、10 行まで）を分ける。
- 入力は伏せ字で、見せる操作を別に設ける。確かめの求めは「はい」「いいえ」、タッチの知らせは答えを求めない知らせとする。ESC は取り消し。欄を示す前と、示してから 300 ミリ秒の間のキーの入力は捨てる。秘密を受け取ったコマンドは秘密を外へ出しうることを、欄に示す。
- 入力した値は画面の履歴に残さない。

### 5. 秘密の扱い

- 秘密は `zeroize` で包み、補助に渡したら消す。セッションの記録、画面の履歴、道具の結果には、求めを示した・答えた・取り消したことだけを書く。
- その呼び出しの出力を LLM と記録に渡す前に、秘密と同じバイトの並びを伏せ字に置き換える。

### 6. 仲介しないとき

- 制限モード、非対話の実行、サンドボックスの中の呼び出しでは、窓口を作らないか、合言葉を渡さない。補助は 1. の文で失敗する。

## 触るファイル

- `crates/benitoiteh/src/cli/mod.rs`（補助の入口）、新しい `src/askpass/`（補助、窓口、要求の形）
- `crates/benitoiteh/src/tools/bash.rs`、`src/sandbox/`（子プロセスの環境）
- `crates/benitoiteh/src/ui/tui/`（入力の欄）、`src/agent/`（呼び出しの終わりと取り消しで合言葉を無効にする）
- `crates/benitoiteh/src/config/mod.rs`（`[secrets]`）
- `crates/benitoiteh/Cargo.toml`、`Cargo.lock`
- テスト: 補助の単体のテスト、窓口のテスト、`tests/` の `bash` の環境のテスト、`src/ui/tui/tests.rs`

## テスト

- 補助: 合言葉がなければ決まった文で 1 で終わること。窓口との要求と答えの形（秘密、確かめ、取り消し、タッチの知らせ）。
- 窓口: 知らない合言葉と無効にした合言葉を断ること。4 回目の求めを断ること。上限を超える要求を断ること。
- 環境: 通常モードの `bash` の子に補助の変数が入り、仲介する呼び出しにだけ合言葉と窓口が入ること。サンドボックスの中の呼び出しと制限モードと非対話の実行に入らないこと。
- 画面: 入力の欄が事実と信頼しない文を分けて示し、伏せ字で、ESC で取り消し、早すぎるキーを捨てること。
- 秘密: 記録と道具の結果に秘密が残らず、出力の中の秘密が伏せ字に置き換わること。
- 本物の `ssh`・`git` との結び付き: `git` の HTTPS の認証の求め（試しの `GIT_ASKPASS` の起動）で、補助が答えを渡せること（ネットワークに出ない形で。例: `git credential fill` か、`GIT_ASKPASS` を直に起動する）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、入力の欄の画面の例と、実行ファイルの大きさの前と後を書く。

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。行の番号は `main`（`5d7151c`）のもの。P5-19 を取り込んだ後に始めるので、`src/ui/tui/` と `src/cli/{exec,tui}.rs` の行の番号は目安とする。

1. **補助の入口**: `src/cli/mod.rs:25-39` の `__sandbox`・`__session`・`__image` の分岐と `panic::install`（:39）より前に、`args_os()` の一つ目の `Path::file_name()` が `benitoiteh-ssh-askpass`・`benitoiteh-git-askpass`・`benitoiteh-sudo-askpass` のどれかなら補助として動き、return する。OpenSSH（`readpass.c`）、sudo（`tgetpass.c`）、git（`prompt.c`）はどれも、渡したパスを `argv[0]` にして起動する。環境の変数に入れるパスは絶対パスにする。リンクの先は正規化した今の実行ファイル（`exec.rs:302` の `helper`）とする。親のプロセスの番号は `rustix::process::getppid()`、自分の番号は `std::process::id()`。`unsafe` は要らない。
2. **子プロセスの環境**:
   - 補助に向ける常の変数（`SSH_ASKPASS`・`GIT_ASKPASS`・`SUDO_ASKPASS`・`SSH_ASKPASS_REQUIRE=force`・`GIT_TERMINAL_PROMPT=0`）は、`exec.rs:470` の `child_environment(...)` の後で `ShellContext.env`（`tools/mod.rs:357`）に足す（`child_environment` には呼び出し元が多いので引数は変えない）。外から引き継いだ窓口と合言葉の変数（例: `BENITOITEH_ASKPASS_SOCKET`・`BENITOITEH_ASKPASS_TOKEN`）は、`child_environment`（`config/environment.rs:25` 付近）でいつも除く。
   - 呼び出しに限る変数（窓口のパスと合言葉）は、`bash.rs` の `prepare`（:187-238）のサンドボックスの外の枝（:225 以降）でだけ、仲介するときに足す。
   - 仲介する呼び出しの条件: 窓口がある（対話の実行）、かつサンドボックスの外で起動する、かつ (`sandbox` が `danger-full-access`) か (`escalate` で、サンドボックスがあり（`policy.is_some()`）、承認の結果が `Approved`)。`approval/mod.rs:144-152` では、サンドボックスがあるときの `escalate` は `Ask` か `Deny` にしかならない。`tool_context.noninteractive` は通常モードではいつも偽なので、対話かどうかの判定に使わない。`ShellContext` に窓口（例: `askpass: Option<Arc<..>>`）を足し、あることで判定する。`ShellContext` を構造体の式で作る所（`exec.rs:489`、`tests/agent_loop.rs:177`、`tests/tui.rs:308`、`tests/support/bash.rs:80`、`tests/support/rewind.rs:67`）はすべて直す。
   - サンドボックスを掛けられずに動かすとき（`--no-sandbox` など、`policy` がなく `danger-full-access` でもないとき）は、設計書の範囲（`escalate` と `danger-full-access`）に当たらないので仲介しない。
3. **補助の置き場所**: 補助のディレクトリは、サンドボックスの中から読めて起動できる場所に置き、サンドボックスから書ける場所の下には置かない（中から起動されても、決まった文で失敗して LLM に返せるようにするため。macOS の通常モードは読み取りと起動を許し（`macos.rs:68-73, 100-102`）、Linux は `--ro-bind / /` で、Landlock は起動を限らない）。セッションのディレクトリ（データの場所の下で、サンドボックスから読めない）と、セッションの一時ディレクトリ（サンドボックスから書ける）は使わない。作った場所の正規化したパスが、どの書ける場所（プロジェクト、一時ディレクトリ、足した場所）の下にもないことを確かめる。サンドボックスの中から窓口にはつなげない（Seatbelt は Unix の宛先を許さず、Linux の seccomp は `socket` を拒む。`filter.rs:40-48`）。
4. **窓口のソケット**:
   - `paths.temporary_root` の下に `bha-<セッション ID>/`（0700。`exec.rs:702-719` の `private_directory` と同じ確かめ）を作り、ソケット `sock` と三つのリンクを直に置く。ソケットのパスが 103 バイトを超えるときは `/tmp/bha-<ID>` に替え、それでも超えるか、書ける場所の下に当たるときは仲介を止め（呼び出しに限る変数を足さない）、起動の診断を一行示す。
   - 後始末は `session::remove::directory`（`pub(super)` を `pub(crate)` に広げる）で行い、その前に `sock` を名前で `unlink` する。落ちた後の残りは、`session/mod.rs:321-345` の `cleanup` が `bha-` の接頭辞も同じ規則で扱う。
   - std の `UnixListener`・`UnixStream` を使い、読む時間の上限を 5 秒にする。Linux では `rustix::net::sockopt::socket_peercred` で相手の `uid` を得て `rustix::process::getuid()` と比べる（`net` の機能は Linux の対象の rustix にだけある。`#[cfg(target_os = "linux")]`）。macOS では相手を確かめず、0700 のディレクトリと合言葉で守る。依存は `zeroize` のほかに増やさない。
5. **要求と答えの形**:
   - 補助から窓口へ: 4 バイトのビッグエンディアンの長さと JSON（例: `{"v":1,"token":"<64 桁の 16 進>","kind":"ssh|git|sudo","ssh_prompt":null|"confirm"|"none","prompt":"…","pid":N,"ppid":N}`）。8 KiB を超えるものと JSON の誤りは断る。知らない `SSH_ASKPASS_PROMPT` の値は断る。
   - 窓口から補助へ: 1 バイトの状態（`S` 秘密が続く、`Y` 許す、`N` いいえ・取り消し・断り）と、`S` なら秘密のバイト。秘密は JSON に入れず、`Zeroizing` の入れ物から直に書いて消す。
   - タッチの知らせ（`SSH_ASKPASS_PROMPT=none`）は、窓口が答えずに接続を開いたまま待ち、補助が止められて接続が切れたら知らせを閉じる。補助は窓口が閉じたら 0 で終わる。
   - 合言葉の比較は、時間が値に依らない形にする。回数の上限（3 回）は、正しい合言葉を持つ求め（確かめとタッチの知らせを含む）を数える。
   - 補助の失敗の文は `askpass/text.rs` の英語の定数にする（変数がない: 設計書の文。つなげない: 例 `Secret input is no longer available for this command.`。断られた: 例 `Secret input was refused.`）。どれも 1 で終わる。
6. **時間の上限と取り消し**: `sandbox/run.rs:331-357` の時間の判定で、経った時間から利用者を待った時間（タッチの知らせの間を含む）を引く。`RunLimits` は `Copy` なので、待った時間を数える共有の物を別の引数で渡す新しい関数（例: `run_with_pause`）を足し、今の `run` はそれに任せる。合言葉は、止める合図を送る時点（`run.rs:363` 付近の `end != Exited` の枝。猶予が最大 10 秒あるため）と、`bash.rs` の呼び出しの守りの `Drop` で無効にする。`bash` は並べて動かす道具でない（`definitions.rs:118`）ので、一つのエージェントで同時に仲介するのは一つの呼び出しだけである。
7. **入力の欄**: `confirmation.rs` の `Confirmation` の新しい種類として作り、300 ミリ秒の判定（:15-42）、描画の後の早すぎるキーの破棄（`runtime.rs:199-208`）、端末の大きさが変わったときの扱い（`mod.rs:142-147`）、Ctrl+C・Ctrl+D の扱い（:675-686）を使い回す。
   - 求めは、`InteractiveApprover`（`approval/interactive.rs`）と同じく返事の口を付けて送り、`Worker::drain` が `!screen.confirming()` のときに開く（`worker.rs:455-465` 付近）。結び付けは `tui.rs:152-155` と `/new`・`/fork` の作り直しの両方で行う。
   - `answer()`（:647-659）は欄の本文を画面の履歴に写すが、秘密の欄では写さず、決まった一行（例: `Secret input for <呼び出しの番号>: answered`。`answered`・`declined`・`cancelled`・`refused`）だけを足す。伏せ字の数も残さない。
   - ESC はその求めだけを取り消し、`bash` は続く。Ctrl+C は今と同じく依頼の全体を取り消す。事実が収まらないとき（`fits`、:227）は Enter と「はい」を受け付けない。信頼しない求めの文は、`justification` と同じ字下げの印と薄い色で、10 行まで示す。呼び出しが終わって合言葉を無効にしたときは、欄を閉じて「閉じた」と記す。
   - 値に `\n`・`\r`・NUL は入れない。貼り付けは許し、末尾の改行だけを除く。上限は 4 KiB。見せる操作は Tab で切り替え、欄ごとに伏せ字に戻す。
   - 欄に示す事実のうち、呼び出しの番号は `invocation.call_id`、作業のディレクトリは `context.project.real_path()`、モードは `normal` に `danger-full-access` か `escalated` を添える。
8. **秘密を消すことと伏せ字への置き換えの両立**: 平文は補助へ渡したらすぐ消し、呼び出しの守りには秘密の `(長さ, SHA-256)` だけを残す（`sha2` は今の依存）。`bash.rs` の `output.tail()`（:111）の後、`ToolOutput` を作る前に、出力の各位置で同じ長さの窓の SHA-256 を比べ、一致した所を `[secret]` に置き換える。秘密を一度でも渡した呼び出しでは、生の出力を書いた保存のファイル（`spill_writer`、:83）を捨て、保存しなかったことを示す決まった一行を結果に足す（LLM が `read` で生の出力を読めないようにするため）。
9. **記録**: `Record` の種類は増やさない。`bash` の結果の文に、求めごとの決まった一行（例: `Secret prompt (git): answered.`）を足し、`Record::ToolResult` に残す。求めの文と値は書かない。
10. **設定**: `UserConfig`（`config/mod.rs:323-336`）に `secrets: SecretsConfig { askpass: bool }`（既定は真、`#[serde(default, deny_unknown_fields)]`）を足す。`askpass = false` のときは、五つの変数をどれも上書きせず引き継いだまま渡し、補助のディレクトリと窓口も作らない。プロジェクトの設定（`ProjectConfig`、:540-546）は `deny_unknown_fields` で `[secrets]` を断るので、そのことを確かめるテストを一つ加える。設計書 05-01 の設定の例は変えない（オーケストレータが取り込むときに行う）。
11. **非対話の実行と `/new`**: `exec`（`args.interactive` が偽）は窓口を作らない。補助のディレクトリとリンクは作り、補助は決まった文で失敗する。窓口と補助のディレクトリはセッションごとに作り直し、古いものは `Drop` で止めて消す。
12. **zeroize**: `Cargo.lock` に 1.9.1 が既にある（rustls が引く）。`zeroize = { version = "=1.9.1", default-features = false, features = ["alloc"] }` とする。
13. **本物との結び付きのテスト**は git に限る（`GIT_ASKPASS` を直に起動するか、`git credential fill`。ネットワークに出ない形で）。sudo が窓口の変数を引き継ぐかは確かめていない。
