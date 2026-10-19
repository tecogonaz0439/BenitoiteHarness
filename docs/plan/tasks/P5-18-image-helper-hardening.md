# P5-18 画像の補助のプロセスの締め直しと、後始末の失敗で起動を止めないこと

## 目的

P5-15 のセキュリティのレビューの所見（low と info）を直す。設計書 01-03「画像のファイルの読み取り」の「サンドボックス」と、01-05「記録を残す期間」を、2026-10-19 に次のとおり改めた。

1. 画像の補助（`benitoiteh __image`）に書ける場所を持たせない。補助ごとの作業ディレクトリをなくす（Linux で、作業ディレクトリごとに `placeholders/` に錠のファイルが増えて消えないことも、これで解消する）。
2. 補助は子のプロセスを作れず（`fork` を許さず、`RLIMIT_NPROC` を 0）、ファイルを書けず（`RLIMIT_FSIZE` を 0）、コアのダンプを残さない（`RLIMIT_CORE` を 0）。
3. 補助は、標準出力に書き終えたら明示して `flush` し、失敗すれば 0 以外で終わる。
4. 起動のときの古いセッションの削除（`session::cleanup`）が失敗しても、起動を止めず、消せなかったことを示して続ける。

この作業で本物の LLM の提供者を呼ばない。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「画像のファイルの読み取り」（「サンドボックス」の項目）
- [セッション](../../design/01-agent/01-05-sessions.md)の「記録を残す期間」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「方針の要素」「Linux のファイルシステムの見え方」
- 既存のもの: `crates/benitoiteh/src/sandbox/restricted.rs`（`image_policy`）、`src/sandbox/policy.rs`（`PolicyConfig` の検証。書ける場所を空にできない規則）、`src/sandbox/macos.rs`（`(allow process-fork)` を出す所と、書ける場所の規則）、`src/sandbox/linux/`（書ける場所の bind、守る名前の錠）、`src/tools/read/image_process.rs`（`Workdir`、起動、後始末）、`src/tools/read/image_helper.rs`（出力、`RLIMIT_AS`）、`src/session/mod.rs`（`cleanup`）、`src/cli/exec.rs`（347 行あたりの `session::cleanup(...)?`）

## この作業が認める型の変更

- `PolicyConfig` に、書ける場所を持たない方針を表す形を加える変更（例: 書ける場所を空にしてよい印、または画像の補助のための専用の方針の型）。今の方針（書ける場所を空にできない）は、ほかの用途ではそのまま保つ。
- macOS の方針の生成で、`process-fork` を許すかを方針ごとに選べるようにする変更。

## 作るもの

### 1. 書ける場所のない方針

- 画像の補助の方針は、書ける場所を持たない。macOS では `file-write*` を一つも許さず、Linux では書ける場所の bind（と守る名前の錠と塞ぐ場所）を作らない。Landlock の書き込みの権利も与えない。
- 補助の作業ディレクトリ（`image-<乱数>`）を作らない。補助の作業ディレクトリ（`cwd`）は、補助が読めなくてもよい既存のディレクトリ（例: `/`）とし、補助はファイルを開かない。
- `cleanup` するものがなくなるので、`Workdir` とその後始末を除く。

### 2. 補助の資源の上限

- 補助の入口で、`RLIMIT_AS`（Linux だけ、今のまま）に加え、両方の OS で `RLIMIT_NPROC`・`RLIMIT_FSIZE`・`RLIMIT_CORE` を 0 にする（`rustix::process::setrlimit`。下げる値なので掛けられないことは想定しないが、失敗したら誤りで終わる）。
- macOS の画像の補助の方針では `(allow process-fork)` を出さない。ほかの方針は今のまま。

### 3. 出力の書き出し

- 補助の書き込みの後に `flush` を加え、失敗したら標準出力に追加で書かずに 1 で終わる（主のプロセスは、終了状態が 0 でない出力を捨てる）。

### 4. 後始末の失敗で起動を止めない

- `session::cleanup` は、個々のセッションのディレクトリと一時ディレクトリの削除に失敗しても、ほかの削除を続け、失敗したものの一覧を返す。起動は止めず、消せなかったセッションの ID と短い理由を画面（対話の画面の知らせと、`exec` の標準エラー出力の一行）に示す。記録の大きさの数え（10 GiB の上限）は、残ったものも数える。
- 削除の深さと件数に上限を掛ける必要はない（標準の `remove_dir_all` のままでよい）が、深い入れ子でスタックが溢れないよう、削除は今どおり主のスレッド（既定のスタックの大きさ）で行う。【要検証】深さ数千の入れ子で `remove_dir_all` がスタックを溢れさせるかは、レビューで macOS の 2 MiB のスタックで深さ 1000 で溢れたと報告された。主のスレッドのスタック（macOS 8 MiB）で深さ 3,000 を消せるかを試し、溢れるなら、削除を明示の大きいスタック（例: 64 MiB）のスレッドで行う。

## 触るファイル

- `crates/benitoiteh/src/sandbox/policy.rs`、`src/sandbox/restricted.rs`、`src/sandbox/macos.rs`、`src/sandbox/linux/`（要れば）
- `crates/benitoiteh/src/tools/read/image_process.rs`、`src/tools/read/image_helper.rs`
- `crates/benitoiteh/src/session/mod.rs`、`src/cli/exec.rs`、`src/ui/`（知らせの文言）
- テスト: `tests/sandbox_read_image.rs`、`tests/image_helper.rs`、`tests/sandbox_macos.rs`・`tests/sandbox_linux.rs`（方針の生成のテストがあれば）、`tests/session.rs`（後始末）

## テスト

- 画像の補助の方針: 補助の中から、どの場所にも書けないこと（`sandbox-probe` の書き込みで確かめる。macOS と Linux）。`fork` できないこと（macOS は方針で、両方の OS で `RLIMIT_NPROC` で）。縮める処理は今までどおり成功すること。
- Linux: 縮めても `placeholders/` に錠のファイルが増えないこと。
- 補助: 正しい入力で JSON と中身を書き、`flush` まで済ませて 0 で終わること（今のテストを保つ）。
- 後始末: 消せないもの（`chmod 000` の子ディレクトリなど）を含む期間切れのセッションがあっても、起動が成功し、消せなかったことが示され、ほかの期間切れのセッションは消されること。深さ 3,000 の入れ子を含む期間切れのセッションを消せること（スタックが溢れないこと）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。サンドボックスのテストは、オーケストレータが外で走らせる全体の検査で確かめる。
- 完了の報告に、画像の補助の macOS の方針の文（書き込みと `fork` の規則がないこと）を書く。
