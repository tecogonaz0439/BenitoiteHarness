# Skill の評価の道具

同梱の Agent Skill（`crates/benitoite/skill/`）だけを与えたコーディングエージェントが、課題のスクリプトを書き、検査と修正を繰り返して期待する出力を得られるかを測る道具である（設計書 06-06「Skill の評価」）。

圧縮の組は `compact_json_summary` と `compact_dir_walk` の二課題である。資料の再生成・一致の検査と、組の準備・判定は次のように確かめる。LLM とキーを使わない。self-test と dry-run の判定は macOS の `sandbox-exec` を要するため、Codex の外で走らせる。

```sh
/usr/bin/python3 -I tools/skill-eval/generate_compaction_materials.py --check
# 再生成するときは --check を外す。同じ引数で同じ資料と generation を作る。
/usr/bin/python3 -I tools/skill-eval/generate_compaction_materials.py --total-kib 268 --chunk-kib 10 --seed 33
/usr/bin/python3 -I tools/skill-eval/skill_eval.py --self-test --suite compaction --benitoite <v0.0.7の処理系>
/usr/bin/python3 -I tools/skill-eval/skill_eval.py --dry-run --suite compaction --benitoite <v0.0.7の処理系> \
  --work-root <リポジトリと共有のtmpの外の作業場所>
```

本物の提供者を使う実行例は次のとおりである。先に設計者へ回数と費用を提示して了承を得る。試しは同じ 1 課題で各条件 1 回の計 3 試行、本体は 2 課題・3 条件・各 3 回の計 18 試行とする。見込みの費用・時間と停止額 $5 は、[設計書 07-02「M の段の残りの規模と費用」](../../docs/design/07-quality/07-02-evaluation.md#m-の段の残りの規模と費用)を参照する。別の起動では、使った費用を差し引いた残額を渡す。

```sh
# 試し: 同じ一課題、三条件を一回ずつ。実行前に設計者の了承を得る。
/usr/bin/python3 -I tools/skill-eval/skill_eval.py --suite compaction --task compact_json_summary \
  --harness benitoiteh-deepseek-compact-base,benitoiteh-deepseek-compact-reserve,benitoiteh-deepseek-compact-keep \
  --allow-unverified --interleave --trials 1 --stop-cost 5 \
  --benitoite <v0.0.7の処理系> --benitoiteh <全条件で共通の実行ファイル> \
  --work-root <リポジトリと共有のtmpの外の作業場所>
# 本体: 試しで共通の長さと段を確定してから、二課題を三回ずつ。
/usr/bin/python3 -I tools/skill-eval/skill_eval.py --suite compaction \
  --harness benitoiteh-deepseek-compact-base,benitoiteh-deepseek-compact-reserve,benitoiteh-deepseek-compact-keep \
  --allow-unverified --interleave --trials 3 --stop-cost <残額> \
  --benitoite <v0.0.7の処理系> --benitoiteh <全条件で共通の実行ファイル> \
  --work-root <リポジトリと共有のtmpの外の作業場所>
```

1 試行は、同じセッションの `exec` と 2 回の `exec --resume` を含む。50 番・200 万トークン・1,800 秒を全段の合計に掛け、各段には残りを渡す。費用も段の後に判定し、残額がないか有料の費用が不明なら後続を止める。`--interleave` は 3 段を終えてから次の条件へ進む。段ごとの出力は `stage-N.stdout`・`stage-N.stderr`、総計と段の内訳は試行ごとに 1 行の `.jsonl`、比較の成立と段の表は Markdown に残る。dry-run は解を置いて判定し、`comparison_valid: false` として本物の成績と区別する。

模擬の通しには `--harness benitoiteh-mock-compact --suite compaction` を使い、同じ `--benitoite`・`--benitoiteh` と、`--record <一時の記録.md>` を指定する。本物の提供者を呼ばずに再開と圧縮の記録まで確かめるものであり、圧縮の効果の評価には使わない。Codex の中では、待ち受けと入れ子のサンドボックスを要するこの通しを走らせない。

資料は各課題の `materials/1/`・`2/`・`3/` にあり、段の直前に対応するディレクトリだけを workspace に写す。依頼の本文は `stages/1.md`〜`3.md` で、一つ 4 KiB 以下、資料は一つ 12 KiB 以下である。原本に置く量は暫定の目標 268 KiB（各段約 90 KiB、各段 9 ファイル）とした。生成引数、実際のバイト数、各ファイルと全体の SHA-256 は `task.json` の `generation`、候補を選んだ理由は [SOURCE.md](SOURCE.md#p5-33-圧縮の二課題と資料) に残す。`--total-kib` は三段の合計の目標量で、完全な例まで書くため実量は少し増える。`--tasks-dir` で別の課題の置き場も指定できる。

有料の試しでは、全条件の圧縮、現行値と残す量を減らす条件での依頼をまたぐ `kept_items > 0` と `kept_estimate` の差、予備を減らす条件での圧縮の時点を確認する。満たさなければ資料と段の区切りを全条件共通で調整し、追加の試しの回数と費用を設計者に示す。本体の前に長さと段を固定する。見込みは試し $0.18〜0.60、本体 $1.08〜3.60、合計 $1.26〜4.20、順次実行で 42〜126 分である。再開直後は模擬の固定使用量が判定に使われないため、模擬で成立した長さを本物でも成立すると扱わない。

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
| `--task <課題>` | 選んだ組の中から課題を選ぶ（繰り返せる。既定はその組のすべて）。組の外の名前は起動前に拒む |
| `--suite benitoite\|compaction` | 課題の組。省略時は従来の 11 課題だけ。圧縮の 2 課題は `compaction` で選ぶ |
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

判定の実行前に、`benitoite check --report=json`（テストの課題では `test --check --report=json`）の宣言したエフェクトが、正しい解から求めた `task_allow` の部分集合であることを確認する。不完全な報告と許可外のエフェクトは失敗として実行しない。判定の環境は親から引き継がず、HOME・XDG・TMPDIR は判定のディレクトリ内、PATH は `/usr/bin:/bin` とする。HTTP の課題の模擬の接続先だけは明示して渡す。判定の `run`・`test` は macOS の `/usr/bin/sandbox-exec` で実行する。書き込みは判定のディレクトリと `/dev/null`、起動は本物の処理系と課題が要するプログラムに限る。`process_run` は `/usr/bin/sort`、`process_shell` は `/bin/sh`・`/bin/bash`・`/usr/bin/grep`・`/usr/bin/cut`・`/usr/bin/sort` を許す。通信は拒み、`http_client` だけは模擬の `127.0.0.1` の指定したポートへの接続を許す。macOS 以外と、サンドボックスを掛けられない場合は判定を失敗にする。実行時にも `--allow-effects=<task_allow>` を渡す。check の報告の種別を検証し、診断のコードと失敗の理由を記録する。親の HOME の実パスの下では `file-read-data` を拒み、その後で実の処理系、課題に許した補助プログラムの各実行ファイル、判定のディレクトリだけを許し直す。判定用の HOME と親の HOME は区別する。親の HOME の外の読み取りは、処理系と OS の動作のために従来の許可を保つ。

判定の理由に含める stdout の相違と誤りの文の抜粋は、制御文字などを ASCII のエスケープにし、抜粋ごとに120文字以下に切る。これは任意の秘密を伏せる処理ではなく、秘密の読み取りは OS の方針で拒む。

エフェクトの検査と `--allow-effects` は直接の起動を早く拒むために使う。`Process.Run` を許す課題では、許された処理系を入れ子に起動すると、このエフェクトの許可を広げられるため、読み取り・書き込み・起動・通信の境界は OS の方針で守る。判定のコマンドが正常に終わったときも、同じプロセス群を SIGKILL で止める。別の群れへ移った子の検出と停止は、この起動の口では保証しない。

`benitoiteh` の起動では、`PWD`・`OLDPWD` とリポジトリのパスを含む環境変数を除く。セッション記録（退避した出力を含む）にリポジトリのパスや `tools/skill-eval/tasks` が現れた試行は、成果物の合否を保ち、`WARNING` と `repository_path_seen: true`・`comparison_excluded: true` を残す。圧縮の `comparison_valid` も `false` にする。場所の推測などによる原本の読み取りを、完全に防ぐものではない。

### 包みのコマンドの記録

`bin/benitoite` は、起動のたびに `SKILL_EVAL_LOG` に一行を加え、本物の処理系の終了状態をそのまま返す。行はタブで区切る。

```text
<開始の時刻 UTC>	<終了の時刻 UTC>	<終了状態>	<診断のコード>	<引数 1>	<引数 2>	...
```

診断のコードは、本物の処理系が標準エラー出力に書いた診断のコード（`error[E0405]`・`warning[W0401]`・`runtime error[R0101]` と、`--diagnostics=json` の `"code":"E0405"`）を、出た順にコンマで区切ったものである。標準エラー出力は `tee` で写しを取るだけで、エージェントに見える出力は変えない（`--diagnostics=json` を足さない）。引数の中のタブと改行は空白に置き換える。回数には、`-` で始まらない最初の引数が `check`・`run`・`test` の起動と、それが `.bnt` で終わる起動（サブコマンドのない実行）を数える。サブコマンドの前の選択肢（`benitoite --diagnostics=json check x.bnt`）は飛ばして見る。`benitoite --version` などの起動も記録するが、回数には数えない。

## 課題

`kind: staged` の課題は圧縮の組に属する。`kind: normal` は後続の通常モードの組のために予約し、それ以外を既定の Benitoite の組とする。`--suite compaction` で次の二課題を選ぶ。`scripts/check.sh` は二課題の存在と資料の再現性を必須にし、Codex の外では self-test・dry-run・模擬の通しを実行する。

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
| `compact_json_summary` | 地域別の注文の集計、JSON の生成、CLI による入力の選択 | 三段の実行 |
| `compact_dir_walk` | Markdown の列挙、拡張子と件数の集計、CLI による根の選択 | 三段の実行 |

課題のディレクトリの中身は次のとおりである。

| ファイル | 内容 |
|---|---|
| `task.md` | エージェントに渡す指示の文（英語）。書くスクリプトの名前と、出力の形を書く |
| `task.json` | `kind`（`run` か `test`）、`script`（書くファイルの名前）、`attempt_limit`（検査と修正の回数の上限、既定 10）、`http`、`exit_code`、`min_tests`、`require_text`、`acceptance`（受け入れテストと一致させるファイル） |
| `input/` | 作業ディレクトリに写す入力 |
| `expected.stdout` | 期待する標準出力（実行の課題） |
| `expected.files/` | 期待する、スクリプトが書くファイル |
| `reference.bnt` | 正しい解のスクリプト。`--self-test` が使い、エージェントには見せない |

三段の課題の `task.json` は、`kind: staged`・`result_kind: run`・`http: false` と、三つの Markdown の相対パスの配列 `stages` を持つ。`judge_cases` は `name`・`input`・`args`・`expected_stdout`・`expected_files` を持ち、各ケースの入力・期待値を `cases/<name>/` の中に置く。引数なしも `args: []` のケースとして宣言する。最上位の `expected.stdout` と `expected.files/` は使わない。判定はケースごとに新しい場所へ原本とケースの入力を重ね、最終のスクリプトだけを写して実行する。JSON の生成ファイルは値で比べる。

`self_test_failures` は `script` と `case` を対応させる。self-test は正しい解が全ケースを通ることに加え、初期条件を一つ外した解が指定のケースで標準出力または生成ファイルの不一致になることを確かめる。構文・エフェクト・実行時の誤りでは合格としない。失敗した条件は `s1_cancelled`・`s1_hidden_directories` などのケース名を理由に残す。段とケースのパスは通常のファイルとディレクトリに限り、リンク、絶対パス、`..` を拒む。

HTTP の課題の接続先は `SKILL_EVAL_HTTP_BASE`（例 `http://127.0.0.1:54321`）で渡し、スクリプトは `Process.environmentVariable` で読む。`http_server.py` を単独で起動すると、接続先の URL を一行書いて応答し続ける。

## ハーネスの設定

`harnesses.json` の型板の `{workspace}`・`{model}`・`{prompt}` と `options` の各キーは、道具が値で置き換える。呼び方は `tools/syntax-measure/bntmeasure/agents.py` に合わせた。2026-10-08 の短い試行で、`claude-code`・`codex`・`antigravity-gemini` を確かめた（`verified: true`）。`claude-code` には `--strict-mcp-config` を加えた。`antigravity-gpt-oss` は短い試行で検査が上限を超え、ウェブの検索を呼んだ（フックを入れる前）ので、既定の対象から外した。下の表の「確かめられていない点」は D23 の時点の記述を含む。

| ハーネス | Skill の置き場所 | インターネットを使わせない設定 | 確かめられていない点 |
|---|---|---|---|
| `codex` | `.agents/skills`（`--agent codex`） | `--sandbox workspace-write`（既定でネットワークを使わない）と `-c tools.web_search=false` | サンドボックスの中で `.agents/skills` の Skill を読むか。`-c tools.web_search=false` の鍵の名前。サンドボックスが 127.0.0.1 への接続も止めるか（止めるなら、`http_client` でエージェントは自分のスクリプトを実行できない。判定は道具が別のサンドボックスで模擬のポートだけを許すので影響しない）。サンドボックスの中のシェルに `PATH` が引き継がれるか |
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

判定に OS のサンドボックスを使うため、`scripts/check.sh --inside-codex` は `requires_sandbox` の Python テストと `test_judge_sandbox.RuntimeTests` の OS テスト、全組の self-test・dry-run を除く。除いたテスト名は検査の出力に示す。方針の組み立て、否定用のスクリプトの構文とエフェクト、診断の記録、段の実行器の再開・上限・使用量のテストは Codex 内でも走る。外の macOS では、正しい解と対にして外への書き込み・未許可の起動・通信の拒否を確かめる。

原本の検出は、worktree と元の checkout のパス、HOME と分けた既知の綴りを印にする。任意に分割して組み立てたパスや、後の段の内容そのものを検出する仕組みはない。後の段の依頼や資料の内容で照合すると、前の段と共通する仕様の文を誤って漏れとするおそれがある。二課題には一般的な語や例もあるため、今回は内容の一致による自動判定を加えず、任意の分割や内容だけの漏れの検出を残す。原本のアクセスをすべて防ぐ保証として、この印を扱わない。

判定の HTTP の URL は `http://127.0.0.1:<番号>` に限り、Seatbelt の方針ではそのポートを `localhost:<番号>` と表す。`process_shell` の許可に含む `/bin/bash` は、macOS の `/bin/sh` が `/private/var/select/sh` の選択に従って起動し直すために必要である。評価用の macOS では、この選択が `/bin/bash` であることを確認する。
