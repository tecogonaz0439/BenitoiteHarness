# 共通のリポジトリへの移行の第1段の確かめ（2026-11-05）

BH-080 の第1段として、本リポジトリを共通のリポジトリ `~/src/BenitoiteLang` の下へ移した後に、処理系の側からの依頼（[11-common-repository-request.md](../../2026-11-02/11-common-repository-request.md)）の手順 6 を確かめた記録である。第2段（処理系の取り込み）の手順の根拠に使う。作業はオーケストレータ（Claude Code）が、設計者の指示で行った。

## 移した後の形

- 共通のリポジトリは `~/src/BenitoiteLang` であり、非公開で、リモートを持たない。既定のブランチは `main` とした（設計者の指示）。
- 共通の `AGENTS.md` は、共通のリポジトリの一番上（`~/src/BenitoiteLang/AGENTS.md`）に置いた。索引と、二つのリポジトリにまたがる決まりを書いた。Claude Code に読ませるため、`@AGENTS.md` だけを書いた `CLAUDE.md` を並べて置いた。
- 本リポジトリは `~/src/BenitoiteLang/BenitoiteHarness` にあり、`git worktree repair` の後の主の作業ツリーのパスは新しい場所を指す。共通のリポジトリは、本リポジトリと、本リポジトリの git worktree（`BenitoiteHarness-<作業の ID>`）、第2段で移す処理系を `.gitignore` で除く。
- 処理系は、第2段まで `~/src/Benitoite` に置いたままである。本リポジトリの作業ツリーの隣に処理系がないので、`AGENTS.md`、[作業の進め方](../00-common.md)、[実装作業の共通依頼文](../../workflow/implementation-request.md)、[P0-01](../tasks/P0-01-workspace.md) の「既定は作業ツリーの隣の `../Benitoite`」を、オーケストレータが依頼ごとに場所を示す形に改めた。P5-38〜P5-40 の作業の文書、`tools/skill-eval/SOURCE.md`、`archive-and-bundle` のスキルの出典の `../Benitoite` は、当時の操作の記録なので直していない。

## エージェントが読む指示とスキル

### Codex

Codex CLI 0.166.0 の `codex debug prompt-input` で、モデルに渡す入力を LLM を呼ばずに出力し、三つの場所から起動したときに読む `AGENTS.md` と、スキルの置き場所（skill roots）を比べた。

| 起動した場所 | 読んだ `AGENTS.md` | 本リポジトリの `.agents/skills/` のスキル |
|---|---|---|
| 共通のリポジトリの一番上 | 共通の `AGENTS.md` だけ | 一覧に出ない |
| 本リポジトリの一番上 | 本リポジトリの `AGENTS.md` だけ | 一覧に出る（`git-workflow`、`test-audit`、`archive-and-bundle` など） |
| 本リポジトリの `docs/plan/` | 本リポジトリの `AGENTS.md` だけ | 一覧に出る |

Codex は、起動した場所が属する git のリポジトリの根より上の `AGENTS.md` を読まなかった。本リポジトリは独立した git のリポジトリなので、その中で起動すると共通の `AGENTS.md` を読まない。依頼の文書が「見込み」としていたことを、この出力で確かめた。逆に、共通のリポジトリの一番上から起動すると、子のリポジトリの `AGENTS.md` もスキルも読まない。共通の `AGENTS.md` の索引に従って、エージェントが子の `AGENTS.md` を読みに行く必要がある。

この確かめは、`~/.codex/config.toml` の信頼するプロジェクトに新しいパスを加える前に行った。加えた後も、一番上からの `codex debug prompt-input` は終了状態 0 で終わった。

### Claude Code

Claude Code 2.1.318 を共通のリポジトリの一番上から起動した会話（共通の `CLAUDE.md` と `AGENTS.md` を置く前に始めた）で、次のことを見た。

- 起動のときに文脈へ読み込まれた指示は、起動した場所の記憶（`MEMORY.md`）だけだった。
- 本リポジトリの下のファイルを読んでも、本リポジトリの `AGENTS.md` は自動では読み込まれなかった。本リポジトリには `CLAUDE.md` がない。
- 本リポジトリの `.claude/skills/` のスキル（`git-workflow` など）は、スキルの一覧に出なかった。本リポジトリの下のファイルを読んだ後も出なかった。

このため、一番上から起動した Claude Code には、共通の `AGENTS.md` を `CLAUDE.md` から取り込ませ、子の `AGENTS.md` とスキルは索引に従って読ませる。共通の `CLAUDE.md`（`@AGENTS.md` だけを書いたもの）を置いた後に、同じ会話を Claude のデスクトップのアプリの Code のタブで開き直した。そのとき、`CLAUDE.md` と、そこから取り込んだ共通の `AGENTS.md` が、起動のときに読み込まれた。

## ビルドとテスト

Cargo の生成物は絶対パスを含むので、`cargo clean` の後に `scripts/check.sh` を走らせた（`gamou` の `51de89f`）。x86_64 の Linux は、x86_64 の試験の機械の podman で走らせた（[作業の進め方](../00-common.md)の「検査」）。処理系は、`v0.0.12`（`d0678ddb`）から移す前にビルドした各対象の実行ファイルを、作り直さずに使った。macOS の実行ファイルは `--version=json` に応え、検査の中でどの対象の実行ファイルも動いた。

| macOS | aarch64 の Linux | x86_64 の Linux | 評価の道具の Python のテスト | 終了状態 |
|---|---|---|---|---|
| 821 件が通り、失敗 0、無視 14 | 721 件が通り、失敗 0、無視 13 | 815 件が通り、失敗 0、無視 16 | 121 件が通った | 0 |

件数は、`cargo test` の各テストの実行ファイルの結果の行を足したものである。検査は約 21 分かかった。release の実行ファイルの大きさは、`aarch64-apple-darwin` が 5,938,400 バイト、`aarch64-unknown-linux-musl` が 6,445,648 バイト、`x86_64-unknown-linux-musl` が 7,753,360 バイトだった。aarch64 の Linux で走らせなかった OS のサンドボックスのテスト（コンテナのカーネルが Landlock を持たないもの）は、x86_64 の Linux で走らせた。

評価の道具も、新しい場所から動いた。圧縮の評価の課題 `compact_json_summary` について、P5-36 の 3 条件（`benitoiteh-deepseek-compact-base`・`-reserve`・`-keep`）の dry-run が、どれも終了状態 0 で通った（LLM は呼ばない）。

## 第2段へ引き渡すこと

- 子のリポジトリの中で Codex を起動する使い方（作業ツリーで実装を担わせる形）では、共通の `AGENTS.md` は読まれない。またがる決まりのうち作業に要るものは、依頼文に含める。
- 一番上から起動した Codex と Claude Code は、子のリポジトリのスキルを一覧に出さない。またがるスキルは共通のリポジトリの `.claude/skills/`・`.agents/skills/` に置く。子のスキルを一番上から使う形が要るなら、そのときに置き方を決める。
- Claude Code は `CLAUDE.md` のない場所の `AGENTS.md` を自動では読まず、`CLAUDE.md` から取り込めば読んだ。処理系を移すときも、処理系の `AGENTS.md` を読ませたい場所には `CLAUDE.md` からの取り込みが要るかを確かめる。
