# ほかのテストのファイルの同じ競合の調べと修正

本書は、[時々失敗するセッションのテスト](02-session-flaky-tests.md)と同じ競合（同じテストのプロセスの別のスレッドが起こした子が、`exec` するまで記述子の複製を持つこと）で、ほかのテストのファイルと `src/` の単体テストの結果が変わる箇所の調べと、見つかった 2 件の修正を記録する。修正は、2026-11-02 の設計者の決定に従った。まとめは [README](README.md) にある。

本物の LLM は呼んでいない。macOS で確かめる項目（Seatbelt、評価の道具の判定）と aarch64 の Linux（Apple の `container`）は、この環境にないので扱わず、別途とする。

## 調べ方

1. 本番のコードで `flock` を使う場所を探し、複製が結果を変えうるものを選んだ。
2. テストで、錠を持つ記述子を落とした直後に、別の記述子で非待機の錠を取る処理（後始末、回収、再開）の結果を確かめる箇所を探した。
3. 候補のテストに、[時々失敗するセッションのテスト](02-session-flaky-tests.md)の「確かめ」と同じ差し込み（`benitoiteh --version` を起こし続ける 4 スレッド。`src/` の単体テストでは `/bin/true`）を、別の git の worktree で一時的に加え、一つだけ（`--exact`）を 15 回ずつ走らせた。差し込みのない同じ回数を対照とした。書き換えはコミットしていない。
4. 差し込みなしの全体のテスト（`cargo test --workspace --target x86_64-unknown-linux-musl`、Alpine、非 root）を 20 回繰り返し、自然に起きる失敗を数えた。この 20 回は、`tests/session.rs` の直しを含むコードで走らせた。

## 本番のコードの flock

| 場所 | 錠の取り方 | 複製の影響 |
|---|---|---|
| `src/session/mod.rs` | 非待機の排他。取れなければ使用中と見る | 結果が変わる（[時々失敗するセッションのテスト](02-session-flaky-tests.md)） |
| `src/sandbox/linux/mounts/placeholders.rs` の回収（`reap`） | 非待機の排他。取れなければ、その記録と塞ぐ場所を回収しない | 結果が変わる |
| `src/auth/store.rs`、`src/ui/tui/history.rs` | 待機する排他（`auth/store.rs` の `lock_cancellable` は 20 ms ごとに取り直す） | 子が `exec` するまで待つだけで、結果は変わらない |

## すでに取り直しで扱っている箇所

次の箇所は、この競合を注記したうえで、`Locked` のときだけ取り直している。今回の調べの対象から外した。

- `tests/support/session.rs` の `open_created_session`（10 ms ごとに最大 100 回）
- `src/agent/resume_tests.rs`（最大 2 秒）
- `src/cli/exec/tests.rs` の再開の準備（最大 2 秒）

`tests/rewind.rs` と `tests/tui.rs` の再開は、持ったままの `Session` を `Agent::resume` に渡し、錠を取り直さないので、影響しない。

## 候補と結果

| テスト | 錠を落とした後の処理 | 差し込みあり | 対照 | 差し込みなしの全体の 20 回 |
|---|---|---|---|---|
| `tests/sandbox_linux.rs::stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths` | `drop(lock)` の直後に次のサンドボックスを準備し、回収で `!locked.exists()` を確かめる | 15 回のうち 3 回（すべて `!locked.exists()`） | 15 回のうち 0 回 | 20 回のうち 1 回（同じ箇所） |
| `src/cli/exec/tests.rs::fork_retains_copy_when_current_provider_was_removed` | 失敗したフォークが残した写しを、取り直しなしの `Session::open` で開く | 15 回のうち 2 回（`Error: Locked`） | 15 回のうち 0 回 | 0 回 |
| `tests/sandbox_exec_restricted.rs::continue_selects_latest_matching_readable_inactive_session` | `saved` で作って落としたセッションを、子の `--continue` が選ぶ | 15 回のうち 1 回（`Kind(TimedOut)`。錠の症状ではなく、4 コアを差し込みが占めたための遅れと見る） | 調べていない | 0 回 |
| `tests/sandbox_exec_restricted.rs::resume_errors_preserve_records_and_never_escalate_or_follow_links` | `drop(lock)` の後に子の `--resume` が形式の誤りを報告する | 15 回のうち 0 回 | — | 0 回 |
| `tests/sandbox_exec_restricted.rs::sessions_lists_last_mode_filters_projects_and_survives_unreadable_records` | `saved` で作って落としたセッションを、子の `sessions` が一覧にする | 15 回のうち 0 回 | — | 0 回 |
| `src/sandbox/linux/mounts/tests.rs::placeholder_records_preserve_path_bytes_and_validate_record_identity` | 回収を続けて呼び、記録が消えたことを確かめる | 15 回のうち 0 回 | — | 0 回 |

`stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths` は、P5-37 の「時々失敗したテスト」の 5 行目（P5-04 の x86_64 の全体の検査で、錠のあるパスの存在の確かめが一度失敗した）に当たる。差し込みで同じ箇所が失敗し、差し込みなしでも同じ箇所が 20 回のうち 1 回失敗したので、`tests/session.rs` と同じ原因と見る。

`fork_retains_copy_when_current_provider_was_removed` は、差し込みで `Locked` になった。差し込みなしの 20 回では失敗しなかったが、同じ形の箇所で取り直しを入れたテスト（「すでに取り直しで扱っている箇所」）と並ぶ、取り直しのない箇所である。

錠の症状が出なかった候補は、錠を落としてから錠を取り直すまでに子プロセスの起動などがはさまり、窓が狭いためと見るが、確かめていない。

## ほかの失敗（同じ競合と確かめていないもの）

差し込みなしの全体の 20 回で、ほかに次の 2 件が 1 回ずつ失敗した。

| テスト | 症状 | 見立て |
|---|---|---|
| `tests/exec.rs::closed_event_output_still_finishes_the_session_record` | `Error: Kind(UnexpectedEof)`（P5-37 の記録では `BrokenPipe`） | テストが子の標準出力の読み口を閉じ、子が書き込みの失敗を受けることを前提とする。別のスレッドの子が読み口の複製を持つと閉じたと見えない、という道筋はありうるが、`?` で返った誤りの場所を特定していない |
| `tests/tui/commands.rs` の `accepted_rewind_finishes_before_quit_and_repeated_keyboard_interrupts` | 画面の出力に `Waiting for rewind to finish` が現れなかった | 時刻に依る表示の確かめであり、差し込みの実験と同時に走った回で起きた。負荷による遅れと見るが、確かめていない |

`tests/session.rs` は、直した後のコードで 20 回とも通った。

2026-11-03 に、この 2 件を負荷をかけて再現し、原因を確かめて、テストの側で直した（[09](09-linux-followups.md)）。

## 直し方と確かめ

2026-11-02 に、設計者が次の 2 件を直すと決めた。どちらも、このリポジトリで同じ競合に使ってきた形（`Locked` や回収の残りだけを短時間待ち、錠を放さない退行は 2 秒で失敗にする）に揃えた。読み書きの錠（`tests/session.rs` の形）にしなかったのは、確かめる箇所が一つであり、`tests/sandbox_linux.rs` の 36 件のテストのすべてに共有の側を持たせる変更が要るためである。本番のコードは変えていない。

- `stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths`: `drop(lock)` の後の準備（回収を含む）を、`locked` が消えるまで 20 ms ごとに最大 2 秒やり直す。
- `fork_retains_copy_when_current_provider_was_removed`: `src/cli/exec/tests.rs` の再開の後の取り直し（`declined_normal_resume_preserves_record_bytes_and_releases_lock` の中にあった）を `open_released` にまとめ、このテストの `Session::open` にも使う。

| テスト | 直す前の差し込みあり | 直した後の差し込みあり |
|---|---|---|
| `stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths` | 15 回のうち 3 回 | 15 回のうち 0 回 |
| `fork_retains_copy_when_current_provider_was_removed` | 15 回のうち 2 回 | 15 回のうち 0 回 |

直した後に、`cargo test --workspace --target x86_64-unknown-linux-musl`（754 件が通り、失敗 0）、`cargo fmt --all --check`、`cargo clippy -p benitoiteh --all-targets`（ホスト、`x86_64-unknown-linux-musl`、`aarch64-apple-darwin`）が通った。
