# P2-07 診断の受け渡し、`benitoite_check`、書いた後の検査、検査と修正の回数

## 目的

処理系の診断を LLM に返す文の形に変える部品を作り、道具 `benitoite_check`（`tests` の旗を含む）と、制限モードの書いた後の検査を作る（設計書 01-03 の「benitoite_check」「書いた後の検査（制限モード）」、04-01 の「終了状態と診断の受け渡し」）。制限モードの検査と修正の回数の上限（01-01 の「上限」）を数える。書いた後の検査の時間を大きなプロジェクトで測る（BH-028）。

## 読む設計書

- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「処理系の起動」（表の「検査」「テストの検査」の行、検査の時間の上限 30 秒、絶対の実のパス、検査をサンドボックスの中で起動すること）「終了状態と診断の受け渡し」（診断の文の形、`code` が `null` の報告、位置の行を `read` と同じ規則で読むこと、修正案を当てないこと、20 件の上限）
- [道具](../../design/01-agent/01-03-tools.md)の「書いた後の検査（制限モード）」（プロジェクトのディレクトリへの `test --check`、書いたファイルの診断とほかのファイルの件数、下位のパッケージの扱い、誤りの印を付けないこと）「benitoite_check」「道具の定義の共通の規則」「読まないファイル」「出力の切り詰め」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「上限」（検査と修正の回数）
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「信頼の境界」の 2
- [未決事項](../../design/open-issues.md)の BH-028
- 既存のもの: P2-05 の `benitoite` モジュール（問い合わせの起動、方針、JSON の読み方）、`src/tools/`（`write.rs`・`edit.rs` の結果の作り方、`write_path.rs`、`paths.rs` の読む範囲の判定、`read.rs`、`exclude.rs`、`definitions.rs`、`text.rs`）、`src/agent/mod.rs`（`Limits.max_check_fix`、「P1 では数えない」の注記）、`src/agent/execution.rs`、`src/session/`（記録の行）

## この作業が認める型の変更

- `ToolId` に `BenitoiteCheck` を加える。
- `ToolOutput` に、ループが検査と修正の回数を数えるための、検査の結果の欄（例: `check: Option<CheckOutcome { key: PathBuf, failed: bool }>`）を加える。欄は LLM に送る文に含めない。
- 上限に達したことを表す `Outcome` の値（例: `Outcome::CheckFixLimit`）を加え、`exec` の終了状態（05-01 の「終了状態」の 1。上限に達した）と JSON Lines の出力に写す。

## 作るもの

### 1. 診断の文

- `src/benitoite/diagnostics.rs` に、処理系の診断（標準エラー出力の一行一件の JSON と、報告の `diagnostics` の配列の要素。欄は同じ）を読み、04-01 の文の形（`error[E0405] path:line:col: message`、ソースの行と `^` の印、`note:`、`help:` と置き換えの範囲）に変える部品を作る。
  - 標準エラー出力の各行のうち、JSON のオブジェクトとして読めて `kind` を持つ行を診断とし、ほかの行は出力として返す（P2-08 の `run` でも使う）。
  - `code` が `null` のもの、位置が `null` のものの形。
  - `error` を先に、出た順に最大 20 件を示し、残りの件数を一行で示す。
  - 位置の行は、そのモードの `read` と同じ規則（読める範囲、読まないファイル、パスの解き方）で、通常のファイルの一行（2,000 文字まで）だけを読む。規則に当たるか、位置がファイルの中にないときは、行を添えない。行の読み方は、`tools` の範囲の判定の関数を使い、新しい読み方を作らない。`benitoite` は `tools` に依存しない（設計書 06-01 の依存の向き）ので、診断の文を作る関数は、行を読む関数を引数（クロージャか小さな trait）で受け取り、`tools` の側が渡す。
  - 修正案（`helps` の `edits`）は、置き換えの範囲と文字列のまま示し、当てない。
- 英文（件数の行など）は `benitoite/text.rs` に置く。

### 2. `benitoite_check`

- 引数は `{ "path": "string", "tests": "boolean (optional)" }`。両方のモードで使う（通常モードで処理系を使えないときは外す）。並べて実行してよい。
- `path` は、そのモードの `read` と同じ規則で解き、シンボリックリンクを解いた絶対の実のパスで処理系に渡す。
- `tests` が偽か省いたときは `check --diagnostics=json <path>`、真のときは `test --check --diagnostics=json <path>` を、P2-05 の問い合わせの起動（サンドボックスの中、時間の上限 30 秒）で起動する。誤りも警告もなければ `ok`、あれば 1 の診断の文を返す。終了状態は 04-01 の定めで解釈し、検査の誤り（2）は道具の誤りの印を付けて診断を返す。使い方の誤りと処理系の不具合（3）は、理由を一行で返す。
- 道具の説明の英文を `prompts/tools/benitoite_check.md` に置く。案は次のとおりとし、オーケストレータが取り込む前に見直す（01-03 の、直接指定したファイルは親のディレクトリを根として取り込みを解くことの注意を含める）。

  ```text
  Check a Benitoite script, or a directory's main.bnt, without running it.
  Set `tests` to true to check test files or a directory of tests without running them (like `benitoite test --check`).
  A file named directly resolves imports from its own folder; to check a module that imports others from the project root, pass the project directory with `tests` set to true.
  ```

### 3. 書いた後の検査（制限モード）

- 制限モードの `write` と `edit` が `.bnt` のファイルを書いた後に、`test --check --diagnostics=json <プロジェクトのディレクトリ>` を起動する（01-03）。書けなかったとき、書く前に止まったとき（確認の拒否など）は検査しない。
- 結果は、書いたファイルの診断（1 の形）と、ほかのファイルの診断の件数とする。英文は 01-03 のもの（`Check of <path> after this change: ok`、`<n> diagnostics in other files. Use benitoite_check with tests=true on the project directory to see them.`、`Not checked automatically: <path> is in a nested package.`）を字句を変えずに `tools/text.rs` に置く。診断がどのファイルのものかは、診断の位置のパスと書いたファイルの実のパスを比べて決める（パスの表し方は `v0.0.7` の出力で確かめ、報告に書く）。
- 下位のパッケージの判定: 書いたファイルの親から、プロジェクトの根の手前までのディレクトリに `package.bnt` があれば、下位のパッケージの中とする（シンボリックリンクを辿らずに調べる）。
- 検査の結果が誤りでも、書く道具の結果に誤りの印を付けない。処理系の誤り（時間切れ、読めない出力など）は、書いたことを示したうえで一行添える。
- 通常モードでは書いた後の検査をしない。

### 4. 検査と修正の回数

- 01-01 の数え方で数える。同じスクリプト（`benitoite_check` では `path` の実のパス、書いた後の検査では書いたファイルの実のパス）について、検査が誤りを返した回数を数え、誤りなく通ったら 0 に戻す。数える状態は `Agent` が会話の外に持つ（圧縮で失われない。01-04 の「圧縮」）。
- いずれかのスクリプトの回数が `max_check_fix`（既定 10）に達したら、その番の道具の結果を返した後に一連の処理を止め、残る診断（最後の検査の結果）とともに、上限に達したことを利用者に示す（英文は `agent/text.rs`）。止めたことを記録に書く。
- 制限モードだけで数える。通常モードの `benitoite_check` は数えない。
- 「P1 では数えない」の注記を除く。

### 5. BH-028 の測定

- 入口のファイル（`main.bnt` か、テストを持つファイル）を 10、100、500、1,000 持つ合成のプロジェクトを作るスクリプト（`tools/` の下か `work/` に置く。リポジトリに置くなら `tools/bench-write-check.sh` など）で、`test --check --diagnostics=json <ディレクトリ>` の時間を、サンドボックスの中で測る。macOS で測り、報告に表で書く（OS、CPU、処理系の版、入口の数、時間の中央値）。30 秒の上限と、書く道具の応答として許せる時間（目安 1 秒）に収まるかを書く。設計書は変えない（BH-028 の結果はオーケストレータが反映する）。

## 触るファイル

- `crates/benitoiteh/src/benitoite/`（`diagnostics.rs`（新）、`text.rs`）
- `crates/benitoiteh/src/tools/`（`benitoite_check.rs`（新）、`write.rs`、`edit.rs`、`mod.rs`、`definitions.rs`、`text.rs`）
- `crates/benitoiteh/prompts/tools/benitoite_check.md`（新）
- `crates/benitoiteh/src/agent/`（回数の状態、止め方、`text.rs`）
- `crates/benitoiteh/src/cli/`（終了状態と出力への写し）
- `crates/benitoiteh/tests/expected/`、関係するテスト、新しいテスト
- 測定のスクリプト（新）

## テスト

- 診断の文: 位置のある誤り、`code` が `null` のもの、位置が `null` のもの、`notes`・`helps`・`edits`、21 件以上での打ち切りと件数、`error` を先にする並び。
- 偽の診断（安全の性質のテスト。07-01）: 診断の位置が、読めない場所（プロジェクトの外、`.env` などの読まないファイル、シンボリックリンクで外を指すもの、名前付きのパイプ）を指しても、行を添えないこと。
- `benitoite_check`（本物の処理系）: 正しいスクリプトで `ok`、誤りのあるスクリプトで診断、`tests` の真で、テストを持つファイルとディレクトリの検査。`-` で始まる名前のファイルが、オプションとして読まれないこと（実のパスで渡すこと）。検査がテストを実行しないこと（テストの中でファイルを書くテストに `tests` を真で掛け、ファイルができないこと）。
- 書いた後の検査（本物の処理系）: 根からの名前で取り込む下のディレクトリのファイル（`Lib/Text.bnt` が `Lib.Other` を取り込む）が `ok` になること。書いたファイルの誤りが返り、ほかのファイルの誤りは件数だけになること。下位のパッケージの中のファイルで `Not checked automatically` になること。結果に誤りの印が付かないこと。通常モードで検査しないこと。
- 回数: 同じスクリプトで 10 回誤りを返すと止まり、途中で誤りなく通れば 0 に戻ること。別のスクリプトの回数が混ざらないこと。`--max-check-fix` で変えられること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、道具の説明と、足した英文の全文、診断の文の例（入力の JSON と出力）、BH-028 の測定の表、実行ファイルの大きさの変化を書く。
