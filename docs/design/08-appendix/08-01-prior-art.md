# 先行事例

本付録は、試作の設計で参考にした三つのハーネス（Codex CLI、Pi Coding Agent、DeepSeek Harness）について、確かめた事実と、試作に採ったもの・採らなかったものをまとめる。事実は、各リポジトリの次のコミットのソースを 2026-10-16 に読んで確かめた（浅い複製で読み、ビルドも実行もしていない）。二次の資料（記事など）による事実は、そのことを明記する。

| ハーネス | リポジトリ | コミット | 言語 |
|---|---|---|---|
| Codex CLI | <https://github.com/openai/codex> | `a45b8ccb016fb74f63a6bc29e701568e387cbe38`（2026-10-16。最新のリリースは `rust-v0.163.0`） | Rust |
| Pi Coding Agent | <https://github.com/earendil-works/pi>（`packages/coding-agent`） | `4954d760504c11df9b2cbdc040346107b6a8153d`（2026-10-15。パッケージの版は 1.3.0） | TypeScript |
| DeepSeek Harness | <https://github.com/deepseek-ai/deepseek-harness> | `86a402200d95fecef4529d969836f1fe13c6cb0d`（2026-10-15。`0.2.1-alpha.4`） | TypeScript |

`https://github.com/badlogic/pi-mono` は `earendil-works/pi` へ転送され、同じ最新のコミットを返すので、同じリポジトリが移ったものである。

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
