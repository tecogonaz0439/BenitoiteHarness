# P2-06 Skill の文書を処理系から得る、`benitoite_explain`・`benitoite_reference`

## 目的

埋め込んでいる `v0.0.6` の Skill の写しをやめ、起動した処理系から `skill list`・`skill show` で Skill の文書を得て、システムプロンプトと参照の道具に使う（設計書 04-01 の「取り込む文書と版」「参照の文書と診断の説明」、01-04 の「Benitoite の Skill の置き方」。BH-024 の決定）。参照の道具 `benitoite_explain` と `benitoite_reference` を作る。

## 読む設計書

- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「取り込む文書と版」（名前の限り方、一度だけ得て保つこと、使わない問い合わせの結果の条件、`contentHash` をヘッダに書くこと、`{{benitoite-version}}` を置き換えないこと、注釈を除くこと）「参照の文書と診断の説明」
- [文脈の管理](../../design/01-agent/01-04-context.md)の「文脈の構成」「システムプロンプトの構成」「Benitoite の Skill の置き方」（対応の説明の 5 項目、通常モードの名前と説明）
- [道具](../../design/01-agent/01-03-tools.md)の「モードごとの道具の一覧」「benitoite_explain と benitoite_reference」「出力の切り詰め」
- [セッション](../../design/01-agent/01-05-sessions.md)の「セッションの記録」（ヘッダの `SKILL.md` の SHA-256、システムプロンプトのハッシュ）
- [テスト](../../design/07-quality/07-01-testing.md)の「LLM に見せる文の固定」「処理系の用意」
- 既存のもの: `src/context/mod.rs`（`SystemContext::new`、`skill_body`、`skill_parts`、`BENITOITE_DOCUMENT_VERSION`、`include_str!` の写し）、`prompts/benitoite-mapping.md`・`prompts/benitoite-index.md`、`src/tools/`（`ToolId`、`definitions.rs`、`text.rs` の `prompt_parts`、`read.rs` の区切り方）、`src/agent/mod.rs`（ヘッダの `skill_version`）、P2-05 の `benitoite` モジュール（`Processor`、問い合わせ、JSON の読み方）、`tests/expected/`、`assets/benitoite/v0.0.7/`

## この作業が認める型の変更

- `ToolId` に `BenitoiteExplain` と `BenitoiteReference` を加える。
- `SystemContext::new` に、処理系から得た `SKILL.md`（なければ `None`）を渡す形にする。`BENITOITE_DOCUMENT_VERSION` と、埋め込んだ写しを使う `skill_body` を除く。
- `SessionHeader` の `skill_version` を除き、処理系の欄の `SKILL.md` の `contentHash` を埋める（既存の記録は `#[serde(default)]` で読む）。
- `ToolContext` に、Skill の文書を問い合わせて保つ部品への参照を持たせる口（例: `ToolContext::with_skill(SkillDocuments)`）を加える。

## 作るもの

### 1. Skill の文書を得る

- `src/benitoite/skill.rs` に、`skill list --format=json` と `skill show --format=json <名前>` を P2-05 の問い合わせで起動し、04-01 の条件（終了状態 0、一つの完全な JSON、`schemaVersion` 1、`kind`、`name` の一致、`byteLength` と `contentHash`（SHA-256）が `text` の UTF-8 のバイト列と合うこと）を確かめる部品を作る。`v0.0.7` の出力は、一覧が `{"documents":[{"name","byteLength","contentHash":{"algorithm":"sha256","value"}}...]}`、文書が `{"kind":"skillDocument","name","byteLength","contentHash":{...},"text",...}` の形である（2026-10-18 にオーケストレータが手元の `v0.0.7` で見た。欄の並びに頼らない）。`algorithm` が `sha256` でなければ使わない。
- 名前は、一覧のうち `SKILL.md` と `references/` で始まるものに限る。一覧は初めて要るときに一度だけ得て保ち、本文はセッションの間、名前ごとに保つ（同じ名前を再び問い合わせない）。保つ部品はスレッドの間で共有できる形（`Mutex` など）にする（参照の道具は並べて実行する）。
- 起動のとき（`prepare`）に、どちらのモードでも `SKILL.md` を得る。得られないとき、制限モードは起動を止め、通常モードは Benitoite の道具と `<benitoite>` の節を外して続ける（理由を診断に積む）。

### 2. システムプロンプト

- 制限モード: `<benitoite>` の節に、対応の説明と、得た `SKILL.md` の本文（先頭の属性の部分と、本文の先頭の HTML の注釈を除く。今の `skill_parts` の規則）を置く。`{{benitoite-version}}` は処理系が置き換えてあるので、置き換えない。
- 対応の説明（`prompts/benitoite-mapping.md`）を、01-04 の 5 項目に合わせて次の英文に改める（オーケストレータが取り込む前に文面を見直す）。

  ```text
  Use benitoite_check for `benitoite check`, benitoite_check with `tests` set to true for `benitoite test --check`, benitoite_run for `benitoite run`, and benitoite_test for `benitoite test` in the Skill below.
  Read `references/<name>.md` with benitoite_reference by specifying `name`.
  Skip "Check the version first", `skill list`, and `skill show`: benitoiteh has already checked the processor and loaded the Skill at startup.
  benitoite_run and benitoite_test obtain the inspection report and pass `--allow-effects` themselves; do not run `--report=json` or choose the allowed effects yourself.
  benitoiteh handles the "Before running" confirmation in its UI; you do not need to ask the user for confirmation yourself. You may explain what the effects mean in your response.
  ```

- 通常モード: 処理系を使えるときは、得た `SKILL.md` の先頭の属性の `name` と `description` と、`prompts/benitoite-index.md` の一行を置く（今の形）。処理系を使えないときは、`<benitoite>` の節を置かない。
- ヘッダのシステムプロンプトのハッシュは、得た `SKILL.md` を含めた全文から求める（今の関数のまま）。`SKILL.md` の `contentHash` をヘッダに書く。

### 3. `benitoite_explain` と `benitoite_reference`

- 両方のモードの `ToolSet` に、01-03 の表の順で加える（`benitoite_check`〜`benitoite_scratch` の後、`create_project` と `web_fetch` の前。まだない道具の位置は空けたまま、相対の順を守る）。どちらも並べて実行してよい。通常モードで処理系を使えないときは加えない。
- `benitoite_reference`: 引数は `{ "name": "string (optional)", "offset": "integer (optional)", "limit": "integer (optional)" }`。名前は `references/` と `.md` を除いたものとし、`references/<名前>.md` が一覧にあるものだけを受け付ける。通常モードでは `SKILL` で `SKILL.md`（注釈を除いた本文）を返す。名前を省いたときは、引ける名前の一覧（通常モードでは `SKILL` を含める）を返す。一覧にない名前は、誤りの印を付けて同じ一覧を返す。本文は `read` と同じく 2,000 行か 50 KiB で区切り、続きの読み方を示す（`offset`・`limit` は行で数える。`read` の区切り方の関数を使う）。
- `benitoite_explain`: 引数は `{ "code": "string" }`。コードは英大文字 1 字と数字 4 字（`E0405`・`W0501` の形）に限る。`references/diagnostics.md` の、そのコードの見出し（`## E0405`）から次の `## ` の見出しの前までを返す。見出しがなければ、誤りとして返す。
- 道具の説明の英文を `prompts/tools/benitoite_reference.md` と `prompts/tools/benitoite_explain.md` に置く。道具の定義は、モードだけで決まる固定の文とする（処理系の一覧に依らない）。文面の案は次のとおりとし、オーケストレータが取り込む前に見直す。

  ```text
  Read a document of the Benitoite Skill that comes with the installed processor.
  `name` is a path under references/ without `.md`, such as `idioms`, `grammar`, `common-mistakes`, `language-comparison`, `diagnostics`, or `inspection`; `stdlib/index` lists the standard library modules.
  Omit `name` to list the available names. Long documents are returned in parts; use `offset` and `limit` to read on.
  ```

  通常モードでは、2 行目の後に ``Use `SKILL` to read the main Skill document.`` を加える（道具の定義はモードで決まる）。

  ```text
  Explain a Benitoite diagnostic code, such as E0405 or W0501.
  Returns the section for that code from the Skill's diagnostics reference.
  ```

- 誤りの英文は `tools/text.rs` に置く。

### 4. 埋め込んだ写しを除く

- `include_str!` で埋め込んだ `assets/benitoite/v0.0.6/skill/SKILL.md` を除き、`assets/benitoite/v0.0.6/` を消す（取り込みの記録は git の履歴に残る。`v0.0.7` の写しは、システムプロンプトの対応の説明を確かめた版の記録として残す。04-01）。
- 実行ファイルの大きさの変化（Skill の約 21 KB の分が減る見込み）を測って報告に書く。

### 5. LLM に見せる文の固定

- `tests/expected/system-*.txt` と `tools-*.json` を、`BENITOITEH_TEST_BENITOITE` の `v0.0.7` の処理系で作り直す。差分が、対応の説明、Skill の本文の版（`v0.0.6` から `v0.0.7`）、二つの道具の分だけであることを報告に書く。通常モードで処理系を使えない場合の道具の定義の期待のファイル（`tools-normal-without-benitoite.json` など）を加える。
- テストで、制限モードのシステムプロンプトの `<benitoite>` の節の本文が、`assets/benitoite/v0.0.7/skill/SKILL.md` の `{{benitoite-version}}` を `0.0.7` に置き換え、属性と注釈を除いたものと一致することを確かめる（07-01）。

## 触るファイル

- `crates/benitoiteh/src/benitoite/`（`skill.rs`（新）、`text.rs`）
- `crates/benitoiteh/src/context/`（`mod.rs`、`tests.rs`）
- `crates/benitoiteh/prompts/benitoite-mapping.md`、`prompts/tools/benitoite_reference.md`（新）、`prompts/tools/benitoite_explain.md`（新）
- `crates/benitoiteh/src/tools/`（`benitoite_reference.rs`（新）、`mod.rs`、`definitions.rs`、`text.rs`）
- `crates/benitoiteh/src/agent/mod.rs`、`src/session/record.rs`（ヘッダ）
- `crates/benitoiteh/src/cli/exec.rs`（起動のときの `SKILL.md`）
- `crates/benitoiteh/tests/expected/`、関係するテスト
- `assets/benitoite/v0.0.6/`（消す）

## テスト

- 問い合わせの結果の確かめ: `byteLength`・`contentHash` の食い違い、`name` の食い違い、`kind` の違い、JSON が二つ続くものを使わないこと（決まった出力を返す差し替えか `sandbox-probe` の操作で）。起動のときの `SKILL.md` で当たれば、制限モードは止まり、通常モードは道具と節を外して続けること。
- 一覧の名前の限り方: ライセンスの文書を返さないこと。`../x`、`references/../LICENSE`、絶対パスの形の名前を受け付けないこと。
- 一度だけ問い合わせること（同じ名前を二度引いても問い合わせは一回）。
- `benitoite_reference`: 名前を省いたとき、一覧にない名前のとき、`stdlib/index`、`SKILL`（通常モード）、2,000 行と 50 KiB の区切りと続き。
- `benitoite_explain`: 本物の `diagnostics.md` で `E0405` の節を返すこと。形の違うコードと、ないコード。
- 期待のファイルが一致すること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、道具の説明と対応の説明の英文の全文、`tools/text.rs` に足した英文の全文、期待のファイルの差分の要約、`skill list` の一覧の欄の名前、実行ファイルの大きさの変化を書く。
