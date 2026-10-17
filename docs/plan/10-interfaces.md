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
scripts/remote-linux-run.sh   x86_64 の Linux 向けのテストの実行ファイルを、ssh で届く機械の podman で走らせる
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

P0-03 で作った主な公開の関数は次のとおりである（`Uri`・`HeaderMap`・`HeaderValue` は `http` から再公開）。`HttpClient::new(HttpTimeouts)`、`get(uri, headers, limit) -> HttpResponse`、`get_resolved(uri, headers, addresses, limit)`、`post_json(uri, headers, json) -> HttpStream`。`HttpStream` は `poll(wait) -> StreamEvent`（`Headers`・`Data`・`Finished`・`Pending`）、`cancellation()`、`last_received()`、`check_timeouts()`、`join()` を持つ。データが届かない時間の見張りは、呼び出す側が `poll` か `check_timeouts` で行う。HTTP の状態の誤りは、状態、`Retry-After`、応答のヘッダ、本文の先頭 64 KiB を保つ。


- `HttpClient`: 接続を使い回す HTTP のクライアント。TLS の設定（`rustls` と `rustls-graviola`、OS の証明書の検証）を持つ。
- ストリーミングの要求は、応答の本文を別のスレッドで読み、受け取ったバイト列を長さに上限のあるチャネルで渡す。打ち切りの印（取り消し）と、最後にデータを受け取った時刻を、呼び出した側が見られるようにする。
- 時間の上限: 接続 10 秒、ヘッダまで 120 秒、データが届かない時間 300 秒、全体 30 分。値は呼び出す側が変えられる（テストで短くするため）。
- 名前の解決の差し替え: 判定済みのアドレスだけに接続する形（`web_fetch` のため）。

### `sandbox`（P0-04、P0-05、P0-07）

P0-04 で作った型: 方針の入力 `PolicyConfig`（`readable: ReadAccess`（`All`・`Roots`）、`denied_read`、`writable`、`protected_names`、`allow_ip`、`programs: ProgramAccess`（`Unrestricted`・`Only`）、`deny_exec_in_writable`（既定は真））と、検証済みの `SandboxPolicy::new(config)`。`SandboxPolicy::seatbelt_profile()`、`SandboxCommand::to_command(&self, helper: &Path) -> Result<Command, SandboxError>`。


- `SandboxPolicy`: 方針の要素（[OS のサンドボックス](../design/03-safety/03-02-sandbox.md)の「方針の要素」）を持つ値。読める範囲、読ませない場所、書ける場所、書ける場所の中の守る名前、IP の通信の可否、起動できるプログラム（制限しない／列挙した実のパスだけ）、書ける場所のファイルの実行の拒否。JSON にでき、`__sandbox` の引数として渡せる。パスはすべて実の絶対のパスとする。
- `SandboxCommand`: 方針、プログラム、引数、作業ディレクトリ、環境変数、標準入力の扱い。OS ごとの起動の形（macOS の `sandbox-exec`、Linux の `__sandbox`）に変えて `std::process::Command` を作る。`std::process::Command` を作る関数は、Linux の補助のプロセス（`benitoiteh` の実行ファイル）のパスを引数に取る（例: `to_command(&self, helper: &Path)`。macOS では使わない）。P0-04 の時点の Linux では、掛けられないという誤りを返す。
- P0-07 で、Linux の起動の形を次のとおりにした（Linux だけに公開する。macOS の `to_command` は変えていない。Linux の `to_command` は除いた）。
  - `SandboxCommand::prepare(&self, helper: &Path, scratch: &Path, placeholders: &Path) -> Result<PreparedSandbox, SandboxError>`（P2-03 で `placeholders` を加えた。呼び出す側は `data_dir.join("placeholders")` を渡す。書ける場所の外の、権限 700 のディレクトリでなければ `InvalidPolicy`）。`/usr/bin/bwrap` の引数を作る。`scratch` は、読ませない場所のファイルを覆う権限 000 の空のファイルを置く、書ける場所の外のディレクトリである。補助のプロセスの実行ファイルが書ける場所の中にあれば `InvalidPolicy` を返す。
  - `PreparedSandbox`: `get_program`・`get_args`（別のプログラムで包むため）、`spawn`・`spawn_wrapped(wrapper: Command)`・`output`・`output_wrapped`。
  - `SandboxChild`: `child_mut`、`wait`、`wait_with_output`。塞ぐために作らせた空のディレクトリと覆いのファイルは、子の終了を確かめた後にだけ片付ける。`Drop` では片付けない。
  - `probe_linux(helper: &Path, scratch: &Path) -> Result<LinuxAvailability, SandboxError>`。`LinuxAvailability` は `bubblewrap`・`user_namespaces`・`landlock_v3`・`seccomp` の欄を持ち、値は `CapabilityStatus`（`Available`・`Unavailable(String)`・`NotChecked`）である。
  - 方針の検査は、`protected_names` に綴りどおりの `.git` と `.benitoiteh` を含むことを求める。
  - P2-04 で、`check_bubblewrap() -> CapabilityStatus`（setuid の印がなく、版が 0.8.0 以上か。プロセスの中で一度だけ求める）を加えた。`probe_linux` と `prepare` の両方が使い、`prepare` は使えなければ `SandboxError::Unavailable` を返す。
- `SandboxPolicy` の中のパスは正規の形（`std::fs::canonicalize` の結果。macOS の `/var` は `/private/var`）とし、作るときに確かめる。理由: Seatbelt は正規の形のパスで照合するので、`/var/folders` の形では規則が当たらない。
- `SandboxError`: 方針の誤り、掛けられない、補助のプロセスの段階ごとの失敗。
- P2-01 で、子プロセスの実行の部分 `benitoiteh::sandbox::run` を加えた。道具が子プロセスを動かすときは、この部分を通す。
  - `run(command: impl Into<RunCommand>, input: Vec<u8>, stdout: Arc<Mutex<O>>, stderr: Arc<Mutex<E>>, limits: RunLimits, cancelled: &AtomicBool) -> Result<RunOutcome, RunError>`（`O`・`E` は `Write + Send + 'static`）。`RunCommand` は `Command`（macOS の `sandbox-exec` で包んだもの、またはサンドボックスなし）か、Linux の `PreparedSandbox`。
  - `RunLimits { timeout, stop_signal: StopSignal（Interrupt・Terminate）, grace（既定 10 秒）, pipe_close_wait（既定 2 秒） }`、`RunOutcome { status, end: RunEnd（Exited・TimedOut・Cancelled）, leftovers: Leftovers（None・PossiblyRemaining） }`、`RunError`（`Io`・`Sandbox`）。
  - `run` に `PreparedSandbox` を渡すときは、入力があれば `SandboxStdin::Piped`、なければ `Null` で準備する。Linux の bubblewrap の標準入力は、補助のプロセスへの制御のソケットである。
  - Linux の `SandboxChild` に `take_stdin`・`stop(StopSignal)`・`terminate` を加えた。補助のプロセスは bubblewrap の名前空間の pid 1 として動く（`--as-pid-1`）。

### 通常モードの起動の準備（P3-02）

- `sandbox::normal_policy(sandbox: Sandbox, locations: &WritableLocations, network: bool, denied_candidates: &[PathBuf]) -> Result<Option<SandboxPolicy>, SandboxError>`。`danger-full-access` は `None`。ない読ませない場所の候補は入れない。
- `sandbox::check_availability(helper: &Path, scratch: &Path) -> Result<(), SandboxError>`。macOS は `/usr/bin/sandbox-exec` の有無、Linux は `probe_linux` の四つの欄。
- `config::PROVIDER_KEY_ENV_VARS` と `config::child_environment(inherited, referenced_keys, temporary, sandbox: Option<(&str, bool)>) -> BTreeMap<OsString, OsString>`。子プロセスの環境は、どの起動の形でもこの関数で作る。
- `config::writable::validate_writable_with_executable(...)`。書ける場所が `benitoiteh` の実行ファイルを含めば `WritableRule::Executable` で拒む。
- `tools::ShellContext`（子プロセスの方針と名前、補助のプロセスのパス、Linux の一時の場所と `placeholders`、子プロセスの環境、承認の方針）。`ToolContext` に足して道具へ渡す。

### 承認の方針（P3-04）

- `approval::ApprovalKind::{WriteScope, Command, Escalate}`、`ApprovalFacts { command, cwd, path, sandbox_policy }`（どれも `Option`）、`ApprovalPolicy { approval: Approval, sandbox: Sandbox, sandboxed: bool }`、`Decision::{Allow, Ask, Deny}`。
- `approval::decide(policy: &ApprovalPolicy, kind: ApprovalKind, facts: &ApprovalFacts, allowed: &BTreeSet<String>) -> Decision`。03-03 の表のとおり。
- `Approver::approve(&mut self, request: &ApprovalRecord) -> ApprovalAnswer`。`ApprovalAnswer { outcome: ApprovalOutcome, message: Option<String> }`。`ApprovalOutcome` は `Approved`・`ApprovedForSession`・`Denied`・`DeniedWithMessage`・`Cancelled`。
- `Agent::with_approvals(self, Box<dyn Approver>, ApprovalPolicy) -> Self`。このセッションで許したコマンドの集合は `Agent` が持つ。
- `tools::ApprovalRequest { kind, facts, justification, summary }` を `ToolOutput::needs_approval(request)` で返すと、ループが判定して、許せば `ApprovalStatus::Approved` で実行し直す。確認しうる道具は `parallel: false` にする。
- `ShellContext` に `writable_roots`（選んだ方針の書ける場所の実のパス）、`sandbox`、`sandboxed` を加えた。
- 方針による拒否は `approval::policy_denial_message(kind)`、利用者による拒否は `approval::user_denial_message(kind, message)`。集計の `approvals.policy_denied`。

### 道具 `bash`（P3-05）

- `tools::Invocation<'a> { call_id: &'a str, cancelled: &'a AtomicBool }`。`ToolSet::execute_validated(&self, context, call, approval, invocation: Invocation<'_>) -> ToolOutput`。`ToolSet::execute` は立たない印と `direct-<n>` の ID を渡す。
- `ToolContext::with_session(self, Session) -> Self`。道具が `spill/` に書くときに使う。
- `ToolOutput.spill: Option<SpillInfo>`（`SpillInfo` は `Eq`）。ループが `Record::ToolResult` の `spill` に入れる。`SpillWriter::discard(self)` は書き手を閉じてファイルを消す。
- `Agent` は、`ShellContext` があれば、判定に使う `sandbox` と `sandboxed`（`ShellContext.policy` が `Some` か）を `ShellContext` から取る。
- 道具の一覧は `definitions(mode)` で作り、`text::unknown_tool` はその名前を並べる。

### 圧縮（P4-01）

- `Agent::compact(instructions, cancelled, events) -> Result<CompactReport, AgentError>` と `Agent::compact_with_options(CompactOptions { reason, instructions, keep_recent }, ...)`。`CompactReport { outcome: CompactOutcome::{Compacted, NothingToCompact, Cancelled, Failed}, usage, estimate_before, estimate_after, summarized_items, kept_items }`。
- `session::CompactionReason::{Threshold, Manual, Overflow}`（`agent` からも再公開）。`Record::Compaction { conversation, reason, summarized_items, kept_items }`、`AgentEvent::Compaction { reason, summarized_items, kept_items, estimate_before, estimate_after }`、`Summary.compactions`、`Summary::add_usage`。
- `UserPartKind::Summary`、`AgentError::Compaction`（`kind` は `compaction`）。
- `session::FileHistory { read, modified }`（`add_call`、`from_records`）。
- 推論を除く処理は `agent/compaction/conversation.rs` にある（P4-02 で共有の関数に切り出す）。

### 再開とセッションの一覧（P4-02）

- `Session::open(paths, id)`（錠を取り、失敗してもディレクトリに触れない）。切れた最後の行は最初の追記の直前に切り詰める。`session::ResumeState`（会話、補う結果、番の番号の最大、最後のモード・提供者・モデル・プロジェクト、推論の区間、`FileHistory`）。
- `Agent::resume(...)`。`Record::SessionStart`・`AgentEvent::SessionStart` に `resumed`・`base_url`・ハッシュの欄。
- 推論を除く共有の関数は `agent/reasoning.rs`。`CompactOptions` に `separate_request`、`CompactionReason::Resume`。
- 書ける量: `Session::reserve(bytes) -> bool`・`release(bytes)`（`spill/` と `objects/` で共有）。`session::storage_usage`。
- `benitoiteh sessions [--project <dir>] [--json]`。期間は記録の更新の時刻で判定する。

### 巻き戻し（P4-03）

- `safe_fs` のリンクを一切辿らない口（Linux は `openat2` の `RESOLVE_NO_SYMLINKS|BENEATH`、macOS は `O_NOFOLLOW_ANY`（`0x20000000`。`O_NOFOLLOW` と同時に使わない））。起動のときの自己の確かめ。
- `rewind` のモジュール: 時点の記録（`CheckpointLimits`、`CheckpointStats`）と戻す処理。`Record::Checkpoint(Checkpoint { id, reason: CheckpointReason, turn, call_id, created_at, items, generation, manifest, incomplete })`、`Record::WriteTarget { call_id, real_path }`。
- `Agent::checkpoints()`、`set_checkpoint_limits`、`checkpoint_stats`、`rewind(checkpoint, scope: RewindScope, allow_incomplete, events) -> RewindReport { restored, removed, created, failures, irreversible, before_rewind, conversation_rewound, changed_paths }`。
- 中身と一覧は `objects/` に SHA-256 を名前として置き、書ける量に数える。一覧は中身だけで、変わらなければ重ねて保存しない。

### `web_fetch`（P4-04）

- `ApprovalKind::WebFetch`、`ApprovalFacts { url, domain }`。`decide` はこのセッションで許したドメインの集合を受け取る。`Invocation.session_domains`。
- `ToolContext::with_web_fetch(WebFetchContext { allowed_domains, client })`、`HttpClient::with_timeouts`。`config::domain` の正規化。
- 戻せないものの一覧に `web_fetch` が入る。

### 安全な辿り方と読み方（P4-09）

- `safe_fs::gitignore`（`.gitignore` を待たない形で 1 MiB まで読み、不正な行を飛ばし BOM を除く）。rewind と `glob`・`grep` が共有する。
- `ToolContext::with_search_limits(SearchLimits { max_entries, max_duration, max_file_bytes })`（既定より広げられない）。`glob`・`grep` に取り消しの印を渡す。
- `grep`・`read` は 64 MiB を超えるファイルを飛ばすか拒む。

### `provider`（P1-01、P1-02）

- 共通の形（`provider::types`、`provider` から再公開）: `Destination { provider, base_url, model }`、`AdapterKind`、`Conversation { items }`、`Item::{UserMessage, AssistantTurn, ToolResult}`、`UserMessage { parts: Vec<UserPart> }`（`UserPartKind::{Environment, Request, Steering}`）、`AssistantTurn { parts, raw_items, finish_reason, usage }`、`AssistantPart::{Text, Reasoning, ToolCall}`、`RawItems { destination, adapter, items }`、`ToolCall { id, provider_id, provider_item_id, name, arguments, raw_arguments }`、`ToolResult { id, provider_id, text, is_error }`、`Usage`（各項目 `Option<u64>` と `raw`）、`FinishReason::{Completed, ToolCalls, OutputLimit, Refusal(String), Error, Cancelled, Disconnected}`、`StreamEvent`、`ProviderError`、`Capabilities`。
- `anthropic-messages`（P1-02）: `AnthropicAdapter::new(destination, capabilities, api_key)`、`build_request(conversation, system, tools, RequestOptions { effort, previous_last_item, disable_tools })`、`stream(&HttpClient, &request) -> AnthropicStream`。`AnthropicStream` は `cancellation()`・`poll(wait) -> Vec<StreamEvent>`・`finish() -> AssistantTurn` を持つ。誤りは `AnthropicError` で、`classification() -> ProviderError` でループの扱いを決める。SSE のイベントを読む部分は `AnthropicAssembler`（`read_event`・`is_finished`・`finish`）。P1-03 のアダプタは同じ形に揃える。
