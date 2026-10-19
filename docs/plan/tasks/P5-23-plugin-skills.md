# P5-23 Agent Plugins のプラグインの Skill を読む

## 目的

設計書 01-04「Agent Plugins のプラグイン」の【決定】を実装する。Agent Plugins 1.0.0 のプラグインのうち、Skill の部品を、制限モードと通常モードの両方で読む。MCP サーバの部品は読まない（BH-116）。この作業で本物の LLM の提供者を呼ばない。

## 読む設計書

- [文脈の管理](../../design/01-agent/01-04-context.md)の「利用者の Agent Skill」「Agent Plugins のプラグイン」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」（`[plugins]`）
- Agent Plugins 1.0.0 の仕様（<https://agent-plugins.org/specification>。§4.1 パスの範囲、§5 `plugin.json`、§7.1 Skill、§8 拡張、§11 クライアントの最小の要件）
- 既存のもの: `crates/benitoiteh/src/context/skills.rs`（P5-09 の利用者の Skill の読み込み、上限、重なり、記録）、`src/tools/`（Skill のディレクトリの読み取り）、`src/config/mod.rs`（`[skills]`）、`src/session/record.rs`（ヘッダの Skill の記録）

## 作るもの

### 1. プラグインの見つけ方

- 利用者のプラグインを `~/.agents/plugins/<名前>/`、プロジェクトのプラグインを、プロジェクトの Skill と同じ探す範囲の各ディレクトリの `.agents/plugins/<名前>/` から探す。直下の項目の数の上限は、利用者の Skill と同じ扱い（今の最初の 1,000 項目まで）とする。
- 項目の実のパスが、利用者のプラグインではホームディレクトリの中、プロジェクトのプラグインでは探す範囲の中にあること、直下の項目の名前と実のパスの最後の要素の名前が `plugin.json` の `name` と同じであることを、今の Skill と同じ規則で確かめる。

### 2. `plugin.json` の検査

- 根の `plugin.json` を、今の Skill の `SKILL.md` と同じ開き方と大きさの上限（64 KiB など、今の上限に合わせる）で読み、JSON として解く。
- `$schema` が `https://agent-plugins.org/schemas/1.0.0/plugin.schema.json` でなければ、プラグインを断り、対応しない版であることを示す。
- `name`（必須。1〜64 文字の英小文字・数字・`-`・`.`、先頭と末尾が英数字）、`version`・`description`・`homepage`・`repository`・`license`（文字列）、`author`（仕様の形）、`keywords`（文字列の配列）、`extensions`（オブジェクト）の型を確かめる。仕様のほかの最上位の欄は、名前を示して無視する。そのほかの形の誤りは、プラグインを断り、その部品を一つも読まない。`extensions` がオブジェクトでなければ、示して無視する（中身は確かめない）。
- 型と規則は、仕様の JSON Schema（<https://agent-plugins.org/schemas/1.0.0/plugin.schema.json>。2026-10-19 にオーケストレータが取得した）に合わせる。Schema の要点は次のとおり。
  - 必須は `$schema`（上の値と完全に一致）と `name`。
  - `name` は文字列で 1〜64 文字、正規表現 `^(?!.*(?:--|\.\.))[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$`（`--` と `..` を含まない）。`regex` クレートは先読みを持たないので、`--`・`..` を含まないことは別に確かめ、残りを `^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$` で確かめる。
  - `version`・`description`・`homepage`・`repository`・`license` は文字列。`keywords` は文字列の配列。
  - `author` はオブジェクトで、欄は `name`・`email`・`url`（どれも文字列）だけ（ほかの欄は誤り）。
  - `extensions` はオブジェクトで、各値はオブジェクト。
  - Schema は最上位の知らない欄を誤りとする（`additionalProperties: false`）が、仕様の本文（§5.2「Clients MUST report and ignore each unknown field」）に従い、最上位の知らない欄は示して無視する。`extensions` がオブジェクトでないときも、本文（§8.1）に従い示して無視する。`extensions` の各値は、`benitoiteh` が使う名前空間を持たないので、本文（§8.1「without validating the contents of their values」）に従い確かめない。それ以外の Schema の違反（型、`author` の知らない欄など）は、プラグインを断る。

### 3. Skill の部品

- `skills/` の直下の各ディレクトリの `SKILL.md` を、P5-09 の規則（名前とディレクトリの名前の一致、説明の長さ、開き方、読む条件、Skill のディレクトリの読み取り、制限モードの `.bnt` の扱い）で読む。合わない Skill はその一つだけを飛ばし、理由を示す。
- パスの実のパスは、プラグインの根の中になければならない（根の中を指すシンボリックリンクは受け付ける）。
- 一覧の並びと上限: 利用者の Skill、利用者のプラグインの Skill（プラグインの名前の順、その中は Skill の名前の順）、プロジェクトの Skill、プロジェクトのプラグインの Skill の順に数え、今の上限（100 個、16 KiB）と、名前の重なりの規則（先に数えたものを採る）と、名前 `benitoite` を読まない規則を当てる。

### 4. MCP サーバの部品と拡張

- `mcp.json` があれば読まず、そのプラグインの MCP サーバを使わないことを示す。拡張のディレクトリは読まない。

### 5. 記録、表示、設定

- 読んだプラグインの名前、版、パス、`plugin.json` の SHA-256 を画面に示し、セッションの記録のヘッダに書く。Skill の記録に、どのプラグインの Skill かを加える。
- 利用者の設定ファイルの `[plugins]` の `user`・`project`（既定はどちらも `true`）を受け付ける。プロジェクトの設定ファイルでは受け付けない。設計書 05-01 の設定の例への追加は、オーケストレータが取り込むときに行う（例を読むテストが落ちないよう、実装と同じ変更で行う）。
- 評価の道具の `benitoiteh` の項目が書く設定に、`[plugins]` の `user = false` を加える。

## 触るファイル

- `crates/benitoiteh/src/context/skills.rs`（か新しい `src/context/plugins.rs`）、`src/config/mod.rs`、`src/session/record.rs`、`src/tools/`（Skill のディレクトリの読み取りの範囲）
- `tools/skill-eval/skill_eval.py`、`test_skill_eval.py`、`SOURCE.md`
- テスト: `tests/` の Skill のテストの置き場所（P5-09 のもの）に合わせる

## テスト

- 見つけ方: 利用者とプロジェクトのプラグインを読み、名前の不一致、範囲の外の実のパスを断ること。
- 検査: `$schema` の違い、`name` の規則の違反、型の違い、知らない欄（示して読み続ける）、`extensions` の形の違い。
- Skill: プラグインの Skill を読み、合わない Skill だけを飛ばすこと。根の外を指すシンボリックリンクを断ること。並びと上限と重なり。
- MCP: `mcp.json` を読まず、示すこと。
- 制限モード: プラグインの Skill の `.bnt` を `benitoite_check` で検査できること。
- 設定: `[plugins]` で読まないようにできること。記録のヘッダにプラグインが書かれること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、読んだプラグインの画面の示し方の例を書く。
