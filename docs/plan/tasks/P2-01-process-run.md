# P2-01 子プロセスの実行と後始末（BH-090）

## 目的

道具が起動する子プロセスを、時間の上限と取り消しで止め、終わった後に子孫のプロセスを残さない形で実行する部分を作る。P2 の `benitoite_*` の道具と P3 の `bash` は、この部分を通して子プロセスを動かす。道具そのものは作らない。

2026-10-17 に決めた方法（設計書 03-02 の「子プロセスの後始末」）を実装し、BH-090 の【要検証】（Linux で、bubblewrap の終わりを受け取った時点で名前空間の中のプロセスが一つも残っていないこと）をテストで確かめる。

## 読む設計書

- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の次の節。
  - 「子プロセスの後始末」（この作業の中心）
  - 「サンドボックスの掛け方」
  - 「Linux のファイルシステムの見え方」の、塞ぐための空のディレクトリの後始末の規則
- [道具](../../design/01-agent/01-03-tools.md)の「bash（通常モード）」の時間の上限と止め方
- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「処理系の起動」（`SIGINT` で止めること、出力を終わりまで読み続けること）
- [テスト](../../design/07-quality/07-01-testing.md)の「安全の性質のテスト」の子孫のプロセスの項目
- [未決事項](../../design/open-issues.md)の BH-090、BH-094
- 既存のもの。
  - `crates/benitoiteh/src/sandbox/`（`mod.rs` の `SandboxCommand`・`configure`、`linux.rs` の `run_helper`・`check_descriptors`、`linux/process.rs` の `PreparedSandbox`・`SandboxChild`、`linux/mounts.rs` の bubblewrap の引数）
  - `crates/sandbox-probe/`
  - `tests/sandbox_linux.rs`、`tests/sandbox_macos.rs`
  - `crates/benitoiteh/src/config/credentials.rs`（子プロセスを起動するほかの箇所）

## 作るもの

### 1. 実行の部分（`sandbox` の下）

`crates/benitoiteh/src/sandbox/run.rs`（と、要れば `run/` の下の OS ごとのファイル）に作る。名前は次を基本とし、細部は任せる。

```rust
/// 時間の上限と取り消しで止めるときに、最初に送る合図。
pub enum StopSignal { Interrupt, Terminate }

/// 止め方の値。
pub struct RunLimits {
    pub timeout: Duration,
    pub stop_signal: StopSignal,
    /// 止める合図から SIGKILL までの猶予。既定 10 秒。
    pub grace: Duration,
    /// SIGKILL の後に、出力のパイプが閉じるのを待つ時間。既定 2 秒。
    pub pipe_close_wait: Duration,
}

/// 子プロセスがどう終わったか。
pub enum RunEnd { Exited, TimedOut, Cancelled }

/// 子孫のプロセスが残ったか。
pub enum Leftovers { None, PossiblyRemaining }

pub struct RunOutcome { pub status: ExitStatus, pub end: RunEnd, pub leftovers: Leftovers }
```

起動の形は、次の二つを受け付ける。

- Linux の `PreparedSandbox`（bubblewrap で包んだもの）。
- `std::process::Command`（macOS の `sandbox-exec` で包んだもの、またはサンドボックスを掛けない起動）。

入力は次のとおりとする。

- 標準入力に書くバイト列（なければ空。`/dev/null` にする）。書いてから閉じる。書き込みは別のスレッドで行い、子プロセスが読まずに終わっても止まらないようにする（`EPIPE` は誤りにしない）。
- 標準出力と標準エラー出力の受け取り先。呼び出す側が `Arc<Mutex<W>>`（`W: Write + Send`）で渡し、実行の部分は子プロセスが終わるまで読み続けて書き込む（04-01 の「処理系の起動」。上限で読むのをやめない）。`bash` が二つをまとめて受け取れるように、同じ受け取り先を両方に渡せる形にする。
- `RunLimits`。
- 取り消しの印（`&AtomicBool`。エージェントのループが使う型）。

手順は設計書 03-02 の「子プロセスの後始末」のとおりとする。要点は次のとおり。

1. 子プロセスを新しいプロセスグループで起動する（既存の `configure` の `process_group(0)`）。
2. 子プロセスの終わりは、回収せずに知る（`rustix::process::waitid` の `WaitIdOptions::EXITED | NOWAIT`）。終わりを待つのは別のスレッドで行い、主の側は、終わり、時間の上限、取り消しの印のどれかを待つ。取り消しの印は 50 ミリ秒ごとに見てよい。
3. 時間の上限か取り消しで止めるときは、Linux の `PreparedSandbox` では後述の制御のパイプで止める合図を伝え、`Command` ではグループに `StopSignal` を送る。`grace` のうちに終わらなければ、Linux では制御のパイプで直ちに終わる指示を送り、`Command` ではグループに `SIGKILL` を送る。
4. 子プロセスが終わったら（止めて終わったときを含む）、回収する前に、グループに `SIGKILL` を送る（グループがもうなければ `ESRCH` を誤りにしない）。その後に回収する。
5. 標準出力と標準エラー出力を読むスレッドが `pipe_close_wait` のうちに終わらなければ、`Leftovers::PossiblyRemaining` とする。そのスレッドは待たずに返る（受け取り先は `Arc` なので、呼び出す側は受け取った分を使える）。
6. Linux の `PreparedSandbox` で、直ちに終わる指示を送ってから 5 秒のうちに bubblewrap が終わらなければ、bubblewrap のグループに `SIGKILL` を送り、`Leftovers::PossiblyRemaining` とする（補助のプロセスが応えないときの最後の手段）。
7. Linux の `PreparedSandbox` の塞ぐための空のディレクトリの後始末は、今と同じく、bubblewrap の終わりを確かめてから行う。

`RunOutcome::status` は、起動したプログラム（bubblewrap か、`Command` のプログラム）の終了状態とする。

### 2. 子プロセスの起動を一か所に集める

制御のパイプの読み口を補助のプロセスへ受け継がせるため、起動のときだけその記述子の `FD_CLOEXEC` を外す。ほかの子プロセスに受け継がれないように、`benitoiteh` の子プロセスの起動は、`sandbox` の一つの関数（例: `spawn_command(&mut Command) -> io::Result<Child>`）を通して、プロセスの中の一つの `Mutex` を持って行う。

- 実行の部分（1）と、`PreparedSandbox` の `spawn`・`spawn_wrapped`・`output`・`output_wrapped`、`probe_linux`、`config/credentials.rs` の起動を、この関数に移す。
- `FD_CLOEXEC` を外すのは、この `Mutex` を持っている間だけとし、起動の後に読み口を親で閉じる（成功しても失敗しても）。
- `src/` の下に、ほかに子プロセスを起動する箇所（`Command::spawn`・`output`・`status`）があれば、同じく移す。テストのコードは移さなくてよい。

### 3. Linux の補助のプロセスを名前空間の pid 1 にする

設計書 03-02 の「子プロセスの後始末」の Linux の項のとおりにする。

- bubblewrap の引数の全体の指定に `--as-pid-1` を加える（`--unshare-pid` は今のまま）。`mounts.rs` のユニットテストの期待の並びも直す。
- `PreparedSandbox` は、作るとき（または起動のとき）に制御のパイプ（`O_CLOEXEC` 付き）を作り、補助のプロセスの引数に読み口の番号を渡す。形は `__sandbox --control-fd <番号> <方針の JSON> -- <プログラム> <引数...>` とする。書き口は `SandboxChild` が持つ。
- `SandboxChild` に、止める合図を伝える関数（`StopSignal` を受ける）と、直ちに終わる指示を伝える関数を加える。書き口を閉じることも、直ちに終わる指示として働く。
- 補助のプロセス（`run_helper`）は次のとおりにする。
  1. `--control-fd` の番号の記述子が開いていて、パイプ（FIFO）であることを確かめる。違えば準備の失敗（終了状態 125）とする。
  2. `check_descriptors` は、標準入出力と、この番号の記述子だけを許す。ほかの記述子が開いていれば、今と同じく失敗とする（既存のテストの `spawn-with-fd` が 125 を期待する）。
  3. 制御の記述子に `FD_CLOEXEC` を付け、目的のプログラムへ受け継がせない。
  4. 今と同じ順で、`no_new_privs`、Landlock、seccomp を自分に掛ける。
  5. 掛けた後に、制御のパイプを読むスレッドを起こす。理由: Landlock と seccomp はスレッドごとに掛かるので、掛けた後に作ったスレッドだけが同じ制限を受ける。
  6. 目的のプログラムを、`exec` で置き換えずに、新しいプロセスグループの子として起動する（`process_group(0)`。標準入出力は受け継ぐ）。起動に失敗したら、今の `exec` の失敗と同じ扱いにする。
  7. 主のスレッドは、`rustix::process::wait` で、名前空間の中の子を回収し続ける。目的のプログラムを回収したら、その終了状態で終わる（終了の値はそのまま、合図で終わったときは 128 に合図の番号を足した値）。目的のプログラムでない子（親を失って引き取ったもの）は回収して捨てる。
  8. 制御のパイプを読むスレッドは、1 バイトの指示を読む。`I` なら目的のプログラムのグループに `SIGINT`、`T` なら `SIGTERM` を送る。`K` を読んだとき、パイプが閉じたとき、読みが失敗したときは、直ちにプロセスを終える（終了状態 137）。pid 1 が終わると、カーネルが名前空間の残りのプロセスに `SIGKILL` を送る。知らない値は捨てる。
- 補助のプロセスは pid 1 なので、名前空間の中のプロセスからは、合図の受け手を設けていない合図で止められない。この性質に頼るコードにはしないが、受け手は設けない（`SIGINT`・`SIGTERM` の受け手を補助のプロセスに置かない）。

### 4. `sandbox-probe` の操作

テストのために、次の操作を加える。名前は任せる。`rustix` の機能 `process` と、`signal-hook`（10-interfaces の表の版）を加えてよい。

- 標準入力を標準出力に写す。
- 決まった時間だけ眠る。
- 自分を子として起動し、すぐに終わる。子は新しいセッションを作り（`rustix::process::setsid`）、目印の文字列を引数に持ったまま眠り続ける。子は、自分のプロセスの番号を指定のファイルに書く。標準出力を閉じるかどうかを引数で選べるようにする。
- 自分を子として起動し、すぐに終わる。子はセッションを作らず（同じグループのまま）、眠り続ける。
- `SIGINT` と `SIGTERM` の受け手を設け、受けた合図の名前を指定のファイルに書いて、終了状態 0 で終わる。
- `SIGINT` と `SIGTERM` を受けても終わらずに眠り続ける。

## 触るファイル

- `crates/benitoiteh/src/sandbox/`（`mod.rs`、`run.rs`（新）、`linux.rs`、`linux/process.rs`、`linux/mounts.rs`、`linux/mounts/tests.rs`、`linux/availability.rs`、`text.rs`、`tests.rs`）
- `crates/benitoiteh/src/config/credentials.rs`（起動を一か所に集めるため）
- `crates/benitoiteh/Cargo.toml`（`rustix` の機能を足すとき。パイプを作るには機能 `pipe` が要る）
- `crates/sandbox-probe/`
- `crates/benitoiteh/tests/sandbox_linux.rs`、`tests/sandbox_macos.rs`、`tests/run.rs`（新。サンドボックスを要さない実行のテスト）

## テスト

テストの名前は任せる。どのテストも、テストごとの一時ディレクトリを使い、残したプロセスを、テストの終わりに自分で止める（目印のファイルのプロセスの番号に `SIGKILL` を送る）。

**サンドボックスを掛けない実行（`tests/run.rs`。`--inside-codex` でも走るもの）**

- 標準入力に渡したバイト列が子プロセスに届き、標準出力と標準エラー出力が受け取り先に届く。
- 時間の上限で止まり、`RunEnd::TimedOut` を返す。`StopSignal::Interrupt` で `SIGINT` が、`Terminate` で `SIGTERM` が目的のプログラムに届く（受けた合図の名前のファイルで確かめる）。
- 合図を無視するプログラムが、`grace` の後に `SIGKILL` で止まる（`grace` は短くしてよい）。
- 取り消しの印で止まり、`RunEnd::Cancelled` を返す。
- 同じグループのまま残した子が、実行が返った時点で止められている。止められたことは、Linux では `/proc/<番号>` がないか、状態が `Z`（ゾンビ）であることで、macOS では `kill(pid, 0)` が `ESRCH` になることで確かめる（どちらも短い時間だけ待ってよい）。理由: 親を失った子は、その環境の pid 1 が回収する。podman と `container` の中では、pid 1 がテストの実行ファイル自身で回収しないので、止めた子はゾンビとして残る。
- 新しいセッションでグループを抜け、標準出力を持ち続ける子を残すと、`Leftovers::PossiblyRemaining` を返す。標準出力を閉じた子では `Leftovers::None` を返す（この方法の限界を示すテスト）。

**Linux のサンドボックスの中（`tests/sandbox_linux.rs`）**

- 新しいセッションでグループを抜けた子（標準出力を閉じたものと持ち続けるものの両方）が、実行が返った直後に一つも残っていない。`/proc/*/cmdline` に目印の文字列を持つプロセスがないことで確かめ、待たずに一度だけ調べる（BH-090 の【要検証】）。`Leftovers::None` を返す。
- 同じグループのまま残した子も、同じく残っていない。
- 時間の上限で止めるとき、止める合図が名前空間の中の目的のプログラムに届く（受けた合図の名前のファイル）。合図を無視するプログラムは `grace` の後に止まり、名前空間の中に何も残らない。
- 目的のプログラムの終了状態が、そのまま `RunOutcome::status` に現れる（0 と 0 でない値、合図で終わったときの 128 足す番号）。
- 制御の記述子の番号が開いていないとき、パイプでないとき、ほかの記述子も開いているときに、目的のプログラムが動かず 125 になる。
- 目的のプログラムが、制御の記述子を受け継いでいない（`/proc/self/fd` に標準入出力のほかがない）。
- 塞ぐための空のディレクトリが、止めて終わったときも残らない。
- 既存の `sandbox_linux` のテストがすべて通る。

**macOS（`tests/sandbox_macos.rs`）**

- `sandbox-exec` で包んだ子で、同じグループのまま残した子が止められ、`Leftovers::None` を返す。
- 新しいセッションでグループを抜け、標準出力を持ち続ける子では、`Leftovers::PossiblyRemaining` を返す（macOS では止められないことを確かめるテスト。テストの終わりに自分で止める）。
- 時間の上限で止める合図が、`sandbox-exec` で包んだ目的のプログラムに届く。

## 確かめること

- Linux の `--as-pid-1` のときに、外側の bubblewrap が名前空間の pid 1 の終わりを待って終わること（`bubblewrap.c` の `monitor_child`）と、その時点で名前空間の中のプロセスが一つも残っていないこと（BH-090）。確かめた bubblewrap の版を書く。
- 起動を一か所に集めた後、`src/` の下に `Command` の `spawn`・`output`・`status` を直接呼ぶ箇所が残っていないこと（`rg` の結果を書く）。
- 補助のプロセスが `exec` から子の起動に変わったことで、既存の Linux のテスト（busybox の `argv[0]` のものなど）の前提が変わらないこと。
- 実行の部分の時間の費用。サンドボックスを掛けない `sandbox-probe` の何もしない操作を 20 回実行した中央値を、実行の部分を通したときと、`Command::output` で直接動かしたときとで比べて書く（macOS）。
- 実行ファイルの大きさの変化（リリースのビルド、macOS と Linux の x86_64）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、上の「確かめること」の結果と、作った公開の型と関数のシグネチャを書く（オーケストレータが 10-interfaces を更新する）。
