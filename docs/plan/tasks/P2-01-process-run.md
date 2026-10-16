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

- 標準入力に書くバイト列（なければ空。`/dev/null` にする）。書いてから閉じる。書き込みは別のスレッドで行い、子プロセスが読まずに終わっても止まらないようにする（`EPIPE` は誤りにしない）。このスレッドは待たない（グループを抜けたプロセスが標準入力を持ち続けると、書き込みが終わらないことがある）。
- 標準出力と標準エラー出力の受け取り先。呼び出す側が `Arc<Mutex<W>>`（`W: Write + Send + 'static`。待たずに返るスレッドが持つため）で渡し、実行の部分は子プロセスが終わるまで読み続けて書き込む（04-01 の「処理系の起動」。上限で読むのをやめない）。`bash` が二つをまとめて受け取れるように、同じ受け取り先を両方に渡せる形にする。
- `RunLimits`。
- 取り消しの印（`&AtomicBool`。エージェントのループが使う型）。

`Mutex` が毒されていたときは、`unwrap` せずに `PoisonError::into_inner` で中身を使う。

手順は設計書 03-02 の「子プロセスの後始末」のとおりとする。要点は次のとおり。

1. 子プロセスを新しいプロセスグループで起動する（既存の `configure` の `process_group(0)`）。
2. 子プロセスの終わりは、回収せずに知る（`rustix::process::waitid` の `WaitIdOptions::EXITED | NOWAIT`）。`waitid` は終わりを知るためだけに使い、終了の状態は、後で回収するときの `Child::wait` から得る。グループへ送る前に `Child::try_wait`・`wait` を呼ばない。終わりを待つのは別のスレッドで行い、主の側は、終わり、時間の上限、取り消しの印のどれかを待つ。取り消しの印は 50 ミリ秒ごとに見てよい。
   - 子プロセスは、実行の部分を呼んだスレッドで起動し、そのスレッドは子プロセスを回収するまで実行の部分の中にとどまる。理由: bubblewrap の `--die-with-parent` は、起動したスレッドが終わったときに働く。終わりを待つスレッドや出力を読むスレッドで起動しない。
3. 時間の上限か取り消しで止めるときは、Linux の `PreparedSandbox` では後述の制御のソケットで止める合図を伝え、`Command` ではグループに `StopSignal` を送る。`grace` のうちに終わらなければ、Linux では制御のソケットで直ちに終わる指示を送り、`Command` ではグループに `SIGKILL` を送る。
4. 子プロセスが終わったら（止めて終わったときを含む）、回収する前に、グループに `SIGKILL` を送る（グループがもうなければ `ESRCH` を誤りにしない）。その後に回収する。
5. 標準出力と標準エラー出力を読むスレッドが `pipe_close_wait` のうちに終わらなければ、`Leftovers::PossiblyRemaining` とする。そのスレッドは待たずに返る（受け取り先は `Arc` なので、呼び出す側は受け取った分を使える）。
6. Linux の `PreparedSandbox` で、直ちに終わる指示を送ってから 5 秒のうちに bubblewrap が終わらなければ、bubblewrap のグループに `SIGKILL` を送り、`Leftovers::PossiblyRemaining` とする（補助のプロセスが応えないときの最後の手段）。
7. Linux の `PreparedSandbox` の塞ぐための空のディレクトリの後始末は、今と同じく、bubblewrap の終わりを確かめてから行う。

`RunOutcome::status` は、起動したプログラム（bubblewrap か、`Command` のプログラム）の終了状態とする。

### 2. Linux の補助のプロセスを名前空間の pid 1 にする

設計書 03-02 の「子プロセスの後始末」の Linux の項のとおりにする。

- bubblewrap の引数の全体の指定に `--as-pid-1` を加える（`--unshare-pid` は今のまま）。`mounts.rs` のユニットテストの期待の並びも直す。
- 制御の通り道は、補助のプロセスの標準入力にした Unix ドメインのソケットの対とする（設計書 03-02）。`--control-fd` のような引数は設けない。理由: 受け継いだ番号から記述子を扱うには `unsafe`（`BorrowedFd::borrow_raw` など）が要り、起動のときに `FD_CLOEXEC` を外すと、同時に起動したほかの子プロセスにも受け継がれうる。標準入力なら `rustix::stdio` で `unsafe` なしに扱える。
- `PreparedSandbox` は、`prepare` のときにソケットの対（`std::os::unix::net::UnixStream::pair()`。`O_CLOEXEC` 付き）を作る。一方を bubblewrap の標準入力にし（`Stdio::from(OwnedFd)`）、もう一方を親が持つ。`spawn_wrapped` の包むプログラムも、標準入力をそのまま bubblewrap に渡すこと（今の包むプログラムのテストは、標準入力を子に受け継がせる形にする）。
- 目的のプログラムの標準入力（`SandboxStdin` の `Null` なら `/dev/null`、`Piped` ならパイプの読み口、`Inherit` なら親の標準入力）は、親が制御のソケットで記述子として送る（起動の前に送ってソケットに溜めておいてよい）（`rustix::net::sendmsg` と `SendAncillaryMessage::ScmRights`）。`Piped` のパイプの書き口は、今と同じく `SandboxChild` の `child_mut().stdin` から呼び出す側が使えるようにする。bubblewrap の標準入力は制御のソケットなので、実行の部分（1）と呼び出す側は、`PreparedSandbox` の標準入力を変えない。
- `SandboxChild` に、止める合図を伝える関数（`StopSignal` を受ける）と、直ちに終わる指示を伝える関数を加える。親の側のソケットを閉じることも、直ちに終わる指示として働く。
- 補助のプロセス（`run_helper`）は次の順で動く。
  1. 標準入力（記述子 0）が Unix ドメインのソケットであることを `rustix::fs::fstat(rustix::stdio::stdin())` で確かめる。違えば準備の失敗（終了状態 125）とする。
  2. 標準入力から、目的のプログラムの標準入力の記述子を一つ受け取る（`rustix::net::recvmsg`、`RecvFlags::CMSG_CLOEXEC`）。受け取れないとき、二つ以上来たときは準備の失敗とする。
  3. 制御のソケットを、`FD_CLOEXEC` の付いた新しい記述子に写し（`rustix::io::fcntl_dupfd_cloexec(stdin(), 3)`）、受け取った記述子を標準入力に置き換える（`rustix::stdio::dup2_stdin`）。この後、記述子 0 は目的のプログラムの標準入力であり、制御のソケットは受け継がれない。
  4. `check_descriptors` は今と同じく働く。ただし、3 で作った制御の記述子の番号は許す（その番号は 3 の結果から分かる）。ほかの記述子が開いていれば、今と同じく失敗とする（既存のテストの `spawn-with-fd` が 125 を期待する）。
  5. 今と同じ順で、`no_new_privs`、Landlock、seccomp を自分に掛ける。
  6. 自分をダンプできない形にする（`rustix::process::set_dumpable_behavior(DumpableBehavior::NotDumpable)`）。理由: 目的のプログラムが `/proc/1/fd` や `pidfd_getfd` で制御のソケットを取ることを防ぐ（設計書 03-02）。
  7. 目的のプログラムを、`exec` で置き換えずに、新しいプロセスグループの子として起動する（`process_group(0)`。標準入出力は受け継ぐ）。起動に失敗したら、今の `exec` の失敗と同じ扱いにする。起動は、制限を掛けた主のスレッドで行う。理由: Landlock と seccomp は、掛けたスレッドと、その後に作ったスレッドと子プロセスにだけ掛かる。
  8. 目的のプログラムを起動した後に、制御のソケットを読むスレッドを起こす。そのスレッドは 1 バイトの指示を読む。`I` なら目的のプログラムのグループに `SIGINT`、`T` なら `SIGTERM` を送る。`K` を読んだとき、ソケットが閉じたとき、読みが失敗したときは、直ちにプロセスを終える（終了状態 137）。知らない値は捨てる。
  9. 主のスレッドは、`rustix::process::wait` で、名前空間の中の子を回収し続ける。目的のプログラムでない子（親を失って引き取ったもの）は回収して捨てる。目的のプログラムを回収したら、すぐに、その終了状態で終わる（終了の値はそのまま、合図で終わったときは 128 に合図の番号を足した値）。プロセスを終える箇所は、この主のスレッドと 8 の二つに限り、どちらも `std::process::exit` で終える。目的のプログラムを回収した後に、8 のスレッドがグループへ送ることはない（送る前にプロセスが終わる）。pid 1 が終わると、カーネルが名前空間の残りのプロセスに `SIGKILL` を送る。
- 補助のプロセスは pid 1 なので、名前空間の中のプロセスからは、合図の受け手を設けていない合図で止められない。この性質に頼るコードにはしないが、受け手は設けない（`SIGINT`・`SIGTERM` の受け手を補助のプロセスに置かない）。
- 補助のプロセスを直接起動する既存のテスト（`__sandbox` を bubblewrap なしで呼ぶもの）は、標準入力を制御のソケットにする形に直す。方針の誤り、Landlock の段の失敗などを見分けるテストが、今と同じ段の名前で失敗するように、1〜4 の検査は方針の JSON の読み取りの後、`no_new_privs` の前に置く。

### 3. `sandbox-probe` の操作

テストのために、次の操作を加える。名前は任せる。`rustix` の機能 `process`（すべての対象）と、`signal-hook`（10-interfaces の表の版。`flag::register` を使うなら既定の機能は要らない）を加えてよい。

- 標準入力を標準出力に写す。
- 決まった時間だけ眠る。
- 自分を子として起動し、子の準備ができたら終わる。子は新しいセッションを作り（`rustix::process::setsid`）、目印の文字列を引数に持ったまま眠り続ける。子は、セッションを作った後に、準備ができたことを親に知らせる（パイプの 1 バイトなど）。親は、それを受けてから終わる。理由: 親がすぐに終わると、グループへの `SIGKILL` が、子がセッションを作る前に届き、テストが揺れる。標準出力と標準エラー出力を持ち続けるか、両方を `/dev/null` にするかを引数で選べるようにする。
- 同じく、子はセッションを作らず（同じグループのまま）、準備ができたことを知らせてから眠り続ける。
- `SIGINT` と `SIGTERM` の受け手を設け、受けた合図の名前を指定のファイルに書いて、終了状態 0 で終わる。
- `SIGINT` と `SIGTERM` を受けても終わらずに眠り続ける。

## 触るファイル

- `crates/benitoiteh/src/sandbox/`（`mod.rs`、`run.rs`（新）、`linux.rs`、`linux/process.rs`、`linux/mounts.rs`、`linux/mounts/tests.rs`、`linux/availability.rs`、`text.rs`、`tests.rs`）
- `crates/benitoiteh/Cargo.toml`（`rustix` の機能を足すとき。Linux の対象で `net` と `stdio` が要る見込み）
- `crates/sandbox-probe/`
- `crates/benitoiteh/tests/sandbox_linux.rs`、`tests/sandbox_macos.rs`、`tests/run.rs`（新。サンドボックスを要さない実行のテスト）

## テスト

テストの名前は任せる。どのテストも、テストごとの一時ディレクトリを使う。目印の文字列はテストごとに一意にする（テストは並んで動く）。残したプロセスは、テストの終わりに自分で止める。

- Linux では、`/proc/*/cmdline` で目印の文字列を持つプロセスを探して止める。理由: サンドボックスの中の子が書く番号は名前空間の中の番号であり、外から同じ番号に送ると別のプロセスに当たる。
- macOS と `tests/run.rs` では、止める直前に、番号のプロセスが生きていて目印を持つことを確かめてから送る（macOS は `ps -o command= -p <番号>` で確かめてよい）。理由: `launchd` がすでに回収していれば、番号が別のプロセスに使い回されうる。

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
- 補助のプロセスの標準入力がソケットでないとき、記述子が送られてこないとき、ほかの記述子も開いているときに、目的のプログラムが動かず 125 になる。
- 目的のプログラムが、制御のソケットを受け継いでいない（`/proc/self/fd` に標準入出力のほかがない）。目的のプログラムが `/proc/1/fd` の下を開けない。
- `SandboxStdin` の三つの形で、目的のプログラムの標準入力が期待どおりになる（`Piped` で書いたバイト列が届く、`Null` で空）。
- 塞ぐための空のディレクトリが、止めて終わったときも残らない。
- 既存の `sandbox_linux` のテストが、補助のプロセスを直接起動するものを制御のソケットの形に直したうえで、すべて通る。

**macOS（`tests/sandbox_macos.rs`）**

- `sandbox-exec` で包んだ子で、同じグループのまま残した子が止められ、`Leftovers::None` を返す。
- 新しいセッションでグループを抜け、標準出力を持ち続ける子では、`Leftovers::PossiblyRemaining` を返す（macOS では止められないことを確かめるテスト。テストの終わりに自分で止める）。
- 時間の上限で止める合図が、`sandbox-exec` で包んだ目的のプログラムに届く。

## 確かめること

- Linux の `--as-pid-1` のときに、外側の bubblewrap が名前空間の pid 1 の終わりを待って終わること（`bubblewrap.c` の `monitor_child`）と、その時点で名前空間の中のプロセスが一つも残っていないこと（BH-090）。確かめた bubblewrap の版を書く。
- 補助のプロセスが `exec` から子の起動に変わったことで、既存の Linux のテスト（busybox の `argv[0]` のものなど）の前提が変わらないこと。
- 実行の部分の時間の費用。サンドボックスを掛けない `sandbox-probe` の何もしない操作を 20 回実行した中央値を、実行の部分を通したときと、`Command::output` で直接動かしたときとで比べて書く（macOS）。
- 実行ファイルの大きさの変化（リリースのビルド、macOS と Linux の x86_64）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、上の「確かめること」の結果と、作った公開の型と関数のシグネチャを書く（オーケストレータが 10-interfaces を更新する）。
