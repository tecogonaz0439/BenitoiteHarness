# P4-05 `doctor`（処理系の確かめを除く）

## 目的

`benitoiteh doctor` を作る（設計書 05-01 の「doctor」）。Benitoite の処理系との連携（P2 の処理系の作業）は `v0.0.7` を待っているので、この作業では処理系の項目を「確かめていない」として示し、サンドボックス、設定、提供者のキーの参照、データの置き場所を確かめる。処理系の項目は P2 の処理系の作業で加える。

## 読む設計書

- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「コマンドラインの形」「終了状態」「doctor」（2026-10-17 に、状態の四つの分類、`--json`、`--project`、処理系の代わりに `benitoiteh` 自身の `--version` を使うことを加えた）「設定ファイル」「プロジェクトの設定ファイル」「データの置き場所」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「サンドボックスの掛け方」（起動のときの確かめ、bubblewrap の版と setuid、特権の除去）「通常モードの方針」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「Linux での保証の範囲」「OS のサンドボックスを使えない環境」
- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「認証の情報」
- 既存のもの: `src/sandbox/`（`mod.rs` の `SandboxCommand`・`to_command`（macOS は `sandbox-exec` で直接起動する）、`normal.rs` の `normal_policy`、Linux の `prepare`・`availability.rs` の `probe_linux`・`check_bubblewrap`・`LinuxAvailability`・`classify`・`CapabilityStatus`、`linux.rs` の特権の確かめ、`linux/mounts.rs`、`placeholders.rs`）、`src/config/`（`UserConfig::load`、`ProjectConfig::load(&ScopeRoot) -> ProjectConfigLoad`、`ProviderConfig::resolved_capabilities`、`credentials.rs` の `ApiKeyRef::resolve`・`CredentialError`、`DataPaths`、`writable.rs` の `validate_writable_with_executable`）、`src/cli/`（振り分け、`text.rs` の使い方の文と「not available yet」の文、`exec.rs` の読ませない場所の候補）、`src/session/quota.rs`（`TOTAL_LIMIT`、`storage_usage`）、`src/ui/mod.rs`（`terminal_text`）、`tests/cli.rs`

## この作業が認める型の変更

- `LinuxAvailability` に `capabilities: CapabilityStatus` を加え、`classify` が補助のプロセスの特権の確かめの失敗（`{SETUP} {CAPABILITIES}:`）を見分けるようにする。そのときの後の Landlock と seccomp は `NotChecked` とする。
- bubblewrap の版の文字列を返す公開の関数を加える（例: `bubblewrap_version() -> Result<String, CapabilityStatus>`。`check_bubblewrap` と結果を共有する）。
- `session::quota` の 10 GiB の上限を公開する（例: `pub const TOTAL_LIMIT`）。
- 読ませない場所の候補を作る処理を、`exec` と共有する関数に切り出す。

## 作るもの

### 1. コマンドと終了状態

- `benitoiteh doctor [--project <dir>] [--json]` を使えるようにする。使い方の文の行を `benitoiteh doctor [--project <dir>] [--json]` に、「not available yet」の文を `Interactive mode and --licenses are not available yet.` に改める（P4-06 でさらに改める）。`tests/cli.rs` の、`doctor` の行と `["doctor"]` が使い方の誤りになることの期待を改める。
- 終了状態: 引数の誤り（知らない引数、重複、`--project` の値がない）と、`--project` のディレクトリを開けないときは 2（`sessions` と同じ扱い）。項目に `unavailable` が一つでもあれば 1、なければ 0。出力の失敗は 1、panic は 3。項目の一覧から終了状態を求める処理は純関数にする。
- doctor 自身は通信しない（LLM の提供者に要求を送らない）。キーの参照のコマンドが何をするかは関知しない（2 の「提供者」）。

### 2. 項目

項目は次の順と識別子で示す。状態は `ok`・`unavailable`・`unchecked`・`warning` の四つとする。前の項目が使えないために確かめられない項目は、`unavailable` を重ねて出さず、`unchecked` と理由（例: `requires bubblewrap`）を示す。

- `benitoite`: `unchecked`。処理系の連携を作るまで確かめないことを示す。
- `config.user`: 利用者の設定のファイルを読めるか。ないときは `ok`（既定を使う）。壊れているときは `unavailable` とし、理由（`ConfigError` の Display。設定の値は含まない）を示す。このとき提供者の項目は `unchecked`、通常モードの方針は既定の値で確かめる。`DataPaths::from_env` が失敗したとき（`HOME` がないなど）は、そのことを示し、データの置き場所と設定に依る項目を `unchecked` にする。
- `config.project`: `--project`（既定は作業ディレクトリ）のプロジェクトの設定のファイル。読めないか、書けない欄や知らない欄があれば、ファイル全体を採用しないことを `warning` で示し、理由を添える（05-01 の「プロジェクトの設定ファイル」は欄ごとに無視しない）。
- `provider.<名前>`（設定の名前の順）: アダプタの種類、ベースの URL（scheme・host・port・path だけ。利用者の情報と問い合わせは示さない）、モデル、既定の提供者かの印。`context_window` は `ProviderConfig::resolved_capabilities().context_window` が `None` なら `warning`（番の前の圧縮が働かない。01-04）。キーの参照は値を得られるかを確かめる。`command` の参照は、`exec` と同じく利用者のプログラムをサンドボックスの外で動かす（02-01 の「認証の情報」。時間の上限は `exec` と同じく設けない）。`exec` が既定の提供者のキーだけを解決するのと違い、doctor はすべての提供者のキーを解決する。既定の提供者のキーが得られなければ `unavailable`、ほかの提供者は `warning` とする。提供者が一つもないとき、または `default_provider` が設定にない名前のときは `unavailable` とする（`exec` が起動できないため）。
- macOS のサンドボックス: `sandbox.sandbox_exec`（`/usr/bin/sandbox-exec` があるか）、`sandbox.restricted_rule`（3 の確かめ）。
- Linux のサンドボックス: `sandbox.bubblewrap`（パスと版、0.8.0 以上か、setuid でないか）、`sandbox.user_namespaces`、`sandbox.landlock`（ABI の版 3 以上の規則を適用できたかだけを示す。版の番号の表示は後に回す。`landlock` のクレートは今の版を親のプロセスで得る口を公開しておらず、直接の呼び出しは `unsafe` になるため）、`sandbox.seccomp`、`sandbox.capabilities`（中のプロセスの特権が空か）、`sandbox.restricted_rule`（3。四つの機能と特権がすべて使えるときだけ試し、ほかは `unchecked`）、`sandbox.linux_scope`（守る場所の範囲が macOS より狭いこと。03-01。`warning`）。
- `writable`: 設定の `writable_roots` が書ける場所の検査（`validate_writable_with_executable`）を通るか。
- `sandbox.normal`: 設定の `sandbox`（既定 `workspace-write`）の方針を、`--project` のプロジェクトのディレクトリで作れるか（`normal_policy`）と、掛けられるか（3 の確かめの結果を使う。`check_availability` は使わない。macOS ではファイルの有無しか見ないため）。`writable` が通らなければ `unchecked`。設定が `danger-full-access` なら「設定でサンドボックスなし」を `warning` で示す。作れないか掛けられなければ `unavailable` とし、`--no-sandbox` の説明を添える。
- `data.sessions`・`data.placeholders`（Linux）・`data.history`: ないときは「まだない」で `ok`。あるときは、シンボリックリンクでなく、権限が 0700 で、持ち主が実効の利用者であれば `ok`、違えば `unavailable`。`history/` は `DataPaths` に欄がないので `data_dir` の下の `history` とする（P4-07 で欄を加えたら揃える）。
- `data.storage`: `sessions/` があるときだけ `storage_usage` で合計の大きさを数え、10 GiB の上限に対する割合を示す。超えていれば `warning`。`sessions/` がなければ 0 バイトとし、`storage_usage` を呼ばない（呼ぶとディレクトリを作るため）。

最後に要約の二行を置く: 「制限モードのサンドボックス: 掛けられる／掛けられない（処理系は確かめていない）」「通常モードのサンドボックス: 使える／設定でなし／使えない」。

### 3. 制限モードの規則の確かめ

- doctor 専用の一時ディレクトリ D を、`DataPaths` の一時の根の下に `benitoiteh-doctor-<乱数>` として排他に作る（0700。`canonicalize` する）。`probe_linux` と確かめの `scratch`・`placeholders` は、すべて D の下に置く。終わったら、誤りの道筋でも消す。
- 書ける場所は `D/write` だけとし、プログラムは `current_exe` を `canonicalize` したものだけを許す方針を作る。子の環境は空にする（キーを子に渡さない）。プロジェクトのディレクトリを書ける場所にして子を起動しない（Linux の `prepare` は書ける場所の根に塞ぐためのディレクトリを作るため）。データのディレクトリの `placeholders/` を使わない。
- 正の確かめ: 方針の中で `benitoiteh --version` を起動し、終了状態 0 で、標準出力が版の文と一致すること。負の確かめ: 方針の中で `/bin/sh -c 'printf benitoiteh-doctor-shell-ran'` を起動する。子プロセスは `sandbox::run` で、10 秒の時間の上限を付けて動かす。
- macOS: `SandboxCommand::to_command` の結果を実行する。正の確かめが通り、負の確かめの終了状態が 0 でなく、標準出力に印の文字列がないときだけ「拒んだ」とする（手元の Darwin 27 では、`sandbox-exec` が終了状態 71 で終わり、`execvp() of '/bin/sh' failed: Operation not permitted` を書く）。`sandbox-exec` を重ねて掛けられない環境（Codex の中など）では、正の確かめが失敗するので `unavailable` とし、`/bin/sh` を拒んだとは示さない。
- Linux: `prepare(helper, D/scratch, D/placeholders)`（`D/placeholders` は 0700）を使う。終了状態 125 で、標準エラー出力が `"{SETUP} {EXEC}:"` で始まり、`os error 13` を含むときだけ「拒んだ」とする。`/bin/sh` が外に存在しないときは `unchecked` とする（存在しないことの ENOENT を拒否と取り違えない）。動的にリンクした `benitoiteh` では正の確かめが Landlock に拒まれるので、その理由を示す。
- `/bin/sh` を一つ拒めたことだけで `ok` にしない（05-01）。正の確かめと、Linux では四つの機能と特権のすべてが要る。
- doctor は `session::cleanup` を呼ばず、`Session` を作らない。

### 4. 秘密を出さない

- キーの参照の誤りは、`CredentialError` の Display を使わずに、種類だけを示す。`Environment` は「環境変数 `<名前>` がないか空」、`Spawn` は「起動できない（`io::ErrorKind`）」、`Command` は「終了状態 N」（標準エラー出力を示さない）、`Output` は「一行目が使えない」とする。コマンドの引数の並びも示さない。得たキーは、値も長さも示さずに直ちに捨てる。
- 説明に含まれるパスや bubblewrap の診断などの文字列は、`ui::terminal_text` で制御の文字を目に見える形に変えて示す。設定のファイルの中身は示さない。

### 5. 表示

- 文字の表示は、項目ごとに `[<状態>] <識別子>: <説明>` の一行とし、最後に要約の二行を置く。英文は `cli` の `text`（か doctor のモジュールの `text`）に置く。
- `--json` では、すべて標準出力に、項目ごとに `{"type":"item","name":...,"status":"ok|unavailable|unchecked|warning","detail":...}`（提供者の行には `"default":bool` を足す）を、最後に `{"type":"summary","restricted_sandbox":bool,"normal_sandbox":"enabled|disabled_by_config|unavailable","exit_status":n}` を一行ずつ書く。JSON の欄は利用者に約束しない（BH-012 と同じ扱い）。

## 触るファイル

- `crates/benitoiteh/src/cli/`（`doctor` のための新しいモジュール、振り分け、`text.rs`、読ませない場所の候補の共有）
- `crates/benitoiteh/src/sandbox/`（`LinuxAvailability` の `capabilities`、`classify`、bubblewrap の版の関数）
- `crates/benitoiteh/src/config/`（要れば）
- `crates/benitoiteh/src/session/quota.rs`（上限の公開）
- `crates/benitoiteh/tests/cli.rs`、新しい `tests/doctor.rs`、`tests/sandbox_macos.rs`、`tests/sandbox_linux.rs`

## テスト

doctor を起動するテストは、`HOME`、`XDG_CONFIG_HOME`、`XDG_STATE_HOME`、`TMPDIR` を、テストごとの一時ディレクトリに必ず向ける（手元の利用者の設定のキーの参照のコマンドを動かさないため）。`HOME` とプロジェクトのディレクトリを分け、書ける場所の検査に当たらない配置にする。環境変数のキーの参照には、テストだけの変数名を使い、`env_remove` する。「読めない設定ファイル」は権限ではなく壊れた TOML で作る（aarch64 のコンテナは root で動く）。

- 終了状態を求める純関数の単体テスト: `unchecked` と `warning` だけなら 0、`unavailable` があれば 1。
- `tests/doctor.rs`（環境に依らない性質だけ）:
  - 壊れた利用者の設定で `config.user` が `unavailable`、終了状態 1、提供者の項目が `unchecked`。
  - 引数の誤りと、開けない `--project` が終了状態 2。
  - `context_window` のない提供者に `warning`（`capabilities` の欄に書いたものも見る）。
  - 既定の提供者のキーの環境変数がないと `unavailable`、ほかの提供者は `warning`。
  - 秘密を出さない（安全の性質のテスト。07-01）: 標準出力にも標準エラー出力にも、キーの値、キーの参照のコマンドの標準エラー出力に書かせた目印、コマンドの引数に入れた目印、ベースの URL の利用者の情報と問い合わせが出ない。
  - `--json` の各行の形と、最後の要約の行。
  - `sessions/` がないとき、doctor が状態のディレクトリを作らない。doctor の一時ディレクトリが終わった後に残らない。プロジェクトのディレクトリに何も作らない。
  - 処理系の項目が `unchecked`。
- `tests/sandbox_macos.rs`・`tests/sandbox_linux.rs`（オーケストレータが手元で走らせる。`sandbox_linux.rs` は x86_64 でだけ走る）: サンドボックスの項目が `ok`、制限モードのサンドボックスを掛けられると示し、ほかがすべて `ok` の設定で終了状態 0。Linux で `sandbox.linux_scope` の `warning` があっても終了状態 0。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、文字の出力の例（macOS と Linux で想定されるもの）と JSON の例、足した英文の全文を書く。
