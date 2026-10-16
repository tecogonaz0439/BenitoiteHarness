# 提供者の層

- 状態: 草稿
- 未決事項: BH-030, BH-031, BH-032, BH-033, BH-034, BH-035

## 目的と範囲

本章は、`benitoiteh` が LLM の提供者と通信する層を定める。対象は、提供者に依らない会話の形、提供者の API ごとのアダプタ、ストリーミングの受け取り、プロンプトのキャッシュの指定、推論の内容の扱い、認証の情報、HTTP の通信である。どの提供者を使うかの設定は[CLI と設定ファイル](../05-interface/05-01-cli.md)の「設定ファイル」で定める。

## 前提

- Benitoite の事前検討は、OpenAI 互換の API（Chat Completions と Responses）、Sign in with ChatGPT、Anthropic の Messages API、Google の Gemini API の四つの提供者を採ると定めた（[ADR 0340](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/docs/archive/2026-10-09-design-first-release/decisions/0340-llm-providers-for-own-agent-harness.md)）。手元の推論サーバと中継のサービスは OpenAI 互換の実装で扱う。Claude の購読のアカウントで処理系が自らサインインする形は候補にしない（ADR 0340 の決定 5）。
- Pi の提供者の層は、会社ごとではなく、通信の形（wire API）ごとに一つの実装を持ち、約 35 の提供者をその上の設定で扱う（[先行事例](../08-appendix/08-01-prior-art.md)）。Codex CLI は、調べたコミットでは Responses API だけを持ち、Chat Completions を外している。
- DeepSeek の API は、Anthropic の Messages API と互換の口（`https://api.deepseek.com/anthropic`）を持ち、DeepSeek Harness はこの口を使う。この口ではキャッシュが自動で効き、`cache_control` を使わない（DeepSeek Harness のソースで確かめた。[先行事例](../08-appendix/08-01-prior-art.md)）。

## 仕様

### 層の分け方

【方針】提供者の層は、次の二段に分ける。

1. **共通の形**: 会話、道具の定義、ストリーミングのイベント、使ったトークンの量、終わりの理由を、提供者に依らない Rust の型で表す。エージェントのループと文脈の管理は、この形だけを扱う。
2. **アダプタ**: 通信の形ごとに一つ置き、共通の形と提供者の要求・応答を相互に変える。

アダプタは次の三つを作る。

| アダプタ | 通信の形 | 主な使い道 |
|---|---|---|
| `anthropic-messages` | Anthropic の Messages API | Claude。DeepSeek の Anthropic 互換の口 |
| `openai-responses` | OpenAI の Responses API | OpenAI のモデル。Sign in with ChatGPT（後で加える） |
| `openai-chat` | OpenAI の Chat Completions API | 手元の推論サーバ（Ollama、llama.cpp の server、LM Studio、vLLM）、中継のサービス（OpenRouter など）、そのほかの互換の口 |

アダプタは、LLM の API のクライアントのクレートを使わずに、HTTP の層（後述の「HTTP の通信」）の上に自前で作る。Benitoite の事前検討は LLM の API のクライアントに既存の OSS を使うとした（ADR 0194 の決定 4）が、これを改める。理由: [目的と設計原則](../00-overview/00-01-goals.md)の「事前検討との関係」に書いた。

Gemini の API の専用のアダプタ（ADR 0340 の E）と、Sign in with ChatGPT の認証（ADR 0340 の B）は、三つのアダプタで評価を行った後に加える（[ロードマップ](../00-overview/00-03-roadmap.md)）。その間、Gemini は `openai-chat` で Gemini の OpenAI 互換の口を呼んで使える。理由: 試作の評価に要るのは、性能の高いモデルを一つ以上と、費用をかけずに繰り返せるモデルを一つ以上使えることであり、三つのアダプタでその両方を賄える。

### 共通の形

【方針】共通の形は次の要素から成る。

- **会話の項目**: 利用者の発言（文）、応答（文、推論の内容、道具の呼び出しの並び）、道具の結果（呼び出しの ID、文、誤りの印）。
- **推論の内容**: 表示用の文（あれば）と、提供者が送り返しを求める不透明なデータ（Anthropic の署名、OpenAI の暗号化した推論）。作ったアダプタの種類とモデルの名前を持つ。
- **道具の呼び出し**: ID、名前、引数（JSON の値）。ID は、英数字と `_`・`-` だけの 64 文字以内に揃える。理由: Pi の記録によれば、OpenAI の Responses API の ID は 450 文字を超えて `|` を含みうる一方、Anthropic は `^[a-zA-Z0-9_-]+$` の形を求める。提供者を変えて再開したときに、ID の形の違いで要求が拒まれないようにする。
- **ストリーミングのイベント**: 文の差分、推論の文の差分、道具の呼び出しの始まり・引数の差分・終わり、使ったトークンの量、終わり。
- **使ったトークンの量**: 入力、キャッシュから読んだ量、キャッシュに書いた量、出力、そのうちの推論。提供者が報告しない項目は、ないものとして扱う。
- **終わりの理由**: 終わった、道具の呼び出し、出力の上限、拒否（Anthropic の `refusal` など）、誤り、取り消し。

### Anthropic の Messages API

【方針】`anthropic-messages` は次のように作る。値は Anthropic の文書（[ストリーミング](https://platform.claude.com/docs/en/build-with-claude/streaming)、[プロンプトのキャッシュ](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)、[適応的な思考](https://platform.claude.com/docs/en/build-with-claude/adaptive-thinking)、[誤り](https://platform.claude.com/docs/en/api/errors)）による。2026-10-16 に、Claude Code に同梱の Claude API の手引き（2026-10-06 の版）で確かめた。

- `POST {base_url}/v1/messages` に `stream: true` で要求する。
- 道具の定義に `strict: true` を付ける（引数が形どおりになることを提供者が保証する）。ストリーミングで大きな引数（ファイルの中身）を早く受け取るため、`eager_input_streaming: true` を付け、受け取った引数を `benitoiteh` の側で JSON Schema で検証する（[エージェントのループ](../01-agent/01-01-agent-loop.md)の「道具の実行」）。
- 道具の呼び出しを強いる `tool_choice`（`any`・`tool`）を使わない。理由: Claude Opus 5.5、Claude Sonnet 5.5、Claude Fable 5.1 では 400 の誤りになる。圧縮で道具を呼ばせない `tool_choice: none` は使える。
- 思考は `thinking: {type: "adaptive"}` とし、深さを `output_config.effort` で指定する。effort の値は設定ファイルで指定し、指定がなければ `high` とする。Claude Opus 5.5 の effort の既定は `medium` であり、省くと既定が使われるので、`benitoiteh` は常に値を明示する。既定の値は評価で見直す（BH-034）。
- プロンプトのキャッシュは、`cache_control` の区切りを三か所に置く。道具の定義の最後、システムプロンプトの最後、会話の最後の項目である（区切りは一つの要求で最大 4 か所）。キャッシュの有効期間は既定の 5 分とする。理由: エージェントのループの要求の間隔は多くの場合 5 分より短く、5 分の期間は読むたびに延びる。1 時間の期間は書き込みの単価が 2 倍になる。
- 終わりの理由が `refusal` のときは、応答の文を示さずに、拒否されたことと分類（`stop_details.category`）を利用者に示して一連の処理を止める。
- thinking のブロックは、同じモデルへの要求ではそのまま送り返す。会話は追記だけで伸ばし、圧縮の後は残した番の thinking のブロックを送らない（[文脈の管理](../01-agent/01-04-context.md)の「圧縮」）。
- HTTP の状態が 429・500・529（`overloaded_error`）のときと、ストリームの中の `error` のイベントが `overloaded_error` のときは、やり直せる誤りとして扱う（[エージェントのループ](../01-agent/01-01-agent-loop.md)の「やり直し」）。
- DeepSeek の Anthropic 互換の口を使うときは、設定で `cache_control` を付けない指定をする（後述の「提供者ごとの機能の表」）。

### OpenAI の Responses API

【方針】`openai-responses` は次のように作る。値は Codex CLI の要求の作り方による（[先行事例](../08-appendix/08-01-prior-art.md)）。OpenAI の公式の文書との照合は実装の前に行う（【要検証】BH-030）。

- `POST {base_url}/responses` に `stream: true` で要求する。`store: false` とし、会話はすべて `benitoiteh` の側で持って毎回送る。
- 推論の内容は `include: ["reasoning.encrypted_content"]` で暗号化した形を受け取り、同じモデルへの要求で送り返す。
- `prompt_cache_key` にセッションの ID を入れる。理由: Codex CLI と Pi が同じ指定をしており、同じセッションの要求を同じキャッシュに寄せる。
- `parallel_tool_calls: true` とする。
- システムプロンプトは `instructions` の欄で送る。Codex CLI は最初の入力の項目に開発者の発言として置くが、試作は文書に記された欄を使う。

### OpenAI の Chat Completions API

【方針】`openai-chat` は次のように作る。

- `POST {base_url}/chat/completions` に `stream: true` と `stream_options: {include_usage: true}` で要求する。
- 道具の呼び出しは、差分の中の `tool_calls` の `index` ごとに引数の断片をつないで組み立てる。
- 推論の文を `reasoning_content` などの欄で返す互換の口がある。受け取った推論の文は画面に示す。次の要求に送り返すかは、提供者ごとの機能の表で決める。DeepSeek の口が推論の文の送り返しをどう扱うかは、実装の前に確かめる（【要検証】BH-031）。

### 提供者ごとの機能の表

【方針】「互換」の口が持つ機能は提供者によって違う。設定ファイルの提供者ごとの欄で、次の機能の有無を指定できるようにし、既知の提供者には既定の値を持たせる。

| 機能 | 内容 |
|---|---|
| `cache_control` | Anthropic の形のキャッシュの区切りを付けるか |
| `strict_tools` | 道具の定義に strict を付けるか |
| `tool_choice_none` | 道具を呼ばせない指定を使えるか（圧縮の作り方が変わる） |
| `reasoning_replay` | 推論の内容を送り返すか |
| `parallel_tool_calls` | 一つの応答で複数の道具を呼ばせるか |
| `context_window`・`max_output_tokens` | モデルの文脈の上限と出力の上限（圧縮の判定に使う） |

既知の提供者とモデルの表を実行ファイルに持つか、設定ファイルだけで与えるかは BH-032 で決める。モデルの文脈の上限は、Anthropic では Models API（`GET /v1/models/{id}` の `max_input_tokens`）で得られる。

### 認証の情報

【方針】API キーは、利用者の設定ファイルに、キーそのものではなく参照として書く。参照は次の二つとする。

- `{ env = "ANTHROPIC_API_KEY" }`: 環境変数から読む。
- `{ command = ["<プログラム>", "<引数>", ...] }`: プログラムを起動し、標準出力の一行目をキーとする（パスワードマネージャを使う利用者のため）。このプログラムはサンドボックスの外で、起動のときに一度だけ動かす。

キーそのものを設定ファイルに書く欄は設けない。プロジェクトの設定ファイルには参照も書けない。Benitoite の事前検討の見立て（Benitoite の docs/todo の TODO-132 の 1・4）を引き継ぐ。OS のキーストア（macOS の Keychain など）に保存する形は、使うクレートの対応の範囲を確かめてから加える（BH-033）。

- キーは、診断・ログ・セッションの記録・LLM に送る文脈に含めない。
- 子プロセスの環境からキーの変数を除く（[Benitoite の処理系との連携](../04-benitoite/04-01-benitoite-integration.md)の「処理系の起動」）。設定で参照した環境変数の名前も除く対象に含める。

### HTTP の通信

【方針】HTTP の通信は、Benitoite の処理系と同じ系統のクレート（`ureq` と、`rustls` に `rustls-graviola` の暗号の実装を組み合わせたもの）で行う（[クレートの構成](../06-impl/06-01-crate-structure.md)）。理由: 非同期の実行時（tokio など）を使わずにすみ、実行ファイルが小さくなる。試作を Benitoite の処理系にマージするときに、依存が増えない。

- ストリーミングの応答は、受け取ったバイト列を行に分け、Server-Sent Events の `event:` と `data:` の行を自前で読む。
- 接続の上限の時間は 10 秒、ストリームで次のデータが届かない時間の上限は 300 秒とする。300 秒は Codex CLI の値である。
- 一つの提供者への接続は、セッションの間使い回す。

HTTP の受け取りを途中で打ち切る方法（取り消しのため）は、`ureq` で確かめる（BH-010）。確かめた方法で足りなければ、ストリームを受け取るスレッドを切り離して結果を捨てる形にする。その場合、提供者の側では応答の生成が続き、利用料がかかりうる。

### 費用の表示

モデルごとの単価を使って費用を画面に示すかは BH-035 で決める。単価は変わるので、実行ファイルに持つと古くなる。それまでは、トークンの量だけを示す。
