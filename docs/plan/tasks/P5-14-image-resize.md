# P5-14 上限を超える PNG と JPEG を縮める

## 目的

設計書 01-03「画像のファイルの読み取り」に 2026-10-19 に決めた BH-108 の決定を実装する。`read` は、縦か横が 2,000 画素を超えるか、3.75 MiB を超える PNG と JPEG を、解いて縮めてから画像として返す。GIF と WebP は縮めず、今までどおり誤りを返す。依存に `image`（既定の機能を切り、`png` と `jpeg` だけ）を加える。

この作業で本物の LLM の提供者を呼ばない。P5-13 を取り込んだ `main` から始める。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「画像のファイルの読み取り」（とくに【決定】の縮め方の項目）
- [クレートの構成](../../design/06-impl/06-01-crate-structure.md)の「依存の基準」「依存の候補」
- [P5-11 の大きさの測定](../results/P5-11-image-resize-size.md)
- 既存のもの: `crates/benitoiteh/src/tools/read/image.rs`（`execute`、`inspect`、形式ごとの寸法の読み取り）、`src/tools/read.rs`、`src/tools/mod.rs`（`MAX_IMAGE_BYTES`、`MAX_IMAGE_DIMENSION`、`MAX_FILE_BYTES` は `read.rs`）、`src/tools/text.rs`、`crates/benitoiteh/Cargo.toml`、`tests/tools_read.rs`

## この作業が認める型の変更と依存

- `crates/benitoiteh/Cargo.toml` に `image = { version = "=0.25.10", default-features = false, features = ["png", "jpeg"] }` を加える。オーケストレータが `image` 0.25.10 を手元のキャッシュに取得済みである（P5-11 の測定で使った）。`Cargo.lock` の変化は、この依存とその依存だけにする。
- `tools/read/image.rs` の中の関数の分け方の変更。

## 作るもの

### 1. 縮めるかの判断

- 形式を判定し、見出しから寸法を読んだ後（動く PNG は今までどおり拒む）、次のときに縮める処理へ進む。
  - 形式が PNG か JPEG で、幅か高さが 2,000 を超えるか、ファイルが 3,932,160 バイトを超える。
- そのほかは今までどおり（GIF と WebP は上限を超えたら誤り。上限の中の画像は縮めずにそのまま送る）。
- 縮める処理へ進む画像は、見出しの幅 × 高さが 40,000,000 を超えるか、ファイルが 64 MiB（`MAX_FILE_BYTES`）を超えるなら、解かずに誤りを返す（大きさ・画素の数と上限を示す文）。
- `offset`・`limit` の指定と、`image_input` が偽のときの誤りは、今までどおり縮めるかの判断より前に返す。

### 2. 縮め方

- ファイルを 64 MiB ＋ 1 バイトまでで読み、読む前と後で版が変わっていないことを確かめる（今と同じ）。
- `image` で解く。解くときに `image::Limits` を掛ける（幅と高さの上限を見出しの値に、確保の上限を例えば 512 MiB に）。解けなければ壊れた画像の誤りを返す。
- JPEG の向きの情報（EXIF の Orientation）があれば、解き手の `orientation()` と `DynamicImage::apply_orientation` で向きを直す。
- 縦横の比を保って、両辺 2,000 画素以下に縮める（`FilterType::Triangle`。もとから 2,000 以下の辺しかない画像（大きさだけが上限を超えるもの）は寸法を変えずに符号化し直す）。
- 透明の部分を実際に持つ画像（アルファの値に 255 未満の画素がある）は PNG に、そうでなければ JPEG（品質 80）に符号化する。JPEG は透明の部分を持たない RGB にしてから符号化する。
- 結果が 3,932,160 バイトを超えるときは、JPEG なら品質 60、40 の順に符号化し直す。それでも超えるときと、PNG で超えるときは、大きさと上限を示す誤りを返す。
- 縮めた後の中身を、今の画像と同じに `objects/` に保存し（容量の上限の扱いも同じ）、`ToolImage` の MIME の型・寸法・バイト数は縮めた後の値とする。縮めた後の中身は `inspect` を通ることを確かめる（再開の確かめ直しと同じ規則に合うように）。
- 読んだファイルの版の記録は、元のファイルに対して行う（今と同じ）。

### 3. 結果の文

- 縮めたときの結果の文と画面の一行は、`Read image file <MIME の型> (<幅>x<高さ>, <バイト数> bytes; resized from <元の幅>x<元の高さ>)` とする（`tools/text.rs`）。縮めないときは今の文のまま。

### 4. 大きさの記録

- 依存を加えた前と後の、`aarch64-apple-darwin` の release の実行ファイルの大きさを、完了の報告に書く（P5-11 の測定では約 +298 KB の見込み）。

## 触るファイル

- `crates/benitoiteh/Cargo.toml`、`Cargo.lock`
- `crates/benitoiteh/src/tools/read/image.rs`（要れば新しい `src/tools/read/resize.rs`）、`src/tools/text.rs`
- テスト: `crates/benitoiteh/tests/tools_read.rs`（要れば `tests/support/images.rs`）

## テスト

テストの画像は、テストの中で `image` を使って組み立てる（リポジトリに画像のファイルを加えない）。

- 3,000×1,500 の PNG（透明なし）が、2,000×1,000 の JPEG になり、結果の文に `resized from 3000x1500` が入ること。`objects/` の中身が JPEG で、`inspect` を通ること。
- 透明の部分を持つ 2,400×2,400 の PNG が、2,000×2,000 の PNG になること。
- 2,000×2,000 以下で 3.75 MiB を超える PNG（雑音の画素で大きくしたもの）が、寸法を変えずに JPEG になり、3,932,160 バイト以下になること。
- EXIF の向き（例: 6 = 時計回りに 90 度）を持つ 3,000×1,000 の JPEG が、向きを直した 667×2,000（か 666×2,000。丸め方は実装に従い、テストは縦が 2,000 で横が縦より短いことを確かめる）になること。
- 見出しの画素の数が 40,000,000 を超える PNG（見出しだけを書いた小さなファイルでよい）が、解かれずに誤りになること。
- 見出しは正しいが中身の壊れた 3,000×3,000 の PNG が、壊れた画像の誤りになり、panic しないこと。
- 2,000 を超える GIF と WebP は、今までどおり寸法の誤りになること。2,000 以下の PNG と JPEG は縮めずにそのまま送られること（今のテストを保つ）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。ライセンスの検査が新しい依存を受け付けること。
- 完了の報告に、実行ファイルの大きさの前と後と、加わったクレートの一覧を書く。

## 補足（2026-10-19、始める前の点検）

本節は、上の各節に優先する。`image` を `docs/plan/10-interfaces.md` の「依存のクレート」の表に加えた。設計書 01-03 に、縮める処理を一度に一つにすることと、向きの情報の扱い（縮めてから直す、PNG の `eXIf` も含む）と、「resized from」の寸法（見出しの寸法）を加えた。

1. **検査の順**（`tools/read/image.rs` の `execute`、35〜97 行）:
   - `offset`・`limit`（44〜46 行）と `image_input`（47〜49 行）は今のまま先に返す。
   - 50〜52 行の、メタデータの大きさの検査は形式ごとの上限にする。PNG と JPEG は `super::MAX_FILE_BYTES`（`read.rs` の 13 行。子のモジュールから届く）、GIF と WebP は `MAX_IMAGE_BYTES`。読む上限（54〜56 行）と読んだ後の検査（66〜68 行）も同じ上限（＋1）にする。版の確かめ（62〜64 行）は今のまま。`Format` は `matches!` で比べるか `PartialEq` を付ける。
   - `inspect`（100〜117 行）を分け、`header(bytes) -> Result<(Format, &'static str, u32, u32), ToolError>`（形式の判定、動く PNG の拒否、`IMAGE_INVALID`、幅か高さが 0 の誤り）を作る。`inspect` は `header` に 2,000 以下の検査を足したものとし、`session/objects.rs` の再開の確かめ直しは今のまま `inspect` を使う。
   - 幅か高さが 0 の誤りは、縮めるかの判断より前に返す。
   - PNG か JPEG で、幅か高さが 2,000 を超えるか、大きさが `MAX_IMAGE_BYTES` を超えるときは、`u64::from(w).saturating_mul(u64::from(h)) > 40_000_000` なら誤りを返し、そうでなければ縮める。縮めた中身に `inspect` を掛け、MIME の型と寸法をそこから取る。そのほかは今の `inspect` を使う（2,000 を超える GIF と WebP は寸法の誤り）。
   - 画素の数の検査は、ファイル全体（64 MiB まで）を読んだ後になる（PNG の見出しの読み取りが `IDAT` までの塊を辿るため）。これでよい。
   - 保存、ハッシュ、元のファイルに対する `record_read_version`（70〜83 行）は今のまま。
2. **誤りの文**（`tools/text.rs`。593〜608 行の近く）:
   - 解けない（`ImageError` の解く誤りと上限の誤り）: `IMAGE_DECODE`、`The image could not be decoded.`
   - 画素の数: `image_pixels(width, height, limit)`。例: `Image has 8001x5000 pixels; the limit for resizing is 40000000 pixels.`
   - 64 MiB を超える: `image_too_large(size, MAX_FILE_BYTES)` を使う（`READ_TOO_LARGE` は grep を勧める文なので使わない）。
   - 縮めた後も上限を超える: 「after resizing」を含む `image_too_large` の形の文。
   - 結果の文: `read_resized_image(media, w, h, bytes, orig_w, orig_h)`。元の寸法は見出しの寸法（向きを直す前）。
3. **一度に一つ**: 縮める区間（解く、縮める、向きを直す、符号化する）を、プロセスで一つの `static RESIZE: std::sync::Mutex<()>`（`OnceLock` は要らない）で囲む。毒された（poisoned）錠は `into_inner` で取り戻して続ける（panic を表に出さない）。
4. **`image` の呼び方**（作業の前の点検で、ワークスペースの lint の下で通ることを確かめた）:

   ```rust
   use std::io::Cursor;
   use image::codecs::jpeg::JpegEncoder;
   use image::codecs::png::PngEncoder;
   use image::imageops::FilterType;
   use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

   let mut reader = ImageReader::with_format(Cursor::new(bytes.as_slice()), format);
   let mut limits = Limits::default(); // #[non_exhaustive] なので式で作れない
   limits.max_image_width = Some(width); // 見出しの値
   limits.max_image_height = Some(height);
   limits.max_alloc = Some(512 * 1024 * 1024);
   reader.limits(limits.clone());
   let mut decoder = reader.into_decoder()?; // set_limits で見出しの寸法と照らす
   limits.reserve(decoder.total_bytes())?; // into_decoder はこれをしない
   let orientation = decoder.orientation()?;
   let mut image = DynamicImage::from_decoder(decoder)?;
   if image.width() > 2000 || image.height() > 2000 {
       image = image.resize(2000, 2000, FilterType::Triangle); // 比を保って箱に収める。拡大しないよう条件の中だけで呼ぶ
   }
   image.apply_orientation(orientation); // 縮めた後に直す
   ```

   - 透明の部分の判定: `has_alpha()` は色の型しか見ないので、`as_rgba8()`・`as_luma_alpha8()` なら画素のアルファの値を、そのほかは `to_rgba16()` で、`MAX` 未満の画素があるかを見る。
   - PNG は `DynamicImage::ImageRgba8(image.into_rgba8()).write_with_encoder(PngEncoder::new(&mut out))`（16 ビットの元の画像を 16 ビットのまま書かない）。JPEG は `DynamicImage::ImageRgb8(image.into_rgb8())` を `JpegEncoder::new_with_quality(&mut out, quality)` で、品質 80・60・40 の順に。
   - 定数の添字（`pixel.0[3]`）は `indexing_slicing` に当たらない。非定数の整数の `*`・`+` は `arithmetic_side_effects` に当たるので、`saturating_*` か `checked_*` を使う。`as` は使わない。
5. **既存のテストの直し**: `tests/tools_read.rs` の `read_image_limits_reject_invalid_headers_and_preserve_text_fallback` のうち、1351〜1354 行（2000×2000 の PNG の見出しを 3,932,161 バイトに埋めたもの）と 1355 行（2001×123・123×2001 の PNG の見出し）は、縮める処理に進んで解く誤りになる。この二つは GIF（例: 2001×123 は `b"GIF89a\xd1\x07\x7b\0"`、大きさの場合は GIF を `set_len` で埋める）に移す。幅か高さが 0 の PNG（`(0,123)`・`(123,0)`）は PNG のまま、寸法の誤りであることを保つ。これで「今のテストを保つ」の趣旨とする。
6. **縮める処理のテストの置き場所と大きさ**: 新しいファイル `tests/tools_read_image.rs` に置く（`check.sh` は `sandbox_*` 以外のテストのファイルを自動で拾う）。画像を組み立てる関数もこのファイルに置き、`tests/support/images.rs` には加えない（提供者のテストが `#[path]` で読み込むので、使わない関数が `dead_code` の警告になり、警告は誤りとして扱われる）。デバッグのビルドでは画像の処理が遅く、同じバイナリの時間の上限のあるテストを揺らしうるので、画像を小さくする。
   - 透明なし: 2,400×1,200 の PNG → 2,000×1,000 の JPEG。
   - 透明あり: 2,100×2,100 の PNG → 2,000×2,000 の PNG。
   - 大きさだけ: 1,200×1,200 の雑音の PNG（約 4.3 MB）→ 1,200×1,200 の JPEG（約 2.0 MB）。
   - 向き: `JpegEncoder::set_exif_metadata` で Orientation 6 を書いた 2,400×800 の JPEG → 縦が 2,000 で横が縦より短い画像、結果の文は `resized from 2400x800`。EXIF は次の TIFF の並びでよい: `b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01"` に値の大端の 2 バイトと 6 バイトの 0 を足したもの。
   - 画素の数: 見出しだけの `png_header(8001, 5000)` が画素の数の誤り、`png_header(8000, 5000)`（ちょうど 4,000 万）は解く誤りになること。
   - 壊れた中身: 本物の 3,000×3,000 の PNG を組み立て、51 バイト目から 200 バイトほど `IDAT` の中身を書き換えたものが解く誤りになり、panic しないこと（この一枚は 2,100×2,100 などに小さくしてよい）。
   - 雑音の画素は、xorshift の式で決まった値にする（例: `s ^= s << 13; s ^= s >> 7; s ^= s << 17`）。
7. **触るファイルの訂正**: `tests/support/images.rs` は触らない。`tests/tools_read_image.rs` を加える。ワークスペースの `Cargo.toml` の開発のビルドの設定は変えない。
8. **大きさの記録**: 完了の報告の大きさは、オーケストレータが取り込むときに `docs/plan/results/` に写す。
