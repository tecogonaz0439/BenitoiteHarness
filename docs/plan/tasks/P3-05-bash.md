# P3-05 通常モードの道具 `bash`

## 目的

通常モードの道具 `bash` を作る（設計書 01-03 の「bash（通常モード）」）。P2-01 の実行の部分、P3-02 の起動の準備（方針、`ShellContext`、子プロセスの環境）、P3-04 の承認の方針を使う。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「bash（通常モード）」「出力の切り詰め」「モードごとの道具の一覧」「道具の定義の共通の規則」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「拒否の検出」「子プロセスの環境」「子プロセスの後始末」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「通常モードの承認の方針」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「保証しないこと」（サンドボックスの外で動くコマンドと秘密）
- 既存のもの: `src/sandbox/run.rs`、`src/sandbox/`（macOS の `SandboxCommand::to_command`、Linux の `SandboxCommand::prepare`、`linux.rs` の `SETUP_FAILURE`）、`src/config/environment.rs`、`src/tools/`（`ToolSet`、`ToolContext`、`ShellContext`、`ToolOutput`、`definitions.rs`、`truncate.rs`、`text.rs`、`write_path.rs` の確認の要求の作り方）、`src/approval/`（P3-04）、`src/agent/`（`mod.rs` の `Agent::new`・`with_approvals`、`execution.rs` の道具の実行と確認）、`src/session/`（`Session::spill_writer`・`directory`、`storage.rs` の `SpillWriter`、`SpillInfo`、`Record::ToolResult`）、`src/cli/exec.rs`、`prompts/tools/`、`tests/expected/`

## 作るもの

### 1. 道具の定義

- `src/tools/bash.rs`（名前は任せる）に作り、通常モードの `ToolSet` にだけ加える。順は 01-03 の「モードごとの道具の一覧」のとおりとする。`parallel` は偽とする（確認を扱う道具は並べて実行しない。P3-04）。
- 引数は 01-03 の JSON のとおり（`command`、`timeout`、`escalate`、`justification`）。`timeout` は秒で、`1..=600` の整数、既定 120。範囲外は道具の誤りとする。`tools/text.rs` の引数の説明に四つの引数を加える。
- `text::unknown_tool` が並べる道具の名前を、モードごとの道具の一覧から作る形にする（通常モードでは `bash` を含む）。
- 道具の説明の英文を `prompts/tools/bash.md` に置く（一行の説明と 0〜3 行の規則。01-03 の英文があればそれを使い、ほかは短く書く。オーケストレータが取り込む前に文面を見直す）。

### 2. 道具の実行に渡す値

- `ToolSet::execute_validated` に、呼び出しの ID と取り消しの印を渡す。形は次のとおりとする（既存の三つの引数は変えない）。

  ```rust
  pub struct Invocation<'a> {
      pub call_id: &'a str,
      pub cancelled: &'a AtomicBool,
  }
  pub fn execute_validated(&self, context: &ToolContext, call: &ValidatedCall,
                           approval: ApprovalStatus, invocation: Invocation<'_>) -> ToolOutput
  ```

  `agent/execution.rs` は、一つずつ実行する経路と並べて実行する経路の両方で、`ToolCall.id`（`turn-N-call-i`）とループの取り消しの印を渡す。`ToolSet::execute`（道具のテストの入口）は引数を変えず、立たない印と、`ToolSet` の中の数で一意に作る ID（`direct-<n>`）を渡す。`tests/tools_write.rs` の呼び出しも直す。
- `ToolContext` に `session: Option<Session>` を加え、`ToolContext::with_session(self, Session) -> Self` で設定する。`cli/exec.rs` が設定する。
- `ToolOutput` に `spill: Option<SpillInfo>` を加える（`SpillInfo` に `Eq` を加える）。`agent/execution.rs` は、この値を `Record::ToolResult` の `spill` に入れる（今は `None` を入れている）。この二つの型の変更は、この作業の文書が認める。

### 3. 確認

道具は承認の判定（`approval::decide`）を呼ばない。判定はループが行う（道具からは、このセッションで許したコマンドの集合に触れないため）。

- 引数の検査（`timeout` の範囲を含む）を、確認の要求を返す前に行う。範囲外の呼び出しで利用者に尋ねない。
- `ApprovalStatus::Unapproved` で呼ばれたら、方針によらず、何もせずに `ToolOutput::needs_approval` を返す。`ApprovalStatus::Approved` で呼ばれたら、要求を返さずに実行する。
- 種類は、`escalate` が真なら必ず `Escalate`、偽なら `Command` とする。理由: `escalate` が真の呼び出しを `Command` として尋ねると、`untrusted` でこのセッションに許したコマンドが `Allow` になり、確認なしにサンドボックスの外で動く。
- 事実（`ApprovalFacts`）は、`command` を引数の文字列そのまま（整形しない。完全一致の判定に使う）、`cwd` をプロジェクトの実のパス、`sandbox_policy` を選んだ方針の名前（`write_path.rs` と同じ）とする。`justification` は引数をそのまま渡す。`summary` は `tools/text.rs` の関数で作る。
- 承認を受けた `escalate` は、その一回だけサンドボックスを掛けずに動かす。サンドボックスを掛けていないとき（`danger-full-access`、`--no-sandbox`）は、`escalate` は何も変えない。
- `Agent` は、`ToolContext` に `ShellContext` があれば、判定に使う `sandbox` と `sandboxed` を `ShellContext` の値（`sandboxed` は `ShellContext.policy` が `Some` か）とする。`approval` は `with_approvals` で受け取った値のままとする。理由: 判定の「サンドボックスがない」と実行の「サンドボックスを掛ける」が食い違うと、`on-request` の `escalate` が確認なしにサンドボックスの外で動く。

### 4. 起動

- `ToolContext.shell` か `ToolContext.session` が `None` のときは、起動せずに道具の誤りを返す。
- サンドボックスを掛けるかは `ShellContext.policy` で決める（`Some` なら掛ける。ただし承認を受けた `escalate` は掛けない）。`to_command`・`prepare` が失敗したときは、サンドボックスなしで動かさずに、道具の誤りを返す。
- シェルは `/bin/bash`、なければ `/bin/sh` とする（固定のパス。`PATH` から探さない）。サンドボックスを掛けるときは、実のパス（`canonicalize`）を目的のプログラムとする。実のパスのファイルの名前が `bash`・`sh`・`dash` のどれでもないとき（Alpine の `/bin/sh` が `busybox` を指すときなど）は、引数の先頭に `sh` を置いて `<実のパス> sh -c <command>` とする。この形を選ぶ関数を純関数にし、ユニットテストで確かめる。
- 作業ディレクトリはプロジェクトのディレクトリ、標準入力は空（Linux の `SandboxCommand` は `SandboxStdin::Null`、`run` の入力は空）とする。
- 環境は `ShellContext.env` とし、`env_clear` してから渡す。サンドボックスを掛けずに動かすとき、`ShellContext.env` から `BENITOITEH_SANDBOX` と `BENITOITEH_SANDBOX_NETWORK_DISABLED` を除いた写しを使う（承認を受けた `escalate` のとき、この二つが入っている）。理由: サンドボックスの外で動くコマンドに、中で動いていると伝えない（03-02「子プロセスの環境」）。
- サンドボックスを掛けるときは、macOS は `SandboxCommand::to_command`、Linux は `SandboxCommand::prepare(helper, scratch, placeholders)` で起動の形を作る。
- 実行は `sandbox::run::run` で行う。止める合図は `StopSignal::Terminate`、時間の上限は `timeout`、取り消しの印は `Invocation.cancelled` とする。`run` は、道具を実行するスレッドで同期に呼ぶ（Linux の `--die-with-parent` は起動したスレッドに結び付くため、短く終わるスレッドの中で呼ばない）。
- `run` が誤りを返したときは、道具の誤りとし、`spill/` のファイルを消す。

### 5. 出力

- 書き手（`Session::spill_writer(call_id)`）は、確認の後、起動の直前に一度だけ開く（ファイルを `O_EXCL` で作るため、同じ ID で二度開けない）。
- 標準出力と標準エラー出力は、同じ受け取り先（一つの `Arc<Mutex<…>>`）に、届いた順にまとめる。受け取り先の `write` は誤りを返さない（`run` は受け取り先の誤りを実行の誤りにするため）。`spill/` への書き込みが I/O の誤りで失敗したら、書き手を捨てて、それ以降は `spill/` へ書かず、その旨を覚えておく。
- 結果は錠の中で取り出す。`Leftovers::PossiblyRemaining` のとき、出力を読むスレッドが `run` の後も生きているため、取り出すときに書き手を `Option::take` で抜き、後から届いた出力は捨てる。
- 受け取り先は、末尾の 2,000 行か 50 KiB だけをメモリに保ち、全体の行の数を数え、すべてを書き手に書く（16 MiB の上限と末尾の印は、今の書き手が持つ）。
- 行の数え方: 改行で区切る。最後の改行の後に文字があれば、それも一行と数える。返すのは、2,000 行と 50 KiB の両方に収まる末尾の行の範囲とする。最後の一行だけで 50 KiB を超えるときは、その行の末尾の 50 KiB（UTF-8 の文字の途中で切らない）を返し、その行を返した範囲とする。印の行は 50 KiB に含めない。行の番号は 1 から数える。
- 出力は `from_utf8_lossy` で文字列にする。
- 切り詰めたときは、出力の後に次の一行を付ける。`<path>` は `Session::directory()` の実のパスに `SpillInfo.path` を連結した絶対のパスとする（`read` がそのまま読めるように）。
  - 書き手がすべてを書けたとき: `[Showing last lines A-B of N. Full output: <path>]`（01-03 の英文）
  - 16 MiB を超えて書き手が捨てたとき: `[Showing last lines A-B of N. Partial output (first X of Y bytes): <path>]`（X は書いた量、Y は書いた量と捨てた量の和）
  - `spill/` への書き込みが失敗したとき: `[Showing last lines A-B of N. Full output was not saved.]`
- 切り詰めなかったときと、`spill/` への書き込みが失敗したときは、`spill/` のファイルを消し、`ToolOutput.spill` を `None` とする（`SpillWriter` に、閉じて消す関数を足す）。残したときは `SpillWriter::finish` の値を `ToolOutput.spill` に入れる。

### 6. 結果の末尾

出力（と切り詰めの印）の後に、次の順で行を加える。英文は `tools/text.rs` に置く。

1. `[exit code: N]`。合図で終わったときは `N` を 128 に合図の番号を足した値とする（Linux の補助のプロセスも同じ値で終わる）。0 でなければ誤りの印を付ける。
2. 時間の上限で止めたとき: `[The command timed out after N seconds and was stopped.]`。取り消しで止めたとき: `[The command was stopped because the user cancelled the tool call.]`
3. `Leftovers::PossiblyRemaining` のとき: `[Processes started by the command may still be running.]`。画面への表示は P4 で作る。
4. 拒否の検出（03-02 の「拒否の検出」）に当たれば、01-03 の英文の案内（`[sandbox: the command may have been blocked by the sandbox. If it must run outside the sandbox, call bash again with escalate=true and a justification; the user will be asked.]`）を字句を変えずに加える。

拒否の検出は純関数（終了状態、合図、出力の末尾、サンドボックスを掛けたか、OS を受け取る）にする。

- 調べる出力は、メモリに保った末尾とする。文字の比較は ASCII の大文字と小文字を区別しない。
- サンドボックスを掛けていないとき（承認を受けた `escalate` を含む）は、案内を付けない。
- 終了状態が 0・2・126・127 のときは付けない。
- Linux でサンドボックスを掛けたときは、補助のプロセスが合図で終わった子の終了状態を 128 足す番号で返すので、`SIGSYS` は終了状態 159 として現れる。終了状態 159 も `SIGSYS` で終わったものとして扱う。
- Linux の補助のプロセスが準備に失敗したときの終了状態 125（`SETUP_FAILURE`）は、目的のプログラムも返しうるので、特別に扱わない。

### 7. 期待の出力

- `tests/expected/system-normal.txt`、`tools-normal.json` を作り直し、差分が `bash` の分だけであることを確かめる。`system-restricted.txt`、`tools-restricted.json` は変わらないこと。

### 8. 試験のイメージ

- `scripts/remote-linux-run.sh` の x86_64 の試験のイメージに `bash` を入れる。`apk add --no-cache bubblewrap bash` にし、イメージの名前を `localhost/benitoiteh-test:alpine-3.22-bwrap-bash` に改める（スクリプトは `podman image exists` で確かめるので、名前を変えれば作り直される）。
- `docs/plan/00-common.md` の「検査」のイメージの記述を、「`alpine:3.22` に bubblewrap と bash を入れたもの（`localhost/benitoiteh-test:alpine-3.22-bwrap-bash`）」に改める（この一か所だけ変えてよい）。
- aarch64 の `scripts/container-run.sh` は素の `alpine:3.22`（bash がない）で全テストを走らせる。テストのコマンドは POSIX の `sh` で書く。

## 触るファイル

- `crates/benitoiteh/src/tools/`（`bash.rs`（新）、`mod.rs`、`definitions.rs`、`text.rs`、要れば `truncate.rs`）
- `crates/benitoiteh/prompts/tools/bash.md`（新）
- `crates/benitoiteh/src/agent/`（`mod.rs` の判定の方針、`execution.rs` の `Invocation` と `spill` の欄）
- `crates/benitoiteh/src/session/`（`SpillInfo` の `Eq`、書き手を消す関数）
- `crates/benitoiteh/src/cli/exec.rs`（`with_session`）
- `crates/benitoiteh/src/context/`（道具の一覧が変わるとき）
- `crates/benitoiteh/tests/expected/`、`tests/` の道具と `exec` のテスト（`tests/tools_write.rs` を含む）、`tests/agent_loop.rs`、`tests/sandbox_macos.rs`、`tests/sandbox_linux.rs`
- `scripts/remote-linux-run.sh`、`docs/plan/00-common.md`（8 のとおり）

## テスト

**サンドボックスを要さないもの**（Codex の中で走る）

- 出力の末尾の保ち方（`--sandbox danger-full-access` か、`ShellContext.policy` が `None` の文脈で）:
  - 2,000 行を超える出力と 50 KiB を超える出力で、本当の末尾が返り、印の行の番号と全体の行の数が正しい。改行で終わらない出力と、50 KiB を超える一行を含む。
  - `spill/` のファイルに全体があり、印のパスを道具 `read` で読める。
  - 16 MiB を超えたときに `Partial output` の印になる。
  - 切り詰めなかったときに `spill/` のファイルが残らない。
  - `Record::ToolResult` の `spill` が、残したときだけ埋まる。
- 結果の末尾:
  - `[exit code: N]` が 0 のときも付く。
  - 時間の上限で止まったことが結果に出る。
  - 取り消しの印を立てると止まり、そのことが結果に出る。
  - `timeout` の範囲外（0、601）が誤りになり、確認を尋ねない。
- 拒否の検出の純関数: 2・126・127 を除くこと、文字の大小を区別しないこと、Linux でサンドボックスを掛けたときに 159 を `SIGSYS` とみなすこと、サンドボックスを掛けていないときに付けないこと。シェルの起動の形を選ぶ関数（`bash`・`sh`・`dash`・`busybox`）。
- 子プロセスの環境に提供者のキーの変数がない（安全の性質のテスト。07-01）。文脈の環境は `config::child_environment` で作る（キーの変数を入れた親の環境から）。承認を受けた `escalate` で動かしたときもなく、`BENITOITEH_SANDBOX` もない。
- 確認（エージェントのループと台本の `Approver` で。文脈は、`ShellContext.policy` を `Some`（検証した方針）、`sandbox` を `workspace-write`、`sandboxed` を真とする。Codex の中ではサンドボックスを掛けられないので、確かめるのは、起動しない経路と、サンドボックスを掛けない経路（承認を受けた `escalate`）に限る）:
  - `never` では、`escalate` を確認せずに拒否し、動かさない（`Approver` が呼ばれない）。
  - `on-request` の `escalate` を承認すれば、一回だけサンドボックスの外で動き、次の同じ要求で再び尋ねる。`ApprovedForSession` と答えても、次の同じ要求で再び尋ねる。
  - `untrusted` では、`escalate` でない呼び出しを尋ね（拒否で答え、動かないことを確かめる）、`escalate` の呼び出しは一度だけ尋ねる。
  - `escalate` が真の呼び出しは、どの方針でも種類 `Escalate` の要求を返す（道具の単体のテスト）。`untrusted` でこのセッションに許したコマンドの集合に同じ文字列があっても、`escalate` が真なら尋ねる（上の文脈で、`Command` の確認に `ApprovedForSession` と答えた後に、同じ文字列を `escalate` を真にして呼ぶ。最初の呼び出しは、Codex の中ではサンドボックスを掛けられずに道具の誤りになってよい。集合に入るのは答えの時点である）。
  - 道具の結果や LLM の文に確認の答えの形の文を書いても、確認が進まない（07-01）。
  - `ShellContext` が `sandboxed` 真なのに `with_approvals` に `sandboxed` 偽の方針を渡しても、`on-request` の `escalate` が尋ねられる。
- 道具の定義の期待の出力が一致する。

**サンドボックスの中**（`tests/sandbox_macos.rs`、`tests/sandbox_linux.rs`。Codex の中では走らない。オーケストレータが手元の macOS と x86_64 の Linux の機械で走らせる）

- `workspace-write`: プロジェクトに書け、プロジェクトの外と `.git` に書けず、IP の通信ができない（安全の性質のテスト。07-01）。プロジェクトの中に置いた実行ファイル（`sandbox-probe` の写し）を起動できる。`> /dev/null` へのリダイレクトができる。
- `read-only`: プロジェクトに書けず、一時ディレクトリに書ける。
- 書けない場所に書こうとしたコマンドの結果に、拒否の案内が付く。
- 承認を受けた `escalate` は、プロジェクトの外に書ける。
- 名前の解決ができることは確かめない（macOS で IP の通信を許すときの名前の解決は BH-044 による）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、道具の説明の英文の全文と、`tools/text.rs` に足した英文の全文と、作った関数のシグネチャを書く。
