# P5-01 評価の道具を本リポジトリで動かす

## 目的

Benitoite の Skill の評価の道具を、`v0.0.8` のタグから `tools/skill-eval/` に写した（オーケストレータが写した。出典は `tools/skill-eval/SOURCE.md`）。写したままでは、道具が Benitoite のリポジトリの中のファイルを参照するので、本リポジトリでは LLM を呼ばない確かめ（`--self-test`・`--dry-run`）が通らない。2026-10-18 に `--self-test` を処理系 `v0.0.7` で実行すると、`json_summary` の受け入れテストのファイル（`crates/benitoite/testdata/acceptance/json_summary.stdout`）がなく、`FileNotFoundError` で止まった。

この作業では、道具を本リポジトリで動かし、LLM を呼ばない確かめを完了の前の検査（`scripts/check.sh`）に加える。`benitoiteh` を評価の対象に加えるのは P5-02 で行う。課題（`tasks/`）と判定は変えない。

## 読む設計書

- [評価](../../design/07-quality/07-02-evaluation.md)の「評価の課題」
- [テスト](../../design/07-quality/07-01-testing.md)の「完了の前に通す検査」
- 既存のもの: `tools/skill-eval/`（`README.md`、`SOURCE.md`、`skill_eval.py`、`bin/benitoite`、`tasks/`）、`scripts/check.sh`

## この作業が認める型の変更

なし（Rust のコードを変えない）。

## 作るもの

1. `skill_eval.py` の `self_test` で、`task.json` の `acceptance` が指す元のファイルが本リポジトリにないときは、比較を飛ばし、その課題の行に `ok (acceptance skipped: <元のパス> is not in this repository)` の形で示す。失敗に数えない。元のファイルがあるときの比較は今のままとする。
2. `skill_eval.py` の `warning_codes` で、`CODES_FILE` が読めないときは、同じディレクトリの `warning-codes.txt`（一行に一つのコード。オーケストレータが置いた）を読む。どちらも読めないときは今のとおり空の一覧とする。
3. `scripts/check.sh` に段 `skill-eval` を `test` の段の後に加え、次を実行する。どちらも LLM もネットワークも使わない。
   - `python3 -I tools/skill-eval/skill_eval.py --self-test --benitoite "$BENITOITEH_TEST_BENITOITE"`
   - `python3 -I tools/skill-eval/skill_eval.py --dry-run --benitoite "$BENITOITEH_TEST_BENITOITE" --work-root <一時ディレクトリ>`。一時ディレクトリは `mktemp -d` で作り、段の終わりに消す。リポジトリの中に作らない。
   - `--inside-codex` のときに、Codex のサンドボックスが 127.0.0.1 での待ち受け（`http_server.py`）を止めて `http_client` が通らないなら、その課題だけを `--task` の指定で除き、`Skipped with --inside-codex:` の一覧に `skill-eval: http_client (loopback listening is blocked in the Codex sandbox)` と示す。止めないなら除かない。どちらだったかを完了の報告に書く。
4. `.gitignore` に `__pycache__/` を加える（`skill_eval.py` が `http_server` を読み込むと、`tools/skill-eval/__pycache__/` ができる）。`python3 -I` でも `__pycache__` は作られうるので、無視の設定で扱う。
5. `tools/skill-eval/SOURCE.md` の「写した後に変えた点」に、P5-01 の節を作り、1〜4 で変えた点を一行ずつ書く。

`README.md`（写した元の文書）は変えない。本リポジトリでの使い方は、P5-02 で `SOURCE.md` に書く。

## 触るファイル

- `tools/skill-eval/skill_eval.py`、`tools/skill-eval/SOURCE.md`
- `scripts/check.sh`
- `.gitignore`

## テスト

- Rust のテストは加えない。`scripts/check.sh` の `skill-eval` の段が、11 課題の `--self-test` と `--dry-run` を処理系 `v0.0.7` で通すことを、変更の確かめとする。
- `--self-test` の出力で、`json_summary` の行が `acceptance skipped` を含み、`self-test: 0 of 12 failed` で終わること。
- `warning_codes()` が `warning-codes.txt` の 5 件（`W0301`・`W0401`・`W0402`・`W0403`・`W0501`）を返すことを、`--dry-run` の記録（作業ディレクトリに書かれる）の「診断のコードの集計」の節に 5 件の警告の行が出ることで確かめる。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、`--self-test` と `--dry-run` の出力の末尾と、3 の `http_client` の扱いを書く。
