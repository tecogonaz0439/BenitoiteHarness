# P4-05 `doctor`（処理系の確かめを除く）

## 目的

`benitoiteh doctor` を作る（設計書 05-01 の「doctor」）。Benitoite の処理系との連携（P2 の処理系の作業）は `v0.0.7` を待っているので、この作業では処理系の項目を「確かめていない」として示し、サンドボックス、設定、提供者のキーの参照、データの置き場所を確かめる。処理系の項目は P2 の処理系の作業で加える。

## 読む設計書

- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「コマンドラインの形」「終了状態」「doctor」「設定ファイル」「データの置き場所」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「サンドボックスの掛け方」（起動のときの確かめ、bubblewrap の版と setuid、特権の除去）「通常モードの方針」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「Linux での保証の範囲」「OS のサンドボックスを使えない環境」
- 既存のもの: `src/sandbox/`（`check_availability`、Linux の `probe_linux`・`check_bubblewrap`・`LinuxAvailability`・`CapabilityStatus`、macOS の `sandbox-exec` の確かめ）、`src/config/`（`Config::load`、`ProjectConfigLoad`、`ApiKeyRef::resolve`、`DataPaths`）、`src/cli/`（コマンドの振り分け、`text.rs` の「not available yet」の文）

## 作るもの

### 1. コマンド

- `benitoiteh doctor` を使えるようにする（`cli/text.rs` の「not available yet」の文から `doctor` を除く）。オプションは `--project <dir>`（プロジェクトの設定を読む場所。既定は作業ディレクトリ）と `--json`（項目ごとに一行一件の JSON）とする。
- 項目ごとに、名前、状態（`ok`「確かめた」・`unavailable`「使えない」・`unchecked`「確かめていない」・`warning`「注意」）、一行の説明を示す。`unavailable` が一つでもあれば終了状態 1、ほかは 0 とする。`unchecked` と `warning` は終了状態を変えない。
- LLM の提供者には要求を送らない。ネットワークにも出ない。

### 2. 項目

- **処理系**: `unchecked` とし、「P2 の処理系の連携で確かめる」旨を示す（英文は `text` に置く）。
- **macOS のサンドボックス**: `/usr/bin/sandbox-exec` があり、`benitoiteh` 自身を補助の形で `--version` を許す方針で起動でき、`/bin/sh` を拒む方針で `/bin/sh` を起動できないこと。両方を確かめたときだけ、制限モードを起動できると示す。
- **Linux のサンドボックス**: bubblewrap のパスと版（0.8.0 以上か、setuid でないか）、特権を持たない利用者の名前空間を作れるか、Landlock の ABI の版（3 以上か）、seccomp を使えるか、中のプロセスの特権が空か（P2-04）を、`probe_linux` の結果から項目ごとに示す。加えて、`/bin/sh` を拒む方針で起動できないことを確かめる。すべてを確かめたときだけ、制限モードを起動できると示す。守る場所の範囲が macOS より狭いこと（03-01 の「Linux での保証の範囲」）を `warning` で示す。
- **通常モードのサンドボックス**: 設定の `sandbox`（既定 `workspace-write`）の方針を、今のプロジェクトのディレクトリで作れるか（`normal_policy` と `check_availability`）。作れなければ `unavailable` とし、`--no-sandbox` の説明を添える。
- **設定ファイル**: 利用者の設定のファイルを読めるか（ないときは `ok` で既定を使うと示す）。プロジェクトの設定のファイルを読めるか、無視した欄があるか（`warning`）。
- **提供者**: 設定の各提供者について、アダプタの種類、ベースの URL、モデル、`context_window` の設定の有無（なければ `warning`。番の前の圧縮が働かないため。01-04 の「上限が分からないとき」）、キーの参照が値を得られるか（値も長さも示さない）。既定の提供者を印で示す。
- **データの置き場所**: セッションのディレクトリ、`placeholders/`（Linux）、`history/` の場所と権限（0700 か）。すべてのセッションの合計の大きさと、10 GiB の上限に対する割合（P4-02 の数え方を使う。P4-02 が先に入っていなければ、この項目は大きさだけを示す）。
- **書ける場所**: 設定の `writable_roots` が書ける場所の検査（`validate_writable_with_executable`）を通るか。

### 3. 表示

- 文字の表示は、項目ごとに `[ok] <名前>: <説明>` の形の一行とし、最後に「制限モード: 起動できる／できない」「通常モード: サンドボックスあり／なし」の二行を置く。英文は `cli/text.rs`（か新しいモジュールの `text`）に置く。
- 項目の説明に、設定のファイルの中身や秘密を出さない。パスは示してよい。

## 触るファイル

- `crates/benitoiteh/src/cli/`（`doctor` のための新しいモジュール、振り分け、`text.rs`）
- `crates/benitoiteh/src/sandbox/`（確かめの関数が足りなければ。例: `/bin/sh` を拒む確かめ）
- `crates/benitoiteh/src/config/`（読めるかを確かめる関数が足りなければ）
- `crates/benitoiteh/tests/`（新しい `tests/doctor.rs`。サンドボックスを要する確かめは `tests/sandbox_macos.rs`・`tests/sandbox_linux.rs` に置く）

## テスト

- 設定のファイルがないとき、壊れているとき、キーの参照が値を得られないとき（環境の変数がない）の表示と終了状態。キーの値が出力に出ない（安全の性質のテスト。07-01）。
- `context_window` のない提供者に `warning` が付く。
- `--json` の出力が一行一件の JSON で、項目の名前と状態を持つ。
- 処理系の項目が `unchecked` で、終了状態を変えない。
- サンドボックスの中（`tests/sandbox_*`。オーケストレータが手元で走らせる）: macOS と x86_64 の Linux で、サンドボックスの項目が `ok` になり、制限モードを起動できると示す。Linux で守る場所の範囲の `warning` が出る。
- Codex の中（サンドボックスを掛けられない）では、サンドボックスの項目が `unavailable` になり、終了状態 1 になることを確かめてよい（環境に依るテストは、条件を確かめて飛ばす形にする）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、文字の出力の例（macOS と Linux で想定されるもの）と JSON の例、足した英文の全文を書く。
