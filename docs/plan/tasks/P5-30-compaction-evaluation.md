# P5-30 圧縮の切り替えの口と長い課題

## 目的

BH-014 の予備と残す量を利用者の設定から変え、文脈の上限を小さく設定した長い課題で比較できるようにする。DeepSeek の Anthropic 互換の口の安全な残し方を変えず、依頼をまたぐ圧縮も測る。本物の LLM を呼ばず、BH-014 は閉じない。

## 読む設計書

- [文脈の管理](../../design/01-agent/01-04-context.md)の「文脈の大きさの見積もり」「圧縮」「圧縮の値を比べる設定」「推論の内容の扱い」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「上限」「やり直し」、[セッション](../../design/01-agent/01-05-sessions.md)の「再開」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」「プロジェクトの設定ファイル」
- [評価](../../design/07-quality/07-02-evaluation.md)の「M の段の共通の条件」「M の段の圧縮」「M の段の残りの規模と費用」
- [テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」、スキル `test-audit`
- [P5-29](P5-29-completion-check.md)、既存の `crates/benitoiteh/src/agent/compaction/{mod.rs,conversation.rs}`、`src/config/mod.rs`、`src/session/record.rs`、`tools/skill-eval/{skill_eval.py,mock_provider.py,harnesses.json,SOURCE.md}`、`tasks/json_summary/`・`tasks/dir_walk/`

## この作業が認める型の変更

- `CompactionConfig { reserve_tokens: u64, keep_tokens: u64 }` を加え、`UserConfig.compaction`・`ResolvedExec.compaction`・`AgentConfig.compaction` に渡す。既定は 16,384 と 20,000。`ProjectConfig` と提供者の機能の型は変えない。
- `Header` に省略可能な `compaction` の実効値を足してよい。現行値なら省略できる。古い記録を読めるようにし、再開では今の利用者の値を使う。評価では全段で同じ設定を保つ。
- `AgentEvent::Compaction` と対応する記録に、省略可能な `usage` と `kept_estimate` を足す。`usage` はその圧縮の要約の要求とやり直しの和、`kept_estimate` は実際に残す会話を既存の `item_bytes` で数え、バイト数を 3 で切り上げて割った値とする。既存の記録には値がなくてもよい。既存の集計へ二重に使用量を足さない。

## 作るもの

### 1. 設定と圧縮の計算

- `threshold_reached` の 16,384 と、`perform_compaction` の残す量の 20,000 を設定へ置き換える。出力の上限、文脈の上限の半分と 4 分の 1、上限が分からない場合と超過した場合の扱いは、設計書の式のとおりとする。加算は飽和させ、0 も受け付ける。
- `tail_start` の道具の呼び出しと結果を分けない境界、Anthropic の道具が続く途中の残す範囲なし、送り先が変わった再開の残す範囲なしは保つ。設定でこれらを上書きしない。
- `CompactReport.usage` は受け取りの合計と混ぜず、その圧縮だけの量をイベントへ渡す。圧縮の理由、番と段、前後の見積もり、要約した項目と残した項目の数と量を評価の行へ取り込めるようにする。

### 2. 評価の道具の設定と選択

- `config.compaction.reserve_tokens`・`keep_tokens` だけを許す。値は `type(value) is int` かつ `0 <= value <= 2**64 - 1` とし、Python の真偽を整数として受け付けない。P5-29 と P5-06 の真偽の鍵は今のままにする。
- 現行の `benitoiteh_config` は `context_window=200000` を無条件に書く。項目の `provider.context_window` があればその正の u64 を使い、省略したときだけ 200,000 とする。`provider.max_output_tokens` も正の u64 として検証して渡す。値の検証は、dry-run でも通る `load_harnesses` の段で行う。
- 次の項目を `benitoiteh-deepseek` の条件から作り、すべて `verified: false`・`default: false` とする。いずれも `context_window=65536`・`max_output_tokens=8192`・effort `high`・促しなしである。

  | 項目 | `config.compaction` |
  |---|---|
  | `benitoiteh-deepseek-compact-base` | `reserve_tokens: 16384, keep_tokens: 20000` |
  | `benitoiteh-deepseek-compact-reserve` | `reserve_tokens: 8192, keep_tokens: 20000` |
  | `benitoiteh-deepseek-compact-keep` | `reserve_tokens: 16384, keep_tokens: 8000` |

- `--suite benitoite|compaction` を加える。省略時は従来の 11 課題だけを選び、有料の既定の対象を増やさない。`--suite compaction` は新しい 2 課題だけを選ぶ。`--task` を併用したらその組の中から選び、不一致は起動前の誤りとする。P5-31 で `normal` を足せる形にする。

### 3. 段を分ける課題と走行

- `tasks/compact_json_summary/`・`tasks/compact_dir_walk/` を新設する。`task.json` は `kind: staged`、`result_kind: run`、`script`、`attempt_limit`、`stages`（相対の Markdown のパス 3 個の配列）、`http: false` を持つ。最終の `reference.bnt`・`expected.stdout`・`expected.files/` は既存の実行の課題と同じ使い方にする。段のパスは課題の中の通常のファイルに限り、リンクと `..`、絶対パスは拒む。
- 元の `json_summary` などを読むが、11 課題の原本は変えない。新しい 2 課題の内容は次のとおりとする。

  | 課題 | 第 1 段で与える条件 | 第 2 段で加える条件 | 第 3 段で求める変更と最終判定 |
  |---|---|---|---|
  | `compact_json_summary` | 注文の JSON を地域ごとに集計する。取消済みを除き、金額の欠落は 0 とし、地域を名前順に出す | 地域ごとの件数と金額を JSON ファイルにも書く。未知の地域を `other` にまとめる | CLI の引数で入力を選べるようにする。取消済み、金額の欠落、未知の地域、空の入力を含む別の入力で、全段の条件を判定する |
  | `compact_dir_walk` | `.md` のファイルの相対パスを名前順に出す。隠れたディレクトリと `drafts/` 以下を除く | 拡張子の大文字も扱い、ディレクトリごとの件数を JSON に書く | CLI の引数で走査の根を選べるようにする。除外する場所、大文字の拡張子、空の根と入れ子の入力で、全段の条件を判定する |

- `task.md` は 3 段の進め方だけを書き、各段の条件はその段の依頼の本文だけで渡す。後の依頼やファイルに前の条件の全文を写さない。条件を応答で繰り返すことを求めない。条件の欠落は、各条件に対応する判定の名前で分類する。
- 各段に、入力仕様の例と期待する中間の集計、走査の対象外になる例など、課題に関係する資料を付ける。標準ライブラリだけの生成の道具で、同じ引数なら同じバイト列になるようにする。解のコード、無意味な反復、確認を促す指示を混ぜない。資料は workspace の `materials/` に置き、一つ 12 KiB 以下に分ける。段の依頼の本文は 4 KiB 以下にする。理由: 大きな依頼を一つの会話の項目にすると、残す量のどちらにも入らず、残す量の差を測れなくなる。
- 初めは資料の総量を 24・48・64 KiB の候補から選んで作り、生成の引数とハッシュを記録する。資料が足りなければ同じ 12 KiB 以下の単位で増やす。模擬の使用量と複数の実際の読み取りの結果で、次の依頼の前に圧縮し、直近の複数の項目が残ることを確かめる。有料の試しでは、設計書 07-02 の圧縮と残す量の条件を満たすかを確認する。本体の前に全条件で共通の長さと段を固定し、足りなければ追加の試しを設計者へ示す。課題ごとの固定値を `task.json` の `generation` に残す。
- 1 試行では、初めの workspace と設定を作り、第 1 段を `exec` で走らせ、返った ID で第 2・3 段を `exec --resume <ID>` で続ける。同じモデル、送り先、プロンプトと道具のハッシュを保つ。段ごとの標準出力と標準エラー出力を別に保存する。途中の段が正常に終わらなければ後の段を送らない。
- 50 番・200 万トークン・30 分を段ごとにリセットしない。段の集計の差分を足し、次の exec に残る番とトークンの量、起動の時間を渡す。残りがなければ止める。要約の量は通常の集計に既に入るので二重に足さない。費用の停止額は段の後にも判定し、超えたら後の段と試行を止める。
- 最終の成果物は原本の入力を置き直した判定用のディレクトリで検査する。`task.json` に `judge_cases`（`name`・`input`・`args`・`expected_stdout`・`expected_files` の配列）を加え、課題の中の `cases/<名前>/` に入力と期待する出力を置く。すべてのパスに段のパスと同じ検査を掛ける。ケースごとに新しい判定のディレクトリを作り、原本の入力とケースの入力を重ね、最終のスクリプトだけを写し、`benitoite run <script> <args>...` の形で検査する（取り込んだ [v0.0.7 の Skill](../../../assets/benitoite/v0.0.7/skill/SKILL.md) の実行の形に従う）。JSON の生成ファイルは値で比べる。初めの条件の欠落もケースの名前で理由に残す。最終の reference の宣言したエフェクトが、各段の解に要るエフェクトをすべて含むようにし、既存の `task_allow` の規則で全段に同じ `--allow` を渡す。

### 4. 記録と使い方

- 1 試行 1 行の総計と、段ごとの内訳、全 exec の起動の数、要約の要求の数を残す。セッションの記録が累積することによる二重の集計を避ける。`compactions` と実際の記録を照合する。
- `comparison_valid` とその理由を残す。圧縮なし、残す量を比べる条件で実際の残す量が 0 のまま、設定による予備の違いが実効値に出ない場合は、比較が成立しなかったことを示す。成果物の判定は別に残す。
- `SOURCE.md` に、元の課題と変えた点、生成の引数、組の選択、複数の依頼と上限の合計、記録の追加を書く。`README.md` に、LLM を呼ばない準備・検査と、本物の走行の使い方を追記する。試し 3 回、本体 18 回の回数と費用は設計書 07-02 を参照する。

## 触るファイル

- `crates/benitoiteh/src/config/mod.rs`、`src/agent/mod.rs`、`src/agent/compaction/`、`src/session/record.rs` と、上記の型の既存の組み立て・表示・再生の箇所
- `crates/benitoiteh/tests/` の設定、圧縮、exec のテスト
- `tools/skill-eval/skill_eval.py`、`mock_provider.py`、`harnesses.json`、`test_skill_eval.py`、`README.md`、`SOURCE.md`
- `tools/skill-eval/tasks/compact_json_summary/`・`compact_dir_walk/`（新）、資料の生成の道具（新）

## テスト

- 設定の境界で既定、0、整数の範囲、真偽と負数の拒否、プロジェクトからの変更の拒否を既存の表に足す。
- ループの境界で、模擬の提供者の固定の使用量を使い、現行値では圧縮し、予備を減らすとまだ圧縮しない見積もりを作る。次の要求の実際の会話で違いを確認する。別のケースで、残す量だけを変え、残った発言と道具の呼び出しと結果の対応を確認する。
- Anthropic の道具が続く途中と送り先の変化の既存のケースを拡張し、`keep_tokens` が大きくても残さないことを確認する。これを確かめるために本物の LLM を使わない。
- 評価の道具で、3 段の準備、同じ ID での再開、段ごとの残る上限、費用での停止、集計の重複がないことを偽の exec の出力で確かめる。模擬の提供者による通しは、実際の再開と圧縮の記録までをつなぐ固有の契約だけを確認する。
- `--self-test --suite compaction` は、最終の解が判定を通り、初めの条件を意図的に一つ外した解が、その条件の判定で落ちることを両課題で確認する。`--dry-run --suite compaction` は LLM とキーを使わず準備と判定を通す。dry-run だけは解を workspace に置き、本物の評価の成績に混ぜない。
- 既定の 11 課題だけの選択と集計が変わらないことを確認する。テストのためだけに圧縮の内部の関数を公開しない。

## 完了条件

- `scripts/check.sh --inside-codex` と、LLM を呼ばない新しい課題の self-test・dry-run が通る。環境で実行できないサンドボックスと模擬の提供者の通しは、オーケストレータが外で実行する。
- 報告に、3 条件の圧縮の境、残した量、3 段の上限の合計、比較が成立しなかった場合の行の例を示す。有料の試しで長さを確定する点を残したこととして明記する。
