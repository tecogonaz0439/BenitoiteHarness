# P1-04 設定ファイル、引数、データの置き場所、書ける場所の検査

## 目的

利用者の設定ファイルとプロジェクトの設定ファイルの読み込みと検証、`exec` のオプションの解釈、データの置き場所、API キーの参照の解決、書ける場所の検査を作る。P1-10 の `exec` がこれらを組み合わせる。

## 読む設計書

- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「コマンドラインの形」「オプション」「設定ファイル」「プロジェクトの設定ファイル」「データの置き場所」
- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「提供者ごとの機能の表」「認証の情報」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「書ける場所の検査」
- [モード](../../design/01-agent/01-02-modes.md)の「モードの選び方」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「信頼の境界」

## 作るもの

1. `crates/benitoiteh/src/config/`。
   - 利用者の設定ファイル（`$XDG_CONFIG_HOME/benitoiteh/config.toml`、なければ `~/.config/benitoiteh/config.toml`）を `toml` と `serde` で読む。知らない欄があれば誤りとし、欄の名前を示す（`deny_unknown_fields`）。05-01 の例のすべての欄を持つ。ファイルがなければ既定の値で動く（提供者がないので、`exec` は提供者を選べない誤りになる）。
   - 提供者の設定: `kind`（`anthropic-messages`・`openai-chat`・`openai-responses`）、`base_url`（省略時は `anthropic-messages` が `https://api.anthropic.com`、`openai-chat` と `openai-responses` が `https://api.openai.com/v1`）、`model`、`effort`、`api_key`（`{ env = "..." }` か `{ command = [...] }`。キーそのものを書く欄はない）、`capabilities`、`context_window`、`max_output_tokens`。`capabilities` の既定は、アダプタの既定（P1-01 の `Capabilities`）とする。既知の提供者の表を持つかは BH-032 で未決なので、この作業では持たず、`base_url` が既定の本家の口のときだけ本家の機能を有効にする（02-01 の「表にない互換の口は、本家と同じ機能を持つと推し量らない」）。どの機能を有効にしたかを、完了の報告に書く。
   - プロジェクトの設定ファイル（`<プロジェクト>/.benitoiteh/config.toml`）: `model` と `read.exclude` だけを受け付ける。ほかの欄があれば、誤りとして示し、ファイル全体を使わずに続ける（05-01）。読んだかどうかと、誤りの文を返し、画面と `exec` が示せるようにする。読むときは P0-02 の `safe_fs` でプロジェクトの根から開く。
   - API キーの参照の解決: `env` は環境変数を読む。`command` はプログラムを起動して、標準出力の一行目をキーとする（サンドボックスの外、起動のときに一度だけ。標準入力は空、標準エラー出力は捨てずに、失敗したときの誤りの文に、キーを含まない形で添える）。キーを持つ型は `Debug` でキーを伏せる。設定で参照した環境変数の名前の一覧を返す（子プロセスの環境から除くため。P2 で使う）。
2. `crates/benitoiteh/src/cli/args.rs`: `exec` のオプションの解釈（自前）。05-01 の「オプション」の表のうち、P1 で使うもの（`--mode`、`--project`、`--provider`、`--model`、`--effort`、`--max-turns`、`--max-check-fix`、`--max-run-tokens`、`--json`、`--output-last-message`）と、`<依頼>`（`-` は標準入力）。ほかのオプション（`--sandbox`・`--approval`・`--allow`・`--project-parent`・`--resume`・`--continue`・`--no-sandbox`）は解釈して値を持つが、使うのは後の段階であり、指定されたら「まだ使えない」として使い方の誤り（終了状態 2）にする。同じオプションを二度書いたら誤りとする。
3. データの置き場所（`config/paths.rs` など）: セッションのディレクトリの根（`$XDG_STATE_HOME/benitoiteh/sessions/`、なければ `~/.local/state/benitoiteh/sessions/`）、一時ディレクトリの根（`$TMPDIR`、なければ `/tmp`）、利用者の指示のファイル。`HOME` がないときは誤りとする。
4. 書ける場所の検査（`config/writable.rs` など。03-02 の「書ける場所の検査」）: プロジェクトのディレクトリと一時ディレクトリ（と通常モードで足した場所）を実のパスにして、四つの規則（根かホームかそれらを含む、ホームの直下の `.` で始まるディレクトリか macOS の `~/Library` の中、設定とデータのディレクトリと重なる、プロジェクトと一時ディレクトリが重なる）に当たれば誤りとする。誤りの文は、当たった規則と、パスを示す。
5. 優先の順: オプション、プロジェクトの設定（`model` だけ）、利用者の設定、既定、の順に決める（05-01 の表の既定の欄）。モードの既定は `restricted` である。

## 触るファイル

- `crates/benitoiteh/src/lib.rs`（`pub mod config;`）
- `crates/benitoiteh/src/cli/`（引数の解釈を加える。`--version`・`--help`・`__sandbox` の振る舞いは変えない）
- `crates/benitoiteh/Cargo.toml`（`toml` を加える）

## テスト

- 設定: 05-01 の例を読める。知らない欄、キーそのものを書いた形、知らない `kind` が誤りになる。
- プロジェクトの設定: `model` と `read.exclude` が効く。`base_url` などを書いたら、誤りが示され、`model` も使われない。
- キーの参照: `env` と `command`（テストの中で作った小さなシェルのスクリプトを起動してよい。これはサンドボックスの外の処理であり、制限モードの規則の対象ではない）。キーが誤りの文と `Debug` に出ない。
- 引数: 表のオプション、二度書き、`-` の依頼、まだ使えないオプション。
- 書ける場所の検査: 四つの規則のそれぞれ（テストの中で `HOME` と `XDG_*` を一時ディレクトリに向けて確かめる。テストの間で環境変数を変える関数は、プロセス全体に効くので、検査の関数に `HOME` などを引数で渡せる形にして、環境変数を変えずに確かめる）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、`exec` が使う関数と型（設定を読む、オプションを解釈する、キーを得る、書ける場所を検査する）の一覧を書く。
