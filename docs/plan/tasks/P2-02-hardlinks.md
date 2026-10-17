# P2-02 サンドボックスの中のハードリンクの作成（BH-092）

## 目的

サンドボックスの中のコマンドが、書ける場所の外のファイルへのハードリンクを、書ける場所の中に作れるかを、macOS の Seatbelt と Linux の bubblewrap の本物の子プロセスで確かめる（BH-092 の【要検証】）。この作業では、確かめるためのテストを作る。テストはオーケストレータが走らせる。

オーケストレータが 2026-10-17 に macOS 27.0.1 で、`(deny default)` から始めて読み取りを広く許し、書き込みを一つのディレクトリに限った小さな方針で `/bin/ln` を試した。外のファイルへのハードリンクは `Operation not permitted` で拒否され、書ける場所の中どうしのハードリンクは作れた。この作業では、`benitoiteh` の本物の方針で確かめる。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「書く道具のパスの規則」のハードリンクの拒否
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「保証しないこと」の「前からあるハードリンク」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「方針の要素」「Linux のファイルシステムの見え方」
- [未決事項](../../design/open-issues.md)の BH-092
- 既存のもの: `crates/sandbox-probe/`、`tests/sandbox_macos.rs`、`tests/sandbox_linux.rs`（`Fixture`、`config()`、読ませない場所を足す既存のテストの書き方）

## 作るもの

1. `sandbox-probe` に、ハードリンクを作る操作（`hardlink <元> <先>`。`std::fs::hard_link`）を加える。拒否は既存の操作と同じ終了状態（`EPERM`・`EACCES` は 10）で返す。`EXDEV` は、この操作に限って終了状態 14 で返す（12 と 13 は既存の操作が使っている）。ほかの操作の終了状態は変えない。
2. 次のテストを、macOS（`tests/sandbox_macos.rs`）と Linux（`tests/sandbox_linux.rs`）の両方に加える。
   - **準備**: 既存の `Fixture` の書ける場所 `write` を使い、テストの方針に読ませない場所を二つ足す（既定の方針の `denied_read` は空なので、既存のテストと同じく足す）。一つは書ける場所の兄弟のディレクトリ `secret`（中に `secret/key` を置く）、もう一つは兄弟のファイル `secret-file` とする。ほかに、書ける場所の兄弟のファイル `outside`（読める）と、書ける場所の中の `write/.git/config` を、子プロセスの起動の前に作る。
   - **正の対照（先に通す）**: 書ける場所の中で、別のディレクトリのファイルへのハードリンクを作れる（`write/a/f` から `write/b/g`）。理由: 同じディレクトリの中のリンクは、Landlock の `REFER` の権利を要さない。ディレクトリをまたぐリンクで、通常の操作が拒まれないことを確かめる。
   - **負の場合**: 次のハードリンクを、書ける場所の中（`write/link-<名前>`）に作れない。
     - `outside`（書ける場所の外の、読めるファイル）
     - `secret/key`（読ませないディレクトリの中のファイル）
     - `secret-file`（読ませないファイル）
     - `write/.git/config`（書ける場所の中の守る場所）
   - 負の場合では、終了状態に加えて、子プロセスが終わった後にサンドボックスの外から次を確かめる。先の名前がない（`symlink_metadata` が `NotFound`）。元のファイルのリンクの数が 1 のまま（`std::os::unix::fs::MetadataExt::nlink`）。
3. 期待の終了状態は次のとおりとする（オーケストレータの見込み。マウントの配置とカーネルの確かめの順から求めた）。

| 場合 | macOS | Linux |
|---|---|---|
| 正の対照 | 0 | 0 |
| `outside` | 10 | 14（元は `--ro-bind / /`、先は書ける場所の `--bind` で、マウントが違う） |
| `secret/key` | 10 | 10（権限 000 の tmpfs の下を辿れない） |
| `secret-file` | 10 | 14（元は覆いのファイルのマウントになる） |
| `write/.git/config` | 10 | 14（すでにある `.git` は `--ro-bind` の別のマウント） |

   Linux の終了状態は、拒んだ仕組みを表すものとしては扱わない（Landlock も、`REFER` の権利がないときなどに `EXDEV` を返す）。テストのコメントには、見込みの理由として上の配置を書く。

## 触るファイル

- `crates/sandbox-probe/`
- `crates/benitoiteh/tests/sandbox_macos.rs`、`tests/sandbox_linux.rs`

## テスト

「作るもの」の 2 と 3 のとおり。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`sandbox_*` のテストは、Codex の中では走らない）。

## オーケストレータが行うこと

- 手元の macOS と x86_64 の Linux の機械で `sandbox_*` のテストを走らせる。期待と違う終了状態が出たら、取り込む前に原因を調べる。書ける場所の外へのハードリンクが作れた場合は、取り込みを止めて設計者に報告する。
- 結果を設計書（01-03 の「書く道具のパスの規則」、03-01 の「保証しないこと」、BH-092）に書く。
