# BH-108 画像を縮めるクレートを加えたときの実行ファイルの大きさ

2026-10-19 にオーケストレータが測った。BH-108（上限を超える画像を縮めるか）の判断の材料である。測るために加えた依存とコードは取り込んでいない。

## 測り方

- `main`（`9963e85`。P5-10 まで）を `git archive` で写した木で、`cargo build --release -p benitoiteh` を行い、`target/release/benitoiteh` の大きさを比べた。macOS（aarch64）、rustc 1.98.1、`opt-level = "s"`。
- 縮める処理は、`image` 0.25.10（`default-features = false`）で、画像を解き、縦と横を 2,000 画素に収まるように縮め（`FilterType::Triangle`）、透明の部分があれば PNG、なければ JPEG にして書く最小の形とした。実行ファイルの入口から呼び、使われないコードとして除かれないようにした。
- 縮めた処理が動くことは、6,016×6,016 の JPEG（9.3 MB）と 3,000×3,000 の PNG（12.1 MB）で確かめた（どちらも 2,000×2,000 の JPEG になった）。

## 結果

| 構成 | 大きさ（バイト） | 増えた量 |
|---|---:|---:|
| 縮めない（今の `main`） | 4,961,072 | — |
| `image` の PNG と JPEG | 5,259,184 | +298,112（+6.0%） |
| `image` の PNG、JPEG、GIF、WebP | 5,391,664 | +430,592（+8.7%） |

4 つの形式の構成で加わるクレートは、`image`、`png`、`zune-jpeg`、`zune-core`、`gif`、`image-webp`、`moxcms`、`pxfm`、`fdeflate`、`weezl`、`color_quant`、`bytemuck`、`byteorder-lite`、`quick-error` などである（`flate2` と `miniz_oxide` は今の依存にもある）。

6,016×6,016 の JPEG を解いて縮めたときの最大の常駐の大きさ（`/usr/bin/time -l`）は約 339 MB、時間は約 0.3 秒だった。ファイルの大きさを 3.75 MiB に抑えても、画素の数の多い JPEG は解くと大きくなるので、縮めるなら、解く前に見出しの寸法で画素の数の上限（例: 4,000 万画素）を掛ける必要がある。
