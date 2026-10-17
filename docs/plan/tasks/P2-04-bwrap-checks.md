# P2-04 bubblewrap の版と setuid の確かめ、特権の除去（BH-094 の一部）

## 目的

Linux の bubblewrap について、2026-10-17 に決めた三つの点（設計書 03-02 の「サンドボックスの掛け方」）を実装する。

- 版が 0.8.0 以上であることを確かめる。
- `/usr/bin/bwrap` に setuid の印があれば使わない。
- 全体の指定に `--cap-drop ALL` を加える。

あわせて、補助のプロセスが、自分の特権が空であることを確かめてから目的のプログラムを起動するようにする。理由: bubblewrap 0.11.0 は、root で動かしたとき、特権を外す `capset` が `EPERM` で失敗しても、誤りにせずに続ける（`bubblewrap.c` の `drop_all_caps`。seccomp で `capset` を拒む環境などで起こりうる）。`--cap-drop ALL` を渡しても特権が残りうるので、補助のプロセスで確かめて、残っていれば動かさない。

BH-094 の残り（`/usr/bin` の外の配布物、AppArmor で名前空間を限る配布物、引数の数の上限）は扱わない。

## 読む設計書

- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「サンドボックスの掛け方」の bubblewrap の確かめ、「Linux のファイルシステムの見え方」の「全体」、「サンドボックスを掛けられないとき」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「doctor」の Linux の項目
- [未決事項](../../design/open-issues.md)の BH-094
- 既存のもの: `crates/benitoiteh/src/sandbox/linux/availability.rs`（`probe_linux`、`LinuxAvailability`、`CapabilityStatus`）、`linux/process.rs`（`SandboxCommand::prepare`）、`linux.rs`（補助のプロセスの入口）、`linux/mounts.rs` と `linux/mounts/tests.rs`、`sandbox/text.rs`、`tests/sandbox_linux.rs`、`crates/sandbox-probe/`

## 作るもの

1. `linux/mounts.rs` の全体の指定に `--cap-drop ALL` を加える（`--die-with-parent` の後、`--unshare-net` の前）。ユニットテストの期待の並び（二か所）も直す。
2. bubblewrap の確かめを一つの関数（例: `check_bubblewrap() -> CapabilityStatus`）にまとめる。順は次のとおりとする。
   1. `/usr/bin/bwrap` のメタデータ（`std::fs::metadata`。リンクを辿る）を取る。ないときは、今の起動の失敗と同じ形の `Unavailable` とする。setuid の印（`PermissionsExt::mode() & 0o4000`）があれば `Unavailable` とする。
   2. `Command::new("/usr/bin/bwrap").arg("--version")` を、サンドボックスの外で直接起動する（環境は空、標準入力は空、標準出力と標準エラー出力は受け取る）。終了状態が 0 でなければ `Unavailable` とする。
   3. 標準出力が、ちょうど一行の `bubblewrap X.Y.Z` と改行であることを確かめる。`X`・`Y`・`Z` は十進の数字だけから成り、それぞれ `u32` に収まる。組 `(X, Y, Z)` を `(0, 8, 0)` と比べ、小さければ `Unavailable` とする。形が違うもの（成分が二つや四つ、`-rc1` のような接尾辞、余計な空白、別の名前、UTF-8 でないもの、空の出力、`u32` に収まらない数）は、すべて `Unavailable` とする。`Unavailable` の文（英語。`text` の関数）は、受け取った行を含める。
3. `probe_linux` は、まず 2 を呼ぶ。`Unavailable` なら、`bubblewrap` の欄をそれとし、ほかの三つの欄を `NotChecked` にして、名前空間の確かめを飛ばす。
4. `SandboxCommand::prepare` も 2 の結果を使い、`Unavailable` なら `SandboxError::Unavailable` を返す。理由: 制限モードの起動は `probe_linux` を通らないので、`prepare` で確かめなければ、setuid のものや古い bubblewrap を使ってしまう。結果はプロセスの中で一度だけ求めて保ってよい（`OnceLock`）。
5. 補助のプロセス（`run_helper`）は、Landlock と seccomp を掛ける前に、自分の特権の集合（有効、許可、継承、アンビエント）が空であることを `rustix` の安全な関数（`rustix::thread::capabilities` など。機能の要否を確かめる）で確かめ、空でなければ準備の失敗（終了状態 125。段の名前は新しく `text` に置く）とする。
6. `sandbox-probe` に、`/proc/self/status` の `CapInh`・`CapPrm`・`CapEff`・`CapBnd`・`CapAmb` の行を標準出力に書く操作（例: `caps`）を加える。

## 触るファイル

- `crates/benitoiteh/src/sandbox/linux/`（`availability.rs`、`process.rs`、`mounts.rs`、`mounts/tests.rs`）、`sandbox/linux.rs`、`sandbox/text.rs`
- `crates/benitoiteh/Cargo.toml`（`rustix` の機能を足すとき）
- `crates/sandbox-probe/src/`（`main.rs`、`text.rs`）
- `crates/benitoiteh/tests/sandbox_linux.rs`

## テスト

- ユニットテスト（表の形でまとめてよい）: 版の読み取りと比べ方。`bubblewrap 0.12.0`、`bubblewrap 0.8.0`、`bubblewrap 0.10.0`（`0.8.0` より新しい）は使える。`bubblewrap 0.7.9`、`bubblewrap 0.11.0-rc1`、`bubblewrap 0.11`、`bubblewrap 0.11.0.1`、`bwrap 0.11.0`、`bubblewrap  0.11.0`（空白が二つ）、`bubblewrap 0.99999999999.0`、空の出力、二行の出力は使えない。setuid の印の判定は、権限の値を引数に取る形にして、同じ表で確かめてよい。
- `tests/sandbox_linux.rs`:
  - 試験の環境の bubblewrap で、`probe_linux` の `bubblewrap` が `Available` になる（版の値は決め打ちしない。試験の環境は Alpine の配布物の bubblewrap であり、ホストの版とは違う）。
  - サンドボックスの中の `sandbox-probe caps` の出力で、`CapEff`・`CapPrm`・`CapAmb` が `0000000000000000` である。
  - 補助のプロセスの特権の確かめで失敗する場合は、root でない利用者の試験の環境では作れないので、確かめる関数を、特権の集合を引数に取る形にしてユニットテストする。

## 確かめること

- `--cap-drop ALL` を加えた後も、既存の `sandbox_linux` のテストがすべて通ること（オーケストレータが走らせる）。
- 試験の環境の bubblewrap の版（`--version` の出力）を、オーケストレータが記録する。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、`check_bubblewrap` の形と、`prepare` での使い方を書く。

## オーケストレータが行うこと

- x86_64 の機械で、`sandbox_linux` の `caps` のテストを、podman の中の root（`--userns=keep-id` なし。`scripts/remote-linux-run.sh` を通さずに手で起動する）でも一度走らせ、特権が空になることを確かめる。root でない利用者では、`--cap-drop ALL` がなくても空なので、この確かめだけで区別できる。
- 結果を設計書（03-02、BH-094）に書く。
