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

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。行の番号は `main`（`a16b866`）のもの。P5-20〜P5-22 を取り込んだ後に始めるので、`src/config/mod.rs`・`src/cli/`・`src/ui/tui/` の行の番号は目安とする。Agent Plugins の仕様の本文の写しは手元にないので、設計書 01-04 と本節と上の Schema の要点に従う。

1. **今の作り**: 入口は `context/skills.rs:141` の `collect(project, home, &UserSkillsConfig, &ExcludeRules) -> (UserSkills, Vec<ScopeRoot>)`（呼ぶのは `cli/exec.rs:562-563`。戻り値は `ToolContext::with_skill_roots` と `Environment::with_skills` へ）。上限と重なりは `Collector`（198-271。`seen_names`、`entries` の鍵、`cutoffs`）、一つの Skill の読み方は `read`（347-421）と `contents`（423-443。`SKILL.md` の上限は **256 KiB**）。道具は `ToolContext.skill_roots`（Skill のディレクトリごとの `ScopeRoot`）を通して Skill のディレクトリを読むので、プラグインの Skill のディレクトリの `ScopeRoot` を同じ並びに入れれば `src/tools/` は変えなくてよい（`benitoite_check` なども同じ `open_file` を通る）。
2. **置き場所**: 新しい `src/context/plugins.rs`（と `plugins/text.rs`）に `plugin.json` の検査の純粋な関数（読んだバイト列から、名前・版・示すだけの通知、か断る理由を返す）を置く。探す処理は `skills.rs` の `Collector` に組み込み、上限・重なり・`(dev, ino)` の重複の確かめ・読まないファイルの規則を共有する。`collect` に `&UserPluginsConfig`（形は `UserSkillsConfig` と同じ、既定はどちらも真）を加え、呼ぶ所（`cli/exec.rs:563`、`tests/user_skills.rs` の 95・155・189・391・466・722 行、`tests/tools_check.rs:99`、`tests/support/benitoite_run.rs:69`、`tests/agent_loop/create_project.rs:50`）をすべて直す。
3. **探す順と数える順**（名前の重なりの勝ち負けも決める）: (1) `~/.agents/skills`、(2) `~/.agents/plugins/*`（プラグインの名前の順、中は Skill の名前の順）、(3) プロジェクトの `.agents/skills`（根からプロジェクトへ、全ディレクトリ）、(4) プロジェクトの `.agents/plugins/*`（根からプロジェクトへ、各ディレクトリの中は名前の順）。プロジェクトの Skill は、どのディレクトリのものでも、プロジェクトのプラグインの Skill に優先する。上限の鍵は `(段, プラグインの名前（プラグインでなければ空）, Skill の名前)` に広げ、`cutoffs` は段ごとに持つ。一覧に示す並びと `<skill>` の形は今のまま（プラグインの属性を加えない）。上限の文（`text::SKILL_LIMIT`・`INDEX_LIMIT`）の数える順の説明を、四つの段の順に改める。
4. **プラグインの根**: `.agents/plugins` の直下も 1,000 項目まで見て、ディレクトリでもリンクでもない項目は黙って飛ばす。根は Skill と同じく `open_dir` で開き、実のパスが範囲の中、UTF-8、project・scope・home と同じでもその祖先でもないこと、項目の名前と実のパスの最後の要素が `plugin.json` の `name` と一致すること（違えばプラグインを断る）を確かめる。`plugin.json` がなければ、プラグインとして扱わず黙って飛ばす。`plugin.json` は根の記述子から開き、`SKILL.md` と同じ検査（解決後のパスが根の中、読まないファイルの規則、通常のファイル、ハードリンク 1、`(dev, ino)` の重複なし、**256 KiB** 以下、UTF-8）を当てる（上の「64 KiB など」を読み替える）。SHA-256 は読んだ生のバイト列から取る。
5. **`plugin.json` の検査**（順: JSON の解析、`$schema`、`name`、ディレクトリの名前の一致、ほかの欄の型、通知、Skill）: 最上位がオブジェクトでなければ断る。`$schema` がない・文字列でない・値が違うときは、対応しない版として断る（値は示さない）。`name` は、全バイトが `[a-z0-9.-]`、先頭と末尾が `[a-z0-9]`、1〜64 バイト、`--` と `..` を含まない、を手で書いた判定で確かめる（`regex` を使わなくてよい）。重複した鍵は `serde_json` の既定（後の値）のままとする。最上位の知らない欄は、プラグインごとに 1 行で示し（名前の順で最大 10 個、超えた分は数だけ）、読み続ける。`extensions` がオブジェクトでなければ示して読み続け、`extensions` の値は確かめない。
6. **Skill の部品**: 今の `search`（275 行目。範囲をパスで開き直す）を、開いた `&ScopeRoot` と範囲の実のパスを受け取る形に分け、プラグインの根の記述子から `skills` と各 Skill を開く。範囲にはプラグインの根の実のパスを使う（根の外を指すリンクは範囲の外として断り、根の中を指すものは受け付ける）。プラグインの根の記述子は、読み終えたら閉じる（`tests/user_skills.rs:614-670` の記述子の数の性質を保つ）。`skills/` がなければ、Skill が 0 個のプラグインとして読み、記録する。名前 `benitoite` の Skill は、プラグインの中でも読まない。
7. **重なりと上限**: プラグインの名前が重なれば、先に見つけたもの（利用者、次に根に近い側）を採り、後のものはプラグインごと飛ばして示す。プラグインの Skill の名前の重なりは今の規則（先に数えたものを採る）で、飛ばしたものは `skipped_skills` に入れる。上限（100 個、16 KiB）は、プラグインの Skill も同じ枠で数える。`[skills]` と `[plugins]` は互いに独立とする。
8. **`mcp.json`**: 根にあれば、開かずに存在だけを調べて示す。拡張のディレクトリは見に行かない。
9. **記録と表示**: どれも `#[serde(default, skip_serializing_if = ...)]` を付け、形式の版は上げない。`record.rs` に `RecordedPlugin { name, version: Option<String>, path（根の実のパス）, sha256, source: SkillSource }` を加え、`Header.plugins` を加える。`RecordedSkill` と `LoadedSkill` に `plugin: Option<String>` を加える。`UserSkills` に読んだプラグイン、断ったプラグイン、無視した部分の通知を加え、`Record::SessionStart`・`AgentEvent::SessionStart`・`AgentStatus` に通す（`agent/mod.rs:345・624-645`、`agent/events.rs:42-66・155-187`、`agent/status.rs`）。構造体の値を書いているテスト（`tests/session.rs`、`tests/support/rewind.rs`、`tests/tui.rs`、`src/ui/tui/tests.rs`）を直す。示すのは開始の通知だけ（exec は標準エラー出力 `src/ui/mod.rs:74-93`、TUI の通知、`/status`）で、プラグインの行を Skill の行の前に出す。文の形の例: `Plugin loaded: {name} {version|-} ({path}; sha256 {64 桁})`、`Plugin skipped: {path}: {reason}`、`Plugin notice: {path}: {reason}`、プラグインの Skill は `Skill loaded: {name} ({path}; plugin {plugin})`。doctor には出さない。
10. **理由の文の例**（`plugins/text.rs`）: 版 `plugin.json must set $schema to https://agent-plugins.org/schemas/1.0.0/plugin.schema.json; other Agent Plugins versions are not supported.`、JSON `plugin.json is not a valid JSON object.`、名前 `Plugin name must contain 1 to 64 lowercase ASCII letters, digits, hyphens, or dots, start and end with a letter or digit, and not contain -- or ...`、ディレクトリの名前 `Plugin name must match both the entry name and the real directory name.`、型 `plugin.json field {field} has an invalid type.`、知らない欄 `Ignored unknown plugin.json fields: {names}.`、extensions `Ignored plugin.json extensions because it is not an object.`、MCP `mcp.json is present; MCP servers from this plugin are not used.`、重なり `Another plugin with this name takes precedence.`、範囲 `Plugin path is outside the search scope.`、上限 `plugin.json exceeds the 256 KiB limit.`。
11. **設定と評価の道具**: `tests/config.rs` の誤りの表（117 行目）、既定値（271 行目）、700・712 行目、プロジェクトでの拒否の表（714-718 行目）に `[plugins]` を加える。プロジェクトの設定は `deny_unknown_fields` で `[plugins]` を断る。設計書 05-01 の設定の例は変えない（オーケストレータが取り込むときに加える）。`tools/skill-eval/skill_eval.py:193` の設定に `[plugins]` の `user = false` を加え、`test_skill_eval.py` の 301・343 行目の期待と `SOURCE.md` の末尾を直す。
12. **テスト**: `tests/user_skills.rs` に `#[path = "user_skills/plugins.rs"] mod plugins;` を置き（`tests/exec.rs:5` の書き方）、見つけ方・検査の表・Skill・重なり・上限・`mcp.json` を確かめる。制限モードの `.bnt` は `tests/tools_check.rs:89-140` に倣う。exec の入口は `tests/exec/skills.rs:7` に利用者のプラグインを一つ加え、ヘッダの `plugins`、Skill の `plugin`、標準エラー出力の `Plugin loaded:` を確かめる。端末の文字の攻撃の確かめ（`tests/tui.rs:95-115`、`src/ui/tui/tests.rs`）にプラグインの行を加える。

13. **レビューを受けた追加**（2026-10-20）: 設計書 01-04「Agent Plugins のプラグイン」の「長さと数の上限」を実装する（`version` の文字と長さ、段ごとのプラグインの数 100、知らない欄の名前の切り詰め、上限を越える Skill を開かずに飛ばしてまとめて示す）。版は表示の行でパスより後に置き、`user`・`project` の出所を示す。理由の文は置換でなく、プラグイン用の文を直に選ぶ。テストは一時の根を TMPDIR の下に置く（`/tmp` に固定しない）。プロジェクトのプラグインの根が範囲の外を指す場合、`skills/` が根の外を指す場合、プラグインの `SKILL.md` が Skill のディレクトリの外を指す場合のテストを加える。

14. **再レビューを受けた追加**（2026-10-20）: 設計書 01-04「Agent Plugins のプラグイン」の「長さと数の上限」と「記録と表示」に加えた点を実装する。表示の行の形は、13 の「版はパスより後に置く」を、下の形に改める。
    - プラグインの Skill の候補は、段ごとに、すべてのプラグインを合わせて 1,000 個までを開く。規則に合わない候補と、同名で負けた候補も数える。超えた分は開かず、段ごとに一行で数を示す。
    - 飛ばした Skill（`skipped_skills`）と断ったプラグイン（`skipped_plugins`）は、段ごとに、利用者・プロジェクトの Skill の分も合わせて 100 件までを一件ずつ示して記録に書く。残りは段ごとに一行で数を示し、記録にも数を書く。上限を越える Skill をディレクトリごとにまとめる今の一行（補足 13）は、この 100 件に一件として数えてよい。
    - 表示の行の形を `Plugin loaded: {name} ({user|project}; sha256 {hex}; version {version}; {path})` とする。版がないときは、`version {version}` の代わりに `no version` と書く。プラグインの Skill の行は `Skill loaded: {name} (plugin {plugin}; {path})` とし、パスを最後に置く。TUI の開始の通知と `/status` も同じ文にする。
    - `.agents/plugins` の項目のうち、`plugin.json` を読む前に断ったもの（範囲の外へのリンク、祖先、UTF-8 でないパス）も、段ごとの 100 個のプラグインの数に入れる。
    - テスト:
      - (a) プロジェクトのプラグイン 100 個の `skills/` に、範囲の外を指すリンクを合わせて 1,000 を超える数だけ置く。それでも標準エラー出力と開始の記録が 256 KiB 以下に収まり、開始の記録が分けて置かれず、`sessions` の一覧に最初の依頼が出ること。
      - (b) 同名の有効な Skill を多数のプラグインに置く場合も、同じく収まること。
      - (c) 利用者の Skill `z000`〜`z004` に大きい説明を付け、16 KiB で `z004` を打ち切る。プロジェクトに小さい `z004` を置いたとき、プロジェクトの `z004` を読まないこと（`Collector::precut` が名前を覚えることのテスト）。
      - (d) `collection_keeps_later_candidates_with_a_256_descriptor_limit` は、理由の文でなく、深い側の `z599` が読んだ一覧にないことで確かめる。
      - (e) パスに `; sha256 0…0) version 9.9 (user; ` を含むプラグインの行で、出所・SHA-256・版がパスより前に出ること。
