# P0-06 要る機能を一通り持つ構成の大きさの測定（BH-062）

## 目的

P1〜P4 で使う依存のクレートをすべてつないだ構成の、リリースのビルドの実行ファイルの大きさを、三つの対象の環境で測る。[性能](../../design/06-impl/06-02-performance.md)の目標の値が妥当かを確かめる材料にする（BH-062）。

## P0-05 を除いて行うこと（2026-10-18 の改め）

P0-05 が止まった（README）ので、この作業は P1-10 の後の `main` で行う。表の「P0-05 の後の `benitoiteh`」の行は「P1-10 の後の `benitoiteh`」と読み替える。Linux だけの依存（`landlock`・`seccompiler`・`libc`）は `main` にないので、`size_probe` でも使わない。P0-05 の完了の報告によれば、補助のプロセスを加えると Linux の実行ファイルは aarch64 で 146,632 バイト、x86_64 で 179,680 バイト増えた。この値を結果の節に添える。

## 読む設計書

- [性能](../../design/06-impl/06-02-performance.md)（全体）
- [クレートの構成](../../design/06-impl/06-01-crate-structure.md)の「依存の候補」「ビルドの設定」
- [作業の間の約束](../10-interfaces.md)の「依存のクレート」

## 作るもの

1. `crates/benitoiteh/examples/size_probe.rs`: 10-interfaces の「依存のクレート」の表のクレートと、P4 で加える `crossterm`・`unicode-width` のそれぞれについて、試作で使う見込みの主な機能を一度ずつ呼ぶ小さなプログラム。呼んだ結果を標準出力に書くか、引数で処理を選ぶ形にして、最適化で消されないようにする。例: `toml` で設定の形を読む、`ignore` でディレクトリを辿る、`regex` で探す、`similar` で差分を作る、`sha2` でハッシュを求める、`crossterm` で端末の大きさを得る、`http` の `HttpClient` を作る、`sandbox` の方針を JSON にする。
   - `[dependencies]` に入っていないクレート（P4 で使う `crossterm`・`unicode-width` など）は、この例のためだけに `[dev-dependencies]` に加える。各作業で `[dependencies]` に移す。理由: `[dependencies]` に加えると、表の「P0-05 の後の `benitoiteh`」の値が変わる。
2. 測った結果の記録。`docs/plan/tasks/P0-06-size-probe.md` の末尾の「結果」の節に、次の表を書く。

| 構成 | aarch64-apple-darwin | x86_64-unknown-linux-musl | aarch64-unknown-linux-musl |
|---|---|---|---|
| P0-05 の後の `benitoiteh`（`opt-level = 3`） | | | |
| `size_probe`（`opt-level = 3`） | | | |
| `size_probe`（`opt-level = "s"`） | | | |

- 大きさはバイト数と MiB（小数点以下 1 桁）で書く。`opt-level = "s"` は、`--config 'profile.release.opt-level="s"'` で一時的に変えて測り、`Cargo.toml` は変えない。
- 測った日、Rust の版、コミット（作業ツリーの元の `main` のコミット）を添える。
- `cargo bloat` などの道具は使わない（入っていない）。大きい依存を知りたいときは、依存を一つずつ外した `size_probe` を測ってよい（任意）。

## 触るファイル

- `crates/benitoiteh/Cargo.toml`（`[dev-dependencies]` と、`examples` の設定）
- 本書の末尾（「結果」の節を加える。00-common の「作業の範囲」の、作業の文書が求めた例外に当たる）

## 確かめること

- 上の表の値。
- [性能](../../design/06-impl/06-02-performance.md)の目標の値と比べて、余裕があるか、超えるか。

## テスト

測定の作業なので、テストを加えない。`scripts/check.sh --inside-codex` が `size_probe` を含めて通ればよい（clippy は `examples` も対象である）。

## 完了条件

- `scripts/check.sh --inside-codex` が通り、「結果」の節がある。

## 結果

2026-10-17（JST）に、次の条件で測定した。

- 測定元の `main` のコミット: `0e6517775bd0638fb65fe65982b2f01d9c804dcc`（P1-10 を取り込んだコミット）。このコミットから分けた `impl/P0-06-size-probe` の作業ツリーで測定した。
- Rust: `rustc 1.98.1 (48a229cea 2026-09-01)`。コンパイラのコミットは `48a229ceaefd4985c50990b14116b6d856af0985`、LLVM は 22.1.8、ホストは `aarch64-apple-darwin` である。Linux の二つの対象はホストからクロスビルドした。
- `Cargo.lock`: 元の `main` の上記コミットに含まれるものに、今回の開発用の依存とその推移的な依存を加えた。測定時の SHA-256 は `943d1892c0ac79873530526001a9ac194409c808ca13a15247bf750f779fe09a` である。既存の依存の版は変えていない。
- 依存の版と機能: [クレートのマニフェスト](../../../crates/benitoiteh/Cargo.toml)のとおり。追加したものは `crossterm = "=0.29.0"`（`default-features = false`、`events`・`bracketed-paste`）と `unicode-width = "=0.2.2"`（`default-features = false`）であり、両方を `[dev-dependencies]` に置いた。
- ビルドの設定: `lto = "fat"`、`codegen-units = 1`、`panic = "abort"`、`strip = "symbols"`。`opt-level` だけを表のとおり変えた。`"s"` はコマンドラインの `--config` で指定し、ワークスペースの `Cargo.toml` は変えていない。
- 大きさ: ビルドした実行ファイルそのもののバイト数。圧縮していない。1 MiB = 1,048,576 バイトとして換算し、小数点以下 1 桁に丸めた。

| 構成 | aarch64-apple-darwin | x86_64-unknown-linux-musl | aarch64-unknown-linux-musl |
|---|---|---|---|
| P1-10 の後の `benitoiteh`（`opt-level = 3`） | 4,526,272 バイト（4.3 MiB） | 6,031,504 バイト（5.8 MiB） | 4,867,224 バイト（4.6 MiB） |
| `size_probe`（`opt-level = 3`） | 4,129,184 バイト（3.9 MiB） | 5,485,712 バイト（5.2 MiB） | 4,444,728 バイト（4.2 MiB） |
| `size_probe`（`opt-level = "s"`） | 3,386,624 バイト（3.2 MiB） | 4,449,904 バイト（4.2 MiB） | 3,663,232 バイト（3.5 MiB） |

ホスト向けには、次のコマンドでビルドした。Linux 向けには、それぞれに `--target x86_64-unknown-linux-musl` または `--target aarch64-unknown-linux-musl` を加えた。

```sh
cargo build --offline --locked --release -p benitoiteh
cargo build --offline --locked --release -p benitoiteh --example size_probe
cargo build --offline --locked --release -p benitoiteh --example size_probe --config 'profile.release.opt-level="s"'
```

基準の `benitoiteh` のビルドは `CARGO_NET_OFFLINE=true scripts/check.sh --inside-codex` の `size` の段で行った。ホスト向けは `target/release/benitoiteh` と `target/release/examples/size_probe`、Linux 向けは `target/<対象>/release/` の下の同じ名前のファイルを測った。`"s"` のビルドで同じ出力先が上書きされる前に、`opt-level = 3` のバイト数を記録した。

`size_probe` は、TOML の読み取りと JSON の読み書き、`ignore` の走査、`globset` の照合、`regex` の検索、`similar` の差分、SHA-256、乱数、プロセス ID、シグナルの登録と解除、サンドボックス方針の JSON の往復、SSE の読み取り、Skill を含む既存の文脈の組み立て、文字の幅、端末の大きさを呼び、結果を標準出力に書く。`--http URL` の分岐は既存の `HttpClient` の GET を呼ぶので、`ureq`・`rustls`・`rustls-graviola` と OS の証明書の検証を含む通信の処理もリンクの対象となる。`--terminal` の分岐は行の描画、入力の待ち合わせ、届いたイベントの読み取りを呼ぶ。引数なしの実行では HTTP 通信も端末の入力待ちも行わない。

`crossterm` の機能は、キャッシュの `crossterm-0.29.0/Cargo.toml` の `[features]` と、`src/terminal.rs` の `size`、`src/event.rs` の `poll`・`read` を確認して選んだ。文字の幅は `unicode-width-0.2.2/src/lib.rs` の `UnicodeWidthStr::width` を使った。これらのソースは `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/` の下にある。

[性能](../../design/06-impl/06-02-performance.md)の大きさの目標は、macOS 向けで 8 MiB（8,388,608 バイト）以下である。今回の macOS 向けはすべてこの値を下回り、P1-10 の後の `benitoiteh` には 3,862,336 バイト（3.7 MiB）、`size_probe` の `opt-level = 3` には 4,259,424 バイト（4.1 MiB）の余裕があった。`"s"` の `size_probe` は `3` より 742,560 バイト（18.0%）小さかった。速度は測っていないので、`opt-level` を変える判断（BH-060）は残る。

この測定は依存の主な機能を使う小さな構成の測定であり、P2〜P4 の実装をすべて組み込んだ値ではない。`size_probe` はエージェント全体の処理を呼ばないので、既存の `benitoiteh` より小さい結果になっている。今回の値だけでは、完成した試作が 8 MiB 以下になるとは確定しない。

Linux の `landlock`・`seccompiler`・`libc` の直接の依存と、Linux の補助のプロセスは、今回の構成には含まない。本書の「P0-05 を除いて行うこと」に記された P0-05 の完了の報告では、補助のプロセスを加えたときの増分は aarch64 Linux で 146,632 バイト、x86_64 Linux で 179,680 バイトであった。この増分は今回測り直した値ではなく、表にも加算していない。

`scripts/check.sh --inside-codex` は `fmt`・三つの対象の `clippy`・`test`・`deny`・三つの対象の `size` の各検査を通った。指定により `crates/benitoiteh/tests/sandbox_macos.rs` と、`aarch64-unknown-linux-musl`・`x86_64-unknown-linux-musl` のコンテナ内のテストは走らせていない。新しいテストは加えていない。ホスト向けの `size_probe` は `opt-level = 3` と `"s"` の両方で通常実行が成功し、`3` の `--terminal` も端末の大きさの取得、描画、100 ms の入力待ちを終えて正常に終了した。本物の LLM の API は呼んでいない。
