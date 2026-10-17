# P3-01 プロジェクトの指示のファイル

## 目的

通常モードで、利用者とプロジェクトの指示のファイル（`AGENTS.md`、ないときの `CLAUDE.md`）を読み、環境の情報に入れる（設計書 01-04 の「プロジェクトの指示のファイル」）。BH-013 は 2026-10-17 に閉じた（ディレクトリごとに `AGENTS.md` を読み、ないときだけ `CLAUDE.md` を読む）。

## 読む設計書

- [文脈の管理](../../design/01-agent/01-04-context.md)の「環境の情報」「プロジェクトの指示のファイル」「送る内容の表示」
- [道具](../../design/01-agent/01-03-tools.md)の「読まないファイル」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「プロジェクトの指示のファイル」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「データの置き場所」（利用者の指示のファイルの場所）
- 既存のもの: `src/context/`（`environment.rs` の `Environment`）、`src/config/paths.rs`（`instructions_file`）、`src/tools/exclude.rs`（`ExcludeRules::reason`）、`src/cli/exec.rs`（`Environment::capture` の呼び出し）、`src/agent/events.rs`（`SessionStart`）、`src/session/record.rs`、`src/ui/`

## 作るもの

1. `context` に、指示のファイルを集める関数を作る（ファイルは `src/context/instructions.rs` など。名前は任せる）。入力は、モード、プロジェクトのディレクトリ、利用者の指示のファイルのパス、読まないファイルの規則（`ExcludeRules`）とする。
   - 制限モードでは何も読まない（BH-042 が決まるまで）。
   - 利用者の指示のファイル（`instructions_file`）は `AGENTS.md` だけを見る。あれば最初に置く。
   - プロジェクトのディレクトリから上へ辿り、`.git`（ディレクトリかファイル）のあるディレクトリで止める。見つからなければ、プロジェクトのディレクトリだけを見る。各ディレクトリで、`AGENTS.md` があればそれを、なければ `CLAUDE.md` を読む。一つのディレクトリからは一つだけ読む。根の側から順に並べる。
   - 「ある」とは、名前のものがリンクを辿らずに存在することとする。`AGENTS.md` があるが読めない（後述の規則で除いた）ときは、同じディレクトリの `CLAUDE.md` を代わりに読まない。理由: 代わりに読むと、除いたことが利用者に分かりにくく、どちらを読んだかが置き方で変わる。
   - プロジェクトの側のファイルは、設計書 01-04 の規則のとおり、実のパスが探索の根（リポジトリの根か、プロジェクトのディレクトリ）の中にあり、`ExcludeRules::reason` が理由を返さず、`O_NONBLOCK` で開いた後の `fstat` で通常のファイルであるときだけ読む。除いたものは、パスと理由（英語。`text` に置く）を結果に残す。
   - 大きさの上限は、一つのファイル 32 KiB、合計 64 KiB とする。超えた分は除き、除いたことを示す一行（英語。`text` に置く。例: `[truncated: N bytes omitted]`）を、その文の末尾に加える。UTF-8 の文字の途中で切らない。合計の上限に達した後のファイルは、中身を入れずに、除いたことだけを残す。UTF-8 でないファイルは除き、理由を残す。
2. `Environment` に、読んだ指示のファイルの欄を加え、`<project_instructions path="...">...</project_instructions>` の形で `<environment>` の中に入れる。`path` は絶対パスとし、属性の値と中身は今の `escape` と同じ規則で逃がす。入れる位置は、今の欄の後とする。差分（`changes_since`）の扱いは、今の欄と同じく、変わったときだけ出す形にする（セッションの途中で読み直さないので、ふつうは最初だけ出る）。
3. 読んだファイルのパスと、除いたファイルのパスと理由を、`SessionStart` のイベントとセッションの記録に加え、`exec` の画面の出力（`ui`）にも示す。イベントと記録の欄の名前は任せる（例: `instruction_files`・`skipped_instruction_files`）。記録の形式の版を変えるかは、既存の記録の読み込み（`replay`）が新しい欄を無視できるかで判断し、報告に書く。
4. `cli/exec.rs` で、通常モードのときに 1 を呼び、2 と 3 に渡す。

## 触るファイル

- `crates/benitoiteh/src/context/`（新しいファイルを含む）
- `crates/benitoiteh/src/agent/events.rs`、`src/agent/mod.rs`（`SessionStart` を作る箇所）
- `crates/benitoiteh/src/session/record.rs`（記録の欄）
- `crates/benitoiteh/src/ui/`
- `crates/benitoiteh/src/cli/exec.rs`
- `crates/benitoiteh/tests/context.rs` と `tests/expected/`（期待の出力を変えるとき）、`tests/` の `exec` のテスト

## テスト

ユニットテストか `tests/context.rs` で、一時ディレクトリに木を作って確かめる。

- リポジトリの根と、その下のプロジェクトのディレクトリの両方に `AGENTS.md` があるとき、根の側から順に入る。根の外の親のディレクトリの `AGENTS.md` は入らない。
- リポジトリの外では、プロジェクトのディレクトリだけを見る。
- `AGENTS.md` がなく `CLAUDE.md` があるディレクトリでは `CLAUDE.md` が入り、両方あるディレクトリでは `AGENTS.md` だけが入る。`CLAUDE.md` が `AGENTS.md` へのシンボリックリンクのときも一度だけ入る。
- 利用者の指示のファイルが最初に入る。利用者の設定のディレクトリの `CLAUDE.md` は入らない。
- 探索の根の外を指すシンボリックリンクの `AGENTS.md`、読まないファイルを指すもの（`.env` など）、名前付きのパイプ、ディレクトリは入らず、除いた理由が残る。名前付きのパイプで止まらない。
- 32 KiB を超えるファイルが切り詰められ、印が付く。合計 64 KiB を超えた後のファイルは中身が入らない。多バイトの文字の途中で切れない。UTF-8 でないファイルは入らない。
- 制限モードでは何も読まない。
- `<project_instructions>` の中身の `<` と `&` が逃がされ、偽の `</environment>` を作れない。
- `exec` の `--json` の出力の `session_start` に、読んだファイルのパスが現れる。

## 確かめること

- 記録の形式の版を変えたか、変えなかった理由。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、作った関数と型のシグネチャを書く。
