# P5-14 画像を縮める依存を加えた後の実行ファイルの大きさ

2026-10-19 に P5-14（上限を超える PNG と JPEG を縮める）の実装者が測り、オーケストレータが写した。設計書 06-02「測り方」の、依存を加えたときの記録である。

## 測り方

- 同じ release の設定（`opt-level = "s"`）で、依存を加える前（P5-13 を取り込んだ `main`、`074e502`）と後の `aarch64-apple-darwin` の `target/release/benitoiteh` の大きさを比べた。

## 結果

| 対象 | 加える前 | 加えた後 | 増えた量 |
|---|---:|---:|---:|
| `aarch64-apple-darwin` | 4,977,648 | 5,308,880 | +331,232（+6.65%） |

P5-11 の測定（[P5-11-image-resize-size.md](P5-11-image-resize-size.md)）の見込み（+298,112）より約 33 KB 大きい。P5-11 の測定は縮めて符号化するだけの最小の形で、P5-14 は確保の上限、向きの直し、品質を下げる符号化のやり直し、透明の部分の判定を含む。

加えた依存は `image` 0.25.10（`default-features = false`、機能 `png`・`jpeg`）で、`Cargo.lock` に加わったのは `image`、`png`、`zune-jpeg`、`zune-core`、`moxcms`、`pxfm`、`num-traits`、`autocfg`（ビルドだけ）、`bytemuck`、`byteorder-lite`、`crc32fast`、`fdeflate`、`flate2`、`miniz_oxide`（0.8.9 と 0.9.1）、`adler2`、`simd-adler32`、`zlib-rs`（`flate2` の任意の依存として記録だけされ、ビルドされない）である。
