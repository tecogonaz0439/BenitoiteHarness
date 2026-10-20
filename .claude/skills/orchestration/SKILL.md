---
name: orchestration
description: Claude Code が BenitoiteHarness の作業を Codex に依頼し、検査・レビュー・取り込みを進めるときの手順。Codex 自身の実装手順は docs/plan/00-common.md に従う。
---

# オーケストレータの作業手順

このスキルは、オーケストレータ（Claude Code）が使う。設計書・実装プラン・実装の執筆は Codex に依頼し、Claude Code は作業の割り当て、点検、検査、レビュー、取り込みを行う。レビューには Codex を使わない。分担は `AGENTS.md`、実装者の手順は [00-common.md](../../../docs/plan/00-common.md) に従う。

## 作業ごとの流れ

1. Codex に作業の文書を書かせる。作るもの、触るファイル、依存、完了条件を明示する。
2. Claude Code が、作業の文書を設計書と既存のコードに照らして点検する。修正が要る場合は Codex に補足を書かせ、点検してから実装へ進む。
3. 作業ごとのブランチと git worktree を用意し、後述の条件で Codex に実装を依頼する。
4. Codex の報告を受け、オーケストレータがサンドボックスの外で `scripts/check.sh` の全体を走らせる。失敗は出力と終了状態を添えて Codex に返す。
5. Claude Code が変更をレビューする。修正を依頼した場合は、その修正に応じて検査とレビューをやり直す。
6. 取り込む直前に、対象の変更に対する検査の出力と `exit=` の行を読み、終了状態 0 を確かめる。運用の台本はリポジトリの外にある。`merge.sh` は、3 番目の引数に、1 行目が実際の検査結果の `exit=0` である状態ファイルを要求する。失敗の記録を成功に書き換えて渡さない。コミットと `main` への `git merge --no-ff` は [git-workflow](../git-workflow/SKILL.md) に従う。
7. 確かめた結果を Codex に設計書と実装プランへ反映させ、Claude Code が点検して取り込む。依存を足した場合は、実行ファイルの大きさの変化を測り、`docs/plan/results/` に記録させる。
8. 取り込んだ内容で release の実行ファイルを作り直す（`cargo build --release -p benitoiteh`）。

## Codex の起動と再開

- 作業ごとに新しいセッションを、その作業の worktree で起こす。モデルは `gpt-6.1-sol`、推論の度合いは `high`、サンドボックスは `workspace-write` とし、同時に動かす Codex は一つとする。起動の形は `codex exec -m gpt-6.1-sol -c model_reasoning_effort=high -s workspace-write` とする。
- 起動前にオーケストレータがその worktree で `cargo fetch` を実行し、依存を取得する。Codex 内の検査は取得済みの依存を使う。
- Codex は `danger-full-access` で起動しない。Codex 内で走らない OS のサンドボックスと Linux のコンテナのテストは、オーケストレータが外で走らせる。失敗と走らせた環境を `codex exec resume` で返し、同じ作業のセッションを再開する。Codex 内では `scripts/check.sh --inside-codex` を使わせ、保護を外した代替の検査をさせない。
- 背景で Codex を起動するときは、Claude Code の実行ツールの `run_in_background` を使い、返されたシェルの ID と終了の通知で追跡する。
- 通知のない形（`nohup` など）で起動した場合は、起動の直後に `echo $!` で PID を記録し、`while kill -0 <pid>; do sleep 30; done` で待つ。シェルの ID と PID を取り違えない。同じシェルの `pgrep -f <pattern>` は待つ側自身に一致するので使わない。終了後は報告と終了状態を別に確かめる。

## Codex の依頼に含めるもの

依頼は、次の情報を含む自己完結の文にする。個人のパスは依頼の中だけで伝え、公開の文書には写さない。

- 作業の文書と補足の場所、作業ツリーのパス、変更を許す範囲、完了条件。
- 読み取り専用の処理系のリポジトリの場所（既定は作業ツリーの隣の `../Benitoite`。違うときは実際の場所）と、必要な場合は DeepSeek Harness のリポジトリの場所。
- テストに使う処理系の実行ファイルのパス。macOS、aarch64 Linux、x86_64 Linux のどれに使うものかと、対応する `BENITOITEH_TEST_BENITOITE`・`BENITOITEH_TEST_BENITOITE_AARCH64`・`BENITOITEH_TEST_BENITOITE_X86_64` を示す。主の作業ツリーの `work/` を、その作業ツリーの `$PWD/work/` と取り違えさせない。
- 本物の LLM を呼ばない。利用者の普段の設定・状態（`~/.config/benitoiteh`・`~/.local/state/benitoiteh` など）、キーチェーン、トークン、`~/.ssh` を読まない。テスト用の模擬の設定と状態を使う。
- コミットしない。共通の検査は `scripts/check.sh --inside-codex` を使い、OS のサンドボックスとコンテナを要する検査は外へ残す。
- テストを作る・変える場合は、[00-common.md](../../../docs/plan/00-common.md) の「実装の規約の補足」にある環境差。
- 再開の場合は、修正する指摘、失敗した検査の出力、終了状態と実行環境。

## コマンドを制限した作業者への依頼

作業者の許可一覧に合わせて、許したコマンドの形を依頼の文にも列挙する。作業ツリー外への `cd` や、許していないコマンドとの連結など、避ける形も書く。一つのコマンドが拒否されても、その道具全体の禁止を意味しないことを伝える。起動の許可一覧と依頼に載せる一覧は、同じスクリプトで管理して食い違いを防ぐ。

## 評価を走らせるとき

本物の LLM を呼ぶ評価の了承は、`AGENTS.md` と [評価](../../../docs/design/07-quality/07-02-evaluation.md)の「評価の手順」に従う。

- 時間を比べる評価の走行中は、Codex と全体の検査を同時に走らせない。
- 評価のまとまりごとに release を写した同じ実行ファイルを使う。サンドボックスの方針を変えた後は、release を作り直して写し直す。
- DeepSeek の走行は、2026-10-20 の設計者の見立てで高い時間帯とされた約 10:00〜19:00 JST を避ける。この時間帯は未確認の運用上の目安であり、料金の確定情報として扱わない。走行の前に、そのまとまりの見込みの費用を示す。
