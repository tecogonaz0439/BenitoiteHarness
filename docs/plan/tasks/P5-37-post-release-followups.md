# P5-37 公開後の所見と不安定なテストの整理

- 状態: 未着手

## 目的

2026-10-21 の記憶の整理で、P5 の実装後に公開後でよいとして残した所見と、検査中に時々失敗したテストを記録する。元のレビューと作業日誌の要点を残すための作業であり、ここへの記載だけで修正方針を決定したり、未決事項の区分や時期を変えたりしない。BH-080 を残りの作業として提案するものでもない。

2026-10-21 のオーケストレータの判断で、P6 が表す「試作の後の判断」と分けるため、作業番号を P5-37 とした。この番号は着手時期を決めるものではない。キーチェーンのサービス名は道具の既定の契約として文書へ戻し、コードと設定の変更が要る絶対パスの一般化は本書の候補に残す。

## 読む文書

- [作業の進め方](../00-common.md)、[テスト](../../design/07-quality/07-01-testing.md)、`.claude/skills/test-audit/SKILL.md`
- [P5-24](P5-24-plugin-skill-followups.md)〜[P5-28](P5-28-image-helper-and-rewind-memory.md) の対応する作業文書
- [P5-31](P5-31-normal-mode-tasks.md)〜[P5-35](P5-35-normal-mode-tasks.md)、[評価](../../design/07-quality/07-02-evaluation.md)、`tools/skill-eval/SOURCE.md`

## 記録の読み方

以下は当時の所見の要約であり、今回新たに再現した結果ではない。場所の行番号はレビュー時点の目安である。Rust のパスで `src/`・`tests/`・`examples/` とだけ記したものは `crates/benitoiteh/` からの相対パスとする。所見の呼び名には元のレビュー報告のファイル名を添え、同じ作業の別の回と区別する。報告の全文は一時の場所へのリンクにせず、本書の要約を記録として残す。原因の推測、任意の改善、既存の仕様の限界は、その性質を保って記す。実装を始める前に現行のコードと履歴を照合し、既に対応したものを除く。

## 公開後に扱う所見

| 元の作業・所見 | 場所（行の目安） | 内容と確認する点 |
|---|---|---|
| MEM1021・中 3（2026-10-21 のオーケストレータの判断） | `tools/skill-eval/harnesses.json` の DeepSeek Harness の `start`・`version`、`tools/skill-eval/skill_eval.py` の起動の組み立て | 設定に残る手元の絶対パスを、置き換えの記号か環境変数から指定する形へ改める。今回の文書の修正ではコードと設定を変えない。 |
| P5-05・L1（2026-10-19 の作業日誌） | `tools/perf/measure.py`、`crates/benitoiteh/tests/perf_measure.rs` | 性能の測定道具の低の所見が残るとの記録があるが、記憶に個別の内容はなく、今回読めた報告と現行コードでは対応済みと確認できなかった。元の所見の特定から始める。 |
| P5-05・L2（2026-10-19 の作業日誌） | `tools/perf/measure.py`、`crates/benitoiteh/tests/perf_measure.rs` | L1 と別の低の所見として記録されているが、個別の内容と対応の有無は未確認である。既存の結果を新たな問題の根拠にせず、元の所見を特定する。 |
| P5-08・空の effort の文（2026-10-19 の作業日誌） | `src/config/mod.rs:467` 付近、`src/config/text.rs:17` | 空の effort を拒むが、理由は今も `configuration contains an empty name` であり、値の空欄と名前の空欄を区別していない。 |
| P5-09・途中の環境の変化（2026-10-19 の作業日誌） | `src/agent/compaction/conversation.rs:308` 付近の `rebuild`、[P5-09](P5-09-user-skills.md) の補足 16 | 圧縮で環境の情報を最初の項目から取り、途中の環境の変化が要約対象になった場合に最新の値を別に復元しない。元の作業では直さず残した。 |
| P5-09・Skill の候補数（2026-10-20 の作業日誌の情報） | `src/context/skills.rs:298,524,531` 付近の祖先の走査と `listing` | `.agents/skills` は各ディレクトリから最大 1,000 件を候補にするが、祖先をまたいだ出所全体の候補数の上限はない（プラグインの候補数の上限とは別）。 |
| P5-24・残した低（2026-10-20 の取り込み時の記録） | `src/context/skills.rs:216` 付近、`absolute_link` とその呼び出し | 途中の要素、`.agents/skills` 自身、`SKILL.md` が絶対パスのリンクの場合に、最後の要素だけの検査では「範囲の外」と区別した理由を示せない。 |
| P5-24・テストの重なり（2026-10-20 の取り込み時の記録） | `src/safe_fs/` の再試行のテスト、`tests/exec/skills.rs` の上限のテスト | EAGAIN の再試行の検査の重なりと、通知の上限のテスト (d) の前提を直接確かめる必要性を整理する（後者は任意）。 |
| P5-25・低 1（`P5-25-review4.md`） | `src/session/remove.rs:175` 付近の `path_of`、`src/session/text.rs:125` 付近の `tail_of` | 表示する最大 20 件の完全なパスを組み立ててから末尾 240 字へ切るため、保持量が深さと名前の長さに比例する。 |
| P5-25・任意のテスト改善（`P5-25-review4.md`） | `src/session/remove.rs:648` 付近の `long_kept_paths_keep_their_protected_tail` | 多バイトの名前でも末尾を正しく残すことを、既存の表の行で守れるか検討する。 |
| P5-25・補足 4 の低（`P5-25-review3.md`） | `src/session/remove.rs` の閉じた段の読み直し、元の作業文書「補足 4」の項目 3 | 読み直しの量は残す子の数と深い兄弟の数の積になる。有限で保持量も増えないとして、この作業では直さなかった。 |
| P5-26・低 1（`P5-26-review.md`） | `src/askpass/mod.rs:755` 付近の `serve` | 照合の値を登録できない分岐でも秘密を書く形であり、将来の上限変更で伏せ字が漏れうる（当時の不変条件ではその分岐に入らない）。 |
| P5-26・低 2（`P5-26-review.md`） | `src/askpass/mod.rs:55` 付近の `Prompt.call`、`src/ui/tui/confirmation.rs:642` 付近 | 閉じた入力欄の `Prompt` が ESC まで呼び出しの状態と照合の値を保持するため、参照の持ち方を検討する。 |
| P5-26・低 3-1（`P5-26-review.md`） | `src/session/mod.rs:463` 付近、`tests/session.rs:2372` 付近 | 不正な `id` の子と、`id` と名前が合わない子を掃除で残す分岐を、既存のテストが守っていない。 |
| P5-26・低 3-2（`P5-26-review.md`） | `src/askpass/tests.rs:281` 付近の偽の `id` の表 | ハードリンクの `id` を拒む行がない。 |
| P5-26・低 3-3〜5（`P5-26-review.md`） | `src/askpass/tests.rs:828` 付近と `:334` 付近 | 状態を手放すテストの前半と長い `id` の行の説明を実際の契約に合わせ、退行テストを修正前に走らせたかも確認する。 |
| P5-26・試験の後始末（`P5-26-review.md`） | `src/askpass/tests.rs`、試験用の補助ディレクトリ | テストが実際の一時の根へ補助ディレクトリを作るため、共有する範囲と後始末を確認する。 |
| P5-27・低 1（`P5-27-review.md`） | `src/auth/mod.rs:294` 付近の `exchange` | ログインの 2xx 応答を `TokenResponse` として読めないと、本文にリフレッシュトークンがあっても取り消さない。 |
| P5-27・低 2（`P5-27-review.md`） | `src/auth/callback.rs:25` 付近の待機と `:224` 付近の `accepted` | Linux で `EMFILE`・`ENFILE` が続くと、待機が空回りする可能性がある（Linux では未再現の推測）。 |
| P5-27・低 3（`P5-27-review.md`） | `tests/auth_chatgpt.rs` の未完の接続を使う表 | 戻りの前半を受けた後の接続数について、64 件の上限そのものを守るケースがない。 |
| P5-27・低 4（`P5-27-review.md`） | `tests/provider_openai_responses.rs:350` 付近と `tests/support/chatgpt.rs` | 誤りの詳細の文字種の検査だけでも通るため、両方のトークンを伏せるというテスト名の約束を直接観察できない。 |
| P5-28・低 2（`P5-28-review.md`） | `src/session/replay.rs:345` 付近の `fill_missing` | 後方の結果を前へ移す場合と、同じ ID の結果が重複する場合の取り出しを直接守るテストがない。 |
| P5-30・低 1（`P5-30-review.md`） | `src/session/record.rs:106` 付近、`src/config/mod.rs:366` 付近 | ヘッダの圧縮の値に未知の鍵を拒む設定の型を使うため、将来の鍵の追加で古い版から再開できなくなる。 |
| P5-30・低 2（`P5-30-review.md`） | `src/agent/mod.rs:378` 付近、`src/session/record.rs` の `SessionStart`、`src/session/fork.rs:326` 付近 | 再開・フォーク時の圧縮の実効値が最初のヘッダと異なっても、その段の記録では区別できない。 |
| P5-30・低 3（`P5-30-review.md`） | `tests/session.rs:460` 付近、`tests/agent_loop/compaction.rs:688` 付近、`tests/config.rs:773` 付近 | 旧イベントの読み戻しの重複、手動圧縮の観察する境界、出力量に依存する表の前提、否定の理由の確かめを整理する。 |
| P5-31・低 1（`P5-31-review3.md`） | `examples/skill_eval_judge.rs:450` 付近の `lsof_path` と対応する単体テスト | lsof が逆斜線を二重に表示するのに根を同じ形へ変換しないため、その名前の根の下に残ったプロセスを見落とす。 |
| P5-31・低 2（`P5-31-review3.md`） | `examples/skill_eval_judge.rs:430` 付近の cwd の照合 | cwd が 1024 バイトを超えて lsof の表示が先頭を欠くと、書ける場所の下にいても残ったプロセスを見落とす。 |
| P5-32・情報 3（`P5-32-review3.md`） | `tools/skill-eval/skill_eval.py` の `judge_run`、`SOURCE.md` の判定の起動先の一覧 | awk・sed などを使う正しい解も起動先の制限で落ちうるため、失敗の分類でサンドボックスの制限と区別できるか確認する。 |
| P5-32・情報 4（`P5-32-review3.md`） | `tools/skill-eval/skill_eval.py` の `judge_check` と `task_allow` | 静的な検査はサンドボックスの外で動く。内容の持ち出しは当時見つからなかったが、依存先の有無を調べられる可能性は未確認の注意として引き継ぐ。 |
| P5-33・情報 4（`P5-33-review.md`） | `tools/skill-eval/skill_eval.py:2301` 付近の `prepare_stage` | 通常ファイルである既存の資料がハードリンクなら写し込みで上書きする（当時の制限モードの課題で作れるかは未確認）。 |
| P5-33・情報 5 の残り（`P5-33-review.md`） | `tools/skill-eval/tasks/compact_dir_walk/task.json` と判定のケース | 根自身の名前が `drafts` や隠れた名前でも除外しない契約を調べるケースがない。 |
| P5-33・情報 6 の残り（`P5-33-review.md`） | `tools/skill-eval/test_skill_eval.py` の段の準備のテスト | `reference`・`cases`・`negative`・`stages` が作業ディレクトリへ写らないことを直接確かめる行がない。 |
| P5-34・情報 I-b（`P5-34-review2.md`） | `tools/skill-eval/README.md:20` 付近の道具の版の記録 | 記録する道具は判定の PATH（`/usr/bin:/bin`）で探すことと、エージェントの PATH と異なりうることを説明する。 |
| P5-35・情報 I1（`P5-35-review.md`） | `tools/skill-eval/SOURCE.md:423` 付近、通常モードの `judge/verify.py` | 成果物の子も `trusted/` を読めるため、期待値に合わせる不正を防ぎきれないという既知の限界を引き継ぐ（I1 の追加の不正例は未検証）。 |
| P5-35・情報 I2（`P5-35-review.md`） | `scripts/check.sh:176` 付近の normal の分岐 | normal の枝の後の `elif` は常に真なので、必要なら `else` に整理する（動作には影響しない）。 |
| P5-35・情報 I4（`P5-35-review.md`） | `tools/skill-eval/tasks/normal_config_merge/task.md` の引数の説明 | `DEFAULTS_JSON OVERRIDES_JSON` がファイルのパスを指すことを明示するか確認する。 |

## 時々失敗したテスト

| 元の検査・作業 | 場所（行の目安） | 症状と扱い |
|---|---|---|
| P5-30・P5-31・P5-32（P5-25 の後）（`P5-30-review.md`） | `tests/session.rs:796,1013,1377,2462` 付近 | `linked_storage_parents_allow_drop_resume_and_cleanup`、`cleanup_reports_failures_continues_and_retries_without_losing_locks`、`expired_session_tombstone_outlives_its_unremovable_temporary_directory`、`unavailable_temporary_root_is_diagnosed_without_skipping_session_cleanup` が断続的に失敗した。TMPDIR の共有が原因という説は推測である。 |
| P5-31 の全体の検査 | `tests/exec.rs:687` 付近の `closed_event_output_still_finishes_the_session_record` | BrokenPipe による失敗が報告された。原因は未確認である。 |
| P5-12・P5-25 の全体の検査 | `src/benitoite/tests.rs:194` 付近の `queries_bound_output_timeout_and_propagate_cancellation` | overflow の行が `OutputLimit` 以外で失敗した。P5-13 で対応したと記録されていたが、2026-10-20 に再発した。 |
| P5-19 の全体の検査 | `tests/run.rs:40` 付近の `input_and_both_output_streams_are_drained_and_may_share_a_sink` | 当時は `pipe_close_wait` が 100 ms で `PossiblyRemaining` になった。移行時のコードでは 5 秒へ変更済みであり、再発の有無を確かめる。 |
| P5-04 の x86_64 の全体の検査 | `tests/sandbox_linux.rs:1520` 付近の `stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths` | 錠のあるパスの存在の確かめが一度失敗し、再実行では通った。原因は未確認である。 |
| P5-16 の検査中の記録 | `tests/user_skills.rs:573` 付近の `user_skills_take_count_and_byte_budgets_before_project_skills` | TMPDIR が長いと件数の上限より先に索引の 16 KiB の上限へ達し、期待する件数を満たさないと報告された。 |

## 後の作業で扱いが記録された所見

元のレビューの次の指摘は、移行時点で後続のコード・文書に対応がある。未着手の修正として重ねず、上の候補を調べるときの手がかりとして残す。

| 元の作業・所見 | 場所（行の目安） | 記録された扱い |
|---|---|---|
| P5-09・create_project 後の Skill（2026-10-19 の作業日誌） | [P5-09](P5-09-user-skills.md) の補足 15、`src/context/environment.rs:57` 付近の `with_project`、`src/agent/execution.rs:393` 付近 | 一覧と根を切り替えで変えないという既存の仕様に従い、コードも Skill を保持する。未修正の不具合として重ねず、この仕様の記録を引き継ぐ。 |
| P5-28・低 1（`P5-28-review.md`） | `src/sandbox/linux/filter/tests.rs`、`src/tools/read/image_helper.rs` | 取り込み前の再開で、BPF の拒否の表と直接の exec の拒否を確かめるテストを追加した。 |
| P5-32・低 1（`P5-32-review3.md`） | `tools/skill-eval/skill_eval.py:1187` 付近の `run_command` | 正常終了後も同じプロセス群を止める変更を行い、`SOURCE.md` の P5-33 の再レビューの記録に記した。 |
| P5-32・低 2（`P5-32-review3.md`） | `tools/skill-eval/SOURCE.md:409` 付近、設計書 07-02「評価の手順」 | 入れ子の処理系をエフェクトの検査だけで止めるとはせず、OS の方針が境界を守ると説明を改めた。 |
| P5-32・低 3（`P5-32-review3.md`） | `tools/skill-eval/SOURCE.md:411` 付近、`test_judge_sandbox.py` | 別のループバックへの通信、許さないシェルの操作、段の課題、特殊なパスを正の対照と組にした検査を加えた。 |
| P5-32・情報 2（`P5-32-review3.md`） | `tools/skill-eval/SOURCE.md` の判定の通信の説明 | localhost の例外が同じポートの `::1` と UDP も許すことを記した。 |
| P5-33・低 1、情報 1・2（`P5-33-review.md`） | `tools/skill-eval/SOURCE.md` の P5-34 の修正、設計書 07-02「評価の手順」 | HOME だけの拒否から、読み取りの根を列挙する共通の実行器へ変更した。HOME 外の読み取りと HOME の差し替えに関する旧方式の所見として扱う。 |
| P5-33・情報 3（`P5-33-review.md`） | `tools/skill-eval/SOURCE.md` の判定の理由の抜粋、設計書 07-02「評価の手順」 | 120 文字の抜粋は任意の秘密の伏せ字ではないことを記した。 |
| P5-33・情報 5 の見積もり（`P5-33-review.md`） | [P5-36](P5-36-compaction-boundaries.md)「調査の結果」 | Skill の読み取りも含めて文脈の量と段の境を調べた。調整は保留のまま引き継ぐ。 |
| P5-34・情報 I-a（`P5-34-review2.md`） | `tools/skill-eval/SOURCE.md` の残ったプロセスの検出の限界 | 印を消して cwd も外へ移した子孫の不存在を保証しないと記した。 |
| P5-35・低 L1〜L3（`P5-35-review.md`） | `normal_text_replace/judge/verify.py:59`、`normal_cli_exit/judge/verify.py:14,21`、`normal_config_merge/judge/verify.py:62` 付近 | 置換文字列と名前の逆斜線、未知のサブコマンド、重なる入れ子の入力を判定するケースを追加済みである。 |
| P5-35・低 L4（`P5-35-review.md`） | `tools/skill-eval/test_skill_eval.py` の `test_self_test_rejects_broken_controls` | 実行不能の対照を self-test が拒む検査を追加した。 |

## 作るものと完了条件

- 各候補を現行のコードと照合し、修正、既存の仕様の説明、対応済み、追加の調査のいずれとして扱うかを記録する。安全の仕様や未決事項の判断を要する変更は、設計者の判断を得る。
- 不安定なテストは、再現した環境と失敗の理由を分け、原因を調べてから必要な修正を行う。既に待ち時間を変更したものは、その後の結果を確かめる。
- テストの追加・変更は `test-audit` の作成時の関門に従う。単に待ち時間を伸ばすことを既定の修正にしない。
- 実装した範囲に応じた検査と `scripts/check.sh --inside-codex` を通し、OS のサンドボックスとコンテナを要するものはオーケストレータが外で確かめる。
- 本物の LLM を呼ぶ評価は、この作業の開始だけでは行わない。必要な場合は AGENTS.md に従う。
