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

1. `skill_eval.py` の `self_test` で、`task.json` の `acceptance` が指す元のファイルが本リポジトリにないときは、そのファイルとの比較を飛ばし、失敗に数えない。元のファイルがあるときの比較は今のままとする。課題の行は次の形にする。
   - 飛ばした元のファイルを、`acceptance skipped: <元のパス>` の句として一つずつ作り、`; ` で区切って並べる（`json_summary` は二つの句になる）。
   - ほかに問題がなければ `ok (<句>; <句>)`、問題（解のスクリプトの失敗、元のファイルがあって中身が違う）があれば、今の `FAILED: <問題>; ...` の後に ` (<句>; <句>)` を付ける。
2. `skill_eval.py` の `warning_codes` で、`CODES_FILE` が読めないときは、同じディレクトリの `warning-codes.txt`（一行に一つのコード。オーケストレータが置いた）を読む。どちらも読めないときは今のとおり空の一覧とする。
3. `scripts/check.sh` に段 `skill-eval` を `test` の段の後に加える。この段は、`--inside-codex` の有無によらず、手元の機械（macOS）で実行し、コンテナと x86_64 の機械では実行しない。どちらの起動も LLM もネットワークも使わない。
   - `python3 -I tools/skill-eval/skill_eval.py --self-test --benitoite "$BENITOITEH_TEST_BENITOITE" [--task <課題>...]`
   - `python3 -I tools/skill-eval/skill_eval.py --dry-run --benitoite "$BENITOITEH_TEST_BENITOITE" --work-root "$skill_eval_work" [--task <課題>...]`。`skill_eval_work` は `mktemp -d` で作り、リポジトリの中に作らない。
   - 一時ディレクトリの後始末: `skill_eval_work=` を `trap finish EXIT` より前に大域の変数として置き、`finish()` の中で空でなければ `rm -rf` する。`trap` を二つ目に掛けない（`finish` が外れる）。`set -e` で途中で止まっても消える。
   - `--dry-run` の後、消す前に、記録 `"$skill_eval_work/record.md"` の警告の行（`^\| W[0-9]{4} \|` に当たる行）がちょうど 5 行であることを確かめ、違えば段を失敗させる（2 の確かめ）。
   - `--inside-codex` のときは `http_client` を除く。Codex の workspace-write のサンドボックスは 127.0.0.1 での `bind` を `EPERM` で止め、`http_server.start()` が例外で止まる（2026-10-18 に確かめた）。`--task` は課題を選ぶ指定であり除く指定ではないので、`tools/skill-eval/tasks/*/task.json` から課題の名前を集め、`http_client` 以外を `--task` で並べる。手元の機械では `--task` を付けない。macOS の `/bin/bash` は 3.2 で、`set -u` のもとで空の配列の `"${arr[@]}"` が「unbound variable」で止まるので、`${arr[@]+"${arr[@]}"}` か、長さの確かめ（既存の `((${#test_args[@]} != 0))` と同じ形）で書く。
   - `finish()` の `Skipped with --inside-codex:` の一覧に、`--inside-codex` のときだけ `skill-eval: http_client (loopback listening is blocked in the Codex sandbox)` を示す。
4. `.gitignore` に `__pycache__/` を加える。この機械の python3（Apple の 3.9）は `-I` のもとでバイトコードを書かず、書くときもホームの下のキャッシュに書くが、ほかの Python で `tools/skill-eval/__pycache__/` ができたときの備えとする。
5. `tools/skill-eval/SOURCE.md` の「写した後に変えた点」に、P5-01 の節を作り、1〜4 で変えた点を一行ずつ書く。

`README.md`（写した元の文書）は変えない。本リポジトリでの使い方は、P5-02 で `SOURCE.md` に書く。処理系を探す既定の場所（`target/`）、記録のディレクトリ `results/` がないこと、誤りの文が Benitoite のスキルの名前を示すことは、P5-02 で扱う（`--self-test` と `--dry-run` は `--benitoite` を渡せば影響を受けない）。

## 触るファイル

- `tools/skill-eval/skill_eval.py`、`tools/skill-eval/SOURCE.md`
- `scripts/check.sh`
- `.gitignore`

## テスト

- Rust のテストは加えない。`scripts/check.sh` の `skill-eval` の段が、11 課題の `--self-test` と `--dry-run` を処理系 `v0.0.7` で通すことを、変更の確かめとする。
- `--self-test` の出力で、`json_summary` の行が `acceptance skipped` を二つ含み、手元の機械では `self-test: 0 of 12 failed`、`--inside-codex` では `self-test: 0 of 11 failed` で終わること。`--dry-run` は、手元の機械では 11 of 11、`--inside-codex` では 10 of 10 の試行が成功すること。
- `warning_codes()` が `warning-codes.txt` の 5 件（`W0301`・`W0401`・`W0402`・`W0403`・`W0501`）を返すことは、3 の記録の行の数の確かめで見る。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、`--inside-codex` での `--self-test` と `--dry-run` の出力の末尾を書く。
