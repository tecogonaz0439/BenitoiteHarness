# Skill の評価の道具

同梱の Agent Skill（`crates/benitoite/skill/`）だけを与えたコーディングエージェントが、課題のスクリプトを書き、検査と修正を繰り返して期待する出力を得られるかを測る道具である（設計書 06-06「Skill の評価」）。

既定の評価の対象（`--harness default`）は、設計者の決定（2026-10-08）により次の三つの組とする。OpenCode は設計者が解約したので、項目を `harnesses.json` に残すが既定の対象から外す。`antigravity-gpt-oss`（Antigravity の CLI、`gpt-oss-120b-medium`、effort medium）も、項目を残すが評価の対象に含めない。

| 項目の名前 | ハーネス | モデルと推論の度合い |
|---|---|---|
| `claude-code` | Claude Code | `claude-opus-5-5`、effort medium |
| `codex` | Codex CLI | `gpt-6-luna`、`model_reasoning_effort` max |
| `antigravity-gemini` | Antigravity の CLI（`agy`） | `gemini-3.8-flash-high`、effort high |

GPT-6-Luna の結果は、オープンウェイトのモデルでもうまく書けるかの目安として扱う。`antigravity-gpt-oss` はオープンウェイトのモデルで直接確かめるための組として用意したが、2026-10-08 の 3 回の短い試行がすべて失敗した（`results/` の 1555・1604・1641）ので、設計者の決定（2026-10-09）により評価の対象に含めない。同じ仕組みを、構文の測定の第二段階（構文の案ごとの評価。docs/todo の TODO-017）にも使う（`--skill-dir`）。

評価の手順（設計者の了承、ハーネスの選び方、試行の回数、記録の書き方）は、プロジェクトのスキル `skill-eval`（`.claude/skills/skill-eval/SKILL.md`）に書く。本書は道具の使い方と、課題と記録の形を書く。

## ファイル

| パス | 内容 |
|---|---|
| `skill_eval.py` | 評価の道具。Python の標準ライブラリだけを使う（python3 3.9 で動く） |
| `harnesses.json` | ハーネスの起動のコマンドの型板と、Skill の置き場所、インターネットを使わせない設定 |
| `http_server.py` | HTTP の課題の接続先。127.0.0.1 の空いたポートで決まった応答を返す |
| `bin/benitoite` | 包みのコマンド（POSIX の sh）。評価の間、この名前で `PATH` の先頭に置く |
| `tasks/<課題>/` | 課題（後述） |
| `results/` | 記録（Markdown）。生の対話の記録は置かない |

## 使い方

LLM を呼ばない二つの使い方は、いつ実行してもよい。先に `cargo build`（または `cargo build --release`）で処理系を作っておく。道具は本物の `benitoite` を、`--benitoite` の指定、`target/release` と `target/debug` のうち新しく作った方、`PATH` の順に探す。`--benitoite <パス>` で指定もできる。

```sh
# 各課題の解のスクリプト（reference.bnt）が期待する出力を出すか。包みのコマンドも確かめる
python3 tools/skill-eval/skill_eval.py --self-test

# ハーネスを起動せずに、準備と判定だけを行う。ハーネスの代わりに解のスクリプトを置き、
# 包みのコマンドで一度検査してから判定する。--agent で Skill の置き場所を選ぶ（既定 codex）
python3 tools/skill-eval/skill_eval.py --dry-run [--agent <harnesses.json の項目の名前>] [--skill-dir <dir>] [--task <課題>]...
```

LLM を呼ぶ評価は、設計者の了承を得てから行う（スキル `skill-eval`）。

```sh
python3 tools/skill-eval/skill_eval.py --harness default --trials 3 --allow-unverified [--model <モデル>] [--task <課題>]... \
    [--skill-dir <dir>] [--work-root <リポジトリの外のディレクトリ>] [--record <記録のファイル>]
```

| 選択肢 | 意味 |
|---|---|
| `--harness <名前>` | `harnesses.json` の項目の名前（コンマで区切って複数を書ける）か、既定の対象を表す `default`。複数のときは `<work-root>/<項目>/` に分け、項目ごとに記録を書く |
| `--trials <n>` | 課題ごとの試行の回数（既定 1） |
| `--task <課題>` | 課題を選ぶ（繰り返せる。既定はすべて） |
| `--model <モデル>` | ハーネスに渡すモデル（既定は `harnesses.json` の値） |
| `--skill-dir <dir>` | `benitoite skill install` の代わりに、このディレクトリを Skill として写す。構文の案ごとの Skill を差し替えて評価するときに使う（docs/todo の TODO-017） |
| `--work-root <dir>` | 作業ディレクトリと生の記録の置き場所。リポジトリの中は指定できない（既定は新しい一時ディレクトリ）。同じディレクトリで再び実行すると、空いた番号の試行のディレクトリを使う |
| `--allow-unverified` | `harnesses.json` の `verified` が `false` のハーネスを起動する。付けなければ起動を断る |
| `--record <file>` | 記録のファイル（既定 `results/<日時>-<ハーネス>.md`） |

### 一回の試行の流れ

1. `<work-root>/<課題>/<試行>/workspace/` を作り、課題の `input/` の中身と `task.md` を写す。解のスクリプト（`reference.bnt`）と期待する出力は写さない。
2. Skill を置く。既定では `benitoite skill install --project --agent <エージェント>` を作業ディレクトリで実行する。`--skill-dir` があれば、そのディレクトリを `<skill_parent>/benitoite` に写す。`harnesses.json` の `files`（インターネットを使わせない設定など）を作業ディレクトリに書く。
3. 包みのコマンドと本物の処理系を試行のディレクトリ（`<試行>/bin/`・`<試行>/real/`。リポジトリの外）に写す。リポジトリの中のパスを見せると、エージェントが解のスクリプトや設計書に辿り着けるためである。`PATH` の先頭に `<試行>/bin/` を置き、`SKILL_EVAL_LOG`（記録のファイル。作業ディレクトリの外の `<試行>/benitoite.log`）と `SKILL_EVAL_REAL_BENITOITE`（本物の処理系の写し）を設定して、ハーネスを起動する。HTTP の課題では `http_server.py` を立て、接続先の URL を `SKILL_EVAL_HTTP_BASE` で渡す。ハーネスの標準出力と標準エラー出力は `<試行>/harness.stdout`・`harness.stderr` に置く。
4. 判定する。ハーネスが起動できなかったとき、記録がないとき、数える起動が 0 回のときは、理由の欄に警告を書いて失敗とする（包みのコマンドを迂回した試行や、サンドボックスが記録を書けない試行を成功と記録しないため）。記録の `check`・`run`・`test` の行の数（検査と修正の回数）が課題の上限（既定 10）を超えたら失敗とする。超えなければ、書かれたスクリプトを、入力を写し直した別のディレクトリで実行して比べる（判定のための起動は包みのコマンドを通さず、回数に数えない）。
   - 実行の課題: `benitoite check` が通り、`benitoite run` の終了状態が期待（既定 0）と等しく、標準出力が `expected.stdout` と等しく、`expected.files/` の各ファイルが書かれていること。`.json` のファイルは値として比べ、ほかはバイト列で比べる。
   - テストを書く課題: `benitoite test --diagnostics=json` の集計の行（`testSummary`）で、失敗が 0 で成功の数が `min_tests` 以上であり、スクリプトが `require_text` の文字列（`handle` など）を含むこと。テストのファイルには `check` を使わない（06-06「同梱の Agent Skill の構成」）。

### 包みのコマンドの記録

`bin/benitoite` は、起動のたびに `SKILL_EVAL_LOG` に一行を加え、本物の処理系の終了状態をそのまま返す。行はタブで区切る。

```text
<開始の時刻 UTC>	<終了の時刻 UTC>	<終了状態>	<診断のコード>	<引数 1>	<引数 2>	...
```

診断のコードは、本物の処理系が標準エラー出力に書いた診断のコード（`error[E0405]`・`warning[W0401]`・`runtime error[R0101]` と、`--diagnostics=json` の `"code":"E0405"`）を、出た順にコンマで区切ったものである。標準エラー出力は `tee` で写しを取るだけで、エージェントに見える出力は変えない（`--diagnostics=json` を足さない）。引数の中のタブと改行は空白に置き換える。回数には、`-` で始まらない最初の引数が `check`・`run`・`test` の起動と、それが `.bnt` で終わる起動（サブコマンドのない実行）を数える。サブコマンドの前の選択肢（`benitoite --diagnostics=json check x.bnt`）は飛ばして見る。`benitoite --version` などの起動も記録するが、回数には数えない。

## 課題

| 課題 | 題材 | 種類 |
|---|---|---|
| `json_summary` | JSON の集計（ロードマップの完了条件）。入力と期待する出力は受け入れテスト `crates/benitoite/testdata/acceptance/json_summary` と同じで、`--self-test` が一致を確かめる | 実行 |
| `file_process` | ファイルを読んで書く | 実行 |
| `csv_report` | CSV を読んで集計し、CSV を書く | 実行 |
| `json_convert` | CSV を JSON に変えて書く | 実行 |
| `process_run` | 外部コマンドの起動（`Process.run`。引数と環境変数と標準入力） | 実行 |
| `process_shell` | シェル（`Process.shell`。パイプと終了状態） | 実行 |
| `http_client` | HTTP のクライアント（`http_server.py` への GET と POST、JSON） | 実行 |
| `regex_text` | テキストの処理（正規表現） | 実行 |
| `dir_walk` | ディレクトリの走査 | 実行 |
| `error_handling` | エラー処理（読めないファイルを報告して続ける） | 実行 |
| `write_tests` | テストを書くこと（組み込みの操作のハンドラでの差し替えを含む） | テスト |

課題のディレクトリの中身は次のとおりである。

| ファイル | 内容 |
|---|---|
| `task.md` | エージェントに渡す指示の文（英語）。書くスクリプトの名前と、出力の形を書く |
| `task.json` | `kind`（`run` か `test`）、`script`（書くファイルの名前）、`attempt_limit`（検査と修正の回数の上限、既定 10）、`http`、`exit_code`、`min_tests`、`require_text`、`acceptance`（受け入れテストと一致させるファイル） |
| `input/` | 作業ディレクトリに写す入力 |
| `expected.stdout` | 期待する標準出力（実行の課題） |
| `expected.files/` | 期待する、スクリプトが書くファイル |
| `reference.bnt` | 正しい解のスクリプト。`--self-test` が使い、エージェントには見せない |

HTTP の課題の接続先は `SKILL_EVAL_HTTP_BASE`（例 `http://127.0.0.1:54321`）で渡し、スクリプトは `Process.environmentVariable` で読む。`http_server.py` を単独で起動すると、接続先の URL を一行書いて応答し続ける。

## ハーネスの設定

`harnesses.json` の型板の `{workspace}`・`{model}`・`{prompt}` と `options` の各キーは、道具が値で置き換える。呼び方は `tools/syntax-measure/bntmeasure/agents.py` に合わせた。2026-10-08 の短い試行で、`claude-code`・`codex`・`antigravity-gemini` を確かめた（`verified: true`）。`claude-code` には `--strict-mcp-config` を加えた。`antigravity-gpt-oss` は短い試行で検査が上限を超え、ウェブの検索を呼んだ（フックを入れる前）ので、既定の対象から外した。下の表の「確かめられていない点」は D23 の時点の記述を含む。

| ハーネス | Skill の置き場所 | インターネットを使わせない設定 | 確かめられていない点 |
|---|---|---|---|
| `codex` | `.agents/skills`（`--agent codex`） | `--sandbox workspace-write`（既定でネットワークを使わない）と `-c tools.web_search=false` | サンドボックスの中で `.agents/skills` の Skill を読むか。`-c tools.web_search=false` の鍵の名前。サンドボックスが 127.0.0.1 への接続も止めるか（止めるなら、`http_client` でエージェントは自分のスクリプトを実行できない。判定は道具がサンドボックスの外で行うので影響しない）。サンドボックスの中のシェルに `PATH` が引き継がれるか |
| `opencode`（既定の対象外） | `.agents/skills`（`--agent opencode`） | 作業ディレクトリの `opencode.json` の `permission` で `webfetch`・`websearch` を `deny` にする | `permission` の鍵の名前と、`.agents/skills` の Skill を読むか |
| `claude-code` | `.claude/skills`（`--agent claude-code`） | `--disallowedTools WebFetch WebSearch` と、`--allowedTools` で Bash を `benitoite` に限る | `tools/syntax-measure/` に型板がない。`--effort`（low・medium・high・xhigh・max）と `--model` は `claude --help` で確かめたが、選択肢の組み合わせ全体は起動して確かめていない。`Bash(benitoite:*)` の許可で包みのコマンドが呼ばれるか |
| `antigravity-gemini`・`antigravity-gpt-oss` | `.agents/skills`（Antigravity の文書 `~/.gemini/antigravity-cli/builtin/skills/agy-customizations/docs/skills.md` が、作業ディレクトリの `.agents/skills/` を Skill の置き場所とする。`--agent codex` と同じ場所なので、`skill install --agent codex` で置く） | 作業ディレクトリの `.agents/hooks.json` の `PreToolUse` で、`search_web`・`read_url.*`・`browser_.*` の道具を拒否する（同じ文書の `hooks.md`）。`--sandbox` は使わない（包みのコマンドの起動と記録の書き込みを止め、`--dangerously-skip-permissions` のもとではエージェントが迂回する） | 2026-10-08 の短い試行で、Skill を読むこと、包みのコマンドが記録されること、フックが検索の道具を拒否することを確かめた。`--disable-slash-commands` は Skill を読めなくするおそれがあるので外した。利用者の設定（`~/.gemini/config/`）を除く方法は確かめていない（開発機では Skill・MCP のサーバ・大域の規則の設定がない）。ウェブの道具の呼び出しの有無は、試行の後に `~/.gemini/antigravity-cli/brain/<対話>/.system_generated/logs/transcript.jsonl` でも確かめる |

ハーネスは利用者の設定（ホームの設定のファイル、ほかの Skill、MCP のサーバ、利用者の指示のファイル）も読む。ほかの文書を与えない条件を守るため、評価の前に、利用者の設定にある Skill と MCP のサーバと、利用者の指示のファイル（`~/.claude/CLAUDE.md`、`~/.codex/AGENTS.md`、Antigravity の `~/.gemini/config/` の設定と大域の `GEMINI.md`・`AGENTS.md`）を外すか、評価のための別の利用者の設定で起動する。Claude Code なら環境変数 `CLAUDE_CONFIG_DIR`、Codex なら `CODEX_HOME` で空の設定のディレクトリを指す案がある。Antigravity は `~/.gemini/config/` の設定と大域の規則を一時的に退けるか、別の `HOME` で起動する案がある（どれも確かめていない）（スキル `skill-eval`）。

## 所要時間と回数の見込み

LLM を呼ばない部分は短い。開発機（debug のビルド）で `--self-test` は約 2 秒、`--dry-run` は約 3 秒で終わる。

LLM を呼ぶ評価の見込みは次のとおりである（実測ではない。`tools/syntax-measure/` の試行の経験からの見積もり）。一回の試行は、エージェントが Skill を読み、数回検査と実行を繰り返すので、2〜10 分かかると見込む。

| 評価 | 試行の回数 | 所要時間の見込み |
|---|---|---|
| 最初の短い試行（`json_summary` だけ、既定の三つの組、各 1 回） | 3 | 6〜30 分 |
| 完了条件の確かめ（`json_summary` だけ、三つの組、各 5 回） | 15 | 30 分〜2.5 時間 |
| 全課題（11 課題、三つの組、各 3 回） | 99 | 3.5〜16.5 時間 |
| 構文の案ごとの評価（案 n 個、全課題、一つのハーネス、各 3 回） | 33 n | 案ごとに 1〜6 時間 |

試行は順に行う（並べると、ハーネスの利用量の上限に掛かりやすく、包みのコマンドの記録も分けにくい）。

## 記録

### 診断のコードの統計

記録の「診断のコードの集計」の節は、LLM が書いたスクリプトに処理系が出した誤りと警告を、コードごとに数える。警告が実装した意味のあるものかを調べる材料である。ハーネスとモデルの組ごとと、複数の組を評価したときの全体（`<日時>-overall.md`）に出す。

| 欄 | 意味 |
|---|---|
| 出た回数・出た試行の数 | 包みのコマンドの記録で、そのコードが出た回数と、出た試行の数 |
| 解消・残った・解消率 | そのコードが出た起動の、次の check・run・test で、コードが消えた回数と残った回数 |
| 最後に残った試行の数 | 判定のときに、最後のスクリプトの check（テストの課題では test）でそのコードが出た試行の数 |

警告は diag::codes の全件を載せ、一度も出なかった警告も 0 とする。試行ごとに、最初の check で出たコードと、最後の check の警告も載せる。読み方の目安は次のとおりである。

- よく出て、よく解消される: 診断が LLM に伝わり、修正に効いている。
- 出るのに解消されない（残る、最後に残る）: 文言や修正案が伝わっていない可能性がある。診断の文言か Skill の説明を見直す。
- 出ない: その警告を評価の課題で測れていない。効果がないとは言えない。


`--harness` で評価すると、`results/<日時>-<ハーネス>.md` に記録を書く。記録には、日時、処理系の版（`benitoite --version`）、Skill の版（`SKILL.md` の `benitoite-version` と、Skill のファイルの SHA-256 の先頭）、ハーネスの名前と版、モデル、試行の回数、生の記録の置き場所、課題ごとの成功率と成功した試行の検査と修正の回数、試行ごとの結果と失敗の理由を書く。

生の対話の記録（`harness.stdout` など）と作業ディレクトリは、`--work-root`（リポジトリの外）に残し、リポジトリに入れない（`tools/syntax-measure/` と同じ扱い）。

JSON の集計の課題（`json_summary`）は、各ハーネスで試行の過半数が成功したときに、初回リリース版の完了条件を満たしたとする（06-06）。ほかの課題の成功率は、Skill を改める材料として記録し、合格の基準を設けない。
