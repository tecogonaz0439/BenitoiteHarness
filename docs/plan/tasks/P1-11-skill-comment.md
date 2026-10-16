# P1-11 埋め込んだ SKILL.md の先頭の注釈を除く

## 目的

制限モードのシステムプロンプトに入れる `SKILL.md` の本文から、本文の先頭の HTML の注釈を除く（2026-10-18 の設計者の決定）。この注釈は、処理系の側のテストの仕組みの説明（878 バイト）であり、LLM の作業に要らない。

## 読む設計書

- [文脈の管理](../../design/01-agent/01-04-context.md)の「Benitoite の Skill の置き方」
- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「取り込む文書と版」
- P1-08 で作った `crates/benitoiteh/src/context/`（`skill_parts` など）と、`tests/context.rs`、`tests/expected/`

## 作るもの

1. `context` で、`SKILL.md` の本文を取り出す処理に、先頭の HTML の注釈を除く処理を加える。
   - 対象: 先頭の属性の部分の後の本文が、空行を除いて `<!--` で始まるときの、その `<!--` から最初の `-->` までと、その直後の改行。
   - 本文の先頭が注釈でないときは何も除かない。本文の途中の注釈は除かない。
   - `<!--` で始まるのに `-->` がないときは、Skill の形の誤り（今の `InvalidSkill` に当たるもの）とする。
2. 通常モードの `benitoite_reference` の `SKILL`（P2 で作る）が同じ本文を返せるように、注釈を除いた本文を得る関数を一つにまとめる。P2 の道具そのものは作らない。
3. `assets/benitoite/v0.0.6/skill/SKILL.md` は変えない（04-01 の「取り込む文書と版」）。
4. 期待の出力（`tests/expected/system-restricted.txt` など）を、`examples/update_context_expected.rs` で作り直す。作り直したら、差分が注釈を除いた分だけであることを確かめる。

## 触るファイル

- `crates/benitoiteh/src/context/`
- `crates/benitoiteh/tests/context.rs`
- `crates/benitoiteh/tests/expected/`

## テスト

- 制限モードのシステムプロンプトに `<!--` と、注釈の中の文（`How the code examples in this file are checked`）が含まれないこと。注釈の直後の `# Benitoite` の見出しから後が、そのまま含まれること。
- 注釈を除く関数の境界: 先頭が注釈でない本文、本文の途中の注釈、`-->` のない注釈（誤り）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、システムプロンプトの大きさの変化（バイト数）と、ハッシュ（`system_prompt_sha256`）が変わったことを書く。
