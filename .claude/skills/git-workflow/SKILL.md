---
name: git-workflow
description: このリポジトリで git のコミットを作成するときの規約。コミットメッセージの書式と、ブランチと push の扱いを定める。git commit や git push を実行する前に使用する。
---

# Git の運用規約

## ブランチ

- 既定のブランチは `gamou` とする。公開のリポジトリ（GitHub の `tecogonaz0439/BenitoiteHarness`）に push するのは `gamou` と、リリースのタグだけである。
- 作業は作業用のブランチ（例: `design/initial`、`impl/P1-provider`）で行い、`gamou` に `git merge --no-ff` で取り込む。
- push は設計者の指示を得てから行う。

## コミットメッセージ

`gamou` の履歴に入るコミットは、次の形で書く。作業用のブランチで作り、`gamou` に取り込むコミットも同じ形で書く。

1. 1 行目に、変更の要約を英語だけで書く。
2. 空行の後に、変更内容を英語の箇条書き（`- `）で列挙する。
3. 空行の後に、1 行目の日本語の訳を書く。
4. 空行の後に、変更内容を日本語の箇条書き（`- `）で列挙する。英語の箇条書きと同じ内容にする。
5. 空行の後に、`Co-Authored-By` などの行を置く。

例:

```
Add the design document of the harness prototype

- Add the chapters of the design document under docs/design/
- Add the list of open issues

試作のハーネスの設計書を追加

- docs/design/ の下に設計書の章を追加
- 未決事項の一覧を追加
```

取り込みのコミット（`git merge --no-ff`）の 1 行目は、git が作る `Merge ...` のままでよい。

## コミットしないもの

- 秘密の情報（API キー、OAuth のトークン、個人の設定ファイル）。
- LLM とのやり取りの生の記録（評価の生の記録は `work/` かリポジトリの外に置く）。
- `reference/` と `work/` の中身（`.gitignore` で除いている）。

## 出典

このスキルは、Benitoite の `git-workflow` スキル（<https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/.claude/skills/git-workflow/SKILL.md>、リリース `v0.0.5`）を、このリポジトリのブランチの名前と公開の扱いに合わせて改めたものである。
