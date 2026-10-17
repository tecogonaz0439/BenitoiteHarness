# P4-09 `glob`・`grep` の辿り方を、リンクを辿らない口に移す

## 目的

`glob` と `grep` が使う `ignore::WalkBuilder` は、各ディレクトリの `.gitignore` と `.ignore` を、シンボリックリンクを辿り、待たない形を使わずに開き、行の長さに上限なく読む。サンドボックスの中のコマンドが `.gitignore` を `/dev/zero` へのリンクや名前付きのパイプにすると、次の `glob`・`grep` で主のプロセスが止まるか、メモリを使い尽くす（P4-03 の点検で見つけた）。P4-03 で作った、リンクを一切辿らない辿り方の口に、`glob` と `grep` を移す。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「glob」「grep」「道具の定義の共通の規則」「読まないファイル」
- [セッション](../../design/01-agent/01-05-sessions.md)の「巻き戻し」の「辿り方」（`.gitignore` の読み方と上限）
- 既存のもの: `src/tools/search.rs`（`WalkBuilder` の設定、`canonicalize` による絞り込み）、P4-03 の辿り方の口と `.gitignore` の読み方（`src/rewind/` か `src/safe_fs/`）

## 作るもの

- `glob` と `grep` の辿り方を、P4-03 の口（プロジェクトのディレクトリの記述子から一要素ずつ、リンクを辿らずに開く。`.gitignore` は待たない形で、通常のファイルで 1 MiB 以下のものだけを読み、`GitignoreBuilder::add_line` で組み立てる）に移す。`ignore::WalkBuilder` と `canonicalize` による絞り込みを使わない。`.ignore` は使わない。
- 今の `glob`・`grep` の結果（順序、件数の上限、除く範囲、守る場所の扱い、`hidden` の扱い）を変えない。変わる点（`.ignore` を読まなくなること、リンクの先を一覧に出さないことなど）があれば、完了の報告に一覧で書く。
- 辿る項目の上限と時間の上限は、今の `glob`・`grep` の上限に合わせる（なければ P4-03 の `CheckpointLimits` と同じ値）。

## 触るファイル

- `crates/benitoiteh/src/tools/search.rs`
- P4-03 の辿り方の口（共有のために置き場所を移すなら）
- `crates/benitoiteh/tests/`（`glob`・`grep` のテスト）

## テスト

- 安全の性質（07-01）: `.gitignore` を `/dev/zero` へのリンクにしたとき、名前付きのパイプにしたとき、10 MiB の一行のファイルにしたときに、`glob` と `grep` が止まらず、メモリを使い尽くさない（見張りのスレッドで時間を区切る）。
- プロジェクトの外を指すディレクトリのリンクの中を辿らない。
- 既存の `glob`・`grep` のテストがそのまま通る。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、結果が変わった点の一覧を書く。
