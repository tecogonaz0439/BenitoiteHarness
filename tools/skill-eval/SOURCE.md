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
