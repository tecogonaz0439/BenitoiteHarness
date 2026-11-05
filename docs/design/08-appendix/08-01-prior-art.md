# 先行事例

本付録は、試作の設計で参考にした三つのハーネス（Codex CLI、Pi Coding Agent、DeepSeek Harness）について、確かめた事実と、試作に採ったもの・採らなかったものをまとめる。事実は、各リポジトリの次のコミットのソースを 2026-10-16 に読んで確かめた（浅い複製で読み、ビルドも実行もしていない）。二次の資料（記事など）による事実は、そのことを明記する。

| ハーネス | リポジトリ | コミット | 言語 |
|---|---|---|---|
| Codex CLI | <https://github.com/openai/codex> | `a45b8ccb016fb74f63a6bc29e701568e387cbe38`（2026-10-16。最新のリリースは `rust-v0.163.0`） | Rust |
| Pi Coding Agent | <https://github.com/earendil-works/pi>（`packages/coding-agent`） | `4954d760504c11df9b2cbdc040346107b6a8153d`（2026-10-15。パッケージの版は 1.3.0） | TypeScript |
| DeepSeek Harness | <https://github.com/deepseek-ai/deepseek-harness> | `86a402200d95fecef4529d969836f1fe13c6cb0d`（2026-10-15。`0.2.1-alpha.4`） | TypeScript |

`https://github.com/badlogic/pi-mono` は `earendil-works/pi` へ転送され、同じ最新のコミットを返すので、同じリポジトリが移ったものである。

後述の「圧縮の作りの比較」は、2026-11-05 に、別のコミットと別のハーネスを加えて調べたものである。

## Codex CLI

### 規模と作り

- 約 150 のクレート、約 208 万行の Rust から成る。一つの実行ファイル `codex` が、自分を補助として起動し直して、Linux のサンドボックスの補助や `apply_patch` の働きもする。
- リリースのビルドの設定は `lto = "thin"`、`codegen-units = 4` で、`panic = "abort"` も大きさ優先の `opt-level` も使わない（`codex-rs/Cargo.toml`）。リリースの配布物の大きさは、aarch64 の macOS 向けが zstd で 71 MB、tar.gz で 100 MB、x86_64 の Linux（musl）向けが zstd で 81 MB である（GitHub のリリースの API、`0.163.0`）。圧縮しない実行ファイルの大きさは確かめていない。
- V8、SQLite、Starlark、OpenTelemetry を含む。調べたコミットでは、多くのモデルが「コードのモード」で動き、モデルが書いた JavaScript を V8 で動かして、その中から道具を呼ぶ。
- 調べたコミットでは、Chat Completions の通信の形、`shell`・`read_file`・`list_dir`・`grep_files` の道具、安全なコマンドの許可の一覧がない。記事の多くが説明する作りとは違う。

### エージェントのループ

- 要求は Responses API に `store: false`、`stream: true`、`parallel_tool_calls: true`、暗号化した推論の内容を含め、`prompt_cache_key` にスレッドの ID を入れて送る。
- 道具の呼び出しは、応答の項目を受け取り終えた時点で実行を始める。並べてよい道具は読み取りのロックを、`apply_patch` などは書き込みのロックをとる。結果は呼び出しの順に会話に加え、結果のない呼び出しには「取り消した」の結果を補う。
- 要求のやり直しは 4 回、ストリームのやり直しは 5 回で、間隔は 200 ms から倍にする。SSE で次のデータが届かない時間の上限は 300 秒である。番の数の上限はない。

### 道具

- `exec_command` は一つのコマンドの文字列を `[shell, -lc, cmd]` で動かす。環境を空にしてから `NO_COLOR=1`・`TERM=dumb`・`PAGER=cat` などを設定し、標準入力は `/dev/null` にする。
- `apply_patch` は、Lark の文法（`codex-rs/core/assets/tools/apply_patch.lark`）で形を限った自由形式の道具であり、`*** Begin Patch`・`*** Update File:`・`@@` などの行で変更を表す。照合は、完全一致、行末の空白を除いたもの、前後の空白を除いたもの、Unicode の句読点を寄せたもの、の四段で行う。
- 道具の出力は、先頭と末尾を残して中ほどを除く一つの規則で切り詰め、モデルに渡すのは約 1 万トークンまでとする。

### サンドボックスと承認

- macOS では `/usr/bin/sandbox-exec` を固定のパスで起動し、`(deny default)` から始める方針を使う。書ける場所は `-D WRITABLE_ROOT_i=<path>` の引数で渡し、方針の文に埋め込まない。書ける場所の中でも `.git`・`.codex`・`.agents`・`.aws` を守る。方針は `process-exec` を広く許す（`codex-rs/sandboxing/src/seatbelt.rs`）。
- Linux では bubblewrap で名前空間とファイルシステムの見え方を作り、その中で自分を起動し直して `no_new_privs` と seccomp を掛ける。seccomp は、ネットワークを使えないときに Unix ドメイン以外のソケットの作成と接続・待ち受けの呼び出しを拒否し、`ptrace` と `io_uring` を常に拒否する（`codex-rs/linux-sandbox/src/seccomp.rs`）。Landlock は古い経路として残り、自分のアプリのサーバの Unix ソケットを隔てられないことを理由に bubblewrap に移った。書ける場所の中の守る場所の作り（まだない `.git` を塞ぐ形など）は、2026-10-17 に新しいコミットで調べ直した（[Linux のサンドボックスの検討の記録](08-04-linux-sandbox.md)）。
- 承認の方針は `untrusted`・`on-request`（既定）・`granular`・`never` である。モデルは道具の引数（`sandbox_permissions` と `justification`）でサンドボックスの外での実行を求め、利用者が承認する。
- サンドボックスに拒否されたかは、終了状態と出力の言葉（`operation not permitted` など）と、Linux の `SIGSYS` で判定する（`codex-rs/sandboxing/src/denial.rs`）。
- コマンドの規則（Starlark）で、利用者が許した前方一致の規則を保存できるが、解釈系と起動の道具（`bash`、`sh -c`、`node -e`、`env` など）の規則は保存させない（`codex-rs/core/src/exec_policy.rs` の `BANNED_PREFIX_SUGGESTIONS`）。

### 文脈と記録

- 基本の指示は約 21 KB である（`codex-rs/protocol/src/prompts/base_instructions/default.md`）。
- `AGENTS.md` は、リポジトリの根から作業ディレクトリまでを辿って読み、合計 32 KiB までとする。各ディレクトリでは、`AGENTS.override.md`、`AGENTS.md`、設定の `project_doc_fallback_filenames` で加えた名前の順に見て、最初に見つかった一つだけを読む（`codex-rs/core/src/agents_md.rs`）。
- 文脈が上限の 90% に達したら圧縮する。トークンの量は、最後の API の報告に、その後に加えた分のバイト数を 4 で割った値を足して見積もる。
- セッションは JSON Lines のファイル（`~/.codex/sessions/YYYY/MM/DD/rollout-<時刻>-<ID>.jsonl`）に記録する。
- プロジェクトの設定は信頼したディレクトリでだけ読み、提供者の URL などを設定させない。
- `codex exec` は、進み具合を標準エラー出力に、最後の応答を標準出力に書き、`--json` で JSON Lines のイベントを出す。

## Pi Coding Agent

### 考え方と規模

- 作者の記事（Mario Zechner、2025-11-30、<https://mariozechner.at/posts/2025-11-30-pi-coding-agent/>）は、四つの道具、MCP なし、圧縮なしの小さなハーネスを説明し、エージェントとして学習したモデルに長いシステムプロンプトは要らないとする。調べたコミットでは、MCP、自動の圧縮、セッションの木、約 35 の提供者などが加わっているが、システムプロンプトと四つの既定の道具とループの核は小さいままである。
- 記事は Terminal-Bench 2.0 で評価したと書くが、点数は画像にあり、確かめていない。

### システムプロンプトと道具

- システムプロンプトは、前置き（「You are an expert coding assistant operating inside pi, a coding agent harness. ...」）、`<tools>`、`<rules>` などの節から成り、核の部分は約 1.6 KB（約 400 トークン）である。日付と OS は入れないが、作業ディレクトリ（`<cwd>`）と `AGENTS.md` の中身（`<project_context>`）は入れる（`packages/coding-agent/src/core/system-prompt.ts`）。
- 各道具が自分の一行の説明と規則を持ち、有効な道具の分だけがプロンプトに入る。
- 既定の道具は `read`・`bash`・`edit`・`write` である。出力の上限は 2,000 行か 50 KiB の先に達した方で、`read` は先頭を、`bash` は末尾を残し、続きの読み方か全体を書いた一時ファイルのパスを結果に書く（`packages/coding-agent/src/core/tools/truncate.ts`）。
- `edit` は一つのファイルに複数の置き換えを受け、各置き換えを元のファイルに対して照合する。完全一致で見つからなければ、行末の空白・引用符・ダッシュ・特殊な空白を寄せた形で照合し、置き換えなかった行は元のバイト列のまま残す。改行の形と BOM を保つ。誤りの文は、次に何をすればよいかを含む。
- `read` は、先頭のバイトで画像を判定し、縮めて（既定は 2000x2000 画素、base64 で 4.5 MB まで）画像として返す。Chat Completions の口では、道具の結果に画像を入れられないので、道具の結果の後に `role: "user"` のメッセージで画像を送る（badlogic/pi-mono のコミット `1402973a`、`packages/coding-agent/src/core/tools/read.ts`、`packages/ai/src/api/openai-completions.ts`。2026-10-19 に読んだ）。
- 引数の配列を JSON の文字列にして送るモデルがあることがソースのコメントに記され、その形を直す処理を持つ。
- `bash` は既定の時間の上限を持たず、環境の変数を除かない（API キーもコマンドから見える）。

### ループと文脈

- 道具の呼び出しは既定で並べて実行し、結果は呼び出しの順に加える。出力の上限で打ち切られた応答の道具の呼び出しは実行せず、引数が切れている可能性を示す誤りを返す。
- 実行中の入力は、操縦（今の道具の実行の後に加える）と後続（一連の処理の後に加える）として扱う。
- やり直しは 3 回、2 秒から倍にし、上限は 60 秒である。番の数の上限はない。
- 圧縮は、文脈の上限から予備（16,384 トークン）を引いた量を超えたら行い、新しい側の約 2 万トークンを残す。要約は、目的・制約・進み具合・決めたこと・次にすること・重要な文脈の見出しの型板で作り、読んだファイルと変えたファイルの一覧をハーネスが加える。
- 別の提供者やモデルの推論の内容は送らない。道具の呼び出しの ID の形を揃える。

### 安全

- サンドボックスを持たず、作業ディレクトリも境界ではない。コンテナや仮想機械の中で動かすことを勧める。

## DeepSeek Harness

### 規模と作り

- DeepSeek が公開した、Cordis のプラグインの枠組みの上の TypeScript のハーネスである。約 40 万行（テストを除く）から成り、開発者向けの試用版であり、`SAFETY.md` は監査を受けていないとする。

### 評価の構成

- `BENCHMARK.md` が指す最小の構成（`packages/bundle/sdk-minimal/cordis.patch.yml`）は、保ったシェルの `bash` の道具一つだけを持ち、システムプロンプトは「You are a helpful software engineer assistant.」の一文で、圧縮もファイルの道具も持たない。設計の記録は、この構成を学習の時の約束（Claude の SWE の形）と呼んでいる。
- コマンドの出力は先頭の 16,000 文字を残し、すべての結果の末尾に終了状態を書く。
- モデルの公表した成績（Terminal-Bench 2.1 など）がこの構成で測られたとする記述は、第三者が写したモデルの説明の文書にあり、DeepSeek の一次の資料では確かめていない。同じモデルを別のハーネスで測ると点が変わったとする第三者の報告もあるが、確かめていない。

### 製品の構成

- システムプロンプトは約 5.8 KB で、変えずに保ち、時刻やサンドボックスの方針などの変わる情報を、変わったときに利用者の側の発言として加える。道具の一覧は、計画のモードでも変えず、実行の側で拒否する（キャッシュを保つため）。
- システムプロンプトと道具の定義の全文を、期待する出力のファイルでテストに固定している。
- `read` は行の番号を付け、2,000 行、一行 2,000 文字、50 KiB までを返す。`edit` は `old_string`・`new_string`・`replace_all` を持つ。ファイルを読んでから直す規則を、ファイルの版（デバイス、inode、大きさ、時刻）で確かめる（`packages/fs/fs-observation-policy`）。
- 並べて実行するのは、並べてよいと明示した道具（`read` など）だけで、ほかは区切りとして扱う。同じ呼び出しを 3・5・8 回繰り返したときに注意を加える。
- 圧縮は、文脈の上限の 80% などで行い、新しい側の 16% を残し、8 つの見出しの型板で要約する。要約の要求は、前の会話をそのまま送ってキャッシュを効かせる（`packages/compaction/compaction-basic`）。
- DeepSeek のモデルには Anthropic 互換の口（`https://api.deepseek.com/anthropic/v1/messages`）を使い、`cache_control` は使わない（キャッシュは自動）。
- サンドボックスは、Linux で bubblewrap か Landlock、macOS で Seatbelt を使い、掛けられないときは実行を拒む。限るのはファイルの書き込みで、ネットワークは限らない。
- DeepSeek の口への要求に、既定で有効な欄（`dsh_session_log`）で、セッションの記録（依頼、道具の引数と結果、推論、作業ディレクトリを含みうる）を送る（`docs/deepseek-llm-api-wire-extensions.md`）。

## 圧縮の作りの比較

2026-11-05 に、BH-014 の評価の準備（[P5-36](../../plan/tasks/P5-36-compaction-boundaries.md)）のために、道具の呼び出しが続く途中の圧縮で、ほかのハーネスが何を残すかを調べた。オーケストレータが、補助のエージェントに次のソースと公式の文書を読ませ、本節に書く値と関数の名前を、手元の複製のソース（Gemini CLI は版を固定した URL のソース）で確かめた。ビルドも実行もしていない。

| ハーネス | 読んだもの |
|---|---|
| Codex CLI | <https://github.com/openai/codex> のコミット `c7234c1174b27d9c55199fba0d546dc27d1b0415`（2026-10-21）の `codex-rs/` |
| Pi Coding Agent | <https://github.com/earendil-works/pi> のコミット `16f5f0af4d85ca583d045ceb5ed6d646fcaf7761`（2026-10-21）の `packages/coding-agent` |
| DeepSeek Harness | <https://github.com/deepseek-ai/deepseek-harness> のコミット `22ddee3792402c3e70ec5f86a63c3e3e895a2f60` の `packages/compaction` |
| OpenCode | <https://github.com/anomalyco/opencode> のコミット `4d5965762ec5e0d1a0b14630553fc62b51a11fba`（2026-10-21）の `packages/core` |
| Hermes Agent | <https://github.com/nousresearch/hermes-agent> のコミット `278aeffe16945e7b1405e3c471ad08553c059237`（2026-10-21） |
| Gemini CLI | <https://github.com/google-gemini/gemini-cli> のコミット `1fe22af5f800d8c3024e9968d35bb75bd09e6b79`（2026-11-04）の `packages/core/src` |
| Claude Code | 公式の文書（<https://code.claude.com/docs/en/context-window>、<https://code.claude.com/docs/en/settings-reference>）。ソースは公開されていない |

### 途中の圧縮と残す範囲

Claude Code を除くどのハーネスも、道具の結果を加えた後、次のモデルの応答を求める前に、圧縮するかを確かめる。見積もりは、直前の提供者の使用量に、その後に加えた分の文字数かバイト数を 4 で割った値を足す形が多い。残す範囲の決め方は、ハーネスによって次のように違う。

- **Codex CLI**: 番の途中の圧縮（`core/src/session/turn.rs` の `CompactionPhase::MidTurn`）でも、手元で要約する形は、直近の利用者の発言だけを 20,000 トークンまで残し、道具の呼び出しと結果を残さない（`core/src/compact.rs` の `COMPACT_USER_MESSAGE_MAX_TOKENS`・`build_compacted_history`）。進行中の番は丸ごと要約に回る。圧縮を始める量は、文脈の上限の 90% と設定の小さい方である（`protocol/src/openai_models.rs` の `auto_compact_token_limit`）。
- **Pi**: 新しい側の `keepRecentTokens`（既定 20,000）を残し、予備の `reserveTokens` は既定 16,384 である（`src/core/compaction/compaction.ts`）。区切りは利用者か応答の発言に置き、道具の結果には置かない。番の途中で区切るときは、区切られた番の前半を別に要約する（`isSplitTurn`、`TURN_PREFIX_SUMMARIZATION_PROMPT`）。最後の道具の結果だけで残す量を超えるときは、その道具を呼んだ応答で区切り、呼び出しと結果を、残す量を超えても残す（`test/compaction.test.ts` の「should fall back to the latest valid cut point before oversized trailing tool results」）。
- **DeepSeek Harness**: 新しい側の一定の割合（既定は `retainRatio` の 0.16）を残す（`compaction-basic/src/config.ts`）。区切りは、道具の呼び出しと結果の組を割らない位置まで古い側へずらす（`compaction-basic/tests/compaction-basic.spec.ts` の「rounds a retention cut head-ward to preserve tool-call/result pairing」）。要約できる範囲がなければ、圧縮しない（`compaction-basic/src/index.ts` の `selectCompactableRange` が範囲を返さないとき）。
- **OpenCode**: 残す量は既定 15,000 トークンである（`packages/core/src/session/compaction.ts`）。残す範囲の始まりを、その前の最後の利用者の発言まで戻し、道具の呼び出しと結果を一続きに残す（同じファイルの `recentStart`）。前の圧縮の後のやり取りが一つだけのときは、何も残さずに要約する。残した会話は文に直して送り、道具の結果は 1,250 文字に切る（`packages/core/src/session/recent-text.ts` の `TOOL_OUTPUT_MAX_CHARS`）。
- **Hermes Agent**: 残す量は 10,000〜25,000 トークンで、文脈の上限の 20% を超えない（`agent/context_compressor.py` の `LEAN_TAIL_FLOOR_TOKENS`・`LEAN_TAIL_CAP_TOKENS`・`TAIL_MAX_CONTEXT_FRACTION`）。進行中の番だけで残す範囲を超えるときは、番の中の道具の区切りで分けて新しい側の道具の組を残し、元の依頼を要約の後に置き直す（同じファイルの `split_oversized_turn`、`_reappend_inflight_user_task`）。モデルがまだ読んでいない最後の道具の結果は、文脈の上限の 20% 以下なら残す（`_spared_pending_tool_round`）。
- **Gemini CLI**: 文脈の上限の 50% で圧縮し、履歴の後ろの 30% を文字数で数えて残す（`packages/core/src/context/chatCompressionService.ts` の `DEFAULT_COMPRESSION_TOKEN_THRESHOLD`・`COMPRESSION_PRESERVE_THRESHOLD`）。残す範囲は利用者の依頼から始め、関数の呼び出しと応答の組を割らない（同じファイルの `findCompressSplitPoint`）。圧縮した後の量が前より多ければ、圧縮の結果を捨てる。
- **Claude Code**: 文書は、文脈の上限に近づくと自動で圧縮し、圧縮の後に、指示のファイルと、直近に読むか書いたファイル 5 つまでを読み直すと書く（context-window）。会話のどれだけを残すかは書いていない。

### 道具の結果を縮める作り

- **DeepSeek Harness**: 圧縮すると判定したら、要約の前に、8,192 文字を超える道具の結果を頭の 4,096 文字と尾の 1,024 文字に縮めて量を測り直し、境を下回れば要約しない（`compaction-tool-result-pruner/src/config.ts`、`compaction-basic/src/index.ts`）。
- **Hermes Agent**: 要約の前に、残す範囲より古く 200 文字を超える道具の結果を、一行の要約に替える（`agent/context_compressor.py` の `_prune_old_tool_results`・`_summarize_tool_result`・`_PRUNE_MIN_CHARS`）。一度に返す道具の結果の合計を 200,000 文字に限り、超えた分をファイルに置く（`tools/budget_config.py` の `DEFAULT_TURN_BUDGET_CHARS`）。並べて動かす道具は 8 本までである（`agent/tool_executor.py` の `_MAX_TOOL_WORKERS`）。
- **Gemini CLI**: 新しい側の道具の出力（50,000 トークン分）を守り、それより古い出力を頭と尾だけにして、全体をファイルに置く（`packages/core/src/context/toolOutputMaskingService.ts` の `DEFAULT_TOOL_PROTECTION_THRESHOLD`）。
- **結果ごとの上限**: Codex CLI は、今のモデルでは結果ごとに 10,000 トークンまでとし、頭と尾を残す（`codex-rs/models-manager/models.json` の `truncation_policy`）。Pi と OpenCode は 2,000 行・50 KiB までとする（Pi の `src/core/tools/truncate.ts`、OpenCode の `packages/core/src/tool-output.ts`）。
- **Claude の API**: 古い道具の結果から順に消す機能（`clear_tool_uses_20250919`）を持つ（<https://platform.claude.com/docs/en/build-with-claude/context-editing>）。Claude Code がこれを使うかは、文書に書かれていない。

### 圧縮の値の評価

圧縮の方針を比べる評価を持つのは、調べた中では Hermes Agent だけだった（`evals/compaction/`）。実際の長い会話の記録 4 本（約 50 万トークン）を方針ごとに圧縮し、記録ごとに 15 問の想起の試験を LLM に解かせる。2026-08-15 の結果（`evals/compaction/results/SCORECARD-2026-08-15.md`）では、残す範囲を 25,000 トークンに抑え、圧縮した範囲を検索で引き直せるようにした方針（lean+recovery）が、平均 68.3% を約 49,000 トークンで、それまでの方針が平均 45.8% を約 162,000 トークンで答えた。この方針は残す量のほかにも複数の変更を含むので、残す量だけの効果ではない。ほかのハーネスで見つかったのは、圧縮の振る舞いの単体と結合の試験であり、値を比べる評価は見つからなかった。DeepSeek Harness の `compaction-basic/README.md` は、既定の比率（0.8 と 0.16）について、資料に基づく指針を記録していないと書く。

### 試作との関係

試作は、Anthropic の形の口で道具の呼び出しが続く途中に圧縮するとき、残す範囲を設けない（[文脈の管理](../01-agent/01-04-context.md)の「圧縮」、BH-036）。Codex CLI の手元の要約と、OpenCode の一つだけのやり取りも、この時点で道具の結果を残さない。Pi と Hermes Agent は番の途中で区切って新しい側を残し、DeepSeek Harness と Hermes Agent は要約の前に道具の結果を縮める。これらを、P5-36 と BH-014 の選択肢の材料とする。

## 試作に採ったもの

| 事柄 | 元 | 試作の章 |
|---|---|---|
| 学習で見慣れた道具の形（`read`・`edit` の `old_string`・`bash`） | DeepSeek Harness、Pi | 01-03 |
| 道具ごとの説明と規則から作る短いシステムプロンプト | Pi | 01-04 |
| 変わる情報をシステムプロンプトに入れない。会話を追記だけで伸ばす | DeepSeek Harness、Codex CLI（Pi は日付と OS を入れない） | 01-04 |
| 出力の上限（2,000 行か 50 KiB）と、続きの読み方を示す文、全体を書く一時ファイル | Pi | 01-03 |
| 置き換えの照合の寄せ方と誤りの文 | Pi、Codex CLI | 01-03 |
| 読んでから直す規則 | DeepSeek Harness | 01-03 |
| 並べてよい道具だけを並べ、結果を呼び出しの順に加える。結果のない呼び出しを補う | DeepSeek Harness、Codex CLI、Pi | 01-01 |
| 打ち切られた応答の道具を実行しない | Pi | 01-01 |
| 操縦と後続 | Pi | 01-01 |
| 同じ呼び出しの繰り返しへの注意 | DeepSeek Harness | 01-01 |
| 圧縮の時機、残す量、要約の型板、ファイルの一覧 | Pi、DeepSeek Harness、Codex CLI | 01-04 |
| JSON Lines のセッションの記録と再開 | 三つすべて | 01-05 |
| Seatbelt の方針を `-D` の引数で作る、`sandbox-exec` のパスの固定、`.git` を守る | Codex CLI | 03-02 |
| 自分を補助として起動し直してサンドボックスを掛ける、seccomp のネットワークの規則 | Codex CLI | 03-02 |
| Linux で bubblewrap のマウントで守る場所と読ませない場所を表す。書ける場所の根の直下のまだない守る名前を、読み取りだけの空のディレクトリで塞ぐ | Codex CLI（08-04） | 03-02 |
| 承認の方針の名前、道具の引数による範囲の外の実行の要求、拒否の判定 | Codex CLI | 03-02、03-03 |
| 解釈系と起動の道具の一覧 | Codex CLI | 03-02 |
| `exec` の標準出力と標準エラー出力の分け方、JSON Lines のイベント | Codex CLI | 05-01 |
| プロジェクトの設定で提供者の URL を変えさせない | Codex CLI（と Benitoite の事前検討） | 05-01 |
| LLM に見せる文の全文のテスト | DeepSeek Harness | 07-01 |

## 試作が採らなかったもの

| 事柄 | 元 | 理由 |
|---|---|---|
| 非同期の実行時、V8、SQLite、Starlark | Codex CLI | 実行ファイルの大きさ（06-01） |
| `apply_patch` の形式 | Codex CLI | 置き換えの形（`edit`）に揃え、道具の形を一つにする。どちらが性能の上でよいかは評価で確かめうる |
| 保ったシェル | DeepSeek Harness | 呼び出しごとに新しいシェルにする方が、状態が単純である（BH-005） |
| サンドボックスなし | Pi | 制限モードの保証に要る（03-01） |
| bubblewrap の同梱 | Codex CLI | 試作の間は配布物を作らず、利用者が導入したものを使う（03-02、08-04） |
| 番の数の上限を持たないこと | 三つすべて | 利用料を上限なく増やさない（01-01） |
| セッションの記録を提供者へ送ること | DeepSeek Harness | 利用者に知らせずに送る内容を増やさない |
| 下位のエージェント、MCP、拡張の仕組み | Codex CLI、DeepSeek Harness、Pi の後の版 | 範囲の外（00-01） |
