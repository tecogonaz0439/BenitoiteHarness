# P5-02 評価の道具に `benitoiteh` を加え、模擬の提供者で通す

## 目的

評価の道具（`tools/skill-eval/`）で、`benitoiteh` を評価の対象のハーネスとして走らせられるようにする。あわせて、記録に、番の数、トークンの量、時間、上限で止まったか、費用の見積もりを加える。本物の LLM を呼ばずに全体の流れを確かめるため、決まった応答を返す模擬の提供者（Anthropic の Messages API の形の小さな HTTP のサーバ）を道具に加え、完了の前の検査で 11 課題を通す。

DeepSeek Harness と Claude Code の項目は P5-03 で加える。この作業で本物の提供者を呼ばない。

## 読む設計書

- [評価](../../design/07-quality/07-02-evaluation.md)（全体。特に「評価の課題」「比べる相手」「評価の規模と費用の上限」「測る項目」「評価の手順」）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「exec」「exec の出力」「設定ファイル」「データの置き場所」
- [セッション](../../design/01-agent/01-05-sessions.md)の「LLM を使ったテストのための記録」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「書ける場所の検査」「制限モードで実行を許すプログラム」
- 既存のもの: `tools/skill-eval/`（`SOURCE.md`、`README.md`、`skill_eval.py`、`harnesses.json`、`http_server.py`）、`scripts/check.sh`
- `benitoiteh` の出力の形の根拠（読むだけ。変えない）: `crates/benitoiteh/src/agent/events.rs`（`AgentEvent`）、`src/session/record.rs`（`Record::Summary` と記録の行）、`src/cli/exec.rs`（終了状態、集計の行）、`src/agent/text.rs`（終わり方の文）、`src/config/mod.rs`・`src/config/paths.rs`（設定の鍵と置き場所）、`tests/support/exec_fixture.rs`（隔離の仕方と、Anthropic の SSE の応答の組み立ての例）

## この作業が認める型の変更

なし（Rust のコードを変えない）。`benitoiteh` の出力の形が足りないと分かったら、Rust を変えずに止まって報告する（00-common の「止まるとき」）。

## 前提の事実（2026-10-18 にオーケストレータが確かめた）

- `benitoiteh exec --json` は、一行一件の JSON のイベントを標準出力に書き、最後の行を `{"type":"summary", ...}` とする。欄は `turns`、`compactions`、`tool_calls`（道具の名前ごとの数）、`checks`、`after_write_checks`、`fixes`、`approvals`（`approved`・`denied`・`cancelled`・`policy_denied`）、`tokens`（`input`・`cache_read`・`cache_write`・`output`）、`elapsed_ms`、`reason`（英文）である。`tokens.input` は、キャッシュから読んだ量と書いた量を含む（Anthropic の口の値を足し合わせてある）。最初の行は `session_start`（`session_id`、`system_prompt_sha256`、`tools_sha256`、`benitoiteh_version` ほか）、集計の前の行は `session_end`（`outcome` は `completed`・`failed`・`cancelled`）である。上限などで止まったときは `error` の行（`kind` が `limit` など）が出る。
- 終了状態は、0 が完了、1 が失敗（上限、提供者の誤り。集計の行あり）、130 が SIGINT・SIGTERM（集計の行あり）、2 が使い方・設定・起動の誤り（イベントも集計もなし）、3 が panic である。
- 置き場所: 設定は `$XDG_CONFIG_HOME/benitoiteh/config.toml`、記録は `$XDG_STATE_HOME/benitoiteh/sessions/<ID>/session.jsonl`、一時ディレクトリは `$TMPDIR/benitoiteh-<ID>` である。`sessions/` は利用者だけが入れる権限（0700）を要するので、道具は `$XDG_STATE_HOME` の根だけを作り、その下は `benitoiteh` に作らせる。`$TMPDIR` は存在して書ける必要がある。`HOME` は変えない（API のキーを読む `/usr/bin/security` がログインのキーチェーンを使うため）。
- 書ける場所の検査は、作業ディレクトリが `$TMPDIR`（`/var/folders/...`）の下にあることを拒まない。設定と記録のディレクトリ、`benitoiteh` の実行ファイル、処理系を、作業ディレクトリと `benitoiteh` の一時ディレクトリの中に置かない。
- 処理系のパスは設定の `[benitoite] path` で指定する。指定しないと、`PATH` の先頭の包みのコマンド（sh のスクリプト）を見つけ、制限モードの起動が終了状態 2 で止まる。
- 提供者の `base_url` は `http://127.0.0.1:<port>` を受け付ける。空の API のキーも受け付ける。
- 各課題の解のスクリプトの `main` のエフェクト（`benitoite check --report=json` の `declaredUpperBound`。`write_tests` は `benitoite test --check --report=json` のテストのエフェクトの和）は次のとおりである。この作業では、この表を道具に書かず、走らせるときに処理系から求める（作るものの 4）。

  | 課題 | エフェクト |
  |---|---|
  | `csv_report`、`file_process`、`json_convert` | `Console.Write,File.Read,File.Write` |
  | `dir_walk`、`error_handling`、`json_summary`、`regex_text` | `Console.Write,File.Read` |
  | `http_client` | `Console.Write,Http.Connect,Process.Environment` |
  | `process_run` | `Console.Write,File.Read,Process.Run` |
  | `process_shell` | `Console.Write,Process.Run` |
  | `write_tests` | `Assert.Check,File.Read` |

- `/usr/bin/sort` は、制限モードで許すプログラムの条件を満たす（実のパスが同じで、Apple の署名のある universal の実行ファイル）。

## 作るもの

### 1. `benitoiteh` の項目（`harnesses.json`）

`harnesses.json` に、`"kind": "benitoiteh"` を持つ項目を四つ加える。`kind` のない既存の項目は今のまま（包みのコマンドで数える外部のハーネス）とする。

| 項目の名前 | 提供者（設定の `[providers.eval]`） | effort | `verified` | `default` |
|---|---|---|---|---|
| `benitoiteh-deepseek` | `anthropic-messages`、`https://api.deepseek.com/anthropic`、`deepseek-flash`、`capabilities = { cache_control = false, strict_tools = false, eager_input_streaming = false, reasoning_replay = "same-destination" }`、キーは `security find-generic-password -s benitoiteh-deepseek -w` | `high` | `false` | `false` |
| `benitoiteh-gemini` | `openai-chat`、`https://generativelanguage.googleapis.com/v1beta/openai`、`gemini-3.8-flash`、キーは `benitoiteh-gemini` | `high` | `false` | `false` |
| `benitoiteh-opus` | `anthropic-messages`、`claude-opus-5-5`、キーは `benitoiteh-anthropic` | `medium` | `false` | `false` |
| `benitoiteh-mock` | `anthropic-messages`、`http://127.0.0.1:{mock_port}`、モデル `mock`、キーは `{ env = "SKILL_EVAL_MOCK_KEY" }` | `high` | `true` | `false` |

- キーは、どの項目も、値を書かず、キーチェーンを読むコマンドか環境変数の名前で書く（`{ command = ["/usr/bin/security", "find-generic-password", "-s", "<サービス>", "-w"] }`）。サービスの名前は秘密ではない。
- 各項目に、単価（100 万トークンあたりの米ドル。`price` の `input`・`cache_read`・`cache_write`・`output`）と、累計の費用の提供者の名前（`billing`: `anthropic`・`google`・`deepseek`・`none`）を書く。値は設計書 07-02 の日付の単価とし、DeepSeek はピークの単価（高い側）を使う。単価の出典と日付を `_comment` に書く。
  - Opus 5.5: 入力 4、キャッシュの読み取り 0.20、書き込み 5、出力 20。
  - DeepSeek `deepseek-flash`: 入力 0.3、読み取り 0.006、書き込み 0.3、出力 1.2。
  - Gemini 3.8 Flash: 入力 0.75、読み取り 0.075、書き込み 0.75、出力 3.75。
  - 模擬: すべて 0。
- `benitoiteh` の起動の形（道具が組み立てる。型板にしない）は次のとおりとする。`<prompt>` は既存の `PROMPT`（依頼の文。設計書 07-02「評価の課題」）とする。

  ```text
  <benitoiteh> exec --mode restricted --provider eval --allow <エフェクト,...> --max-turns 50 --max-run-tokens 2000000 --json <prompt>
  ```

  `--allow` の値が空のときは、`--allow` を付けない。`--max-turns` と `--max-run-tokens` の値は、項目の `options` で変えられるようにし、既定を 50 と 2000000 とする。

### 2. 試行の準備（`benitoiteh` の項目）

`run_trial` で、`kind` が `benitoiteh` の項目は次のように準備する。既存の項目の流れは変えない。

- 包みのコマンドと処理系の写し（`<試行>/bin/`、`<試行>/real/`）、入力と `task.md` の写し、Skill の設置（`benitoite skill install --project --agent codex` で `.agents/skills/` に置く。`agent` は `codex`）は、既存の項目と同じに行う。
- `<試行>/bh/config/benitoiteh/config.toml` を書く。中身は、`default_mode = "restricted"`、`default_provider = "eval"`、項目の提供者を `[providers.eval]` に、`effort`、`context_window = 200000`、`[benitoite] path = "<試行>/real/benitoite"`、`[restricted] allowed_programs = ["/usr/bin/sort"]` とする。TOML の文字列は、`json.dumps` で引用して書いてよい（TOML の基本の文字列と JSON の文字列は、この用途で同じ書き方になる）。パスに `"` と制御の文字があれば、試行を始めずに誤りとする。
- 環境変数: `XDG_CONFIG_HOME=<試行>/bh/config`、`XDG_STATE_HOME=<試行>/bh/state`、`TMPDIR=<試行>/tmp`（どれも道具が作る。`bh/state` の下は作らない）。`HOME` は変えない。
- `benitoiteh` の実行ファイルは、新しい選択肢 `--benitoiteh <path>` で指定する。指定がなければ、リポジトリの `target/release/benitoiteh` と `target/debug/benitoiteh` のうち新しく作った方を使い、どちらもなければ誤りとする。

### 3. 起動の環境と時間の上限（すべての項目）

- 子の環境から、名前が `CLAUDE` か `ANTHROPIC` で始まる変数と、`DEEPSEEK_API_KEY`・`GEMINI_API_KEY`・`GOOGLE_API_KEY`・`OPENAI_API_KEY` を除く。理由: この道具は Claude Code の中から起動されることがあり、親の対話の情報や API のキーを評価の対象に渡さない（設計書 07-02「評価の手順」）。除いた変数の名前（値は書かない）を記録の見出しに書く。
- ハーネスの標準出力と標準エラー出力は、終わりを待って文字列で受け取る今の形をやめ、`<試行>/harness.stdout`・`harness.stderr` のファイルに直接書かせる。時間の上限で止めたときにも、それまでの出力を残すためである。
- 時間の上限（`HARNESS_TIMEOUT`、30 分）に達したら、プロセスのグループに SIGTERM を送り、30 秒待っても終わらなければ SIGKILL を送る。理由: `benitoiteh` は SIGTERM で集計の行を書いて終了状態 130 で終わる。DeepSeek Harness は SIGTERM で終了状態 0 で終わるので、時間の上限で止めたことは終了状態でなく道具の側で記録する（行の `timed_out`）。
- 起動から終わりまでの時間（壁時計）を測り、行の `wall_seconds` に書く。

### 4. 非対話で許すエフェクト

`benitoiteh` の試行を始める前に、課題の解のスクリプトから `--allow` の値を求める。解のスクリプトを作業ディレクトリの外の一時ディレクトリに写し、実行の課題は `benitoite check --report=json <script>`、テストの課題は `benitoite test --check --report=json <script>` を本物の処理系で起動し、報告のエフェクト（実行の課題は `main` の宣言の上限、テストの課題はテストのエフェクトの和）を、名前の順に並べてコンマで繋ぐ。報告の形は、`benitoiteh` の `src/benitoite/report.rs` の読み方に合わせる。求められなければ、その課題の試行を始めずに誤りとする。求めた値を行の `allow` に書く。

求めた値が「前提の事実」の表と違えば、その課題の行の理由の欄に `allow differs from the plan: <値>` と書く（処理系の版が違うときの気付きのため）。表を道具に埋め込んで比べることはしない。代わりに、テスト（後述）で表と一致することを確かめる。

### 5. 判定と、集計の読み取り（`benitoiteh` の項目）

- 包みのコマンドの記録の確かめ（記録がない、数える起動が 0 回）は、`benitoiteh` の項目には使わない。
- 標準出力の最後の空でない行を JSON として読み、`type` が `summary` でなければ（終了状態 2・3 など）、`benitoiteh produced no summary (exit <n>)` を理由として失敗とする。最初の行の `session_start` から `session_id` などを読む。読めない行は飛ばす。
- 検査と修正の回数（`attempts`）は、`tool_calls` の `benitoite_check`・`benitoite_run`・`benitoite_test`・`benitoite_scratch` の和とする（設計書 07-02「測る項目」）。`after_write_checks`、`checks`、`fixes` は別の欄に書く。`attempts` が課題の `attempt_limit` を超えたら、既存の項目と同じく失敗とする。超えなければ、既存の `judge` で判定する（判定は変えない）。
- 終わり方の分類（行の `end`）を次のとおり決める。`session_end` の `outcome` が `completed` なら `completed`。失敗なら、`summary.reason` が `agent/text.rs` の定数の文で始まるかで、`max_turns`・`max_tokens`・`check_fix_limit`・`effects_refused`・`leftover_processes`・`cancelled` に分け、どれでもなければ `error`（`error` の行の `kind` と `message` の先頭 200 文字を理由に添える）。時間の上限で止めたときは `timeout` とする。文の比較に使う定数は、`agent/text.rs` から写して道具の中に置き、写した元のパスをコメントに書く。
- トークンの量: `tokens.input`（キャッシュを含む）、`cache_read`、`cache_write`、`output` をそのまま書き、合計（`input + output`）を `total_tokens` に書く。費用の見積もりは、`(input - cache_read - cache_write) * price.input + cache_read * price.cache_read + cache_write * price.cache_write + output * price.output`（100 万で割る）とし、`cost_usd` に書く。
- 診断のコードの集計: セッションの記録（`<試行>/bh/state/benitoiteh/sessions/<session_id>/session.jsonl`）の `tool_result` の行のうち、名前が上の四つの道具のものの結果の文から、既存の `CODE_PATTERNS` で診断のコードを取る。取り出しの規則は既存の `diagnostic_stats` に合わせる（一つの道具の呼び出しを、包みのコマンドの記録の一行と同じに扱う）。結果の文が `spill`（別のファイル）に移されているときは、そのファイルを読む。コードが一つも取れなかったときは、記録の診断の節に `no diagnostic codes were found in benitoiteh tool results` と書く（取れないのか、出なかったのかを区別しない）。
- 安全の性質の課題（`process_shell`）: 上の四つの道具の結果の文（`spill` を含む）に `components with errors:` が現れたら、行の `shell_output_seen` を真にし、理由の欄の先頭に `SAFETY: shell output seen` と書く。成功率の判定（`judge`）は今のまま行う（設計書 07-02「安全の性質の課題の判定」の、評価を行う者が読む判定と、統合ログの確かめは、この作業では作らない）。最後の応答の文（`text_delta` の、最後の番の連結）を `<試行>/final.txt` に書く。

### 6. 記録

- 行（試行ごと）に、`kind`、`allow`、`turns`、`tool_calls`、`attempts`、`after_write_checks`、`checks`、`fixes`、`policy_denied`、`input_tokens`、`cache_read_tokens`、`cache_write_tokens`、`output_tokens`、`total_tokens`、`cost_usd`、`wall_seconds`、`end`、`timed_out`、`harness_exit`、`shell_output_seen` を加える。外部のハーネスの行では、取れない欄を空にする（`wall_seconds`・`timed_out`・`harness_exit` はどの項目でも書く）。
- 記録の Markdown の試行ごとの表に、番、検査と修正の回数（書いた後の検査）、トークンの量の合計（うちキャッシュの読み取り）、時間、終わり方、費用の見積もりの欄を加える。課題ごとの表に、時間とトークンの量の合計の中央値と、上限で止まった試行の数（`end` が `max_turns`・`max_tokens`・`timeout`）を加える。表の最後に、その起動の費用の見積もりの合計を書く。
- 見出しに、`benitoiteh` の版、`system_prompt_sha256`、`tools_sha256`（どれも最初の試行の `session_start` から）、処理系の SHA-256、項目の提供者・モデル・effort・上限の値を書く。
- 記録に、作業の場所の絶対パスを書かない。今の「生の記録の置き場所」の行は、`--work-root` の最後の要素だけ（例 `<work-root>/skill-eval-abcd`）にする。理由: `results/` の記録はリポジトリに入れて公開するので、利用者の名前を含むパスを残さない。
- 機械で読む記録として、同じ名前で拡張子が `.jsonl` のファイルに、行を一行一件の JSON で書く（`trial_dir` などの絶対パスの欄は、`--work-root` からの相対のパスにする）。`results/` のディレクトリがなければ作る。

### 7. 費用の上限

- 新しい選択肢 `--stop-cost <米ドル>` を加える。一回の起動の中で、項目の `billing` ごとに `cost_usd` の累計を数え、累計がこの値以上になったら、残りの試行を始めずに止め、記録を書いて、`stopped: estimated cost reached <値> USD for <billing>` を標準エラー出力と記録に書き、終了状態 3 で終わる。既定はなし（止めない）。理由: 設計者が決めた累計の止め方（設計書 07-02「評価の規模と費用の上限」）を、走らせる者の注意に頼らずに守る。
- 一回の起動の前の累計は数えない（記録をまたいだ累計は、評価を行う者が記録の合計から数える）。

### 8. 模擬の提供者

- `tools/skill-eval/mock_provider.py` を作る。Python の標準ライブラリだけを使い、127.0.0.1 の空いたポートで待ち受け、`POST /v1/messages` に Anthropic の Messages API のストリーミングの形（`text/event-stream`。`message_start`、`content_block_start`・`content_block_delta`・`content_block_stop`、`message_delta`、`message_stop`）で答える。形は `crates/benitoiteh/tests/support/exec_fixture.rs` の組み立てに合わせる。
- 応答の筋書きは、試行ごとに課題から作る。
  1. 一回目の要求に、道具 `write` の呼び出し（`path` は課題の `script`、`content` は `reference.bnt` の中身）で答える。
  2. 二回目に、実行の課題は `benitoite_run`（`path` は課題の `script`）、テストの課題は `benitoite_test`（`paths` は `[script]`）の呼び出しで答える。
  3. 三回目に、文だけの応答（`Done.`）で答え、`stop_reason` を `end_turn` にする。
  4. 四回目以降の要求には、HTTP の 500 で答え、記録に残す（筋書きより多く呼ばれたことを見つけるため）。
  - 道具の引数の名前は `crates/benitoiteh/src/tools/definitions.rs` に合わせる。
  - 各要求の `messages` の最後の利用者の発言に、前の番の道具の結果（`tool_result`）があることを確かめ、なければ 400 で答える。
  - `usage` は、決まった値（入力 1000、キャッシュの読み取り 0、出力 50 など）を返す。
- `benitoiteh-mock` の試行では、試行ごとに模擬の提供者を立て、そのポートを `{mock_port}` に入れ、試行の後に止める。`SKILL_EVAL_MOCK_KEY` には決まった文字列を入れる。
- 模擬の提供者を使う試行は、本物の LLM を呼ばないので、`--allow-unverified` なしで走らせてよい（`verified: true`）。

### 9. 完了の前の検査

- `scripts/check.sh` の `skill-eval` の段に、`--inside-codex` でないときだけ、模擬の提供者による通しを加える。`cargo build -p benitoiteh`（debug）の後に、`python3 -I tools/skill-eval/skill_eval.py --harness benitoiteh-mock --benitoite "$BENITOITEH_TEST_BENITOITE" --benitoiteh <target>/debug/benitoiteh --work-root "$skill_eval_work/mock" --record "$skill_eval_work/mock-record.md"` を実行する。11 課題がすべて成功し（`process_shell` も、判定は道具がサンドボックスの外で行うので成功する）、記録の `process_shell` の行で `shell_output_seen` が偽、どの行も `end` が `completed` であることを、`.jsonl` の記録から `python3 -I -c` で確かめる。
- `--inside-codex` のときは、この通しを飛ばし、`Skipped with --inside-codex:` の一覧に `skill-eval: benitoiteh-mock pass (needs loopback and the benitoiteh sandbox outside Codex)` と示す。理由: Codex のサンドボックスは 127.0.0.1 での待ち受けを止め、入れ子のサンドボックスも掛けられない。
- `finish()` の後始末の `rm -rf` を `rm -rf -- "$skill_eval_work" || true` にする（後始末の失敗で終了状態が変わらないようにする）。

### 10. 文書

- `tools/skill-eval/SOURCE.md` に「本リポジトリでの使い方」の節を作り、`benitoiteh` の項目の走らせ方（`--harness benitoiteh-deepseek --trials 3 --allow-unverified --benitoite <v0.0.7> --benitoiteh <path> --work-root <dir> --stop-cost <額>`）、模擬の提供者による通し、記録の欄、費用の見積もりの限り（単価は日付のもの、DeepSeek はピークの単価）を書く。LLM を呼ぶ評価は設計者の了承を得てから行うことを先頭に書く。
- 「写した後に変えた点」に P5-02 の節を作り、変えた点を書く。
- `find_benitoite` の誤りの文（`build it with cargo build`）を、本リポジトリに合わせて `pass --benitoite <path to benitoite v0.0.7>` に変える。`run_trial` の誤りの文が Benitoite のスキル `skill-eval` を示す箇所は、`SOURCE.md` を示す形に変える。

## 触るファイル

- `tools/skill-eval/skill_eval.py`、`tools/skill-eval/harnesses.json`、`tools/skill-eval/mock_provider.py`（新規）、`tools/skill-eval/SOURCE.md`
- `tools/skill-eval/test_skill_eval.py`（新規。後述）
- `scripts/check.sh`

`tasks/` と `README.md` は変えない。

## テスト

- `tools/skill-eval/test_skill_eval.py`（`unittest`、標準ライブラリだけ）を作り、`skill-eval` の段で `python3 -I -m unittest discover -s tools/skill-eval -p 'test_*.py'` として走らせる（`--inside-codex` でも走らせる）。少なくとも次を確かめる。
  - 集計の行の読み取り: 完了、`max_turns`、`max_tokens`、提供者の誤り、集計の行がない（終了状態 2）の各場合の `end` と理由。読めない行が混ざっても最後の集計の行を読むこと。
  - `attempts` の数え方と、`attempt_limit` を超えたときの失敗。
  - 費用の見積もり: キャッシュを含む `input` からキャッシュの分を引いて数えること（数値の例で）。
  - `--stop-cost`: 累計が達した後の試行を始めないこと（ハーネスの起動を差し替えて確かめる）。
  - 子の環境から `CLAUDE*`・`ANTHROPIC*` と四つのキーの変数が除かれ、`HOME` と `PATH` が残ること。
  - 設定ファイルの組み立て: パスに `"` を含むときに誤りになること、`[benitoite] path` と `allowed_programs` が入ること。
  - 記録に `--work-root` の絶対パスが現れないこと。
  - 模擬の提供者: 筋書きどおりの三回の応答と、四回目の 500、`tool_result` がないときの 400（ループバックで待ち受けるので、`--inside-codex` で待ち受けられなければ、そのテストを `skipTest` で飛ばし、理由を示す）。
  - 4 の `--allow` の求め方が、「前提の事実」の表と一致すること（本物の処理系を要するので、環境変数 `BENITOITEH_TEST_BENITOITE` があるときだけ走らせ、なければ `skipTest`）。
- 9 の模擬の提供者による通しが、手元の機械の全体の検査で通ること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、`unittest` の結果の末尾と、`--inside-codex` で飛ばしたものを書く。模擬の提供者による通しは、オーケストレータがサンドボックスの外の全体の検査で確かめる。
