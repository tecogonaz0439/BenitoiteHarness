# P5-35 通常モードの残りの 8 課題

## 目的

P5-34 の評価の道具と試しの 2 課題に、残りの 8 課題を足す。BH-071 のバグの修正 4 個、小さな機能 4 個、テストを通す変更 2 個の計 10 個を揃える。本物の LLM を呼ばない。

## 依存と読む文書

- [P5-34](P5-34-normal-mode-evaluation.md) の後に行う。課題の形、パスの検証、許した編集と生成の規則、判定の実行器、起動と記録をそのまま使う。
- [評価](../../design/07-quality/07-02-evaluation.md)の「通常モードの課題の作り方」「M の段の通常モード」「M の段の残りの規模と費用」
- `tools/skill-eval/README.md`、`SOURCE.md`、`normal_csv_totals/`・`normal_path_filter/`。テストの計画は `test-audit` の作成時の関門に従う

## 作るもの

| 名前 | 種類・編集を許すファイル | 要求する振る舞いと自動の判定 |
|---|---|---|
| `normal_config_merge` | バグの修正、`config.py` | 既定の JSON の辞書へ上書きの辞書を再帰的に重ねる。辞書以外は値全体を置き換え、`false`・0・空文字・`null` を欠落と混同しない。元の辞書を変更しないことも判定する |
| `normal_cli_exit` | バグの修正、`tool.sh` | `greet <name>` は `hello <name>` と改行を標準出力へ出し、0 で終える。引数の不足、余分な引数、未知のサブコマンドは、説明を標準エラー出力だけへ出して 2 で終える。空白を含む名前も判定する |
| `normal_jsonl_filter` | 小さな機能、`filter.py` | JSONL の数値の `score` が `--min-score` 以上の行を、元の順に JSONL で出す。score がない行は除く。不正な JSON は行番号を標準エラー出力へ出し、2 で終える。等号、0、空の入力を判定する |
| `normal_text_replace` | 小さな機能、`replace.py` | 既存の文字列の置換の CLI に `--dry-run` を加える。通常は指定の UTF-8 のファイルを更新し、dry-run は置換後の内容を標準出力へ出して元を変えない。該当なし、日本語、末尾の改行の有無も判定する |
| `normal_stable_unique` | 小さな機能、`unique.py` | 行の重複を除き、最初に現れた順に出す `--ignore-case` を加える。大文字と小文字を無視しても、出力は最初の行の表記を保つ。既定の区別あり、空行、CRLF、末尾の改行なしも仕様を定めて判定する |
| `normal_report_sort` | 小さな機能、`report.py` | JSON のレコードを指定したキーで昇順に出す CLI に `--descending` を加える。同じ値では入力の順を保つ。数値のキー、文字列のキー、同値、空の配列、キーの欠落の誤りを判定する |
| `normal_test_ranges` | テストを通す変更、`ranges.py` | 公開のテストが落ちる半開区間 `[start, end)` の重なりの判定を直す。端だけが接する区間と空の区間は重ならず、不正な逆順の区間は `ValueError` にする。包含、負数、端の一致を追加の検査でも判定する |
| `normal_test_manifest` | テストを通す変更、`manifest.py` | 公開のテストが落ちる manifest の検証を直す。`files` は重複しない相対の POSIX のパスの配列とし、絶対パス、`..` の要素、空文字を拒む。配列の型、重複、正しい入れ子を追加の検査でも判定する |

各課題に、英語の `task.md`、`task.json`、小さな初めのプロジェクトと公開のテストを置く `input/`、正しい解の `reference/`、追加の判定の `judge/` を作る。`category` は表の種類に対応する `bugfix`・`feature`・`tests` とする。`artifacts` と `writable_files` は原則として表のソース 1 ファイルにし、`allowed_generated` に生成を認めるものを明示する。`attempt_limit` と Benitoite の `script` は使わない。

CLI の引数の順、入出力の名前、判定するエラーの文、改行と順序の規則は `task.md` と公開のテストに書く。テストを通す 2 課題も原本のテストを通すソースの修正を求め、テストの作成自体を課題にしない。外部のパッケージ、時計、乱数、特定の絶対パスに依存させない。

`SOURCE.md` に自作の課題であることと課題ごとの条件を書き、`README.md` の表を 10 課題にする。写した元のタグ・コミットと従来の 11 課題の出典は変えない。本体の走行の例は `--suite normal --harness benitoiteh-deepseek-normal --trials 3` とし、試し 2 回、本体 30 回、停止額 $3 の条件と費用の提示は設計書 07-02 に従う。

## 触るファイル

- `tools/skill-eval/tasks/normal_config_merge/`、`normal_cli_exit/`、`normal_jsonl_filter/`、`normal_text_replace/`（新）
- `tools/skill-eval/tasks/normal_stable_unique/`、`normal_report_sort/`、`normal_test_ranges/`、`normal_test_manifest/`（新）
- `tools/skill-eval/README.md`、`SOURCE.md`
- `tools/skill-eval/skill_eval.py`、`test_skill_eval.py`、`mock_provider.py`、`harnesses.json` の `benitoiteh-mock-normal`、`scripts/check.sh`（8 課題を既存の仕組みへ接続するために不足がある場合に限る）

## テスト

- `--self-test --suite normal` で、全 10 課題の未修正の入力が意図した理由で落ち、正しい解が通ることを確かめる。期待値を解から計算しない。
- 追加の 8 課題でも、固定の出力、公開のテストの削除、追加機能のために既定の振る舞いを壊した修正など、課題の条件を欠く対照を用意し、対応する判定で落ちることを確認する。
- `--dry-run --suite normal` は判定の実行器を使い、LLM とキーを使わず全 10 課題を通す。workspace の公開のテストを書き換えても判定を偽れないことは P5-34 の道具のテストに任せ、同じ契約を課題ごとに重ねない。
- `scripts/check.sh` で全 normal の課題を選び、既定の 11 課題と成績を混ぜない。Codex 内で走らせられない self-test・dry-run・模擬の通しは外の macOS でオーケストレータが確かめる。

## 完了条件

- 計 10 課題と、未修正・解・不完全な修正の判定が揃う。通常の準備では解と判定用のファイルを workspace へ置かない。
- `scripts/check.sh --inside-codex` が通り、オーケストレータが外で全 10 課題の self-test・dry-run と必要な模擬の通しを完了できる。
- 課題の一覧、種別ごとの数、判定の例、走らせなかった検査、試し 2 回と本体 30 回の条件を報告する。外部のハーネスとの比較を済ませたとは報告しない。

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。以下の推奨を、明記した修正を除いてこの作業の指示として採る。コードの行番号は、点検報告が参照した main の `d67e1af` のものである。

### 判定の Python と隔離

- 判定の Python は 3.9.6 で動く書き方とする。公開の `verify` に `-I` を付けず、課題のモジュールを読み取れる形を保つ。
- `judge/verify.py` は成果物を同じプロセスに import せず、ケースごとに子プロセスで動かして信頼した親が比べる。公開のテストの後に `judge/` を置くか、コマンドごとに新しい判定のディレクトリを使う。
- 判定の環境に `PYTHONDONTWRITEBYTECODE=1` を付ける。workspace で公開のテストを走らせたときの生成を許す範囲は `allowed_generated` に明示する。

### 作業の範囲と順序

- 点検報告の 31c をこの作業へ割り当てる。31a は P5-31、31b は P5-34 とする。
- 課題の中身は P5-30 と並べて下書きできるが、作業の完了は P5-34 の判定と記録へ接続してからとする。
- normal の self-test・dry-run・模擬の通しを Codex 内で除き、除いた名前を報告する扱いを引き継ぐ。サンドボックスなしで代替しない。
