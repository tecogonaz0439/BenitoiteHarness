# P4-09 `glob`・`grep`・`read` を、リンクを辿らず上限のある読み方に移す

## 目的

`glob` と `grep` が使う `ignore::WalkBuilder` は、各ディレクトリの `.gitignore` と `.ignore` を、シンボリックリンクを辿り、待たない形を使わずに開き、行の長さに上限なく読む。サンドボックスの中のコマンドが `.gitignore` を `/dev/zero` へのリンクや名前付きのパイプにすると、次の `glob`・`grep` で主のプロセスが止まるか、メモリを使い尽くす（P4-03 の点検で見つけた）。また、`grep` と `read` は、ファイルの中身を上限なく読む（疎な巨大なファイルでメモリを使い尽くす）。これらを、P4-03 の辿り方の部品と、上限のある読み方に移す（設計書 01-03 の 2026-10-17 の【方針】）。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「read」「glob」「grep」（2026-10-17 に、辿り方、`.gitignore` の扱い、上限、64 MiB のファイルの扱いを加えた）「道具の定義の共通の規則」「読まないファイル」「出力の切り詰め」
- [セッション](../../design/01-agent/01-05-sessions.md)の「巻き戻し」の「辿り方」
- 既存のもの: `src/tools/search.rs`（`WalkBuilder` の設定 `hidden(false)`・`follow_links(false)`・`parents(false)`・`sort_by_file_path`、`filter_entry` の `canonicalize` による絞り込み、glob の照合の基準と表示の形、grep の `read_to_end`）、`src/tools/paths.rs`（`search_path`・`SearchRoot`・`check_file`）、`src/tools/read.rs`、`src/tools/exclude.rs`（`ExcludeRules::descend`・`reason`、spill の例外）、`src/rewind/capture.rs`（`load_gitignore`、gitignore の判定、一要素ずつ開く辿り方）、`src/rewind/mod.rs`（gitignore の組み立て）、`src/safe_fs/`（`no_follow` の口、`open_read`）、`tests/tools_read.rs`、`tests/safe_fs.rs`（`mkfifo` の作り方）

## この作業が認める型の変更

- `.gitignore` を読む関数（`load_gitignore`）と、gitignore に当たるかの判定を、`rewind` から共有の場所（例: `src/safe_fs/gitignore.rs`）に移し、`rewind` もそれを使う（rewind のテストを変えずに通す）。辿る処理そのものは共有しない（下の 1 の順序と、rewind の一覧に結び付いた処理が違うため）。
- `ToolContext` に、検索の上限を差し替える口（例: `with_search_limits(SearchLimits { max_entries, max_duration, max_file_bytes })`）を加える（テストで小さくする）。
- `glob`・`grep` に `Invocation.cancelled` を渡す。

## 作るもの

### 1. 辿り方（`glob`・`grep`）

- 起点: `path`（既定はプロジェクトのディレクトリ）を、今と同じくプロジェクトの `ScopeRoot` から一度だけ解いて開く（`search_path` の扱い。範囲の中のリンクは辿る。通常モードの範囲の外の `SearchRoot::External` も今のまま）。起点がファイルなら、そのファイルだけを探し、`.gitignore` の判定にかけない。
- 起点より下は、`safe_fs` の `no_follow` の口で一要素ずつ開いて辿る。シンボリックリンクは一覧に出さず、その中を辿らない（今と同じ）。`ignore::WalkBuilder` と `filter_entry` の `canonicalize` による絞り込みを使わない。
- 順序: 親を子より先に出す深さ優先とし、兄弟は名前のバイト列の順に並べる（今の `sort_by_file_path` と同じ出力の順。grep の 100 件の打ち切りで出す行が変わらないように）。再帰ではなく明示のスタックで書く。開いたディレクトリの記述子は、そのディレクトリの項目を読み終えたら閉じる（深い木で記述子を使い尽くさないように。兄弟が残っている階層の記述子は持たずに、名前の一覧を持つ）。
- `.gitignore`: 起点がプロジェクトの下のディレクトリなら、プロジェクトの根から起点までの各ディレクトリの `.gitignore` も当てる（プロジェクトより上は読まない）。起点より下は各ディレクトリの `.gitignore` を当てる。読み方は共有の `load_gitignore`（待たない形で開き、通常のファイルで 1 MiB 以下のものだけを読む）とし、不正な行は飛ばして残りの行を使い、先頭の BOM を除く（ファイル全体を捨てない。rewind の扱いと揃える）。読めない `.gitignore`（リンク、名前付きのパイプ、1 MiB を超えるもの、UTF-8 でないもの）はないものとして扱い、読まなかったパスを結果の末尾に一行で示す。`.ignore`、`.git/info/exclude`、大域の gitignore は読まない。
- 読まないファイルの規則: ディレクトリは `ExcludeRules::descend`（spill の例外を含む）で、ファイルは `ExcludeRules::reason` で、実のパスで判定する（今と同じ）。`grep` が開いた記述子では、今と同じく `resolved_path` でも判定する。rewind の除外（守る場所の扱い、大きさ、種類）は流用しない（`.benitoiteh/` は今と同じく読まないファイルの規則に従う）。
- 隠しファイルを含める（今の `hidden(false)` と同じ）。
- 上限: 辿る項目は 20 万まで、時間は 10 秒まで（`grep` がファイルを読む時間を含む）。超えたら、そこまでの結果を返し、打ち切ったことを末尾に一行で示す（`glob` の `Showing 500 of N` の N は「少なくとも N」と示す）。取り消しの印も見る。読めないディレクトリは飛ばして続け、飛ばしたことを末尾に一行で示す。
- `glob` の更新の時刻は、ファイルを開かずに、持っているディレクトリの記述子から `fstatat`（リンクを辿らない）で取る。
- 英文（打ち切り、読まなかった `.gitignore`、飛ばしたディレクトリ、飛ばした大きなファイル）は `tools/text.rs` に置く。

### 2. 読み方（`grep`・`read`）

- `grep`: 各ファイルを一要素だけリンクを辿らずに開き、`fstat` で通常のファイルであることを確かめる。64 MiB を超えるファイルは読まずに飛ばし、末尾に示す。先に 8 KiB を読んでバイナリかを判定し、残りを塊ごとに読みながら行を照合する（ファイル全体をメモリに読まない。一行が長いときは、照合に要る範囲だけを保つ）。塊ごとに時間の上限と取り消しを見る。
- `read`: 64 MiB を超えるファイルは読まず、`grep` で探すよう示す誤りを返す（英文は `tools/text.rs`）。読むときは、塊ごとに読み、返す範囲（`offset` から 2,000 行か 50 KiB）の行と、全体の行の数を数え、メモリに保つのは返す範囲だけにする。
- 名前付きのパイプは、今と同じく待たない形で開き、通常のファイルでないものとして拒む。

### 3. 変わる点の一覧

完了の報告に、今の挙動と変わった点を一覧で書く。少なくとも次を照合する。

- 保つ点: 出力の順、隠しファイルを含めること、読まないファイルの規則、表示の形と `glob` の照合の基準、起点のファイルを `.gitignore` の判定にかけないこと、リンクを一覧に出さずその中を辿らないこと、件数と大きさの上限。
- 変わってよい点: `.ignore` を読まないこと。入れ子の git のリポジトリの中に親の `.gitignore` が当たること（今の `ignore` は `.git` を持つディレクトリより上の規則を当てない）。下のディレクトリを起点にしたときに、プロジェクトの根から起点までの `.gitignore` が当たること。BOM と不正な行の扱い。読めないディレクトリを飛ばして続けること（今は道具全体が誤りになる）。打ち切りと 64 MiB の扱い。

### 4. 実行ファイルの大きさ

`ignore::WalkBuilder` を使わなくなった後の実行ファイルの大きさを、取り込む前の `main` と比べて報告に書く（`ignore` のクレートは、`gitignore` の部品のために残る）。

## 触るファイル

- `crates/benitoiteh/src/tools/`（`search.rs`、`read.rs`、`paths.rs`、`mod.rs`（上限の口と取り消しの受け渡し）、`text.rs`）
- `crates/benitoiteh/src/safe_fs/`（共有の `gitignore` の部品）
- `crates/benitoiteh/src/rewind/`（`capture.rs`・`mod.rs` を共有の部品に寄せる）
- `crates/benitoiteh/tests/`（`tests/tools_read.rs` ほか）

## テスト

上限は `SearchLimits` で小さくする。止まらないことは、見張りのスレッドで時間（例: 5 秒）を区切り、その中で成功して普通のファイルを返すことで確かめる。名前付きのパイプは、`tests/safe_fs.rs` と同じく macOS では `mkfifo` の命令で作る。

- 安全の性質（07-01）:
  - 下の階層の `.gitignore` を `/dev/zero` へのリンクにしたとき、名前付きのパイプにしたとき、10 MiB の一行のファイルにしたときに、`glob` と `grep` が止まらず、そのディレクトリのファイルを返し、読まなかったことを示す。
  - `grep` と `read` が、名前付きのパイプ、`/dev/zero` へのリンク、疎な巨大なファイル（`set_len` で 64 GiB）で止まらず、メモリを使い尽くさない（`read` は誤り、`grep` は飛ばしたことを示す）。
  - 数百階層の深い木で、異常終了せず、記述子を使い尽くさない。
- 順序: ディレクトリと兄弟のファイルの出力の順が今と同じ（`a/`、`a/x`、`b.txt`）。
- 起点の形: ファイル、下のディレクトリ、プロジェクトの中のリンク（表示が実のパスになる）。下のディレクトリを起点にしたとき、プロジェクトの根の `.gitignore` が当たる。
- 否定の規則（`!keep`）、不正な行を含む `.gitignore`、BOM 付きの `.gitignore`。
- 上限: 項目の数と時間の上限で打ち切られ、そこまでの結果と打ち切りの一行が返る。取り消しで止まる。
- 既存の `glob`・`grep`・`read` のテストがそのまま通る（プロジェクトの外を指すリンクの中を辿らないことは、既存の `tools_read.rs` のテストが確かめているので、重ねて書かない）。
- rewind のテストがそのまま通る。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、3 の変わる点の一覧、足した英文の全文、実行ファイルの大きさの変化を書く。
