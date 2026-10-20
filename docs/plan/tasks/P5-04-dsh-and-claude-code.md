# P5-04 評価の道具に DeepSeek Harness と Claude Code の項目を整える

## 目的

P5 の比べる相手のうち、外部のハーネス二つ（DeepSeek Harness × DeepSeek-V4.1-Flash、Claude Code × Opus 5.5）を、評価の道具で走らせて記録できるようにする（設計書 07-02「比べる相手」）。どちらも、包みのコマンド（`bin/benitoite`）で検査と修正の回数を数える既存の流れを使い、それに番の数、トークンの量、時間、終わり方の読み取りを加える。

この作業で本物の提供者を呼ばない。DeepSeek Harness と Claude Code も、LLM を呼ぶ形では起動しない（`--version` と `--help` だけは起動してよい）。

## 読む設計書

- [評価](../../design/07-quality/07-02-evaluation.md)（特に「比べる相手」の DeepSeek Harness と Claude Code の項、「評価の規模と費用の上限」「測る項目」「評価の手順」）
- [P5-02](P5-02-benitoiteh-harness.md)・[P5-03](P5-03-records-and-cost.md)（行の欄、記録、費用の上限、時間の上限、子の環境）
- 既存のもの: `tools/skill-eval/`（`skill_eval.py`、`harnesses.json`、`test_skill_eval.py`、`SOURCE.md`、`bin/benitoite`）
- DeepSeek Harness の手元の作業ツリー（コミット `687f282e`、`0.2.1-rc.2`。場所はオーケストレータが示す。読むだけ。変えない）: `packages/bundle/headless/README.md`、`packages/bundle/headless/src/json-stream.ts`、`packages/bundle/base/cordis.patch.yml`、`packages/sandbox/sandbox/src/roots.ts`、`packages/subprocess/subprocess/src/index.ts`、`packages/skill/skill-filesystem/README.md`、`packages/context/agent-instructions/src/files.ts`

## この作業が認める型の変更

なし（Rust のコードを変えない）。道具のコードは python3 3.9 で動く書き方にし、標準ライブラリだけを使う。

## 前提の事実（2026-10-18 に、オーケストレータの調べで、ソースを読んで確かめた。DeepSeek Harness は API を呼ばずに `--version`・`--dump-config`・`--profile headless --help` だけを起動した）

**DeepSeek Harness**

- 大域のコマンドはない。組み立て済みの `node <DeepSeek Harness のリポジトリ>/apps/cli/lib/bin.js` で起動する（どの作業ディレクトリからでも動く。`pnpm dsh` は作業ツリーの中でしか動かない）。版は `--version` で `0.2.1-rc.2` を返す。
- 非対話の形は `bin.js --profile headless [--patch <file>] --json <依頼>` である。起動の選択肢（`--profile`、`--patch`）は `--json` と依頼より前に置く。依頼を果たして静かになるまで動き、子のエージェントを待って終わる。終了状態は、最後の番が完了すれば 0、取り消し・誤り・番なしなら 1 である。SIGTERM では終了状態 0、SIGINT では 130 で終わるので、時間の上限で止めたことは終了状態で判断しない（P5-03 の `timed_out`）。
- 確認に答える部品を持たないので、確認を要する操作は拒まれる。既定のサンドボックス（`workspace-write`）では、`write`・`edit` の道具とシェルのコマンドは、作業ディレクトリ（`cwd`）、`/tmp`、`os.tmpdir()`（`TMPDIR`）の下にだけ書ける。ほかへの書き込みは拒まれる。読み取りは限らない。ネットワークは限らない。
- `--json` は、一行一件の JSON（NDJSON）を標準出力に書く。種類は `session`、`status`（`phase` が `turn_start`・`step_start`・`step_end`・`turn_end`）、`thinking`、`text`、`tool_call`、`tool_result`、`final`（`text`）、`error` である。`step_end` の `usage` に `inputTokens`（キャッシュを含まない）、`outputTokens`、`cacheReadTokens`、`cacheWriteTokens`（省かれうる `totalTokens`）がある。一つの `step_end` が、モデルへの一回の要求に当たる。親のセッションの分だけで、子のエージェントの分は含まない。文字列は、`final` を除き 8 KiB で、一行は 32 KiB で切られる。
- モデルと推論の深さは、設定の重ね書き（`--patch` の YAML）の行 `agent-default-model` で決める（`{provider: deepseek-official, model: deepseek-flash, reasoningEffort: high}`）。値は `off`・`low`・`high`・`max` で、既定は `high` である。
- 重ね書きで止めるもの: `tool-web`（`web_search`・`web_fetch`）、`session-log-deepseek`（`config: {enabled: false}`。要求にセッションの記録を含める欄。既定で有効）、`plugin-package-inventory-deepseek`（導入した部品の一覧を送る欄）、`session-title-llm`（会話の題を作る補助の LLM の呼び出し）。オーケストレータが確かめた重ね書きの例は次のとおりである（`--dump-config` と `--help` の起動で読み込めることを確かめた）。

  ```yaml
  - id: agent-default-model
    config:
      provider: deepseek-official
      model: deepseek-flash
      reasoningEffort: high
  - id: session-log-deepseek
    config:
      enabled: false
  - id: plugin-package-inventory-deepseek
    disabled: true
  - id: tool-web
    disabled: true
  - id: session-title-llm
    disabled: true
  ```

- 状態と利用者の設定は `DSH_HOME`（既定 `~/.dsh`）と `DSH_AGENTS_HOME`（既定 `~/.agents`。`AGENTS.md` と `skills/`）の下にある。試行ごとに空のディレクトリを指せば、利用者の設定・スキル・MCP のサーバを読まない。`DSH_TELEMETRY_DISABLED=1` で遠隔の計測を止める。
- API のキーは、起動の環境の `DEEPSEEK_API_KEY`、作業ディレクトリの `.env`、`$DSH_HOME/.env` の順に読む。キーチェーンは読まない。シェルの子には、名前が `/KEY|PASSWORD|SECRET|TOKEN/i` に当たる変数と `DSH_*` の変数を渡さない（`PATH`・`HOME`・`SKILL_EVAL_LOG` などは渡す）。作業ディレクトリの `.env` に `DSH_*` があると起動を止める。
- プロジェクトの Skill は `<プロジェクトの根>/.agents/skills/<名前>/SKILL.md` と `.dsh/skills/` から読む（`.claude/skills` は読まない）。プロジェクトの根は、`.git` を持つ最も近い祖先か、なければ作業ディレクトリである。その間の `AGENTS.md`・`CLAUDE.md` も読む。
- 番の数とトークンの量の上限を持たない。

**Claude Code**（手元の 2.1.302）

- `harnesses.json` の既存の項目 `claude-code` の起動の形を使う（`claude -p <依頼> --model claude-opus-5-5 --effort medium --output-format json --permission-mode acceptEdits --allowedTools Bash(benitoite:*) Read Write Edit Glob Grep Skill --disallowedTools WebFetch WebSearch --strict-mcp-config`）。
- `--output-format json` の結果のオブジェクトは、`type: "result"`、`subtype`（`success`・`error_max_turns`・`error_during_execution` ほか）、`is_error`、`num_turns`、`duration_ms`、`total_cost_usd`（購読では見かけの値）、`usage`（`input_tokens`（キャッシュを含まない）、`output_tokens`、`cache_creation_input_tokens`、`cache_read_input_tokens`）、`modelUsage`（モデルごとの `inputTokens`・`outputTokens`・`cacheReadInputTokens`・`cacheCreationInputTokens`）、`permission_denials` を持つ（実行ファイルの中の検査の定義から読んだ。起動しては確かめていない）。
- 非対話（`-p`）では、`ANTHROPIC_API_KEY` があれば必ずそれを使い、Console のクレジットに課金する。P5-03 の子の環境の整理が `ANTHROPIC*` を除くので、普段のログイン（購読）で動く。
- 番の数の上限 `--max-turns` は `--help` に載っていない（実行ファイルの中に選択肢の名前はある）。この作業では使わない。

## 作るもの

### 1. 外部のハーネスの集計の読み取り

`harnesses.json` の項目に、出力の読み方を表す `"output": "dsh-ndjson"` か `"output": "claude-json"` を置けるようにする。ない項目（`codex` など）は今のまま読まない。読んだ値は、P5-02・P5-03 の行の欄に入れる。トークンの量は、`benitoiteh` の欄と同じく、入力をキャッシュを含む量に揃える。

- `dsh-ndjson`: 標準出力の各行を JSON として読む（読めない行は飛ばす）。
  - `turns` は `status` の `turn_start` の数、`steps`（新しい欄）は `step_end` の数、`tool_calls` は `tool_call` の `tool` ごとの数とする。
  - トークンの量は、すべての `step_end` の `usage` を足す。`input_tokens` は `inputTokens + cacheReadTokens + cacheWriteTokens`、`cache_read_tokens` は `cacheReadTokens`、`cache_write_tokens` は `cacheWriteTokens`、`output_tokens` は `outputTokens`、`total_tokens` は `input_tokens + output_tokens` とする。省かれた欄は 0 と数え、`usage` のない `step_end` があれば、その数を `usage_missing`（新しい欄）に書く。
  - 最後の応答の文は `final` の `text` とし、`<試行>/final.txt` に書く。
  - `end`: 時間の上限で止めたら `timeout`（P5-03）。終了状態が 0 で、最後の `turn_end` の `reason` が完了を表せば `completed`、そうでなければ `error` とし、`reason` と最後の `error` の文の先頭 200 文字を理由に添える。完了を表す `reason` の値は、`json-stream.ts` と headless の README で確かめて定数に写し、写した元のパスをコメントに書く。
  - 子のエージェントの使用量が含まれないことを、記録の見出しに書く。
- `claude-json`: 標準出力の最後の空でない行を JSON として読み、`type` が `result` のときだけ使う。
  - `turns` は `num_turns` とする。`tool_calls` は取れないので `None` とする。
  - トークンの量は、`modelUsage` があれば全モデルの和、なければ `usage` とする。`input_tokens` は `inputTokens + cacheReadInputTokens + cacheCreationInputTokens`（`usage` では `input_tokens + cache_read_input_tokens + cache_creation_input_tokens`）とし、ほかの欄も同じ対応で入れる。
  - `end` は、時間の上限なら `timeout`、`subtype` が `success` で `is_error` が偽なら `completed`、`error_max_turns` なら `max_turns`、ほかは `error`（`subtype` を理由に添える）とする。
  - `total_cost_usd` は、購読では請求の額でないので、`cost_usd` に入れず、`claude_reported_cost_usd`（新しい欄）に書く。
  - 結果のオブジェクトの `result`（最後の応答の文）を `<試行>/final.txt` に書く。

どちらも、集計が読めないとき（結果の行がない、形が違う）は、P5-03 の 7 と同じく、その試行だけを失敗にせず、トークンの欄を `None` として記録し、理由に `harness summary unreadable` を添える（判定は、包みのコマンドの記録と書かれたスクリプトで今のとおり行う）。

### 2. DeepSeek Harness の項目

`harnesses.json` に、項目 `deepseek-harness` を加える。

- キーチェーンの既定のサービス名は `benitoiteh-deepseek` とする。変えるときは、`harnesses.json` の該当する項目の `api_key.command` を直す。
- `agent` は `codex`、`skill_parent` は `.agents/skills`、`model` は `deepseek-flash`、`options` は `{"effort": "high"}`、`output` は `dsh-ndjson`、`price` と `billing` は P5-03 の `benitoiteh-deepseek` と同じ（`billing` は `deepseek`）、`verified` は `false`、`default` は `false` とする。
- `start` は `["node", "<DeepSeek Harness のリポジトリ>/apps/cli/lib/bin.js", "--profile", "headless", "--patch", "{trial}/dsh-eval-overlay.yml", "--json", "{prompt}"]`、`cwd` は `{workspace}`、`version` は `["node", "<DeepSeek Harness のリポジトリ>/apps/cli/lib/bin.js", "--version"]` とする。ここでは実行ファイルの場所を記号で示す。現在の `harnesses.json` には手元の絶対パスが入っており、ほかの機械では書き換える。この扱いは `SOURCE.md` に理由（大域のコマンドがない）とともに書く。`{trial}` は、型板に新しく加える置き換え（試行のディレクトリ）である。
- 試行の準備（`run_trial`。項目に新しい `"prepare": "dsh"` を置き、それで分ける）:
  - `<試行>/dsh-eval-overlay.yml` に、前提の事実の重ね書きを書く（`reasoningEffort` は `options.effort`、`model` は `--model` か項目の `model`）。値は ASCII の英数字・`-`・`.` だけを受け付け、ほかは試行を始めずに誤りとする。
  - 環境: P5-03 の整理した環境に、`DSH_HOME=<試行>/dsh-home`、`DSH_AGENTS_HOME=<試行>/agents-home`（どちらも空のディレクトリを作る）、`DSH_TELEMETRY_DISABLED=1`、`TMPDIR=<試行>/tmp`（作る）を加える。
  - 包みのコマンドの記録 `SKILL_EVAL_LOG` を、`<試行>/tmp/benitoite.log` に置く（DeepSeek Harness のサンドボックスは、作業ディレクトリと `TMPDIR` の下にだけ書けるため）。判定と行の `log_exists` も、その場所を読む。`<試行>/bin/` と `<試行>/real/` は `TMPDIR` の外に残す（書けないので、LLM が包みのコマンドを書き換えられない）。
  - API のキー: 起動の直前に、`/usr/bin/security find-generic-password -s "benitoiteh-deepseek" -w` を起動して値を得て（標準出力の末尾の改行を除く）、子の環境の `DEEPSEEK_API_KEY` だけに入れる。値をファイル・記録・標準出力・誤りの文に書かない。得られなければ、値を含めずに `cannot read the DeepSeek key from the keychain (exit <n>)` として評価を止める。
  - 作業ディレクトリの祖先（`/` まで）に `.git` があれば、試行を始めずに誤りとする（DeepSeek Harness がその祖先をプロジェクトの根とし、上の `AGENTS.md` を読むため）。`--work-root` の既定（`tempfile.mkdtemp`）と、git リポジトリを祖先に持たない、利用者が指定した評価用の場所は当たらない。
- 試行の後、`<試行>/dsh-home` は消さずに残す（生の記録であり、リポジトリの外にある）。

### 3. Claude Code の項目

既存の項目 `claude-code` を次のとおり改める。

- `output` を `claude-json`、`billing` を `none`（購読のため）とし、`price` は置かない（`cost_usd` は `None`。`billing` が `none` なので、`--stop-cost` の「費用が分からない」の止め方にも当たらない）。`verified` は、起動の形を変えないので `true` のままとする。
- 起動の前に、利用者の設定のスキルの置き場所 `~/.claude/skills/` の直下に、`synced` 以外の項目（ディレクトリかリンク）が残っていれば、試行を始めずに誤りとし、その名前を示す（設計書 07-02「比べる相手」の、利用者が加えたスキルを外す決定。外すことと戻すことは、評価を行う者が評価の前後に行う。道具は利用者のファイルを動かさない）。`~/.claude/settings.json` の `enabledPlugins` に真の値があるときも、同じく誤りとする。
- 記録の見出しに、上の確かめを通ったことと、`~/.claude/skills/synced/` の Skill の名前の一覧（マニフェストの `name`。値だけを書き、ほかの欄は書かない）を書く。

### 4. 文書

- `tools/skill-eval/SOURCE.md` の「本リポジトリでの使い方」に、二つの項目の走らせ方を書く。Claude Code の項目の前後の手順（ほかの Claude Code のセッションを止めておくこと、利用者が加えたスキルを `~/.claude/skills/` から外へ動かし、評価の後に戻すこと）と、DeepSeek Harness の重ね書きで止めるものを書く。
- 「写した後に変えた点」に P5-04 の節を作る。

## 触るファイル

- `tools/skill-eval/skill_eval.py`、`tools/skill-eval/harnesses.json`、`tools/skill-eval/test_skill_eval.py`、`tools/skill-eval/SOURCE.md`

`tasks/`、`README.md`、`bin/benitoite` は変えない。

## テスト

`tools/skill-eval/test_skill_eval.py` に、少なくとも次を加える。どのテストも、DeepSeek Harness と Claude Code を起動しない（決まった出力を書く偽の実行ファイルか、出力の文字列で確かめる）。

- `dsh-ndjson` の読み取り: `step_end` の使用量の和と、入力へのキャッシュの足し方、`usage` のない `step_end` の数、`tool_call` の数、`final` の文、完了と誤りの `end`。
- `claude-json` の読み取り: `modelUsage` の和と `usage` への戻り、`subtype` ごとの `end`、`total_cost_usd` を `cost_usd` に入れないこと。
- DeepSeek Harness の準備: 重ね書きの中身、環境の `DSH_HOME`・`DSH_AGENTS_HOME`・`TMPDIR`・`SKILL_EVAL_LOG` の場所、祖先に `.git` があるときの誤り、許さない値の誤り。キーの読み取りは、`/usr/bin/security` の代わりに決まった値を返す偽のコマンドを差し替えて確かめ、その値が記録（Markdown、`.jsonl`）と `final.txt` と設定のファイルに現れないこと。
- Claude Code の確かめ: 偽のホーム（`HOME` を差し替えた試し）の `~/.claude/skills/` に `synced` 以外があるときの誤りと、`enabledPlugins` が真のときの誤り、どちらもないときに通ること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、`unittest` の結果の末尾と、二つの項目の行の欄の例（偽の出力から作ったもの）を書く。

## 補足（2026-10-18、始める前の点検）

本節は、上の各節に優先する。

1. **DeepSeek Harness の番の数**: 用語集の「番」（LLM への一回の要求と、その道具の実行）は、DeepSeek Harness の `step` に当たる。`turns` は `step_end` の数とする。`turn_start` の数（DeepSeek Harness の「turn」は一連の処理に当たる）は `dsh_turns`（新しい欄）に書く。`steps` の欄は作らない。
2. **終わり方**: `turn_end` の `reason` は文字列でなく、`{kind: completed|aborted|blocked|error|max-tokens|interrupted|forked, ...}` のオブジェクトである（`packages/core/session/src/types.ts` の `TurnEndReasonMap`）。番の中の失敗では `error` の行が出ず、`reason.error.{code,message}` だけが残り、`final` は空のことが多い（headless の README の Exit）。
   - `end` は、時間の上限なら `timeout` とする。終了状態が 0 で、最後の `turn_end` の `reason.kind` が `completed` なら `completed` とする。ほかは `error` とする。
   - `error` のときは、理由に `reason.kind` を添え、`kind` が `error` なら `reason.error.code: reason.error.message`（先頭 200 文字）も添える。`turn_end` がなければ、最後の `error` の行の `message`（先頭 200 文字）を添える。
3. **DeepSeek Harness の費用**: `output` を持つ項目は、トークンを読めたら `cost_usd` を P5-03 の `estimate_cost` で求める（`price` を持つとき）。`usage_missing` が 0 でなければ `cost_usd` を `None` とし、P5-03 の補足 5 の「費用が分からない」として止まる。
4. **受け継ぐ設定を除く**: DeepSeek Harness の子の環境から、名前が `DSH_`・`DEEPSEEK_` で始まる変数と `NODE_OPTIONS` を除いてから、2 の値を加える。理由: `DSH_PERMISSION_MODE=danger-full-access` はサンドボックスと確認を外し、`DSH_SYSTEM_PROMPT`・`DSH_TOOLS_MODE`・`DSH_BUNDLED_SKILL_DIR`・`DEEPSEEK_BASE_URL` なども振る舞いを変える。除いた名前（値は書かない）を見出しに書く。
5. **キーの値の漏れの確かめ**: LLM のシェルは、同じ利用者のプロセスの環境を `ps -E` で読める（macOS で確かめた。サンドボックスは止めない）。試行の後、`harness.stdout`・`harness.stderr`・`final.txt`・`dsh-home/` の下のファイル・作業ディレクトリの下のファイルを、キーの値のバイト列で探す。見つかれば、値を書かずに、理由の先頭に `SAFETY: DeepSeek key value found in <試行からの相対パス>` を加え、標準エラー出力にキーの差し替えを促す文を書く。記録（Markdown、`.jsonl`）には、その行より後に値が入らないことを、書き出しの前にも確かめる。
6. **200 万トークンの規則**（設計書 07-02「評価の規模と費用の上限」）: 外部の項目で `total_tokens` が 2000000 を超えた行は `over_token_cap` を真にし、理由に `over the 2M token cap` を添える。判定は変えない。
7. **子のエージェント**: `tool_call` の `tool` が `subagent` で始まる呼び出しの数を `subagent_calls` に書き、0 でなければ理由に `usage excludes subagents` を添える。
8. **重ね書きの書き方**: 値は `json.dumps` で引用して書く（YAML として正しい。YAML 1.1 では引用しない `off` が真偽になる）。effort は `{off, low, high, max}` のどれかであることを確かめ、モデルの名前は ASCII の英数字・`-`・`.` だけを受け付ける。
9. **キーを読むコマンド**: `deepseek-harness` の項目に、`benitoiteh-deepseek` と同じ形の `"api_key": {"command": ["/usr/bin/security", "find-generic-password", "-s", "benitoiteh-deepseek", "-w"]}` を置き、道具はそのコマンドを起動する（テストで差し替えられるようにする）。コマンドは P5-03 の整理した環境で起動し、一回の `evaluate` で一度だけ読む（試行ごとにキーチェーンを読まない）。空の出力は誤りとする。`--dry-run` では読まない。
10. **Claude Code の確かめの範囲**: 設定のディレクトリは、`CLAUDE_CONFIG_DIR` があればそれ、なければ `~/.claude` とする。次のどれかがあれば、試行を始めずに誤りとし、名前（内容でなく）を示す。設計書 07-02 の「利用者が自分で加えたもの」を外す決定と、「評価の手順」の、利用者の指示のファイルを読ませない定めに当たるものである。
    - `skills/` の直下の、`synced` 以外の項目（ディレクトリかリンク）（利用者が追加した Skill のディレクトリやリンク）。
    - `agents/`・`commands/`・`output-styles/` の下の項目（利用者が追加したエージェントなど）。
    - `CLAUDE.md`。
    - `settings.json` の `enabledPlugins` の真の値、`hooks`、`outputStyle`。
    - 作業ディレクトリの祖先（`/` まで）の `CLAUDE.md`・`CLAUDE.local.md`。
    - 同期された Skill の名前は、`skills/synced/*/manifest.json` の `skills[].name` から読み、見出しに書く。
    - `--dry-run` では確かめない。
11. **P5-03 との食い違いの直し**: 1 の「集計が読めないとき」は、P5-03 の 7（その試行を失敗にする）と違い、評価を止めず、試行の判定は包みのコマンドの記録と書かれたスクリプトで通常どおり行い、`end` を `None`、トークンの欄を `None` とし、理由に `; harness summary unreadable` を添える。
12. **小さな点**:
    - 外部の項目の型板の置き換えに、どの項目でも `trial`（試行のディレクトリ）を加える（`cwd` の型板も含む）。
    - `final.txt` の書き出しと、`end` が `completed` でないときの理由の添え書きは、`output` を持つ項目にも行う。
    - `.git` の祖先の確かめは `realpath` で行い、`ValueError` を投げる（`evaluate` が評価を止める）。テストでは、`tempfile.gettempdir()` 自身に `.git` の祖先があれば `skipTest` とする。
    - `SOURCE.md` に次を書く。評価の道具はサンドボックスの外で走らせる（入れ子の `sandbox-exec` は失敗し、DeepSeek Harness はシェルの呼び出しをすべて拒む）。`TMPDIR` の下の包みのコマンドの記録は、エージェントが書き換えうる（既知の制約）。Claude Code は、対話の記録を `~/.claude/projects/` に書く。
