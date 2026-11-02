# ほかのハーネスとサンドボックスの OSS との機能の比べとキャッチアップの案

本書は、2026-11-02 に、四つのエージェントハーネスと二つのサンドボックスの OSS のソースコードを読み、`benitoiteh` に足りない機能を洗い出した結果と、追い付くための案をまとめる。設計上の決定を加えるものではない。案の採否と順は設計者が決める。まとめは [README](README.md) にある。

## 対象と方法

| 対象 | リポジトリ | コミット | 言語 |
|---|---|---|---|
| Codex CLI | <https://github.com/openai/codex> | `3026a8e54e0f48c4d8074cd700f35b0697abc329`（2026-11-02） | Rust |
| Pi Coding Agent | <https://github.com/earendil-works/pi> | `752019da075163d72eb447cd9fe9b27d1fd072c7`（2026-11-02。パッケージの版は 1.5.0） | TypeScript |
| Hermes Agent | <https://github.com/NousResearch/hermes-agent> | `0aab6dcf8b7869acbab06507c34881b53036c09a`（2026-11-02） | Python、TypeScript |
| DeepSeek Harness | <https://github.com/deepseek-ai/deepseek-harness> | `ce7073d51254c26cd63b22a60835b8f948deb447`（2026-10-30。`0.2.2-alpha.2`） | TypeScript |
| nono | <https://github.com/nolabs-ai/nono> | `c657265f65457efbd0b4e9e2cf246bdeb45cbe22`（2026-11-02） | Rust |
| sandbox-runtime（`srt`） | <https://github.com/anthropic-experimental/sandbox-runtime> | `98a4016d27609ed34dd1fb43d23ed6efb6eeeb77`（2026-10-25。`0.0.81`） | TypeScript |

各リポジトリを浅い複製で取り、ソースと同梱の文書だけを読んだ。ビルドも実行もしていない。対象ごとに一つのサブエージェントが読み、オーケストレータが、挙がったパスのうち主なもの（各対象で 5〜11 件）がその場所にあることと、`benitoiteh` についての主張（子プロセスの環境、読ませない場所、ダンプの抑止、Landlock の `Refer`）を、ソースで確かめた。以下の出典のパスは各リポジトリの根からの相対パスであり、ソースから確かめていないものは「（未確認）」と書く。

`benitoiteh` の側は、ブランチ `claude/stoic-ride-otqoj5`（2026-11-02）の設計書とコードを基準とした。

## 総評

`benitoiteh` が強いところは次のとおりである。

- **OS のサンドボックス**: 6 つのどれよりも厳しい。Linux で、まだない `.git` と `.benitoiteh` も塞ぎ、Unix ドメインのソケットを既定で拒み、書ける場所での実行を拒み、掛けられないときは起動を止める。Pi は承認もサンドボックスも持たず（`packages/coding-agent/docs/security.md`）、Hermes はローカルの実行に OS のサンドボックスを持たない。DeepSeek Harness は書き込みだけを限り、通信を分けない（`packages/sandbox/sandbox-local/src/profiles.ts`）。nono は Linux で「書ける根の中の守る場所」を表せない（`crates/nono-cli/src/protected_paths.rs:74-110`）。srt は `.git` の大半を書けるままにする（`src/sandbox/sandbox-utils.ts:21-50`）。
- **LLM に承認させない**: Codex の `auto_review`（`codex-rs/protocol/src/config_types.rs:183-200`）、Hermes の既定の `smart`（`hermes_cli/config_defaults.py:1663`）、DeepSeek Harness の auto review（`packages/experimental/auto-review/README.md`）は、LLM が利用者の確認の代わりをする。`benitoiteh` は持たず、今後も持たない。
- **会話の木と巻き戻し**: DeepSeek Harness は持たず、Pi と同等である。
- **制限モード**: 6 つのどれにもない。

足りないところは、次の五つに分かれる。

1. **安全の仕組みの周り**:
   - 子プロセスに渡す環境から、秘密を除いていない（提供者のキーだけを除く。`crates/benitoiteh/src/config/environment.rs:11-23`）。
   - サンドボックスで読ませない場所が狭い（`~/.ssh`、`~/.gnupg`、`~/.aws`。`crates/benitoiteh/src/cli/mod.rs:134-136`）。
   - 通信をドメインで限れない（BH-046）。
   - 拒否の理由を正確に返せない。
2. **エージェントの性能**: 複数の箇所を一度に直す `edit`、圧縮の前の道具の結果の刈り込み、下のディレクトリの指示のファイル、計画の道具、背景の処理。
3. **対話の使い勝手**: 途中のモデルの切り替え、`@` によるファイルの指定、外部のエディタ、利用者の定義するコマンド、書き出し。
4. **ほかのプログラムとの連携**: MCP、ACP（エディタとの連携）、RPC。
5. **評価の方法**: 文言を A/B で決める測定、文書の有無で成功率を比べる評価。

## 機能の比べ

○ は持つ、△ は一部か試験的に持つ、× は持たない（ソースで見つからなかった）を表す。`benitoiteh` の列の「BH」は、未決事項として扱っていることを表す。

| 機能 | benitoiteh | Codex | Pi | Hermes | DSH | nono | srt |
|---|---|---|---|---|---|---|---|
| OS のサンドボックス（macOS・Linux） | ○ | ○ | × | △（docker などの裏方） | △（書き込みだけ） | ○ | ○ |
| 通信のドメインの許可の一覧（プロキシ） | ×（BH-046） | ○ | × | △（外部のプロキシ） | × | ○ | ○ |
| 子プロセスの環境からの秘密の除去 | △（提供者のキーだけ） | △（設定で除ける。既定の除外は既定で無効。`codex-rs/config/src/shell_environment_policy.rs:136`） | × | △（`execute_code` の子だけ） | （未確認） | ○ | ○ |
| 偽のトークンを本物に差し替えるプロキシ | × | ○ | × | ○ | × | ○ | ○ |
| サンドボックスの拒否の記録を LLM に返す | △（文字列の推定） | （未確認） | × | × | × | ○（`why`） | ○ |
| 計画・todo の道具 | ×（BH-008） | ○ | × | ○ | ○ | — | — |
| 一度に複数の箇所を直す `edit` | × | ○（`apply_patch`） | ○ | ○ | ○（任意） | — | — |
| 圧縮の前の道具の結果の刈り込み | × | △（残す出力の扱い。同じ働きかは未確認） | △（`context_edit`） | △（micro compaction） | ○ | — | — |
| 下のディレクトリの AGENTS.md | ×（根から作業のディレクトリまで） | （未確認） | （未確認） | ○ | ○ | — | — |
| 背景の処理・PTY | × | ○ | × | ○ | ○ | — | — |
| スクリプトから道具を呼ぶ | ×（BH-002） | ○（V8） | ○（QuickJS） | ○（Python） | ○（TS。既定は無効） | — | — |
| サブエージェント | × | ○ | △ | ○ | ○ | — | — |
| MCP のクライアント | ×（BH-116） | ○ | ○ | ○ | ○ | — | — |
| エディタとの連携（ACP・JSON-RPC） | × | ○ | ○（RPC） | ○（ACP） | ○（ACP） | — | — |
| 途中のモデルの切り替え | × | ○ | ○ | （未確認） | （未確認） | — | — |
| 利用者の定義する `/` のコマンド | × | ○ | ○ | ○ | （未確認） | — | — |
| プロジェクトの設定と Skill の信頼の確認 | × | △（フックの値の照合） | ○ | △（信頼するディレクトリの Skill だけ） | （未確認） | ○（署名） | — |
| OS のキーストア | ×（BH-033） | ○ | （未確認） | ○ | （未確認） | ○ | — |
| セッションの木・巻き戻し | ○ | △ | ○ | △ | × | △（スナップショット） | — |

## 安全の仕組みの不足

### 子プロセスの環境と読ませない場所

`benitoiteh` は、子プロセスの環境から提供者のキーと自分の印だけを除き、残りをそのまま渡す（`crates/benitoiteh/src/config/environment.rs:11-23`）。`GITHUB_TOKEN` や `AWS_SECRET_ACCESS_KEY` などは、サンドボックスの中の `bash` に届き、`env` を打てば LLM に見える。

- nono は、`allow_vars`・`deny_vars`・`set_vars` を持ち、`LD_PRELOAD`・`DYLD_INSERT_LIBRARIES`・`PYTHONPATH`・`NODE_OPTIONS` を必ず除く（`docs/cli/features/environment.mdx:176-200`、`crates/nono/src/keystore.rs:159`）。
- Codex は、継承・除外・含める名前・設定を選べる（`codex-rs/config/src/shell_environment_policy.rs:15-35`）。ただし、既定の除外は、設定しなければ使わない（同 136 行）。
- srt は、名前を指定して除く（`credentials.envVars` の `mode: "deny"`）。

読ませない場所も狭い。nono の既定の拒否の組は、クラウドと kube と docker の資格情報、`.netrc`、`.npmrc`、`.git-credentials`、`.vault-token`、シェルの設定と履歴、ブラウザのプロファイル、パスワードの保管庫を含む（`crates/nono-cli/data/policy.json`）。srt は `**/.env` のような glob の読ませない場所を、Linux では起動のときに展開して塞ぐ（`src/sandbox/read-deny-glob.ts`）。`benitoiteh` の道具は `.env*` を読まない（`crates/benitoiteh/src/tools/exclude.rs:170`）が、通常モードの `bash` は `cat .env` で読める。

### 通信のドメインの制限（BH-046、BH-040）

Codex、nono、srt は、いずれもホストの側の HTTP CONNECT（と SOCKS5）のプロキシにだけ通信を許し、プロキシでドメインを判定する。

- srt は、拒否の一覧、許可の一覧、厳格な許可の一覧、コマンドごとの一覧、問い合わせの呼び戻しの順で判定し、許した名前を一度だけ解決して、ループバック・リンクローカル・クラウドのメタデータ・ホスト自身のアドレスを拒んでからつなぐ（`src/sandbox/resolved-address-guard.ts`、`src/sandbox/sandbox-manager.ts:732-800`）。
- nono は、プロキシのセッションのトークン（256 ビット）で、ほかのローカルのプロセスがプロキシを使うことを防ぐ（`docs/cli/features/networking.mdx:33`）。また Linux で、ネットワークの名前空間の中で作ったループバックの待ち受けの記述子を、監督のプロセスへ渡す（`crates/nono/src/sandbox/linux_namespace.rs`）。
- Codex は、全体の `*` を拒み、許した名前でもプライベートのアドレスを拒む（`codex-rs/network-proxy/README.md`）。

`benitoiteh` の macOS の方針は、評価の判定のために `localhost` の一つのポートへの通信を許す要素を、すでに持つ（`PolicyConfig.allow_loopback_port`）。Linux は seccomp で `AF_UNIX` を拒むので、nono の方式（名前空間の中で作った待ち受けを、すでにある制御の記述子の受け渡しで渡す）が合う。srt はこの点の限界として、許したドメインへの持ち出しとドメインの偽装を挙げている（README）。

### 拒否の理由を返す

`benitoiteh` は、サンドボックスの拒否を出力の文字列から推定する（設計書 03-02「拒否の検出」）。

- srt は、macOS の方針の拒否に、コマンドごとの乱数の印を付け（`(deny default (with message ...))`）、`log stream` で拾って、道具の結果に拒否の一覧を添える（`src/sandbox/macos-sandbox-utils.ts:149-157,1627-1713`、`src/sandbox/sandbox-manager.ts:3319-3337`）。
- nono は、あるパス・ソケット・ホスト・コマンドがなぜ許されるか拒まれるかを答える `nono why` を持つ（`crates/nono-cli/src/why_runtime.rs`）。

正確な理由を返せば、LLM の無駄なやり直しが減る見込みがある（未測定）。

### 守る場所と PATH

- srt は、`.git` に加えて、シェルの設定、`.gitconfig`、`.gitmodules`、`.mcp.json`、`.vscode/`、`.idea/`、`.claude/commands`、`.claude/agents`、`.git/hooks`、`.git/config` を書かせない（`src/sandbox/sandbox-utils.ts:21-50`）。`benitoiteh` に当てはめると、次の起動の指示になる `.agents/`（プロジェクトの Skill）と、エディタが実行しうる `.vscode/` と `.idea/` が候補である。
- srt は Linux で、書ける根から守る場所までの途中のディレクトリをマウントの場所にして、付け替えを `EBUSY` で止める（`src/sandbox/linux-sandbox-utils.ts:1917` の `ancestorPinArgs`）。`benitoiteh` にはこれがない。bubblewrap の読み取りだけの重ね（`--ro-bind`）だけで試すと、すでにある `sub/.git` の親の `sub` を名前を変えて動かし、新しい `sub/.git/hooks/pre-commit` を書けた（2026-11-02、本環境の bubblewrap 0.9.0、root でない利用者）。`benitoiteh` は書ける根に Landlock の `Refer` を許す（`crates/benitoiteh/src/sandbox/linux/filesystem.rs:114`）ので、実際のサンドボックスでも同じに通ると見るが、確かめていない【要検証】。設計書 03-02 は、子プロセスが新しく作った入れ子の守る場所を保証の外とするが、この手順は、すでにある入れ子の守る場所の保護を、付け替えで実質的に外す。
- nono は、サンドボックスの外で動かす補助のプログラムの `PATH` から、サンドボックスが書ける場所を、シンボリックリンクの各段まで調べて除く（`crates/nono/src/broker_path.rs`）。`benitoiteh` の `escalate` と秘密の入力の仲介は、サンドボックスの外で、継承した `PATH` のまま動く。サンドボックスの中のコマンドが、`PATH` にあるプロジェクトのディレクトリに偽の `git` を置ける場合がある（未確認）。
- Codex は、`main` の前に、コアのダンプと ptrace の接続を止め、`LD_PRELOAD` と `DYLD_*` を除く（`codex-rs/process-hardening/src/lib.rs`）。`benitoiteh` は、サンドボックスの補助と画像の補助でダンプを止める（`crates/benitoiteh/src/sandbox/linux.rs:106`、`crates/benitoiteh/src/tools/read/image_helper.rs:67`）が、API のキーと OAuth のトークンを持つ主のプロセスでは止めていない。

### プロジェクトの信頼

- Pi は、プロジェクトの設定、MCP の設定、拡張、Skill、プロンプト、`SYSTEM.md` を、信頼の確認（毎回・保存・このセッションだけ）の後にだけ読む（`packages/coding-agent/src/core/project-trust.ts`）。
- Codex は、フックの内容の値を利用者が信頼したときだけ動かし、内容が変われば信頼を外す（`codex-rs/hooks/src/engine/discovery.rs:1000-1027`）。
- Hermes は、`AGENTS.md` などの指示のファイルへの書き込みを、確認を省く設定でも必ず確認する（`tools/file_tools_write_guards.py`）。

`benitoiteh` は、プロジェクトの `.agents/skills` を確認なしに読む。信頼しないリポジトリが LLM への指示を足せる道筋なので、信頼の確認が合う。

## エージェントの性能に関わる不足

効果は、どれも `tools/skill-eval` で測ってから採る（00-01「性能の目標」）。

| 案 | 出典 | 見込み |
|---|---|---|
| 一度に複数の置き換えを渡す `edit`（`edits[]`） | Pi `packages/coding-agent/src/core/tools/edit.ts:34`。Hermes の空白や字下げのずれを許す照合 `tools/fuzzy_match.py` | 編集の失敗と手数を減らす |
| 圧縮の前に、古い道具の結果を、頭と尾だけに刈り込む（LLM を呼ばない） | DeepSeek Harness `packages/bundle/base/cordis.patch.yml:424-431`（8,192 字を超える結果を、頭 4,096 字と尾 1,024 字にする） | 要約による情報の欠けと費用を減らし、キャッシュの先頭を保つ |
| 触ったディレクトリの下の `AGENTS.md` を、道具の結果に添える | Hermes `agent/subdirectory_hints.py`。DeepSeek Harness `packages/context/agent-instructions/README.md` | キャッシュを崩さずに指示を届ける |
| 計画・todo の道具 | Codex `codex-rs/core/src/tools/handlers/plan_spec.rs`。DeepSeek Harness `packages/todo/tool-todo` | BH-008 の比較に使う |
| 編集の直後に終えようとしたら、検査やテストを促す | Hermes `agent/verification_stop.py` | BH-011（完了の前の確かめ）と比べる |
| 背景の処理（開発用のサーバ、長いテスト）の起動・待ち・停止 | Codex `codex-rs/core/src/tools/handlers/shell_spec.rs:34-147`。Hermes `tools/process_registry.py` | 通常モードの課題で効く。子プロセスを残さない規則（03-02）との両立に設計が要る |
| Benitoite のスクリプトから `read`・`glob`・`grep` を呼ぶ（エフェクトとして） | DeepSeek Harness `packages/core/tool-executor-ptc/README.md`。Pi `packages/coding-agent/docs/codemode.md`。Hermes `tools/code_execution_tool.py` | 多くのファイルを扱う課題で手数を大きく減らしうる。BH-002 の具体の手本になる。ほかの言語の処理系は持ち込まない |
| 一つの文言を A/B で決める測定 | DeepSeek Harness `.agents/notes/implemented/bug-fix/2026-10-21-subagent-end-turn-guidance.md`（1 条件 150 回で Fisher の正確検定） | 「測っていない改善を主張しない」に合う |
| 同梱の文書の有無で成功率を比べる評価 | Pi `packages/evals/README.md`（`without_docs` と `with_docs`） | 同梱の Benitoite の Skill の効果を測れる |

## 使い勝手と連携の不足

| 案 | 出典 | 備考 |
|---|---|---|
| 途中のモデルの切り替え（`/model`） | Codex `codex-rs/tui/src/slash_command.rs`。Pi `packages/coding-agent/src/core/slash-commands.ts` | 機能の表と推論の内容の扱いを、切り替えの時に確かめる |
| `@` によるファイルの指定、外部のエディタ、画像の貼り付け | Codex `codex-rs/tui/src/file_search.rs`、`external_editor.rs`。Pi `packages/coding-agent/docs/usage.md` | 利用の多い操作 |
| 利用者の定義するコマンド（プロンプトの雛形） | Pi `packages/coding-agent/src/core/prompt-templates.ts`。Codex の presets | 安価 |
| 会話の書き出し（Markdown、HTML） | Codex `/export`。Pi `packages/coding-agent/src/core/export-html/` | 評価の記録の読み返しにも使える |
| 通知（端末が前面にないとき） | Codex `codex-rs/config/src/types.rs:730-823` | 安価 |
| `exec` の便宜（最後の応答をファイルへ、記録を残さない、出力の型） | Codex `codex-rs/exec/src/cli.rs` | 評価と自動化に効く |
| 実際に LLM に送る入力を示すコマンド | Codex `codex debug prompt-input`（`codex-rs/cli/src/main.rs:268`） | プロンプトのテストと BH-013 の確かめに使える |
| `/init`（`AGENTS.md` の生成） | Codex、Hermes | 安価 |
| MCP のクライアント | 4 つのハーネスすべて | BH-116。道具の承認と、MCP サーバのプロセスへのサンドボックスが要る |
| エディタとの連携（ACP） | Hermes `acp_adapter/`、DeepSeek Harness `packages/acp` | 承認を利用者の側に残せる |
| JSON の行で操作する RPC | Pi `packages/coding-agent/src/modes/rpc/rpc-types.ts` | 評価の道具からの操作にも使える |
| OS のキーストアと、外部のコマンドからのトークンの取得 | Codex `codex-rs/keyring-store/`、`config_types.rs:570`。nono `crates/nono/src/keystore.rs` | BH-033 |

## 採らないもの

次のものは、`benitoiteh` の安全の性質（00-01「目的の衝突」）か費用と通信の方針に反するので、採らない。

- **LLM による承認**: Codex の `auto_review`、Hermes の `smart`、DeepSeek Harness の auto review。DeepSeek Harness は、許した呼び出しを OS のサンドボックスなしで動かす。
- **確認とサンドボックスをまとめて外す設定**: Codex の `--dangerously-bypass-approvals-and-sandbox` と `--dangerously-bypass-hook-trust`、Hermes の `/yolo`。
- **過去のセッションから自動で作った記憶を、次の指示に入れること**: Codex の memories、Hermes の自己改善。信頼しない内容を読んだ会話から作るので、注入が持ち越される。採るなら、利用者が確かめてから入れる形にする。
- **制限モードへのほかの言語の処理系**: Codex の V8、Pi の QuickJS、Hermes の Python、DeepSeek Harness の TypeScript。考え方（スクリプトから道具を呼ぶ）だけを Benitoite のエフェクトとして採る。
- **利用者の知らない費用と送信**: Pi の既定のキャッシュの温め直し（`packages/coding-agent/src/core/cache-warmer.ts`）と既定の利用の記録、DeepSeek Harness の既定のセッションの記録の送信。
- **時間切れの処理を背景に移すこと**: DeepSeek Harness（`packages/shell/tool-bash/src/index.ts`）。検査と実行の間に残った処理がファイルを変えうる。背景の処理は、別の設計で扱う。

## キャッチアップの案

段に分けて示す。各段は独立に進められるが、上の段ほど小さく、安全への効果が大きい。依存のクレートを加えるものは、実行ファイルの大きさの変化を測って記録する（AGENTS.md「実装の規約」）。

### 段 1: 安全の穴を塞ぐ（小さい変更）

1. 子プロセスの環境の方針を加える。`LD_PRELOAD`・`DYLD_*`・`NODE_OPTIONS`・`PYTHONPATH` などを必ず除き、秘密に見える名前（`*_TOKEN`、`*_SECRET`、`*_KEY`、`AWS_*` など）を既定で除き、利用者が許す名前を足せるようにする。
2. サンドボックスで読ませない場所に、nono の組（クラウドの資格情報、`.netrc`、`.npmrc`、`.git-credentials`、シェルの履歴など）と、道具の「読まないファイル」（`.env*` と鍵の形の名前）を加え、`bash` でも読めないようにする（BH-100 と関わる）。
3. `escalate` と秘密の入力の仲介の補助を起動する前に、`PATH` から書ける場所を除く。
4. 主のプロセスで、起動の直後にコアのダンプと ptrace の接続を止める。
5. Linux の入れ子の守る場所の付け替えを、実際のサンドボックスで確かめる【要検証】。通るなら、srt の方式（途中のディレクトリをマウントの場所にする）で塞ぐ。
6. 守る名前に `.agents` を加えるかを決める。
7. プロジェクトの `.agents/skills` とプロジェクトの設定を、信頼の確認の後にだけ読む。

### 段 2: エージェントの性能（評価で確かめてから採る）

1. 複数の置き換えを渡せる `edit`。
2. 圧縮の前の道具の結果の刈り込み。
3. 下のディレクトリの `AGENTS.md` を道具の結果に添える。
4. 計画の道具（BH-008）と、編集の直後の確かめの促し（BH-011 の比較に加える）。
5. Benitoite のスクリプトから読む道具を呼ぶエフェクト（BH-002）。処理系への要望（04-01）として、Benitoite の側と形を詰める。

各案は、DeepSeek Harness の方法に倣い、同じ課題・同じモデルで条件を並べて比べる。本物の LLM を呼ぶ評価は、設計者の了承の後に行う。

### 段 3: 通信の制限（BH-046、BH-040）

1. ループバックの CONNECT のプロキシ（中身は覗かない）を作る。許可の一覧、拒否の一覧、解決したアドレスの確かめ、セッションのトークンを持たせ、`web_fetch` のドメインの確認と同じ答え方を使う。
2. macOS は、`allow_loopback_port` を使う。Linux は、`--unshare-net` の中で待ち受けを作って記述子を渡す（nono の方式）。
3. 拒否をドメインと理由つきで道具の結果に返す。macOS の方針の拒否の記録も同じに返す（srt の方式）。

### 段 4: 使い勝手

`/model`、`@` によるファイルの指定、外部のエディタ、利用者の定義するコマンド、書き出し、通知、`exec` の便宜、送る入力を示すコマンド、`/init`。どれも安全の性質に触れず、小さく作れる。

### 段 5: 連携

MCP（BH-116）、ACP、RPC、OS のキーストア（BH-033）、偽のトークンを本物に差し替えるプロキシ（段 3 の後）。規模が大きく、実行ファイルの大きさへの影響も大きいので、試作の後の判断（BH-080）とあわせて決める。

## 要検証と未確認

- Linux の入れ子の守る場所の付け替え（「守る場所と PATH」）。`benitoiteh` の実際のサンドボックスで、`sandbox_linux.rs` に否定のテストとして加えて確かめる。
- `PATH` に偽のプログラムを置いて、`escalate` に拾わせる道筋。`benitoiteh` の `PATH` の組み立てを読んで確かめる。
- 各対象の機能のうち「（未確認）」としたもの。
- 効果の見込みは、どれも測っていない。
