# 評価の道具の出典

このディレクトリは、Benitoite の Skill の評価の道具 `tools/skill-eval/` の写しに、`benitoiteh` を評価するための変更を加えたものである（設計書 07-02「評価の課題」）。

## 写した元

| 項目 | 値 |
|---|---|
| リポジトリ | <https://github.com/tecogonaz0439/Benitoite> |
| タグ | `v0.0.8` |
| コミット | `6b0568a3ddcc46b0a0e64504139ce286a27fd86a`（2026-10-18 11:00 +09:00） |
| 写した日 | 2026-10-18 |
| 写し方 | `git -C ../Benitoite archive v0.0.8 tools/skill-eval` を展開し、`results/` を除いて写した |

`results/` は Benitoite の評価の記録なので写さない。本リポジトリの評価の要約は、同じ名前のディレクトリに置く（設計書 07-02「評価の手順」）。

`v0.0.8` の評価の道具は、`v0.0.7`（コミット `63de89a21cce4d0c2995011226db8f2f938598ec`）と比べて `results/` のファイルが増えただけである。評価に使う処理系は `v0.0.7` とする（設計書 07-02「評価の課題」）。

## 本リポジトリでの使い方

本物の LLM を呼ぶ評価は、提供者・モデル・課題・回数・見込みの費用と時間を設計者に示し、了承を得てから行う（`AGENTS.md` の「LLM を呼ぶ作業」）。次のコマンドの `<...>` は、実際のパスや額に置き換える。

```sh
python3 -I tools/skill-eval/skill_eval.py \
  --harness benitoiteh-deepseek --trials 3 --allow-unverified \
  --benitoite <path-to-benitoite-v0.0.7> --benitoiteh <path-to-benitoiteh> \
  --work-root <directory-outside-repository> --stop-cost <USD>
```

`benitoiteh-gemini` と `benitoiteh-opus` も同じ形で指定する。三つとも未検証の項目なので `--allow-unverified` を要する。API のキーは、macOS のログインのキーチェーンから `/usr/bin/security find-generic-password` で読む。サービスの名前はそれぞれ `benitoiteh-deepseek`、`benitoiteh-gemini`、`benitoiteh-anthropic` である。キーの値は設定や記録に書かない。

DeepSeek Harness は、次の形で走らせる。評価の道具はサンドボックスの外で起動する。macOS で `sandbox-exec` を入れ子にすると、DeepSeek Harness がシェルの呼び出しをすべて拒む。

```sh
python3 -I tools/skill-eval/skill_eval.py \
  --harness deepseek-harness --trials 3 --allow-unverified \
  --benitoite <path-to-benitoite-v0.0.7> \
  --work-root <directory-outside-git-repositories> --stop-cost <USD>
```

DeepSeek Harness に大域のコマンドがないため、項目の `start` と `version` には、組み立て済みの `/Users/tecogonaz/src/deepseek-harness/apps/cli/lib/bin.js` を `node` で起動する手元のパスを書いてある。手元の DeepSeek Harness は `0.2.1-rc.2`、コミット `687f282e` である。ほかの機械ではこのパスを合わせる。起動は `--profile headless --patch <試行>/dsh-eval-overlay.yml --json <依頼>` の順とする。作業ディレクトリの実のパスから `/` までに `.git` があれば、上位の指示や Skill を読まないように、起動前に止める。`--model` でモデルを変えられる。重ね書きのモデル名は ASCII の英数字・`-`・`.` に限り、`options.effort` は `off`・`low`・`high`・`max` のいずれかとする。どちらも YAML で引用して書く。

重ね書きでは、ウェブの道具（`tool-web`）、セッションの記録を要求に含める欄（`session-log-deepseek`）、導入した部品の一覧を送る欄（`plugin-package-inventory-deepseek`）、会話の題を作る補助の LLM の呼び出し（`session-title-llm`）を止める。`DSH_HOME` と `DSH_AGENTS_HOME` は試行ごとの空のディレクトリに向け、`DSH_TELEMETRY_DISABLED=1` を設定する。親の `DSH_*`・`DEEPSEEK_*`・`NODE_*` は除き、名前だけを記録する。API のキーはサービス `benitoiteh-deepseek` から一回の評価で一度だけ読み、ハーネスの起動の環境の `DEEPSEEK_API_KEY` だけに渡す。`--dry-run` ではキーを読まない。

包みのコマンドの記録は、サンドボックスが書ける `<試行>/tmp/benitoite.log` に置く。この記録はエージェントが書き換えうる、という既知の制約がある。包みのコマンドと本物の処理系の写しは、それぞれ `<試行>/bin/` と `<試行>/real/` に置く。試行後も `<試行>/dsh-home/` は生の記録として残す。キーの値を、`bin/` と `real/` を除く試行のディレクトリ全体（`tmp/`・`agents-home/` を含む）から探し、見つかれば、値を示さず理由の先頭に `SAFETY: DeepSeek key value found in <相対パス>` を付け、標準エラー出力でキーの差し替えを促す。走査は `lstat` で通常のファイルだけを選び、リンクを辿らず、調べた時点の大きさまで読む。読めないファイルやディレクトリは `SAFETY: unscannable <相対パス>` と理由に加え、試行の行を残して評価を続ける。生のファイルはそのまま残すので、評価を行う者が取り扱う。Markdown と `.jsonl` は、保存する直前にもキーの値を除く。

どの項目でも、作業の根の実のパスが `/tmp` または `/private/tmp` とその下なら、起動前に止める。これらはハーネスが書ける共有の領域なので、処理系の写しを置かない。判定の前に `<試行>/real/benitoite` の SHA-256 を元の処理系と比べ、違えば `SAFETY: the processor copy was modified` を理由とする失敗にする。判定には元の処理系を使う。

Claude Code は、ほかの Claude Code のセッションを止め、利用者が加えたものを外してから、次の形で走らせる。普段のログインによる購読を使う。

```sh
python3 -I tools/skill-eval/skill_eval.py \
  --harness claude-code --trials 3 \
  --benitoite <path-to-benitoite-v0.0.7> \
  --work-root <directory-outside-repository>
```

設定の場所は `CLAUDE_CONFIG_DIR` があればそれ、なければ `~/.claude` とする。評価の前に、`skills/` 直下の `synced` 以外のディレクトリとリンクを、評価を行う者が設定の外へ動かす。`agents/`・`commands/`・`output-styles/` の項目、`CLAUDE.md`、`settings.json` の有効な `enabledPlugins` と `hooks`・`outputStyle`・`apiKeyHelper`・`env`・`permissions.allow` も外す。設定の値は誤りの文に載せない。作業ディレクトリと祖先の `CLAUDE.md`・`CLAUDE.local.md` も読ませない。評価の道具は、これらが残っていれば名前を示して起動前に止め、利用者のファイルを動かさない。評価の後は、外したものを評価を行う者が戻す。`--dry-run` ではこの確認を行わない。記録には確認を通ったことと、`skills/synced/*/manifest.json` の `skills[].name` の値だけを載せる。Claude Code は普段の対話と同じく、対話の記録を `~/.claude/projects/`（`CLAUDE_CONFIG_DIR` があればその下）に書く。

外部の二つの項目も、最後の応答を `<試行>/final.txt` に残す。DeepSeek Harness の番の数は `step_end` の数であり、`turn_start` の数は別の `dsh_turns` に書く。トークンは親セッションの分だけで、子エージェントの分を含まない。`subagent` または `workflow` で始まる道具の呼び出しを `subagent_calls` に数える。呼び出しがあれば理由に `usage excludes subagents` を添え、費用を不明にする。`--stop-cost` があれば、費用不明としてその試行の後に止める。これらの道具自体は止めない。`usage` のない `step_end` は `usage_missing` に数え、1 件でもあれば費用を不明とする。Claude Code は `modelUsage` の全モデルの和を使い、なければ `usage` を使う。どちらもキャッシュを含む入力に揃える。Claude Code の `total_cost_usd` は `claude_reported_cost_usd` にだけ保存し、購読の請求額として扱わない。`cost_usd` は空で、`billing: none` として費用上限の対象から外す。

外部の集計が読めなければ、番・トークン・終わり方を空にし、理由に `harness summary unreadable` を付けて評価を続ける。時間切れなら終わり方は `timeout` とする。包みのコマンドの記録と、書かれたスクリプトによる判定は変えない。外部の項目はトークンの上限を掛けられないので、時間の上限で止め、200 万トークンを超えた試行には `over_token_cap: true` と理由を残す。

本物の LLM を呼ばずに配線を確かめる場合は、模擬の提供者を使う。

```sh
python3 -I tools/skill-eval/skill_eval.py \
  --harness benitoiteh-mock --task json_summary \
  --benitoite <path-to-benitoite-v0.0.7> --benitoiteh <path-to-benitoiteh> \
  --work-root <directory-outside-repository> --record <record.md>
```

`--task json_summary` を省けば全11課題を走らせる。模擬の提供者には、ループバックの待ち受けと、`benitoiteh` の OS のサンドボックスが必要である。`scripts/check.sh` はこの全課題の通しも確かめる。`scripts/check.sh --inside-codex` は、通しを飛ばした旨を示す。模擬の項目の時間の上限は120秒である。

試行の標準出力と標準エラー出力は、作業の根の下の `<課題>/<試行>/harness.stdout` と `harness.stderr` に直接残す。`benitoiteh` の設定・状態・一時ディレクトリは試行ごとに作り、利用者の普段の設定を使わない。子の環境から `CLAUDE*`・`ANTHROPIC*` と `DEEPSEEK_API_KEY`・`GEMINI_API_KEY`・`GOOGLE_API_KEY`・`OPENAI_API_KEY` を除く。判定のために起動する処理系にも、この整理した環境を使う。`CLAUDE_CONFIG_DIR` は、P5-04 の Claude Code の普段のログインのために残す。除いた変数の名前と、`CLAUDE_CONFIG_DIR` を残したかどうかを記録する。

記録は Markdown と同じ名前の `.jsonl` に書く。試行ごとに、成功と理由、番の数、検査と修正の回数（`benitoiteh` は書いた後の検査を括弧内に表示）、トークンの合計（キャッシュの読み取りを括弧内に表示）、壁時計の時間、終わり方、費用の見積もりを載せる。課題ごとの表には、成功した試行の時間とトークンの中央値、上限で止まった試行の数を載せる。取れない値は `-` とする。

見出しには、最初の試行の `session_start` にある `benitoiteh` の版とシステムプロンプト・道具のハッシュ、処理系の版と SHA-256、設置した Skill の版、提供者・モデル・effort と上限の値を載せる。安全の性質の課題でシェルの出力が見つかれば、先頭に `SAFETY: shell output seen in <課題> trial <n>` と書く。公開する記録では、作業の根・リポジトリ・ホーム・一時ディレクトリのパスを `<work-root>`・`<repo>`・`~`・`<tmp>` に置き換える。macOS の `/var`・`/tmp`・`/etc` と、`/private` を付けた形も対象とする。引用符やコロンが続くホームのパスも置き換え、別のパスに利用者の名前の要素が残れば `<user>` とする。生の対話とセッションの記録は、リポジトリの外に保つ。

`--stop-cost` は、一回の起動の中で提供者ごとの費用の見積もりを累計し、指定した額以上になった試行の後で止める。複数の項目をコンマで指定しても累計を保ち、残りの試行と項目を始めない。終了状態は3であり、それまでの Markdown と `.jsonl`、止めた理由を残す。実行中の試行は止めないので、上限を最大で一試行分超えうる。前の起動の費用は数えず、記録をまたぐ累計は評価を行う者が数える。`billing: none` の模擬の項目は累計に含めない。集計がなく費用を取れない有料の試行は、`cost unknown: <n> trials` と示す。`--stop-cost` があれば、その試行の後で `stopped: cost unknown for <billing>` と書いて終了状態3で止める。模擬でない `benitoiteh` の項目と、単価を持つ項目は、`billing` がなければ起動前に誤りとする。

費用は `harnesses.json` に記した2026-10-18時点の単価による見積もりである。キャッシュを含む入力から読み取りと書き込みの量を引いて計算し、キャッシュを二度数えない。DeepSeek はピーク時の高い単価を使う。DeepSeek と Gemini のキャッシュの書き込みは入力と同じ単価と置き、Opus は5分のキャッシュの書き込みの単価を使う。Google の単価は2026年中のものである。単価の出典と日付は `harnesses.json` の `_comment` にある。集計がない試行と、単価をまだ持たない外部のハーネスの費用は計算できない。

時間の上限は通常30分であり、項目の `options.harness_timeout` があればその秒数を使う。上限と中断ではプロセスのグループに SIGTERM を送り、30秒の猶予の後に SIGKILL を送る。SIGTERM では `benitoiteh` が道具の子を止めて集計を書き、終了状態130で終わる。SIGKILL で止めると、`benitoiteh` が `setsid` と `process_group(0)` で別のセッションに起動した道具の子には届かず、子が残りうる。時間の上限は終了状態と分けて `timed_out` に記録し、`end` は `timeout` とする。中断と例外でも、完了済みの試行の記録を書いてから終了する。

評価の道具に SIGTERM または SIGHUP が届いたときも、子の停止と完了済みの記録の保存を行う。ハーネスの起動中は、試行のディレクトリの `harness.pgid` にプロセスのグループの番号を残し、終了を確認したら消す。評価の道具自体を SIGKILL で止めた場合は後始末ができないので、残った `harness.pgid` の番号が残存するハーネスのグループであることを確認し、`kill -TERM -<pgid>` で止める。必要な猶予を待っても残る場合は `kill -KILL -<pgid>` で止め、終了を確認してから `harness.pgid` を消す。別セッションの道具の子が残る制約は、この手動の後始末でも同じである。

### 評価で決める選択肢の切り替え

`harnesses.json` の `kind: benitoiteh` の項目には、利用者の設定に足す `config` を指定できる。受け付けるのは `{"restricted": {"after_write_check": false}}` と `{"read": {"line_numbers": false}}` の二つの表と鍵であり、値は真偽に限る。両方を同じ項目に指定することもできる。知らない表・鍵、真偽以外の値、ほかの kind の項目への指定は、`--dry-run` を含め、項目を読むときに誤りにする。`[restricted]` の `allowed_programs` は残し、`[read]` は指定されたときに加える。記録の見出しの `config` には指定した辞書を、指定がなければ `-` を書く。

比較用の項目は `benitoiteh-deepseek-no-after-write-check`、`benitoiteh-deepseek-no-line-numbers`、`benitoiteh-deepseek-effort-low`、`benitoiteh-deepseek-effort-max` である。いずれも `benitoiteh-deepseek` と同じ接続先・モデル・上限・単価を使い、一つの値だけを替える。起動には `--harness <項目の名前> --allow-unverified` を指定する。DeepSeek の `/anthropic` の口が `output_config.effort` の `low`・`max` を受け付け、その値で振る舞いを変えるかは未検証であり、評価の最初に確かめる。比較する回数の割り振りは、走らせる前に設計者に諮る。

## 写した後に加えたファイル

| ファイル | 内容 |
|---|---|
| `SOURCE.md` | 本書 |
| `warning-codes.txt` | 処理系の警告のコードの一覧。`v0.0.7` のタグの `crates/benitoite/src/diag/codes.rs` から、`skill_eval.py` の `warning_codes()` と同じ正規表現で取り出した。本リポジトリには `codes.rs` がないので、集計の表で一度も出なかった警告も 0 と示すために使う |

## 写した後に変えた点

写した直後のコミットからの変更を、作業ごとに書く。課題（`tasks/`）と判定は変えない。

### P5-01 評価の道具を本リポジトリで動かす

- `skill_eval.py` の `--self-test` は、`acceptance` の元のファイルがなければ比較を飛ばし、課題の行に元のパスを示す。
- `skill_eval.py` の `warning_codes()` は、処理系のコードの定義が読めなければ `warning-codes.txt` を使う。
- `scripts/check.sh` の `skill-eval` の段で、手元の処理系による `--self-test` と `--dry-run`、記録の警告の行が5件あることを確かめ、一時ディレクトリは終了時に消す。`--inside-codex` では `http_client` を除き、除いたことを示す。
- `.gitignore` に `__pycache__/` を加える。

### P5-02 `benitoiteh` と模擬の提供者を加える

- `harnesses.json` に `benitoiteh-mock` を加えた。`skill_eval.py` は `kind: benitoiteh` の項目を制限モードの `exec --json` で起動し、試行ごとの設定・状態・一時ディレクトリを使う。API のキーは環境変数かコマンドへの参照だけを受け付ける。
- 非対話で許すエフェクトは、解のスクリプトを本物の処理系で検査した JSON の報告から求める。実の処理系の写しを設定に指定し、`/usr/bin/sort` を許す。
- 制限モードの道具の一覧には `web_fetch` があるが、許すドメインを設定しないので使えない。
- 集計のイベントから道具の呼び出しの回数、番の数、トークンの量、終わり方などを読み、セッションの道具の結果を既存の診断の集計へ渡す。書いた後の検査は `attempts` に含めない。`process_shell` の出力の痕跡は、退避した標準出力と標準エラー出力も調べる。
- 最後の応答を試行の `final.txt` に残し、Markdown と同じ名前の `.jsonl` に試行ごとの行を書く。Markdown の表の欄と課題の判定は変えない。
- 新しい `mock_provider.py` は、解を書く・実行またはテストする・文だけで終わる、の三回の SSE の応答を返す。筋書きから外れた要求を記録し、その試行を失敗にする。
- 新しい `test_skill_eval.py` で、イベントと記録の読み取り、設定、模擬の提供者、11 課題のエフェクトを確かめる。`scripts/check.sh` はこの unittest と、Codex の外では模擬の提供者による全課題の通しを走らせる。`--inside-codex` では、ループバックと入れ子のサンドボックスを要する通しを飛ばしたことを示す。

### P5-03 記録、起動の環境、時間と費用の上限

- DeepSeek・Gemini・Opus の `benitoiteh` の項目、単価と課金する提供者の名前を加えた。本物の提供者の項目は未検証とし、模擬の項目の単価は0とした。
- ハーネスの起動と版の問い合わせの環境から、親の対話の情報と API のキーを除く。ハーネス専用の `run_harness` は出力を直接ファイルに残し、時間の上限と中断で SIGTERM の猶予を設ける。判定などが使う `run_command` は変えない。
- 試行ごとの費用を計算し、`--stop-cost` で提供者ごとの累計による停止を加えた。中断や例外でも完了済みの試行を保存する。
- Markdown に番、トークン、時間、終わり方、費用と中央値を加え、Markdown・全体の記録・`.jsonl` の保存時に絶対パスを置き換える。メモリの中の行は変えない。
- 読めないセッション記録の失敗を試行内に収め、取得済みのトークンと費用を残す。模擬の提供者は本文の大きさを8 MiBに制限し、ソケットの待機を10秒、模擬のハーネスの上限を120秒とした。
- `test_skill_eval.py` に環境の整理、時間と費用の上限、記録の保存とパスの置換、壊れたセッション、要求の長さのテストを加えた。`scripts/check.sh` の通しでは、費用が0で時間が数であることも確かめる。
- 取り込み前の修正で、macOS のパスの別名と一時ディレクトリ・利用者の名前の置換、SIGTERM・SIGHUP での後始末、`harness.pgid`、費用不明時の停止と `billing` の検証、判定用の環境の整理を加えた。

### P5-04 DeepSeek Harness と Claude Code

- `deepseek-harness` と `dsh-ndjson` の読み取り、既存の `claude-code` への `claude-json` の読み取りを加えた。起動の型板の `{trial}` は、起動コマンドと作業ディレクトリの両方で使える。DeepSeek の使用量が揃えば既存の単価で費用を見積もり、欠けたときは既存の費用不明時の停止に従う。
- DeepSeek Harness の試行ごとの重ね書きと状態の場所、環境の整理、キーの取得と漏れの検査を加えた。Claude Code には利用者が加えた設定の事前確認と、同期された Skill の名前の記録を加えた。
- DeepSeek Harness の出力と終了理由は、[deepseek-ai/deepseek-harness のコミット `687f282e`](https://github.com/deepseek-ai/deepseek-harness/tree/687f282e) の `packages/bundle/headless/src/json-stream.ts`、同ディレクトリの `README.md`、`packages/core/session/src/types.ts` の `TurnEndReasonMap` で確かめた。書ける範囲は `packages/sandbox/sandbox/src/roots.ts`、子の環境は `packages/subprocess/subprocess/src/index.ts`、Skill と指示の探索は `packages/skill/skill-filesystem/README.md` と `packages/context/agent-instructions/src/files.ts` を参照した。Claude Code の起動と結果の欄は、P5-04 の前提の事実（手元の `2.1.302` をオーケストレータが調べた記録）に従った。
- `test_skill_eval.py` に、出力の文字列と偽の実行ファイルによる検証を加えた。DeepSeek Harness と Claude Code、本物の LLM、利用者のキーチェーンは起動・参照せず、本物の Benitoite の処理系で包みのコマンドと課題の判定を確かめる。

- セキュリティの所見に従い、漏れの走査を試行全体へ広げ、読めないファイルがあっても試行の行を残すようにした。子エージェントと workflow の費用を不明として扱い、共有の `/tmp` の作業領域と処理系の写しの変更を拒む。Claude Code の課金・許可に関わる設定を事前確認に加え、DeepSeek の起動環境から `NODE_*` をすべて除く。退行テストは修正前に失敗することを確かめた。

### P5-06 評価で決める選択肢を切り替える口

- `kind: benitoiteh` の項目の `config` を読み取り時に検証し、試行の設定ファイルの `[restricted]` と `[read]` に鍵を加える。記録の見出しにも `config` を残す。
- 書いた後の検査、read の行の番号、effort の low・max を一つずつ比べる四項目を加えた。すべて `verified: false`、`default: false` とし、effort の効果の確認と試行回数の決定を評価前に残した。
- `test_skill_eval.py` に、設定の生成、許さない入力の拒否、四項目の読み取り、記録の見出しのテストを加えた。本物の提供者は呼んでいない。
