# P3-05 通常モードの道具 `bash`

## 目的

通常モードの道具 `bash` を作る（設計書 01-03 の「bash（通常モード）」）。P2-01 の実行の部分、P3-02 の起動の準備（方針、`ShellContext`、子プロセスの環境）、P3-04 の承認の方針を使う。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「bash（通常モード）」「出力の切り詰め」「モードごとの道具の一覧」「道具の定義の共通の規則」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「拒否の検出」「子プロセスの環境」「子プロセスの後始末」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「通常モードの承認の方針」
- 既存のもの: `src/sandbox/run.rs`、`src/sandbox/`（macOS の `to_command`、Linux の `prepare`）、P3-02 の `ShellContext` と子プロセスの環境を作る関数、P3-04 の承認の判定と確認の種類、`src/tools/`（`ToolSet`、`definitions.rs`、`truncate.rs`、`ToolOutput`）、`src/agent/execution.rs`（道具の実行と確認）、`src/session/`（`Session::spill_writer`、`SpillWriter`、`SpillInfo`、`Record::ToolResult`）、`prompts/tools/`、`tests/expected/`

## 作るもの

### 1. 道具の定義

- `src/tools/bash.rs`（名前は任せる）に作り、通常モードの `ToolSet` にだけ加える。順は 01-03 の「モードごとの道具の一覧」のとおりとする。
- 引数は 01-03 の JSON のとおり（`command`、`timeout`、`escalate`、`justification`）。`timeout` は秒で、`1..=600` の整数、既定 120。範囲外は道具の誤りとする。
- 道具の説明の英文を `prompts/tools/bash.md` に置く（一行の説明と 0〜3 行の規則。01-03 の英文があればそれを使い、ほかは短く書く。オーケストレータが取り込む前に文面を見直す）。

### 2. 起動

- シェルは `/bin/bash`、なければ `/bin/sh` とする（固定のパス。`PATH` から探さない）。サンドボックスを掛けるときは、実のパス（`canonicalize`）を目的のプログラムとする。実のパスのファイルの名前が `bash`・`sh`・`dash` のどれでもないとき（Alpine の `/bin/sh` が `busybox` を指すときなど）は、引数の先頭に `sh` を置いて `<実のパス> sh -c <command>` とする。
- 作業ディレクトリはプロジェクトのディレクトリ、標準入力は空、環境は P3-02 の関数で作ったものとする。サンドボックスを掛けないとき（`danger-full-access`、`--no-sandbox`、承認を受けた `escalate`）も、同じ環境を使い、`env_clear` してから渡す。
- サンドボックスを掛けるときは、macOS は `SandboxCommand::to_command`、Linux は `prepare(helper, scratch, placeholders)` で起動の形を作る。
- 実行は `sandbox::run::run` で行う。止める合図は `StopSignal::Terminate`、時間の上限は `timeout`、取り消しの印はエージェントのループのものを渡す（`execute_validated` に `cancelled: &AtomicBool` を足す。呼ぶ側は `agent/execution.rs` だけ）。`run` は、道具を実行するスレッドで同期に呼ぶ（Linux の `--die-with-parent` は起動したスレッドに結び付くため、短く終わるスレッドの中で呼ばない）。

### 3. 出力

- 標準出力と標準エラー出力は、同じ受け取り先（一つの `Arc<Mutex<…>>`）に、届いた順にまとめる。受け取り先の `write` は誤りを返さない（`run` は受け取り先の誤りを実行の誤りにするため）。結果は錠の中で取り出す（`PossiblyRemaining` のとき、出力を読むスレッドが `run` の後も生きているため）。
- 受け取り先は、末尾の 2,000 行か 50 KiB だけをメモリに保ち、全体の行の数を数え、すべてを `Session::spill_writer(<呼び出しの ID>)` の書き手に書く（16 MiB の上限と末尾の印は、今の書き手が持つ）。
- 切り詰めたときは、結果の末尾に `[Showing last lines A-B of N. Full output: <path>]`（01-03 の英文）を付ける。`<path>` は `spill/` のファイルの絶対の実のパスとする（`read` がそのまま読めるように）。16 MiB を超えて書けなかったときは、`Full output` の代わりに `Partial output (first X of Y bytes)` とする。切り詰めなかったときは、`spill/` のファイルを消す（書き手に消す関数を足す）。
- 出力は `from_utf8_lossy` で文字列にする。
- `Record::ToolResult` の `spill` の欄を、`spill/` のファイルを残したときに埋める（今は `execution.rs` が `None` を入れている）。

### 4. 結果の末尾

- `[exit code: N]` を付ける。合図で終わったときは `N` を 128 に合図の番号を足した値とする（Linux の補助のプロセスも同じ値で終わる）。0 でなければ誤りの印を付ける。
- 時間の上限と取り消しで止めたときは、そのことを一行で加える（英語。`text` に置く）。
- `Leftovers::PossiblyRemaining` のときは、子孫のプロセスが残っている可能性を一行で加える。画面への表示は P4 で作る。
- 拒否の検出（03-02 の「拒否の検出」）に当たれば、01-03 の英文の案内（`[sandbox: the command may have been blocked by the sandbox. ...]`）を字句を変えずに加える。サンドボックスを掛けていないときは加えない。Linux では、補助のプロセスが合図で終わった子の終了状態を 128 足す番号で返すので、`SIGSYS` は終了状態 159 として現れる。Linux でサンドボックスを掛けたときは、終了状態 159 も `SIGSYS` で終わったものとして扱う。

### 5. 確認

- P3-04 の承認の判定を使う。`escalate` でない呼び出しは種類 `Command`、`escalate` が真の呼び出しは種類 `Escalate` として尋ねる。`untrusted` で `escalate` が真のときは、`Escalate` の確認を一度だけ行う。
- `escalate` を承認されたら、その一回だけサンドボックスを掛けずに動かす。`danger-full-access` と `--no-sandbox` のときは、`escalate` は何も変えない（もともと掛けていない）。

### 6. 期待の出力

- `tests/expected/system-normal.txt`、`tools-normal.json` を作り直し、差分が `bash` の分だけであることを確かめる。`system-restricted.txt`、`tools-restricted.json` は変わらないこと。

## 触るファイル

- `crates/benitoiteh/src/tools/`（`bash.rs`（新）、`mod.rs`、`definitions.rs`、`text.rs`、要れば `truncate.rs`）
- `crates/benitoiteh/prompts/tools/bash.md`（新）
- `crates/benitoiteh/src/agent/execution.rs`（`cancelled` を渡す、`spill` の欄）
- `crates/benitoiteh/src/session/`（書き手に消す関数）
- `crates/benitoiteh/src/context/`（道具の一覧が変わるとき）
- `crates/benitoiteh/tests/expected/`、`tests/` の道具と `exec` のテスト、`tests/sandbox_macos.rs`、`tests/sandbox_linux.rs`
- `scripts/remote-linux-run.sh`（x86_64 の試験のイメージに `bash` を入れる。`apk add bubblewrap bash` にし、イメージの名前の版を改める）、`docs/plan/00-common.md` の「検査」のイメージの名前（この一か所だけ変えてよい）

## テスト

**サンドボックスを要さないもの**（`--sandbox danger-full-access` で確かめる。`tests/` の道具と `exec` のテスト）

- 出力の末尾の保ち方: 2,000 行を超える出力と 50 KiB を超える出力で、本当の末尾が返り、印の行の番号と全体の行の数が正しい。`spill/` のファイルに全体がある。16 MiB を超えたときに `Partial output` の印になる。切り詰めなかったときに `spill/` のファイルが残らない。
- `[exit code: N]` が 0 のときも付く。時間の上限で止まったことが結果に出る。`timeout` の範囲外（0、601）が誤りになる。
- 子プロセスの環境に提供者のキーの変数がない（安全の性質のテスト。07-01）。`escalate` で動かしたときもない。
- 確認: `never` では `escalate` を確認せずに拒否し、動かさない。台本の `Approver` で、`on-request` の `escalate` を承認すれば一回だけ動き、次の同じ要求で再び尋ねる。`untrusted` では `escalate` でない呼び出しも尋ね、`escalate` の呼び出しは一度だけ尋ねる。
- 道具の定義の期待の出力が一致する。

**サンドボックスの中**（`tests/sandbox_macos.rs`、`tests/sandbox_linux.rs`。Codex の中では走らない。オーケストレータが手元の macOS と x86_64 の Linux の機械で走らせる）

- `workspace-write`: プロジェクトに書け、プロジェクトの外と `.git` に書けず、IP の通信ができない（安全の性質のテスト。07-01）。プロジェクトの中に置いた実行ファイル（`sandbox-probe` の写し）を起動できる。`> /dev/null` へのリダイレクトができる。
- `read-only`: プロジェクトに書けず、一時ディレクトリに書ける。
- 書けない場所に書こうとしたコマンドの結果に、拒否の案内が付く。
- 名前の解決ができることは確かめない（macOS で IP の通信を許すときの名前の解決は BH-044 による）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、道具の説明の英文の全文と、作った関数のシグネチャを書く。
