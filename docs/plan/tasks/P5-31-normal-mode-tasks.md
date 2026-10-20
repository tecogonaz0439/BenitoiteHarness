# P5-31 通常モードの自作の課題と評価の道具の対応

## 目的

BH-071 で決めた通常モードの自作の課題を 10 個作り、既存の skill-eval の準備・走行・記録に対応させる。ファイルの編集とコマンドの実行を要する課題を、手元の sh と Python の標準ライブラリだけで自動で判定する。本物の LLM を呼ばない。

## 読む設計書

- [評価](../../design/07-quality/07-02-evaluation.md)の「通常モードの課題の作り方」「M の段の共通の条件」「M の段の通常モード」「M の段の残りの規模と費用」「評価の手順」
- [モード](../../design/01-agent/01-02-modes.md)の通常モード、[道具](../../design/01-agent/01-03-tools.md)の「bash（通常モード）」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「通常モードの方針」「子プロセスの後始末」、[利用者の確認](../../design/03-safety/03-03-approval.md)の「非対話での扱い」
- [テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」、スキル `test-audit`
- [P5-30](P5-30-compaction-evaluation.md)の組の選択と上限、`tools/skill-eval/{README.md,SOURCE.md,skill_eval.py,test_skill_eval.py,mock_provider.py,harnesses.json}` と既存の課題
- 既存の `crates/benitoiteh/src/sandbox/{mod.rs,policy.rs,normal.rs,run.rs}`、`src/tools/bash.rs`（同じサンドボックスと子プロセスの実行を再利用する）

## 作るもの

### 1. 課題の形と 10 個の内容

`tools/skill-eval/tasks/normal_<名前>/` に、`task.md`（英語）、`task.json`、`input/`、`reference/`、`judge/` を置く。`input/` は数個のソースと公開のテスト、入力例から成る小さなプロジェクトとする。次の内容をそれぞれ独立した課題にする。

| 名前 | 種類・編集を許すファイル | 要求する振る舞いと自動の判定 |
|---|---|---|
| `normal_csv_totals` | バグの修正、`totals.py` | CSV の品目ごとの金額の集計で、二進の浮動小数による端数の誤りを直す。小数 2 桁までの入力を正確に足し、品目の名前順に小数 2 桁で出す。0.10 と 0.20、負数、同じ品目、空の入力を判定する |
| `normal_path_filter` | バグの修正、`collect.sh` | 指定したディレクトリ直下の通常の `.txt` のファイル名を名前順に出す。空白を含む名前を壊さず、ディレクトリとほかの拡張子を除く。空のディレクトリも成功する。ファイル名の改行は対象外と仕様に書く |
| `normal_config_merge` | バグの修正、`config.py` | 既定の JSON の辞書へ上書きの辞書を再帰的に重ねる。辞書以外は値全体を置き換え、`false`・0・空文字・`null` を欠落と混同しない。元の辞書を変更しないことも判定する |
| `normal_cli_exit` | バグの修正、`tool.sh` | `greet <name>` は `hello <name>` と改行を標準出力へ出し、0 で終える。引数の不足、余分な引数、未知のサブコマンドは、説明を標準エラー出力だけへ出して 2 で終える。空白を含む名前も判定する |
| `normal_jsonl_filter` | 小さな機能、`filter.py` | JSONL の数値の `score` が `--min-score` 以上の行を、元の順に JSONL で出す。score がない行は除く。不正な JSON は行番号を標準エラー出力へ出し、2 で終える。等号、0、空の入力を判定する |
| `normal_text_replace` | 小さな機能、`replace.py` | 既存の文字列の置換の CLI に `--dry-run` を加える。通常は指定の UTF-8 のファイルを更新し、dry-run は置換後の内容を標準出力へ出して元を変えない。該当なし、日本語、末尾の改行の有無も判定する |
| `normal_stable_unique` | 小さな機能、`unique.py` | 行の重複を除き、最初に現れた順に出す `--ignore-case` を加える。大文字と小文字を無視しても、出力は最初の行の表記を保つ。既定の区別あり、空行、CRLF、末尾の改行なしも仕様を定めて判定する |
| `normal_report_sort` | 小さな機能、`report.py` | JSON のレコードを指定したキーで昇順に出す CLI に `--descending` を加える。同じ値では入力の順を保つ。数値のキー、文字列のキー、同値、空の配列、キーの欠落の誤りを判定する |
| `normal_test_ranges` | テストを通す変更、`ranges.py` | 公開のテストが落ちる半開区間 `[start, end)` の重なりの判定を直す。端だけが接する区間と空の区間は重ならず、不正な逆順の区間は `ValueError` にする。包含、負数、端の一致を追加の検査でも判定する |
| `normal_test_manifest` | テストを通す変更、`manifest.py` | 公開のテストが落ちる manifest の検証を直す。`files` は重複しない相対の POSIX のパスの配列とし、絶対パス、`..` の要素、空文字を拒む。配列の型、重複、正しい入れ子を追加の検査でも判定する |

CLI の引数の順、入出力の名前、エラーの文のうち判定する部分、改行や順序の規則は、各 `task.md` と公開のテストに明記する。通常はソース 1 ファイルだけを変えれば解ける大きさにする。テストの作成そのものを求める課題は含めず、原本のテストを通す変更を求める。

`task.json` は次の形を基準とする。`attempt_limit` と Benitoite 用の `script` は使わない。

```json
{
  "kind": "normal",
  "category": "bugfix",
  "artifacts": ["totals.py"],
  "writable_files": ["totals.py"],
  "verify": ["python3", "-m", "unittest", "discover", "-s", "tests"],
  "judge": ["python3", "-I", "judge/verify.py"]
}
```

`category` は `bugfix`・`feature`・`tests` のいずれかとする。2 個の sh の課題にも Python の標準ライブラリの公開のテストを置き、テストから `/bin/sh` で実行する。`judge/verify.py` は判定ごとの名前と成否と理由を JSON で出す信頼した検査のプログラムとし、公開のテストとは別の入力と境界の値を使う。`reference/` は `artifacts` と同じ相対パスで正しい解を持つ。

### 2. 準備と判定

- `--suite normal` を P5-30 の選択に足す。既定と `--suite benitoite` は従来の 11 課題のままにする。通常モードの課題を既存の制限モードの起動へ渡した場合は、LLM を呼ぶ前に断る。
- 必要な `python3`・`/bin/sh` と、課題で実際に使う `sort`・`find` などのコマンドを準備のときに確認し、実のパスと版を記録する。なければ飛ばさず、LLM を呼ぶ前に止める。外部のパッケージの導入、更新、通信をしない。
- `kind: normal` では `input/` と `task.md` だけを写す。Benitoite の Skill のプロジェクトへの配置、包みのコマンド、`task_allow`、Benitoite の判定を呼ばない。ただし比較の構成を揃えるため、`benitoiteh` 自身には `v0.0.7` の処理系の写しを設定し、Benitoite の道具を使える状態を保つ。
- `task.json` のパスは正規化した相対パスに限る。絶対パス、`..`、重複、リンクと特殊ファイルを拒み、課題の根の外を読まない。解と判定の場所やリポジトリのパスを、モデルへの依頼や子の環境に出さない。公開のテストの確認のコマンドだけは `task.md` に示す。
- 試行の後に、初めの入力を新しい判定のディレクトリへ置き直し、`artifacts` の通常のファイルだけを workspace から写す。公開のテストを workspace から写さない。必須の成果物の欠落、リンク、特殊ファイルは失敗とする。`writable_files` の外の入力の変更や削除、許していないファイルの新規作成は記録して失敗とし、`__pycache__/` など生成を認めるものは各課題で明示する。
- 原本の公開のテストと `judge/verify.py` を、次項の通信を許さない判定の実行器で走らせる。1 コマンド 60 秒、出力は標準出力と標準エラー出力を各 1 MiB まで保存する。時間切れ、出力の打ち切り、判定の形の誤りは理由を残して失敗とする。各課題の判定の全項目が通れば、成果物の判定を成功にする。
- `--self-test --suite normal` では、初めの未修正の入力が意図した判定で落ち、解を重ねた入力が通ることを全課題で確認する。`--dry-run --suite normal` は、ハーネスの代わりに解を置き、公開の確認のコマンドと判定を通す。dry-run だけは解を workspace に置き、本物の評価の成績に混ぜない。

### 3. 判定の実行器

- `crates/benitoiteh/examples/skill_eval_judge.rs` を作り、Python の道具から起動する小さな実行器とする。LLM、認証、Agent のループを使わず、既存の `sandbox::{PolicyConfig, SandboxPolicy, SandboxCommand}` と `sandbox::run` を使う。テストの差し込み口として本番の API を広げない。
- 引数は `--helper <benitoiteh の実のパス> --cwd <判定のディレクトリ> --temp <専用の一時ディレクトリ> --timeout 60 -- <コマンドの実のパス> <引数>...` とする。コマンドはシェルの文字列に組み立てず、配列で渡す。
- `PolicyConfig::new` の書ける場所を判定のディレクトリと専用の一時ディレクトリだけにし、`allow_ip=false`、`deny_exec_in_writable=false` とする。通常モードの子プロセスと同じ起動と後始末を使う。サンドボックスを掛けられなければ、サンドボックスなしで続けず失敗させる。
- 子の環境は評価の道具で整理したものにし、`PYTHONPATH`・`PYTHONHOME` を除き、`PYTHONNOUSERSITE=1`、`LC_ALL=C`、判定用の HOME・一時ディレクトリを設定する。キー、利用者の設定、元の作業ツリーの環境を判定へ渡さない。
- 呼び出し元へ、終了状態、時間切れ、残る子孫の可能性と出力の打ち切りを返す。残る子孫の可能性があれば失敗として後続を止める。実行器のパスは `--judge-runner` で明示でき、省略時はビルド済みの同名の example を探す。normal の self-test・dry-run・本物の走行のすべてで必要とする。

### 4. 起動と記録

- `harnesses.json` に `benitoiteh-deepseek-normal` を加え、`mode: normal` として区別する。提供者、価格、effort `high`、50 番・200 万トークンの上限は `benitoiteh-deepseek` と同じ、`verified: false`・`default: false` とする。`mode` は `benitoiteh` の項目で `restricted|normal` だけを許し、省略時は restricted とする。
- 生成する利用者の設定で `normal.sandbox="workspace-write"`・`normal.network=false`・`normal.writable_roots=[]`・`agent.completion_check=false` を明示し、`exec --mode normal --approval never` で起動する。`skills.user=false`・`plugins.user=false` を保つ。プロジェクトの指示、Skill、プラグインを初めの入力へ混ぜない。
- 通常モードの依頼を定数として次の文にする。Benitoite 用の `PROMPT` は変えず、課題の kind で選ぶ。

  ```text
  Your task is described in `task.md` in the current directory. Inspect the project, edit only the files the task permits, and run the documented local checks. Use only the tools already installed, stay within the current directory, and do not use the Internet. Report what you changed and the results of your checks.
  ```

- `bash` の道具の結果から、拒否されず実際に起動したコマンドが 1 回以上あることを確認する。単に道具を要求した回数や、拒否された実行を使わない。判定用に後から実行したコマンドも数えない。実行なしは `verification_missing` による失敗とし、`artifact_success`（成果物だけの判定）は別の欄に残す。
- 行に `suite`、`category`、`artifact_success`、`verification_missing`、`bash_calls`、実際のコマンドの終了状態、`changed_files`、判定ごとの結果を足す。Benitoite の検査と修正の回数は `null` とし、0 回や `attempt_limit` で失敗させる既存の分岐を適用しない。
- 成功率、番と道具ごとの回数、入力と出力とキャッシュのトークンの量、時間、上限、費用を今と同じように残す。種別ごとにも成功数と試行数を示し、制限モードと成績を混ぜない。
- 試しは `normal_csv_totals`・`normal_path_filter` を各 1 回、本体は全 10 課題を各 3 回とする。実装の完了の報告では、設計書 07-02 の回数と費用を示す準備までにとどめる。

### 5. 出典と使い方

- `SOURCE.md` の「写した後に変えた点」に、自作の課題であること、形の追加、通常モードの起動、判定の実行器、記録の追加を書く。写した元のタグ・コミットと、従来の 11 課題の出典は変えない。
- `README.md` に、10 課題の表、形、前提の道具、example のビルド、self-test・dry-run・模擬の提供者による通しの使い方を追記する。本物の走行の例は `--suite normal --harness benitoiteh-deepseek-normal --trials 3` とし、停止額、事前の回数と費用の提示も書く。

## 触るファイル

- `tools/skill-eval/tasks/normal_*/`（新）、`skill_eval.py`、`harnesses.json`、`test_skill_eval.py`、`mock_provider.py`、`README.md`、`SOURCE.md`
- `crates/benitoiteh/examples/skill_eval_judge.rs`（新）とその確認のためのテスト。既存の sandbox の API を再利用し、実行の本体を写さない
- `scripts/check.sh` の評価の道具の段（normal の準備とサンドボックスを要する通しを区別して加える範囲）

## テスト

`test-audit` の作成時の関門に従い、課題の良否と道具の振る舞いを分けて確認する。

- 課題の確認は self-test が持つ。全 10 課題で未修正なら落ち、解なら通ることと、代表的な不完全な修正（既知の入力だけの固定の出力、公開のテストの削除、機能の既定の振る舞いの破壊）が判定で落ちることを確認する。同じ期待値を解から計算しない。
- Python の評価の道具の境界では、成果物だけを写すこと、解と judge を workspace へ置かないこと、リンクと範囲の外のパスの拒否、原本のテストを使うことを確認する。公開のテストを壊した workspace を対照に使い、別の理由で落ちただけの否定のテストにしない。
- 偽の exec の記録で、モードの選択、Benitoite の回数 0 を失敗にしないこと、実行の拒否と実際の起動の区別、上限、費用、成果物と確認不足の別の欄を確認する。模擬の提供者の通しでは、少なくとも Python と sh の 1 課題ずつで本物の編集と bash を使う。
- 判定の実行器では、既存のサンドボックスの OS ごとのテストを繰り返さず、今回の実行器固有の契約を本物の子プロセスで確認する。通信と判定の外への書き込みが拒まれ、時間切れの子が止まり、開始前に失敗した実行が成功にならないことを確認する。ローカルの模擬の待ち受けだけを使い、外部へ接続しない。
- normal の dry-run と既定の 11 課題を通す。Codex のサンドボックスで重ねて掛けられない検査は `--inside-codex` の対象から外して名前を示し、オーケストレータがすべての検査で確認する。

## 完了条件

- 自作の 10 課題が揃い、解を置かない通常の準備と、未修正・解の判定を LLM なしで確かめられる。
- `scripts/check.sh --inside-codex` が通る。オーケストレータがサンドボックスの外で self-test・dry-run・模擬の提供者による通しを完了できる。
- 報告に課題の一覧、必要な道具、判定の例、走らせなかった検査、本物の試し 2 回と本体 30 回の条件を示す。外部のハーネスとの比較を済ませたとは報告しない。
