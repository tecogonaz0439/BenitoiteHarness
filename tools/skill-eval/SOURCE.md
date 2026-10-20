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

評価の道具に SIGTERM または SIGHUP が届いたときも、子の停止と完了済みの記録の保存を行う。ハーネスの起動中は、試行のディレクトリの `harness.pgid` にプロセスのグループの番号を残し、終了を確認したら消す。評価の道具自体を SIGKILL で止めた場合は後始末ができないので、残った `harness.pgid` の番号が残存するハーネスのグループであることを確認し、`kill -TERM -<pgid>` で止める。必要な猶予を待っても残る場合は `kill -KILL -<pgid>` で止め、終了を確認してから `harness.pgid` を消す。別セッションの道具の子が残る制約は、この手動の後始末でも同じである。Antigravity の試行では、評価の道具自体を SIGKILL で止めると、`.agents/` の下のディレクトリは0555、ファイルは0444のまま残る。残存するプロセスを止めた後、作業の根を消す前に `chmod -R u+w <作業の根>` で書ける状態へ戻す。

### Antigravity の CLI

Antigravity は、普段の Google アカウントのログインとプランの利用枠で走らせる。次のコマンドも、本物の LLM を呼ぶので、設計者の了承を得てから実行する。`--work-root` はリポジトリと `HOME` の外（例 `/Users/Shared/bh-eval/...`）に置く。祖先の `.agents/` を拒むため、`~/.agents/` があるホームの下では試行を始められない。

```sh
python3 -I tools/skill-eval/skill_eval.py \
  --harness antigravity-gemini --trials 3 --allow-unverified \
  --benitoite <path-to-benitoite-v0.0.7> \
  --work-root <directory-outside-home-and-repository> --record <record.md>
```

起動は `agy -p <依頼> --model gemini-3.8-flash-high --effort high --dangerously-skip-permissions --output-format json --print-timeout 1800s --log-file <試行>/agy/agy.log` とする。すべての操作を自動で許し、試行ごとのフックでコマンドとファイルの操作を絞る。理由: `allow` と `permissionOverrides` のいずれも非対話の既定の拒否に優先しなかったため、利用者の設定に許可を書かずに実行する形を設計者が選んだ（P5-07「補足 3」、2026-10-19）。`agy` 自身の時間の上限を1800秒、評価の道具の時間の上限を1860秒とし、途中の応答を書いて終わる猶予を設ける。試行と版の問い合わせが終わった後も、プロセスのグループに SIGKILL を一度送り、同じグループの背後の処理を止める。

フックは作業ディレクトリの `.agents/hooks.json` に、`matcher: ".*"` の `PreToolUse` を一つだけ置く。門は、作業ディレクトリの外の `<試行>/agy/agy_gate.py` を `python -I` で起動し、実の作業ディレクトリを引数に渡す。実行ファイルとパスはシェル用に引用する。門の時間の上限は30秒とする。全操作を自動で許す起動では、門が時間切れや異常終了で失敗すると、その道具は通る（P5-07「補足 3」）。制限時間を延ばして時間切れを起こしにくくし、決定の記録が不足した場合は試行後の照合で検知する。

門は `run_command` と、明示した読み書きの道具だけを許す。コマンドは4096文字以下の `benitoite( [A-Za-z0-9_./=:,+-]+)*` に全体が一致し、第一引数が `check`・`run`・`test` であることを確かめる。各引数を `=`・`,`・`:` でも分け、絶対パスと `..` のパス要素を拒む。zsh のコマンド名の展開を防ぐため、引数そのものや区切りの後が `=` で始まる形も拒む。`Cwd` は作業ディレクトリかその下の絶対パスに限る。`skill install` など、ホームへ書き込めるサブコマンドは許さない。

読み取りの道具は `view_file`・`list_dir`・`grep_search`・`find_by_name`・`view_file_outline`・`view_code_item` とする。書き込みの道具は `write_to_file`・`replace_file_content`・`multi_replace_file_content` とする。入れ子の辞書も含め、鍵名が `path`・`file`・`directory`・`uri` で終わるものをパスの鍵と推し量る（大文字と小文字を区別しない）。`File` そのものも含め、`NodePaths`・`SecurityNote` は含めない。パスの鍵の値が真偽値か数なら、表示などの指定として検査から外す。文字列か空でない文字列の一覧ならすべてのパスを検査し、それ以外の型は拒む。検査するパスが一つもなければ、読み取りも書き込みも拒む。`CodeContent`・`TargetContent`・`Query` など、ほかの鍵の値はパスとして扱わない。

パスは `/` または小文字の `file://` で始まり、実のパスで作業ディレクトリ内に収まるものだけを許す。相対パス、`~`、ほかの scheme、先頭か末尾の空白・制御文字、NUL を拒む。URI の復号後も同じ文字の検査を行う。書き込みでは、すべてのパスについて、相対パスの要素に `.agents`・`_agents`・`.agent`・`_agent`・`.gemini`・`.git` があるものと、末尾が `AGENTS.md`・`GEMINI.md` のものを拒む（大文字と小文字を区別しない）。ほかのすべての道具は拒む。実走で確かめた引数の鍵は後述する。それ以外の道具の鍵名の推し量りは未検証である。

決定は `<試行>/agy/gate.log` に JSON Lines で残す。時刻、決定、道具の名前、入れ子を含む引数の鍵の名前の一覧、`CommandLine` の先頭200文字だけを書く。ほかの引数の値は書かない。ログは準備で作り、門は `O_APPEND|O_NOFOLLOW|O_NONBLOCK` で開いて、通常ファイルであることを `fstat` で確かめる。ログがない・形が違う・追記できない場合は、拒否を返す。会話の記録に道具の呼び出しがあるのに有効な門の記録がない場合は、`SAFETY: hooks were not loaded` を理由に加える。包みのコマンドの記録に行があるのに、門の記録に `run_command` の `allow` がない場合も同じとする。後者は会話の記録が読めなくても働く。

試行の後、会話の記録の `PLANNER_RESPONSE` の `tool_calls` と、`gate.log` の `allow`・`deny` の行を道具の名前ごとに数える。道具ごとに、呼び出しの数から決定の数を引き、正の差だけを足す。不足があれば理由の先頭に `SAFETY: tool calls without a gate decision (<不足の合計>)` を加える。別の道具の余分な決定では不足を相殺しない。会話の記録がないか読めない場合と、集計が読めず会話の記録を特定できない場合は、`SAFETY: gate coverage unverifiable` を先頭に加える。これは試行後の検知であり、門が失敗した時点の操作を止めるものではない。

起動前に `.agents/` のファイルを0444、ディレクトリを0555にする。理由: フックや Skill の偶発的な編集を起こしにくくするためである。同じ利用者のプロセスは権限を戻せるので、事後の比較も行う。ハーネスが終わった後は、例外の場合も、片付けと記録の前にファイルを0644、ディレクトリを0755へ戻す。リンクの参照先は変更しない。親ディレクトリの記述子を基準に `follow_symlinks=False` の `chmod` を使い、対応していなければ `O_NOFOLLOW` で開いた記述子の `fchmod` を使う。消えた項目などの失敗は `<試行>/agy/permissions.log` に記録し、ほかの項目の処理と試行の記録を続ける。公開する理由の欄には失敗の件数だけを書く。

試行の前後で、`hooks.json` のバイト列、設置した Skill の `skill_digest`、`.agents/` の項目の一覧、包みのコマンドと門の SHA-256 を比べる。違えば理由の先頭に `SAFETY: <対象> was modified` を付ける。作業ディレクトリ全体でも、上記の設定ディレクトリ名や指示ファイル名が新しく現れれば `SAFETY: <印の名前> was created (<件数>)` を加える。印は `.agents`・`AGENTS.md` などの固定の名前に揃え、途中のディレクトリ名は理由に書かない。準備で設置したものと課題の初めからあるものは除く。書き換えて元に戻した変更は検知できない。会話の記録に許す一覧の外の道具があれば、理由に警告を書く。

この門が検査するのは、道具の呼び出しの形と、道具の引数に現れるパスである。`benitoite run` のスクリプトの中のパスとプロセスは検査しない。`--allow-effects=<Process のエフェクト>` を指定したスクリプトは、任意のコマンドを起動できる。この範囲は Claude Code の `Bash(benitoite:*)` と同じものとして受け入れる。すべての kind の試行で、ホームの `.claude/skills/benitoite` と `.agents/skills/benitoite` の有無、種類、mtime（下位の項目を含む）を比べ、違えば `SAFETY: user skill directory changed` を先頭に加える。ファイルの内容は読まない。この二つの設置先は、[Benitoite v0.0.7 の skill/mod.rs](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.7/crates/benitoite/src/cli/tools/skill/mod.rs) の `agent_parent`・`target_dirs`・`parse_skill_args` で確認した（コミット `63de89a21cce4d0c2995011226db8f2f938598ec`）。

試行の準備では、利用者が加えた指示ファイル、スキル・プラグイン・規則・フック・ワークフロー・MCP のサーバなどの設定を拒む。祖先の `.agents/`・`.agent/`・`_agents/`・`_agent/`、指示ファイル、`.gemini/settings.json`、管理者設定の `/Library/Application Support/Antigravity/admin_settings.json` も拒む。調べる範囲はホームの `.gemini/` 直下、`config/`、`antigravity-cli/` 直下に限り、`brain/` の会話やトークンの領域は歩かない。同梱 Skill の名前は `builtin/skills/` 直下から取る。誤りにはパスと鍵の名前だけを示し、設定値は示さない。製品が同梱するスキルは残し、名前の一覧と設定の確認に通った旨を見出しに書く。`--dry-run` ではこの確認も `agy` の起動も行わない。

設定の根の鍵は、`settings.json` では `model`・`trustedWorkspaces`、`config/config.json` では `userSettings`、`config/projects/*.json` では `id`・`name`・`projectResources` に限る。`userSettings` の下は `remoteControlHostname` だけを許す。一覧にない鍵は、綴りの違う `useG1Credits` も含めて拒む。`projectResources` は `resources` の一覧を持つ辞書として読み、各項目の `folderUri` と `gitFolder.folderUri` の両方について、作業の根かその祖先・子孫との重なりを拒む。`projectResources` の下は `resources`、各項目は `folderUri`・`gitFolder`、`gitFolder` の下は `folderUri`・`allowWrite` だけを許す。一覧を直接置いた形や、入れ子の知らない鍵も誤りとする。`antigravity-cli/mcp_config.json` は空か `{}` だけを許す。

子の環境から `AGY_*`・`ANTIGRAVITY_*`・`JETSKI_*`・`GOOGLE_*`・`GEMINI_*`・`CLOUDSDK_*` を除き、除いた名前を見出しに書く。`AGY_CLI_DISABLE_AUTO_UPDATE=1` と `TMPDIR=<試行>/tmp` を設定する。版の問い合わせにも同じ整理を行う。API の単価は置かず、`billing: none`、`cost_usd: null` とする。`--stop-cost` ではプランの利用枠の残量を管理できない。

2026-10-19 に設計者の了承を得て行った最初の試しの走行では、次の事実を確かめた。対象は `json_summary` の1試行、起動時の版は `agy` 1.4.0 である。出典は [P5-07 の「補足 2」](../../docs/plan/tasks/P5-07-antigravity.md#補足-22026-10-19最初の試しの走行の後) のオーケストレータによる調査結果である。同節が挙げる試行の記録は `2026-10-19-0642-antigravity-gemini-trial.md` である。

- `-p` と `--mode accept-edits` のもとでフックが読み込まれ、`matcher: ".*"` の門が全26回の道具の呼び出しで起動された。
- 門が `benitoite check main.bnt` に `allow` を返しても、非対話の既定の拒否に優先せず、`command` の権限不足として走行が終わった。最後の応答は空で、`denied_actions` に `command` が残った。門が `ls -la` に `deny` を返したときは走行が続いた。
- `view_file` の引数は `AbsolutePath`・`StartLine`・`EndLine`・`ContentOffset`、`write_to_file` は `TargetFile`・`CodeContent`・`Description`・`Overwrite`、`run_command` は `CommandLine`・`Cwd`・`WaitMsBeforeAsync` だった。各道具には `toolAction`・`toolSummary` も加わった。
- `AGY_CLI_DISABLE_AUTO_UPDATE=1` を設定しても背後の更新が起動し、走行開始の9秒後に利用者の `agy` が1.4.2に置き換わった。文書にある更新の止め方はない。
- 利用者の `~/.gemini/config/` と `~/.gemini/antigravity-cli/settings.json` は変わらなかった。

2回目の試しの走行（`agy` 1.4.2）でも、門が検査済みの `benitoite check main.bnt` に `permissionOverrides: ["command(benitoite check main.bnt)"]` を添えて `allow` を返したが、`command` の権限不足として走行が終わった。出典は [P5-07 の「補足 3」](../../docs/plan/tasks/P5-07-antigravity.md#補足-32026-10-192-回目の試しの走行の後) のオーケストレータによる調査結果である。同節が挙げる試行の記録は `2026-10-19-0710-antigravity-gemini-trial2.md` である。この結果を受け、起動を全操作の自動許可に替え、門の `PERMISSION_OVERRIDES` を `False` に戻した。許可が利用者の設定に残るかは未確認であり、この起動では不要なので返さない。

許可が設定に保存される場合を検知するため、各試行の前後で `~/.gemini/config/config.json`・`mcp_config.json`・`projects/*.json` と `~/.gemini/antigravity-cli/settings.json` の SHA-256、および `~/.gemini/config/` 直下の項目の一覧を比べる。変更か事後の比較不能を検知すると、理由の先頭に `SAFETY: user Antigravity settings changed` を付ける。設定値とハッシュは公開する記録に残さず、`agy_settings_changed` の真偽だけを残す。試行中に書き換えて元に戻した変更は検知できない。

自動更新は止めず、試行の起動の前後に `agy --version` を問い合わせ、行の `harness_version_start` と `harness_version_end` に書く。見出しには最初の試行の起動前の版を使う。前後の版が異なる場合は理由に `WARNING: harness version changed <前> -> <後>` を加える。設定か版の変化を検知したら、後続の試行と項目を始めず、完了済みの Markdown と `.jsonl`、見出しの停止理由を保存して終了状態3で終える。利用者の実行ファイルと設定には手を加えない。

集計は標準出力のオブジェクトから読み、全体を読めなければ最後の空でない行を読む。入力にキャッシュの読み取りを加え、キャッシュの書き込みは0とする。`thinking_tokens` は別の欄に残し、出力に含まれるか未確認なので合計には足さない。`num_turns` は利用者の発言数なので番として使わず、ホームの `brain/<conversation_id>/.system_generated/logs/transcript_full.jsonl`（なければ `transcript.jsonl`）の `PLANNER_RESPONSE` の数と、その道具の名前を数える。ID の文字種と記録の実のパスを検査する。会話の記録は `O_NONBLOCK` で開き、通常ファイルだけを読む。深い JSON の `RecursionError` も欠損として扱う。記録が読めなければ番と道具の回数を `null` にし、使用量が標準出力と違えば理由に記す。使用量は標準出力を正とする。

終わり方は、評価の道具の時間切れ、`agy` の print timeout、終了状態3または `AGY_ERROR`、終了状態0かつ `SUCCESS`、ほかの誤りの順に判定する。`AGY_ERROR` の JSON は `status`・`code`・`retryable`・`error_id`・`short_error` の値だけを残す。JSON として読めないときは文を使い、どちらもメールアドレスを `<email>` に伏せて先頭200文字を残す。道具の名前が `[a-z0-9_]{1,64}` に当たらなければ `<invalid>` として数える。集計が読めなくても課題の判定は続け、読めないトークンの欄は `null` にする。

次の試しの走行で、残る点を確かめてから評価を広げる。補足 3 の実装では本物の LLM を呼ばず、項目は `verified: false` のままとする。

- `agy` 1.4.2 の全操作を自動で許す起動でも、門の `deny` が効くことを最初に確かめる。
- `.*` の門が拒む `command_status`・`task_boundary`・`notify_user` などで、走行が止まらないか。
- まだ実走で使われていない道具の名前と、`gate.log` に記録した引数の鍵の名前が、門のパスの検査に合っているか。
- パスの鍵としない `grep_search` の `Includes`、`find_by_name` の `Pattern`・`Excludes` に置いたパスや `..` を、agy が検索の根の外に向けて解かないか。
- 試行の途中で書き換えたフックを読み直すか。
- `run_command` の子が別のプロセスのグループに残るか。同じグループの終了後の SIGKILL では、その子は止められない。

フックと Skill と設定の探索の出典は、`agy` 1.4.0 の同梱文書 `~/.gemini/antigravity-cli/builtin/skills/agy-customizations/docs/` の `hooks.md`・`skills.md`・`json_configs.md` である。JSON の集計、会話の記録、時間切れの文言、課金と環境変数は、P5-07 の「前提の事実」と「補足」の、オーケストレータによる2026-10-18の調査記録に従った。

### 評価で決める選択肢の切り替え

`harnesses.json` の `kind: benitoiteh` の項目には、利用者の設定に足す `config` を指定できる。受け付けるのは `{"restricted": {"after_write_check": false}}`、`{"read": {"line_numbers": false}}`、`{"agent": {"completion_check": true}}` の三つの表と鍵であり、値は真偽に限る。同じ項目に組み合わせて指定することもできる。知らない表・鍵、真偽以外の値、ほかの kind の項目への指定は、`--dry-run` を含め、項目を読むときに誤りにする。`[restricted]` の `allowed_programs` は残し、`[read]` と `[agent]` は指定されたときに加える。記録の見出しの `config` には指定した辞書を、指定がなければ `-` を書く。

比較用の項目は `benitoiteh-deepseek-no-after-write-check`、`benitoiteh-deepseek-no-line-numbers`、`benitoiteh-deepseek-effort-low`、`benitoiteh-deepseek-effort-max` である。いずれも `benitoiteh-deepseek` と同じ接続先・モデル・上限・単価を使い、一つの値だけを替える。起動には `--harness <項目の名前> --allow-unverified` を指定する。DeepSeek の `/anthropic` の口が `output_config.effort` の `low`・`max` を受け付け、その値で振る舞いを変えるかは未検証であり、評価の最初に確かめる。比較する回数の割り振りは、走らせる前に設計者に諮る。

### 完了の前の確かめの比較（BH-011）

`benitoiteh-deepseek-completion-check` は `benitoiteh-deepseek` に `config.agent.completion_check: true` だけを加えた条件である。`verified: false`・`default: false` のまま置く。比較の両側を同じ `--benitoiteh` で起動し、過去の試行を使い回さない。省略した側の実効値は `false` である。両側とも制限モード、DeepSeek-V4.1-Flash（`deepseek-flash`、Anthropic 互換）、effort `high`、行の番号あり、書いた後の検査あり、文脈の窓 200,000、1 試行の上限 50 番・200 万トークン・30 分を使う。

本物の提供者を呼ぶ前に、設計者へ回数・費用・時間・停止額を示して了承を得る。[設計書 07-02「M の段の残りの規模と費用」](../../docs/design/07-quality/07-02-evaluation.md#m-の段の残りの規模と費用)の条件は、試しの走行が同じ 1 課題で各条件 1 回の計 2 回、本体が `json_summary`・`file_process`・`error_handling`・`write_tests` の各条件 3 回の計 24 回である。計 26 回で $0.31〜0.52、13〜52 分を見込み、試しの走行と本体を合わせた停止額は $0.60 とする。別の起動では、使用済みの費用を差し引いた残額を `--stop-cost` に渡す。試しの走行の後に見積もりを見直し、本体の前に改めて了承を得る。

了承を得た後の本体の指定例は次のとおりである。`<残額>`、処理系と実行ファイルのパス、リポジトリの外の作業場所を、走行前に確定する。

```sh
python3 -I tools/skill-eval/skill_eval.py \
  --harness benitoiteh-deepseek,benitoiteh-deepseek-completion-check \
  --allow-unverified --interleave --trials 3 \
  --task json_summary --task file_process --task error_handling --task write_tests \
  --benitoite <v0.0.7の処理系> --benitoiteh <両条件で共通の実行ファイル> \
  --work-root <リポジトリの外の作業場所> --stop-cost <残額>
```

`--interleave` は複数条件にだけ使える。同じ課題・反復の全条件を実行してから次の組へ進み、（課題, 反復）の組の通し番号で条件の先頭を回す。課題が変わっても回転を戻さない。2 条件・各課題 3 反復なら、最初の課題は A→B、B→A、A→B、次の課題は B→A、A→B、B→A となる。各行の `repetition` と `condition_order` に反復と全体の起動順、`condition_position` に組の中で何番目に起動したか（1 から始まる）を残す。全体の記録の `起動順` にも組の中の位置を書き、中断した試行は `completed: False` として残す。費用は条件をまたいで同じ提供者の累計に足す。費用不明でも後続を止め、上限は試行の後に判定するので最後の 1 試行分は超えうる。

試行ごとの `.jsonl` には次を加える。促しの境は、環境の発言の本文が `prompts/completion-check.md` を `<completion_check>` で囲んだ文に一致する行である。見出しの `completion_check_sha256` は、評価が照合に使う包んだ文の UTF-8 の SHA-256 である（末尾の `</completion_check>` の後に改行を足さない）。環境の部分が `<completion_check>\n` で始まるのに文が一致しなければ、`CompletionPromptMismatch` としてその試行を失敗にし、次の試行を続ける。これにより、別の版の実行ファイルが送った促しを、黙って 0 回として集計しない。記録の行・環境の発言・部分の型が違う場合も、その試行の記録を読めないものとして失敗にする。

- `completion_prompts`: 固定の促しの回数。
- `verification_calls_before`・`verification_calls_after`: `benitoite_check`・`benitoite_run`・`benitoite_test`・`benitoite_scratch` の名前ごとの回数。`validation_error` がない `tool_call` だけを数え、単なる検査済みという文や `tool_result` の行は数えない。促しのない側の `after` はすべて 0 とする。
- `edits_after_prompt`: 同じ規則で数えた、促しの後の `write` と `edit` の合計。

`verification_calls_before`・`verification_calls_after` と `edits_after_prompt` は、引数の検証を通った、要求された呼び出しの数である。出力の上限、取り消し、トークンの上限、拒否などで実行されなかった呼び出しも含む。実際の実行回数や検査の成功回数を表すものではない。`tool_result` にも実行されなかった呼び出しの結果が残るため、結果の行を数えても実行回数にはならない。

書いた後の自動の検査は、既存の `after_write_checks` に分けて残す。最後の応答は試行の `final.txt` に保存する。未検証の報告の有無は自動で採点しない。要約の記録を作る者が `final.txt` を読み、試行ごとに報告の有無、判断の理由、判定した者を記す。成功率と、促しの前後の実行・修正、余分な番・トークン・時間・費用を比べる。差が小さい場合の追加評価と BH-011 を閉じる判断は設計者に委ねる。

`benitoiteh` の起動と版の問い合わせには、親の `HOME` をそのまま渡す。理由: 制限モードの読ませない場所と資格情報の探索に、利用者のホームを使うからである。`XDG_CONFIG_HOME`・`XDG_STATE_HOME`・`TMPDIR` は試行の中へ向ける。テストでは `scripts/check.sh` の `skill_eval_python` が Python の処理全体を一時のホーム・設定・状態・一時領域で起動し、利用者の設定とキーチェーンを使わずに確かめる。

本物の LLM を呼ばない通しには `benitoiteh-mock-completion-check` を使う。既存の模擬の 11 課題とは別に、`scripts/check.sh` が `json_summary` の 1 課題を実行する。3 番目の `Done.`、固定の促し、4 番目の検査・実行、5 番目の `Done.` まで進み、前後の呼び出しが各 1 回となることを確認する。`--inside-codex` では、この通しを既存の模擬の通しとともに除く。

## 写した後に加えたファイル

| ファイル | 内容 |
|---|---|
| `SOURCE.md` | 本書 |
| `agy_gate.py` | Antigravity の `PreToolUse` で全道具を検査し、決定を試行ごとに記録する門 |
| `warning-codes.txt` | 処理系の警告のコードの一覧。`v0.0.7` のタグの `crates/benitoite/src/diag/codes.rs` から、`skill_eval.py` の `warning_codes()` と同じ正規表現で取り出した。本リポジトリには `codes.rs` がないので、集計の表で一度も出なかった警告も 0 と示すために使う |

## 写した後に変えた点

- P5-34 で通常モードの組を追加した。`normal_csv_totals` と `normal_path_filter` は本リポジトリの自作の課題であり、従来の 11 課題の出典は変えない。`kind: normal` の形、入力と成果物の分離、原本の公開のテストと別の入力での判定、パス・変更範囲・生成物の検査を加えた。
- `benitoiteh-deepseek-normal` と `benitoiteh-mock-normal` を追加し、`exec --mode normal --approval never`、通信を拒む workspace-write、利用者の Skill とプラグインを読まない設定、完了の促しなしで起動する。通常モードの模擬は既存のファイルを read してから write し、bash で公開の確認を行う。
- 判定を P5-31 の `skill_eval_judge` へ接続し、通常モードと既存の Benitoite・圧縮の判定で時間・出力の上限と後始末の結果を共通に扱う。コマンドごとに新しい場所を使い、信頼する検査は書ける場所の外に置く。`--judge-runner` を追加した。2026-10-21 のオーケストレータの決定に従い、実行器に模擬の localhost のポートだけの外向きの例外を加えた。製品の起動経路は設定せず、Linux は指定を拒む。読み取りと起動先を列挙する実行器の引数も加え、既存の課題の制約を保った。
- 通常モードの記録に成果物の合否、確認不足、実際に起動した bash の回数と終了状態、変更したファイル、判定ごとの結果、必要な道具の実のパスと SHA-256 と取得できる版を加えた。Benitoite の検査と修正の回数は null とし、種別ごとに成績を示す。費用は従来どおり stdout の summary から求め、セッションの圧縮の記録の usage は足さない。

写した直後のコミットからの変更を、作業ごとに書く。従来の 11 課題（`tasks/`）の入力と期待する出力は変えない。

### P5-32 圧縮の組と段の実行器

既定の組 `--suite benitoite` は従来の 11 課題だけとする。`--suite compaction` は `kind: staged` の課題だけを選び、`--task` もその組の中で検証する。`kind: normal` は後続の組のために予約する。元の 11 課題の入力と期待する出力、写した元のタグとコミットは変えていない。具体的な圧縮の 2 課題、出典・生成の引数とハッシュは後の「P5-33 圧縮の二課題と資料」に記す。

`config.compaction` には `reserve_tokens` と `keep_tokens` だけを許す。真偽を除く 0〜2**64−1 の整数とし、提供者の `context_window` と `max_output_tokens` は同じ範囲の正の整数とする。これらは項目の読み取り時に検証するので dry-run でも誤りを拒む。文脈の窓は指定があるときにその値を使い、省略時だけ 200,000 とする。

`benitoiteh-deepseek-compact-base`・`reserve`・`keep` を、`benitoiteh-deepseek` から作った。すべて未検証・既定の対象外で、effort `high`、文脈の窓 65,536、出力の上限 8,192、完了の促しなしとする。予備・残す量は順に (16,384, 20,000)、(8,192, 20,000)、(16,384, 8,000) であり、閾値は 40,960・49,152・40,960、残す量の実効の上限は 16,384・16,384・8,000 となる。

段の課題の `task.json` は次の形を使う。パスは課題のディレクトリからの相対パスで、通常のファイル・ディレクトリだけを許し、途中のリンク、`..`、絶対パス、特殊ファイルを拒む。`script` は作業ディレクトリ直下の `.bnt` のファイル名とする。

```json
{
  "kind": "staged", "result_kind": "run", "script": "main.bnt",
  "attempt_limit": 10, "http": false,
  "stages": ["stage1.md", "stage2.md", "stage3.md"],
  "judge_cases": [{
    "name": "early-rule", "input": "cases/early-rule/input",
    "args": ["alternate.json"],
    "expected_stdout": "cases/early-rule/expected.stdout",
    "expected_files": "cases/early-rule/expected.files"
  }],
  "self_test_failures": [{"script": "negative/forget.bnt", "case": "early-rule"}]
}
```

`expected_files` は期待する生成ファイルを置くディレクトリで、ケースごとに空のディレクトリも指定できる。ケースの 3 種のパスは `cases/<name>/` の中に限る。`args` は文字列の配列で、そのまま処理系へ渡す。判定はケースごとに新しいディレクトリを作り、原本の `input/` にケースの入力を重ね、最終のスクリプトだけを置く。JSON の生成ファイルは値で比べる。失敗はケース名付きで記録する。self-test は正しい解が全ケースを通ることと、否定の対照が検査を通り、指定のケースで出力か生成ファイルの違いによって落ちることを両方確かめる。検査や実行時の誤り、別のケースだけの失敗は成功にしない。段の課題は `task.md` の存在を検証し、最上位の `expected.stdout`・`expected.files/` を拒む。期待はケースだけに置き、引数なしの確認も `args: []` のケースで表す。

資料は課題の `materials/1/`・`2/`・`3/` に置く。`input/` の下に `materials/` を置くことは禁止し、段 N の直前にだけその段の資料を workspace へ写す。第 1 段の本文は既存の `PROMPT`、空行、段の本文の順とし、第 2・3 段は段の本文だけを渡す。dry-run だけは全段の準備の後に正しい解を置いて判定する。

設定ファイル、workspace、環境、模擬の待ち受けは試行で一つずつ作る。第 1 段の最初の `session_start` の ID を取り、次の段に `exec --resume <ID>` を使う。`--project` は付けない。送り先、モデル、モード、プロンプトと道具のハッシュ、圧縮と促しの設定が段をまたいで変わらず、再開の印が正しいことを照合する。50 番、200 万トークン、1,800 秒を全段で共有し、より小さい項目の上限があればそちらを使う。正常に終わらない段の後に依頼を送らない。

段の使用量は各段の標準出力の最後の `summary` を足す。入力にはキャッシュ分が既に入るため、トークンの上限は input＋output の和とする。セッションの `summary` は累積するので集計に使わず、`benitoiteh_results` は全段の後に一度だけ呼ぶ。費用は段ごとにも計算し、既に終わった試行の累計＋今の試行の途中の費用を、累計を変えずに判定する。行を最後に一度だけ費用の累計へ加える。残額がないか有料の費用が不明なら、後の段と試行を止める。停止額を超える最後の段の費用は発生しうる。

試行の `.jsonl` は 1 行で総計を持ち、`stages` に段別の使用量・費用・時間・終了状態・渡した上限・`session_start` を残す。`exec_launches` はその試行の起動数、`compactions` は成功した圧縮の回数である。`compaction_events` は理由、段、番、見積もりの前後、要約・保持した項目の数、`kept_estimate` と `usage` を持つ。`compaction_records` と全件を順に照合し、`compaction_records_match` に回数と内容の一致を残す。超過はまだ終わっていない番へ、閾値と再開は次の番へ対応させる。次の番がなければ `turn: null` と理由を残す。

2026-10-20 のオーケストレータの決定に従い、要約の要求の総数の欄は設けない。失敗・取り消し・上限で止まった圧縮の使用量は、段の `summary` の合計には入るが、圧縮の記録とイベントには残らない。成功した圧縮の `usage` も既に `summary` に入っているため、費用や使用量に足し直さない。フォークした記録は元の圧縮の `usage` も写すので、記録の使用量を足す方法は使わない。模擬だけは、受けた要約の要求数を `mock_summary_requests` に残す。

成果物の合否は `artifact_success` と判定の理由に残し、比較の成立は `comparison_valid` と `comparison_reasons` に分ける。圧縮なし、現行値と残す量を減らす条件で段をまたいだ圧縮の保持した項目・量が 0、予備の 2 条件が同じ実効値になる場合、記録の不一致、段の未完了、dry-run は比較不成立とする。予備だけを減らす条件には保持量を求めず、圧縮の時点を比べる。成立の印だけで条件間の差が証明されたとは扱わず、有料の試しで保持した量と圧縮の時点の差を確かめる。

再開直後の判定は提供者の固定の使用量でなく、システムプロンプト・道具・会話のバイト数からの見積もりを使う。したがって、模擬の大きい `input_tokens` だけで段の境の圧縮を起こせるとは限らない。P5-33 の資料の量の選び方は、後の「再開と残す量を確かめる境界」に記す。`kept_estimate` は再構築前の選んだ範囲の量であり、推論を除いた後の実際の送信量とは違う。

`benitoiteh-mock-compact` は各段で資料を読み、解を書き、実行し、完了する筋書きを使う。通常の応答の固定の入力は 45,000、出力は 50、要約は入力 1,000・出力 50 とする。要約の依頼はリポジトリの `prompts/compaction.md` と照合し、段の筋書きを進めずに応答する。これは再開と集計の配線を確かめる模擬であり、本物の LLM の課題の達成率や圧縮の効果を表さない。

P5-32 で加えた宣言したエフェクトの検査は保つ。生成したコードの `check --report=json`（テストでは `test --check --report=json`）の完全な `aggregateEffects.expanded` が、正しい解の `task_allow` の部分集合であるときだけ実行する。P5-34 で実行・テストと段の課題の判定を `skill_eval_judge` に統合し、macOS の `/usr/bin/sandbox-exec` で囲む。実行器自身には親の HOME・XDG の場所を知らせ、秘密の候補を除外させる。判定の子へは親の環境を引き継がず、HOME・XDG をコマンド専用の一時の場所の下、TMPDIR をその専用の場所へ向け、PATH を `/usr/bin:/bin` に限る。読み取りは OS・判定・専用の一時の場所・信頼する検査の根、必要な実行ファイルと `/` 自身の literal だけに許す。`/` と親 HOME を包含する読み取りの根を拒み、`--allow-program` には `--read-root` を必須にする。書き込みは判定の場所、専用の一時の場所、`/dev/null` のデータに限る。起動は Benitoite の課題では実の処理系と固定した補助プログラムだけに許す。`process_run` は `/usr/bin/sort`、`process_shell` は `/bin/sh`・`/bin/bash`・`/usr/bin/grep`・`/usr/bin/cut`・`/usr/bin/sort` を許す。通信は `http_client` の模擬の localhost のポートへの外向きの接続だけを例外とし、ほかは拒む。macOS 以外とサンドボックスの準備の失敗は判定の失敗にする。

`benitoiteh` の起動と版の問い合わせでは、`PWD`・`OLDPWD` と、値に評価元のリポジトリのパス（実のパスと macOS の別名）を含む変数を除く。除いた名前は `environment_removed_paths` に残す。セッションの記録と退避した出力に、リポジトリのパスまたは `tools/skill-eval/tasks` があれば、`WARNING` と `repository_path_seen: true`・`comparison_excluded: true` を残す。成果物の判定は保ち、圧縮の比較は不成立とする。場所の推測などによる読み取りを完全に防ぐ仕組みではない。XDG を変更しても普段の秘密を読ませないよう、`benitoiteh` の拒否の候補にも HOME の既定の設定・状態のディレクトリを加えた。

課題は kind の分類と選択の後に検証する。分類できない JSON の未選択の課題は警告して除き、明示した課題なら誤りにする。段の実行器の誤りや起動の失敗は、費用が不明でも `end: error` と理由を保ち、費用の停止を `budget_stop` に分ける。起動の失敗の理由には段の標準エラー出力を使う。

### P5-01 評価の道具を本リポジトリで動かす

- `skill_eval.py` の `--self-test` は、`acceptance` の元のファイルがなければ比較を飛ばし、課題の行に元のパスを示す。
- `skill_eval.py` の `warning_codes()` は、処理系のコードの定義が読めなければ `warning-codes.txt` を使う。
- `scripts/check.sh` の `skill-eval` の段で、手元の処理系による `--self-test` と `--dry-run`、記録の警告の行が5件あることを確かめ、一時ディレクトリは終了時に消す。`--inside-codex` では当初 `http_client` を除いていた。P5-32 の再レビュー後は判定のサンドボックスが入れ子にできないため、全組の self-test・dry-run を外の macOS で走らせる。
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

### P5-07 Antigravity の項目を整える

- `antigravity-gemini` を全操作の自動許可と試行ごとの一つの門で起動する形に改めた。`prepare: agy`、`agy-json`、プランの課金の扱い、時間の上限、記録のパスを加えた。`antigravity-gpt-oss` の項目と、写した元の `README.md` は変えていない。
- 門、利用者の設定の事前確認、試行の後の改変検知、会話の記録による番と道具の集計を加えた。トークンの量、拒否した操作、思考の量、最後の応答、終了理由を記録する。試行と版の問い合わせの環境を整理し、終了後のプロセスのグループを止める。
- テストは偽のホームと偽の `agy`、手で組み立てた集計と会話の記録を使う。生成したフックをシェルから起動して門の判断を確かめ、包みのコマンドと判定には本物の Benitoite v0.0.7 を使う。本物の LLM、`agy`、トークンとキーチェーンは使っていない。

- オーケストレータのセキュリティの所見に従い、門を全道具共通にし、引数の部分パスと読み書きのパスを検査した。設定の鍵を許可の一覧に替え、読む設定の範囲を限定した。読み取り専用の `.agents/`、新しい設定の作成とフック未読込の検知、すべての kind の利用者向け Skill のメタデータ比較を加えた。JSON の深さと特殊ファイルを欠損として扱い、ログの追記先と、記録に残す誤りの欄を限定した。

- 再レビューの所見 A〜I に従い、パスを鍵名で判定し、本文や検索語をパスとして扱わないようにした。実際の `projectResources.resources` の形と入れ子の許可の一覧に合わせ、権限変更のリンクの追跡と失敗の扱い、包みの起動に対応する門の許可の確認、作成した印の名前と件数だけの報告を改めた。テスト全体の HOME・設定・状態・一時領域も一時ディレクトリに向けた。

- 補足 2 に従い、検査済みのコマンドに `permissionOverrides` を加えた。利用者の設定と各試行の前後の版を比較し、変化を検知したら記録を保存して後続を止める。最初の試しの走行で確かめたフック、権限、引数、自動更新と設定の状態を記録し、未検証の項目を改めた。

- 補足 3 に従い、`--mode accept-edits` を `--dangerously-skip-permissions` に替え、`PERMISSION_OVERRIDES` を `False` に戻し、門の時間の上限を30秒にした。門の失敗時も道具が通るため、試行後に会話の記録と門の決定を道具ごとの件数で照合し、不足や照合不能を理由の先頭に記録する。包みのコマンドと `run_command` の `allow` の照合も残した。

### P5-09 利用者の Agent Skill

- `benitoiteh` の評価の設定に `[skills] user = false` を加え、利用者の Skill を読まないようにした。

### P5-10 openai-chat の reasoning_effort

- `benitoiteh-gemini` の `provider.capabilities.reasoning_effort` を真にし、生成する設定に `capabilities = { reasoning_effort = true }` が入ることを確かめるテストを加えた。

### P5-23 Agent Plugins の Skill

- `benitoiteh` の評価の設定に `[plugins] user = false` を加え、利用者のプラグインを読まないようにした。生成する設定のテストも合わせて更新した。

### P5-29 完了の前の確かめと条件の順

- 利用者だけの `[agent] completion_check` の生成と検証、および既定条件からその値だけを変えた DeepSeek と模擬の項目を加えた。
- セッションの固定の促しから、前後の検査・実行と後の編集の要求を集計する。包んだ促しの SHA-256 を見出しへ残し、記録の促しとの版違いと、記録の型違いを試行の失敗として扱う。最後の応答の未検証の報告は、判定した者を添えて人が記録する手順にした。
- `--interleave` を加え、（課題, 反復）の組の通し番号で条件の順を回し、中断した試行を含む全体の起動順・組の中の位置と、条件をまたぐ費用を記録する。
- `benitoiteh` の起動と版の問い合わせは、読ませない場所と資格情報の探索を変えないため、親の `HOME` を保持する。`scripts/check.sh` の評価の検査では、Python の処理全体のホーム・設定・状態・一時領域を分ける。
- 既存の模擬の 11 課題に加え、促しのある 1 課題を別に検査する。

P5-32 の再レビューの低-A・低-C に従い、check の JSON の診断から `final_codes` と失敗の理由を残し、`checkReport`／`testCheckReport` の種別も検証する。run・test には `--allow-effects=<task_allow>` を渡す。通常モードの bash でも XDG を移した後の HOME の既定の設定・状態を読めないテストを加えた。

原本のパスの印には、worktree の `.git` と `commondir` から求めた元の checkout も含め、HOME を別に連結する既知の綴りも検出する。任意の文字列の分割・符号化、後の段の内容を手掛かりにした検出は実装していない。内容の検出には段ごとに識別できる文を具体的な課題から選ぶ必要があり、共通する仕様の文だけで除外すると比較を誤る。P5-33 での扱いは、後の「再開と残す量を確かめる境界」に記す。

外の macOS の検査で判定の HTTP とシェルの方針が失敗したため、接続先と起動先を修正した。Seatbelt の `remote ip` のホストは `localhost` または `*` に限られ、`127.0.0.1` の表記は拒まれる（オーケストレータの検査出力、2026-10-20）。模擬の URL は従来どおり `http://127.0.0.1:<番号>` と検証し、方針では同じポートの `localhost:<番号>` と書く。起動先は、手元の `/bin/sh` の文字列と `/private/var/select/sh -> /bin/bash` を照合し、sh が起動し直す `/bin/bash` が許可から抜けていると確認した。`process_shell` だけに `/bin/bash` を加え、`process_run` の許可は `/usr/bin/sort` のままとする。別の sh の選択への対応は確認していない。


### P5-33 圧縮の二課題と資料

`compact_json_summary` は本ディレクトリの `tasks/json_summary/`、`compact_dir_walk` は `tasks/dir_walk/` を基にした。元の十一課題は変えていない。前者は分類別から地域別へ変え、取消済みの除外・金額欠落の扱い・名前順を第一段、未知の地域と JSON の生成を第二段、CLI の入力選択を第三段で渡す。後者は行数の集計をやめ、Markdown の列挙・隠れたディレクトリと drafts の除外・名前順を第一段、大文字小文字の拡張子とディレクトリ別の件数を第二段、CLI の根の選択を第三段で渡す。依頼と資料は英語で、前の条件の全文と解のコードを後の段に写さない。

入力・正しい解・手計算の期待値は本作業で作成した。正しい解は取り込んだ v0.0.7 の Skill の構文と標準ライブラリを使う。実行の形は `benitoite run <script> <args>...` とし、CLI の引数は `Process.arguments()` で取る（`assets/benitoite/v0.0.7/skill/references/stdlib/IO.Process.md`、同 `idioms.md`）。全段の `task_allow` は最終解が宣言する `Console.Write,File.Read,File.Write,Process.Environment` である。追加の起動と通信は要らない。

各ケースの入力は原本を重ねる先に置き、最終解だけを写して判定する。JSON の期待値は値で比較する。注文には取消済みだけを除くケース、欠落金額でも件数に入るケース、未知の複数地域をまとめるケース、空配列、名前順、全条件の組を置く。走査には隠れたディレクトリと通常の `.visible.md`、入れ子の drafts と通常の drafts.md ディレクトリ、大文字小文字の拡張子、空の根、深い根、名前順を置く。大文字と小文字だけが違うファイル名は、macOS の既定のファイルシステムで共存しないので使わない。

否定の対照は、注文の `include_cancelled.bnt` → `s1_cancelled`、`missing_is_one.bnt` → `s1_missing_amount`、`reverse_names.bnt` → `s1_name_order`、走査の `include_hidden.bnt` → `s1_hidden_directories`、`include_drafts.bnt` → `s1_drafts`、`reverse_names.bnt` → `s1_name_order` である。一つの初期条件だけを外し、構文・型・エフェクトの検査は通す。self-test は指定したケースで stdout または生成ファイルの不一致が出た場合だけ、その否定の対照を合格にする。

#### 生成の引数とハッシュ

資料は `generate_compaction_materials.py`（Python 3.9 の標準ライブラリだけ）で作る。引数の既定は `--total-kib 268 --chunk-kib 10 --seed 33`、量は三段の合計である。段に均等に割り振り、例の途中で切らずに目標量以上まで書く。例ごとに識別子・金額・名前・走査の根を変え、入力と中間の集計を併記する。段に属する新しい条件だけの例を生成し、資料にコードや確認を促す指示を入れない。JSON のキー順、区切り、ASCII の表現、LF、ファイルの並びを固定する。入力ファイルと期待値を生成する道具ではない。

各ファイルの SHA-256 と実量、段ごとの実量は各 `task.json` の `generation` に記録する。全体のハッシュは相対パス順の `sha256  path\n` を連結した ASCII のバイト列の SHA-256 とする。候補の生成結果は次のとおりである（seed 33、chunk 10 KiB）。

| 課題 | 目標 KiB | 実量 B | 全体の SHA-256 |
|---|---:|---:|---|
| `compact_json_summary` | 24 | 24857 | `702c48f3425db304e2138a4c1f5c92a45286b59afb00e1b117720fca145f01cf` |
| `compact_json_summary` | 48 | 49545 | `630cafedc6e7313365855b9d58abf933b44e47e2cb93335399e4d5d6fbfe1da0` |
| `compact_json_summary` | 64 | 66093 | `aaa247d865f0413795f10701b341b5cd169ce6331704e035fd565e130cde45a9` |
| `compact_json_summary` | 268 | 275165 | `9e153abcfb4a208f3eaaa6cfec8cc66a6ab6457b5ed5c2c54e86428bd56318a0` |
| `compact_dir_walk` | 24 | 24965 | `d07d8b1303ae8445da5e7dc226c4e5a01f5a1a92f2e9a8754280ffc529cf4090` |
| `compact_dir_walk` | 48 | 49718 | `fe9ee2390671283c62b06e528bd635d3dd4eec2f95fccad4797d3f3e8220361a` |
| `compact_dir_walk` | 64 | 66022 | `245edff264db46107ca03ba8afb6837a18e1415e42545eef8b0357f9ac3caf6d` |
| `compact_dir_walk` | 268 | 274880 | `ed977d6a1d30fc626270810d44558ff30a424eee07c611d3fa6601e2fe53f24f` |

初めの候補は 24・48・64 KiB とした。再開直後の固定部分を約 39 KB とすると、64 KiB を三等分した第一段までの単純な見積もりは `(39 KB + 約22 KB) / 3 ≒ 20,300` トークンであり、40,960 に届かない。このため 64 KiB を初期候補とし、12 KiB を 17 単位足した 268 KiB を暫定値とした（`generation.added_units_kib`）。各段は約 90 KiB、9 ファイル、一つの資料は 12 KiB 以下、依頼の本文は 4 KiB 以下である。第一段までの同じ単純な見積もりは約43,600トークンとなり、現行値の境40,960を超え、予備を減らした境49,152より小さい。これは資料量を選ぶための計算であり、実際の読み取り結果や提供者の使用量を測った値ではない。行番号・依頼・スクリプト・実行結果なども実際の見積もりに含まれるので、外の模擬の通しと有料の試しで確認する。

#### 再開と残す量を確かめる境界

`agent/mod.rs` の再開は `last_usage` を空に戻す。`context/mod.rs::estimate_tokens` と `agent/compaction/mod.rs::total_bytes` は、報告がなければシステムプロンプト・道具の定義・会話の UTF-8 の全バイト数を 3 で切り上げて割る。作業の補足にある固定部分の約39 KB（31,390 + 7,569 B）だけでも約13,000トークンを占める。模擬の固定 `input_tokens` は、再開した最初の判定には使われない。したがって模擬の報告値だけで資料の長さを決めない。

`benitoiteh-mock-compact` の固定入力使用量は1,000にした。各段の複数の実際の読み取りを途中の圧縮で失わず、次の依頼を加えた再開直後にバイト数で圧縮させるためである。小さい既存の一時課題のテストは45,000を明示し、提供者の使用量による途中の圧縮の契約を保つ。どちらも本物の使用量と成績を再現するものではない。

| 検査 | 確かめること | 実行する場所 |
|---|---|---|
| 資料の生成と公開順 | 同じ引数のバイト列とハッシュが一致し、後の段の資料はまだない | Codex 内も可 |
| self-test・dry-run | 最終解が全ケースを通り、否定の対照は指定の条件の不一致で落ちる。dry-run の `comparison_valid` は false | Codex 外の macOS |
| 具体的な二課題の三条件の模擬 | 実際の read の結果、再開、イベントと記録の照合、全段の上限と使用量の和、圧縮と残す量・時点の差 | Codex 外の macOS、待ち受け不可なら skipTest |
| Anthropic の道具が続く途中 | `tool_sequence` のため残す範囲を設けない。小さい既存の課題で固定45,000を使う | Codex 外の模擬、ループの既存のテスト |
| Anthropic の依頼の境 | 次の依頼の前で `kept_items > 0`、複数の呼び出しと結果、現行値と8,000の `kept_estimate` の差を確かめる | Codex 外の二課題の模擬、有料の試し |

`kept_estimate` は再構築前に選んだ範囲の見積もりであり、推論を除いた後の送信量より多い場合がある。失敗・取り消し・上限で止まった圧縮の使用量は総計にだけ入り、圧縮の記録・イベントにはない。フォークで写した記録の `usage` を合計すると元の費用を重ねて数えるので、評価の費用は各段の stdout の最後の `summary` の和から求める。

比較が成立しない試行も成果物の合否を残す。例えば dry-run は `comparison_valid: false`、`comparison_reasons: ["dry-run; no harness execution", "no compaction occurred", ...]` となる。圧縮がない、現行値で依頼をまたぐ残す範囲がない、予備の実効値が等しい、三段が完了しない、集計と記録が一致しない、原本のパスが見えた場合も理由を残す。原本のパスの検出は任意の分割・符号化を完全に検出しない。後の段の文には前の条件や一般的な語も含まれるため、文の一致による漏れの自動判定は加えていない。検出がないことだけを漏れがない証明に使わない。

有料の試しで全条件共通の長さと段を確定すること、BH-014 の決定は残す。DeepSeek-V4.1-Flash（`deepseek-flash`、Anthropic 互換の口）、effort high、文脈65,536、出力8,192、同じ実行ファイルで、試しは同じ一課題を三条件で各一回の計三試行、本体は二課題・三条件・各三回の計十八試行とする。1試行は三段と要約を含む。設計書07-02の見込みは1試行$0.06〜0.20、計$1.26〜4.20、42〜126分、試しと本体の累計の停止額$5である。別の起動には使った費用を差し引いた残額を渡す。追加の試しが要る場合は回数・費用を改めて設計者に示す。本作業は本物のLLMを呼ばない。


### P5-32 第三回の判定のレビューへの対応（P5-33 と同時に実装）

以下は P5-32・P5-33 の時点の記録である。HOME の data を拒んで必要な場所を許し直す方式と、その文の順を調べるテストは、P5-34 で読み取りの根と literal を列挙する方式、および実行器へ渡す引数と実際の拒否のテストへ置き換えた。

中-1への対応として、SBPL の末尾に、親の HOME の realpath の下の `file-read-data` の拒否、実の処理系と課題が許す補助プログラムの各実行ファイルの読み取り、判定のディレクトリの読み取りの順に規則を加えた。後の規則による許し直しは、設計者が提示したレビューと再現の台本 `rr-P5-32c/fix.py`（2026-10-20、main 2c2d1a9 の点検）の確認結果に従う。判定用の HOME を拒否の根として使うと普段の秘密を守れないので、親の HOME と分ける。パスの制御文字は realpath の後にも検査する。

判定の理由に含める stdout の相違と誤りの文の抜粋は、制御文字などを ASCII のエスケープにし、抜粋ごとに120文字以下に切る。これは任意の秘密を伏せる処理ではなく、秘密の読み取りは OS の方針で拒む。

エフェクトの検査と `--allow-effects` は直接の起動を早く拒むために使う。`Process.Run` を許す課題では、許された処理系を入れ子に起動すると、このエフェクトの許可を広げられるため、読み取り・書き込み・起動・通信の境界は OS の方針で守る。判定のコマンドが正常に終わったときも、同じプロセス群を SIGKILL で止める。別の群れへ移った子の検出と停止は、この起動の口では保証しない。

低-1は `run_command` の finally で正常終了後にも同じ群れを止める形にした。低-2は上記の文書と実行の口の説明を改め、入れ子の処理系をエフェクト検査だけで止めるとは書かない。低-3は、通信の否定を外部の TEST-NET の宛先から、応答できることを直接確認した別の IPv4・IPv6 のループバックの待ち受けへ変え、接続した数が0であることと `Operation not permitted` を確かめる。シェルの未許可の id・touch・外へのリダイレクト、process_run の /bin/sh、実際の段の課題の外への書き込み、特殊な判定のパスも、正しい解と対にして確かめる。

Codex 内では、親 HOME の拒否と必要な許し直しの順、理由の抜粋、正常終了後の背景の子の停止を確かめる。この三つの退行テストは修正前に意図した理由で失敗することを確認した。OS を要するテストには、偽 HOME の .ssh・設定・認証・セッションの囮を stdout と誤りの両方で出そうとするコードを置いた。処理系と判定ディレクトリも偽 HOME の下に置いて正しい解が動くことを対にし、HOME のリンクの別名からも秘密を読めず理由に含まれないことを、Codex 外の macOS で確かめる。本物の秘密は使わない。

### P5-34 の全体の検査とレビューへの修正（2026-10-21）

実行器で読み取りの根を列挙するときは、起動に必要な `/` 自身を literal として加える。`--read-root /` と親 HOME を包含する根、および読み取りの根の指定を伴わない `--allow-program` は拒む。判定の子の PATH と、道具の実のパス・SHA-256・版の探索は `/usr/bin:/bin` に揃えた。

workspace の snapshot も、ディレクトリの記述子から `O_NOFOLLOW`・`O_NONBLOCK` で開き、`fstat` で通常のファイルであることを確かめる。判定前に workspace とその下を cwd とする残りを lsof で調べ、検出や検査の失敗では後続を止める。印を消して cwd も外へ移した子孫の不存在は保証しない。

ポートの規則 `localhost:PORT` は、IPv4 の `127.0.0.1` に加え、IPv6 の `::1` の同じポートへの外向きの接続も許す。これは TCP だけに限定する規則ではない。HTTP の模擬は `127.0.0.1` で待ち受ける。ほかの宛先の拒否のテストは、127.0.0.2 の別名や待ち受けを作らず、外での接続の結果と内での PermissionDenied を対にする。

既知の限界として、`trusted/` は書けないが、成果物の子からも読める。同じサンドボックスで検査の親と子を動かすため、子が期待値を読んで出力を合わせる不正を防ぎきれない。設計書 07-02 の「完全な保証ではない」という範囲であり、この修正では分離の方式を変えていない。
