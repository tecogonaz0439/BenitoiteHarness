# 作業の間の約束

本章は、作業の間で共有するもの（ファイルの配置、モジュールの持ち主、依存のクレートの版、作業をまたいで使う型）を定める。作業は一つずつ順に行うので、後の作業は、前の作業が作った型をソースで読んで使う。本章は、その型の名前と責任を先に決め、別々の作業が似た名前の別の型を作ることを防ぐ。

## ファイルの配置

```text
Cargo.toml                    ワークスペース（[workspace.lints]、[profile.release]）
Cargo.lock
.cargo/config.toml            warnings = "deny"、Linux 向けのリンカと実行の仕方
deny.toml                     ライセンスの許可の一覧
scripts/check.sh              完了の前に通す検査
scripts/container-run.sh      Linux 向けのテストの実行ファイルを container の中で走らせる
crates/benitoiteh/            試作のクレート（ライブラリと実行ファイル）
  src/main.rs                 cli::main を呼ぶだけ
  src/lib.rs                  モジュールの宣言
  src/<モジュール>/mod.rs、text.rs ほか
  prompts/                    LLM に見せる英文（P1 から）
  tests/                      統合テスト。OS のサンドボックスを要するものは tests/sandbox_*.rs
  tests/support/              統合テストが共有する補助（一時ディレクトリ、模擬の提供者など）
crates/sandbox-probe/         サンドボックスのテストが起動する、決まった操作だけをする小さなプログラム（P0-04）
assets/benitoite/v0.0.6/      取り込んだ Skill（手で直さない）
```

`crates/sandbox-probe` は配らない（`publish = false`）。設計書 06-01 の「ワークスペースとクレート」は試作のクレートを一つとするが、これはテストの道具であり、試作の実行ファイルには入らない。`cargo test` はほかのパッケージの実行ファイルを作らないので、`scripts/check.sh` がテストの前に `cargo build -p sandbox-probe` を、ホストと Linux の二つの対象で走らせる。テストの対象の外の操作（ファイルを読む、書く、ソケットにつなぐ、プログラムを起動する）を、引数で指示したとおりに一つ行い、結果を決まった終了状態で返す。

## モジュールの持ち主

[クレートの構成](../design/06-impl/06-01-crate-structure.md)の「モジュール」の表のうち、次の二つは、実装プランで加えたものである。

| モジュール | 役割 | 作る作業 |
|---|---|---|
| `safe_fs` | 範囲の根のディレクトリの記述子から相対に、範囲の外へ出ない形でファイルを開き、読み、書き、名前を付け替え、消す（[道具](../design/01-agent/01-03-tools.md)の「道具の定義の共通の規則」） | P0-02 |
| `http` | HTTP と TLS の設定、ストリーミングの応答の読み取り、打ち切り、データが届かない時間の見張り、名前の解決の差し替え（[提供者の層](../design/02-providers/02-01-provider-layer.md)の「HTTP の通信」） | P0-03 |

`safe_fs` と `http` は、依存の向きで `config` と同じ最下層に置き、ほかのモジュールに依存しない。`provider`（P1）と `tools` の `web_fetch`（P4）が `http` を使い、`tools`・`session`・`config`（書ける場所の検査、P1-04）が `safe_fs` を使う。

P0 で作るモジュールは `cli`（`--version`・`--help`・`__sandbox` の入口だけ）、`safe_fs`、`http`、`sandbox` である。ほかのモジュールは P1 で作る。

## 依存のクレート

版は `=` で固定する。Benitoite の処理系（`v0.0.6`）と同じクレートは同じ版にする。既定の機能を切り、作業が要る機能だけを足す（[クレートの構成](../design/06-impl/06-01-crate-structure.md)の「依存の基準」）。

| クレート | 版 | 使う作業 | 備考 |
|---|---|---|---|
| `ureq` | `=3.4.2` | P0-03 | 機能 `rustls-no-provider`・`platform-verifier`。Benitoite と同じ |
| `rustls` | `=0.23.45` | P0-03 | 機能 `std`・`tls12`。Benitoite と同じ |
| `rustls-graviola` | `=0.4.0` | P0-03 | Benitoite と同じ |
| `rustix` | `=1.1.5` | P0-02 ほか | 機能は要るものだけ（`fs`・`process`・`thread` など）。Benitoite と同じ |
| `serde` | `=1.0.229` | P0-04 ほか | 機能 `std`・`derive` |
| `serde_json` | `=1.0.151` | P0-04 ほか | Benitoite と同じ |
| `getrandom` | `=0.4.3` | P1 | Benitoite と同じ |
| `sha2` | `=0.11.0` | P1 | Benitoite と同じ |
| `regex` | `=1.13.1` | P1 | Benitoite と同じ |
| `signal-hook` | `=0.4.5` | P1 | Benitoite と同じ |
| `toml` | `=1.1.8` | P1 | 既定の機能を切り、`std`・`parse`・`serde` を有効にする |
| `ignore` | `=0.4.33` | P1 | |
| `globset` | `=0.4.20` | P1 | `ignore` が依存する版。`ignore` は `globset` を再公開しないので直接加える |
| `similar` | `=3.2.0` | P1 | 機能 `text` |
| `landlock` | `=0.4.7` | P0-05 | Linux だけ（`[target.'cfg(target_os = "linux")'.dependencies]`） |
| `seccompiler` | `=0.5.0` | P0-05 | Linux だけ |
| `libc` | `=0.2.190` | P0-05 | Linux だけ。seccomp の規則の呼び出しの番号と `AF_*`・`SOCK_*` の定数を得るためだけに使い、`unsafe` の関数を呼ばない。Benitoite の `v0.0.6` の `Cargo.lock` と同じ版 |
| `crossterm` | `=0.29.0` | P0-06（`[dev-dependencies]`）、P4 | |
| `unicode-width` | `=0.2.2` | P0-06（`[dev-dependencies]`）、P4 | |

`rustls-platform-verifier` などを直接使う必要が生じたときは、`ureq` が依存する版（`Cargo.lock` の版）に揃える。これは表にないクレートとして止まらなくてよい。`crossterm`・`unicode-width` は、P0-06 では大きさの測定のためだけに `[dev-dependencies]` に置き、P4 で `[dependencies]` に移す。

## 作業をまたいで使う型

P0 の作業が名前と責任を決め、シグネチャの細部を完了の報告に書く。オーケストレータが、取り込むときに本節を実際のシグネチャに合わせて更新する。

### `safe_fs`（P0-02）

P0-02 で作った主な公開の関数は次のとおりである（誤りはすべて `SafeFsError`）。`ScopeRelPath::new`、`ScopeRoot::open`・`real_path`・`open_dir`・`resolve_path(project, requested)`・`open_read`・`metadata`（`std::fs::Metadata`。リンクを辿らない）・`read_dir`（`DirectoryEntry { name, kind }`）・`write(path, contents, create_parents)`・`write_from`・`create_dir`・`remove_file`・`remove_dir`・`symlink`・`read_link`、開いたファイルの実のパスを得る `resolved_path(&File)`。


- `ScopeRoot`: 範囲の根のディレクトリを開いて保持した記述子と、その実のパス。範囲の中の操作はすべてこの型の関数で行う。
- 開いた記述子から実のパスを得る関数（macOS は `rustix::fs::getpath`、Linux は `/proc/self/fd/<n>` の読み取り）。範囲の中のシンボリックリンクを辿って開いたときに、拡張子と守る場所の判定を、開いたものの実のパスで行うため。
- `ScopeRelPath`: 根からの相対のパス。空の要素、`.`、`..`、絶対のパスを持たないことを、作るときに確かめた値。
- 操作: 読むために開く、メタデータ（リンクを辿らない）、ディレクトリの一覧、同じディレクトリの一時ファイルに書いてから名前を付け替える書き込み、新しいディレクトリの作成、ファイルとシンボリックリンクの削除、シンボリックリンクの作成と読み取り。
- 誤り: `SafeFsError`。範囲の外へ出るパス、途中の要素のシンボリックリンク（macOS で辿れないとき）、OS の誤りを分ける。

### `http`（P0-03）

- `HttpClient`: 接続を使い回す HTTP のクライアント。TLS の設定（`rustls` と `rustls-graviola`、OS の証明書の検証）を持つ。
- ストリーミングの要求は、応答の本文を別のスレッドで読み、受け取ったバイト列を長さに上限のあるチャネルで渡す。打ち切りの印（取り消し）と、最後にデータを受け取った時刻を、呼び出した側が見られるようにする。
- 時間の上限: 接続 10 秒、ヘッダまで 120 秒、データが届かない時間 300 秒、全体 30 分。値は呼び出す側が変えられる（テストで短くするため）。
- 名前の解決の差し替え: 判定済みのアドレスだけに接続する形（`web_fetch` のため）。

### `sandbox`（P0-04、P0-05）

- `SandboxPolicy`: 方針の要素（[OS のサンドボックス](../design/03-safety/03-02-sandbox.md)の「方針の要素」）を持つ値。読める範囲、読ませない場所、書ける場所、書ける場所の中の守る名前、IP の通信の可否、起動できるプログラム（制限しない／列挙した実のパスだけ）、書ける場所のファイルの実行の拒否。JSON にでき、`__sandbox` の引数として渡せる。パスはすべて実の絶対のパスとする。
- `SandboxCommand`: 方針、プログラム、引数、作業ディレクトリ、環境変数、標準入力の扱い。OS ごとの起動の形（macOS の `sandbox-exec`、Linux の `__sandbox`）に変えて `std::process::Command` を作る。`std::process::Command` を作る関数は、Linux の補助のプロセス（`benitoiteh` の実行ファイル）のパスを引数に取る（例: `to_command(&self, helper: &Path)`。macOS では使わない）。P0-04 の時点の Linux では、掛けられないという誤りを返す。
- `SandboxPolicy` の中のパスは正規の形（`std::fs::canonicalize` の結果。macOS の `/var` は `/private/var`）とし、作るときに確かめる。理由: Seatbelt は正規の形のパスで照合するので、`/var/folders` の形では規則が当たらない。
- `SandboxError`: 方針の誤り、掛けられない、補助のプロセスの段階ごとの失敗。
