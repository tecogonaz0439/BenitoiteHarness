# P2-07 診断の受け渡し、`benitoite_check`、書いた後の検査、検査と修正の回数

## 目的

処理系の診断を LLM に返す文の形に変える部品を作り、道具 `benitoite_check`（`tests` の旗を含む）と、制限モードの書いた後の検査を作る（設計書 01-03 の「benitoite_check」「書いた後の検査（制限モード）」、04-01 の「終了状態と診断の受け渡し」）。制限モードの検査と修正の回数の上限（01-01 の「上限」）を数え、セッションの集計の `checks` と `fixes` を埋める。書いた後の検査の時間を、サンドボックスの中で測り直す（BH-028）。

## 読む設計書

- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「処理系の起動」（表の「検査」「テストの検査」の行、検査の時間の上限 30 秒、絶対の実のパス、検査をサンドボックスの中で起動すること、検査の標準エラー出力の 16 MiB の上限）「終了状態と診断の受け渡し」（検査の終了状態の分類の表、診断の欄、文の形、位置の行の読み方と印の付け方、読まないファイルの診断を件数だけにすること、重なった診断と誤った表示のパスのまとめ方、20 件の上限と残りの件数の行）「処理系への要望」の 7 と 8
- [道具](../../design/01-agent/01-03-tools.md)の「書いた後の検査（制限モード）」（入口の定義、BH-028 の測定と【方針】、検査しないとき、結果の行、件数の行の単数と複数、誤りの印を付けないこと）「benitoite_check」（パスの解き方、結果の分類）「道具の定義の共通の規則」「読まないファイル」「出力の切り詰め」「モードごとの道具の一覧」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「上限」（検査と修正の回数、誤りを返したとする条件、回数を変えないとき、止めた後と再開のときの回数）
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「信頼の境界」の 2、「制限モードで保証すること」の R5、「保証しないこと」の「LLM の提供者へ送る内容」
- [評価](../../design/07-quality/07-02-evaluation.md)の「測る項目」の後の段落（処理系の起動の回数を数え、書いた後の検査の回数を分けること）
- [未決事項](../../design/open-issues.md)の BH-028、BH-029
- 既存のもの: P2-05 の `benitoite` モジュール（`Processor::query`、`QueryLimits`、`QueryOutput`、`BenitoiteError`、JSON の読み方、`benitoite/text.rs`）、P2-06 の `ToolSet::new`・`ToolSet::without_benitoite`、`definitions.rs` の `arg_described`、`examples/update_context_expected.rs`、`src/tools/`（`write.rs`・`edit.rs` の結果の作り方、`write_path.rs`、`paths.rs` の `open_file`・`search_path`・`requested_path`、`read.rs` の `MAX_FILE_BYTES`、`exclude.rs`、`definitions.rs`、`text.rs` の `wrote`）、`src/agent/mod.rs`（`Limits.max_check_fix`、「P1 では数えない」の注記、`AgentError` と `kind()`）、`src/agent/execution.rs`、`src/agent/events.rs`（`Outcome`）、`src/session/record.rs`（`Summary`）

## 前提（オーケストレータが確かめた事実）

2026-10-18 に、オーケストレータが手元の `v0.0.7` で次のことを確かめた。設計書の 01-03 と 04-01 にも同じ事実を書いてある。

- 診断の一行の欄は `kind`、`severity`、`code`、`message`、`primary{file,start{line,column,offset},end{…},label}`、`secondary[{file,start,end,label}]`、`notes[]`、`helps[{message,edits[{file,start,end,replacement}]}]` である。`relatedInformation` はない。`column` は 1 から数える Unicode のスカラー値の位置で、タブも 1 桁と数える。`offset` はバイトで数える。`file` は渡したパスの形のまま書かれる。
- 使い方の誤りを診断の形で書くときは、`code` と `primary` が `null` である。
- 終了状態は、警告だけなら 0（診断は標準エラー出力）、検査の誤りなら 2、使い方の誤りも 2（`error: directory … has no main.bnt` のような JSON でない文）である。`check` と `test --check` の標準出力は空である。
- `test --check <ディレクトリ>` は、`package.bnt` と下位のパッケージの中のものを除く、すべての `.bnt` を入口として検査する。テストも `main` も持たないファイル、`.env.bnt`、`.git/y.bnt` も含む。シンボリックリンクを辿らず、名前付きのパイプを飛ばす。
- 取り込んだモジュールの診断は、入口ごとに繰り返される。300 の入口で 302 行、121 KB になった。
- 処理系の不具合（BH-029）: 下のディレクトリにある入口が取り込んだモジュールの表示のパスが `<入口のディレクトリ>/<モジュールのパス>` になる（本物の `Lib/Other.bnt` のほかに、ない `Lib/Lib/Other.bnt` の診断が出る）。同じ誤った形が、報告の `sources.displayPath` と `edits.file` にも出る。
- 発見の失敗（読めない下のディレクトリ）は、`primary` が `null` の `E0101` を一件だけ書いて止まり、ほかのファイルを報告しない。
- `.env.bnt` に書いた識別子（`SECRET_TOKEN_abc`）が、診断の `message` に現れる。
- BH-028 の測定（macOS 27.0.1、Apple M4、`v0.0.7`、5 回の中央値、サンドボックスの外。Seatbelt の方針を掛けても変わらなかった）: 入口 10 で 0.083 秒、100 で 0.68 秒、300 で 1.98 秒、500 で 3.27 秒、1,000 で 6.45 秒。入口一つにつき約 6.4 ms で、入口の数に比例し、一つのスレッドで動く。300 の入口で最大のメモリは 19.7 MB。1 秒を約 150 の入口で、30 秒を約 4,600 の入口で超える見込み。

## この作業が認める型の変更

00-common の「止まるとき」に当たる次の変更は、この作業で行ってよい。10-interfaces はオーケストレータが更新する。

- `ToolId` に `BenitoiteCheck` を加える。
- `ToolOutput` に、LLM に送る文に含めない二つの欄を加える。
  - `check_launch: Option<CheckLaunch>`（`CheckLaunch::Tool` か `CheckLaunch::AfterWrite`）: 検査のために処理系を起動したら `Some` とする（結果が使い方の誤り、処理系の誤り、時間切れ、取り消しでも `Some`）。集計に使う（後述の 5）。
  - `check: Option<CheckOutcome>`（例: `CheckOutcome { key: PathBuf, failed: bool, diagnostics: String }`）: 検査と修正の回数を数える結果。`key` は数える対象の実のパス、`failed` は誤りを返したか、`diagnostics` は上限に達したときに利用者に示す、最後の検査の診断の文である。
- `AgentError` に `CheckFixLimit(String)` を加え、`kind()` は `"limit"` とする。`Outcome` と `cli` は変えない（既存の `AgentError::Limit` と同じ経路で終わる）。
- `QueryLimits` に標準エラー出力の上限を加える（例: `stderr: StderrLimit`。`StderrLimit::KeepFirst(64 KiB)` は今の扱い（先頭を保ち、残りを捨て、誤りにしない）、`StderrLimit::FailOver(16 MiB)` は超えたら読むのをやめて子プロセスを止め、誤りとする）。版と Skill の問い合わせは 64 KiB の今の扱いのまま、検査は 16 MiB とする。超えたことを表す `BenitoiteError` の変種を加えてよい。
- `session::record::Summary` に `after_write_checks: u64`（`#[serde(default)]`）を加える（後述の 5）。

## 作るもの

### 1. 診断の部品

`src/benitoite/diagnostics.rs` に、処理系の診断を読み、分類し、まとめ、04-01 の文の形に変える部品を作る。`benitoite` は `tools` に依存しない（設計書 06-01 の依存の向き）ので、パスを解く処理とソースの行を読む処理は、小さな trait（例: `DiagnosticSources`）で受け取り、`tools` の側が実装を渡す。

- **読み方**: 標準エラー出力の各行のうち、JSON のオブジェクトとして読めて、診断の欄（前提の一つ目）をすべて正しい型で持つ行を診断とする。`code` と `primary` は `null` を許す。欄が欠けるか型が違う行は、診断とせず出力の行とする（P2-08 の `run` と P2-12 の `test` でも使う。`kind` が `"die"` と `"runtime"` の報告がこの読み方で診断として読めるかは P2-08 で確かめる）。
- **終了状態の分類**（04-01 の表）: 関数（例: `classify_check(status, diagnostics, other_lines) -> CheckClass`）で、次のどれかに分ける。
  - 通った: 0 で診断がない。
  - 通った（警告あり）: 0 で診断が警告だけ。
  - 検査の誤り: 2 で、`code` と `primary` の少なくとも一方が `null` でない `error` の診断が一つ以上ある。位置が `null` の `error`（発見の失敗の `E0101` など）もここに入る。
  - 使い方の誤り: 2 で、`error` の診断が `code` と `primary` がともに `null` のものだけ（理由は最初のその診断の `message`）か、2 で診断の行がない（理由は標準エラー出力の最初の空でない行）。
  - 処理系の誤り: 上のどれでもないもの（0 で `error` の診断がある、2 で警告だけ、3、そのほかの値）と、`BenitoiteError`（時間切れ、16 MiB の超過など）。理由は一行とする。
- **パスの解き方**: 診断の `file` が相対のパスなら、プロジェクトのディレクトリ（処理系の作業ディレクトリ）に繋げる。`tools` の側は、そのパスを `ToolContext::open_file(path, false)` で開けるかで、`read` が開くファイルかを決める（読まないファイルの規則、読める範囲、ないファイル、通常のファイルでないものを、`read` と同じ判定で除く）。開けたときは、開いた記述子の実のパスを、プロジェクトの実のパスの下ならプロジェクトからの相対のパス、ほかは絶対のパスにして、示すパスとする。
- **重なりのまとめ**（04-01）: 重さ、コード、文、位置のファイルのパス（示すパス。開けないものは繋げた後のパス）、位置の始まりと終わりが同じ診断は、最初のものだけを残す。位置のファイルがない（繋げた後のパスを `symlink_metadata` で調べて、ないと分かる）診断は、位置のファイルがあるほかの診断と、重さ、コード、文、位置（行と桁の始まりと終わり）が同じなら除く。書いた後の検査と `benitoite_check` の両方に当てる。
- **読まないファイルの診断**（04-01）: 位置のファイルを `read` が開かない診断は、文で示さず数だけを数える。`secondary` と `edits` の要素のうち、ファイルを `read` が開かないものは示さない。位置が `null` の診断は、この規則に当たらず、文で示す。
- **文の形**（04-01）:
  - 見出しは `severity` で決める（`error[E0405] <path>:<line>:<column>: <message>`、`warning[W0501] ...`）。`code` が `null` なら角括弧を省き、`primary` が `null` なら位置を省く。
  - 位置の行のソースと `^` の印を添える。`primary` の `label` が空でなければ、`^` の印の後に空白を一つ置いて添える。
  - `secondary` は `note: <label> (<path>:<line>:<column>)`、`notes` は `note: <文>`、`helps` は `help: <message>` の行とし、この順に並べる。
  - 置き換えは `replace <line>:<column>-<line>:<column> with <replacement>` とし、`replacement` は JSON の文字列の形（`serde_json` で文字列として書いたもの）とする。`edits` の `file` が `primary` の `file` と違うときは、`replace <path>:<line>:<column>-<line>:<column> with ...` とする。置き換えは当てない。
- **ソースの行**（04-01）:
  - `tools` の側は、行を `ToolContext::open_file(_, false)` で開いた記述子から、`read::MAX_FILE_BYTES` までの範囲で読む。読んだファイルの版を記録しない（`read` の版の記録を呼ばない）。
  - 位置の行がファイルの行の数を超えるときは、行を添えない。
  - 2,000 文字を超える行は 2,000 文字で切り、切った先に `^` を付けない（範囲が全部切った先にあるときは、印の行を付けない）。
  - 桁は Unicode のスカラー値で数える。印の前の部分は、元の行のタブをタブのまま写し、ほかの文字を空白に置き換える。
  - 複数の行にわたる範囲は、最初の行の終わりまで印を付ける。幅のない範囲には `^` を一つ付ける。
- **件数と上限**: まとめて除いた後に、文で示す診断を `error` を先に出た順に並べ、最大 20 件を示す。残りがあれば `[<n> more diagnostics not shown. Fix the ones above, then check again.]`（一件は `[1 more diagnostic not shown. Fix the ones above, then check again.]`）を添える。読まないファイルの診断があれば、`<n> diagnostics in files that benitoiteh does not read are not shown.`（一件は `1 diagnostic in a file that benitoiteh does not read is not shown.`）を添える。
- 英文は `benitoite/text.rs` に置く。件数の行は、単数と複数の対を一つの関数で選ぶ。

### 2. `benitoite_check`

- 引数は `{ "path": "string", "tests": "boolean (optional)" }`。並べて実行してよい。`path` の説明は今の `text::argument` のものを使い、`tests` の説明は `arg_described` で ``Check without running tests and without requiring main, like benitoite test --check.`` とする。
- 道具の一覧: 両方のモードの `ToolSet::new` で、`bash`（制限モードにはない）の後、`benitoite_explain` の前に置く。`ToolSet::without_benitoite` には加えない。
- **パスの解き方**（01-03）: `path` を、そのモードの `read` と同じ規則で解く。パスがディレクトリ（リンクを辿った種類）なら、`search_path` と同じ判定（制限モードではプロジェクトの下に限り、読まないファイルの規則を当てる）を通した実のパスを渡す。ほかは `ToolContext::open_file(path, false)` で開き、開いた記述子の実のパスを渡す。判定の誤りは、`read` と同じ道具の誤りとして返す。
- `tests` が偽か省いたときは `check --diagnostics=json <実のパス>`、真のときは `test --check --diagnostics=json <実のパス>` を、P2-05 の `Processor::query` で起動する（サンドボックスの中、時間の上限 30 秒、標準エラー出力は `StderrLimit::FailOver(16 MiB)`、`Invocation.cancelled` を渡す）。
- **結果**（1 の分類による）:
  - 通った: `ok`。誤りの印なし。
  - 通った（警告あり）: 診断の文。誤りの印なし。
  - 検査の誤り: 診断の文。誤りの印あり。
  - 使い方の誤り、処理系の誤り: `Could not check <path> with the Benitoite processor: <reason>.`。誤りの印あり。`<path>` は LLM が渡した `path`、`<reason>` は一行の理由（`BenitoiteError` の表示なら末尾の `.` を除いたもの）とする。16 MiB を超えたときだけは、この形でなく `The diagnostics from benitoite exceeded 16 MiB and were not read. Check fewer or narrower paths.` を誤りとして返す。
  - `ToolContext` が処理系を持たないとき（本番では、道具が一覧にないので生じない。テストの組み合わせで生じる）: `Could not check <path> with the Benitoite processor: the processor is not available in this session.`。誤りの印あり。
- `check_launch` は、処理系を起動したら `Some(CheckLaunch::Tool)` とする。`check` は、通ったとき（警告ありを含む）に `failed: false`、検査の誤りのときに `failed: true` とし、`key` は処理系に渡した実のパスとする。使い方の誤り、処理系の誤り、取り消し、処理系がないときは `None` とする。
- 道具の説明の英文を `prompts/tools/benitoite_check.md` に置く（`prompt_parts` により、1 行目が説明、残りが規則）。

  ```text
  Check a Benitoite script, or the main.bnt of a directory, without running it.
  Set `tests` to true to check a file, or every .bnt file under a directory, without requiring `main` and without running tests (like `benitoite test --check`).
  A file passed directly resolves imports from its own directory; to check a module that imports others by names from the project root, pass the project directory with `tests` set to true.
  ```

### 3. 書いた後の検査（制限モード）

- 制限モードの `write` と `edit` が `.bnt` のファイルを書いた後に行う（01-03）。書けなかったとき、書く前に止まったとき（確認の拒否など）は行わない。通常モードでは行わない。
- `ToolContext` が処理系を持たないときは、検査せず、結果に何も足さない（`tests/tools_write.rs` は変えずに通る）。
- **検査しないとき**: 書いた結果の行（`Wrote <path> (<N> lines).`）の次の行に一行を足す。
  - 書いたファイルの実のパスがプロジェクトの実のパスの下にない（セッションの一時ディレクトリ）: `Not checked automatically: <path> is outside the project directory.`
  - 書いたファイルの親から、プロジェクトの根の手前までのディレクトリに `package.bnt` がある（シンボリックリンクを辿らずに調べる）: `Not checked automatically: <path> is in a nested package.`
- **検査するとき**: `test --check --diagnostics=json <プロジェクトの実のパス>` を、`benitoite_check` と同じ起動の形で起動する。書いた結果の行の次の行に、次のどれかを置く。`<path>` は `Wrote` の行と同じ表し方とする。
  - 書いたファイルに診断がなく、位置が `null` の `error` もないとき: `Check of <path> after this change: ok`
  - 書いたファイルに診断があるか、位置が `null` の `error` があるとき: `Check of <path> after this change:` の行に続けて、書いたファイルの診断と位置が `null` の診断を 1 の文の形で示す（20 件の上限と残りの件数の行も 1 のとおり）。
  - 使い方の誤り、処理系の誤り、時間切れ、取り消し: `Check of <path> after this change did not complete: <reason>.`。16 MiB を超えたときの `<reason>` は `the diagnostics from benitoite exceeded 16 MiB`、取り消しは `the check was cancelled` とする。
- 続けて、ほかのファイル（`read` が開くもの）の診断があれば `<n> diagnostics in other files. Use benitoite_check with tests=true on the project directory to see them.`（一件は `1 diagnostic in another file. Use benitoite_check with tests=true on the project directory to see it.`）を、`read` が開かないファイルの診断があれば 1 の件数の行を足す。書いたファイルを `read` が開かない（読まないファイルの名前で書いたなど）ときは、その診断も 1 の件数の行に入れる（見出しの行の後には、文の代わりにこの件数の行だけが続く）。
- 診断がどのファイルのものかは、1 のパスの解き方で開いた記述子の実のパスと、書いたファイルの実のパスを比べて決める。重なりのまとめは、ほかのファイルを数える前に行う（誤った表示のパスの診断を、ほかのファイルの件数にも、読まないファイルの件数にも入れない）。
- 結果には、検査の結果によらず誤りの印を付けない。
- `check_launch` は、起動したら `Some(CheckLaunch::AfterWrite)`。`check` は、検査を終えたとき（通った、警告あり、検査の誤り）に `key` を書いたファイルの実のパスとし、`failed` を、書いたファイルに `error` の診断がある（文で示さないものも含む）か、位置が `null` の `error` があるときに真とする。使い方の誤り、処理系の誤り、取り消し、検査しないとき、処理系がないときは `None` とする。
- 英文（`Check of ...`、`Not checked automatically: ...`、ほかのファイルの件数の行）は `tools/text.rs` に置く。

### 4. 検査と修正の回数（制限モード）

- 01-01 の数え方で数える。`ToolOutput.check` が `Some` で `failed` が真なら、その `key` の回数を 1 増やし、偽なら 0 に戻す。`None` は回数を変えない。数える状態は `Agent` が会話の外に持つ（圧縮で失われない。01-04 の「圧縮」）。セッションの記録から戻さないので、再開したセッションは 0 から数える。
- 制限モードだけで数える。通常モードの `benitoite_check` は数えない（集計は 5 のとおり両方のモードで数える）。
- いずれかの `key` の回数が `max_check_fix`（既定 10。`--max-check-fix` で変わる）に達したら、その番の道具の結果をすべて記録した後、次の要求を送る前に一連の処理を止める。止めるときは、その `key` の回数を 0 に戻し、`AgentError::CheckFixLimit` を返す。文は次のとおりとし、`agent/text.rs` に置く。`<path>` は `key` を示すパスの形（プロジェクトからの相対のパスか絶対のパス）にしたもの、`<n>` は回数、`<max>` は上限である。続く行に、最後の検査の `CheckOutcome.diagnostics` を置く。

  ```text
  The run stopped because checks of <path> failed <n> times without passing (limit: <max>, set by --max-check-fix). Remaining diagnostics:
  <最後の検査の診断>
  ```

- 止めたことは、既存の `AgentError::Limit` と同じ経路で、記録と画面と JSON Lines に出す。`Outcome` と `exec` の終了状態の写し方は変えない。
- `agent/mod.rs` の「P1 では数えない」の注記を除く。

### 5. セッションの集計

- `Summary.checks` と `Summary.fixes` は、今はどこでも埋めていない。この作業で次のとおり埋める（どちらのモードでも数える）。
  - `checks`: `ToolOutput.check_launch` が `Some` の数（検査のために処理系を起動した回数）。`benitoite_check` と書いた後の検査を含む。P2-08 が `ToolOutput.processor_launches`（報告の問い合わせと実行の起動の回数）を加え、`checks` をこの数との和にする。`benitoite_run`（P2-08）、`benitoite_test`（P2-12）、`benitoite_scratch`（P2-09）の起動がここに入る（07-02 の数え方）。
  - `after_write_checks`（新）: `checks` のうち `CheckLaunch::AfterWrite` の数。07-02 が、書いた後の検査の回数を分けて示すことを求めるので加える。
  - `fixes`: `ToolOutput.check` が `Some` で `failed` が真の数（検査が誤りを返した回数。その後に LLM が直すので、修正の回数とみなす）。
- 加える欄は `#[serde(default)]` とし、既存の記録を読めるようにする。

### 6. BH-028 の測り直し

- 前提の測定は、オーケストレータがサンドボックスの外で測ったものである。同じ形を、検査の起動と同じサンドボックスの中（P2-05 の問い合わせの方針）で測り直す。
- 合成のプロジェクトを作るスクリプトを `tools/bench-write-check.sh` などに置く。入口は、`package.bnt` と下位のパッケージの中のものを除くすべての `.bnt` である（テストを持つファイルに限らない）。入口の数 10、100、300、500、1,000 のディレクトリを作り、`test --check --diagnostics=json <ディレクトリ>` の時間の中央値（5 回）を測る。
- 報告に、OS、CPU、処理系の版、入口の数、時間の中央値の表と、前提の値との違いを書く。設計書は変えない（オーケストレータが BH-028 に反映する）。

## 触るファイル

- `crates/benitoiteh/src/benitoite/`（`diagnostics.rs`（新）、`mod.rs`、`text.rs`、問い合わせの `QueryLimits`）
- `crates/benitoiteh/src/tools/`（`benitoite_check.rs`（新）、`write.rs`、`edit.rs`、`mod.rs`、`definitions.rs`、`text.rs`、`paths.rs`）
- `crates/benitoiteh/prompts/tools/benitoite_check.md`（新）
- `crates/benitoiteh/src/agent/`（回数の状態、止め方、`AgentError`、`text.rs`、集計）
- `crates/benitoiteh/src/session/record.rs`（`Summary`）
- `crates/benitoiteh/examples/update_context_expected.rs` で作り直す `tests/expected/tools-restricted.json`・`tools-normal.json`・`system-restricted.txt`・`system-normal.txt`。`*-without-benitoite.*` は変わらないこと
- 関係するテスト、新しいテスト
- 測定のスクリプト（新）

## テスト

テストを書く前に、スキル `test-audit` の作成時の関門に従う。本物の処理系を要するテストは、P2-05 の `BENITOITEH_TEST_BENITOITE` を使う。

- **診断の文**（純粋な関数にバイト列を渡す）: 位置のある誤り、警告の見出し、`code` が `null` のもの、位置が `null` のもの、`label` の有無、`secondary`、`notes`、`helps` と `edits`（同じファイルと違うファイル、改行を含む置き換えの JSON の文字列の形）、21 件以上での打ち切りと残りの件数の行（一件と複数）、`error` を先にする並び。欄が欠けた JSON の行が出力の行になること。
- **終了状態の分類**: 表の各行（0 で診断なし、0 で警告だけ、2 で誤り、2 で位置が `null` の誤り、2 で使い方の誤りの形の診断だけ、2 で診断の行なし、0 で `error` あり、2 で警告だけ、3）。
- **偽の診断**（安全の性質のテスト。07-01）: 位置が、プロジェクトの外、`..` を含むパス、外を指す相対のパス、`.env` などの読まないファイル、外を指すシンボリックリンク、名前付きのパイプを指すとき、行を添えず、文で示さず件数だけになること。最後の行より後の行、2,000 文字を超える行（切った先に印がないこと）。行を添えた後に、そのファイルの読んだ版が記録されていないこと（`write` が `File has not been read in this session.` を返すこと）。
- **重なりと誤った表示のパス**（本物の処理系）: `Lib/Text.bnt` が `Lib.Other` を取り込むプロジェクトで `Lib/Other.bnt` を壊し、`benitoite_check` に `tests` を真にしてプロジェクトのディレクトリを渡すと、`Lib/Other.bnt` の誤りが一度だけ示され、`Lib/Lib/Other.bnt` が現れず、読まないファイルの件数の行も出ないこと。同じ形で `Lib/Other.bnt` を書いた後の検査でも、書いたファイルの誤りが一度だけで、ほかのファイルの件数に誤った表示のパスが入らないこと。
- **D1**（本物の処理系）: `.env.bnt` に誤りのある識別子を置いたプロジェクトで、`benitoite_check`（`tests` を真）と書いた後の検査が、読まないファイルの件数の行だけを示し、その識別子と `message` の文が結果のどこにも現れないこと。
- **`benitoite_check`**（本物の処理系）: 正しいスクリプトで `ok`。誤りのあるスクリプトで診断と誤りの印。警告だけのスクリプトで、誤りの印のない診断。`main.bnt` のないディレクトリを `tests` なしで渡すと、使い方の誤りの文（`Could not check ...: error: directory ...`）と誤りの印。`tests` を真にして、テストを持つファイルとディレクトリを検査すること。`-` で始まる名前のファイルが、オプションとして読まれないこと（実のパスで渡すこと）。発見の失敗（読めない下のディレクトリを作る）が `ok` にならないこと。処理系を持たない `ToolContext` で、処理系がない形の誤り。
- **テストを実行しないこと**（本物の処理系）: テストの中で、セッションの一時ディレクトリの絶対のパスにファイルを書くテスト（`File.Write`）を置き、`tests` を真にして検査しても、そのファイルができないこと。理由: 検査のサンドボックスはプロジェクトへの書き込みを拒むので、プロジェクトに書くテストでは、テストを実行しなかったことを確かめられない。
- **書いた後の検査**（本物の処理系）: 根からの名前で取り込む下のディレクトリのファイル（`Lib/Text.bnt` が `Lib.Other` を取り込む）が `ok` になること。書いたファイルの誤りが `Check of <path> after this change:` の見出しとともに返り、ほかのファイルの誤りは件数の行（一件と複数）だけになること。警告だけのとき。下位のパッケージの中のファイルで `Not checked automatically: ... nested package.` になること。一時ディレクトリに書いたとき `Not checked automatically: ... outside the project directory.` になり、処理系を起動しないこと。結果に誤りの印が付かないこと。通常モードで検査しないこと。処理系を持たない `ToolContext` で、`tests/tools_write.rs` の結果が変わらないこと（そのファイルを変えずに通ること）。
- **時間切れ**: 書いた後の検査が時間切れになると、`Wrote` の行と `Check of <path> after this change did not complete: <reason>.` の一行だけが返り、誤りの印がないこと。時間の上限は、モジュールの中のテストで短い値を渡して確かめ、30 秒を待たない（止まらない偽の処理系には `sandbox-probe` の操作を使ってよい）。
- **16 MiB の上限**: 標準エラー出力が上限を超えると、読むのをやめて子プロセスを止め、`benitoite_check` が上限の文を返すこと。版と Skill の問い合わせが 64 KiB の今の扱いのままであること。偽の出力には `sandbox-probe` の操作を使ってよい。
- **回数**: 同じスクリプトで 10 回誤りを返すと止まり、`AgentError::CheckFixLimit` の文に最後の診断が続くこと。途中で誤りなく通る（警告だけを含む）と 0 に戻ること。使い方の誤りと処理系の誤りが回数を変えないこと。別のスクリプトの回数が混ざらないこと。`--max-check-fix` で変えられること。止めた後に同じスクリプトが 0 から数えられること。通常モードで数えないこと。
- **集計**: `checks`、`after_write_checks`、`fixes` が、起動の回数と誤りの回数のとおりになること。`after_write_checks` のない既存の集計を読めること。
- **LLM に見せる文の固定**: `tests/expected/tools-*.json` と `system-*.txt` を作り直し、差分が `benitoite_check` の定義、説明、規則の分だけであること。`*-without-benitoite.*` が変わらないこと。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、道具の説明と、足した英文の全文、診断の文の例（入力の JSON と出力。警告、`label`、`secondary`、別のファイルの `edits` を含むもの）、期待のファイルの差分の要約、BH-028 の測り直しの表、実行ファイルの大きさの変化を書く。

## P2-06 の実装の後の補足（2026-10-18）

P2-05 と P2-06 を取り込んだ後に、この文書を実装に突き合わせた。上の記述と食い違うところは、この節を正とする。

- 既存のもの: `config::Limits.max_check_fix`（`--max-check-fix` と設定の `[limits] max_check_fix` は P1 で実装済み）、`AgentConfig.limits` の「P1 では数えない」の注記、`AgentError` と非公開の `kind()`。`json_object` は標準出力の全体を一つのオブジェクトとして読むもので、診断の行には使わず、`serde_json` で一行ずつ読む。P2-06 の部品: `benitoite::skill::query_reason`、`benitoite::text::skill_exit(code)`、`tools::text::SKILL_UNAVAILABLE`・`skill_failure`。`ToolContext` は処理系の取得口を持たず、公開の欄 `benitoite` と `skill` を持つ。道具の一覧は `Agent::new` が `tool_context.skill.is_some()` で選ぶ。`prompts/benitoite-mapping.md` の `benitoite_check` の行は P2-06 で入れてあるので、`system-*.txt` の差分は `<tools>` の一行と `<rules>` の行だけになる。
- `ToolOutput` は `Clone, Debug, Eq, PartialEq` を導くので、`CheckLaunch` と `CheckOutcome` も同じものを導く。`ToolOutput::success` と、構造体を全部書く `needs_approval` で、二つの欄を `None` にする。
- `QueryLimits { timeout, grace }`（`Copy`、`Default` は 10 秒と 10 秒）に `stderr: StderrLimit`（`Copy`）を加え、`Default` は `StderrLimit::KeepFirst(64 KiB)` とする。`src/benitoite/tests.rs` の構造体のリテラルを直す。超過の誤りの変種は `BenitoiteError::DiagnosticsLimit` とし、`query_reason` の網羅の match にも加える。超過の判定は `query.rs` の `Output.overflow` の仕組みを標準出力とは別の旗で使い、標準出力の判定と並べて、`RunEnd::Cancelled` と `Exit` への写しより前に置く。
- 終了状態の分類: `Processor::query` は終了状態が 0 でないとき `Err(BenitoiteError::Exit { code, stderr })` を返す。`Ok(output)` は終了状態 0 で `output.stderr` を読む。`Err(Exit { code: Some(c), stderr })` は終了状態 c でその `stderr` を読む。`Exit { code: None }` と `Exit` 以外の誤りは処理系の誤り（取り消しは取り消し）。
- `<reason>`: `Exit` は `skill_exit(code)` の文（名前を `exit_reason` に改めて共用してよい）、ほかは誤りの表示から末尾の `.` を除いたもの。検査の理由を作る関数は `query_reason` の隣に別に作る（`query_reason` は標準出力の超過を Skill の文にするため）。処理系を持たないときの文は `tools::text::SKILL_UNAVAILABLE` を使う（共用の名前に改めてよい）。
- `tests` の説明は `arg_described("tests", Boolean, false, text::CHECK_TESTS)` とし、文は `tools/text.rs` の定数に置く。
- `ToolSet::without_benitoite` が除く対象（`retain` の `matches!`）に `ToolId::BenitoiteCheck` を加える。
- 検査の問い合わせの口: `benitoite::CheckQuery`（`dyn Fn(&[OsString], QueryLimits, &AtomicBool) -> Result<QueryOutput, BenitoiteError> + Send + Sync`）を置き、`ToolContext` に `check_query: Option<Arc<CheckQuery>>` と `with_check_query` を置く。`exec` は `with_benitoite` と同じ所で `Processor::query` を包んだ口を渡す（`SkillDocuments::from_processor` と同じ形）。`benitoite_check` と書いた後の検査はこの口を使い、口がないことを「処理系を持たない」とする。理由: 統合テストは `Processor` を作れず（`Processor::inspect` は `pub(crate)`）、`sandbox-probe` は `check` の引数を受けられない。
- テストの分け方: 本物の処理系は、`tests/support/skill.rs` に倣い、作業ディレクトリをプロジェクトにして直接起動する口を `tests/support/` に置く。サンドボックスの中で起動することは、`tests/sandbox_exec_restricted.rs` の `Fixture::restricted` で、模擬の提供者が `.bnt` の `write` と `benitoite_check` を呼ぶテストを一つ置いて確かめる。時間切れと 16 MiB は、問い合わせの段（`benitoite/tests.rs`。`sandbox-probe` の `query-output 0 16777217 1000` と `FailOver(16 MiB)` で新しい変種になること、既存の `3 65537 0` が既定のまま 64 KiB を保つこと）と、道具の段（`TimedOut` と新しい変種を返す偽の口。口に渡された `QueryLimits`（30 秒、`FailOver(16 MiB)`）も確かめる）に分ける。回数のテストは `tests/agent_loop.rs` で、`Exit { code: Some(2), stderr: <診断の JSON> }` を返す偽の口を渡して確かめる。
- 回数は、すべての道具の結果が通る `execution.rs` の `Agent::result` で数える（`summary` を受け取る形にする）。止める判定は `run_request` の中で `execute_calls` を終えた後に置く。`summary.reason` と `AgentEvent::Error` の `message` には、止めた理由の一行（下の英文の一行目）だけを入れ、診断の全文は入れない（診断はその道具の結果にある）。
- 止めるときの英文は `The run stopped because checks of <path> failed <n> times without passing (limit: <max>; change it with --max-check-fix or limits.max_check_fix).` とする。`max_check_fix` の 0 は、引数でも設定でも誤り（1 以上）とする（「最初の誤りで止める」と「上限なし」のどちらにも読めるため。2026-10-18 に決めた）。
- 書いた後の検査: `edit` も `write::output(path, before, after)` で結果を作る（`edit.rs`）。`Wrote` の行の `<path>` は LLM が渡した文字列のまま（`text::wrote`）で、書いた実のパスは `WriteTarget.real_path` にある。足す行は `text` にだけ加え、`display` と `screen` は変えない。
- 触るファイルに加える: `src/benitoite/skill.rs`、`src/benitoite/tests.rs`、`src/tools/benitoite_reference.rs`（共用の名前に改めるとき）、`src/agent/execution.rs`、`src/cli/exec.rs`、`src/config/`（`max_check_fix` の 0 の誤り）、`tests/support/`、`tests/agent_loop.rs`。
