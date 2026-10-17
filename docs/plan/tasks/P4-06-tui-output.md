# P4-06 対話の画面の土台と出力

## 目的

対話の画面（`benitoiteh [オプション] [依頼]`）の土台を作る（設計書 05-02 の「画面の構成」「出力の示し方」）。端末の履歴に出力を流し、画面の下の入力欄と状態の行だけを描き直すインラインの形とする。この作業では、依頼を送って結果を示すところまでを作る。操縦と後続、ESC の取り消し、入力の履歴の保存、確認の画面、対話の形での再開は P4-07、`/` で始まるコマンドは P4-08 で作る。後の二つが困らない形（4 と 6）をこの作業で決める。

## 読む設計書

- [対話の画面](../../design/05-interface/05-02-tui.md)の「画面の構成」「出力の示し方」「入力の操作」（この作業では Enter、Shift+Enter・Ctrl+J、Ctrl+C、Ctrl+D、↑・↓ のうちセッションの中の履歴だけ）
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「イベント」「取り消し」
- [文脈の管理](../../design/01-agent/01-04-context.md)の「送る内容の表示」「推論の内容の扱い」（画面には要約だけ。BH-015。Anthropic は BH-096）「文脈の大きさの見積もり」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「コマンドラインの形」「オプション」「終了状態」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「保証しないこと」（2026-10-17 に、サンドボックスの外で動くコマンドからの端末の操作を加えた。BH-097）
- [クレートの構成](../../design/06-impl/06-01-crate-structure.md)の依存の基準、「端末」（`crossterm`）、依存の向き（`agent` は `ui` を知らない）、「スレッドと非同期」「ビルドの設定」（panic のときに端末を戻す）
- [性能](../../design/06-impl/06-02-performance.md)の起動の時間と実行ファイルの大きさの目標と「測り方」
- 既存のもの: `src/cli/`（`mod.rs` の振り分け、`args.rs` の `ExecArgs::parse`、`exec.rs` の `prepare`（依頼の読み取り、診断の直接の出力、`panic::install`、承認の方針の固定、再開の経路）と `run`（`sync_channel(64)`、`Record::Summary`）、`panic.rs`、`signals.rs`、`text.rs`）、`src/config/mod.rs`（`resolve_exec` の `--approval` の扱い）、`src/session/quota.rs`（`STORAGE_LIMIT` の直接の出力）、`src/ui/mod.rs`（`terminal_text`、`ExecOutput`）、`src/agent/`（`Agent::run`、`AgentEvent`、`compaction/mod.rs` の見積もり）、`src/tools/`（`ToolOutput.display`、`write.rs` の差分）、`Cargo.toml`（`crossterm =0.29.0`・`unicode-width =0.2.2` は今は開発用の依存）、`tests/cli.rs`

## この作業が認める型の変更と依存

- `crossterm`（機能は `events` と `bracketed-paste` だけ）と `unicode-width` を、開発用から本体の依存に移し、`[dev-dependencies]` の二つの行を消す。推移的に入る `signal-hook 0.3.18`・`mio`・`signal-hook-mio`・`parking_lot` は、すでに `Cargo.lock` にあり、止まる理由にしない。実行ファイルの大きさを、取り込む前の `main` と比べて完了の報告に書く（06-02 の「測り方」）。
- `AgentEvent::ToolResult` に、画面の一行の要約（`screen: Option<String>`）と差分（`display: Option<String>`）を、`#[serde(default, skip_serializing_if = "Option::is_none")]` で加える。`exec --json` には差分を含めない（大きくなるため。`skip` するか、JSON の出力の側で除く）。`ToolOutput` に、道具が作る画面の一行の要約（`screen`）を加える。
- `Agent` に、見積もりと上限を読む口（`context_estimate() -> u64`、`context_window() -> Option<u64>`）を加える。
- `cli::exec::prepare` を、依頼を読まず、診断を返し、何度でも呼べる形に分ける（1）。`cli::panic::install` を、端末を戻すかを指定できる形に改める（3）。
- `session::quota` の上限の通知を、標準エラー出力に直接書かずに、呼び出し側に返す形（`AgentEvent::Error` の `kind: "storage_limit"` など）に改める。

## 作るもの

### 1. 起動

- 引数: `exec`・`doctor`・`sessions` のどれもないときを対話の形とする。依頼は省略できる。`--json`・`--output-last-message`・依頼の `-` は使い方の誤りとする。`--allow`・`--project-parent` は今のまま「まだ使えない」とする。対話の形の `--resume`・`--continue` は、P4-07 まで「まだ使えない」とする。使い方の文と「not available yet」の文から対話の形を除き、`tests/cli.rs` の `[]` の行の意味が対話の形の起動に変わることを直す。
- 端末の判定: 引数の解釈の直後、`DataPaths`・設定・セッションより前に、`std::io::IsTerminal` で標準入力と標準出力の両方を調べる。どちらかが端末でなければ、`exec` を使うよう一行（`benitoiteh: ...`）で示して終了状態 2 で終える（セッションのディレクトリを作らない）。
- 起動の準備の共有: 今の `prepare` を、依頼を読まない形（例: `prepare(args: &StartArgs) -> Result<Prepared, StartupError>`）に分ける。依頼の読み取りは `exec::run` に移す。診断（設定を無視したこと、サンドボックスを外したことなど）は `Vec<String>` で返し、呼んだ側が出す。`Prepared` は、`Agent` のほかに、画面に示す値（モード、提供者の設定の名前、モデル、ホストの名前、サンドボックスの方針の名前、`--no-sandbox` で外したか、`context_window`、起動のときの通知）を持つ。P4-08 の `/new` が、raw のモードの最中に呼び直せる形にする。
- 承認の方針: 確認の画面は P4-07 で作るので、この作業の間は対話の形でも `never` で動かす。`--approval` と利用者の設定の `normal.approval` が `never` でなければ、対話の形の誤り（P4-07 で使えるようになることを示す文言）として終了状態 2 で終える。起動のときに、確認が要る操作は拒否されることを一行で示す（英文は `text` に置き、P4-07 で除く）。
- 端末の準備: raw のモードにし、bracketed paste を有効にし、kitty のキーボードの拡張（`PushKeyboardEnhancementFlags(DISAMBIGUATE_ESCAPE_CODES)`）を問い合わせずに押す。起動のときに待つ問い合わせ（`cursor::position()`、キーボードの拡張の問い合わせ、DECRQM）はしない。代替の画面は使わない。

### 2. 画面の構成と描き方

- 画面の芯は、端末の大きさ、入力のイベントの列、`AgentEvent` の列、書き先（`Write`）を外から受け取る形にする。芯の中で `terminal::size()` や `enable_raw_mode` を呼ばない（端末なしでテストできるように）。
- 端末に書く部分は、`terminal_text` を通してしか作れない型（例: `SafeText`）と、自前の装飾の列挙だけを受け取る。制御の列は一つのモジュールで、`crossterm` の命令から作る。許す制御の列は、SGR、カーソルの上への移動と列への移動、行と下の消去、`?25`（カーソルの表示）、`?7`（行の折り返し）、`?2026`（同期した出力）、`?2004`（bracketed paste）、キーボードの拡張の押し戻しだけとする。
- 下の部分（入力欄と状態の行）: `DisableLineWrap` にして自分で折り返す（`unicode-width` で測る。タブは測る前に空白に展開する）。履歴に書く前に `EnableLineWrap` に戻す。状態の行は折り返さずに端末の幅で切る。入力欄は端末の高さの半分を超える行数ならカーソルの周りだけを示す。下の部分の合計は端末の高さ − 1 以下にする。高さが 3 未満の端末では、状態の行を省く。
- 一回の描画は、メモリに組み立ててから `write_all` と `flush` を一度ずつ行い、同期した出力（CSI ? 2026 h/l）で囲む（対応しない端末は無視するので、対応の有無は調べない）。描く間はカーソルを隠し、最後に入力欄の入力位置へ置く。
- 出力: 下の部分を消してから（前に描いた行数だけ上へ動いて `Clear(FromCursorDown)`）、端末の履歴に書き足し、下の部分を描き直す。一度履歴に書いた行は書き直さない。
- 端末の大きさが変わったら（`Resize`。続けて届いたものは一つにまとめる。SIGWINCH は `crossterm` の `events` が扱う）、同じ手順で下の部分を描き直す。狭めたときに端末が折り返し直して残骸が残りうることは受け入れ、手で確かめる項目に入れる。
- 状態の行: モード、提供者とモデル、文脈の見積もりと上限の比（上限がなければ見積もりだけ）、サンドボックスの方針、実行中なら経過の時間（1 秒ごとに描き直す）。見積もりは依頼と依頼の間に `Agent::context_estimate()` で読み、処理の途中は前の値か、`Compaction` のイベントの `estimate_after` で更新する（`Agent` を `Mutex` で包まない）。

### 3. 端末を戻すことと終え方

- 端末を戻す列（`EndSynchronizedUpdate`、SGR の解除、カーソルの表示、`EnableLineWrap`、bracketed paste を切る、キーボードの拡張を戻す、下の部分の後ろへ動いて `\r\n`）を組み立てる関数を作り、raw のモードを解く処理と合わせて、正常に終えるとき、誤りで終えるとき、シグナルのとき、panic のときに使う。
- panic: `panic = "abort"` なので Drop は走らない。`panic::install` を、端末を戻すかを指定できる形に改める（`prepare` の中の `install` が後から上書きしないようにする）。panic の hook は、画面のスレッドの錠に頼らず、dup した記述子へ戻す列を直接書き、起動のときに保存した termios を `rustix` の `tcsetattr` で戻してから、文言を書いて終了状態 3 で終える。
- シグナル: 終える印（SIGTERM・SIGHUP・外から送られた SIGINT）と、依頼ごとに作る取り消しの印（`Arc<AtomicBool>`）を分ける（今は一つの印を `run` の取り消しにも使っており、一度取り消すと以後の依頼がすべて取り消される）。SIGHUP の登録は対話の形だけに加える。画面のループは、目を覚ますたびに終える印を確かめる。
- 処理の途中で終えるとき（Ctrl+C の二度押し、Ctrl+D、終える印）は、取り消しの印を立て、ループが戻るまでイベントを受けて描き続け（受け取りをやめるとループが送れずに止まる）、戻ったら端末を戻して終える。依頼ごとに、`exec` と同じく `Record::Summary` を追記する。
- 終了状態: 利用者が終えたら 0、使い方と設定の誤りは 2、`run` の `Err`（記録の失敗など）は示してから 1、終える印で終えたら 130、panic は 3。
- raw のモードの間は、`ui` の書き手のほかに端末へ書かない。`session/quota.rs` の上限の通知を呼び出し側に返す形に改める。ほかに `eprintln!`・`io::stderr` で直接書く箇所がないことを `grep` で確かめ、完了の報告に結果を書く。

### 4. ループとの接続と待ち方

- `Agent` を別のスレッドで動かす。そのスレッドの仕事は、一連の処理（`run`）のほかに、後の P4-08 の `/compact` と P4-07 の取り消しの二つの段階や入力の受け口を載せられる形にする（例: 仕事の列挙と、仕事ごとの制御の値）。`Agent` は `ui` を知らない。
- 画面のスレッドは、`crossterm::event::poll` で待つ。待つ時間は、「次の 1 秒の更新までの時間」と「処理中は 33 ミリ秒、待っている間は 250 ミリ秒」の小さい方とする。起きたら、`AgentEvent` を `try_recv` でまとめて受け、描画は一度だけ行う。

### 5. 出力の示し方

- 信頼しない文字列は、端末に書く前にすべて `terminal_text` で制御の文字を目に見える形に変える。一行で示すものは `terminal_text(.., false)` を使う。対象: `TextDelta`、`ReasoningDelta`、`ToolCall` の名前と引数（JSON に整形した後にも通す。`serde_json` は C1 と文字の向きを変える文字を変えない）、`ToolResult` の `summary`・`screen`・`display`、`ApprovalResult` の要約・事実・`justification`・利用者の文、`Error` の文、`Retry` の理由、`SessionEnd` の理由、`SessionStart` の提供者・モデル・ホスト・指示のファイルのパス・読まなかった理由・通知、状態の行の文字列、`prepare` が返す診断、入力欄に打った文と貼った文。
- 送り先はホストの名前だけを示し、`base_url` は示さない（利用者の情報や問い合わせを含みうる）。
- 起動の直後（最初の依頼の前）に、`Prepared` の値から、提供者とモデル、要求を送るホスト、送る内容（01-04 の「送る内容の表示」）、サンドボックスの方針、通知を示す。読んだプロジェクトの指示のファイルは、最初の `SessionStart` のイベントで示す。
- LLM の応答の文: 届いた順に示す。改行で終わった行は履歴に書き、終わっていない行は下の部分の上に仮に示す（仮の行は Markdown の変換をせずに示す）。仮の行が下の部分の上限を超えたら、超えた分を折り返した行として履歴に確定する。仮の行は、`ToolCall`、`TurnEnd`、推論と応答の切り替わり、`Error`、`SessionEnd` で確定する。取り消したときは、中断したことを一行で示す。
- Markdown の変換（行ごと。制御の文字を変えた後の文字列に行う）: 見出しは `#` を残して太字に、箇条書きの印（`-`・`*`・`+`・`1.`。行頭の `* ` は箇条書き）は残す。強調（`**`・`*`）とインラインのコード（`` ` ``）は、一行の中で対になった印だけを装飾に変え、対にならない印はそのまま示す。インラインのコードの中は変えない。```` ``` ```` と `~~~` でコードのブロックに入って出る（中身は変えない）。行の終わりごとに装飾を解く。
- 推論の文: `ReasoningDelta` は要約と全文を区別しないので、すべてに上限を掛ける。`TurnStart` から、応答の文か `ToolCall` が来るまでを一つの区切りとし、最初の 3 行だけを薄い色で示し、省いたときは `...` の一行を示す（幅の曖昧な `…` を使わない）。
- 道具の呼び出しの一行（引数から作る。端末の幅で切る）: `read <path>`（offset と limit があれば `(<offset>-<offset+limit>)`）、`glob <pattern>`、`grep <pattern> <path>`、`write <path>`、`edit <path>`、`bash: <command の 1 行目>`、`web_fetch <url>`、知らない名前は名前と短くした JSON。
- 道具の結果の一行: `ToolOutput.screen` があればそれ（道具が作る。`read` は `N lines`、`bash` は `exit N`、`write`・`edit` は `+A -D` など）、なければ `ok` か `error` と、`summary` を短くしたもの。
- `write`・`edit` の差分（`display`）は、unified diff の形で、追加を緑、削除を赤で示す。200 行を超えたら残りを `N more lines` の一行にする。
- 誤り、取り消し、文脈の超過、圧縮、やり直し、上限の通知のイベントは、それぞれ一行で示す（誤りと取り消しは目立つ色）。

### 6. 入力（この作業の範囲）

- 処理するのは `KeyEventKind::Press` だけとする。
- Enter: 一連の処理の途中でなければ、入力欄の文を依頼として送る（空は送らない）。途中なら、P4-07 まで受け付けない（入力欄に残す）。Alt+Enter は無視する（P4-07 が後続に使う）。
- Shift+Enter（キーボードの拡張があるとき）と Ctrl+J: 入力欄の中で改行する。
- 文字の入力、左右の移動、Backspace、Delete、Home・End、貼り付け（bracketed paste の文を一度に入れる。`\r\n` と `\r` は `\n` に揃える）。
- ↑・↓: 入力欄が一行のとき、このセッションの前の入力を辿る（保存は P4-07）。
- Ctrl+C: 一連の処理の途中なら、入力欄の中身によらず、取り消しの印を立てる（P4-07 の二度の ESC と同じ扱い）。途中でなければ、入力欄が空でなければ空にし、空なら 1 秒以内に二度押すと終える（一度目で状態の行に示す。ほかのキーで数え直す）。
- Ctrl+D: 入力欄が空なら終える（処理の途中なら、3 の処理の途中で終えるときの流れ）。

## 触るファイル

- `crates/benitoiteh/Cargo.toml`（依存の移動）
- `crates/benitoiteh/src/cli/`（振り分け、`args.rs`、起動の準備の分け方、対話の起動、`panic.rs`、`signals.rs`、`text.rs`）
- `crates/benitoiteh/src/config/`（対話の形の `--approval` の扱い）
- `crates/benitoiteh/src/session/quota.rs`（通知の返し方）
- `crates/benitoiteh/src/ui/`（新しい `tui/` などの子のモジュール、`text.rs`）
- `crates/benitoiteh/src/agent/`（`ToolResult` の欄、見積もりの口、上限の通知のイベント）
- `crates/benitoiteh/src/tools/`（`ToolOutput.screen` を道具ごとに作る）
- `crates/benitoiteh/tests/`（`tests/cli.rs`、新しい `tests/tui.rs`）

## テスト

- 描画の純関数: 入力欄の折り返し（全角の文字、タブ）、状態の行の切り方と高さが 3 未満のとき、Markdown の変換の表、差分の色と 200 行の上限、道具の呼び出しと結果の一行、推論の 3 行と `...`。
- 安全の性質（07-01）: 信頼しない文字列（ESC の列、OSC 52（`\x1b]52;c;...\x07`）、OSC 0 の題名、`\r`、`\x08`、`\x9b2J`、`\u{202e}`、ESC の列を含む貼り付け）を含む応答の文、推論、道具の呼び出しの引数、道具の結果、ファイルの名前、差分、誤りの文、入力欄の文を、芯に与えて書き先に描かせる。出力の ESC がすべて許した列の始まりで、`\r\n` のほかの C0・C1・文字の向きを変える文字が残らないことを、検査する関数で確かめる。
- 端末を戻す列を組み立てる関数のユニットテスト（panic の道は統合テストで起こせないため）。
- 入力の状態の遷移（文字の入力、改行、貼り付けの改行の揃え、Ctrl+C の二度押しと数え直し、処理中の Ctrl+C、Ctrl+D、↑・↓、Alt+Enter の無視）を、端末なしで、イベントの列から状態を作る形で確かめる。
- 標準入力か標準出力が端末でないとき、対話の形を起動せずに終了状態 2 で終え、セッションのディレクトリを作らない（`tests/cli.rs` の形）。対話の形の `--json`・`--approval on-request` が使い方の誤りになる。
- 模擬の提供者で、芯に書き先（`Vec<u8>`）を渡して、依頼から結果までの出力に、応答の文と道具の一行が入り、`Record::Summary` が依頼ごとに追記される。二つ目の依頼が、一つ目の取り消しの影響を受けない。
- `exec --json` の出力に差分が入らない。

## 手で確かめる項目（オーケストレータ）

完了の報告に、手順を書く。macOS の Terminal.app、Ghostty か kitty（Shift+Enter）、tmux の中で、応答の途中に端末の幅を変える、日本語の IME と全角の文字、複数行の貼り付け、Ctrl+C の二度押し、Ctrl+D、別の端末から `kill -TERM` を送って端末が戻ること（`stty -a`）、終えた後のカーソルとプロンプトの位置。起動の時間を `printf '\004' | script -q /dev/null target/release/benitoiteh` を 10 回走らせて測る（中央値と最大値。06-02 の目標は入力を受けられるまで 150 ミリ秒）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、画面の例（文字の写し）、足した英文の全文、実行ファイルの大きさの変化、直接の標準エラー出力の `grep` の結果、`10-interfaces.md` に書く新しいシグネチャ（分けた `prepare`、`Prepared`、`Agent` の見積もりの口、`ToolResult` の欄、`ui` の公開の口）を書く。
