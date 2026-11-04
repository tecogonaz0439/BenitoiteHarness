# P5-42 クラウドの Linux での検査の環境と、時々失敗するテストの直し

- 状態: 完了
- 依存: P5-41

## 目的

使い捨ての x86_64 の Linux（Claude Code のクラウドのセッション）で全体の検査を走らせる環境を作り、検査の中で時々失敗するテストの原因を確かめて直す。時々失敗するテストは、テストの側で直し、本番のコードを変えない（2026-11-02 の設計者の条件）。

## 経緯

本作業は、2026-11-02〜03 にオーケストレータ（Claude Code）が設計者の指示で、作業用のブランチ `claude/stoic-ride-otqoj5` の上で行った。実装プランの外で行ったので、2026-11-04 に、`gamou` へ取り込んだ後で本書を作業の記録として起こした。作業の一覧と残っていることの入口は [2026-11-02〜03 の作業のまとめ](../../2026-11-02/README.md)にある。

## 作ったもの

- `scripts/cloud-linux-setup.sh`: クラウドの Linux の準備（bubblewrap、処理系 `v0.0.12` のビルド、`alpine:3.22` の rootfs、試験の利用者）。
- `scripts/cloud-linux-run.sh`: rootfs へ `pivot_root` し、root でない利用者でテストを走らせる cargo の runner。
- 時々失敗するテストの直し:
  - `tests/session.rs`: 子プロセスを起こすテストと錠に依るテストを、ファイルで共有する `RwLock` で重ねない形にした。
  - `tests/sandbox_linux.rs`、`src/cli/exec/tests.rs`: 継承された錠を短時間待つ形を加えた。
  - `tests/exec.rs` の `closed_event_output_still_finishes_the_session_record` と `tests/tui/commands.rs` の 1 件: 負荷をかけた再現で原因を確かめ、テストの側で直した。
- `tests/tui/pty.rs`: 確認、通知、外部のエディタを擬似端末で通すテストを加えた。
- `tools/skill-eval/test_skill_eval.py`: 深い JSON のテストを、Python の版に依らない入れ子の深さにした。

## 確かめた結果

- [クラウドの Linux での検査](../results/P5-42-cloud-linux-check.md)
- [`tests/session.rs` の時々失敗するテスト](../results/P5-42-session-flaky-tests.md)
- [ほかのテストのファイルの同じ競合](../results/P5-42-other-test-files.md)
- [Linux で進められる残件](../results/P5-42-linux-followups.md)

[P5-37](P5-37-post-release-followups.md) の「時々失敗したテスト」のうち、`tests/session.rs` の 4 件、`closed_event_output_still_finishes_the_session_record`、`stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths` は、本作業で原因を確かめて直した。
