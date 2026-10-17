# P3-01 プロジェクトの指示のファイル

## 目的

通常モードで、利用者とプロジェクトの指示のファイル（`AGENTS.md`、ないときの `CLAUDE.md`）を読み、環境の情報に入れる（設計書 01-04 の「プロジェクトの指示のファイル」）。BH-013 は 2026-10-17 に閉じた（ディレクトリごとに `AGENTS.md` を読み、ないときだけ `CLAUDE.md` を読む）。

## 読む設計書

- [文脈の管理](../../design/01-agent/01-04-context.md)の「環境の情報」「プロジェクトの指示のファイル」「送る内容の表示」
- [道具](../../design/01-agent/01-03-tools.md)の「道具の定義の共通の規則」「読まないファイル」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「プロジェクトの指示のファイル」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「データの置き場所」（利用者の指示のファイルの場所）
- 既存のもの: `src/context/`（`environment.rs` の `Environment`、`escape`、`fields`、`changes_since`）、`src/safe_fs/`（`ScopeRoot::open`・`metadata`・`open_read`、`resolved_path`）、`src/config/paths.rs`（`instructions_file`、`home`）、`src/tools/exclude.rs`（`ExcludeRules::reason`）、`src/cli/exec.rs`（`Environment::capture` と `ToolContext::new` の呼び出し）、`src/agent/events.rs`・`src/agent/mod.rs`（`SessionStart`）、`src/session/record.rs`（`RecordedMode` などの記録の型）、`src/ui/`（`ExecOutput::diagnostic`、`terminal_text`）

## 作るもの

### 1. 指示のファイルを集める関数

`src/context/instructions.rs`（名前は任せる）に作る。入力は、モード、プロジェクトのディレクトリの実のパス（`ScopeRoot::real_path()` の値）、利用者の指示のファイルのパス、ホームディレクトリの実のパス、読まないファイルの規則（`&ExcludeRules`）とする。結果は、読んだファイルの並び（探したときのパス、中身の文字列、切り詰めた量）と、除いたファイルの並び（パスと理由）とする。

- 制限モードでは何も読まない（BH-042 が決まるまで）。

**利用者の指示のファイル**（`instructions_file`。`AGENTS.md` だけを見る）

- 利用者は信頼するので、シンボリックリンクを辿って開いてよい（設定のファイルを別の場所で管理する利用者がいる）。読まないファイルの規則は掛けない（この規則は `benitoiteh` の設定のディレクトリを除くので、掛けると必ず除かれる）。
- `rustix::fs::open(path, OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC, Mode::empty())` で開き、`fstat` で通常のファイルであることを確かめる。ないときは何もしない。ほかの誤りは、除いたファイルとして理由を残す。

**プロジェクトの側の指示のファイル**

- 探す範囲の根を決める。プロジェクトのディレクトリから上へ辿り、`.git`（ディレクトリかファイル。リンクを辿らずに調べる）のあるディレクトリを根とする。見つからないとき、または根がホームディレクトリの実のパスと同じかその上のときは、プロジェクトのディレクトリだけを範囲とする（設計書 01-04 の 2）。
- 根を `ScopeRoot::open` で開く。開けなければ、何も読まずに、除いた理由を残す（起動は止めない）。
- 根からプロジェクトのディレクトリまでの各ディレクトリについて、根の側から順に、`root.metadata(rel)`（リンクを辿らない）で `AGENTS.md` の有無を調べる。あれば `AGENTS.md` を、なければ `CLAUDE.md` を候補とする。`AGENTS.md` があるが後の規則で除いたときは、同じディレクトリの `CLAUDE.md` を代わりに読まない。
- 候補は `root.open_read(rel)` で開く（範囲の根から相対に、範囲の外へ出ない形で、待たずに開く）。開けなければ（範囲の外へ出るリンク、途中のリンク、権限など）、除いた理由を残す。Linux の `openat2` の `RESOLVE_BENEATH` は、範囲の中を指す絶対パスのシンボリックリンクも拒むので、テストは相対のリンクで書く。
- 開いたものについて、`safe_fs::resolved_path(&file)` の実のパスに `ExcludeRules::reason` を掛け、`file.metadata()` で通常のファイルであることと、ハードリンクの数が 1 であることを確かめる。同じ装置と i ノードのファイルは、二度目から除く（`CLAUDE.md` が `AGENTS.md` へのリンクのときなど）。
- 結果と画面に示すパスは、探したときのパス（根のパスに `rel` を足したもの）とする。

**大きさと文字**

- 上限は、逃がす前のバイトの数で数える。一つのファイルは 32 KiB、合計は 64 KiB とする。逃がすと最大で 5 倍に伸びうるが、それは受け入れる。
- 読むのは「上限＋4 バイト」までとする（`Read::take`）。読んだものが UTF-8 として正しくなく、`Utf8Error::error_len()` が `None`（終わりで文字が切れた）なら、`valid_up_to()` で切る。ほかの誤りなら、UTF-8 でないものとして除く。
- 切り詰めたら、中身の後の別の行に、除いた量を示す印（英語。`text` の関数。例: `[truncated: N bytes omitted]`）を置く。`N` は `fstat` の大きさから入れたバイトの数を引いた値とする。印は上限に数えない。
- 合計の上限に達した後のファイルは、中身を入れずに、除いたファイルとして理由を残す。

### 2. 環境の情報

- `Environment` に、読んだ指示のファイルの欄を加える。`Environment::capture` の形は変えず、`with_project_instructions(self, …) -> Self` のような関数で足す（`tests/agent_loop.rs` などの既存の呼び出しを壊さないため）。読んだファイルの並びを返す関数も置き、`SessionStart` を作る側が使う。
- `<environment>` の中の今の欄の後に、ファイルごとに `<project_instructions path="...">...</project_instructions>` を置く。中身は今の `escape` で逃がす。`path` の属性の値は、`escape` に加えて `"` を `&quot;` にする別の関数で逃がす（今の `escape` は変えない。`writable_roots` の欄の JSON の `"` が変わるため）。
- `fields` を固定の長さの配列から `Vec` に改め、`changes_since` の比べ方は今と同じ（名前と値）とする。指示のファイルの欄の値にはパスを含める。

### 3. 表示と記録

- `SessionStart` のイベントとセッションの記録に、読んだファイルのパスの並びと、除いたファイルのパスと理由の並びを加える。記録の型は `session` に置き（`RecordedMode` と同じく、`session` が `context` に依らないように）、新しい欄には `#[serde(default)]` を付ける（前の記録を読めるように）。空の並びは書かない（`skip_serializing_if`）。記録の形式の版は 1 のままとする。
- `exec` の画面の出力（`ui`）では、読んだファイルと除いたファイルを、一件ずつ `ExecOutput::diagnostic` の一行で示す（`terminal_text` を通る）。文は `ui/text.rs` に置く。

### 4. 起動の入口

- `cli/exec.rs` で、通常モードのとき、`ToolContext::new` に `exclude` と `root` を渡す前に 1 を呼ぶ（または `exclude.clone()` を使う）。探し始めるディレクトリは `root.real_path()` とする。

## 触るファイル

- `crates/benitoiteh/src/context/`（新しいファイルを含む）
- `crates/benitoiteh/src/agent/events.rs`、`src/agent/mod.rs`
- `crates/benitoiteh/src/session/`（記録の型）
- `crates/benitoiteh/src/ui/`
- `crates/benitoiteh/src/cli/exec.rs`
- `crates/benitoiteh/tests/context.rs`、`tests/exec.rs`、要れば `tests/agent_loop.rs`

`tests/expected/` は、システムプロンプトと道具の定義だけを持ち、環境の情報を含まないので、作り直さない。

## テスト

木は `std::env::temp_dir()` の下（どの git のリポジトリの外）に作る。シンボリックリンクは相対のパスで作る。

- リポジトリの根と、その下のプロジェクトのディレクトリの両方に `AGENTS.md` があるとき、根の側から順に入る。根の外の親のディレクトリの `AGENTS.md` は入らない。`.git` がファイルのとき（worktree）も根として扱う。
- リポジトリの外では、プロジェクトのディレクトリだけを見る。
- 根がホームディレクトリと同じとき（テストでは、ホームディレクトリの値に、`.git` を置いた一時のディレクトリを渡す）は、プロジェクトのディレクトリだけを見る。
- `AGENTS.md` がなく `CLAUDE.md` があるディレクトリでは `CLAUDE.md` が入り、両方あるディレクトリでは `AGENTS.md` だけが入る。`CLAUDE.md` が `AGENTS.md` へのシンボリックリンクのときも、下のディレクトリの `AGENTS.md` が上の `AGENTS.md` へのリンクのときも、一度だけ入る。
- 利用者の指示のファイルが最初に入る。シンボリックリンクでも入る。利用者の設定のディレクトリの `CLAUDE.md` は入らない。
- 探す範囲の外を指すシンボリックリンクの `AGENTS.md`、読まないファイルを指すもの（`.env` など）、ハードリンクの数が 2 のもの、名前付きのパイプ、ディレクトリは入らず、除いた理由が残る。名前付きのパイプで止まらない。
- 32 KiB を超えるファイルが切り詰められ、印が付き、`N` が正しい。合計 64 KiB を超えた後のファイルは中身が入らない。多バイトの文字の途中で切れない。UTF-8 でないファイルは入らない。
- 制限モードでは何も読まない。
- `<project_instructions>` の中身の `<` と `&`、パスの `"` が逃がされ、偽の `</environment>` や属性を作れない。
- `exec` の `--json` の出力の `session_start` に、読んだファイルのパスが現れる。前の版の形の `session.jsonl`（新しい欄のないもの）を、今の読み込みで読める。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、作った関数と型のシグネチャを書く。
