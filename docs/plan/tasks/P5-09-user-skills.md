# P5-09 利用者の Agent Skill を読む

## 目的

設計書 01-04「利用者の Agent Skill」（2026-10-19 に設計者が決めた）を実装する。`benitoiteh` は、両方のモードで、利用者の `~/.agents/skills/<名前>/SKILL.md` とプロジェクトの `.agents/skills/<名前>/SKILL.md` を読み、名前と説明を環境の情報に並べ、Skill のディレクトリの中を読めるようにする。制限モードでは、Skill のディレクトリの `.bnt` を検査し実行できるようにする。

この作業で本物の LLM の提供者を呼ばない。

## 読む設計書

- [文脈の管理](../../design/01-agent/01-04-context.md)の「利用者の Agent Skill」「環境の情報」「プロジェクトの指示のファイル」「Benitoite の Skill の置き方」
- [道具](../../design/01-agent/01-03-tools.md)の「read」（Skill のディレクトリを読める追記）、「glob」「grep」、`benitoite_check`・`benitoite_run`・`benitoite_test`
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「プロジェクトの指示のファイル」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」（`[skills]` の方針）
- [評価](../../design/07-quality/07-02-evaluation.md)の「評価の手順」（`skills.user = false`）
- 既存のもの:
  - 指示のファイル: `src/context/instructions.rs`（`collect`、`search_root`、`Collector::read`）、`src/context/instructions/text.rs`
  - 環境の情報: `src/context/environment.rs`（`fields`、`render`、`changes_since`、`changes_since_rendered`、`render_fields` の `project_instructions` の特別な扱い）
  - システムプロンプト: `src/context/mod.rs`、`prompts/mode-restricted.md`、`tests/expected/`、`examples/update_context_expected.rs`、`tests/context.rs`
  - 道具の範囲: `src/tools/mod.rs`（`ToolContext`）、`src/tools/paths.rs`（`open_file`、`search_path`、`DiagnosticSources::resolve`）、`src/tools/search.rs`、`src/tools/text.rs`（`OUTSIDE_SCOPE`）、`src/tools/exclude.rs`
  - Benitoite の道具: `src/tools/benitoite_check.rs`・`benitoite_run.rs`・`benitoite_test.rs`・`benitoite_process.rs`
  - Benitoite の Skill の先頭の読み取り: `src/benitoite/skill.rs` の `skill_parts`
  - 設定: `src/config/mod.rs`（`UserConfig`、P5-06 の `UserReadConfig` の形）
  - 記録と表示: `src/session/record.rs`（`Header`、`Record::SessionStart`）、`src/agent/events.rs`（`AgentEvent::SessionStart`）、`src/agent/mod.rs`（ヘッダと SessionStart を書く所）、`src/agent/status.rs`、`src/ui/text.rs`、`src/ui/mod.rs`、`src/ui/tui/mod.rs`、`src/ui/tui/text.rs`
  - 呼び出し: `src/cli/exec.rs`（指示のファイルの `collect` と `ToolContext` を作る所）
  - 評価の道具: `tools/skill-eval/skill_eval.py` の `benitoiteh_config`、`test_skill_eval.py`

## この作業が認める型の変更

- 利用者の設定に `[skills]` の表 `UserSkillsConfig { user: bool, project: bool }`（`deny_unknown_fields`、`Default` は手で書き、どちらも `true`）を加え、`ResolvedExec` に同じ二つの値を渡す。プロジェクトの設定ファイル（`ProjectConfig`）には加えない。
- Skill を探して読む新しいモジュール（例 `src/context/skills.rs`）と、その結果の型（例 `UserSkills { loaded: Vec<LoadedSkill>, skipped: Vec<SkippedSkill> }`。`LoadedSkill` は `name`・`description`・`skill_md`（`SKILL.md` の絶対の実のパス）・`directory`（Skill のディレクトリの絶対の実のパス）・`source`（`user` か `project`）・`sha256`）。
- `ToolContext` に、読める Skill のディレクトリの根の一覧（`ScopeRoot` の列）を持たせる欄と、組み立ての関数（例 `with_skill_roots`）。
- `Environment` に Skill の一覧を持たせる欄と組み立ての関数。
- `Header` に `#[serde(default, skip_serializing_if = "Vec::is_empty")] skills: Vec<RecordedSkill>`（`name`・`path`・`sha256`・`source`）。`FORMAT_VERSION` は 1 のまま。
- `AgentEvent::SessionStart` と `Record::SessionStart` に、読んだ Skill と読まなかった Skill の欄（指示のファイルの欄と同じ形）。

## 作るもの

### 1. Skill を探して読む

- `[skills] user` が真なら、`~/.agents/skills` の直下の各ディレクトリの `SKILL.md` を見る（ホームは指示のファイルと同じく `canonicalize` したもの）。
- `[skills] project` が真なら、指示のファイルと同じ探す範囲（`search_root` の結果。git の根まで、根がホームかそれより上ならプロジェクトだけ、git の外ならプロジェクトだけ）の各ディレクトリについて、根の側から順に `.agents/skills` の直下の各ディレクトリの `SKILL.md` を見る。
- どちらのモードでも読む（指示のファイルの `collect` は制限モードで何も読まないが、Skill はこれと別の関数として作り、モードに依らず呼ぶ）。
- 読む条件は、指示のファイルと同じとする。通常のファイルであること、待たない形（`O_NONBLOCK`）で開くこと、読まないファイル（`exclude.reason`）に当たらないこと、ハードリンクの数が 1 であること、同じ (dev, ino) を二度読まないこと。開き方は、利用者の Skill はホームを、プロジェクトの Skill は探す範囲の根を `ScopeRoot` で開いて、その相対で開く。実のパスは、利用者の Skill ではホームの中、プロジェクトの Skill では探す範囲の中になければならない（`.agents/skills/<名前>` が範囲の中の `.claude/skills/<名前>` へのシンボリックリンクであるのは受け付ける）。
- `SKILL.md` は 256 KiB までとし、超えたら読まない（理由を示す）。全文を読んで SHA-256 を取る。全文は LLM に渡さない（LLM が要るときに `read` で読む）。
- 先頭の欄の読み取りは、`src/benitoite/skill.rs` の `skill_parts` と同じ形（`---` の行で囲まれた範囲の、`name:` と `description:` で始まる最初の行）を、共通の関数に分けて使う。値の前後の空白を除き、全体を `"` か `'` で囲んでいればその囲みを外す（中の逃がしの文字は解かない）。`>` や `|` で始まる複数行の値、先頭の欄がないもの、`name` か `description` が空のものは読まない（理由を示す）。
- 受け付ける条件（設計書の値。BH-102 で確かめる見込みの値）:
  - `name` が `^[a-z0-9-]{1,64}$` に当たり、Skill のディレクトリの名前と同じ。
  - `description` が 1,024 文字（Unicode のスカラ値の数）まで。改行を含まない。
  - `name` が `benitoite` でない。
- 同じ名前が二つ以上あれば、利用者の Skill を、次にプロジェクトの根に近い側（探す順で先に見たもの）を採り、ほかは「名前の重なり」として読まなかったものに入れる。
- 採った Skill は名前の順に並べる。100 個を超えた分と、一覧を描いた大きさの合計が 16 KiB を超えた分は、名前の順で後ろのものから除き、読まなかったものに入れる。
- 読まなかったものには、パスと理由（英語の決まった文。`text` のモジュールに置く）を持たせる。ディレクトリに `SKILL.md` がないものは、黙って飛ばす（読まなかったものに入れない）。

### 2. 環境の情報

- 読んだ Skill が一つ以上あれば、環境の情報に次の形の塊を加える。置く位置は、プロジェクトの指示のファイルの後とする。

  ```text
  <skills>
  <skill name="NAME" path="/abs/path/to/SKILL.md">DESCRIPTION</skill>
  ...
  </skills>
  ```

  `path` は `SKILL.md` の実のパス、`DESCRIPTION` は先頭の欄の値である。属性の値と本文に `<`・`>`・`&`・`"` があれば、`&lt;` などに置き換える。
- 塊の前に、「Skill の全文は、要るときに `read` で `path` を読む」旨の一行を置く（英語。`prompts/` の下のファイルに置き、実行ファイルに埋め込む）。
- 一覧はセッションの間は変えない。`render`・`changes_since`・`changes_since_rendered`・`render_fields` で、`project_instructions` と同じく特別に扱う（`create_project` でも一覧を変えない。再開のときは、記録に描いた文字列と照らす今の仕組みに乗せる）。

### 3. Skill のディレクトリを読む・検査する・実行する

- `ToolContext` に、読んだ Skill のディレクトリを `ScopeRoot` として開いた一覧を持たせる。開くときはシンボリックリンクを辿らない形（`no_follow` などの今の口）を使い、実のパスを確かめる。
- `open_file`（`search` が偽のとき）の根の一覧に、Skill のディレクトリを加える（project、temporary、spill の後）。`search_path` は、制限モードでも、パスが Skill のディレクトリの中ならそこを起点に許す。読まないファイルの規則はそのまま掛かる。
- `OUTSIDE_SCOPE` の文に、読んだ Skill のディレクトリも読めることを加える（文が変わるので、LLM に見せる文の期待のファイルに当たるかを確かめ、当たれば作り直す）。
- `write`・`edit` は今までどおりプロジェクトの中だけに書ける（制限モードは `.bnt` だけ）。Skill のディレクトリには書けない。
- 制限モードの `benitoite_check`・`benitoite_run`・`benitoite_test` は、`open_file` と `search_path` を通るので、Skill のディレクトリの `.bnt` を受け付けるようになる。処理系の作業ディレクトリは今までどおりプロジェクトとする。確認の要約と診断の表示で、プロジェクトの外のパスは絶対のパスで示す（プロジェクトからの相対にできないときに、壊れた相対のパスを作らない）。`DiagnosticSources::resolve` が Skill のディレクトリの取り込み先を読めるものとして数えることを確かめる。
- 書いた後の検査と `nested_package` の判定は、プロジェクトの中だけを対象とする今のままとする。
- 一時的なスクリプト（`benitoite_scratch`）の方針は変えない。

### 4. 制限モードの規則の文

- `prompts/mode-restricted.md` に、次の一文を加える（英語の文は実装する者が整える。意味を変えない）。

  ```text
  Skills may describe shell or other-language commands; those cannot run in restricted mode, so do the same work in Benitoite or tell the user it cannot be done.
  ```

- `examples/update_context_expected.rs` で期待のファイルを作り直し、変わるのが `tests/expected/system-restricted.txt` と `system-restricted-read-plain.txt` の規則の行だけであることを確かめる（`system_prompt_sha256` が変わる）。

### 5. 設定、記録、表示

- 設定: `[skills] user`・`project`。既定はどちらも `true`。偽なら、その側を探さない。
- 記録: ヘッダに読んだ Skill（`name`・`path`・`sha256`・`source`）を書く。SessionStart の記録とイベントに、読んだものと読まなかったもの（理由付き）を書く。
- 表示: `exec` の標準エラー出力と対話の画面の通知に、指示のファイルと同じ形で、読んだ Skill（`Loaded skill: <名前> (<パス>)`）と読まなかった Skill（`Skipped skill: <パス>: <理由>`）を示す。`/status` にも同じ一覧を示す。文言は `text` のモジュールに置く。

### 6. 評価の道具

- `tools/skill-eval/skill_eval.py` の `benitoiteh_config` が書く設定に、`[skills]` の表と `user = false` を加える（`project` は書かない。評価の作業ディレクトリの `.agents/skills/benitoite` は、名前 `benitoite` を取っておくので読まれない）。`test_skill_eval.py` の、設定の末尾を確かめるテストを合わせる。`SOURCE.md` の「写した後に変えた点」に一行加える。

## 触るファイル

- `crates/benitoiteh/src/context/`（`skills.rs` を新しく、`environment.rs`、`mod.rs`、`instructions.rs`（共通の関数に分けるとき））
- `crates/benitoiteh/src/benitoite/skill.rs`（先頭の欄の読み取りを共通にするとき）
- `crates/benitoiteh/src/tools/mod.rs`、`paths.rs`、`search.rs`、`text.rs`、`benitoite_check.rs`・`benitoite_run.rs`・`benitoite_test.rs`（パスの表示）
- `crates/benitoiteh/src/config/mod.rs`、`src/cli/exec.rs`
- `crates/benitoiteh/src/session/record.rs`、`src/agent/events.rs`、`src/agent/mod.rs`、`src/agent/status.rs`、`src/ui/`
- `crates/benitoiteh/prompts/mode-restricted.md`、`prompts/` の新しいファイル
- `crates/benitoiteh/tests/` の該当のテストと `tests/expected/`
- `tools/skill-eval/skill_eval.py`、`test_skill_eval.py`、`SOURCE.md`

## テスト

- 探して読む（一時的なホームとプロジェクトで）: 利用者の側とプロジェクトの側の両方、`[skills]` の各値、git の根までの探し方とホームより上の扱い、`benitoite` の名前、名前とディレクトリの名前の食い違い、名前の形、説明の長さ、複数行の値、先頭の欄のないもの、256 KiB を超えるもの、100 個と 16 KiB の上限、名前の重なりの採り方、範囲の中へのシンボリックリンク（受け付ける）と外へのシンボリックリンク（拒む）、読まないファイル、ハードリンクの数、名前付きのパイプ（待たずに拒む）、`SKILL.md` のないディレクトリ（黙って飛ばす）。
- 環境の情報: 塊の形と置き換え、一覧が `create_project` と再開で変わらないこと、Skill がないときに塊を置かないこと。
- 道具: 両方のモードで、Skill のディレクトリの中を `read`・`glob`・`grep` で読めること、外は今までどおり拒むこと、読まないファイルを拒むこと、`write`・`edit` で書けないこと。制限モードで Skill のディレクトリの `.bnt` を `benitoite_check` で検査できること（処理系を使うテストの既存の形で）。`benitoite_run` と `benitoite_test` は、処理系とサンドボックスを使うテストの既存の形で確かめる（`--inside-codex` で除かれるものは、除かれることを完了の報告に書く）。
- システムプロンプトと道具の定義の期待のファイル（変わるのは制限モードの二つだけ）。
- 設定（既定、偽の値、知らない欄、プロジェクトの設定ファイルでの誤り）、記録のヘッダ（欄のない前の記録を読めること）、表示の文言。
- 評価の道具の設定の出力。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、二つの Skill（利用者の側とプロジェクトの側）を置いた一時的なホームで `exec` を模擬の提供者で走らせたときの、環境の情報の `<skills>` の塊と、標準エラー出力の表示の例を書く。

## 補足（2026-10-19、始める前の点検）

本節は、上の各節に優先する。設計書 01-04「利用者の Agent Skill」に、1 と 11 の条件を 2026-10-19 に加えた。

1. **Skill のディレクトリの条件**: `.agents/skills` の直下の項目の名前と、行き先の実のパスの最後の要素が、どちらも `name` と同じでなければならない。Skill のディレクトリの実のパスが、プロジェクトのディレクトリ、探す範囲の根、ホームのどれかと同じかその祖先なら、理由を示して読まない（`.agents/skills/x -> ../..` で、リポジトリの全体が読める範囲になることを防ぐ）。
2. **開き方と記述子**: Skill のディレクトリは、ホーム（または探す範囲の根）の `ScopeRoot` から `open_dir` で開く（範囲の外へ出ない形。範囲の中のリンクは辿る）。`SKILL.md` は、そのディレクトリの記述子から相対に開く。「この作業が認める型の変更」の `directory` は開いたディレクトリの実のパスであり、`SKILL.md` の親ではない。開いた記述子は開き直さずに `ToolContext` へ移す。保持した根は `no_follow` にしない。読み込みの関数は `(UserSkills, Vec<ScopeRoot>)` を返し、`UserSkills` は記述子を持たない（`Environment` が `Clone`・`Eq` を導出するため）。
3. **`search_path` と `search.rs`**: `search_path` は、当たった根（project か、どの Skill の根か）と相対のパスを返す形にする（非公開の関数の形の変更を認める）。制限モードでは、project で解けなければ各 Skill の根で `resolve_path` を試し、実のパスがその根の下にあることを確かめる。`search.rs` の 63〜72 行あたりの、プロジェクトの外の起点を `ScopeRoot::open(directory)` で開き直す所は、当たった根から `open_dir` で開く形に改める（パスから開き直すと、判定の後の差し替えで別の場所を辿る）。相対のパスは今どおりプロジェクトからの相対として解く。
4. **通常モード**: 通常モードでは Skill のディレクトリは元々読める。根は両方のモードで持つが、通常モードの振る舞いは変えない。外を拒むことと Skill の中を読めることのテストは、制限モードだけで行う。
5. **書く道具**: 「3.」の「Skill のディレクトリには書けない」を、「Skill であることで書ける範囲を広げない。書く道具は変えない」に読み替える。テストは、制限モードでプロジェクトの外の利用者の Skill に書けないことだけとする。
6. **処理系の子のサンドボックス**: `restricted.rs` と `effects.rs` は変えない（実行・テスト・問い合わせの読み取りは元々、読ませない候補を除いた全体である）。
7. **パスの表示**: 確認の要約、確認の画面、診断、`glob`・`grep` の結果は、プロジェクトの外を絶対のパスで示している。直さずに保ち、テストで確かめる。`benitoite_check.rs`・`benitoite_run.rs`・`benitoite_test.rs` は、要らなければ変えない。
8. **取り込みの確かめ**: Skill のディレクトリの中で兄弟のモジュールを取り込むスクリプトに `benitoite_run` を掛け、確認の要求で `unread_imports` が 0 になることを `tests/tools_benitoite_run.rs` で確かめる。0 にならなければ止まって報告する。
9. **読み取りの細目**: `Collector::read` は使わず、同じ項目を確かめる処理を別に書く（通常のファイル、ハードリンクの数 1（利用者の Skill も含む）、(dev, ino) の重なり、待たない形）。fstat の大きさが 256 KiB を超えるものは読まない。読むときは 256 KiB + 1 バイトまでとし、超えたら読まない。UTF-8 でないものは読まない。`search_root` は `pub(super)` にする。ホームの `canonicalize` は両方のモードで行い、失敗したときは起動を止めず、利用者の Skill を読まなかったものとして示す。
10. **先頭の欄**: 共通に分けるのは先頭の欄の読み取りだけとし、`skill_parts` の本文の検査（注釈、256 KiB、`</benitoite>`）は利用者の Skill に掛けない。引用符を外すのは利用者の Skill の側だけとし、`tests/expected/system-normal*.txt` の `benitoite:` の行が変わらないことを確かめる。次のものは「複数行の値」または「先頭の欄がない」として読まない: `name:`・`description:` の次の行が空白で始まるもの、値が引用符で始まり同じ引用符で終わらないもの、CRLF の改行の SKILL.md、BOM の付いた SKILL.md。
11. **直下の項目**: `.agents/skills` の直下のディレクトリでない項目（`README.md` など）は黙って飛ばす。範囲の外へ出るリンクと開けないものは、理由を示す。直下は名前の順で最初の 1,000 項目だけを見て、超えた分は数を添えて一件の読まなかったものとして示す。直下の項目が名前付きのパイプでも止まらないことを確かめる。
12. **16 KiB の数え方**: 置き換えた後の `<skills>`〜`</skills>` の UTF-8 のバイト数とし、案内の一行は含めない。
13. **環境の情報の形**: `skills` の欄の値は、案内の一行（`prompts/` のファイル）と `<skills>…</skills>` を合わせたもの一つとし、`<environment>` の中で指示のファイルの後に置く。`render_fields` と `changes_since_rendered` で、`project_instructions` と同じく、そのまま並べる（もう一度置き換えない）。
14. **再開**: 再開のときは Skill を読み直す。記録に描いた文字列に今の塊がなければ、塊の全体を次の依頼の前に加える。読める根は今の一覧に従う。消えた Skill は古い塊に残るが、制限モードでは読めない。ヘッダは書き直さず、再開の SessionStart に今の一覧を書く。このことをテストで確かめる。
15. **create_project**: Skill の一覧と根は切り替えで変えない（01-04 を正とする）。前のプロジェクトの中のプロジェクトの Skill は、切り替えの後も読める。設計書 01-03 の「create_project」の、前のプロジェクトを範囲から外す記述と字面で違うことを、完了の報告の「残したこと」に書く。
16. **圧縮**: 圧縮の後に残る環境の情報は、会話の最初の項目のものだけである（`agent/compaction/conversation.rs` の `rebuild`）。初めの `<skills>` は保たれる。再開や create_project で途中に加えた変化が要約で消えることは、この作業では直さず、「残したこと」に書く。
17. **設計書の字面**: 01-03 の `benitoite_check` の「制限モードではプロジェクトの下に限り」は、Skill の根を含まない記述として読み、01-04 の決定に従う。止まらなくてよい。
18. **記録の型**: SessionStart（記録とイベント）に `skills: Vec<RecordedSkill>` と `skipped_skills: Vec<SkippedInstruction>` を足す。どちらも `#[serde(default, skip_serializing_if = "Vec::is_empty")]` を付ける。`RecordedSkill { name, path, sha256, source }` とし、`source` は `enum SkillSource { User, Project }`（`snake_case`）、`sha256` は 16 進の小文字とする。`session` に置いて再公開する。ヘッダの `skills` も `RecordedSkill` の列とする。`AgentStatus` に同じ二つの欄を足すことを認める。
19. **表示の文言**: `Skill loaded: <name> (<path>)` と `Skill skipped: <path>: <reason>` とする（`Instruction file loaded:` と揃える。「5.」の文言はこれに置き換える）。読んだものは実のパス、読まなかったものは探したときのパスを示す。
20. **`OUTSIDE_SCOPE`**: 期待のファイルにも設計書にも入っていない。書く道具の誤りにも使われるので、`Path is outside the allowed scope. Use a project-relative path; read also allows this session's temporary and spill directories, and read, glob, and grep allow the loaded skill directories.` とする。
21. **制限モードの一文**: 「4.」の英文をそのまま `prompts/mode-restricted.md` の末尾の行に加える。変わるのは、`system-restricted.txt` と `system-restricted-read-plain.txt` の `<mode>` の節の一行だけである。
22. **テストの置き場所**: `benitoite_check` と、`benitoite_run`・`benitoite_test` の確認の要求までは、`tests/tools_check.rs`・`tools_benitoite_run.rs`・`tools_benitoite_test.rs` に置く（Codex の中で走る）。実際の実行は `tests/sandbox_benitoite_run.rs`・`sandbox_benitoite_test.rs` に、制限モードの `exec` は `tests/sandbox_exec_restricted.rs` に置き、走らせなかったことを報告に書く。
23. **完了条件の例**: `exec` の例は、`tests/exec.rs` の、指示のファイルを表示し記録する試しと同じ形（通常モード、`--sandbox danger-full-access`、一時的な `HOME`）で取る。ループバックが使えず走らなければ、走らなかったことと理由を書く。
24. **触るファイルの補い**: `src/session/mod.rs`、`src/session/resume.rs`、`src/cli/exec/tests.rs`、`src/context/instructions/text.rs` または `src/context/text.rs`、`tests/session.rs`、`tests/support/rewind.rs` を加える。新しいモジュールは `pub mod skills` とする。
25. **設計書の例**: 05-01 の設定の例への `[skills]` の追加は、取り込みの後にオーケストレータが行う。Codex は設計書を変えない。
