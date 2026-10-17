# P2-04 bubblewrap の版と setuid の確かめ、特権の除去（BH-094 の一部）

## 目的

Linux の bubblewrap について、2026-10-17 に決めた三つの点（設計書 03-02 の「サンドボックスの掛け方」）を実装する。

- 版が 0.8.0 以上であることを確かめる。
- `/usr/bin/bwrap` に setuid の印があれば使わない。
- 全体の指定に `--cap-drop ALL` を加える。

BH-094 の残り（`/usr/bin` の外の配布物、AppArmor で名前空間を限る配布物、引数の数の上限）は扱わない。

## 読む設計書

- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「サンドボックスの掛け方」の bubblewrap の確かめ、「Linux のファイルシステムの見え方」の「全体」、「サンドボックスを掛けられないとき」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「doctor」の Linux の項目
- [未決事項](../../design/open-issues.md)の BH-094
- 既存のもの: `crates/benitoiteh/src/sandbox/linux/availability.rs`（`probe_linux`、`LinuxAvailability`）、`linux/mounts.rs` と `linux/mounts/tests.rs`、`tests/sandbox_linux.rs`

## 作るもの

1. `linux/mounts.rs` の全体の指定に `--cap-drop ALL` を加える（`--die-with-parent` の後）。ユニットテストの期待の並びも直す。
2. `probe_linux` の `bubblewrap` の欄の確かめに、次を加える。どちらかを満たさなければ、`CapabilityStatus::Unavailable` に理由の文（英語。`text` に置く）を入れる。
   - `/usr/bin/bwrap` のメタデータ（リンクを辿る）に setuid の印（`0o4000`）がない。
   - `bwrap --version` の標準出力の最初の行が `bubblewrap <版>` の形で、版が 0.8.0 以上である。版は `.` で区切った数の並びとして比べる（`0.10.0` は `0.8.0` より新しい）。形が違えば使えないものとする。`--version` は今と同じ起動の仕方（`probe_linux` の中の起動）で取る。
3. 版の文字列を読む関数と、版を比べる関数を、ユニットテストできる形で分ける。

## 触るファイル

- `crates/benitoiteh/src/sandbox/linux/`（`availability.rs`、`mounts.rs`、`mounts/tests.rs`）、`sandbox/text.rs`
- `crates/benitoiteh/tests/sandbox_linux.rs`

## テスト

- ユニットテスト: 版の読み取り（`bubblewrap 0.12.0`、`bubblewrap 0.8.0`、`bubblewrap 0.7.9`、`bubblewrap 0.10.0`、余計な文字のある行、空の出力）と、比べ方の境界。
- `tests/sandbox_linux.rs`:
  - 試験の機械の bubblewrap（0.12.0）で、`probe_linux` の `bubblewrap` が `Available` になる。
  - サンドボックスの中のプロセスの `/proc/self/status` の `CapEff` が `0000000000000000` である（目的のプログラムに `sandbox-probe` の `read` で `/proc/self/status` を読ませるか、内容を出す操作を加えてよい）。

setuid の印の確かめは、試験の機械に setuid の bubblewrap がないので、メタデータから判定する関数をユニットテストで確かめる（権限の値を引数に取る形にする）。

## 確かめること

- 試験の機械で、`--cap-drop ALL` を加えた後も既存の `sandbox_linux` のテストがすべて通ること（オーケストレータが走らせる）。
- `CapEff` のテストを、podman の中の root（`--userns=keep-id` なし）でも一度走らせ、空になること（オーケストレータが走らせる。root でない利用者では、`--cap-drop ALL` がなくても空なので、このテストだけでは区別できない）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
