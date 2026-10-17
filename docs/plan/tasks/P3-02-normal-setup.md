# P3-02 通常モードの起動の準備（サンドボックスの方針と子プロセスの環境）

## 目的

通常モードの道具が子プロセスを動かすために要るものを、起動の段階でそろえる。道具 `bash` は P3-05、承認の方針は P3-04 で作る。この作業は、その二つが使う土台を作る。

- 起動のオプション `--sandbox`・`--approval`・`--no-sandbox` を使えるようにする。
- 通常モードのサンドボックスの方針（`read-only`・`workspace-write`・`danger-full-access`）から、子プロセスの方針を作る。
- サンドボックスを掛けられるかを確かめ、掛けられなければ通常モードを起動しない（`--no-sandbox` を除く）。
- 子プロセスの起動に要るもの（補助のプロセスのパス、Linux の一時の場所と塞ぐ場所の錠のディレクトリ、子プロセスの環境）を、道具に渡せる形にまとめる。
- 書ける場所が `benitoiteh` の実行ファイルを含むときに拒む（設計書 03-02 の「書ける場所の検査」にあり、まだ作っていない規則）。

## 読む設計書

- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「方針の要素」「書ける場所の検査」「通常モードの方針」「サンドボックスを掛けられないとき」「子プロセスの環境」「Linux のファイルシステムの見え方」（`placeholders/`）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の起動のオプションの表、利用者の設定の `[normal]`、「データの置き場所」
- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「処理系の起動」（提供者のキーの変数を除く規則）
- [文脈の管理](../../design/01-agent/01-04-context.md)の「環境の情報」「送る内容の表示」
- 既存のもの: `src/cli/args.rs`（`DeferredOptions`、`ArgsError::Unavailable`）、`src/cli/exec.rs`、`src/config/`（`NormalConfig`、`Sandbox`、`Approval`、`writable.rs` の `WritableRule`、`UserConfig::referenced_env_vars`、`paths.rs`）、`src/sandbox/`（`PolicyConfig`、`SandboxPolicy::validate`、`SandboxCommand`、Linux の `prepare`・`probe_linux`・`check_bubblewrap`）、`src/tools/mod.rs`（`ToolContext`）、`src/context/environment.rs`、`src/agent/mod.rs`（`session_start` の通知の文）、`tests/args.rs`、`tests/cli.rs`、`tests/exec.rs`

## 作るもの

### 1. 起動のオプション

- `--sandbox`・`--approval`・`--no-sandbox` を `DeferredOptions` から `ExecArgs` に移し、`ArgsError::Unavailable` を返さないようにする。
- 通常モードのサンドボックスの方針は、`--sandbox`、利用者の設定の `[normal].sandbox`、既定の `workspace-write` の順で決める。
- `exec` の承認の方針は `never` に固定する。`--approval` に `never` のほかを指定したら、使い方の誤り（終了状態 2）とする。利用者の設定の `[normal].approval` は `exec` では使わない（対話の画面で使う。P4）。
- 制限モードで `--sandbox`・`--approval`・`--no-sandbox` を指定したら、使い方の誤り（終了状態 2）とする。理由: 制限モードの方針はエフェクトから決まるので、指定が効くと利用者が思い込むことを防ぐ。
- `--no-sandbox` は、サンドボックスを掛けられないときにだけ効く。掛けられるときは何もしない。

### 2. 通常モードの方針を作る関数

`sandbox` に置く（名前は任せる）。入力は、`Sandbox`、プロジェクトのディレクトリ、セッションの一時ディレクトリ、設定の `writable_roots` と `network`、読ませない場所の候補とする。どのパスも、書ける場所の検査（`config/writable.rs` の `WritableLocations`）を通した実のパスを使う。macOS の一時ディレクトリは `/private/var` の下にあるので、`Session::create` の後に正規の形にする。

- 読ませない場所の候補は、`~/.ssh`・`~/.gnupg`・`~/.aws`、`benitoiteh` の設定とデータのディレクトリとする。あるものだけを正規の形にして入れ、ないものは入れない（`SandboxPolicy::validate` は、ないパスを拒むため）。利用者の設定で読ませない場所を足す欄は、今の設定にないので、関数が引数で受け取る形だけを作り、設定の欄は作らない。
- `read-only`: 書ける場所は一時ディレクトリだけ。IP の通信なし。
- `workspace-write`: 書ける場所はプロジェクトのディレクトリ、一時ディレクトリ、設定の `writable_roots`。IP の通信は設定の `network`。
- どちらも、読める範囲は全体（読ませない場所を除く）、起動するプログラムは制限しない、守る名前は `.git` と `.benitoiteh`、書ける場所のファイルの実行は拒否しない（`deny_exec_in_writable` を偽）。理由: 通常モードは、ビルドしたプログラムをプロジェクトの中で動かす。
- `danger-full-access`: 方針を作らない。

### 3. サンドボックスを掛けられるかの確かめ

- macOS: `/usr/bin/sandbox-exec` があるかだけを確かめる。理由: 実際に起動して確かめると、Codex の中のように `sandbox-exec` を重ねられない環境で、起動の確かめが失敗する。
- Linux: `probe_linux`（2 の一時の場所を使う）の四つの欄がすべて `Available` であることを確かめる。
- 掛けられないときは、`--no-sandbox` がなければ、通常モードを起動せず、理由を示して終了状態 2 で終わる。`--no-sandbox` があれば、方針を作らずに動かす。そのことを、起動のときに `ExecOutput::diagnostic` で一行示し、`session_start` の通知の文にも加える（`agent/mod.rs`）。
- `danger-full-access` のときは確かめない。

### 4. 子プロセスの起動に要るもの

`tools` が使える形の値（例: `ShellContext`）にまとめ、`ToolContext` に、今の `ToolContext::new` の形を変えずに足せる関数（例: `with_shell(self, ShellContext) -> Self`）で持たせる。持たせるものは次のとおりとする。

- 子プロセスの方針（`Option<SandboxPolicy>`。`None` はサンドボックスを掛けない）と方針の名前（`read-only`・`workspace-write`。掛けないときはなし）。
- 補助のプロセスのパス: `exec` では `canonicalize(current_exe())`。テストでは `env!("CARGO_BIN_EXE_benitoiteh")` を使う（テストの `current_exe` はテストの実行ファイルである）。
- Linux の一時の場所: セッションのディレクトリの下の `sandbox/`（権限 700）。`probe_linux` にも同じものを使う。
- Linux の塞ぐ場所の錠のディレクトリ: データのディレクトリの `placeholders/`（権限 700。`exec` の起動のときに作る）。
- 子プロセスの環境（後述の 5）。
- 承認の方針（P3-04 が使う。この作業では値を持たせるだけ）。

### 5. 子プロセスの環境

- 提供者のキーの変数の固定の一覧（04-01 の `OPENAI_API_KEY`・`ANTHROPIC_API_KEY`・`GEMINI_API_KEY`・`DEEPSEEK_API_KEY`）を、`config` の定数（例: `PROVIDER_KEY_ENV_VARS`）に置く。
- 子プロセスの環境を作る関数を一つにし、どの起動の形（サンドボックスあり、`danger-full-access`、`--no-sandbox`、P3-04 の `escalate`）でも使う。`benitoiteh` の環境から、固定の一覧と `UserConfig::referenced_env_vars()` の変数を除き、03-02 の「子プロセスの環境」の値（`TMPDIR`、`BENITOITEH_SANDBOX`、`BENITOITEH_SANDBOX_NETWORK_DISABLED=1`、`NO_COLOR=1`・`TERM=dumb`・`PAGER=cat`・`GIT_PAGER=cat`）を加える。`BENITOITEH_SANDBOX` と `…_NETWORK_DISABLED` は、サンドボックスを掛けるときだけ設定する。

### 6. 書ける場所の検査に加える規則

- `config/writable.rs` に、書ける場所（プロジェクト、一時ディレクトリ、`writable_roots`）が `benitoiteh` の実行ファイルの実のパスを含むときに拒む規則（例: `WritableRule::Executable`）を加える（03-02 の「書ける場所の検査」）。理由: `workspace-write` の子プロセスが、`benitoiteh` 自身を書き換えられないようにする。Linux の `prepare` には同じ確かめ（`WRITABLE_HELPER`）があるが、macOS とサンドボックスを掛けない起動にはない。

### 7. 環境の情報

- `Environment::capture` に渡す書ける場所とネットワークの可否を、2 の方針から渡す。`danger-full-access` と `--no-sandbox` のときは、書ける場所を空の一覧、ネットワークを可とし、方針の名前（`danger-full-access`、`none`）を示す欄を加えるかは任せる（加えたら報告する）。

## 触るファイル

- `crates/benitoiteh/src/cli/`（`args.rs`、`exec.rs`、`text.rs`）
- `crates/benitoiteh/src/config/`（`mod.rs`、`writable.rs`）
- `crates/benitoiteh/src/sandbox/`（方針を作る関数）
- `crates/benitoiteh/src/tools/mod.rs`（`ToolContext` に足す関数）
- `crates/benitoiteh/src/context/environment.rs`、`src/agent/mod.rs`（通知の文）
- `crates/benitoiteh/tests/args.rs`、`tests/cli.rs`（今の `Unavailable` の期待を直す）、`tests/exec.rs`、`tests/config.rs`

## テスト

- オプション: 三つの優先の順、`exec` での `--approval untrusted` の誤り、制限モードでの三つの指定の誤り。
- 方針: 三つの方針の書ける場所、ネットワーク、守る名前、`deny_exec_in_writable`。読ませない場所の候補のうち、ないものが入らず、起動が止まらない。
- 書ける場所が `benitoiteh` の実行ファイルを含むときの拒否（テストでは実行ファイルのパスを引数で与える形にして確かめる）。
- 子プロセスの環境: 固定の一覧と設定で参照した変数がなく、03-02 の値がある。サンドボックスを掛けないときに `BENITOITEH_SANDBOX` がない。
- 掛けられないとき: テストのために、確かめの結果を差し替えられる形（関数の引数など）にし、`--no-sandbox` のあるときとないときの起動を確かめる。
- 既存の通常モードの `exec` のテスト（`tests/exec.rs` の通常モードのもの）は、Apple の `container` の aarch64 の Linux に bubblewrap がないので、`--sandbox danger-full-access` を付ける形に直す。
- `tests/expected/` の道具の定義とシステムプロンプトは変わらないこと。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、作った関数と型のシグネチャ（方針を作る関数、`ShellContext`、環境を作る関数）を書く（オーケストレータが 10-interfaces を更新する）。
