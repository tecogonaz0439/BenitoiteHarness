# 提供者の層

- 状態: 草稿
- 未決事項: BH-030, BH-031, BH-032, BH-033, BH-036, BH-115, BH-117

## 目的と範囲

本章は、`benitoiteh` が LLM の提供者と通信する層を定める。対象は、提供者に依らない会話の形、提供者の API ごとのアダプタ、ストリーミングの受け取り、プロンプトのキャッシュの指定、推論の内容の扱い、認証の情報、HTTP の通信である。どの提供者を使うかの設定は[CLI と設定ファイル](../05-interface/05-01-cli.md)の「設定ファイル」で定める。

## 前提

- Benitoite の事前検討は、OpenAI 互換の API（Chat Completions と Responses）、Sign in with ChatGPT、Anthropic の Messages API、Google の Gemini API の四つの提供者を採ると定めた（[ADR 0340](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.7/docs/archive/2026-10-09-design-first-release/decisions/0340-llm-providers-for-own-agent-harness.md)）。手元の推論サーバと中継のサービスは OpenAI 互換の実装で扱う。Claude の購読のアカウントで処理系が自らサインインする形は候補にしない（ADR 0340 の決定 5）。
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
| `openai-responses` | OpenAI の Responses API | OpenAI のモデル。ChatGPT のプランでの利用（後述の「Sign in with ChatGPT」） |
| `openai-chat` | OpenAI の Chat Completions API | 手元の推論サーバ（Ollama、llama.cpp の server、LM Studio、vLLM）、中継のサービス（OpenRouter など）、そのほかの互換の口 |

アダプタは、LLM の API のクライアントのクレートを使わずに、HTTP の層（後述の「HTTP の通信」）の上に自前で作る。Benitoite の事前検討は LLM の API のクライアントに既存の OSS を使うとした（ADR 0194 の決定 4）が、これを改める。理由: [目的と設計原則](../00-overview/00-01-goals.md)の「事前検討との関係」に書いた。

Gemini の API の専用のアダプタ（ADR 0340 の E）は、三つのアダプタで評価を行った後に加えるかを決める。Sign in with ChatGPT の認証（ADR 0340 の B）は、【決定】評価の結果によらず加える（2026-10-18 に設計者が決めた。[ロードマップ](../00-overview/00-03-roadmap.md)）。その間、Gemini は `openai-chat` で Gemini の OpenAI 互換の口を呼んで使える。理由: 試作の評価に要るのは、性能の高いモデルを一つ以上と、費用をかけずに繰り返せるモデルを一つ以上使えることであり、三つのアダプタでその両方を賄える。

### 共通の形

【方針】共通の形は次の要素から成る。

- **会話の項目**: 利用者の発言（文）、応答（文、推論の内容、道具の呼び出しの並び）、道具の結果（呼び出しの ID、文、誤りの印）。
- **提供者の元の項目**: 応答ごとに、提供者が返した項目（Anthropic の content block、Responses の output の item）を、順序と識別子と署名などの不透明なデータを変えずに、JSON の値として持つ。作った送り先（提供者の設定の名前、ベースの URL、モデル）とアダプタの種類も持つ。同じ送り先への要求では、この元の項目から送る内容を作る。共通の形の会話の項目は、画面、道具の実行、別の送り先へ送るときに使う、元の項目から作った写しである。理由: 文、推論、道具の呼び出しを別々の欄に分けると、文と推論と道具の呼び出しが交互に並ぶ応答の順序が失われる。Anthropic の thinking のブロックは前の内容に結び付けて検査されるので（[文脈の管理](../01-agent/01-04-context.md)の「推論の内容の扱い」）、順序が変わった応答を送ると拒まれうる。
- **推論の内容**: 表示用の文（あれば）と、提供者が送り返しを求める不透明なデータ（Anthropic の署名、OpenAI の暗号化した推論）。
- **道具の呼び出し**: `benitoiteh` が付ける内部の ID、提供者の ID、名前、引数（JSON の値）。提供者の ID は変えずに持つ。Responses の function call の項目は、項目の `id` と、道具の結果が指す `call_id` を別々に持ち、道具の結果（`function_call_output`）には `call_id` を使う（[OpenAI の function calling の文書](https://developers.openai.com/api/docs/guides/function-calling)。2026-10-16 に確かめた）。別の送り先へ送るときだけ、その提供者の形の ID（Anthropic は `^[a-zA-Z0-9_-]+$`）を作り、元の ID との対応の表を持つ。作る ID は、元の ID のハッシュを含めて、別の呼び出しと同じにならないようにする。理由: Pi の記録によれば、OpenAI の Responses API の ID は 450 文字を超えて `|` を含みうる。字を削るだけで揃えると、別の呼び出しが同じ ID になりうる。
- **ストリーミングのイベント**: 文の差分、推論の文の差分、道具の呼び出しの始まり・引数の差分・終わり、使ったトークンの量、終わり。
- **使ったトークンの量**: 入力の全体、そのうちキャッシュから読んだ量とキャッシュに書いた量、出力、そのうちの推論と、提供者が返した値そのもの。提供者が報告しない項目は、0 ではなく「報告なし」として扱う。入力の全体は、提供者ごとの報告の形の違いを揃えた値とする。Anthropic の `input_tokens` はキャッシュを使わない入力だけを数えるので、`cache_read_input_tokens` と `cache_creation_input_tokens` を足す。OpenAI はキャッシュから読んだ量を入力の内数として報告するので、足さない。
- **終わりの理由**: 終わった、道具の呼び出し、出力の上限、拒否（Anthropic の `refusal` など）、誤り、取り消し。ストリームの終わりのイベントを受け取る前に切れたものは「切断」とし、正常な終わりと区別する。

### Anthropic の Messages API

【方針】`anthropic-messages` は次のように作る。値は Anthropic の文書（[ストリーミング](https://platform.claude.com/docs/en/build-with-claude/streaming)、[プロンプトのキャッシュ](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)、[適応的な思考](https://platform.claude.com/docs/en/build-with-claude/adaptive-thinking)、[誤り](https://platform.claude.com/docs/en/api/errors)）による。2026-10-16 に、Claude Code に同梱の Claude API の手引き（2026-10-06 の版）で確かめた。

- `POST {base_url}/v1/messages` に `stream: true` で要求する。
- 道具の定義に `strict: true` を付ける（引数が形どおりになることを提供者が保証する）。厳密な形の生成が要求する JSON Schema の形（省略できる引数の書き方など）は提供者ごとに違うので、道具の引数の型から、アダプタごとに道具の定義を作る（後述の「道具の定義の変換」）。ストリーミングで大きな引数（ファイルの中身）を早く受け取るため、`eager_input_streaming: true` を付け、受け取った引数を `benitoiteh` の側で JSON Schema で検証する（[エージェントのループ](../01-agent/01-01-agent-loop.md)の「道具の実行」）。
- 道具の呼び出しを強いる `tool_choice`（`any`・`tool`）を使わない。理由: Claude Opus 5.5、Claude Sonnet 5.5、Claude Fable 5.1 では 400 の誤りになる。圧縮で道具を呼ばせない `tool_choice: none` は使える。
- 思考は `thinking: {type: "adaptive"}` とし、深さを `output_config.effort` で指定する。effort の値は設定ファイルで指定し、指定がなければ `low` とする（2026-10-19 に設計者が決めた。BH-034 を閉じた）。Claude Opus 5.5 の effort の既定は `medium` であり、省くと既定が使われるので、`benitoiteh` は常に値を明示する。理由: 評価の比較（`tools/skill-eval/results/2026-10-19-p5-option-comparison.md`。DeepSeek-V4.1-Flash を Anthropic の口で使った）で、`low` は `high` と同じく全試行が成功し、1 回あたりの時間が約 4 割、費用の見積もりが約 2 割少なかった。`max` は時間とトークンが増えただけだった。比べた課題はどれも数番で終わる大きさであり、難しい課題で `low` が成功率を落とすかは確かめていない。利用者は設定の `effort` で上げられる。`openai-responses` の既定（`high`）は変えない（比べていない）。`openai-chat` は、機能の表の `reasoning_effort` が真のときだけ effort を要求に付ける（後述の「OpenAI の Chat Completions API」）。
- プロンプトのキャッシュは、`cache_control` の区切りを三か所に置く。システムプロンプトの最後（その前の道具の定義も含めてキャッシュされる）、前の要求の会話の最後の項目、今の要求の会話の最後の項目である。理由: 区切りは一つの要求で最大 4 か所であり、キャッシュに合う位置を探すのは、区切りから前へ 20 のブロックまでである（[プロンプトのキャッシュ](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)。2026-10-16 に確かめた）。一つの番で多くの道具を並べて呼ぶと、一つの番で 20 を超えるブロックが加わり、今の最後の区切りだけでは前の要求のキャッシュに届かない。前の要求の最後に区切りを残せば、届く。区切りを動かしても、thinking のブロックの検査には関わらない（[preserved thinking](https://platform.claude.com/docs/en/build-with-claude/preserved-thinking)）。キャッシュの有効期間は既定の 5 分とする。理由: エージェントのループの要求の間隔は多くの場合 5 分より短く、5 分の期間は読むたびに延びる。1 時間の期間は書き込みの単価が 2 倍になる。
- 終わりの理由が `refusal` のときは、応答の文を示さずに、拒否されたことと分類（`stop_details.category`）を利用者に示して一連の処理を止める。
- thinking のブロックは、同じ送り先で、同じシステムプロンプトと道具の定義の要求ではそのまま送り返す（[文脈の管理](../01-agent/01-04-context.md)の「推論の内容の扱い」）。会話は追記だけで伸ばし、圧縮の後は残した番の thinking のブロックを送らない（同章の「圧縮」）。Claude Sonnet 5.5 と Haiku 5.5 の thinking のブロックは、作ったアカウント（と結び付いたアカウント）でだけ使え、ほかのアカウントへ送ると、誤りにはならずに捨てられる（[preserved thinking](https://platform.claude.com/docs/en/build-with-claude/preserved-thinking)）。
- 前の内容が変わった thinking のブロックを、誤りにせずに捨てさせる指定（`thinking.block_binding.prefix_mismatch_behavior` の `drop_block`）は、ベータの機能なので使わない。thinking のブロックの検査で拒まれたら、やり直せない誤りとして示す。
- HTTP の状態が 429・500・529（`overloaded_error`）のときと、ストリームの中の `error` のイベントが `overloaded_error` のときは、やり直せる誤りとして扱う（[エージェントのループ](../01-agent/01-01-agent-loop.md)の「やり直し」）。
- DeepSeek の Anthropic 互換の口を使うときは、`cache_control`・`strict`・`eager_input_streaming` を付けない（後述の「提供者ごとの機能の表」。既知の提供者の表を持つときは、その DeepSeek の既定とする。BH-032）。DeepSeek の文書の互換の表では、`cache_control` は無視され、`thinking` は使え（`budget_tokens` は無視）、`redacted_thinking` は使えない。`strict` と `eager_input_streaming` は表にない（[DeepSeek の文書の Anthropic 互換の口](https://api-docs.deepseek.com/guides/anthropic_api/)。2026-10-16 に確かめた）。この口で thinking のブロックの送り返しが Anthropic と同じ規則で扱われるかは確かめる（【要検証】BH-031）。

### OpenAI の Responses API

【方針】`openai-responses` は次のように作る。2026-10-17 に、OpenAI の公式の文書（[推論](https://developers.openai.com/api/docs/guides/reasoning)、[function calling](https://developers.openai.com/api/docs/guides/function-calling)、[会話の状態](https://developers.openai.com/api/docs/guides/conversation-state)、[プロンプトのキャッシュ](https://developers.openai.com/api/docs/guides/prompt-caching)、[誤りの符号](https://developers.openai.com/api/docs/guides/error-codes)）、公式の OpenAPI の定義（`openai/openai-openapi` の `c9fc5f8` の `openapi.yaml`）、公式の Python の SDK（`openai/openai-python` の `c969e32` の `src/openai/types/responses/`）で確かめた（BH-030）。

- `POST {base_url}/responses` に `stream: true` で要求する。`store: false` を明示する（省くと `true` になる）。会話はすべて `benitoiteh` の側で持って毎回送る。
- システムプロンプトは `instructions` の欄で送る。Codex CLI は最初の入力の項目に開発者の発言として置くが、試作は文書に記された欄を使う。
- 道具の定義は、Responses の平らな形（`{type: "function", name, description, parameters, strict}`。Chat Completions の `function` の入れ子と違う）で送り、`strict: true` を明示する。理由: `strict` を省くと、Responses は形を厳密にできるときだけ厳密にし、できなければ黙って厳密でない形に戻す。明示すれば、形が要件に合わないときに誤りになる。
- `tool_choice: "auto"`、`parallel_tool_calls: true` とする。圧縮で道具を呼ばせないときは `tool_choice: "none"` を使う。
- 推論は `reasoning: {effort, summary: "auto"}` とし、`effort` は提供者の設定の値とする。`include: ["reasoning.encrypted_content"]` を付ける。理由: 今の API は、`store: false` でも推論の項目の `encrypted_content` を既定で返し、この指定を求めないが、互換のために受け付ける。付けておけば、古いモデルと互換の口でも暗号化した推論を受け取れる。
- 推論の項目と暗号化した推論は、`response.output_item.done` の項目から取る。`response.output_item.added` の項目の `encrypted_content` は欠けうる（推論の文書）。
- 応答の `output` の項目は、順序と `id` と欄（助手の発言の `phase` など）を変えずに持ち、次の要求の入力に、そのまま並べて送る（前述の「共通の形」の「提供者の元の項目」）。理由: 推論の文書は、最後の利用者の発言の後の推論、道具の呼び出し、道具の結果の項目を、手を加えずに送り返すことを求める。`phase` を落とすと性能が落ちうる（SDK の型の説明）。`store: false` で、サーバが付けた `id` を送り返してよいかは、公式の文書に書かれていない。Codex CLI は、`store: false` の要求で、接頭辞の付いた `id`（`rs_` など）を残し、接頭辞のないものだけを除いて送る（`codex-rs/core/src/client.rs` の `prepare_response_items_for_request`。`a45b8cc`）。試作もこれに倣い、本物の API で確かめる（【要検証】BH-030）。
- 道具の結果は `function_call_output` の項目で、`call_id` と、文字列の `output` を送る。
- `prompt_cache_key` にセッションの ID を入れる。理由: Codex CLI と Pi が同じ指定をしており、同じセッションの要求を同じキャッシュに寄せる。キャッシュの文書は、この値を振り分けの手がかりとし、当たることを保証しないと書く。`gpt-5.6` 以降のモデルでは要らないとも書くが、付けても害はない。キャッシュの区切り（`prompt_cache_options` など、`gpt-5.6` 以降の機能）は試作では使わない。
- ストリームの各行は `event:` の名前と、同じ名前を `type` に持つ `data:` の JSON から成り、`:` で始まる注釈の行を含みうる。終わりは `response.completed`・`response.incomplete`・`response.failed` のイベントで知り、Chat Completions の `[DONE]` の行はない（OpenAPI の定義の例）。`error` のイベントは誤りとして扱う。
- 使ったトークンの量は、最後の応答の `usage`（`input_tokens`、`input_tokens_details.cached_tokens`・`cache_write_tokens`、`output_tokens`、`output_tokens_details.reasoning_tokens`）から取る。キャッシュから読んだ量は入力の内数として扱う（前述の「共通の形」）。キャッシュの文書は、当たりの率を、キャッシュから読んだ量を入力の量で割って求めるとしている。
- 終わりの理由は、`status` と `incomplete_details.reason` から作る。`max_output_tokens` は出力の上限、`content_filter` は拒否とする。応答の文の部分が `refusal` の形のときも拒否とする。
- HTTP の状態が 500〜599 のときと、429 のうち回数の上限によるもの（`slow_down` など）は、やり直せる誤りとして扱い、`Retry-After` があれば少なくともその時間を待つ。429 のうち、利用の上限や支払いに関わるもの（`credit_balance_exhausted`・`organization_spend_limit_exceeded`・`project_spend_limit_exceeded`・`organization_usage_limit_exceeded` の符号と、`insufficient_quota`）は、やり直さない。誤りの符号の文書は、支払い、使う額、割り当ての誤りをやり直しても使えるようにならないと書いている。

### OpenAI の Chat Completions API

【方針】`openai-chat` は次のように作る。

- `POST {base_url}/chat/completions` に `stream: true` と `stream_options: {include_usage: true}` で要求する。
- 道具の呼び出しは、差分の中の `tool_calls` の `index` ごとに引数の断片をつないで組み立てる。
- 【方針】`index` のない `tool_calls` の要素は、断片でなく完全な呼び出しとみなし、届いた順に別々の呼び出しとして受け取る。`[DONE]` まで届き、道具の呼び出しが揃っていれば、`finish_reason` が `stop` でも道具の呼び出しの番として扱う（途中で切れた応答と出力の上限では、今までどおり道具を実行しない）。差分と道具の呼び出しの要素に `extra_content` があれば、元の発言の一部として保ち、同じ送り先へ送り返すときにだけ、受け取ったとおりに付けて送る。`extra_content` が `null` のときは欄がないものとして扱う。`extra_content` の値は、入れ子の深さ 16 まで、直列化した大きさ 64 KiB まで（一つの応答の合計）とし、超えたら読めない応答とする。理由: 深い入れ子はセッションの記録を読み直せなくし（JSON の読み取りの再帰の上限）、大きな値は毎回の要求で送り返される。Gemini の値は深さ 2、数百バイトである（2026-10-18 の P1-12 の点検）。`extra_content` は、推論の内容（[文脈の管理](../01-agent/01-04-context.md)の「推論の内容の扱い」）と違い、同じ送り先へ送るときには `reasoning_replay` によらず送り返す。理由: Gemini は推論の印を送り返すよう求め、印は作った提供者にだけ送られ、画面にも LLM の文脈にも示さない。差分ごとに繰り返される `usage` は、最後の値を使う。理由: 2026-10-18 に、Gemini の OpenAI 互換の口（`https://generativelanguage.googleapis.com/v1beta/openai/`、`gemini-3.8-flash`）に要求を送って確かめると、道具の呼び出しを `index` なしの一つの断片で送り、道具を呼んだ応答の `finish_reason` を `stop` とし、推論の印（thought signature）を `delta.extra_content.google.thought_signature` と `tool_calls[].extra_content.google.thought_signature` に入れて返した。Gemini は推論の印を次の要求で送り返すよう求めている（[Gemini の文書の推論](https://ai.google.dev/gemini-api/docs/thinking)）。
- 応答が読めない形のときは、ストリームが切れたときと分けて示す（`The provider returned a response that benitoiteh could not read.` の形）。理由: 2026-10-18 の Gemini の試しでは、読めない応答を「切れた」と示したので、原因を取り違えた。
- 【決定】提供者ごとの機能の表の `reasoning_effort` が真のときだけ、要求に `reasoning_effort` を付け、値を解決した effort とする（2026-10-19 に設計者が決めた）。既定は偽とし、互換の口に知らない欄を送らない。理由: Google の Gemini の OpenAI 互換の口は `reasoning_effort` を受け付けて推論の量を変え（2026-10-19 に要求を送って確かめた。値は `none`・`minimal`・`low`・`medium`・`high` で、ほかの値は 400 の誤り）、付けなければ提供者の既定の深さで推論する。OpenAI 互換を名乗るほかの口は、この欄を知らずに誤りを返しうる。
- 【決定】使ったトークンの量は、`prompt_tokens`・`prompt_tokens_details.cached_tokens`・`completion_tokens`・`completion_tokens_details.reasoning_tokens` から取る。`completion_tokens_details.reasoning_tokens` がなく、`total_tokens` が `prompt_tokens` と `completion_tokens` の和より大きいときは、その差を推論のトークンとし、出力の量（共通の形の出力全体）に足す（2026-10-19 に設計者が決めた）。理由: Google の Gemini の OpenAI 互換の口は、推論に使ったトークンを `completion_tokens` に含めず、`total_tokens` にだけ含めて返す（同日に確かめた）。数えないと、トークンの量の上限と費用の見積もりが推論の分だけ少なくなる。
- 推論の文を `reasoning_content` などの欄で返す互換の口がある。受け取った推論の文は画面に示す。次の要求に送り返すかは、提供者ごとの機能の表で決める。
- DeepSeek の推論のモードでは、要求が道具の定義を含むとき、前のすべての番の `reasoning_content` を送り返す。道具の定義を含まないときは送り返さなくてよく、送っても使われない。送り返すべきものが欠けると、400 の誤りになる（[DeepSeek の文書の推論のモード](https://api-docs.deepseek.com/guides/thinking_mode/)。2026-10-16 に確かめた）。`benitoiteh` の要求はいつも道具の定義を含むので、DeepSeek の Chat Completions の口の `reasoning_replay` は `all-turns` とする（既知の提供者の表を持つときは、その既定とする。BH-032）。

### 提供者ごとの機能の表

【方針】「互換」の口が持つ機能は提供者によって違う。設定ファイルの提供者ごとの欄で、次の機能の有無を指定できるようにし、既知の提供者には既定の値を持たせる。

| 機能 | 内容 |
|---|---|
| `cache_control` | Anthropic の形のキャッシュの区切りを付けるか |
| `strict_tools` | 道具の定義に strict を付けるか |
| `eager_input_streaming` | Anthropic の道具の引数の細かいストリーミングを求めるか |
| `tool_choice_none` | 道具を呼ばせない指定を使えるか（圧縮の作り方が変わる） |
| `reasoning_replay` | 推論の内容を送り返す規則。`none`（送らない）、`same-destination`（同じ送り先でそのまま送る。圧縮の後は残した番から除く）、`all-turns`（すべての番の推論の文を送る。圧縮の後も残した番の分を送る）のどれか |
| `parallel_tool_calls` | 一つの応答で複数の道具を呼ばせるか |
| `reasoning_effort` | `openai-chat` で、要求に `reasoning_effort` を付けるか（既定は偽） |
| `image_input` | 道具の結果の画像を送るか（既定は偽。本家の Anthropic と Responses の既定は真。モデルが画像を受け付けないときは利用者が偽にする） |
| `context_window`・`max_output_tokens` | モデルの文脈の上限と出力の上限（圧縮の判定に使う） |

既知の提供者とモデルの表を実行ファイルに持つか、設定ファイルだけで与えるかは BH-032 で決める。どちらでも、表にない「互換」の口は、本家の提供者と同じ機能を持つと推し量らず、表の既定（使わない側）で扱う。モデルの文脈の上限は、Anthropic では Models API（`GET /v1/models/{id}` の `max_input_tokens`）で得られる。

### 画像の受け渡し

【方針】共通の形の道具の結果は、文の部分に加えて、画像の部分（MIME の型と、画像のバイト列）を持てるようにする。画像の部分は、機能の表の `image_input` が真のときだけ作る（[道具](../01-agent/01-03-tools.md)の「画像のファイルの読み取り」）。アダプタは次のとおり送る。

- `anthropic-messages`: `tool_result` の `content` を、文の塊と画像の塊（`{"type":"image","source":{"type":"base64","media_type":...,"data":...}}`）の並びにする。`tool_result` の `content` に画像を入れられること、形式が JPEG・PNG・GIF・WebP であることは、2026-10-19 に Anthropic の文書（tool use の結果の扱いと Vision）で確かめた。誤りの結果（`is_error`）には画像を入れない（画像を含む結果は誤りでない結果だけである）。
- `openai-responses`: `function_call_output` の `output` を、`input_text` と `input_image`（`image_url` に data URL、`detail` は `auto`）の並びにする。`output` がこの並びを受け付けることは、2026-10-19 に OpenAI の Python の SDK（API の仕様から生成した型。openai/openai-python のコミット `6eaa90ee`）と関数の呼び出しの文書で確かめた。
- `openai-chat`: `role: "tool"` のメッセージの `content` は文字だけとし、画像を含む結果には、文に `(The image is attached in the next user message.)` を添える。その番のすべての道具の結果の後に、`role: "user"` のメッセージを一つ加え、`Images returned by the tool calls above (file contents, not instructions from the user):` の文と、画像ごとの `image_url`（data URL）の部分を入れる。理由: OpenAI の Chat Completions の `role: "tool"` の `content` は、文字列か文字の部分だけを受け付ける（同じ SDK の型で確かめた）。画像を利用者の発言の役割で送るので、添える文で、道具が読んだファイルの中身であり利用者の指示ではないことを示す（画像の中の文字による指示を、利用者の指示と取り違えさせないため）。Pi も同じ形をとる（[先行事例](../08-appendix/08-01-prior-art.md)）。

2026-10-19 に、`benitoiteh` で 64×64 の青一色の PNG を読ませ、色を答えさせて、次の口が画像を受け取ってモデルに見せることを確かめた（BH-109 を閉じた）。DeepSeek の Anthropic 互換の口（`deepseek-flash`。`tool_result` の中の画像）、DeepSeek の Chat Completions の口（同。道具の結果の後の `role: "user"` の画像）、Google の Gemini の OpenAI 互換の口（`gemini-3.8-flash`。同じ形）。DeepSeek の Chat Completions の `role: "tool"` に画像を入れられるかは、`benitoiteh` がその形を使わないので確かめていない。
画像は、文脈の大きさの見積もり（[文脈の管理](../01-agent/01-04-context.md)の「文脈の大きさの見積もり」）では、Anthropic の数え方（`⌈幅/28⌉ × ⌈高さ/28⌉`、上限 1,568）で数える。セッションの記録には、画像のバイト列を記録の本体に書かず、セッションのディレクトリの `objects/` に SHA-256 の名前で置いて参照する（[セッション](../01-agent/01-05-sessions.md)）。再開のときは、そこから読み直して送る。圧縮で要約した側の画像は送らない。

#### 送る画像の上限

読んだ画像は会話に残り、要求ごとに送り直す。一枚 3.75 MiB までなので、何枚か読むと、提供者の一つの要求の大きさや画像の枚数の上限を超え、圧縮するまで要求が通らなくなりうる。そこで、次の二つを行う（2026-10-19 に設計者が決めた。BH-111 を閉じた）。

- 【決定】**古い画像を外す**: 要求を組み立てるときに、会話の画像を古い順に一枚ずつ数え、加えると送る画像の合計が 12 MiB（12,582,912 バイト。base64 にして約 17 MB）か 20 枚を超えるときは、送る画像のうち古いものから外し、6 MiB 以下かつ 10 枚以下にしてから加える。外した画像は、`(This image was removed to keep the request small. Read the file again if you need it.)` の文に替える。記録の画像は消さない。この規則は会話の画像の並びだけで決まるので、要求ごとに外す画像は変わらず、再開しても同じになる。理由: 外す線と外した後の線を離すので、外すことは時々しか起きず、そのたびにだけ提供者のキャッシュが崩れる。12 MiB は、Google の Gemini の文書の小さい方の一つの要求の上限（20 MB。文書の間で 100 MB と食い違う）にも収まる。Anthropic の一つの要求の上限は 32 MB、画像は 100 枚（文脈の上限が 200k のモデル）、21 枚以上では一枚ごとに両辺 2,000 画素以下である（2026-10-19 に Anthropic の Vision の文書と API の概要で確かめた）。古い画像を外す形は OpenCode に倣った（合計が 25 MiB を超えたら古い順に文に替えて 15 MiB 以下にする。sst/opencode のコミット `ba06ff9e`、`packages/core/src/session/model-request.ts` の `boundImages`）。
- 【決定】**断られたら画像を外して一度だけ送り直す**: 画像を含む要求が、HTTP の状態 400 か 413 で、応答のストリームを受け取る前に断られたら（文脈の超過、認証、残高と利用の上限に分類したものを除く）、その時点までのすべての画像を上の文に替えて、一度だけ送り直す。送り直しが通ったら、その時点までの画像を外したことを記録に書き、以後の要求と再開でも送らない。送り直しも断られたら、誤りを利用者に示す（外したことは記録しない）。理由: 見出しだけを確かめた画像は、中身が壊れていても読めてしまい、提供者が断ると、画像が会話に残る限りすべての要求が通らなくなる。誤りの中身で画像が原因かを見分ける方法は提供者ごとに違うので、画像を含む要求の、ストリームの前の 400 と 413 に限って、一度だけ試す。ストリームの途中の誤りでは送り直さない。理由: 途中の誤りは出力が既に課金されており、画像が原因とは限らない（P5-12 のセキュリティのレビュー）。Claude Code も、大きすぎる画像を会話から外して送り直す（公式の更新の履歴の 2.1.126）。

圧縮で要約させる側の画像は、要約の要求に送らず、結果の文だけを渡す（今の形。Pi、Cline、Codex も同じく文の印に替える）。圧縮で残した側の画像には、上の上限を同じく掛ける。

### 道具の定義の変換

【方針】道具の引数は、`benitoiteh` の中で一つの型として定め、アダプタごとに、その提供者の厳密な形の生成が受け付ける JSON Schema に変えて送る。

- OpenAI の strict では、各オブジェクトに `additionalProperties: false` を付け、すべての引数を必須にし、省略できる引数は型に `null` を加えて表す（[OpenAI の function calling の文書](https://developers.openai.com/api/docs/guides/function-calling)。2026-10-16 に確かめた）。受け取った `null` は、省いたものとして既定の値を使う。
- Responses API と Chat Completions API は、道具の定義の外側の形が違うので、アダプタごとに包む。
- 厳密な形の生成を使うかに依らず、受け取った引数は `benitoiteh` の側で検証する（[エージェントのループ](../01-agent/01-01-agent-loop.md)の「道具の実行」）。理由: 厳密な形の生成は形を保証するが、パスの範囲や大きさの上限までは保証しない。互換の口が strict をどう扱うかは、提供者ごとに確かめていない。

### 認証の情報

【方針】API キーは、利用者の設定ファイルに、キーそのものではなく参照として書く。参照は次の二つとする。

- `{ env = "ANTHROPIC_API_KEY" }`: 環境変数から読む。
- `{ command = ["<プログラム>", "<引数>", ...] }`: プログラムを起動し、標準出力の一行目をキーとする（パスワードマネージャを使う利用者のため）。このプログラムはサンドボックスの外で、起動のときに一度だけ動かす。

キーそのものを設定ファイルに書く欄は設けない。プロジェクトの設定ファイルには参照も書けない。Benitoite の事前検討の見立て（Benitoite の docs/todo の TODO-132 の 1・4）を引き継ぐ。OS のキーストア（macOS の Keychain など）に保存する形は、使うクレートの対応の範囲を確かめてから加える（BH-033）。

- キーは、診断・ログ・セッションの記録・LLM に送る文脈に含めない。
- 子プロセスの環境からキーの変数を除く（[Benitoite の処理系との連携](../04-benitoite/04-01-benitoite-integration.md)の「処理系の起動」）。設定で参照した環境変数の名前も除く対象に含める。

### Sign in with ChatGPT

【決定】ChatGPT の購読のプランで OpenAI のモデルを使えるように、OpenAI が公開しているオープンソースのツール向けの方法（ChatGPT plan usage。<https://developers.openai.com/siwc/token-sharing-open-source>）で、利用者の ChatGPT のアカウントに許可を求め、得たトークンで公開の Responses API を呼ぶ（2026-10-19 に設計者が決めた）。ログインで受け取る ID トークンは、署名を確かめる。理由: API キーでは ChatGPT の購読を使えない（[ロードマップ](../00-overview/00-03-roadmap.md)）。この方法は、OpenAI の文書が「ChatGPT plan usage is available to open-source projects, personal projects that run locally」とする公式の形である（<https://developers.openai.com/cookbook/articles/sign-in-with-chatgpt>、2026-10-19 に読んだ）。Codex CLI のクライアントの ID を流用して ChatGPT の内部の口（`chatgpt.com/backend-api/codex`）を呼ぶ形は、Cline と Roo Code などが使い、Pi と OpenCode は「legacy」と呼んで公式の形に移った。流用を許す公式の記述は見つからず、公式の文書はこの用途に内部の口を使わないよう求める（「do not point it at ChatGPT's `backend-api` endpoints」。<https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference>）。

以下の値と手順は、2026-10-19 に OpenAI の文書（<https://developers.openai.com/siwc/llms-full.txt>）で確かめた。同じ形を実装した例として、OpenCode（sst/opencode のコミット `ba06ff9e` の `packages/core/src/plugin/provider/chatgpt.ts`。ID トークンを確かめる）と Pi（badlogic/pi-mono の `1402973a` の `packages/ai/src/auth/oauth/openai-chatgpt.ts`）を読んだ。OpenAI の DevKit（openai/sign-in-with-chatgpt-devkit）は非商用の許諾なので、作りの参考にだけ使い、コードを写さない。

- **ログイン**: `benitoiteh login chatgpt` で始める（[CLI と設定ファイル](../05-interface/05-01-cli.md)の「login と logout」）。LLM が呼べる道具からは始められない。
  - 認可の口 `https://auth.openai.com/api/accounts/authorize` に、PKCE（S256）、`state`、`nonce`、`response_type=code`、`scope=openid profile email offline_access resource.invoke chatgpt.tokens.use.direct`、`resource=https://api.openai.com/v1`、`ext_agent_host_id`、`client_id` を付けて、利用者のブラウザで開く（開けなければ URL を示す）。`client_id` は、初めては `dynamic_agent_client`（と `agent_name_hint=benitoiteh`）とし、発行された ID（`oaiapp_` で始まる）を保存して、以後はそれを使う。`ext_agent_host_id` は `urn:uuid:` の形の乱数とし、機械ごとに一度作って保存する。
  - 戻り先は `http://127.0.0.1:<番号>/auth/callback` とし、`127.0.0.1` に、OS が選ぶ番号で待ち受ける（`localhost` と書かない。文書が求める）。待ち受けるのは `GET /auth/callback` だけとし、`Host` の値も確かめる。待つ時間の上限は 10 分とする。戻りの `client_id` は、code の交換が通るまで「確かめていない」値として保存し、確かめる前にログインが失敗したら（通信の失敗と、408・429・5xx の応答を除く）、次は `dynamic_agent_client` から始める。408 と 429 は、2026-10-20 の点検で除く側に加えた。理由: どちらも一時的な断りであり、発行された ID が無効であることを示さない。【方針】同じ機械を使うほかの利用者に対しては限界がある: 認可の URL はブラウザを開くプロセスの引数として見えうるので、それを読んだ者は、戻りを先に送ってログインを妨げたり、自分のアカウントの許可に結び付けたりしうる（ループバックの戻り先を使う形に共通の性質）。待ち受けの番号へ大量に接続して、ログインを妨げることもできる。成功のときに示すメールの宛先で、利用者が結び付いたアカウントを確かめられるようにする。【方針】待ち受けは一つのスレッドで行い、待ち受けのソケットと受けた接続を `poll` でまとめて待つ。要求の頭を読み終えていない接続は同時に 64 まで保ち、超えたら最も古いものを閉じる。一つの接続からは、5 秒と 8 KiB までで要求の頭を読む。完全な戻りを受けたら、200 の応答を書いて書く側を閉じてから、待ち受けと残りの接続を閉じる（2026-10-20 の点検で改めた）。理由: 接続ごとにスレッドを起こして未完の接続を 4 つまでに限る形では、少しの接続で、ブラウザの戻りの接続を読み終える前に閉じさせられる。一つのスレッドで扱えば、古い接続を閉じる処理と戻りを受け終える処理が競合しない。
  - 戻りの `state` を、時間の一定な比べ方で確かめる。違えばその戻りを捨てて待ち続ける。戻りに含まれる `client_id` が保存した ID と違えば断る。`error=access_denied` なら、`state` を確かめたうえで、交換せずに終える。
  - トークンの口 `https://auth.openai.com/api/accounts/oauth/token` に、フォームの形で、`grant_type=authorization_code`、`client_id`、`code`、`code_verifier`、同じ `redirect_uri`、同じ `resource` を送る。クライアントの秘密は使わない。
  - ID トークンは、`https://auth.openai.com/.well-known/openid-configuration` から得た JWKS の公開鍵で、RS256 の署名を確かめ、発行者、宛先（発行された `client_id`）、期限、`nonce` を確かめる。署名を確かめる処理は、TLS のために既に持つ `graviola` の RSA の検証（RSASSA-PKCS1-v1_5、SHA-256）を使う（JWKS の `n`・`e` から PKCS#1 の DER を組み立て、`graviola` 0.4.1 の公開の口 `signing::rsa::VerifyingKey::from_pkcs1_der` と `verify_pkcs1_sha256` で確かめる。P5-21 で、`graviola` の `to_pkcs1_der` の出力と一致することを確かめた。BH-114 を閉じた。`graviola` は確かめないので、署名の長さが法の長さと等しいことと、指数が 3 以上の奇数であることは自前で確かめる）。加えて、得た許可に `chatgpt.tokens.use.direct` があることを確かめる（文書: 「A valid ID token alone does not authorize ChatGPT plan usage.」）。
  - 【決定】code の交換でトークンを受け取った後にログインが失敗したら（ID トークンの確かめ、許可の範囲、アカウントの違い、保存の失敗など）、受け取ったリフレッシュトークンの取り消しを、ログアウトと同じ形（`revocation_endpoint`、10 秒の上限）で一度だけ送り、取り消しの結果によらず元の誤りを返す。取り消しの結果は利用者に示さない。保存に同じ `client_id` のトークンがあり、それが受け取ったトークンと同じアカウント（ID トークンの `sub` が同じ）のものか、受け取ったトークンのアカウントを確かめられなかったときは、送らない（2026-10-20 に設計者が決めた）。理由: 保存せずに捨てたリフレッシュトークンは、取り消さなければ、期限（30 日）まで使える形で残る。保存したトークンと同じ許可に属しうるときに送らないのは、取り消しが同じ許可のほかのトークンに及ぶなら、保存したトークンまで使えなくなるからである。【要検証】一つのリフレッシュトークンの取り消しが、同じ許可（同じ `client_id` とアカウント）のほかのトークンに及ぶかは、文書で確かめていない（BH-117）。
- **保存**: 発行された `client_id`、`ext_agent_host_id`、アクセストークン、リフレッシュトークン、期限、`earliest_refresh_at`、アカウントの表示用の情報（メールの宛先など）を、`$XDG_STATE_HOME/benitoiteh/auth/chatgpt.json` に置く。ディレクトリは 0700、ファイルは 0600 とし、一時ファイルに書いてから名前を替える（途中で止まっても壊れたファイルを残さない）。トークンは、診断・ログ・セッションの記録・LLM に送る文脈・URL に含めない。子プロセスの環境にも渡さない。OS のキーストアに置く形は、API キーと同じく後に回す（BH-033）。
- **更新**: アクセストークンは 1 時間、リフレッシュトークンは 30 日で切れ、更新のたびにリフレッシュトークンが替わる（文書）。要求の前に、アクセストークンの期限まで 60 秒を切っていれば（`earliest_refresh_at` より後なら）更新する。更新は、`auth/` の錠のファイルに排他の錠を掛けて、プロセスをまたいで一つずつ行い、錠を取った後に保存の内容を読み直して、ほかのプロセスが既に更新していれば使う。更新の要求はフォームの形で `grant_type=refresh_token`、`client_id`、`refresh_token`、`resource` を送る。受け取ったトークンは、すぐに保存する（使い終えたリフレッシュトークンを失わないため）。更新が `invalid_grant`・`refresh_token_reused` などで断られたら、保存したトークンを消し、`benitoiteh login chatgpt` をやり直すよう示す。【方針】更新の応答が 2xx で、中身の確かめに通らない（期限の欄がないなど）ときも、空でないリフレッシュトークンを読めれば、それを保存してから誤りを返す。このとき、アクセストークンと期限は前の値を残し、`earliest_refresh_at` は消す（2026-10-20 の点検で加えた）。理由: リフレッシュトークンは更新のたびに替わるので、読めた新しいものを捨てると、前のもので更新し直せる保証がなく、ログインのやり直しになりうる。【方針】要求の取り消しは、更新の錠を待つ間と、トークンの口へ送る前に受ける。錠は待たずに取る試しを繰り返し、その合間に取り消しの印を見る。トークンの口へ送った後は、取り消しを受けず、受け取ったトークンを保存するまで待つ。待つのは最長 60 秒（HTTP の通信の上限）であり、この待ちのことは画面に示さない（2026-10-20 の点検で改めた）。理由: 錠を待つ間に限りがない形では、ほかのプロセスが錠を持ち続けると、取り消しが返らない。送った後に止めると、替わったリフレッシュトークンを失う。
- **要求**: 提供者の設定で `kind = "openai-responses"` と `auth = "chatgpt"` を指定すると、API キーの代わりに保存したトークンを使い、送り先を `https://api.openai.com/v1/responses` に固定する。要求は今の `openai-responses` のアダプタで作り、次の点を変える。文書が受け付けないとする欄（`background`、`conversation`、`max_output_tokens`、`max_tool_calls`、`metadata`、`moderation`、`prompt`、`prompt_cache_retention`、`safety_identifier`、`temperature`、`top_logprobs`、`top_p`、`truncation`、`user`、`previous_response_id` など）を送らない。システムプロンプトは `instructions` で送り、`role: "system"` の項目を送らない。`store: false`、`stream: true` とする。【要検証】道具の定義を今の形（最上位の `function`）のまま受け付けるか、名前空間にまとめる必要があるかは、文書が後者を勧めるので、本物の要求で確かめる（BH-115。設計者の了承を得てから行う）。
- **誤り**: `subscription_sharing_usage_limit_exceeded`（429）は、やり直さずに止め、利用の上限に達したことと <https://chatgpt.com/settings/usage> を示す。`subscription_sharing_user_not_eligible`（403）と `subscription_sharing_unsupported_capability`（400）も、やり直さずに止める。401 は一度だけ更新してやり直す。【方針】`auth = "chatgpt"` の提供者では、誤りの詳しい内容を LLM・画面・記録に示すとき、提供者の文（`message`）を示さない。`code` と `type` は、トークンを伏せた後に `^[a-z0-9_.]{1,64}$` に合うときだけ示し、`Retry-After` は数字だけのときに示す（2026-10-20 の点検で加えた）。理由: 今の伏せ方は、トークンの全体と一致する並びだけを置き換えるので、提供者の文がトークンの一部を含むと、その部分が残る。API キーの提供者では、提供者の文が設定の誤りを直す手がかりになるので、今の形を保つ。【方針】ログインと更新の失敗の後片付け（確かめていない `client_id` を `dynamic_agent_client` に戻す保存と、断られた更新の後にトークンを消す保存）が失敗しても、後片付けの誤りではなく元の誤りを返す（2026-10-20 の点検で改めた）。理由: 後片付けの誤り（保存の失敗）を返すと、利用者は、ログインや更新がなぜ失敗したかを知れない。
- **ログアウト**: `benitoiteh logout chatgpt` は、`openid-configuration` の `revocation_endpoint` にリフレッシュトークンの取り消しを 10 秒の上限で求め、保存したトークンを消す（`client_id` と `ext_agent_host_id` は残す）。取り消しを確かめられなければ、そのことを示す。
- **端末だけの環境**: 文書はデバイスコードの形を示さず、別の機械でログインした資格のファイルを写す形を示す。`benitoiteh` も、ログインの URL を示し、戻りを同じ機械の `127.0.0.1` で受ける形だけを作る。写すときは `ext_agent_host_id` を写さない（文書: 移した先の機械の値を保つ）。
- **画面の文言**: ログインの操作は「Continue with ChatGPT」と示す（文書の UI の指針）。

### HTTP の通信

【方針】HTTP の通信は、Benitoite の処理系と同じ系統のクレート（`ureq` と、`rustls` に `rustls-graviola` の暗号の実装を組み合わせたもの）で行う（[クレートの構成](../06-impl/06-01-crate-structure.md)）。理由: 非同期の実行時（tokio など）を使わずにすみ、実行ファイルが小さくなる。試作を Benitoite の処理系にマージするときに、依存が増えない。

- ストリーミングの応答は、受け取ったバイト列を行に分け、Server-Sent Events の `event:` と `data:` の行を自前で読む。一つのイベントの大きさの上限は 16 MiB とし、超えたら誤りとして受け取りを止める。理由: 上限なく読むと、壊れた応答や悪意のある互換の口で、主のプロセスのメモリを使い尽くす。
- 時間の上限を、接続（10 秒）、応答のヘッダが届くまで（120 秒）、ストリームで次のデータが届かない時間（300 秒）、一つの要求の全体（30 分）に分ける。300 秒は Codex CLI の値である。`ureq` の時間の上限は、どれも段階ごとの合計の時間であり、データが届かない時間の上限を持たない（`timeout_recv_body` は本文の受け取りの合計の時間である。[ureq 3.4.2 の文書](https://docs.rs/ureq/latest/ureq/config/struct.ConfigBuilder.html)。2026-10-16 に確かめた）。このため、データが届かない時間は、ストリームのスレッドが最後にデータを受け取った時刻を記録し、エージェントのスレッドが見張って判定する。超えたら、その要求を打ち切るか切り離し（後述）、やり直せる誤り（ストリームの途中の切断）として扱う。全体の上限は `ureq` の要求の全体の上限で掛ける。
- 一つの提供者への HTTP の接続は、セッションの間使い回す。

HTTP の受け取りは、ストリーミングの要求ごとに専用の `ureq` の `Agent` を使い、差し替えた接続の処理が持つ TCP の接続の写しを `shutdown` して打ち切る。2026-10-17 に、試作の P0-03 で、平文と TLS の接続のどちらでも、別のスレッドから打ち切ると読み取りのスレッドが 1 秒以内に終わることを確かめた（`ureq` 3.4.2 の `Agent::with_parts` で接続の処理と名前の解決を差し替えられる。`ureq-3.4.2/src/agent.rs`）。使い回した接続は接続の処理を通らないので、写しと要求を結び付けられない。このため、ストリーミングの要求は接続を使い回さない（前述の「一つの提供者への接続をセッションの間使い回す」は、ストリーミングでない要求に限る）。要求ごとに TLS の接続を作り直す時間の影響は、評価で測る。接続の前の名前の解決と TCP の接続の途中は、接続の時間の上限（10 秒）まで打ち切れないことがある。打ち切ったときも、提供者の側では応答の生成が続き、利用料がかかりうる。

### 費用の表示

【決定】試作では費用を示さず、トークンの量（入力、キャッシュから読んだ量、キャッシュに書いた量、出力）だけを示す（2026-10-17 に設計者が決めた。BH-035 を閉じた）。理由: 単価は変わるので、実行ファイルに持つと古くなる。
