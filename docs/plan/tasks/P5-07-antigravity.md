# P5-07 評価の道具に Antigravity の項目を整える

## 目的

設計者の決定（2026-10-18）により、Antigravity の CLI（`agy`）× Gemini 3.8 Flash を、P5 の比べる相手に加えた（設計書 07-02「比べる相手」）。評価の道具の既存の項目 `antigravity-gemini`（Benitoite の Skill の評価から引き継いだもの）を、P5 の記録の形と安全の方針に合わせて改める。

この作業で LLM を呼ばない。`agy` は、`--version` と `--help`（`agy help <サブコマンド>` を含む）だけを起動してよい。`agy -p`、`agy models`（ネットワークに問い合わせる）、対話の起動は行わない。

## 読む設計書

- [評価](../../design/07-quality/07-02-evaluation.md)（「比べる相手」の Antigravity の項、「測る項目」「評価の手順」）
- [P5-03](P5-03-records-and-cost.md)・[P5-04](P5-04-dsh-and-claude-code.md)（それぞれの補足を含む。行の欄、時間の上限、子の環境、外部のハーネスの集計の読み取り、利用者が加えた設定の確かめ）
- 既存のもの: `tools/skill-eval/`（`skill_eval.py`、`harnesses.json`、`test_skill_eval.py`、`SOURCE.md`、`README.md` の Antigravity の行）
- Antigravity の同梱の文書（読むだけ）: `~/.gemini/antigravity-cli/builtin/skills/agy-customizations/docs/`（`hooks.md`、`skills.md` ほか）。実行ファイルの中の変更の記録の写しが `/private/tmp/claude-501/-Users-tecogonaz-src-BenitoiteHarness/def9f39f-a672-4ff4-ab05-a3a7fe59fbfd/scratchpad/agy/changelog.txt` にある（以下の「L1.x.y」は、その中の版である）

## この作業が認める型の変更

なし（Rust のコードを変えない）。道具のコードは python3 3.9 で動く書き方にし、標準ライブラリだけを使う。

## 前提の事実（2026-10-18 に、オーケストレータの調べで、LLM を呼ばずに確かめた。`agy` 1.4.0）

- **出力**: `--output-format json` は一つのオブジェクトを書く。2026-10-17 の Benitoite の評価の記録（`harness.stdout`）では、鍵は `conversation_id`、`status`（`"SUCCESS"`）、`response`（最後の応答の文）、`duration_seconds`、`num_turns`（利用者の発言の数で、モデルへの要求の数ではない）、`usage`（`input_tokens`、`output_tokens`、`thinking_tokens`、`cache_read_tokens`、`total_tokens`）だった。`total_tokens = input_tokens + output_tokens` であり、`cache_read_tokens` は `input_tokens` より大きいので、`input_tokens` はキャッシュの読み取りを含まない。キャッシュの書き込みの欄はない。`thinking_tokens` が `output_tokens` に含まれるかは確かめていない。拒んだ操作は `denied_actions` に載る（L1.1.27）。誤りでは終了状態 3 で、標準エラー出力に `AGY_ERROR: {...}` の行を書き、JSON に途中までの応答を含める（L1.2.6、L1.2.10）。使い方の誤りは終了状態 2 である。
- **会話の記録**: `~/.gemini/antigravity-cli/brain/<conversation_id>/.system_generated/logs/transcript.jsonl` に、一行一件で、`type`（`USER_INPUT`・`PLANNER_RESPONSE`・`GENERIC`）、`step_index`、`tool_calls[].{name,args}`（例 `run_command` の `CommandLine`・`Cwd`）、手ごとの `input_tokens`・`output_tokens`・`cache_read_tokens` がある。10-17 の記録では、`PLANNER_RESPONSE` の 37 手の使用量の和が、標準出力の `usage` と一致した。このため、`PLANNER_RESPONSE` の数がモデルへの要求の数（番）に当たる。
- **利用者の設定**（2026-10-18 の状態）: `~/.gemini/config/config.json` の根の鍵は `userSettings` だけ、`mcp_config.json` は空（0 バイト）、`projects/` に 4 つの JSON。`~/.gemini` の下と `~` に、`GEMINI.md`・`AGENTS.md`・`skills/`・`plugins/`・`rules/`・`hooks.json` はない。`~/.gemini/antigravity-cli/settings.json` の鍵は `model`・`trustedWorkspaces`。同梱のスキルは `~/.gemini/antigravity-cli/builtin/skills/` に 9 個ある（製品が標準で持つもの）。`~/.agents/skills` の 3 個は、Antigravity が読まない見込みである（10-17 の会話の記録に名前が現れなかった）。設定の別の置き場所を指す、文書にある方法はない。ログインはキーチェーン（keyring）による。
- **権限**: 非対話の既定は `request-review` で、確認を要する操作は待たずに断られ、`denied_actions` に載る（L1.1.3、L1.1.27）。`--mode accept-edits` はファイルの編集を自動で許し、コマンドは確認を要する（非対話では断られる）。`--mode` は `-p` でも効く（L1.1.12）。`--dangerously-skip-permissions` はすべてを許す（L1.3.2）。
- **フック**: 作業ディレクトリの `.agents/hooks.json` の `PreToolUse` は、`allow`・`deny`・`ask`・`force_ask` を返せる（`hooks.md`）。入力は `toolCall.name` と `args.CommandLine` などを持つ。フックは `sh -c` で、`hooks.json` のあるディレクトリを作業ディレクトリとして起動される。
- **時間の上限**: `--print-timeout`（既定 0 は無制限。L1.2.6）。上限に達すると、途中までの出力を書き、標準エラー出力に警告を書いて、終了状態 0 で終わる（L1.1.28）。番の数の上限の選択肢はない。SIGTERM での振る舞いは文書にない。
- **課金**: まずプランの利用枠を使い、設定 `useG1Credits` で有効にしたときだけ追加のクレジットを使う。残高が足りなければ止まる（L1.0.3、L1.2.13、L1.2.15）。今の設定に `useG1Credits` はない。
- **自動の更新**: 起動のたびに、背後で更新を確かめるプロセスが起動する。環境変数 `AGY_CLI_DISABLE_AUTO_UPDATE` が実行ファイルの中にある（文書にはない）。

## 作るもの

### 1. 起動の形

既存の項目 `antigravity-gemini` を次のとおり改める。

- `start` は `["agy", "-p", "{prompt}", "--model", "{model}", "--effort", "{effort}", "--mode", "accept-edits", "--output-format", "json", "--print-timeout", "1800s"]` とする（`--dangerously-skip-permissions` を除く）。`model` は `gemini-3.8-flash-high`、`options` は `{"effort": "high"}` のまま。
- `options` に `harness_timeout` を `1860` とする（`--print-timeout` より 60 秒長くし、`agy` が自分で止まるのを先にする）。
- `output` を `agy-json`、`billing` を `none`（プランの利用枠のため）とし、`price` は置かない。`verified` は、起動の形を変えたので `false` にする。
- 子の環境に `AGY_CLI_DISABLE_AUTO_UPDATE=1` と `TMPDIR=<試行>/tmp`（作る）を加える。

### 2. フック

- 作業ディレクトリに `.agents/hooks.json` を書く（今の `files` の書き方を改める）。二つのフックを置く。
  - **ウェブを拒む**: `PreToolUse`、`matcher` は `search_web|read_url.*|.*browser.*|retrieve_content|internal_search|mcp_tool|tool_search|shell_exec|run_extension_code|send_command_input|invoke_subagent`、決まった `deny` を返す。
  - **`benitoite` だけを許す**: `PreToolUse`、`matcher` は `run_command`。コマンドは `python3 -I <試行>/agy/agy_gate.py`（作業ディレクトリの外の写し。パスは道具が絶対のパスで書く）とする。
- `tools/skill-eval/agy_gate.py` を作り、試行ごとに `<試行>/agy/` へ写す。標準入力のフックの入力（JSON）を読み、次のすべてを満たすときだけ `allow` を、ほかは `deny`（理由の文を添える）を返す。入力の形は `hooks.md` に合わせる。
  - 道具の名前が `run_command` である。
  - `CommandLine` が、正規表現 `^benitoite( [A-Za-z0-9_./=:,+-]+)*$` に全体で当たる（シェルの特別な文字、引用符、空白の連続、改行を含まない）。
  - `Cwd` が、作業ディレクトリ（`hooks.json` の親の親）の実のパスか、その下である。
  - 読めない入力は `deny` とする。
- 作業ディレクトリの `.agents/` の下のファイル（`hooks.json` と Skill）の編集を拒むフックは、道具の名前が文書で確かめられないので、この作業では置かない。代わりに、試行の後に、`.agents/hooks.json` が書いたときと同じ内容であることを確かめ、違えば理由の先頭に `SAFETY: hooks.json was modified` を加える。
- 確かめられていないこと（フックの `allow` が非対話の既定の「断る」に優先するか、`matcher` が全体の一致か）は、`SOURCE.md` に書き、試しの走行で最初に確かめることとする。

### 3. 集計の読み取り（`agy-json`）

- 標準出力の最後の空でない行を JSON として読む（`conversation_id`・`status`・`response`・`duration_seconds`・`usage` を持つオブジェクト）。
- トークンの量: `input_tokens` は `usage.input_tokens + usage.cache_read_tokens`（キャッシュを含む量に揃える）、`cache_read_tokens` は `usage.cache_read_tokens`、`cache_write_tokens` は 0、`output_tokens` は `usage.output_tokens`、`total_tokens` は `input_tokens + output_tokens` とする。`usage.thinking_tokens` は `thinking_tokens`（新しい欄）に書き、合計には足さない。
- 番の数: `conversation_id` の会話の記録（前提の事実のパス）を読み、`type` が `PLANNER_RESPONSE` の行の数を `turns` とし、その `tool_calls[].name` を数えて `tool_calls` とする。`conversation_id` は、英数字・`-`・`_` だけからなるときだけ使い、`brain/` の下に収まることを `realpath` で確かめる。記録が読めなければ `turns` と `tool_calls` を `None` とする。手ごとの使用量の和が `usage` と違えば、理由に `transcript usage differs` を添える（使用量は `usage` を正とする）。
- 拒んだ操作: `denied_actions` の数を `denied_actions`（新しい欄）に書く。
- 最後の応答: `response` を `<試行>/final.txt` に書く。
- `end`: 時間の上限（道具の側）なら `timeout`。標準エラー出力に `--print-timeout` の警告があれば `timeout`（警告の文は `agy` の実行ファイルの中の文字列で確かめて定数にし、出典をコメントに書く）。`status` が `SUCCESS` で終了状態 0 なら `completed`。終了状態 3 か `AGY_ERROR` の行があれば `error`（`AGY_ERROR` の JSON の先頭 200 文字を理由に添える）。ほかは `error`。
- 集計が読めないときは、P5-04 の補足 11 と同じく、評価を止めず、判定は包みのコマンドの記録と書かれたスクリプトで通常どおり行い、`end` とトークンの欄を `None` とし、理由に `; harness summary unreadable` を添える。

### 4. 走らせる前の確かめ

試行を始める前に、次のどれかに当たれば誤りとし、名前（内容でなく）を示す（設計書 07-02 の、利用者が加えた設定を外す決定）。`--dry-run` では確かめない。

- `~/.gemini/` の下と `~` の `GEMINI.md`・`AGENTS.md`。
- `~/.gemini/config/` の下の `skills/`・`plugins/`・`rules/`・`hooks.json`。
- `~/.gemini/config/mcp_config.json` が空でも `{}` でもなく、MCP のサーバの項目を持つ。
- `~/.gemini/config/config.json` と `~/.gemini/antigravity-cli/settings.json` のどこかに、真の `useG1Credits` がある（追加のクレジットで課金させない）。
- 作業ディレクトリの祖先（`/` まで）の `GEMINI.md`・`AGENTS.md` と `.agents/` のディレクトリ。

同梱のスキル（`~/.gemini/antigravity-cli/builtin/skills/`）の名前の一覧を見出しに書く。

### 5. 文書

- `tools/skill-eval/SOURCE.md` に、Antigravity の項目の走らせ方と、起動の形を改めた理由（`--dangerously-skip-permissions` を使わない）、フック、確かめられていないこと、課金の扱いを書く。「写した後に変えた点」に P5-07 の節を作る。
- 写した元の `README.md` の Antigravity の行は変えない。

## 触るファイル

- `tools/skill-eval/skill_eval.py`、`tools/skill-eval/harnesses.json`、`tools/skill-eval/agy_gate.py`（新規）、`tools/skill-eval/test_skill_eval.py`、`tools/skill-eval/SOURCE.md`

`tasks/`、`README.md`、`bin/benitoite` は変えない。

## テスト

どのテストも `agy` を起動しない（決まった出力を書く偽の実行ファイルか、出力の文字列で確かめる）。

- `agy_gate.py`: `benitoite check main.bnt`、`benitoite run --allow-effects=Console.Write,File.Read main.bnt` を許すこと。`benitoite check main.bnt; rm -rf ~`、`benitoite run $(cat x)`、引用符・改行・二つの空白を含むもの、`ls`、作業ディレクトリの外の `Cwd`、`run_command` でない道具、読めない入力を拒むこと。
- `agy-json` の読み取り: 入力へのキャッシュの足し方、`thinking_tokens` を合計に足さないこと、会話の記録からの `turns` と `tool_calls`、記録がないときの `None`、`conversation_id` の形とパスの確かめ、`denied_actions`、`end` の各場合（完了、`AGY_ERROR`、`--print-timeout` の警告）。
- 走らせる前の確かめ: 偽のホーム（`HOME` を差し替えた試し）で、4 の各項目が誤りになること、どれもないときに通ること。
- フック: 作業ディレクトリに書いた `hooks.json` の形と、`agy_gate.py` の写しのパス。`hooks.json` を書き換えた試行で `SAFETY: hooks.json was modified` が付くこと。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、`unittest` の結果の末尾と、偽の出力から作った行の欄の例を書く。
