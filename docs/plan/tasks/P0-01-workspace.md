# P0-01 ワークスペース、lint、検査の仕組み、空の実行ファイル

## 目的

試作のワークスペースと、すべての作業が使う検査の仕組みを作る。P0 の完了条件のうち「空の実行ファイルが三つの対象の環境でビルドでき、検査を通る」を満たす。

## 読む設計書

- [クレートの構成](../../design/06-impl/06-01-crate-structure.md)（全体）
- [テスト](../../design/07-quality/07-01-testing.md)の「完了の前に通す検査」
- [性能](../../design/06-impl/06-02-performance.md)の「測り方」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「コマンドラインの形」「終了状態」
- 処理系のリポジトリ（既定は作業ツリーの隣の `../Benitoite`。違うときはオーケストレータが示す）にある、Benitoite の `v0.0.6` の `Cargo.toml`（`[workspace.lints]`）と `deny.toml`。`git -C <処理系のリポジトリ> show v0.0.6:Cargo.toml` のように、タグの時点のものを読む。

## 作るもの

1. ワークスペースの `Cargo.toml`。
   - `members = ["crates/benitoiteh"]`、`resolver = "3"`。`[workspace.package]` に `edition = "2024"`、`rust-version = "1.98.1"`、`license = "MIT OR Apache-2.0"`、`version = "0.0.1"`。
   - `[workspace.lints]` は Benitoite の `v0.0.6` と同じ項目にし、`unsafe_code` だけを `"forbid"` にする。
   - `[profile.release]` は 06-01 の「ビルドの設定」のとおりとする。
2. `.cargo/config.toml`。
   - `[build] warnings = "deny"`（Benitoite と同じ）。
   - `x86_64-unknown-linux-musl` と `aarch64-unknown-linux-musl` のリンカを `rust-lld` にする。2026-10-17 に、手元の macOS で `rustc --target aarch64-unknown-linux-musl -C linker=rust-lld` で静的リンクの実行ファイルを作れることを、オーケストレータが確かめた。二つの対象は rustup に導入済みである。
   - 二つの対象の `runner` を `scripts/container-run.sh` にする。
3. `deny.toml`。Benitoite の `v0.0.6` と同じ許可の一覧と対象の一覧にする。
4. `crates/benitoiteh/Cargo.toml`（ライブラリと実行ファイル `benitoiteh`、`[lints] workspace = true`）。この作業では依存のクレートを加えない。
5. `src/main.rs`、`src/lib.rs`、`src/cli/mod.rs`、`src/cli/text.rs`。
   - `benitoiteh --version` は `benitoiteh <版>` の一行を標準出力に書き、0 で終わる。版は `CARGO_PKG_VERSION` とする。
   - `benitoiteh --help` は使い方の短い文を標準出力に書き、0 で終わる。中身は 05-01 の「コマンドラインの形」の一覧とし、まだ作っていないサブコマンドも載せてよい。
   - それ以外の引数は、使い方の誤りとして標準エラー出力に一行を書き、2 で終わる。
   - `main` は `std::process::ExitCode` を返す。引数の解釈は自前で行う（05-01）。
6. `scripts/check.sh`（bash）。次の段を順に走らせ、失敗した段で止まり、段の名前を示す。
   1. `fmt`: `cargo fmt --all --check`
   2. `clippy`: `cargo clippy --workspace --all-targets`（ホスト）と、二つの Linux の対象について `cargo clippy --workspace --all-targets --target <対象>`
   3. `test`: `cargo test --workspace`
   4. `linux`: `cargo test --workspace --target aarch64-unknown-linux-musl` と `--target x86_64-unknown-linux-musl`（`runner` で `container` の中で走る）
   5. `deny`: `cargo deny check licenses`
   6. `size`: `cargo build --release -p benitoiteh` の後、実行ファイルのバイト数を一行で示す。Linux の二つの対象のリリースのビルドも作り、同じく示す。
   - `--inside-codex` を付けたときは、`test` の段で `tests/sandbox_*.rs` の統合テストを除き、`linux` の段を飛ばす。除いたテストと飛ばした段を最後に示す。`tests/sandbox_*.rs` を除く方法（`--test` を並べる、`cargo test` の引数で名前を除く、など）は任せる。統合テストのファイルがまだないときも動くようにする。
   - `--inside-codex` のときに `cargo deny` が勧告のデータベースなどをネットワークから取ろうとして失敗するなら、`licenses` の検査だけを手元の情報で行う指定を使う。
7. `scripts/container-run.sh`（bash）。`cargo` の `runner` として、`<実行ファイル> <引数...>` を受け取り、Apple の `container` で Linux のコンテナを起動して走らせる。
   - 実行ファイルの ELF の機械の種類から、`aarch64` はそのまま、`x86_64` は `--arch amd64` を付けて起動する（2026-10-17 に、`container run --arch amd64 alpine:3.22` で x86_64 の静的リンクの実行ファイルが動くことを、オーケストレータが確かめた）。
   - リポジトリの根と、実行ファイルのあるディレクトリを、手元と同じパスでコンテナに見せ（`-v <パス>:<パス>`）、作業ディレクトリを手元と同じにする。
   - イメージは `alpine:3.22` とし、スクリプトの中の一か所で定める。
   - 終了状態は、コンテナの中のプログラムの終了状態をそのまま返す。
8. `crates/benitoiteh/tests/cli.rs`: `--version`・`--help`・知らない引数の終了状態と出力を、実行ファイルを起動して確かめる統合テスト（`env!("CARGO_BIN_EXE_benitoiteh")`）。

## 触るファイル

新しく作るものだけである。既存の `rust-toolchain.toml` は変えない。

## 確かめること

- `scripts/check.sh --inside-codex` が通ること。
- リリースのビルドの実行ファイルの大きさ（三つの対象）。完了の報告に書く。

## テスト

- `tests/cli.rs` の三つの契約（`--version`、`--help`、知らない引数）。

## 完了条件

- 上の「作るもの」がそろい、`scripts/check.sh --inside-codex` が通る。
- オーケストレータが、サンドボックスの外で `scripts/check.sh`（`linux` の段を含む）を通す。`linux` の段で失敗したら、記録を添えて直しを依頼する。
