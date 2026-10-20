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
- `crates/benitoiteh/src/agent/compaction/conversation.rs`（促しを環境の情報として先頭へ持ち上げない処理）
- `tools/skill-eval/skill_eval.py`、`mock_provider.py`、`harnesses.json`（`benitoiteh-mock-completion-check` を含む）、`test_skill_eval.py`、`SOURCE.md`
- `scripts/check.sh`（促しありの模擬の通しを別に加える）

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

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。以下の推奨を、明記した修正を除いてこの作業の指示として採る。コードの行番号は、点検報告が参照した main の `d67e1af` のものである。

### 1. 触る箇所

- **ループ（`crates/benitoiteh/src/agent/mod.rs`）**
  - `AgentConfig` は 47–62、`From<ResolvedExec>` は 64–79。
  - `Agent` の欄は 222–260。促したかの印はここに足す。
  - ヘッダは 344–375 で書く。`after_write_check` の 370 と同じ形で `completion_check` を足す。
  - `run_request` は 726–793。先頭の `check_limit.clear()`（733）の並びで印を `false` に戻す。
  - `run_inner` は 795–976。番とトークンの上限は 806–811、残る子孫は 926、許可の外は 929、検査と修正の上限は 934–958、取り消しは 959–964、トークンは 965、拒否は 968、操縦は 971。
  - 一連の処理を終える判定は 972–974 の `calls.is_empty() && response.finish_reason != FinishReason::OutputLimit && !steered`。
  - `apply_steering` は 978–1005。`UserKind::Steer` で記録する書き方の手本になる。
- **`AgentConfig { .. }` の字面の組み立て（12 箇所）:** `agent/execution.rs:634`、`agent/resume_tests.rs:68,229,327,605`、`agent/compaction/tests.rs:91`、`tests/agent_loop.rs:82`、`tests/support/rewind.rs:96`、`tests/tui.rs:301`、`tests/sandbox_benitoite_test.rs:337`、`tests/sandbox_benitoite_run.rs:476`、`tests/sandbox_benitoite_scratch.rs:293`。
- **設定（`src/config/mod.rs`）:** `UserConfig` は 360–376、`parse` は 392–450、`resolve_exec` は 461–534（`ResolvedExec` を作るのは 506–533）、`ResolvedExec` の定義は 573–588。`ProjectConfig`（591–596）は変えない。
- **記録（`src/session/record.rs`）**
  - `Header` は 91–115。手本は 102–103 の `after_write_check`。
  - `UserKind` は 131–138。`Environment` は定義だけで、今はどこからも書かれていない。
  - `Record::User` は 300–305。
  - `Header` を字面で組み立てるのは `tests/session.rs:165` と `tests/support/rewind.rs:140` の付近。
  - フォークは元のヘッダを写す（`session/fork.rs:323–331`）。
- **exec:** `AgentConfig` への変換は `cli/exec.rs:750`。最後の応答は 878–895 で、会話の最後の `AssistantTurn` の文を使う。促しの後の応答が自然に選ばれるので変更は要らない。TUI も `exec::prepare` を通る（`cli/tui.rs:81`）。
- **評価の道具（`tools/skill-eval/skill_eval.py`）:** `load_harnesses` は 1463–1482（許可の一覧は 1470）、`benitoiteh_config` は 166–194、`benitoiteh_results` は 528–575、`final.txt` は 1686、模擬の起動は 1594、見出しの `config` は 2186。

### 2. 食い違い・曖昧な点と推奨

1. **記録の型と再開**
   - 推奨は `append_node(Record::User { kind: UserKind::Environment, message: UserMessage { parts: vec![UserPart { kind: UserPartKind::Environment, text }] } })` の後に `push_item` する形である。
   - 読み手を辿った結果、ほかに変更は要らない。
     - 再生は種類を問わず会話へ戻す（`session/replay.rs:311`）。
     - 依頼の一覧（巻き戻し、木）は `kind == Request` だけを見る（`replay.rs:272–287`）。
     - TUI の履歴は `Request|Steering` の部分だけを示す（`ui/tui/mod.rs:481,490`）。
     - 圧縮の依頼の数は `Request|Steering` だけを数える（`compaction/conversation.rs:138`）。
   - 既存の不一致: ループの中の環境の変化は `UserKind::Request` で書いている（`agent/mod.rs:819–823`）。依頼の数には影響しないので、この作業では変えない。
   - `UserKind::Environment` を書くのはこれが初めてなので、再開とフォークで読めることを exec の境界で一度確かめる。
   - 印はメモリだけに持つ。`construct` で `false` にし、記録からは戻さない。
2. **圧縮で促しが先頭に残り続ける**
   - `rebuild`（`compaction/conversation.rs:310–363`）は、最初の項目の `Environment` の部分をすべて先頭へ持ち上げる（316–323）。
   - 残した範囲の先頭が促しの発言だと、次の圧縮から促しの文が環境の情報として先頭に残り続け、後続の依頼にも送られる。
   - 推奨: 持ち上げるときに `<completion_check>\n` で始まる部分を除く。`compaction/conversation.rs` を触るファイルに足す。
   - Pi の作り方の文字化（176）は環境の部分を落とすので、促しは要約の別の要求には入らない。これは許容し、テストの表に書く。
3. **文の包み方:** `include_str!` は末尾の改行を含む。`compaction/mod.rs:519` の `PROMPT.trim_end()` にならい、`format!("<completion_check>\n{}\n</completion_check>", TEXT.trim_end())` とする。
4. **判定の位置**
   - 972 の直前に、条件 `calls.is_empty() && finish_reason == FinishReason::Completed && !steered && 有効 && !印` を置く。
   - 取り消し、トークン、拒否、検査と修正の上限、残る子孫、許可の外は、それより前で戻るので促さない。
5. **「次の番を送れる余地がなければ…通常の上限の終わり方」は曖昧**
   - 番の上限は次の番の先頭（806）で判定する。促して続けると、最後の番で正常に終えた試行が `Limit(TURN_LIMIT)` の失敗に変わる。
   - 推奨: `summary.turns >= max_turns` なら促さず、今と同じ `Outcome::Completed` で終える。無効のときと同じ終わり方になる。
6. **イベント:** 促しの `AgentEvent` は加えない（認める型にない）。そのため `--json` には現れず、評価は記録から数える。
7. **評価の道具の集計**
   - 境は、`type=="user"`・`kind=="environment"` で、本文が `crates/benitoiteh/prompts/completion-check.md` を同じ規則で包んだ文と一致する行とする。
   - 文は `REPO` から読む（`CODES_FILE` と同じ扱い）。同じ英文を Python に二重に持たない。
   - 数えるのは `tool_call` の行のうち `validation_error` が `null` のものとし、名前ごとに境の前後で数える。`Summary.tool_calls`（`execution.rs:69–71`）と既存の `attempts`（`skill_eval.py:271`）と同じ規則である。
   - `tool_result` の行で数えると、出力の上限や取り消しで実行しなかった呼び出し（`execution.rs:78–95`）も数えてしまう。
   - `edits_after_prompt` も同じ規則で `write`・`edit` を数える。
8. **模擬の提供者が触るファイルにない**
   - `mock_provider.py` は 4 回目の要求を誤りにし（38–41）、2 回目以降の要求の末尾が `tool_result` であることを求める（44–51）。促しありでは 4 回目の末尾が促しの文になるので、このままでは通らない。
   - 推奨: `start(..., completion_check=False)` を足し、有効なときだけ、4 回目（末尾が促しの文）で検査の道具を呼び、5 回目で `Done.` を返す。
   - `harnesses.json` に `benitoiteh-mock-completion-check`（`mock: true`、`config.agent.completion_check: true`、`default: false`）を足す。
   - `check.sh` の模擬の通し（167–179）は 11 行を期待している。促しありは別の起動として足す。`check.sh` も触るファイルに足す。
9. **条件の順の入れ替えがない**
   - 設計書 07-02「M の段の共通の条件」は、同じ課題・反復の中で条件の順を入れ替えると定める。
   - 今の道具は、項目ごとに全課題・全試行を走らせてから次の項目へ移る（`evaluate_main` の 2127）。
   - 推奨: この作業で `--interleave`（複数の項目のときだけ、反復ごとに順を回す）を加え、P5-32 の圧縮の走行でも使う。同じ課題・反復の全条件を走らせてから次へ進み、条件の先頭を反復ごとに回す。起動した条件の順を記録し、全条件で一つの費用の累計を共有する。オーケストレータが手で起動を分ける方法もあるが、記録が試行ごとに分かれ、`--stop-cost` の残額も手で渡すことになるので勧めない。

### 3. 依存と分け方

- 最初に行う。P5-30 と P5-31 をこの作業の後に並べて進められる。P5-30 は設定の型とヘッダを、P5-32 は `load_harnesses` と `--interleave` を、P5-34 は `[agent]` の表を前提にする。
- `--interleave` はこの作業で実装し、別の作業へは回さない。偽の試行の結果で順の回転、中断時の記録、条件をまたぐ費用の停止を確かめる。

### 4. テストの注意

- **ループのテスト:** `tests/agent_loop/` の模擬の提供者と、偽の `query`（`tests/agent_loop/checks.rs`）で書く。`tests/sandbox_*.rs` は Codex の中で走らないので、そこに置かない。
- **字面の組み立て:** 12 箇所のうち `tests/sandbox_*.rs` の 3 箇所は、Codex の中でも `clippy --all-targets` がビルドする。新しい型には `Default` を付け、欄は字面で明示する。
- **文の固定:** `tests/expected/completion-check.txt` を置き、模擬が受け取った要求の末尾の `text` と比べる。
- **Python の版:** `/usr/bin/python3` は CLT の 3.9.6 である。`X | None` と `match` を使わない。
- **待ち受け:** 模擬を起動するテストは、待ち受けに失敗したら `skipTest` する（`test_skill_eval.py:377–379` の形）。
