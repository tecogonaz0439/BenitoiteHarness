# P5-32 圧縮の評価の道具・段の実行と上限

## 目的

P5-30 が加えた設定と記録を使い、同じセッションで 3 段の依頼を走らせる評価の道具を作る。組の選択、設定の検証、段の実行、上限と費用の合計、模擬、検査の組み込みを担当する。圧縮の 2 課題の中身は P5-33 で作る。本物の LLM を呼ばず、BH-014 は閉じない。

## 依存

[P5-30](P5-30-compaction-evaluation.md) の後に行う。[P5-29](P5-29-completion-check.md) の設定の許可の一覧と `--interleave` を引き継ぐ。P5-31 の判定の実行器とは並べて進められる。

## 読む設計書

- [文脈の管理](../../design/01-agent/01-04-context.md)の「文脈の大きさの見積もり」「圧縮」「圧縮の値を比べる設定」「推論の内容の扱い」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「上限」「やり直し」、[セッション](../../design/01-agent/01-05-sessions.md)の「再開」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」「プロジェクトの設定ファイル」
- [評価](../../design/07-quality/07-02-evaluation.md)の「M の段の共通の条件」「M の段の圧縮」「M の段の残りの規模と費用」
- [テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」、スキル `test-audit`
- [P5-29](P5-29-completion-check.md)、既存の `crates/benitoiteh/src/agent/compaction/{mod.rs,conversation.rs}`、`src/config/mod.rs`、`src/session/record.rs`、`tools/skill-eval/{skill_eval.py,mock_provider.py,harnesses.json,SOURCE.md}`、`tasks/json_summary/`・`tasks/dir_walk/`
- [P5-33](P5-33-compaction-tasks.md) の課題の形と生成・判定の条件（後続の課題との受け渡し）

## 作るもの

### 設定と組の選択

- `config.compaction.reserve_tokens`・`keep_tokens` だけを許す。値は `type(value) is int` かつ `0 <= value <= 2**64 - 1` とし、Python の真偽を整数として受け付けない。P5-29 と P5-06 の真偽の鍵は今のままにする。
- 現行の `benitoiteh_config` は `context_window=200000` を無条件に書く。項目の `provider.context_window` があればその正の u64 を使い、省略したときだけ 200,000 とする。`provider.max_output_tokens` も正の u64 として検証して渡す。値の検証は、dry-run でも通る `load_harnesses` の段で行う。
- 次の項目を `benitoiteh-deepseek` の条件から作り、すべて `verified: false`・`default: false` とする。いずれも `context_window=65536`・`max_output_tokens=8192`・effort `high`・促しなしである。

  | 項目 | `config.compaction` |
  |---|---|
  | `benitoiteh-deepseek-compact-base` | `reserve_tokens: 16384, keep_tokens: 20000` |
  | `benitoiteh-deepseek-compact-reserve` | `reserve_tokens: 8192, keep_tokens: 20000` |
  | `benitoiteh-deepseek-compact-keep` | `reserve_tokens: 16384, keep_tokens: 8000` |

- `--suite benitoite|compaction` を加える。省略時は従来の 11 課題だけを選び、有料の既定の対象を増やさない。`--suite compaction` は新しい 2 課題だけを選ぶ。`--task` を併用したらその組の中から選び、不一致は起動前の誤りとする。P5-34 で `normal` を足せる形にする。

### 段の課題の読み取りと実行

- `task.json` の `kind: staged`、`result_kind: run`、`script`、`attempt_limit`、`stages`（相対の Markdown のパス 3 個）、`http: false` を受け付ける。従来の `run`・`test` の `result_kind` は元の kind とし、実行かテストかの分岐をすべて `result_kind` に移す。
- 段のパスは課題の中の通常のファイルに限り、リンク、`..`、絶対パスを拒む。`materials/1/`・`2/`・`3/` は段の番号に対応させ、その段の起動の直前に workspace の `materials/N/` へ写す。通常の準備で後の段の資料を先に写さない。
- 初めの workspace と設定を一度作り、第 1 段を `exec`、第 2・3 段を同じ ID の `exec --resume <ID>` で走らせる。モデル、送り先、プロンプトと道具のハッシュを保つ。段ごとの標準出力と標準エラー出力を別に保存し、途中の段が正常に終わらなければ後の段を送らない。
- 1 試行全体で 50 番・200 万トークン・1,800 秒の上限を掛ける。要約の量は通常の集計に既に入っているので二重に足さない。段の後にも費用を判定し、残額がないか費用が不明なら後の段と試行を止める。
- P5-29 の `--interleave` は、一つの課題・反復の 3 条件を順に回す。3 段を含む 1 試行の途中で別の条件へ移らない。

### 判定と self-test の仕組み

- `judge_cases`（`name`・`input`・`args`・`expected_stdout`・`expected_files`）を読み、課題の中の `cases/<名前>/` の入力と期待する出力を使う。すべてのパスに段のパスと同じ検査を掛ける。
- ケースごとに新しい判定のディレクトリを作り、原本の入力とケースの入力を重ね、最終のスクリプトだけを写す。`benitoite run <script> <args>...` で検査し、JSON の生成ファイルは値で比べる。初めの条件の欠落もケースの名前で理由に残す。
- `self_test_failures` のスクリプトとケース名を読み、指定されたケースで落ちることを確かめる。無関係なケースで落ちただけでは否定の対照の成功にしない。正しい解を通すことと対で行う。
- `--dry-run --suite compaction` は LLM とキーを使わず準備と判定を通す。dry-run だけは解を workspace に置き、本物の評価の成績に混ぜない。

### 記録と使い方

- 1 試行 1 行の総計と、段ごとの内訳、全 exec の起動の数、要約の要求の数を残す。セッションの記録が累積することによる二重の集計を避ける。`compactions` と実際の記録を照合する。
- `comparison_valid` とその理由を残す。圧縮なし、残す量を比べる条件で実際の残す量が 0 のまま、設定による予備の違いが実効値に出ない場合は、比較が成立しなかったことを示す。成果物の判定は別に残す。
- `SOURCE.md` に、元の課題と変えた点、生成の引数、組の選択、複数の依頼と上限の合計、記録の追加を書く。`README.md` に、LLM を呼ばない準備・検査と、本物の走行の使い方を追記する。試し 3 回、本体 18 回の回数と費用は設計書 07-02 を参照する。

- 記録を作る仕組みと `SOURCE.md` の説明はこの作業で更新する。課題の出典・生成の引数と具体的な 2 課題の使い方は P5-33 で追記する。

## 触るファイル

- `tools/skill-eval/skill_eval.py`、`test_skill_eval.py`（課題を一時ディレクトリに作る既存の準備も使う）
- `tools/skill-eval/mock_provider.py`（段別の筋書き、要約への応答、固定の使用量）
- `tools/skill-eval/harnesses.json`（3 条件と `benitoiteh-mock-compact`）、`README.md`、`SOURCE.md`
- `scripts/check.sh`（既定の組の選択、圧縮の self-test・dry-run と模擬の通しの段）

## テスト

- Python の既存の表に、非負の u64、真偽と範囲の外の拒否、提供者の正の u64、組と `--task` の不一致を加える。既定の選択が 11 課題のままであることを確かめる。
- 偽の `--json` の行と手で書いた `session.jsonl` で、3 段の準備、同じ ID での再開、残る番・トークン・時間、費用の停止、集計の重複がないことを確かめる。
- 課題の中身の完成には依存しない。既存の Python のテストの一時ディレクトリに短い段の課題を作り、段の実行とケースの判定の契約を確かめる。公開の `tasks/` に試し用の課題を残さない。
- 模擬の提供者による通しは、実際の再開と圧縮の記録までをつなぐ固有の契約だけを確認する。待ち受けが使えないテストは既存の形で `skipTest` し、Codex で実行できない通しはオーケストレータが外で確認する。

## 完了条件

- `scripts/check.sh --inside-codex` が通り、既定の 11 課題の選択と集計が保たれる。
- 段の実行、全段の上限、途中の費用の判定、1 試行 1 行の記録を LLM なしで確かめられる。報告に行の例と実行できなかった検査を示す。
- P5-33 の 2 課題がまだない間は、`check.sh` の圧縮の組の実行を「課題未作成」と明示して除く。Python の一時的な課題のテストは必ず走らせる。P5-33 でこの除外を外し、両課題の self-test・dry-run と外での模擬の通しを必須にする。

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。以下の推奨を、明記した修正を除いてこの作業の指示として採る。コードの行番号は、点検報告が参照した main の `d67e1af` のものである。

### 触る箇所

- `skill_eval.py` の `Task` は 91–107、`load_tasks` は 110–116、`benitoiteh_config` は 166–194。kind の分岐は 200、1286、1594、1602–1608 にある。
- 集計は `benitoiteh_output`（226–289）と `benitoiteh_results`（528–575）、費用は `CostBudget`（1203–1219）と 2243、判定は `judge_run`（1303–1319）と `same_file`（1222–1235）。
- `scripts/check.sh` の skill-eval の段は 142–181。

### 段の実行と集計

1. **`exec --resume` で段を続ける走らせ方**
   - 推奨: `run_trial` の benitoiteh の分岐から、段の列を回す関数（例 `run_stages`）を呼ぶ。
   - 全段で同じ `env`（試行の下の `XDG_CONFIG_HOME`・`XDG_STATE_HOME`・`TMPDIR`）、同じ設定ファイル、一つの模擬の待ち受けを使う（ポートは設定に書くため）。
   - 第 2・3 段の引数は `exec --resume <ID> --mode restricted --provider eval --allow <同じ値> --max-turns <残り> --max-run-tokens <残り> --json <本文>` とする。
   - `--project` は付けない。付けると記録のパスと照合する（`exec.rs:270–276`）。
   - ID は第 1 段の最初の `session_start` から取る。後の段の `session_start` が同じ ID と `resumed: true` を持つことを確かめる。
   - 残りの番は 50 − Σ`turns`、残りのトークンは 2,000,000 − Σ(`input`+`output`)（`agent/mod.rs:1007–1011` と同じ和）、残りの時間は 1,800 − Σ壁時計とする。
   - 残りが 0 以下なら起動しない。`--max-turns 0` は受け付けられる（`args.rs:258–263`）が、依頼の行を記録に書いた（784）後に上限の誤り（806）で終わり、記録を汚す。
   - 集計は、各段の標準出力の最後の `summary` の行を足す。`session.jsonl` にも段ごとの `summary` が累積する（`exec.rs:896–902`）が、そちらは足さない。
   - `benitoiteh_results` は記録の全体を読むので、全段の後に一度だけ呼ぶ。
   - 費用は、段の後に、累計を変えない判定の口（今の累計＋この試行の途中の費用）で見る。行は最後に一度だけ `add` する。

2. **段の依頼の本文が曖昧:** 推奨: 第 1 段は既存の `PROMPT`、空行、段の本文の順とし、第 2・3 段は段の本文だけとする。`attempt_limit`（1510）の判定は全段の和で行う。

3. **段ごとの資料の置き方がない**
   - 初めから全段の資料を置くと、第 1 段で後の段の資料を読めてしまう。
   - 推奨: 課題に `materials/1/`・`2/`・`3/` を置き、道具が段 N の起動の直前に workspace の `materials/N/` へ写す。`stages` の形は変えず、規約で対応させる。

4. **段の境の見積もりの元が変わる**
   - 再開した各段の最初の判定は、提供者の報告ではなく、全体のバイト数 ÷ 3 で見積もる（`agent/mod.rs:529`、`context/mod.rs:179`）。
   - 制限モードのシステムプロンプトと道具の定義だけで約 39 KB（31,390＋7,569 B）、見積もりで約 13,000 トークンを占める。
   - 資料の量の候補はこの見積もりで選ぶ。予備の比較の解釈の注意として `SOURCE.md` に書く。
   - 模擬の固定の `input_tokens` は、再開した最初の判定には効かない。

### 組・検査・模擬

1. **既定で選ばれる課題が増える**
   - 推奨: `task.json` の `kind` で組を決める（`staged` は compaction、`normal` は normal、ほかは benitoite）。
   - `task.kind == 'test'` の分岐は、すべて新しい `result_kind` を見るように変える。

2. **`scripts/check.sh` が触るファイルにない**
   - `--inside-codex` の段は、全ディレクトリを `--task` で並べる（147–155）。「組の外の `--task` は誤り」の規則と組み合わせると、既定の self-test・dry-run が止まる。
   - 推奨: 既定の組だけを並べ、`--suite compaction` の self-test と dry-run を別に足す。dry-run の合格の条件（2260）も段の課題で満たすようにする。

3. **否定の対照の置き場がない:** 推奨: `task.json` に `self_test_failures: [{"script": "negative/<名前>.bnt", "case": "<judge_cases の名前>"}]` を足し、名前を挙げたケースで落ちることを確かめる。

4. **模擬の提供者**
    - 3 回で終わる今の筋書きは使えない。段ごとの筋書き、要約の要求（末尾の発言が `prompts/compaction.md` の文を含む）への応答、指定した `input_tokens` が要る。
    - 模擬の項目（例 `benitoiteh-mock-compact`、`context_window: 65536`、`max_output_tokens: 8192`）を `harnesses.json` に足す。

5. **エフェクト:** 最終の解は `Process.arguments()` を使うので `Process.Environment` を要する（`assets/benitoite/v0.0.7/skill/references/idioms.md:417`）。`task_allow` は最終の `reference.bnt` から決まるので、全段に同じ `--allow` が付く。

### P5-30・P5-33 との受け渡し

- 記録には `usage` と `kept_estimate`、イベントにはそれらと `estimate_before`・`estimate_after` がある。番は下記の規則で取り、段の番号とともに残す。
- `kept_estimate` は再構築前に選んだ範囲の見積もりであり、推論を除いた後の送信量とは違う。比較の成立の理由にもこの意味を使う。
- P5-33 が `materials/N/`、`judge_cases`、`self_test_failures` の具体的な中身を作る。固定の使用量は再開直後の判定には効かないので、P5-33 はバイト数の見積もりでも資料の量を選ぶ。
- Python 3.9.6 で動く書き方にする。

### 点検報告から修正した点

報告の「番は、イベントの直後の `turn_start` から取る」を次の規則へ改める。理由: `agent/mod.rs:846–873` では、超過による圧縮は `turn_start` の後に起き、要求の送り直しも同じ番に属する。直後の `turn_start` を使うと次の番へ誤って対応させる。

- `overflow` は、直前の `turn_start` からまだ `turn_end` に達していない実行中の番へ対応させる。
- `threshold` と `resume` は、圧縮の後の `turn_start` の番へ対応させる。上限などで次の番を始めなければ `turn: null` とし、その理由を残す。
- 偽のイベント列に超過と閾値の両方を含め、段と番の対応を確かめる。
