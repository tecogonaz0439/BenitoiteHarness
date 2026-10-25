# クレートの構成

- 状態: 草稿
- 未決事項: BH-061

## 目的と範囲

本章は、試作のソースコードの構成（ワークスペース、モジュール、スレッド）、依存のクレートの選び方と候補、ビルドの設定、lint と実装の規約を定める。性能の目標と測り方は[性能](06-02-performance.md)で定める。

## 前提

- Benitoite の処理系は、Rust の 1.98.1（`rust-toolchain.toml`）、edition 2024 で書き、`[workspace.lints]` で `unwrap_used`・`expect_used`・`panic`・`indexing_slicing`・`arithmetic_side_effects` などを禁じている（[Benitoite の Cargo.toml](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.7/Cargo.toml)）。依存のクレートの版は `=` で固定し、ライセンスを `cargo-deny` で確かめる（[Benitoite の deny.toml](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.7/deny.toml)）。
- Benitoite の処理系は、HTTP に `ureq`、TLS に `rustls` と `rustls-graviola`、ファイルの操作に `rustix`、JSON に `serde_json`、正規表現に `regex`、SHA-256 に `sha2` を使う（[Benitoite のクレートの Cargo.toml](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.7/crates/benitoite/Cargo.toml)）。
- Codex CLI は約 150 のクレートと約 208 万行の Rust から成り、V8・SQLite・Starlark を含む。aarch64 の macOS 向けのリリースの zstd で圧縮した配布物は 71 MB である（[先行事例](../08-appendix/08-01-prior-art.md)）。試作はこの規模を目指さない。

## 仕様

### ワークスペースとクレート

【方針】ワークスペースに一つのクレート `crates/benitoiteh`（ライブラリと実行ファイル）を置く。Benitoite の処理系と同じ形である。理由: 試作の規模（数万行の見込み）ではクレートを分けてもビルドの時間の利点が小さく、マージを決めるときに、一つのクレートを Benitoite のワークスペースへ移すか、処理系のモジュールとして取り込むかを選びやすい。テストの道具（OS のサンドボックスのテストが起動する小さなプログラム `crates/sandbox-probe`）は、試作の実行ファイルに入らないので、別のクレートとしてワークスペースに置く（[実装プラン](../../plan/10-interfaces.md)）。`crates/benitoiteh/examples/` にある評価・測定・テスト資料の更新の道具も、試作の実行ファイルと配布物には含めない。

### モジュール

【方針】

| モジュール | 役割 | 設計書の章 |
|---|---|---|
| `cli` | 引数の解釈、サブコマンドの振り分け、`__sandbox` の入口 | 05-01 |
| `config` | 利用者とプロジェクトの設定ファイルの読み込みと検証 | 05-01 |
| `agent` | エージェントのループ、イベント、上限、繰り返しの検出 | 01-01 |
| `context` | システムプロンプトと道具の定義の組み立て、環境の情報、指示のファイル、見積もり、圧縮 | 01-04 |
| `provider` | 共通の形、SSE の読み取り、HTTP、三つのアダプタ | 02-01 |
| `tools` | 道具の定義と実行、パスの判定、読まないファイル、出力の切り詰め | 01-03 |
| `benitoite` | 処理系の発見と機能の確かめ、起動、診断の変換、実行の前の検査の報告の読み取り、処理系から得た Skill の文書 | 04-01 |
| `sandbox` | 方針、macOS の Seatbelt、Linux の Landlock と seccomp、拒否の検出 | 03-02 |
| `approval` | 確認の規則と記録 | 03-03 |
| `session` | 記録、再開、巻き戻しの記録、一時ディレクトリ | 01-05 |
| `safe_fs` | 範囲の根のディレクトリの記述子から相対に、範囲の外へ出ない形で開く、ファイルの操作 | 01-03 |
| `http` | HTTP と TLS の設定、ストリーミングの応答の読み取りと打ち切り、名前の解決の差し替え | 02-01、01-03 |
| `ui` | 対話の画面と、`exec` の出力 | 05-01、05-02 |
| `prompts/`（ファイル） | システムプロンプトと道具の説明の英文。`include_str!` で埋め込む | 01-04 |

依存の向きは、`cli` → `ui`・`agent` → `context`・`provider`・`tools`・`approval`・`session` → `benitoite`・`sandbox` → `config`・`safe_fs`・`http` とし、逆向きに依存しない。`safe_fs` と `http` は、主のプロセスの範囲を限ったファイルの操作と HTTP の通信を、それぞれ一か所にまとめるために分けた（理由: 道具、巻き戻し、`web_fetch` が同じ規則を別々に書くと、規則が食い違いうる）。`agent` は `ui` を知らず、イベントを送るだけとする。

### スレッドと非同期

【方針】非同期の実行時（tokio など）を使わず、OS のスレッドと `std::sync::mpsc` のチャネルで組む。

| スレッド | 働き |
|---|---|
| 主のスレッド | 画面の描画と、端末の入力の読み取り |
| エージェントのスレッド | エージェントのループ。主のスレッドから依頼・操縦・取り消し・確認の答えを受け、イベントを送る |
| ストリームのスレッド | 要求ごとに一つ。HTTP の応答を読み、ストリーミングのイベントをエージェントのスレッドへ送る |
| 道具のスレッド | 並べて実行する区間ごとに、`std::thread::scope` で必要な数だけ作る |
| 子プロセスの読み取りのスレッド | 子プロセスの標準出力と標準エラー出力を読む |

理由: 試作が同時に扱う入出力は、一つの HTTP のストリームと、数個の子プロセスと、端末だけである。この数なら、スレッドで書いても資源の消費は小さい。非同期の実行時と、それに合わせた HTTP のクライアント（`reqwest` など）を加えると、実行ファイルが大きくなる。Codex CLI は tokio の上に作り、巨大な future のために再帰の上限とスタックの大きさを上げている（[先行事例](../08-appendix/08-01-prior-art.md)）。

取り消しは、エージェントのスレッドが持つ取り消しの印（原子的な真偽値）と、子プロセスへのシグナルで行う。HTTP の受け取りは、ソケットを閉じて打ち切る（[提供者の層](../02-providers/02-01-provider-layer.md)の「HTTP の通信」）。

TLS の暗号の実装 `graviola`（0.4.1）は、x86_64 では CPU の ADX の命令を要し、持たない CPU では panic する（`graviola-0.4.1/src/low/x86_64/cpu.rs`）。Benitoite の処理系も同じ実装を使う。Apple の `container` の Rosetta は ADX を持たないので、x86_64 の TLS のテストはそこで走らせられない（[実装プラン](../../plan/00-common.md)の「検査」）。

ストリームのスレッドからエージェントのスレッドへのチャネルと、子プロセスの読み取りのスレッドからのチャネルは、長さに上限のあるもの（`std::sync::mpsc::sync_channel`）とする。理由: 受け取る側が遅れたときに、送る側を待たせて、メモリに溜まる量を限る。

### 依存の基準

【方針】依存のクレートは次の基準で選ぶ。

1. Benitoite の処理系がすでに使うクレートを、同じ機能に使えるなら優先する。理由: マージのときに依存が増えない。
2. 既定の機能（default features）を切り、要る機能だけを有効にする。
3. ライセンスは Benitoite の `deny.toml` の許可の一覧（MIT、Apache-2.0、ISC、BSD-3-Clause、Apache-2.0 WITH LLVM-exception、Zlib、Unicode-3.0）に入るものに限る。
4. C のコードをビルドするクレートを避ける。理由: ビルドの環境を増やさず、クロスビルドを簡単に保つ。
5. 加える前と後で、リリースのビルドの実行ファイルの大きさを測り、記録する（[性能](06-02-performance.md)の「測り方」）。

【方針】本プロジェクトでは、依存のクレートのライセンスと勧告を `cargo-deny` で確かめる（[テスト](../07-quality/07-01-testing.md)の「完了の前に通す検査」）。

### 依存の候補

【方針】次のクレートを候補とする。版は実装プランで、その時点の最新の安定版から選んで固定する。

| 用途 | クレート | 備考 |
|---|---|---|
| HTTP | `ureq` | Benitoite と同じ |
| TLS | `rustls`、`rustls-graviola`、`rustls-platform-verifier` | Benitoite と同じ組み合わせ |
| JSON | `serde`（または `serde_core`）、`serde_json` | |
| TOML | `toml` | 既定の機能を切る |
| 端末 | `crossterm` | |
| 文字の幅 | `unicode-width` | 画面の描画 |
| 差分 | `similar` | `write`・`edit` の差分の表示 |
| ファイルの走査と `.gitignore` | `ignore`（`globset` を含む） | `glob`・`grep`・巻き戻しの記録 |
| 正規表現 | `regex` | Benitoite と同じ |
| SHA-256 | `sha2` | 巻き戻しの記録。Benitoite と同じ |
| OS の呼び出し | `rustix` | プロセスグループ、シグナル、`no_new_privs`。Benitoite と同じ |
| 乱数 | `getrandom` | セッションの ID。Benitoite と同じ |
| Landlock | `landlock` | Linux だけ |
| seccomp | `seccompiler` | Linux だけ。Codex CLI と同じ |
| 画像を縮める | `image`（既定の機能を切り、`png` と `jpeg` だけ） | `read` で上限を超える PNG と JPEG を縮める（[道具](../01-agent/01-03-tools.md)の「画像のファイルの読み取り」） |

次のものは使わない。`tokio` と非同期の HTTP のクライアント（前述）、`clap`（[CLI と設定ファイル](../05-interface/05-01-cli.md)の「コマンドラインの形」）、`ratatui`（[対話の画面](../05-interface/05-02-tui.md)）、トークナイザ（[文脈の管理](../01-agent/01-04-context.md)の「文脈の大きさの見積もり」）、HTML を文字に変えるクレート（`web_fetch` の変換は、タグを除いて段落と見出しと箇条書きと `pre` を残す自前の小さな変換で行う）、日時のクレート（記録の時刻は UTC だけを使い、自前で整形する）。

### ビルドの設定

【決定】リリースのビルドは次の設定とする（`opt-level` は 2026-10-19 に設計者が決めた。BH-060 を閉じた）。

```toml
[profile.release]
opt-level = "s"
lto = "fat"
codegen-units = 1
panic = "abort"
strip = "symbols"
```

- `panic = "abort"` にするのは、巻き戻しの処理のコードを除いて実行ファイルを小さくするためである。Benitoite の処理系は panic を境界で受け止めるので `panic = "unwind"` を使うが、試作は panic を不具合として扱い、受け止めない。panic のときは、panic hook で端末の状態（入力の形、カーソル）を元に戻し、セッションの記録に一行を書いてから終える。
- `opt-level` を `"s"`（大きさの優先）にするのは、2026-10-19 の P5-05 の測定（[結果](../../plan/results/P5-perf.md)）で、`3` と比べて実行ファイルが 18% 小さく（5.72 MiB から 4.68 MiB）、時間とメモリの差は `grep` の繰り返し（約 9% 遅い。それでも目標の 1 秒の約 1/5）を除いてゆらぎの範囲だったからである。Linux の x86_64 向けは `3` で 7.74 MiB であり、大きさの目標（8 MiB）に余裕を残す。

### lint と実装の規約

【方針】`[workspace.lints]` は Benitoite の処理系と同じ項目を禁じ、加えて `unsafe_code` を `forbid` にする。理由: 試作は OS のサンドボックスを補助のプロセスで掛けるので `unsafe` が要らない（[OS のサンドボックス](../03-safety/03-02-sandbox.md)の「サンドボックスの掛け方」）。依存のクレートの中の `unsafe` はこの規則の外である。

Linux の名前空間は、利用者が導入した bubblewrap が作るので、試作のコードは名前空間を作る関数を呼ばない（[OS のサンドボックス](../03-safety/03-02-sandbox.md)の「サンドボックスの掛け方」）。名前空間を作る `rustix` の関数は `unsafe` である（`unshare` は 1.1.0 から非推奨とされ、`unsafe` の `unshare_unsafe` に置き換えられた）。`unsafe` が要る場面が生じたら、作業を止めて設計者に例外を求める（`AGENTS.md` の「実装の規約」）。`unsafe` の関数を、規則を満たすためだけに、別のクレートの安全な形の関数に置き換えることはしない。

実装の規約（panic を使わない、文言の置き場所、コメントの書き方など）は、リポジトリの `AGENTS.md` の「実装の規約」に従う。

### 対象の環境

【方針】試作のビルドの対象は、`aarch64-apple-darwin`、`x86_64-unknown-linux-musl`、`aarch64-unknown-linux-musl` とする。Linux 向けを musl の静的リンクにするのは、Benitoite の処理系の対象と揃え、配るときに C の標準ライブラリの版に依存しないためである。試作の間、配布物（ビルド済みの実行ファイル）は作らない。配るかは試作の後に決める（BH-061）。
