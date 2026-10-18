# P5-06 評価で決める選択肢を切り替える口

## 目的

設計書 07-02「評価で決める選択肢」のうち、S の段で比べる書いた後の検査（BH-004）と effort の既定（BH-034）、M の段で比べる `read` の行の番号（BH-003）を、評価の道具から一つずつ切り替えて走らせられるようにする。effort は既存の設定（`providers.<名前>.effort`）で切り替えられるので、`benitoiteh` に加えるのは次の二つの設定である。

- `restricted.after_write_check`（既定 `true`）: 書いた後の検査を止める。
- `read.line_numbers`（既定 `true`）: `read` の結果に行の番号を付けない。

評価の道具には、項目ごとに設定を足す口と、比べるための項目を加える。この作業で本物の提供者を呼ばない。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「read」「書いた後の検査（制限モード）」（どちらも、評価のための切り替えの【方針】を 2026-10-18 に加えた）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」「プロジェクトの設定ファイル」
- [評価](../../design/07-quality/07-02-evaluation.md)の「評価で決める選択肢」「評価の規模と費用の上限」
- 既存のもの: `crates/benitoiteh/src/config/mod.rs`（`RestrictedConfig`、`ReadConfig`）、`src/tools/read.rs`、`src/tools/definitions.rs`、`prompts/tools/read.md`、書いた後の検査を行う箇所（`src/agent/execution.rs` ほか。`after_write_checks` を数える箇所）、`tests/` の設定・`read`・書いた後の検査のテスト、`tools/skill-eval/`（`skill_eval.py`、`harnesses.json`、`test_skill_eval.py`、`SOURCE.md`）

## この作業が認める型の変更

- `RestrictedConfig` に `after_write_check: bool`（既定 `true`）、`ReadConfig` に `line_numbers: bool`（既定 `true`）を加える。どちらも利用者の設定ファイルだけで受け付け、プロジェクトの設定ファイル（05-01「プロジェクトの設定ファイル」）では受け付けない（知らない欄として誤りにする今の扱いのまま）。理由: プロジェクトのファイルで道具の振る舞いを変えさせない。
- 道具の定義を組み立てる関数に、`read` の説明の文を選ぶ引数を加えてよい。

## 作るもの

### 1. `restricted.after_write_check`

- `false` のときは、制限モードの `write`・`edit` で `.bnt` を書いた後の検査を行わず、結果に検査の結果を添えない。集計の `after_write_checks` は 0 になる。ほかの振る舞い（読んでから直す規則、書ける場所、巻き戻しの記録）は変えない。
- 通常モードには、もともと書いた後の検査がないので、影響しない。
- `doctor` と `/status` の出力は変えない。セッションの記録のヘッダに、この値を書く（既定と違うときだけでよい。欄の名前は `after_write_check`）。

### 2. `read.line_numbers`

- `false` のときは、`read` の結果の各行の先頭に、行の番号と区切りのタブを付けない。続きの印（`[Showing lines A-B of N. Use offset=K to continue.]`）、切り詰めの印、`offset` と `limit` の意味は変えない。
- 道具の説明の文を `prompts/tools/read-plain.md` に替える。文は次のとおりとする（`read.md` の一行目だけを替える）。

  ```text
  Read a text file.
  Use offset and limit to read a range; follow the continuation marker for more lines. Use grep to inspect truncated long lines.
  ```

- LLM に見せる文の固定（設計書 07-01）のテストに、`read-plain.md` の文を加える。道具の定義のハッシュ（`tools_sha256`）が、`false` のときに変わることを確かめる。
- セッションの記録のヘッダに、この値を書く（既定と違うときだけでよい。欄の名前は `read_line_numbers`）。

### 3. 評価の道具の設定の口

- `harnesses.json` の `kind: benitoiteh` の項目に、`config`（表の名前から鍵と値への辞書。例 `{"restricted": {"after_write_check": false}}`）を置けるようにする。道具は、試行の設定ファイルの同じ名前の表に鍵を足す（`[restricted]` の `allowed_programs` は残す）。表の名前と鍵は、`read`・`restricted` と、その下の `after_write_check`・`line_numbers` だけを受け付け、ほかは誤りとする。値は真偽だけを受け付ける。
- 記録の見出しに、項目の `config` を書く。
- 次の項目を加える。どれも `benitoiteh-deepseek` の写しで、違う所だけを書く。`verified` は `false`、`default` は `false` とする。

  | 項目の名前 | 違う所 |
  |---|---|
  | `benitoiteh-deepseek-no-after-write-check` | `config` が `{"restricted": {"after_write_check": false}}` |
  | `benitoiteh-deepseek-no-line-numbers` | `config` が `{"read": {"line_numbers": false}}` |
  | `benitoiteh-deepseek-effort-low` | `effort` が `low` |
  | `benitoiteh-deepseek-effort-max` | `effort` が `max` |

- `SOURCE.md` の使い方と「写した後に変えた点」に書く。

## 触るファイル

- `crates/benitoiteh/src/config/mod.rs` と、その読み取りのテスト
- `crates/benitoiteh/src/tools/read.rs`、`src/tools/definitions.rs`、`prompts/tools/read-plain.md`（新規）
- 書いた後の検査を行う箇所（`src/agent/execution.rs` ほか）
- セッションの記録のヘッダ（`src/session/record.rs`）
- `crates/benitoiteh/tests/` の該当のテスト
- `tools/skill-eval/skill_eval.py`、`harnesses.json`、`test_skill_eval.py`、`SOURCE.md`

## テスト

- 設定: 二つの鍵の既定が `true` であること、利用者の設定で `false` を受け付けること、プロジェクトの設定ファイルでは知らない欄として誤りになること。
- `after_write_check = false`: 制限モードで `.bnt` を `write` したときに、結果に検査の結果が添えられず、処理系の `test --check` が起動されず、集計の `after_write_checks` が 0 であること（模擬の提供者と、起動を数える処理系の偽物か、本物の処理系の記録で確かめる）。既定では今のとおり添えられること。
- `line_numbers = false`: 結果に行の番号がないこと、続きの印と切り詰めの印が今と同じであること、道具の説明の文が `read-plain.md` であること、`tools_sha256` が変わること。
- 評価の道具: `config` が設定ファイルに入ること、許さない表・鍵・値が誤りになること、四つの項目が読めること。
- 模擬の提供者による 11 課題の通し（P5-02）は、今のまま通ること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、二つの設定の既定と `false` のときの振る舞いの例（`read` の結果の先頭の数行と、`write` の結果）を書く。

## 補足（2026-10-18、始める前の点検）

本節は、上の各節に優先する。

1. **設定の型**: `ReadConfig { exclude }` は、利用者の設定・プロジェクトの設定（`ProjectConfig.read`）・`ExcludeRules`・`ResolvedExec.read` が共有している。`line_numbers` を `ReadConfig` に加えると、プロジェクトの設定ファイルの `read.line_numbers` を受け付けてしまう。`ReadConfig` は変えず、利用者の設定の `read` の型として `UserReadConfig { exclude: Vec<String>, line_numbers: bool }`（`deny_unknown_fields`）を加える。`resolve_exec` は、利用者とプロジェクトの `exclude` から今のとおり `ReadConfig { exclude }` を作り、新しい `ResolvedExec.read_line_numbers` を設定する。`restricted` は `ProjectConfig` にないので、`after_write_check` はプロジェクトの設定ファイルで今のとおり誤りになる。
2. **既定の値**: `RestrictedConfig` と新しい `UserReadConfig` の `Default` は、`Limits` と同じく手で書き、二つの真偽の既定を `true` にする（`#[derive(Default)]` では `false` になる）。serde の欄の既定も `true` にする。
3. **認める型の変更**（「この作業が認める型の変更」に加える）:
   - `ResolvedExec.read_line_numbers: bool`。
   - `ToolContext` の `pub(crate) after_write_check: bool` と `pub(crate) read_line_numbers: bool`。`ToolContext::new` では `true` とし、組み立ての関数 `with_eval_switches(after_write_check, read_line_numbers)` を加え、`cli/exec.rs` の準備の処理が利用者の設定と `ResolvedExec` から呼ぶ。
   - `ToolSet` の組み立ての関数（例 `ToolSet::plain_read(self)`。`read` の説明を `text::READ_PLAIN` に替える）。`Agent::new`（`agent/mod.rs`）で `!tool_context.read_line_numbers` のときに使い、`ToolSet::new(mode)` の形は変えない。
   - `Header` の二つの欄（7）。
4. **システムプロンプトのハッシュも変わる**: `read` の要約はシステムプロンプトにも現れる（`tests/expected/system-restricted.txt` の `read: Read a text file with line numbers.`）。`line_numbers = false` のときは、`system_prompt_sha256` と `tools_sha256` の両方が変わる。`tests/expected/system-restricted-read-plain.txt` と `tools-restricted-read-plain.json` を加え、`tests/context.rs` と `examples/update_context_expected.rs` に同じ場合を加える。システムプロンプトの中の要約は `read-plain.md` の一行目（`Read a text file.`）になる。
5. **長い行の切り詰め**: `read.rs` の 2,000 文字の切り詰めと `long_line` の印は、`LineStyle::Numbered` のときだけ働く。`LineStyle::Plain` を加え、行の番号を付けない（`text::numbered` を通さない）ことのほかは `Numbered` と同じに振る舞わせる。`benitoite_reference` は `Reference` のままとする。
6. **書いた後の検査を止める箇所**: `!context.after_write_check` のときは、`benitoite_check::after_write` の先頭で `output` をそのまま返す。`Not checked automatically: …` の注記も加えない。処理系を起動しないので、`after_write_checks` と `checks` は増えない。
7. **セッションの記録のヘッダ**: `Header` に `#[serde(default, skip_serializing_if = "Option::is_none")] pub after_write_check: Option<bool>` と `read_line_numbers: Option<bool>` を加え、`false` のときだけ `Some(false)` を書く。形式の版（`FORMAT_VERSION`）は 1 のままとする（`Header` は知らない欄を拒まないので、欄を加えても前の記録を読める）。再開は、そのときの利用者の設定を使い、ヘッダは開始のときの値を残す（ヘッダは書き直さない。設計書 01-05）。テストの `Header` の組み立てに新しい欄を加える。
8. **評価の道具**:
   - `config` は、`benitoiteh_config` でなく `load_harnesses` で確かめる（`--dry-run` は `benitoiteh_config` を呼ばないので、綴りの誤りが有料の走行まで見つからない）。`kind` が `benitoiteh` でない項目の `config` は誤りとする。値は `type(v) is bool` のときだけ受け付ける。
   - 生成する設定には今 `[read]` の表がないので、`[read]` の表を新しく書く。`[restricted]` には、`allowed_programs` の後に鍵の名前の順で加える。`toml_value` の辞書（行内の表）の形では書かない。
   - 記録の見出しには、`json.dumps(config, sort_keys=True)` か、ないときは `-` を書く。
9. **effort の値**: DeepSeek の `/anthropic` の口が `output_config.effort` の `low`・`max` を受け付けて効かせるかは確かめていない（`verified: false`）。評価で最初に確かめることとして、`SOURCE.md` に書く。比べる回数の割り振りは、走らせる前に設計者に諮る（この作業では決めない）。
10. **テストに加えるもの**:
    - プロジェクトの設定ファイルの `[read] line_numbers = false` が誤りになること。
    - 新しい欄のないヘッダを読めること、既定の走行ではどちらの鍵も書かないこと。
    - `tests/agent_loop/checks.rs` の偽の `query(...)` で起動を数え、切り替えを止めたときに `write` でも `edit` でも 0 回であること、一時ディレクトリの `.bnt` に `Not checked…` の注記が付かないこと。
    - 通常モードが影響を受けないこと。
    - 行の番号のない `read` でも `[line N truncated…]` の印と `screen_lines` が残ること。
