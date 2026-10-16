# P0-04 サンドボックスの方針の型と macOS の Seatbelt の試作

## 目的

モジュール `sandbox` の土台（方針の型と、方針を掛けて子プロセスを起動する形）を作り、macOS の Seatbelt で、方針が許す操作と拒む操作を、本物の子プロセスで確かめる。P0 の完了条件の「補助のプロセスが、ダミーの静的なプログラムに macOS と Linux の方針を掛け、許す操作と拒む操作（実行、読み書き、外部の Unix IPC、引き継いだファイル記述子）を確かめた」のうち、macOS の側と、両方の OS が使う確かめの道具を作る。

## 読む設計書

- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)（全体。特に「サンドボックスの掛け方」「方針の要素」「制限モードの方針」「通常モードの方針」「子プロセスの環境」）
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「制限モードで保証すること」「通常モードで保証すること」
- [テスト](../../design/07-quality/07-01-testing.md)の「安全の性質のテスト」（この作業で確かめる項目を選ぶため）
- [作業の間の約束](../10-interfaces.md)の「`sandbox`（P0-04、P0-05）」と、`crates/sandbox-probe` の説明

## 作るもの

1. `crates/sandbox-probe/`（ワークスペースの新しい要素、`publish = false`、依存のクレートなし、`[lints] workspace = true`）。引数で指示した操作を一つ行い、結果を終了状態で返す小さなプログラム。
   - 操作: `read <path>`、`write <path>`（作るか置き換える）、`mkdir <path>`、`rename <from> <to>`、`exec <path> [args...]`（起動して終了状態を待つ）、`unix-connect <path>`（ストリームのソケットで接続）、`unix-dgram-send <path>`（接続していないデータグラムのソケットから宛先付きで送る）、`tcp-connect <host:port>`、`socketpair`（`std::os::unix::net::UnixStream::pair`）、`fd-open <n>`（記述子の番号 n が開いているかを、macOS では `/dev/fd/<n>`、Linux では `/proc/self/fd/<n>` のメタデータで調べる）、`spawn-with-fd <path> -- <プログラム> <引数...>`（`path` のファイルを開き、close-on-exec の印を外して（`rustix::io::fcntl_setfd`）、その記述子を開いたまま `<プログラム>` を起動し、記述子の番号を環境変数 `PROBE_FD` で渡して、子の終了状態を返す）。`spawn-with-fd` は、テストのプロセスの中で close-on-exec の印を外すと、並べて走るほかのテストの子にも記述子が渡って結果が揺れるので、別のプロセスでこれを行うためにある。
   - 終了状態: 成功 0、OS が拒否した（`EPERM`・`EACCES`）10、それ以外の失敗 11、使い方の誤り 2。`exec` では、起動を拒否されたら 10、起動できたら子の終了状態が 0 なら 0、それ以外は 12。`fd-open` は、開いていれば 0、閉じていれば 13。
   - 標準エラー出力に、行った操作と `std::io::Error` の種類を一行で書く。
   - `unsafe` を使わない。依存のクレートは `rustix`（10-interfaces の版）だけを使ってよい。
2. `crates/benitoiteh/src/sandbox/`（`mod.rs`、`text.rs`、`policy.rs`、`macos.rs` ほか）。
   - `SandboxPolicy`（10-interfaces の説明のとおり）。`serde` で JSON にでき、読み戻せる。作るときに、パスが正規の形の絶対のパスであることと、書ける場所が空でないこと（`read-only` の一時ディレクトリを含む）を確かめる。
   - `SandboxCommand` から `std::process::Command` を作る関数（10-interfaces のとおり、Linux の補助のプロセスのパスを引数に取る。この作業の時点の Linux では、掛けられないという誤りを返す）。macOS では `/usr/bin/sandbox-exec -p <方針の文> -D<名前>=<値>... -- <プログラム> <引数...>` の形にする（03-02 の「サンドボックスの掛け方」）。パスの値は `-D` で渡し、方針の文に埋め込まない。子プロセスは新しいプロセスグループで起動する（`std::os::unix::process::CommandExt::process_group(0)`）。環境変数は、呼び出した側が与えたものだけにする（`env_clear` の後に設定する）。
   - Seatbelt の方針の文を、`SandboxPolicy` から作る関数。`(deny default)` から始め、次の要素を表す。
     - 読める範囲（全体から読ませない場所を除く）。
     - 書ける場所と、その中の守る名前（`.git`・`.benitoiteh`。まだないときの作成、ほかの名前からの付け替えも拒む）。
     - IP の通信を許すかどうか（許すときも、Unix ドメインのソケットへの接続は許さない。名前の解決に要る宛先は P2 の BH-044 で決めるので、この作業では許さない）。
     - 起動できるプログラム（制限しない／列挙した実のパスだけ）。
     - 書ける場所の下のファイルの実行と、実行できる形でメモリに写すこと（`file-map-executable`）の拒否。
     - プロセスの作成（`process-fork`）、シグナル（同じグループの中）、子プロセスが動くのに要る最小の `mach-lookup` と `sysctl-read` など。要ると確かめたものだけを許し、許したものと、許した理由（何がそれなしで失敗したか）を、方針の文のコメントと完了の報告に書く。
   - Codex CLI の方針の文（<https://github.com/openai/codex> のコミット `a45b8ccb016fb74f63a6bc29e701568e387cbe38`。設計書 08-01 の「先行事例」で読んだ版）を参考に読んでよい。浅い複製を `$TMPDIR` の下に作って読む。方針の文の一部を写すときは、写した元のファイルとコミットを、方針の文のコメントに書く（Apache-2.0 の表示の扱いは完了の報告に書く）。
3. `crates/benitoiteh/tests/sandbox_macos.rs`（`#![cfg(target_os = "macos")]`）。`sandbox-probe` を、各方針で `SandboxCommand` を通して起動し、終了状態で確かめる。`sandbox-probe` の実行ファイルの場所は、テストの実行ファイルの場所（`std::env::current_exe()` の `target/<…>/deps/` の親）から求める。見つからなければ、理由を示してテストを失敗させる（飛ばさない）。`cargo test` はほかのパッケージの実行ファイルを作らないので、`scripts/check.sh` が、テストの前に `cargo build -p sandbox-probe` を走らせる。

## 触るファイル

- ワークスペースの `Cargo.toml`（`members` に `crates/sandbox-probe` を加える）
- `crates/benitoiteh/src/lib.rs`（`pub mod sandbox;`）
- `crates/benitoiteh/Cargo.toml`（`serde`・`serde_json` を加える）
- `scripts/check.sh`（テストの前に `cargo build -p sandbox-probe` をホストと Linux の二つの対象で走らせる。`--inside-codex` で `sandbox_macos` が除かれることの確かめ）

## 確かめること

テスト（`tests/sandbox_macos.rs`）で、少なくとも次のことを確かめる。オーケストレータがサンドボックスの外で走らせる。

1. 読み: 読ませない場所（テストで作った一時ディレクトリを読ませない場所にする）を読めない。ほかの場所は読める。
2. 書き: 書ける場所の中に書ける。外に書けない。書ける場所の中の `.git` に、すでにあるときも、まだないとき（`mkdir`）も、ほかの名前のディレクトリから `rename` して作るときも、書けない。`.git` という名前のファイルも作れない。
3. 実行: 起動できるプログラムを列挙した方針で、列挙した実行ファイル（`sandbox-probe` 自身の写しを書ける場所の外に置いたもの）は起動でき、`/bin/sh` は拒否される。`exec` を通した孫も同じ規則に従う（`sandbox-probe exec <sandbox-probe> exec /bin/sh`）。書ける場所の下に置いた実行ファイルは、列挙していても起動できない。
4. 外部の Unix IPC: サンドボックスの外でテストが立てた Unix ドメインのソケット（ストリームとデータグラム）に、IP の通信を許さない方針でも許す方針でも、接続も宛先付きの送信もできない。`socketpair` は使える。
5. IP の通信: 許さない方針で、テストが立てたループバックの TCP のサーバに接続できない。許す方針で接続できる。
6. 引き継いだ記述子: 標準のライブラリで開いた記述子（close-on-exec の印が付く）が、サンドボックスの中の子で開いていない。macOS の Seatbelt は記述子を閉じないので、`spawn-with-fd` で印を外して渡した記述子は子に残る。macOS では、これを確かめた結果として記録する（親は標準入出力のほかを渡さないことで守る。03-02）。
7. `DYLD_INSERT_LIBRARIES`: 書ける場所に置いた動的ライブラリを、子に読み込ませられないこと。ライブラリは、テストのための小さな Rust の `cdylib` のクレートを `crates/sandbox-probe` の下に置いて作ってよい（作れなければ、`file-map-executable` の拒否が方針の文にあることを確かめ、作れなかった理由を報告に書く）。

Codex の中では `sandbox-exec` を重ねて掛けられないので、テストは書くところまでとし、Codex の中で走らせない（`--inside-codex` で除かれる）。方針の文の誤り（文法の誤り）は、Codex の中でも `sandbox-exec` を起動して得られる誤りの文で一部は分かる見込みがある。分からなければ、オーケストレータが走らせた結果を待つ。

テストの一時ディレクトリは正規の形のパスで方針に渡す。Unix ドメインのソケットのパスは 104 バイトまでなので、ソケットは `/tmp` の下の短い名前のディレクトリに作る。

子プロセスの環境変数（`TMPDIR`・`BENITOITEH_SANDBOX` など。03-02 の「子プロセスの環境」）は、P2 と P3 で処理系と `bash` を起動する作業が設定する。この作業では、`SandboxCommand` が呼び出す側の与えた環境変数だけを渡すことまでを作る。

## テスト

上の「確かめること」の 1〜6 を、`tests/sandbox_macos.rs` の別々のテストとして書く。ほかに、`SandboxPolicy` の JSON の読み書きと、方針の文に `-D` の名前だけが現れパスの文字列が現れないこと（パスに `"` や `)` を含めても方針の文が変わらないこと）を、ユニットテストで確かめる。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが、サンドボックスの外で `scripts/check.sh` を走らせ、`sandbox_macos` が通る。通らなければ、結果を添えて直しを依頼する（`codex exec resume`）。
