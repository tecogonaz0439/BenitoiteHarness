# P5-15 画像を縮める処理を補助のプロセスに移す

## 目的

設計書 01-03「画像のファイルの読み取り」に 2026-10-19 に決めた【決定】を実装する。P5-14 で主のプロセスの中で行っている縮める処理（解く、縮める、向きを直す、符号化する）を、`benitoiteh` 自身を補助のプロセス `benitoiteh __image` として OS のサンドボックスの中で起動して行う形に移す。縮めた結果は、主のプロセスが確かめてから使う。

この作業で本物の LLM の提供者を呼ばない。P5-14 を取り込んだ `main` から始める。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「画像のファイルの読み取り」（とくに補助のプロセスの【決定】と、その下の「受け渡し」「サンドボックス」「上限」「一度に一つ」）
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「サンドボックスの掛け方」「方針の要素」「サンドボックスを掛けられないとき」「端末からの切り離し」「子プロセスの後始末」
- 既存のもの: `crates/benitoiteh/src/tools/read/image.rs`（P5-14 の縮める処理、`RESIZE` の錠、取り消しの検査、`inspect`）、`src/cli/mod.rs`（`__sandbox`・`__session` の入口の分け方）、`src/sandbox/mod.rs`（`SandboxCommand`、`SandboxStdin`、`to_command`）、`src/sandbox/policy.rs`（`PolicyConfig`、`ReadAccess`、`ProgramAccess`）、`src/sandbox/restricted.rs`（`scratch_policy` の、macOS で起動に要る読める場所の組）、`src/sandbox/run.rs`（`run`、`RunLimits`、`RunEnd`）、`src/sandbox/session.rs`、`src/tools/mod.rs`（`ToolContext` がサンドボックスと一時ディレクトリをどう持つか）

## この作業が認める型の変更

- `ToolContext` に、補助を起動するのに要るもの（補助の実行ファイルのパス、補助のための一時ディレクトリの親、サンドボックスを掛けられるか）を持たせる変更。既にあるもの（制限モードの `RunContext`、通常モードの `ShellContext` など）から取れるなら、それを使う。
- `src/cli/mod.rs` の補助の入口に `__image` を加える。

## 作るもの

### 1. 補助の入口（`benitoiteh __image <png|jpeg> <幅> <高さ>`）

- `__sandbox`・`__session` と同じく、panic の hook と通常の引数の解釈より前に分ける。
- 最初に自分にメモリーの上限を掛ける（`rustix` の `setrlimit` で `RLIMIT_AS` を 1 GiB。`unsafe` を使わない）。掛けられなければ誤りで終わる。macOS で掛けられても守られるかは BH-113 で確かめる（この作業では、掛ける呼び出しと、Linux で効くことのテストだけを行う）。
- 標準入力を 64 MiB ＋ 1 バイトまで読み、超えたら誤りで終わる。
- P5-14 の縮める処理（解く前の上限、`image::Limits`、解いた画素の上限、縮める、向き、透明の判定、符号化と品質の下げ方）を、そのまま補助の中で行う。引数の寸法は、解き手の `Limits` の幅と高さに使う。
- 成功したら、標準出力に一行の JSON（`{"media_type":"image/jpeg","width":2000,"height":1000}`）と改行を書き、続けて符号化した中身を書いて、終了状態 0 で終わる。失敗したら、誤りの種類（解けない、縮めても上限を超える、など）を示す短い一行を標準エラー出力に書き、0 でない終了状態で終わる（文言は固定の定数。ファイルの中身を書かない）。

### 2. 主のプロセスの側（`tools/read/image.rs`）

- 縮めるかの判断と、解く前の上限（形式、4,000 万画素、64 MiB、0 の寸法、動く PNG）は、今のとおり主のプロセスで行う。
- 縮めるときは、`RESIZE` の錠を今のとおり取り（3.75 MiB を超えるファイルは読む前から）、補助を起動する。
  - サンドボックスの方針: 通信を許さない。書ける場所は、補助ごとに作る空の一時ディレクトリ（セッションの一時ディレクトリの下など。終わったら消す）だけ。読める場所は、`benitoiteh` の実行ファイルと、起動に要る OS の場所だけ（macOS は `scratch_policy` の組を参考に、要るものだけ。Linux は bubblewrap の見え方に任せ、Landlock の読める範囲を同じく狭める）。起動できるプログラムは `benitoiteh` の実行ファイルだけ。守る名前は今の既定のまま。
  - 起動の形は、子プロセスと同じ `SandboxCommand` と `run`（Linux の bubblewrap の起動、macOS の `sandbox-exec` と `__session`）を使う。標準入力に中身を書き、標準出力は 3,932,160 ＋ 4,096 バイトまでで打ち切って受ける（超えたら誤り）。標準エラー出力は 4 KiB までとする。
  - `RunLimits` の時間の上限を 30 秒とする。取り消しの印は `run` に渡す（取り消されたら補助を止め、今の取り消しの誤りを返す）。
  - 補助が 0 以外で終わったか、合図で終わったか、時間の上限に達したか、出力の形が違うときは、`IMAGE_DECODE`（か、縮めても大きすぎるときの今の文）の誤りを返す。標準エラー出力の一行は、既知の種類に当たるときだけ文の選び分けに使い、そのまま LLM に返さない。
  - 受け取った中身に `inspect` を掛け、MIME の型と寸法が JSON の行と合うことを確かめる。合わなければ壊れた画像の誤りとする。
- サンドボックスを掛けられないとき（`SandboxError::Unavailable` など。Linux で bubblewrap がないとき）。通常モードの設定（`danger-full-access` など）によらず、補助にはいつもサンドボックスを掛けるは、補助を起動せず、P5-11 の上限の誤り（大きさか寸法）を返す。主のプロセスの中では縮めない。
- 主のプロセスから、`image` を使う解く・縮める・符号化するコードをなくす（補助の側だけが使う。同じクレートの中なので依存は変わらない）。

### 3. 後始末

- 補助の一時ディレクトリは、成功でも失敗でも消す（シンボリックリンクを辿らない削除）。

## 触るファイル

- `crates/benitoiteh/src/cli/mod.rs`
- `crates/benitoiteh/src/tools/read/image.rs`（要れば新しい `src/tools/read/resize.rs`、`src/image_helper.rs` など）、`src/tools/mod.rs`、`src/tools/text.rs`
- `crates/benitoiteh/src/sandbox/`（要れば方針を作る関数を加える）
- 道具の文脈を作る所（`src/cli/exec.rs`、`src/cli/tui.rs` など。補助の起動に要るものを渡す）
- テスト: `crates/benitoiteh/tests/tools_read_image.rs`（今のテストを保つ）、補助の入口の単体のテスト、サンドボックスの中での起動のテスト（`tests/sandbox_*.rs` の置き場所の流儀に合わせる。macOS と Linux）

## テスト

- 今の `tests/tools_read_image.rs` のテストが、補助を通しても同じ結果で通ること（テストの道具の文脈で、補助を起動できる設定にする）。
- 補助の入口: 正しい PNG と JPEG で、一行の JSON と中身を書くこと。壊れた中身、64 MiB を超える入力、引数の誤りで、0 以外で終わり、標準出力に何も書かないこと。
- 主のプロセス: 補助の出力の JSON と中身が合わない場合、出力が上限を超える場合、補助が合図で終わる場合（試し用に補助を差し替えられる口を、テストだけに設けてよい）に、壊れた画像の誤りになり、保存しないこと。時間の上限に達した場合も同じこと。
- サンドボックス: 補助の中から、ホームディレクトリのファイルを読めないこと、通信できないこと（`sandbox_*.rs` の流儀で。macOS と Linux）。
- サンドボックスを掛けられない設定で、縮める画像が上限の誤りになり、縮めない画像は今までどおり読めること。
- Linux: 補助のメモリーの上限が効くこと（上限を超える確保で補助が失敗すること）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。サンドボックスのテストは、オーケストレータが外で走らせる全体の検査で確かめる。
- 完了の報告に、補助を起動して縮めるのにかかる時間（2,400×1,200 の PNG で、補助なしの P5-14 と比べて）と、実行ファイルの大きさの変化を書く。

## 補足（2026-10-19、始める前の点検）

本節は、上の各節に優先する。設計書 01-03 の「上限」を、macOS ではメモリーの上限を掛けない形に改めた（BH-113）。

1. **メモリーの上限は Linux だけ**: macOS は `setrlimit` で 512 GiB より小さい `RLIMIT_AS`・`RLIMIT_DATA`・`RLIMIT_RSS` を `EINVAL` で拒む（どのプロセスも仮想の大きさが約 466 GiB ある）。上限は `#[cfg(target_os = "linux")]` の中だけで掛け（`rustix::process::setrlimit(Resource::As, Rlimit { current: Some(1 << 30), maximum: Some(1 << 30) })`。今の `process` の機能で使える。`src/agent/execution.rs:576-580` に NOFILE の例がある）、掛けられなければ誤りで終わる。macOS では掛けない。作るもの 1 の「掛けられなければ誤りで終わる」は Linux だけに当てる。
2. **テストの置き場所**（`scripts/check.sh --inside-codex` は `tests/sandbox_*.rs` を除き、入れ子の `sandbox-exec` は Codex の中で掛けられない。aarch64 のコンテナは Landlock がない）:
   - 補助をサンドボックスの中で起動して縮めるテストは、新しい `tests/sandbox_read_image.rs` に移す。このファイルを `scripts/check.sh` の 186 行あたりの aarch64 の除外の一覧に加える（`scripts/check.sh` を触るファイルに加える）。
   - `tests/tools_read_image.rs` には、補助を起動しないテスト（縮めない画像、補助を使えないときの上限の誤り、錠の前の取り消し）だけを残す。
   - 補助の入口（解く、縮める、向き、ICC、16 ビット、品質の下げ方、透明）は、サンドボックスなしで `fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))` を `__image` の引数で直に起動するテスト（`sandbox_` で始まらないファイル）で確かめる。これは Codex の中でも走る。
   - Linux のメモリーの上限のテストは、`src/agent/execution.rs:586-599` の、自分を起動し直す単体のテストの形（`Command::new(current_exe()).args(["--exact", …]).env(CHILD, "1")`）で、子が同じ上限の関数を呼び、`try_reserve_exact(2 GiB)` が失敗することを確かめる（`cfg(target_os = "linux")`）。
3. **補助を使えるかの文脈**: `ShellContext.sandboxed`（`danger-full-access` で偽）と `RunContext`（制限モードで処理系があるときだけ）は使わない。`ToolContext::with_image_helper(ImageHelperContext { helper, scratch, placeholders, work_parent, .. })` の形の組み立てを加え、既定（`None`）は「使えない」とする（今の `ToolContext::new` の呼び出しを変えない）。`exec::prepare` で一度だけ設定する（`tui.rs` は `exec::prepare`・`prepare_new` を使うので変えない）。使えるかは `availability.is_ok() && !sandbox_disabled` とし（`select_query_launcher` の `exec.rs:676-682` と同じ規則）、`exec.rs:376-394` で `availability` を動かす前に求める。`helper`・`scratch`・`placeholders` は `exec.rs:302-304`・`365-366` で両方のモードについて既に求めている。
4. **補助の作業ディレクトリ**: セッションの非公開の `sandbox/` ディレクトリ（`exec.rs:365`、0700）の下に `image-<乱数>` を `create_dir` で作り、正規のパスにしてから方針の書ける場所にする。セッションの一時ディレクトリ（LLM の道具と子プロセスが書ける）は使わない。終わったら `std::fs::remove_dir_all` で消し、`NotFound` は許す。
5. **方針**: `sandbox::image_policy(exe, workdir) -> Result<SandboxPolicy, SandboxError>` を公開で加える。`readable: ReadAccess::Roots(vec![])`、`readable_files: [exe]`（macOS だけ `"/"` も。Linux の検証は `readable_files` を通常のファイルに限るので、`"/"` は macOS だけ）、`programs: ProgramAccess::Only(vec![exe])`、`allow_ip: false`、`writable: vec![workdir]`、`deny_exec_in_writable: true`、守る名前は既定。macOS で `benitoiteh` が起動するのに要るのは、既定の `hw.pagesize_compat`、`/` そのものの読み取り、実行ファイルの実行とマップだけである（点検で確かめた）。Linux の対象は musl の静的なビルドなので、ライブラリの読み取りは要らない。サンドボックスのテストでは、同じ方針で `sandbox-probe` の `read <path>` と `tcp-connect <addr>` を起動し、ホームディレクトリのファイルを読めず、通信できないことを確かめる。
6. **起動**: macOS は `SandboxCommand { policy, program: exe, args: ["__image", 形式, 幅, 高さ], cwd: workdir, env: 空, stdin: SandboxStdin::Piped }.to_command(&exe)`、Linux は `SandboxCommand.prepare(&helper, &scratch, &placeholders)`（`stdin: Piped` は入力が空でないときに要る）。`SandboxError::Unavailable` は P5-11 の上限の誤りに、`InvalidPolicy`・`Setup` は `IMAGE_DECODE` に、`Io` は道具の I/O の誤りにする。終了状態は、Linux では bubblewrap のもの（合図での終わりは `128+N`、準備の失敗は 125）、macOS では補助のものなので、テストは「成功でない」ことだけを確かめる。`__image` は 125 と 128 以上の終了状態を使わない。`__image` は panic の hook の前に分け（`cli/mod.rs:28-36`）、設定ファイルと環境変数を読まない。
7. **出力の受け方**: 書き込みの誤りを上限の代わりに使わない（`run` は誤りを記録して読み続けるだけ）。`src/benitoite/query.rs` の 153〜207 行と 231〜251 行の形（上限で切り詰めて `exceeded` と共有の `stop` を立て、`stop` を `run` の取り消しの印として渡し、本物の取り消しの印を `stop` に写す見張りのスレッドを置く）を使う（共有のモジュールに移してよい）。`RunEnd::Cancelled` は上限の超過でも起きるので、先に `exceeded` を見る。本物の取り消しは今の取り消しの誤りに、そのほかは `IMAGE_DECODE` にする。`Leftovers::PossiblyRemaining` も失敗とする。`RunLimits` は時間の上限 30 秒、猶予 1 秒、`StopSignal::Terminate` とする。
8. **主のプロセスの確かめ**: 標準出力の最初の 4,096 バイトの中の最初の改行までを一行目とし、`deny_unknown_fields` の構造体で読む。欄は `media_type`（`image/png` か `image/jpeg` だけ）、`width`、`height`、`original_width`、`original_height` とし（設計書 01-03 の「元の寸法」）、元の寸法は主のプロセスが見出しから読んだ値と合うこと、`media_type`・`width`・`height` は中身に掛けた `inspect` と合うこと、中身の長さが `MAX_IMAGE_BYTES` 以下であることを確かめる。
9. **縮めても大きすぎるとき**: 今の文 `image_too_large_after_resizing(bytes, limit)` はバイト数を含む。補助は標準エラー出力に固定の語と十進の数字の一行（例: `too-large 4123456`）を書き、主のプロセスは数字だけで `MAX_IMAGE_BYTES` より大きい値のときだけその文に使う。そのほかの一行は `IMAGE_DECODE` にする。
10. **補助を使えないとき**: 錠を取る前、読む前に判定する。大きさが `MAX_IMAGE_BYTES` を超えるなら、64 MiB を読まずに `image_too_large(size, MAX_IMAGE_BYTES)` を返し、そうでなければ `image_dimensions` を返す（使えないときの上限は `MAX_FILE_BYTES` ではなく `MAX_IMAGE_BYTES`）。
11. **試しの起動の差し替え**: 主のプロセスの扱い（JSON の不一致、出力の超過、合図での終わり、時間の上限）を確かめるために、`src/benitoite/query.rs` の `QueryLauncher::TestDirect`（15〜21 行、138〜149 行）と同じく、`#[cfg(test)]` の単体のテストだけで作れる起動の形（方針は作って検証したうえで、`/bin/sh -c '…'` を直に起動する）と、テストで変えられる時間の上限を設けてよい。これらは Codex の中でも走る。サンドボックスの中での出力の超過と時間の上限は、`sandbox-probe` の `query-output <stdout> <stderr> <delay_ms>` と `wait-stdin` を補助の代わりに使って確かめてよい。
12. **移すコードと残すコード**（`src/tools/read/image.rs`）: 補助に移すのは `resize`（197〜268 行）、`png_has_icc_profile`（287〜305 行）、`has_transparency`（307〜321 行）と、233・241 行の取り消しの検査（補助の中では、主のプロセスの `run` の取り消しになる）。主のプロセスに残すのは `detect`、`header`、`png`（`acTL`）、`jpeg`、`webp`、`inspect`、大きさと 4,000 万画素の検査（60〜106 行）、`RESIZE` の錠と `lock_resize`（21 行、270〜278 行）、`check_cancelled`、保存、版の記録。補助も `header` と `png_has_icc_profile` を使うので、両方から呼べる所に置く。
13. **デバッグのビルドの時間**: 補助なしの点検の測定で、2,400×1,200 の RGBA の PNG → JPEG が 1.23 秒、2,100×2,100 の 16 ビットが 2.65 秒、2,000×2,000 の雑音が 2.06 秒だった（release ではそれぞれ 20 ミリ秒、52 ミリ秒、148 ミリ秒）。30 秒の上限の中だが、負荷の高い Linux や x86_64 では余裕が減る。
