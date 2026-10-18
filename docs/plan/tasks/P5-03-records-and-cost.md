# P5-03 評価の道具の記録、起動の環境、時間の上限、費用の上限、本物の提供者の項目

## 目的

P5-02 で評価の道具に `benitoiteh` を加えた。この作業では、本物の提供者で評価を走らせる前に要るものを加える。

- 本物の提供者の `benitoiteh` の項目（DeepSeek、Gemini、Opus）と、その単価。
- 子の環境から、親の対話の情報と API のキーを除くこと。
- 時間の上限で止めるときに、出力を残し、`benitoiteh` に集計を書かせること。
- 試行ごとの費用の見積もりと、累計で止める上限（設計書 07-02「評価の規模と費用の上限」）。
- 記録の Markdown の欄（番、トークンの量、時間、終わり方、費用）と、記録から利用者の絶対パスを除くこと。
- 本リポジトリでの使い方の文書。

この作業で本物の提供者を呼ばない。DeepSeek Harness と Claude Code の項目は P5-04 で加える。

## 読む設計書

- [評価](../../design/07-quality/07-02-evaluation.md)（全体）
- [P5-02](P5-02-benitoiteh-harness.md)（前提の事実と、行の欄）
- 既存のもの: `tools/skill-eval/`（P5-02 の後の `skill_eval.py`、`harnesses.json`、`mock_provider.py`、`test_skill_eval.py`、`SOURCE.md`）、`scripts/check.sh`

## この作業が認める型の変更

なし（Rust のコードを変えない）。道具のコードは python3 3.9 で動く書き方にし、標準ライブラリだけを使う。

## 作るもの

### 1. 本物の提供者の `benitoiteh` の項目

`harnesses.json` に、P5-02 の `benitoiteh-mock` と同じ形で次の三つを加える。どれも `verified: false`、`default: false` とする。

| 項目の名前 | `provider` | `model` | `effort` |
|---|---|---|---|
| `benitoiteh-deepseek` | `anthropic-messages`、`base_url` は `https://api.deepseek.com/anthropic`、`capabilities = {"cache_control": false, "strict_tools": false, "eager_input_streaming": false, "reasoning_replay": "same-destination"}`、`api_key = {"command": ["/usr/bin/security", "find-generic-password", "-s", "benitoiteh-deepseek", "-w"]}` | `deepseek-flash` | `high` |
| `benitoiteh-gemini` | `openai-chat`、`base_url` は `https://generativelanguage.googleapis.com/v1beta/openai`、キーはサービス `benitoiteh-gemini` | `gemini-3.8-flash` | `high` |
| `benitoiteh-opus` | `anthropic-messages`（`base_url` は書かない）、キーはサービス `benitoiteh-anthropic` | `claude-opus-5-5` | `medium` |

- キーチェーンのサービスの名前は秘密ではない。キーの値は、どこにも書かない。
- どの `benitoiteh` の項目にも、単価 `price`（100 万トークンあたりの米ドル。`input`・`cache_read`・`cache_write`・`output`）と、累計を数える提供者の名前 `billing`（`anthropic`・`google`・`deepseek`・`none`）を書く。`benitoiteh-mock` は単価をすべて 0、`billing` を `none` とする。
  - Opus 5.5: 入力 4、読み取り 0.20、書き込み 5、出力 20（Anthropic の公式の価格表。2026-10-18 に確かめた。書き込みは 5 分のキャッシュの単価）。
  - DeepSeek `deepseek-flash`: 入力 0.3、読み取り 0.006、書き込み 0.3、出力 1.2（DeepSeek の公式の価格表のピーク時の単価。2026-10-18 に確かめた。ピークでない時間は半額だが、高い側で見積もる。書き込みの単価は表にないので、入力と同じと置く）。
  - Gemini 3.8 Flash: 入力 0.75、読み取り 0.075、書き込み 0.75、出力 3.75（Google の公式の価格表の 2026 年中の単価。2026-10-18 に確かめた。書き込みは入力と同じと置く）。
  - 単価の出典と日付と、書き込みの単価を置いたことを、`harnesses.json` の `_comment` に書く（`docs/plan/results/P2-safety.md` の「DeepSeek と Gemini での試し」と同じ値）。

### 2. 子の環境（すべての項目）

- 子の環境から、名前が `CLAUDE` か `ANTHROPIC` で始まる変数と、`DEEPSEEK_API_KEY`・`GEMINI_API_KEY`・`GOOGLE_API_KEY`・`OPENAI_API_KEY` を除く。そのあとで、道具が設定する変数（`PATH`、`SKILL_EVAL_*`、`XDG_*`、`TMPDIR` など）を加える。理由: この道具は Claude Code の中から起動されることがあり、親の対話の情報（`CLAUDECODE`、`CLAUDE_CODE_SESSION_ID` など）と API のキーを評価の対象に渡さない（設計書 07-02「評価の手順」）。
- 除いた変数の名前（値は書かない）を、名前の順に記録の見出しに書く。

### 3. 出力の置き方と時間の上限（すべての項目）

- ハーネスの標準出力と標準エラー出力は、終わりを待って文字列で受け取る今の形をやめ、`<試行>/harness.stdout`・`harness.stderr` のファイルに直接書かせる。読み取り（P5-02 の集計の読み取りなど）は、終わった後にファイルから行う。
- 時間の上限（`HARNESS_TIMEOUT`、30 分）に達したら、プロセスのグループに SIGTERM を送り、30 秒待っても終わらなければ SIGKILL を送る。行の `timed_out` を真にする。理由: `benitoiteh` は SIGTERM で集計の行を書いて終了状態 130 で終わる。DeepSeek Harness は SIGTERM で終了状態 0 で終わるので、時間の上限で止めたことは、終了状態でなく道具の側で記録する。
- P5-02 の終わり方の分類の先頭に、`timed_out` が真なら `timeout` とする規則を加える。
- 起動から終わりまでの時間（壁時計）を測り、行の `wall_seconds`（小数第 1 位まで）に書く。
- `harness_exit` が -1（今の時間の上限の印）になる今の扱いは、`timed_out` と実の終了状態に置き換える。

### 4. 費用の見積もりと上限

- `benitoiteh` の項目の行に、`cost_usd` を書く。`(input_tokens - cache_read_tokens - cache_write_tokens) * price.input + cache_read_tokens * price.cache_read + cache_write_tokens * price.cache_write + output_tokens * price.output` を 100 万で割る（`input_tokens` はキャッシュを含むので、キャッシュの分を引いて数える）。単価のない項目（外部のハーネス）は `None` とする（P5-04 で加える）。
- 新しい選択肢 `--stop-cost <米ドル>` を加える。一回の道具の起動の中で、項目の `billing` ごとに `cost_usd` の累計を数える。累計は、複数の項目（`--harness a,b`）の `evaluate` の呼び出しをまたいで保つ。`billing` が `none` の項目は数えない。累計がこの値以上になったら、残りの試行を始めずに止め、それまでの記録（Markdown と `.jsonl`）を書き、`stopped: estimated cost reached <値> USD for <billing>` を標準エラー出力と記録の見出しに書き、終了状態 3 で終わる。既定はなし（止めない）。理由: 設計者が決めた累計の止め方を、走らせる者の注意に頼らずに守る。
- 一回の道具の起動の前の累計は数えない。記録をまたいだ累計は、評価を行う者が記録の合計から数える。

### 5. 記録の Markdown

- 試行ごとの表に、番、検査と修正の回数（書いた後の検査の回数を括弧で）、トークンの量の合計（うちキャッシュの読み取り）、時間、終わり方、費用の見積もりの欄を加える。取れない欄は `-` とする。
- 課題ごとの表に、成功した試行の時間とトークンの量の合計の中央値と、上限で止まった試行の数（`end` が `max_turns`・`max_tokens`・`timeout`）を加える。表の後に、その記録の費用の見積もりの合計を書く。
- 見出しに、`benitoiteh` の版、`system_prompt_sha256`、`tools_sha256`（どれも最初の試行の `session_start` から）、処理系の SHA-256、項目の提供者・モデル・effort・上限の値（`max_turns`・`max_run_tokens`・`HARNESS_TIMEOUT`）、2 の除いた変数の名前、4 の止めた旨を書く。`benitoiteh` の項目の Skill の版は、設置した Skill（既存の取り方）から書く。
- 安全の性質の課題の行で `shell_output_seen` が真なら、記録の先頭に `SAFETY: shell output seen in <課題> trial <n>` を書く。

### 6. 記録から絶対パスを除く

- Markdown と `.jsonl` に書くすべての文字列で、`--work-root` の実のパス、リポジトリの実のパス、`HOME` の接頭辞を、それぞれ `<work-root>`・`<repo>`・`~` に置き換える（長い接頭辞から順に置き換える）。理由: `results/` の記録はリポジトリに入れて公開するので、利用者の名前を含むパスを残さない。対象には、今の「生の記録の置き場所」の行、`main()` の全体の記録（`<日時>-overall.md`）の `生の記録` の行、理由の欄（`error` の行の `message`、`first_line(err)` など）、`trial_dir`、`--skill-dir` を含む。
- `.jsonl` の `trial_dir` などのパスの欄も同じく置き換える。

### 7. 試行の失敗を評価の全体に広げない

P5-02 の点検で、次のことが分かった（2026-10-18 のセキュリティのレビュー）。

- 記録の行が想定と違う形のとき、評価の全体が止まり、それまでの試行の行も記録に残らない。想定と違う形とは、`spill` のファイルがない、`parts` が文字列、`result.text` が `null`、`call` の鍵がない、などである。このとき `FileNotFoundError`・`TypeError`・`KeyError` は traceback で止まり、`ValueError` は `SystemExit` に変えられて止まる。本物の提供者で長い時間をかけた評価の結果を失いうる。
- 模擬の提供者は、要求の本文を上限なく読み、ソケットに時間の上限がない。
- 模擬の項目の試行にも、時間の上限の 30 分が掛かる。`benitoiteh` が固まると、完了の前の検査が長く止まる。

これを次のとおり直す。

- 一回の試行の中で、記録の読み取りと集計（`benitoiteh_output`、`benitoiteh_results`、`session_file`）の `OSError`・`ValueError`・`KeyError`・`TypeError` を捕らえ、その試行だけを `unreadable session record: <例外の種類>` を理由とする失敗にし、評価を続ける。`task_allow` の失敗（解の報告が読めない）は、今のとおり評価を止める（処理系か課題の誤りなので、続けても意味がない）。
- 評価の途中で止まるとき（`--stop-cost`、`KeyboardInterrupt`、捕らえない例外）も、それまでの行で記録を書いてから終わる。
- 模擬の提供者は、`Content-Length` がないか、負か、8 MiB を超える要求に 400 で答え、ハンドラに `timeout = 10`（秒）を置く。
- 項目の `options` に `harness_timeout`（秒）を置けるようにし、`benitoiteh-mock` は 120 とする。ないときは `HARNESS_TIMEOUT` を使う。

### 8. 誤りの文と文書

- `find_benitoite` の誤りの文（`build it with cargo build`）を、`pass --benitoite <path to benitoite v0.0.7>` に変える。誤りの文が Benitoite のスキル `skill-eval` を示す箇所は、`tools/skill-eval/SOURCE.md` を示す形に変える。
- `tools/skill-eval/SOURCE.md` に「本リポジトリでの使い方」の節を作る。先頭に、LLM を呼ぶ評価は設計者の了承を得てから行うこと（`AGENTS.md` の「LLM を呼ぶ作業」）を書く。そのあとに、`benitoiteh` の項目の走らせ方（`--harness benitoiteh-deepseek --trials 3 --allow-unverified --benitoite <v0.0.7> --benitoiteh <path> --work-root <リポジトリの外> --stop-cost <額>`）、模擬の提供者による通し、記録の欄、費用の見積もりの限り（単価は日付のもの、DeepSeek はピークの単価、書き込みの単価を置いたこと）を書く。
- 「写した後に変えた点」に P5-03 の節を作り、変えた点を書く。

## 触るファイル

- `tools/skill-eval/skill_eval.py`、`tools/skill-eval/harnesses.json`、`tools/skill-eval/test_skill_eval.py`、`tools/skill-eval/SOURCE.md`
- 必要なら `scripts/check.sh`（模擬の提供者による通しの確かめの欄を足すときだけ）

`tasks/` と `README.md` は変えない。

## テスト

`tools/skill-eval/test_skill_eval.py` に、少なくとも次を加える。

- 子の環境から `CLAUDE*`・`ANTHROPIC*` と四つのキーの変数が除かれ、`HOME` と `PATH` が残ること。
- 時間の上限: 上限を短くした試しの起動（SIGTERM を受けると決まった行を書いて終わる小さな Python のプロセス、SIGTERM を無視するプロセス）で、`timed_out` が真になり、前者は SIGTERM で終わってそれまでの出力が残り、後者は猶予の後に SIGKILL で止まること（猶予の値はテストで短くできるようにする）。
- 費用の見積もり: キャッシュを含む `input_tokens` からキャッシュの分を引いて数えること（数値の例で）。
- `--stop-cost`: 累計が達した後の試行を始めないこと、`billing` が `none` の項目を数えないこと、項目をまたいで累計を保つこと（ハーネスの起動を差し替えて確かめる）。
- 記録に `--work-root`・リポジトリ・`HOME` の絶対パスが現れないこと（Markdown、全体の記録、`.jsonl` のそれぞれで）。
- 7: `spill` のファイルがない、`result.text` が `null` の記録で、その試行だけが失敗になり、次の試行が走り、記録が書かれること。模擬の提供者が大きすぎる `Content-Length` に 400 で答えること。
- `harnesses.json` の三つの本物の項目が、`api_key` に値の形を持たず、`price` と `billing` を持つこと。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、`unittest` の結果の末尾と、模擬の提供者による記録の Markdown の例（課題一つ分の行）を書く。
