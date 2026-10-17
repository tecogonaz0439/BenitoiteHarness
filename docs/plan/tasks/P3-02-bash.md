# P3-02 通常モードのサンドボックスの方針と `bash`

## 目的

通常モードのサンドボックスの方針（`read-only`・`workspace-write`・`danger-full-access`）から子プロセスの方針を作り、通常モードの道具 `bash` を、P2-01 の実行の部分を通して動かす（設計書 01-03 の「bash（通常モード）」、03-02 の「通常モードの方針」）。承認の方針の細部（`untrusted` の毎回の確認、このセッションでの許可、書ける場所の外への書き込み）は P3-03 で作る。この作業では、`escalate` の要求と `untrusted` の確認を、既存の `Approver` に尋ねる形までを作る。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「bash（通常モード）」「出力の切り詰め」「モードごとの道具の一覧」「道具の定義の共通の規則」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「方針の要素」「通常モードの方針」「拒否の検出」「子プロセスの環境」「子プロセスの後始末」「サンドボックスを掛けられないとき」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」「通常モードの承認の方針」「非対話での扱い」
- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「処理系の起動」の環境変数の規則（提供者のキーの変数を除く）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の起動のオプション（`--sandbox`・`--approval`・`--no-sandbox`）と、利用者の設定の `[normal]`
- 既存のもの: `src/sandbox/`（P2-01 の実行の部分、`SandboxCommand`、`PolicyConfig`、Linux の `prepare`）、`src/tools/`（`ToolSet`、`definitions.rs`、`truncate.rs`）、`src/approval/`、`src/config/`（`NormalConfig`、`Sandbox`、`Approval`）、`src/context/`（道具の定義と期待の出力）、`src/cli/exec.rs`、`prompts/tools/`

## 作るもの

### 1. 通常モードの方針

- `sandbox` に、通常モードの方針を作る関数を置く。入力は、`Sandbox`、プロジェクトのディレクトリ、セッションの一時ディレクトリ、利用者の設定の `writable_roots` と `network`、読ませない場所（`~/.ssh`・`~/.gnupg`・`~/.aws`、`benitoiteh` の設定とデータのディレクトリ、利用者の設定で足したもの）とする。
  - `read-only`: 読める範囲は全体から読ませない場所を除いたもの、書ける場所は一時ディレクトリだけ、IP の通信なし、起動するプログラムは制限しない。
  - `workspace-write`: 書ける場所はプロジェクトのディレクトリ、一時ディレクトリ、設定の `writable_roots`。IP の通信は設定の `network`。ほかは `read-only` と同じ。
  - `danger-full-access`: 方針を作らない（サンドボックスを掛けない）。
  - 守る名前は `.git` と `.benitoiteh`。書ける場所のファイルの実行は拒否しない（`deny_exec_in_writable` を偽にする）。理由: 通常モードは、ビルドしたプログラムをプロジェクトの中で動かす。
  - 書ける場所の検査（P1-04 の `config/writable.rs`）を、足した `writable_roots` にも掛ける。
- OS のサンドボックスを掛けられないとき（`sandbox-exec` がない、Linux で `probe_linux` のどれかが使えない）は、`--no-sandbox` がなければ通常モードを起動しない。`--no-sandbox` があれば、方針を作らずに動かし、そのことを `session_start` と画面に示す（03-02 の「サンドボックスを掛けられないとき」）。

### 2. `bash`

`src/tools/bash.rs`（名前は任せる）に作り、通常モードの `ToolSet` に加える。順は設計書 01-03 の「モードごとの道具の一覧」のとおりとする。

- 引数は 01-03 の JSON のとおり（`command`、`timeout`、`escalate`、`justification`）。`timeout` は秒で、既定 120、上限 600。範囲外は道具の誤りとして返す。
- `bash -c <command>` を起動する。`bash` は `/bin/bash`、なければ `/bin/sh`（固定のパス。`PATH` から探さない）。作業ディレクトリはプロジェクトのディレクトリ、標準入力は空。
- 環境は、`benitoiteh` の環境から提供者のキーの変数（04-01 の一覧と、設定でキーとして参照した変数）を除き、03-02 の「子プロセスの環境」の値（`TMPDIR`、`BENITOITEH_SANDBOX`、`BENITOITEH_SANDBOX_NETWORK_DISABLED`、`NO_COLOR=1`・`TERM=dumb`・`PAGER=cat`・`GIT_PAGER=cat`）を設定する。`BENITOITEH_SANDBOX` の値は方針の名前（`read-only`・`workspace-write`）とし、サンドボックスを掛けないときは設定しない。
- 実行は P2-01 の実行の部分で行い、止める合図は `SIGTERM` とする。標準出力と標準エラー出力は、同じ受け取り先に、届いた順にまとめる。
- 受け取り先は、末尾の 2,000 行か 50 KiB だけをメモリに保ち、すべてをセッションの `spill/` のファイルに 16 MiB まで書く（01-03 の「出力の切り詰め」）。切り詰めたときは、結果の末尾に `[Showing last lines A-B of N. Full output: <path>]` を付ける。16 MiB を超えて書けなかったときは `Full output` の代わりに `Partial output (first X of Y bytes)` とする。切り詰めなかったときは `spill/` のファイルを残さない。
- 結果の末尾に `[exit code: N]` を付ける。0 でなければ誤りの印を付ける。時間の上限と取り消しで止めたときは、そのことを一行で加える（英語。`text` に置く）。`Leftovers::PossiblyRemaining` のときは、子孫のプロセスが残っている可能性を一行で加え、画面にも示す（03-02 の「子プロセスの後始末」）。
- 拒否の検出（03-02 の「拒否の検出」）に当たれば、01-03 の英文の案内（`[sandbox: the command may have been blocked by the sandbox. ...]`）を字句を変えずに加える。サンドボックスを掛けていないときは加えない。
- `escalate` が真のときは、サンドボックスを掛けずに（`danger-full-access` で）動かす要求として、`Approver` に新しい種類の確認（`ApprovalKind` に加える。例: `Escalate`）で尋ねる。確認の要求には、コマンドの全文、作業ディレクトリ、`justification` を、LLM の文と `benitoiteh` の事実を分けて入れる（03-03 の「確認の共通の規則」）。拒否されたら、`approval::denial_message()` を返して実行しない。承認はその一回だけに効く。
- 承認の方針（`--approval`・設定の `approval`）は、この作業では次の範囲で扱う。`never` は `escalate` を確認せずに拒否する。`untrusted` は、`escalate` でない呼び出しも毎回 `Approver` に尋ねる（新しい種類。例: `Command`）。`on-request` は `escalate` だけを尋ねる。「このセッションでは同じコマンドを許可する」は P3-03 で作る。
- 道具の説明の英文を `prompts/tools/bash.md` に置く（一行の説明と 0〜3 行の規則。01-03 の英文があればそれを使い、ほかは短く書く。オーケストレータが取り込む前に文面を見直す）。

### 3. 起動の入口

- `cli/exec.rs` で、通常モードのときに 1 の方針を作り、`bash` に渡す。非対話の `exec` の承認の方針は、03-03 の「非対話での扱い」のとおり `never` とする。`--approval` に `never` のほかを指定したら、使い方の誤り（終了状態 2）とする（05-01）。`--sandbox danger-full-access` のときは方針を作らない。
- 環境の情報（`Environment::capture`）の書ける場所とネットワークの可否を、作った方針から渡す。
- システムプロンプトと道具の定義の期待の出力（`tests/expected/system-normal.txt`、`tools-normal.json`）を作り直し、差分が `bash` の分だけであることを確かめる。

## 触るファイル

- `crates/benitoiteh/src/sandbox/`（通常モードの方針を作る関数）
- `crates/benitoiteh/src/tools/`（`bash.rs`（新）、`mod.rs`、`definitions.rs`、`text.rs`、要れば `truncate.rs`）
- `crates/benitoiteh/prompts/tools/bash.md`（新）
- `crates/benitoiteh/src/approval/`（確認の種類）
- `crates/benitoiteh/src/cli/exec.rs`
- `crates/benitoiteh/src/context/`（道具の一覧が変わるとき）
- `crates/benitoiteh/tests/expected/`、`tests/` の道具と `exec` のテスト、`tests/sandbox_macos.rs`、`tests/sandbox_linux.rs`

## テスト

**サンドボックスを要さないもの**（`danger-full-access` か、模擬の起動で確かめる）

- 出力の末尾の保ち方: 2,000 行を超える出力と 50 KiB を超える出力で、本当の末尾が返り、印の行の番号と全体の行の数が正しい。`spill/` のファイルに全体がある。16 MiB を超えたときに `Partial output` の印になる。
- `[exit code: N]` が 0 のときも付く。時間の上限で止まったことが結果に出る。
- 子プロセスの環境に提供者のキーの変数がない（安全の性質のテスト。07-01）。`NO_COLOR` などが設定されている。
- `escalate` の要求: `never` では確認せずに拒否し、実行しない。`on-request` で承認すれば一回だけ実行し、同じ呼び出しを繰り返すと再び尋ねる。拒否すれば実行しない。確認の要求に、コマンドの全文と `justification` が分けて入る。
- `untrusted` では、`escalate` でない呼び出しも毎回尋ねる。
- `timeout` の範囲外が誤りになる。
- 道具の定義の期待の出力が一致する。

**サンドボックスの中**（`tests/sandbox_macos.rs`、`tests/sandbox_linux.rs`）

- `workspace-write`: プロジェクトに書け、プロジェクトの外と `.git` に書けず、IP の通信ができない（安全の性質のテスト。07-01）。プロジェクトの中でビルドした実行ファイル（テストでは `sandbox-probe` の写しをプロジェクトに置いたもの）を起動できる。
- `read-only`: プロジェクトに書けず、一時ディレクトリに書ける。
- 書けない場所に書こうとしたコマンドの結果に、拒否の案内が付く。
- 読ませない場所（テストでは方針に足した場所）を読めない。

## 確かめること

- 期待の出力の差分が `bash` の分だけであること。
- 道具の説明の英文の全文（オーケストレータが見直す）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、作った関数と型のシグネチャ、確認の種類の名前を書く。
