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
