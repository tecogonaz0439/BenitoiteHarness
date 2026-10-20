# P5-29 完了の前の確かめの促しと切り替えの口

## 目的

BH-011 を M の段で比べるために、正常に終えようとした LLM に、一つの依頼につき一度だけ検査やテストを促す仕組みを作る。利用者の設定 `agent.completion_check` で切り替え、既定は今と同じく促さない形にする。この作業では本物の LLM を呼ばず、BH-011 を閉じない。

## 読む設計書

- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「番の流れ」「完了の前の確かめ」「上限」「利用者の割り込み」
- [文脈の管理](../../design/01-agent/01-04-context.md)の「文脈の構成」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」「プロジェクトの設定ファイル」「exec の出力」
- [評価](../../design/07-quality/07-02-evaluation.md)の「M の段の共通の条件」「M の段の完了の前の確かめ」「M の段の残りの規模と費用」
- [テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」「LLM に見せる文の固定」、スキル `test-audit`
- [P5-06](P5-06-evaluation-switches.md)の補足（利用者だけの設定、記録、評価の道具の検証）
- 既存のもの: `crates/benitoiteh/src/agent/mod.rs` の `run_request`・`run_inner`・`apply_steering`、`src/config/mod.rs`、`src/cli/exec.rs`、`src/session/record.rs`、`src/provider/types.rs`、`tools/skill-eval/skill_eval.py`・`harnesses.json`・`test_skill_eval.py`

## この作業が認める型の変更

- `config` に `AgentBehaviorConfig { completion_check: bool }`（既定 `false`、`serde(default, deny_unknown_fields)`）を加え、`UserConfig.agent`・`ResolvedExec.agent`・`AgentConfig.agent` に渡す。`ProjectConfig` は変えない。
- `Agent` に一連の処理の間だけ持つ、促したかの真偽を加える。設定の型と `AgentConfig` の組み立てに伴う既存の呼び出し元の変更を認める。
- `Header` に省略可能な `completion_check: Option<bool>` を加え、有効のときだけ `Some(true)` を書く。既存の記録は読める形にする。再開では、そのときの利用者の設定を使い、古いヘッダを書き換えない。
- 促しの記録には既存の `Record::User` の `UserKind::Environment` と `UserPartKind::Environment` を使う。利用者の依頼の数を増やす列挙値や、新しい道具を加えない。

## 作るもの

### 1. 促しの文とループ

- `prompts/completion-check.md` を作り、設計書 01-01 の英文をそのまま置く。`include_str!` で埋め込み、会話に加えるときに `<completion_check>\n` と `\n</completion_check>` で囲む。過去の会話とシステムプロンプトを変えない。
- `run_request` の開始で、促したかの印を `false` にする。`run_inner` で取り消し、上限、拒否を処理し、操縦を加えた後の完了の分岐に判定を置く。`calls.is_empty()` だけで判定せず、終わりの理由が `Completed` のときだけを対象にする。操縦を加えた番は先に操縦に応じさせる。
- 有効でまだ促していなければ、上の文を記録へ追記してから会話へ加え、印を `true` にし、ループを続ける。次の要求も圧縮と上限の判定を通す。次の番を送れる余地がなければ、促さず通常の上限の終わり方にする。
- 追加の道具の実行も、今の実行器、確認、サンドボックス、取り消しを通す。検査やテストの実行をループが代行しない。
- 最初の完了の応答を削除せず、`exec` の最後の応答の出力は、続けた後の最後の応答にする。印は圧縮や操縦では戻さず、後続の依頼・再開後の新しい依頼で戻す。

### 2. 評価の道具

- `load_harnesses` の `config` の許可の一覧に `agent.completion_check`（厳密な真偽）だけを足す。綴り違い、知らない表、ほかの kind の `config` を拒む今の扱いを保つ。試行用の利用者の設定に `[agent]` を書く。
- `benitoiteh-deepseek-completion-check` を `benitoiteh-deepseek` の条件を写して加える。差は `config: {"agent": {"completion_check": true}}` だけとし、`verified: false`・`default: false` にする。比較の片側には、同じ実行ファイルで走らせる `benitoiteh-deepseek` を使う。
- セッションの記録の固定の促しの発言を境に、`completion_prompts`、`verification_calls_before`・`verification_calls_after`（`benitoite_check`・`benitoite_run`・`benitoite_test`・`benitoite_scratch` の名前ごとの数）、`edits_after_prompt`（`write`・`edit` の数）を行に残す。道具の実行を伴わない単なる文を検査の実行として数えない。促しがない側の `after` は 0 とする。
- 未検証の報告の有無は自動で採点せず、最後の応答を既存の `final.txt` に残し、要約の記録を作る者が判定した者とともに書く手順を `SOURCE.md` に示す。
- 見出しに設定を残す今の処理を使う。`SOURCE.md` の使い方と「写した後に変えた点」に、設定、比較の項目、集計の追加を書く。写した元のタグとコミットは変えない。

## 触るファイル

- `crates/benitoiteh/src/config/mod.rs`、`src/agent/mod.rs`、`src/cli/exec.rs` と、上記の型を組み立てる既存の呼び出し元
- `crates/benitoiteh/src/session/record.rs`、`prompts/completion-check.md`（新）
- `crates/benitoiteh/tests/` の設定、ループ、exec の既存のテストと、促しの文の固定の期待値
- `tools/skill-eval/skill_eval.py`、`harnesses.json`、`test_skill_eval.py`、`SOURCE.md`

## テスト

`test-audit` の作成時の関門を通し、既存の設定とループのテストを拡張する。次の契約を別の層で重ねて検査しない。

- 設定の境界: 省略で `false`、利用者の `true` の受理、型違いと知らない欄の拒否、プロジェクトの設定全体を使わず通知する扱いを確認する。
- ループの境界: 模擬の提供者を使い、正常な完了、促し、道具での検査や修正、再度の完了までを通す。次の要求に正しい文が一度だけあり、促しを再送してループが続きすぎないことを確かめる。無効では追加の要求がないことも対照とする。
- 同じ境界で、出力の上限・拒否・取り消し・番とトークンの上限では促さないこと、操縦と圧縮で印が戻らないこと、新しい依頼で戻ることを、既存のケースの表に足す。実際の LLM の判断は模擬しない。
- exec の境界では固有の契約だけを確認する。最後の応答と集計が追加の番を含み、記録からの再開で発言を失わず、促しを依頼として数えないこと。古いヘッダの読み取りも既存の互換のテストに加える。
- 評価の道具の境界: 設定の生成と拒否、促しの前後の実行と編集の集計を、手で用意した記録で確認する。英文は固定の期待値で守る。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定する）。本物の LLM とキーチェーンを使わない。
- オーケストレータが、サンドボックスの外で既存の模擬の提供者による 11 課題と、促しありの少なくとも 1 課題を通せる。Codex の環境で走らせなかった検査を報告する。
- 報告に、促しを挟んだ会話、最後の応答、前後の集計の例を示す。有料の試しの走行 2 回と本体 24 回の条件を、設計書 07-02 から提示できる状態にする。
