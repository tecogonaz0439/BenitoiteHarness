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
