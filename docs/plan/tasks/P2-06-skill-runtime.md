# P2-06 Skill の文書を処理系から得る、`benitoite_explain`・`benitoite_reference`

## 目的

埋め込んでいる `v0.0.6` の Skill の写しをやめ、起動した処理系から `skill list`・`skill show` で Skill の文書を得て、システムプロンプトと参照の道具に使う（設計書 04-01 の「取り込む文書と版」「参照の文書と診断の説明」、01-04 の「Benitoite の Skill の置き方」。BH-024 の決定）。参照の道具 `benitoite_explain` と `benitoite_reference` を作る。

## 読む設計書

- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「取り込む文書と版」（名前の限り方、一度だけ得て保つこと、使わない問い合わせの結果の条件、`contentHash` をヘッダに書くこと、`{{benitoite-version}}` を置き換えないこと、注釈を除くこと）「参照の文書と診断の説明」
- [文脈の管理](../../design/01-agent/01-04-context.md)の「文脈の構成」「システムプロンプトの構成」「Benitoite の Skill の置き方」（対応の説明の 6 項目、通常モードの名前と説明）
- [道具](../../design/01-agent/01-03-tools.md)の「モードごとの道具の一覧」「benitoite_explain と benitoite_reference」「出力の切り詰め」
- [セッション](../../design/01-agent/01-05-sessions.md)の「セッションの記録」（ヘッダの `SKILL.md` の SHA-256、システムプロンプトのハッシュ）
- [テスト](../../design/07-quality/07-01-testing.md)の「LLM に見せる文の固定」「処理系の用意」
- 既存のもの: `src/context/mod.rs`（`SystemContext::new`、`skill_body`、`skill_parts`、`BENITOITE_DOCUMENT_VERSION`、`include_str!` の写し）、`src/context/text.rs`（`INVALID_SKILL`）、`prompts/benitoite-mapping.md`・`prompts/benitoite-index.md`、`src/tools/`（`ToolId`、`ToolSet::new`、`Invocation`、`definitions.rs` の `definition`・`arg`、`text.rs` の `prompt_parts`・`argument`・`read_continuation`、`read.rs` の `ReadLines`）、`src/agent/mod.rs`（`Agent::construct` が `ToolSet::new` と `SystemContext::new` を呼び、ヘッダの `skill_version` を書く）、`src/session/record.rs`（`Header`（`Record::Header`）の `skill_version`、`RecordedBenitoite`）、P2-05 の `benitoite` モジュール（`Processor`、`Processor::query`、`QueryContext`、`QueryLauncher`、JSON の読み方、`BenitoiteError`）と `ToolContext::with_benitoite`、`examples/update_context_expected.rs`・`examples/size_probe.rs`、`tests/context.rs`、`tests/expected/`、`assets/benitoite/v0.0.7/`

## この作業が認める型の変更

- `ToolId` に `BenitoiteExplain` と `BenitoiteReference` を加える。
- `ToolSet::new(mode)` は、二つの参照の道具を含む一覧を返す。通常モードで処理系を使えないときのために、`ToolSet::without_benitoite(mode)` を加える。`ToolSet::new` を呼ぶ既存の約 30 の箇所（`tests/tools_read.rs` など）は変えない。理由: 道具の一覧を変えるのは処理系を使えない場合だけであり、既定の口の意味を変えなければ既存の呼び出しを直さずに済む。
- `SystemContext::new(tools: &ToolSet, skill: Option<&SkillDocument>) -> Result<SystemContext, ContextError>` とする。`skill` は処理系から得た `SKILL.md` である。制限モードで `None` のとき、および `tools` が Benitoite の道具を含むことと `skill` が `Some` であることが食い違うときは、`ContextError` を返す（新しい変種。英文は `context/text.rs` に置き、案は ``The Benitoite Skill must be given exactly when the Benitoite tools are enabled, and restricted mode requires both.`` とする。オーケストレータが取り込む前に見直す）。`BENITOITE_DOCUMENT_VERSION` と、埋め込んだ写しを使う `skill_body` を除く。
- `context/text.rs` の `INVALID_SKILL` を ``The Benitoite Skill from the processor has an invalid format.`` に改める。
- `benitoite::skill` に、公開の純粋な関数 `SkillDocument::from_json(requested_name: &str, stdout: &[u8]) -> Result<SkillDocument, BenitoiteError>` を置く。04-01 の条件（後述の 1）を確かめ、`text()` と `content_hash()` を持つ。統合テストと `examples/` は、`std::process::Command` で `skill show --format=json SKILL.md` を直に起動し、その標準出力をこの関数に渡して `SkillDocument` を作る（`Processor` と問い合わせのサンドボックスを通さない）。
- `session::record::Header` の `skill_version` を除き、`RecordedBenitoite` の `SKILL.md` の `contentHash` の欄（P2-05 が `None` で加えた欄）を埋める。`Header` は `deny_unknown_fields` を持たないので、`skill_version` を含む既存の記録はそのまま読める。除くことに `#[serde(default)]` は要らない。
- `ToolContext` に、Skill の文書を問い合わせて保つ部品への参照を持たせる口（例: `ToolContext::with_skill(Arc<SkillDocuments>)`）を加える。

## 作るもの

### 1. Skill の文書を得る

- `src/benitoite/skill.rs` に、`skill list --format=json` と `skill show --format=json <名前>` を P2-05 の `Processor::query` で起動し、結果を確かめる部品を作る。`v0.0.7` の出力の形は次のとおりである（2026-10-18 にオーケストレータが手元の `v0.0.7` で見た。欄の並びに頼らず、名前で読む。`producer` は読まない）。
  - 一覧: `{"schemaVersion":1,"kind":"skillList","producer":{...},"documents":[{"name","byteLength","contentHash":{"algorithm":"sha256","value"}}...]}`。`documents` は 62 項目で、`LICENSE-APACHE` と `LICENSE-MIT` を含む。
  - 文書: `{"schemaVersion":1,"kind":"skillDocument","producer":{...},"name","byteLength","contentHash":{"algorithm":"sha256","value"},"text"}`。
- 一覧は、終了状態 0、一つの完全な JSON のオブジェクト、`schemaVersion` 1、`kind` が `"skillList"` であることを確かめる。文書は、04-01 の条件（終了状態 0、一つの完全な JSON のオブジェクト、`schemaVersion` 1、`kind` が `"skillDocument"`、`name` が求めた名前と一致、`byteLength` と `contentHash`（SHA-256）が `text` の UTF-8 のバイト列と合うこと）を確かめる。`algorithm` が `sha256` でなければ使わない。終了状態のほかの条件は `SkillDocument::from_json` で確かめ、JSON の読み方は P2-05 の部品を使う。
- 道具が求めてよい名前は、一覧のうち `SKILL.md` と、`references/` で始まり `.md` で終わるものに限る。`LICENSE-APACHE`・`LICENSE-MIT` などのライセンスの文書は、道具の一覧にも示さず、求められても返さない。
- 一覧は初めて要るときに一度だけ得て保ち、本文はセッションの間、名前ごとに保つ（同じ名前を再び問い合わせない）。保つ部品は、参照の道具を並べて実行するので、スレッドの間で共有できる形にする。同じ名前を並べて求めたときも問い合わせを一回にするため、名前ごとに一度だけ初期化する（例: 名前ごとの枠の `Mutex` を持ったまま問い合わせ、成功したときだけ本文を保つ。失敗と取り消しは保たず、次の呼び出しで問い合わせ直す）。一覧も同じ形で保つ。
- 問い合わせには、道具の呼び出しの `Invocation.cancelled` を渡す。
- `SKILL.md` と `references/diagnostics.md` は、名前を組み立てずに直に問い合わせてよい。ただし、問い合わせる前に一覧にあることを確かめる。
- 起動のとき（`cli::exec::prepare`）に、どちらのモードでも一覧と `SKILL.md` を得る。得た `SKILL.md` には、システムプロンプトと同じ解析（先頭の属性の `name` と `description`、本文の先頭の HTML の注釈。後述の 2）を当てる。解析の失敗は問い合わせの失敗と同じに扱う。得られないか解析に失敗したとき、制限モードは起動を止め（`exec` の終了状態は 2）、通常モードは Benitoite の道具と `<benitoite>` の節を外して続ける（理由を診断に積む）。
- 部品の問い合わせは、テストで回数を数えられるように、問い合わせの関数を差し替えられる形にしてよい（例: 部品が `Fn(&[OsString], &AtomicBool) -> Result<QueryOutput, BenitoiteError> + Send + Sync` を受け取る）。

### 2. システムプロンプト

- `SKILL.md` の解析（先頭の属性の部分と、本文の先頭の HTML の注釈を除くこと。今の `skill_parts` の規則）を、`benitoite::skill` の純粋な関数に移す。`context` と `tools` が同じ層にあり、`context` が `tools` に依存するので（設計書 06-01「依存の向き」）、二つが共有する解析は下の層の `benitoite` に置く。この関数は、属性の `name` と `description` と、属性と注釈を除いて前後の空白を落とした本文を返す。`context` は解析の失敗を `ContextError::InvalidSkill` にする。`context/tests.rs` の `skill_parts` のテストは、この関数のテストとして移す。
- 制限モード: `<benitoite>` の節に、対応の説明と、得た `SKILL.md` の本文（解析の関数が返す本文）を置く。`{{benitoite-version}}` は処理系が置き換えてあるので、置き換えない。本文の先頭の HTML の注釈は処理系の出力にも残るので、除く（P1-11）。
- 対応の説明（`prompts/benitoite-mapping.md`）を、01-04 の 6 項目に合わせて次の英文に改める（オーケストレータが取り込む前に文面を見直す）。

  ```text
  Use benitoite_check for `benitoite check`, benitoite_check with `tests` set to true for `benitoite test --check`, benitoite_run for `benitoite run`, and benitoite_test for `benitoite test` in the Skill below.
  Read `references/<name>.md` with benitoite_reference, passing `<name>` as `name` (for example, `idioms` or `stdlib/List`).
  Use benitoite_explain to look up a diagnostic code.
  Skip the "Check the version first" section, including `skill list` and `skill show`: benitoiteh has already checked the processor at startup, and benitoite_reference reads the Skill documents.
  benitoite_run and benitoite_test obtain the inspection report and pass `--allow-effects` themselves; you do not read the report or choose the allowed effects.
  benitoiteh handles the "Before running" confirmation in its UI; you do not need to ask the user for confirmation yourself. You may explain what the effects mean in your response.
  ```

- 対応の説明の 1 行目と 5 行目は、まだない道具（`benitoite_check`・`benitoite_run`・`benitoite_test`）を名指す。P2-07、P2-08、P2-12 がこれらの道具を加えるまでの間、システムプロンプトがまだない道具を名指すことは、意図した途中の形とする。理由: 対応の説明は `v0.0.7` の `SKILL.md` に合わせた一つの文であり、作業ごとに書き換えると、期待のファイルの差分が作業ごとに生じて見直しが増える。
- 通常モード: 処理系を使えるときは、得た `SKILL.md` の先頭の属性の `name` と `description` と、`prompts/benitoite-index.md` の一行を置く（今の形）。処理系を使えないときは、`<benitoite>` の節を置かない。
- `Agent::construct` は、`ToolContext` が Skill の部品を持てば `ToolSet::new`、持たなければ（通常モードで処理系を使えないとき）`ToolSet::without_benitoite` を使い、部品が保つ `SKILL.md` を `SystemContext::new` に渡す。
- ヘッダのシステムプロンプトのハッシュは、得た `SKILL.md` を含めた全文から求める（今の関数のまま）。`SKILL.md` の `contentHash` は、どちらのモードでもヘッダに書く（設計書 01-05「セッションの記録」）。

### 3. `benitoite_explain` と `benitoite_reference`

- 両方のモードの `ToolSet::new` に、01-03 の表の順で加える（`benitoite_check`〜`benitoite_scratch` の後、`create_project` と `web_fetch` の前。まだない道具の位置は空けたまま、相対の順を守る）。どちらも並べて実行してよい。`ToolSet::without_benitoite` は加えない。
- `benitoite_reference`: 引数は `{ "name": "string (optional)", "offset": "integer (optional)", "limit": "integer (optional)" }`。
  - 名前は `references/` と `.md` を除いたものとし、`references/<名前>.md` が一覧にあるものだけを受け付ける（`stdlib/index` はモジュールの一覧）。名前の照合は一覧の名前との完全な一致だけで行い、パスとして解釈しない。
  - 通常モードでは、`SKILL` で `SKILL.md` を返す。返す本文は、属性と先頭の注釈を除き、前後の空白を落としたもの（2 の解析の関数が返す本文。制限モードの `<benitoite>` の節の本文と同じ）とする。制限モードでは `SKILL` を一覧にない名前として扱う。
  - 名前を省いたときは、``Available Benitoite Skill documents (pass one as `name`):`` の行に続けて、引ける名前を一行に一つずつ返す。順は処理系の一覧の順とし、通常モードでは先頭に `SKILL` を置く。
  - 一覧にない名前は、`Unknown Skill document "<name>".` の行に続けて同じ一覧を返し、誤りの印を付ける。
  - 本文は、`read` と同じく 2,000 行か 50 KiB で区切り、`text::read_continuation` で続きの読み方を示す。`offset` と `limit` は行で数える（`offset` の既定は 1、`limit` の上限は 2,000）。`read` と違い、行番号を付けず、2,000 文字を超える行も切り詰めない。理由: `v0.0.7` の `references/grammar.md` の 17 行目は 2,089 文字あり、Skill の文書は `grep` で中を探せないので、切り詰めると読めない部分が残る。`offset` が行の数を超えるときは、`read` と同じ `ToolError::Offset` を返す。
  - 区切り方は、`read.rs` の `ReadLines` から、文字列を受け取って区切る関数を取り出して共有する（例: `pub(super) fn split_text(text: &str, offset: u64, limit: u64, style: LineStyle) -> Result<String, ToolError>`。`LineStyle` は、行番号と長い行の切り詰めを行う `read` の形と、行わない参照の形を分ける）。`read` はファイルを塊で読んで同じ区切りの処理へ流す今の形を保ち、出力を変えない（`tests/tools_read.rs` を変えずに通す）。
- `benitoite_explain`: 引数は `{ "code": "string" }`。
  - コードは英大文字 1 字と数字 4 字に限る。満たさなければ ``Code must be one uppercase letter followed by four digits, such as E0405.`` の誤りを返す。
  - `references/diagnostics.md` の、行の全体がちょうど `## <コード>` である行から、次の `## ` で始まる行の前まで（なければ文書の終わりまで）を返す。見出しの行を含め、末尾の空白を落とす。`v0.0.7` の `diagnostics.md` の `## ` の見出しは 305 あり、すべて `[A-Z][0-9]{4}` の形である。処理系には `explain` の副コマンドがないので、文書から切り出す。
  - 見出しがなければ ``No section for <code> in the diagnostics reference. Check the code, or read `diagnostics` with benitoite_reference.`` の誤りを返す。
- 問い合わせの失敗（一覧、文書、条件に合わない結果）は、``Could not read <name> from the Benitoite processor: <reason>.`` の誤りとして返す。`<name>` は求めた文書の名前（`references/idioms.md`、`SKILL.md` など）、一覧なら `the Skill document list` とする。`<reason>` は `BenitoiteError` の表示から末尾の `.` を除いたものとする。`ToolContext` が Skill の部品を持たないとき（本番では生じない。テストの `ToolSet::new` と `ToolContext` の組み合わせで生じる）も、同じ形の誤りとし、`<reason>` を `the processor is not available in this session` とする（案。オーケストレータが取り込む前に見直す）。
- 道具の説明の英文を `prompts/tools/benitoite_reference.md` と `prompts/tools/benitoite_explain.md` に置く。`prompt_parts` により、1 行目が道具の説明（`ToolSpec.description`）、残りの行が `<rules>` の規則になる。道具の定義は、モードだけで決まる固定の文とする（処理系の一覧に依らない）。文面は次のとおりとし、オーケストレータが取り込む前に見直す。

  ```text
  Read a document of the Benitoite Skill that comes with the installed processor.
  Pass `name` as the path under references/ without `.md`, such as `idioms`, `grammar`, `common-mistakes`, `language-comparison`, `diagnostics`, `inspection`, or `stdlib/List`; `stdlib/index` lists the standard library modules.
  Omit `name` to list the available names. Long documents are returned in parts; use `offset` and `limit` to read on.
  ```

  通常モードだけ、規則に ``Pass `SKILL` as `name` to read the main Skill document.`` を加える。この文は `prompts/tools/benitoite_reference-normal.md` に置き、`definitions(mode)` が通常モードのときだけ規則の二つ目として渡す（`<rules>` では `benitoite_reference:` を前に付けた別の行になる）。

  ```text
  Explain a Benitoite diagnostic code, such as E0405, W0501, or R0101.
  Pass the code shown in a diagnostic; the result is that code's section of `references/diagnostics.md`.
  ```

- 引数の説明は、今の `text::argument(name)` が名前だけで決めるので、道具ごとに説明を渡す補助（例: `arg_described(name, kind, required, description)`）を `definitions.rs` に加える。`name` は ``Skill document name, such as idioms or stdlib/List; omit to list names.``、`code` は ``Diagnostic code, such as E0405.`` とする。`offset` と `limit` は今の `text::argument` の説明を使う。
- 誤りと一覧の英文は `tools/text.rs` に置く。

### 4. 埋め込んだ写しを除く

- `include_str!` で埋め込んだ `assets/benitoite/v0.0.6/skill/SKILL.md` を除き、`assets/benitoite/v0.0.6/` を消す（取り込みの記録は git の履歴に残る。`v0.0.7` の写しは、システムプロンプトの対応の説明を確かめた版の記録とテストの比べる元として残す。04-01）。
- `examples/size_probe.rs` は、埋め込んだ Skill がなくなるので、処理系なしに作れる `SystemContext::new(&ToolSet::without_benitoite(Mode::Normal), None)` を表示する形に改め、注釈を合わせる。
- 実行ファイルの大きさの変化（Skill の約 21 KB の分が減る見込み）を測って報告に書く。

### 5. LLM に見せる文の固定

- `examples/update_context_expected.rs` は、`BENITOITEH_TEST_BENITOITE` の処理系を `std::process::Command` で起動して `skill show --format=json SKILL.md` を得て、`SkillDocument::from_json` を通し、両方のモードの期待のファイルを書く。あわせて、通常モードで処理系を使えない場合（`ToolSet::without_benitoite(Mode::Normal)` と `None`）の `tests/expected/system-normal-without-benitoite.txt` と `tests/expected/tools-normal-without-benitoite.json` を書く。環境変数がなければ、理由を示して失敗する。
- `tests/expected/system-*.txt` と `tools-*.json` を、`v0.0.7` の処理系で作り直す。差分が、対応の説明、Skill の本文の版（`v0.0.6` から `v0.0.7`）、通常モードの Skill の説明、二つの道具の分だけであることを報告に書く。
- 期待のファイルは人が見直したものとし、制限モードの `<benitoite>` の節の本文は、見直した `system-restricted.txt` で固定する。テストの期待値を、解析の関数（今の `skill_parts`）で作らない（07-01「LLM に見せる文の固定」、スキル `test-audit`）。
- 別のテストで、本物の処理系の `skill show --format=json SKILL.md` の `text` が、`assets/benitoite/v0.0.7/skill/SKILL.md` の `{{benitoite-version}}` を `0.0.7` に置き換えたものと、バイト列で一致することを確かめる（04-01「取り込む文書と版」）。

## 触るファイル

- `crates/benitoiteh/src/benitoite/`（`skill.rs`（新）、`mod.rs`、`text.rs`）
- `crates/benitoiteh/src/context/`（`mod.rs`、`text.rs`、`tests.rs`）
- `crates/benitoiteh/prompts/benitoite-mapping.md`、`prompts/tools/benitoite_reference.md`（新）、`prompts/tools/benitoite_reference-normal.md`（新）、`prompts/tools/benitoite_explain.md`（新）
- `crates/benitoiteh/src/tools/`（`benitoite_reference.rs`（新）、`mod.rs`、`definitions.rs`、`text.rs`、`read.rs`）
- `crates/benitoiteh/src/agent/mod.rs`、`src/session/record.rs`（`Header`）
- `crates/benitoiteh/src/cli/exec.rs`（起動のときの一覧と `SKILL.md`）
- `crates/benitoiteh/examples/update_context_expected.rs`、`examples/size_probe.rs`
- `crates/benitoiteh/tests/context.rs`、`tests/session.rs`、`tests/support/rewind.rs`（`skill_version` を除く）、関係するテスト
- `crates/benitoiteh/tests/expected/`（`system-restricted.txt`、`system-normal.txt`、`tools-restricted.json`、`tools-normal.json`、`system-normal-without-benitoite.txt`（新）、`tools-normal-without-benitoite.json`（新））
- `assets/benitoite/v0.0.6/`（消す）

## テスト

- 問い合わせの結果の確かめ（`SkillDocument::from_json` と一覧の確かめの純粋な関数にバイト列を渡す）: `byteLength`・`contentHash` の食い違い、`algorithm` が `sha256` でないもの、`name` の食い違い、`kind` の違い（一覧に `skillDocument`、文書に `skillList` など）、`schemaVersion` の違い、JSON が二つ続くものを使わないこと。終了状態が 0 でないものは、決まった出力を返す差し替えか `sandbox-probe` の操作で確かめる。
- 起動のときの `SKILL.md`: 得られないとき、または属性や注釈の解析に失敗するとき、制限モードは終了状態 2 で止まり、通常モードは道具と節を外して続けること。
- 一覧の名前の限り方: ライセンスの文書（`LICENSE-APACHE`・`LICENSE-MIT`）を名前の一覧に示さず、返さないこと。`../x`、`references/../LICENSE-MIT`、絶対パスの形の名前を受け付けないこと。
- 一度だけ問い合わせること: 同じ名前を二度引いても、二つのスレッドで並べて引いても、問い合わせは一回であること（差し替えた問い合わせの関数で回数を数える）。失敗の後は問い合わせ直すこと。
- `benitoite_reference`: 名前を省いたとき（両方のモード。通常モードは先頭に `SKILL`）、一覧にない名前のとき、`stdlib/index`、`SKILL`（通常モードでは本文を返し、制限モードでは一覧にない名前として扱う）、2,000 行と 50 KiB の区切りと続き、2,000 文字を超える行を切り詰めないこと（本物の `references/grammar.md` の 17 行目）、行番号を付けないこと、`offset` が行の数を超えるとき。
- `benitoite_explain`: 本物の `diagnostics.md` で `E0405` の節を返し、次の見出しの行を含まないこと。最後の節が文書の終わりまでであること。形の違うコード（`e0405`、`E405`、`E04050`）と、ないコード。
- 部品を持たない `ToolContext` で、二つの道具が問い合わせの失敗の形の誤りを返すこと。
- `SystemContext::new`: 制限モードで `None`、Benitoite の道具があって `None`、`without_benitoite` で `Some` のときに `ContextError` を返すこと。
- `tests/tools_read.rs` が変えずに通ること（`read` の出力を変えていないこと）。
- 期待のファイルが一致すること（両方のモードと、通常モードで処理系を使えない場合）。本物の処理系の `SKILL.md` の `text` が、写しの `{{benitoite-version}}` を `0.0.7` に置き換えたものとバイト列で一致すること。
- ヘッダに、どちらのモードでも `SKILL.md` の `contentHash` が書かれ、`skill_version` がないこと。`skill_version` を含む既存の記録を読めること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、道具の説明と対応の説明の英文の全文、`tools/text.rs` と `context/text.rs` に足した英文の全文、期待のファイルの差分の要約、`skill list` の一覧の欄の名前、実行ファイルの大きさの変化を書く。

## P2-05 の実装の後の補足（2026-10-18）

P2-05 を取り込んだ後に、この文書を実装に突き合わせた。上の記述と食い違うところは、この節を正とする。

- P2-05 で作ったもの: `Processor::query(&self, args: &[OsString], QueryLimits, cancelled: &AtomicBool) -> Result<QueryOutput, BenitoiteError>`、`QueryLimits::default()`（10 秒と猶予 10 秒。標準エラー出力の上限の欄はまだない）、`QueryOutput { stdout, stderr }`、`json_object(bytes, kind)`、`BenitoiteError`、`benitoite::text::DISABLED`、`src/benitoite/tests.rs` の `Fixture`（`QueryLauncher::TestDirect` と `sandbox-probe`）、`cli/exec.rs` の `prepare`（`processor_result` をモードで分ける流れ）、`tests/support/processor.rs` の `benitoite()`。`Processor::inspect` と `QueryContext` は `pub(crate)` である。
- P2-05 が済ませたので、この作業では作らない・繰り返さない: `RecordedBenitoite.skill_content_hash` の欄（`Option<String>`、`serde(default)`。`Agent::construct` が `None` を書いている。この作業は `SKILL.md` の `contentHash` の `value` を書く）、`--version=json` の `skillList`・`skillDocument` の版の確かめ、終了状態の確かめ（`Processor::query` が `Exit` にする）、一つの完全なオブジェクト・`schemaVersion`・`kind` の確かめ（`json_object(stdout, "skillDocument")`、一覧は `"skillList"`）。`name`・`byteLength`・`contentHash`・`algorithm` の条件は、`SkillDocument::from_json` と一覧の確かめの関数で確かめ、食い違いは `BenitoiteError::Invalid`（英文は `benitoite/text.rs`）とする。
- Skill の部品は、問い合わせの関数（`Fn(&[OsString], &AtomicBool) -> Result<QueryOutput, BenitoiteError> + Send + Sync`）を受け取る公開の作り方と、`Arc<Processor>` から作る作り方（`Processor::query` に `QueryLimits::default()` を渡す）を持つ。理由: `Processor` は `pub(crate)` の `Processor::inspect` でしか作れない。制限モードの `Agent` を作る統合テスト（`tests/agent_loop.rs` の `web_fetch` の両モードのテストなど）は、本物の処理系（`tests/support/processor.rs` の `benitoite()`）を `std::process::Command` で起動する関数を渡して部品を作る。問い合わせの回数を数えるテストも同じ口を使う。`examples/` は `BENITOITEH_TEST_BENITOITE` で処理系のパスを得る。
- 問い合わせには `QueryLimits::default()` と、道具の呼び出しの `Invocation.cancelled`（起動のときは `AtomicBool::new(false)`）を渡す。
- 起動のとき（`prepare`）は、`Processor::inspect` が成功したときだけ、一覧と `SKILL.md` を得る（P2-05 が処理系を使えないとした場合は問い合わせない）。一覧と `SKILL.md` の取得、属性と注釈の解析（失敗は `BenitoiteError::Invalid`）の失敗は、`processor_result` の `Err` と同じ分岐に入れる。制限モードは `StartupError::Benitoite` で止め（終了状態 2）、通常モードは `benitoite::text::DISABLED` の診断を積み、`with_benitoite(None)` とし、Skill の部品も持たせずに続ける（ヘッダの `benitoite` と `/status` の処理系の欄が、道具を外したことと食い違わないように）。版の警告と処理系の変化の通知は、`SKILL.md` を得た後に積む。この分岐は関数に分け、`cli/exec/tests.rs` で失敗の結果を渡して確かめる（本物の処理系は失敗しないため）。成功の流れは、既存の本物の処理系での `prepare` のテストに、道具と `<benitoite>` の節があることの確かめを足す。
- `context` は、起動のときに解析が通った本文を受け取るので、`ContextError::InvalidSkill` は部品の食い違いのときだけ起きる。
- 道具の誤りの `<reason>` で、`BenitoiteError::OutputLimit` は Skill の文書向けの文（例: `the Skill document from benitoite exceeded 16 MiB`）、`Exit` は `benitoite exited with status <n>` の形にする（P2-05 の検査の報告向けの文を使わない）。
- 環境の情報（`context/environment.rs`）の処理系の版（今は `not configured` のまま）を、確かめた処理系の `version` で埋める。処理系がないときは今のまま（01-04「環境の情報」）。
- テストのファイル: `tests/sandbox_exec_restricted.rs` のヘッダの `/benitoite/skill_content_hash` を `null` と確かめている所を、64 字の十六進の確かめに改める。`tests/agent_loop.rs`（制限モードの `Agent` に Skill の部品を渡す）と `src/cli/exec/tests.rs` も触る。`kind` の取り違えは、`json_object` に正しい `kind` を渡していることだけを確かめ、`schemaVersion` と二つ続く JSON は繰り返さない。差し替えた関数が `BenitoiteError::Exit` を返したときに、本文を保たず道具の誤りになることを確かめる。
