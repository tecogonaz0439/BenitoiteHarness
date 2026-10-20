# P5-34 通常モードの評価の道具と試しの 2 課題

## 目的

BH-071 の自作の課題を走らせるために、skill-eval を通常モードに対応させる。課題の形、準備と判定、環境、起動と記録、模擬、検査への組み込みを担当し、試しの `normal_csv_totals`・`normal_path_filter` を作る。残りの 8 課題は P5-35 で作る。本物の LLM を呼ばない。

## 依存

[P5-31](P5-31-normal-mode-tasks.md) の判定の実行器と [P5-32](P5-32-compaction-runner.md) の組の選択が完了してから始める。P5-29 の `[agent]` の表を前提にする。P5-33 の圧縮の課題とは並べて進められる。

## 読む設計書

- [評価](../../design/07-quality/07-02-evaluation.md)の「通常モードの課題の作り方」「M の段の共通の条件」「M の段の通常モード」「M の段の残りの規模と費用」「評価の手順」
- [モード](../../design/01-agent/01-02-modes.md)の通常モード、[道具](../../design/01-agent/01-03-tools.md)の「bash（通常モード）」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「通常モードの方針」「子プロセスの後始末」、[利用者の確認](../../design/03-safety/03-03-approval.md)の「非対話での扱い」
- [テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」、スキル `test-audit`
- [P5-32](P5-32-compaction-runner.md)の組の選択と上限、`tools/skill-eval/{README.md,SOURCE.md,skill_eval.py,test_skill_eval.py,mock_provider.py,harnesses.json}` と既存の課題
- 既存の `crates/benitoiteh/src/sandbox/{mod.rs,policy.rs,normal.rs,run.rs}`、`src/tools/bash.rs`（同じサンドボックスと子プロセスの実行を再利用する）

## 作るもの

### 課題の形と試しの 2 課題

`tools/skill-eval/tasks/normal_<名前>/` に、`task.md`（英語）、`task.json`、`input/`、`reference/`、`judge/` を置く。`input/` は数個のソースと公開のテスト、入力例から成る小さなプロジェクトとする。試しに使う次の 2 課題を作る。

| 名前 | 種類・編集を許すファイル | 要求する振る舞いと自動の判定 |
|---|---|---|
| `normal_csv_totals` | バグの修正、`totals.py` | CSV の品目ごとの金額の集計で、二進の浮動小数による端数の誤りを直す。小数 2 桁までの入力を正確に足し、品目の名前順に小数 2 桁で出す。0.10 と 0.20、負数、同じ品目、空の入力を判定する |
| `normal_path_filter` | バグの修正、`collect.sh` | 指定したディレクトリ直下の通常の `.txt` のファイル名を名前順に出す。空白を含む名前を壊さず、ディレクトリとほかの拡張子を除く。空のディレクトリも成功する。ファイル名の改行は対象外と仕様に書く |

CLI の引数の順、入出力の名前、エラーの文のうち判定する部分、改行や順序の規則は、各 `task.md` と公開のテストに明記する。通常はソース 1 ファイルだけを変えれば解ける大きさにする。テストの作成そのものを求める課題は含めず、原本のテストを通す変更を求める。

`task.json` は次の形を基準とする。`attempt_limit` と Benitoite 用の `script` は使わない。

```json
{
  "kind": "normal",
  "category": "bugfix",
  "artifacts": ["totals.py"],
  "writable_files": ["totals.py"],
  "allowed_generated": ["__pycache__/"],
  "verify": ["python3", "-m", "unittest", "discover", "-s", "tests"],
  "judge": ["python3", "-I", "judge/verify.py"]
}
```

`category` は `bugfix`・`feature`・`tests` のいずれかとする。sh の課題にも Python の標準ライブラリの公開のテストを置き、テストから `/bin/sh` で実行する。`judge/verify.py` は判定ごとの名前と成否と理由を JSON で出す信頼した検査のプログラムとし、公開のテストとは別の入力と境界の値を使う。`reference/` は `artifacts` と同じ相対パスで正しい解を持つ。

### 準備と判定

- `--suite normal` を P5-32 の選択に足す。既定と `--suite benitoite` は従来の 11 課題のままにする。通常モードの課題を既存の制限モードの起動へ渡した場合は、LLM を呼ぶ前に断る。
- 必要な `python3`・`/bin/sh` と、課題で実際に使う `sort`・`find` などのコマンドを準備のときに確認し、実のパスと SHA-256 を記録し、版を取得できる道具は版も記録する。なければ飛ばさず、LLM を呼ぶ前に止める。外部のパッケージの導入、更新、通信をしない。
- `kind: normal` では `input/` と `task.md` だけを写す。Benitoite の Skill のプロジェクトへの配置、包みのコマンド、`task_allow`、Benitoite の判定を呼ばない。ただし比較の構成を揃えるため、`benitoiteh` 自身には `v0.0.7` の処理系の写しを設定し、Benitoite の道具を使える状態を保つ。
- `task.json` のパスは正規化した相対パスに限る。絶対パス、`..`、重複、リンクと特殊ファイルを拒み、課題の根の外を読まない。解と判定の場所やリポジトリのパスを、モデルへの依頼や子の環境に出さない。公開のテストの確認のコマンドだけは `task.md` に示す。
- 試行の後に、初めの入力を新しい判定のディレクトリへ置き直し、`artifacts` の通常のファイルだけを workspace から写す。公開のテストを workspace から写さない。必須の成果物の欠落、リンク、特殊ファイルは失敗とする。`writable_files` の外の入力の変更や削除、許していないファイルの新規作成は記録して失敗とし、`__pycache__/` など生成を認めるものは各課題で明示する。
- 原本の公開のテストと `judge/verify.py` を、P5-31 の通信を許さない判定の実行器で走らせる。1 コマンド 60 秒、出力は標準出力と標準エラー出力を各 1 MiB まで保存する。時間切れ、出力の打ち切り、判定の形の誤りは理由を残して失敗とする。各課題の判定の全項目が通れば、成果物の判定を成功にする。
- `--self-test --suite normal` では、初めの未修正の入力が意図した判定で落ち、解を重ねた入力が通ることを全課題で確認する。`--dry-run --suite normal` は、ハーネスの代わりに解を置き、公開の確認のコマンドと判定を通す。dry-run だけは解を workspace に置き、本物の評価の成績に混ぜない。

- `--judge-runner` で P5-31 の実行器のパスを指定できるようにし、省略時はビルド済みの同名の example を探す。normal の self-test・dry-run・本物の走行のすべてで必要とする。macOS 以外では LLM を呼ぶ前に止める。
- 実行器へ `--deny-read <リポジトリの実のパス> --deny-read <workspace の実のパス>` を渡す。判定ごとの作業ディレクトリは workspace と別に作る。
- 公開のテストの後に `judge/` を置くか、コマンドごとに新しい判定のディレクトリを作る。成果物はケースごとに子プロセスで動かし、信頼した親が出力を比べる。判定の親へ成果物を import しない。

### 起動と記録

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

- この作業の完了時に走らせられる normal の課題は 2 個である。本体の 10 課題 × 3 回の準備が揃うのは P5-35 の後とする。

### 出典と使い方

- `SOURCE.md` の「写した後に変えた点」に、自作の課題であること、形の追加、通常モードの起動、判定の実行器、記録の追加を書く。写した元のタグ・コミットと、従来の 11 課題の出典は変えない。
- `README.md` に、この作業の 2 課題の表（残りの 8 課題は P5-35 で足す）、形、前提の道具、example のビルド、self-test・dry-run・模擬の提供者による通しの使い方を追記する。本物の走行の例は `--suite normal --harness benitoiteh-deepseek-normal --trials 3` とし、停止額、事前の回数と費用の提示も書く。

## 触るファイル

- `tools/skill-eval/skill_eval.py`、`test_skill_eval.py`、`README.md`、`SOURCE.md`
- `tools/skill-eval/mock_provider.py`（通常モードの read → write/edit → bash → Done. の筋書き）
- `tools/skill-eval/harnesses.json`（`benitoiteh-deepseek-normal` と `benitoiteh-mock-normal`）
- `tools/skill-eval/tasks/normal_csv_totals/`・`normal_path_filter/`（新。入力、公開のテスト、解、判定を含む）
- `scripts/check.sh`（normal の準備とサンドボックスを要する通し、Codex 内で除く名前の表示）

P5-31 の実行器と既存の sandbox の API を使う。通常モードに対応するための変更を Rust の本番のコードへ広げない。

## テスト

`test-audit` の作成時の関門に従い、課題の良否と道具の振る舞いを分けて確認する。

- 2 課題の self-test で未修正なら意図した判定で落ち、解なら通ることを確かめる。既知の入力だけの固定の出力、公開のテストの削除などの不完全な修正を対照に使い、別の理由で落ちただけの否定のテストにしない。
- Python の評価の道具の境界で、成果物だけを写すこと、解と judge を workspace に置かないこと、リンクと範囲の外のパスの拒否、原本のテストを使うことを確認する。
- 偽の exec の記録で、モードの選択、Benitoite の回数 0 を失敗にしないこと、実行の拒否と実際の起動の区別、上限、費用、成果物と確認不足の別の欄を確認する。
- 模擬の提供者による通しでは、Python と sh の各 1 課題で、本物の read・write/edit・bash を使う。既存のファイルは読み取りを先に行う。
- normal の self-test・dry-run・模擬の通しは Codex 内では除き、名前を示す。オーケストレータが外の macOS で走らせる。Python の評価の道具のテストは偽の出力と判定用の一時ファイルでも確かめる。

## 完了条件

- 2 課題が揃い、LLM を呼ばず未修正・解の判定を確かめられる。既定の 11 課題の選択と成績を保つ。
- `scripts/check.sh --inside-codex` が通る。オーケストレータが外で normal の self-test・dry-run・模擬の通しを完了できる。
- 必要な道具、判定と記録の例、走らせなかった検査、試し 2 回の条件を報告する。本体 30 回は P5-35 の後と明記する。

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。以下の推奨を、明記した修正を除いてこの作業の指示として採る。コードの行番号は、点検報告が参照した main の `d67e1af` のものである。

### 触る箇所

- `skill_eval.py` の `PROMPT` は 80–88、`Task` は 91–107、`benitoiteh_config` は 166–194。入力と task の写しは 1549–1550、Skill は 1551、包みの PATH と `SKILL_EVAL_*` は 1570–1572、`task_allow` は 1575、起動は 1613–1618、判定は 1690–1707、行は 1784。
- `attempts %d` は 2240、dry-run の合格の条件は 2260、`harness_environment` は 616–621。
- `--approval never` は既に `args.rs:213–219` と `config/mod.rs:471–477` が受け付け、exec は `Never` を使う（`exec.rs:328–336`）。

### 実行の証拠・環境・判定の分離

1. **実行したことの確かめ:** 推奨: `harness.stdout` の `tool_result` のうち、`name=="bash"` で `screen` が `^exit -?[0-9]+$` に合うものを数え、終了状態もそこから取る。

2. **環境から場所が漏れる**
   - `harness_environment` は親の環境をほぼそのまま渡し、`child_environment`（`config/environment.rs`）も鍵以外を `bash` の子へ渡す。
   - このため `PWD`・`OLDPWD`（リポジトリのパスでありうる）、`SKILL_EVAL_*`、包みの PATH が子に見える。
   - 推奨: normal ではこれらを除き、`PWD` を workspace にする。`HOME` は変えない（キーチェーンの読み取りに要る）。

3. **python3 の実体**
   - `/usr/bin/python3` は xcrun を介す入口で、実体は `/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9/bin/python3.9`（3.9.6）である。
   - 判定には実のパスを渡し、準備のときにサンドボックスの中での起動も試す。
   - 判定の環境には `PYTHONDONTWRITEBYTECODE=1` も足す。
   - BSD の `find` には `--version` がない。道具は実のパスと SHA-256 で記録する。

4. **`-I` の扱い**
   - `verify` に `-I` を付けると作業ディレクトリが `sys.path` から外れ、`import totals` が失敗する。`verify` には `-I` を付けない。
   - `judge/verify.py` が成果物を同じプロセスに取り込むと、成果物が判定の JSON を偽れる。推奨: 成果物はケースごとに子プロセスで動かし、親が比べる。
   - 公開のテストは判定のディレクトリに書けるので、`judge/` はその後に置くか、コマンドごとに新しいディレクトリを使う。

5. **生成を認めるファイルの欄がない:** `task.json` の例に「生成を認めるもの」の欄がない。推奨: `allowed_generated: ["__pycache__/"]` を足す。

### 制限モードとの分岐と模擬

1. **制限モードを前提にした処理**
   - `install_skill`・`task_allow`・模擬の起動・dry-run の条件・`attempts %d` が制限モードを前提にしている。kind で分ける。
   - 記録の表も、Benitoite の列と normal の列を分ける。

2. **模擬の提供者と項目**
   - 通常モードの筋書きと `benitoiteh-mock-normal` の項目が要る（作業の文書にない）。
   - 既存のファイルを上書きするには先に `read` が要る（`write_path.rs:168–178`、`edit.rs:30`）。筋書きは `read` → `write`/`edit` → `bash` → `Done.` の 4 回とする。

### 検査の組み込み

- normal の self-test・dry-run・模擬の通しは `--inside-codex` から除き、除いた名前を `check.sh` の一覧（45–53）へ足す。実行器の example は P5-31 が明示的にビルドする。
- `--work-root` は `/tmp` の下を拒む（`skill_eval.py:2117–2118`）。外での検査でもリポジトリと `/tmp` の外を使う。
- Python と課題の `judge/verify.py` は 3.9.6 で動く書き方にする。BSD の `find` に `--version` を渡さず、実のパスと SHA-256 を記録する。
- ハーネスの HOME はキーチェーンのために保ち、判定の HOME は専用のものにする。整理する二つの環境を混同しない。

- Linux の root の実行環境でも、権限の印だけによる拒否をテストしない。Python の道具のテストでは拒否の理由を確かめ、OS の方針による拒否は P5-31 の macOS の実行器のテストへ任せる。
