# P5-39 TLS の依存を更新した前後の実行ファイルの大きさ

2026-10-25 に P5-39 の実装者が測った。設計書 06-01「依存の基準」と 06-02「測り方」に従い、TLS の依存の版を更新した前後の release の実行ファイルの大きさを比べた。

## 測り方

- 測定した環境は macOS 27.0.1（26A434）、arm64。Rust は `rustc 1.98.1 (48a229cea 2026-09-01)`、Cargo は `1.98.1 (797e8a9bc 2026-08-05)` である。ホストは `aarch64-apple-darwin`、Linux の二つは同じホストからクロスビルドした。
- 更新前は P5-38 のコミット `a559e9553c90fc27e7c73ede69b0d89151651188` の依存である。更新後は、同じコミットに P5-39 の依存と文書の変更を加えた未コミットの作業ツリーである。本体とテストの Rust のソースは変えていない。
- 両方で `CARGO_NET_OFFLINE=true` を指定した。取得済みのレジストリのデータを参照する `CARGO_HOME` を作業ツリー内の `work/P5-39-tls-advisories/cargo-home` に用意し、Cargo の書き込み先を分けた。`CARGO_TARGET_DIR` は同じ作業ツリー内の `target/` とした。
- release の設定は両方とも `opt-level = "s"`、`lto = "fat"`、`codegen-units = 1`、`panic = "abort"`、`strip = "symbols"` である。Linux のリンカは `.cargo/config.toml` の `rust-lld` を使った。測定のための追加の strip は行っていない。
- `rustls` の `default-features = false` と機能 `std`・`tls12`、`ureq` 3.4.2 の機能 `rustls-no-provider`・`platform-verifier`、`rustls-graviola` 0.4.0 の `default-features = false` を保った。ほかの依存の版と機能も変えていない。

`scripts/check.sh` の `size` の段と同じく、ビルドを実行した後に、ホストは `target/release/benitoiteh`、Linux は `target/<対象>/release/benitoiteh` を `wc -c` でバイト単位で測った。更新前は次のコマンドを順に実行した。

```sh
export CARGO_NET_OFFLINE=true
export CARGO_HOME="$PWD/work/P5-39-tls-advisories/cargo-home"
export CARGO_TARGET_DIR="$PWD/target"
cargo build --offline --release -p benitoiteh
wc -c < "$CARGO_TARGET_DIR/release/benitoiteh"
cargo build --offline --release -p benitoiteh --target aarch64-unknown-linux-musl
wc -c < "$CARGO_TARGET_DIR/aarch64-unknown-linux-musl/release/benitoiteh"
cargo build --offline --release -p benitoiteh --target x86_64-unknown-linux-musl
wc -c < "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/benitoiteh"
```

更新後の値は、同じ Cargo の環境変数を指定した `scripts/check.sh --inside-codex` の `size` の段から記録した。同スクリプトは `cargo build --release -p benitoiteh` と、Linux の二つの `--target` を付けたビルドを行い、同じファイルを `wc -c` で測る。テストには、オーケストレータが `v0.0.7` のタグのアーカイブからビルドした macOS 向けの処理系を `BENITOITEH_TEST_BENITOITE` に指定した。検査全体は終了状態 0 で `All checks passed.` に到達した。

## 結果

バイト数であり、増減の割合は更新前の値を分母として小数点以下 2 桁に丸めた。

| 対象 | 更新前 | 更新後 | 増減 |
|---|---:|---:|---:|
| `aarch64-apple-darwin` | 5,723,072 | 5,723,072 | 0（0.00%） |
| `aarch64-unknown-linux-musl` | 6,250,344 | 6,249,264 | -1,080（-0.02%） |
| `x86_64-unknown-linux-musl` | 7,515,856 | 7,517,296 | +1,440（+0.02%） |

更新した依存は次の三つである。`Cargo.lock` の差分は、この三つの版と checksum だけであり、ほかのパッケージの追加・削除・版の変更はない。

| クレート | 更新前 | 更新後 |
|---|---|---|
| `rustls` | 0.23.45 | 0.23.46 |
| `rustls-webpki` | 0.103.15 | 0.103.16 |
| `webpki-root-certs` | 1.0.9 | 1.0.10 |

比較した `Cargo.lock` の SHA-256 は、更新前が `134251633faa384dffc00f1aa79be4ffbc76dea2c5f428289cfe1e2a897658b7`、更新後が `4164e679a7e5bf648ff1858ad515e670a337193875886ce733358319ba4e73d4` である。

この記録は実行ファイルの大きさの比較であり、速度やメモリー使用量は測っていない。`--inside-codex` で除く OS のサンドボックスと Linux 上の実行、最新のデータベースによる勧告の検査は、オーケストレータがサンドボックスの外で行う。`webpki-root-certs` は検査対象の三つの構成に組み込まれないため、勧告の検査の出力とは別に `Cargo.lock` の更新後の版を確認した。
