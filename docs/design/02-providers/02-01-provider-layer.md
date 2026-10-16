# 提供者の層

- 状態: 草稿
- 未決事項: BH-030, BH-031, BH-032, BH-033, BH-034, BH-035, BH-036

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
- 思考は `thinking: {type: "adaptive"}` とし、深さを `output_config.effort` で指定する。effort の値は設定ファイルで指定し、指定がなければ `high` とする。Claude Opus 5.5 の effort の既定は `medium` であり、省くと既定が使われるので、`benitoiteh` は常に値を明示する。既定の値は評価で見直す（BH-034）。
- プロンプトのキャッシュは、`cache_control` の区切りを三か所に置く。システムプロンプトの最後（その前の道具の定義も含めてキャッシュされる）、前の要求の会話の最後の項目、今の要求の会話の最後の項目である。理由: 区切りは一つの要求で最大 4 か所であり、キャッシュに合う位置を探すのは、区切りから前へ 20 のブロックまでである（[プロンプトのキャッシュ](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)。2026-10-16 に確かめた）。一つの番で多くの道具を並べて呼ぶと、一つの番で 20 を超えるブロックが加わり、今の最後の区切りだけでは前の要求のキャッシュに届かない。前の要求の最後に区切りを残せば、届く。区切りを動かしても、thinking のブロックの検査には関わらない（[preserved thinking](https://platform.claude.com/docs/en/build-with-claude/preserved-thinking)）。キャッシュの有効期間は既定の 5 分とする。理由: エージェントのループの要求の間隔は多くの場合 5 分より短く、5 分の期間は読むたびに延びる。1 時間の期間は書き込みの単価が 2 倍になる。
- 終わりの理由が `refusal` のときは、応答の文を示さずに、拒否されたことと分類（`stop_details.category`）を利用者に示して一連の処理を止める。
- thinking のブロックは、同じ送り先で、同じシステムプロンプトと道具の定義の要求ではそのまま送り返す（[文脈の管理](../01-agent/01-04-context.md)の「推論の内容の扱い」）。会話は追記だけで伸ばし、圧縮の後は残した番の thinking のブロックを送らない（同章の「圧縮」）。Claude Sonnet 5.5 と Haiku 5.5 の thinking のブロックは、作ったアカウント（と結び付いたアカウント）でだけ使え、ほかのアカウントへ送ると、誤りにはならずに捨てられる（[preserved thinking](https://platform.claude.com/docs/en/build-with-claude/preserved-thinking)）。
- 前の内容が変わった thinking のブロックを、誤りにせずに捨てさせる指定（`thinking.block_binding.prefix_mismatch_behavior` の `drop_block`）は、ベータの機能なので使わない。thinking のブロックの検査で拒まれたら、やり直せない誤りとして示す。
- HTTP の状態が 429・500・529（`overloaded_error`）のときと、ストリームの中の `error` のイベントが `overloaded_error` のときは、やり直せる誤りとして扱う（[エージェントのループ](../01-agent/01-01-agent-loop.md)の「やり直し」）。
- DeepSeek の Anthropic 互換の口を使うときは、`cache_control`・`strict`・`eager_input_streaming` を付けない（後述の「提供者ごとの機能の表」。既知の提供者の表を持つときは、その DeepSeek の既定とする。BH-032）。DeepSeek の文書の互換の表では、`cache_control` は無視され、`thinking` は使え（`budget_tokens` は無視）、`redacted_thinking` は使えない。`strict` と `eager_input_streaming` は表にない（[DeepSeek の文書の Anthropic 互換の口](https://api-docs.deepseek.com/guides/anthropic_api/)。2026-10-16 に確かめた）。この口で thinking のブロックの送り返しが Anthropic と同じ規則で扱われるかは確かめる（【要検証】BH-031）。

### OpenAI の Responses API

【方針】`openai-responses` は次のように作る。値は Codex CLI の要求の作り方による（[先行事例](../08-appendix/08-01-prior-art.md)）。OpenAI の公式の文書との照合は実装の前に行う（【要検証】BH-030）。

- `POST {base_url}/responses` に `stream: true` で要求する。`store: false` とし、会話はすべて `benitoiteh` の側で持って毎回送る。
- 推論の内容は `include: ["reasoning.encrypted_content"]` で暗号化した形を受け取り、同じ送り先で、同じシステムプロンプトと道具の定義の要求で送り返す（[文脈の管理](../01-agent/01-04-context.md)の「推論の内容の扱い」）。
- `prompt_cache_key` にセッションの ID を入れる。理由: Codex CLI と Pi が同じ指定をしており、同じセッションの要求を同じキャッシュに寄せる。
- `parallel_tool_calls: true` とする。
- 応答の `output` の項目は、順序と `id` を変えずに持ち、次の要求の入力に、そのまま並べて送る（前述の「共通の形」の「提供者の元の項目」）。
- システムプロンプトは `instructions` の欄で送る。Codex CLI は最初の入力の項目に開発者の発言として置くが、試作は文書に記された欄を使う。

### OpenAI の Chat Completions API

【方針】`openai-chat` は次のように作る。

- `POST {base_url}/chat/completions` に `stream: true` と `stream_options: {include_usage: true}` で要求する。
- 道具の呼び出しは、差分の中の `tool_calls` の `index` ごとに引数の断片をつないで組み立てる。
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
| `context_window`・`max_output_tokens` | モデルの文脈の上限と出力の上限（圧縮の判定に使う） |

既知の提供者とモデルの表を実行ファイルに持つか、設定ファイルだけで与えるかは BH-032 で決める。どちらでも、表にない「互換」の口は、本家の提供者と同じ機能を持つと推し量らず、表の既定（使わない側）で扱う。モデルの文脈の上限は、Anthropic では Models API（`GET /v1/models/{id}` の `max_input_tokens`）で得られる。

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

### HTTP の通信

【方針】HTTP の通信は、Benitoite の処理系と同じ系統のクレート（`ureq` と、`rustls` に `rustls-graviola` の暗号の実装を組み合わせたもの）で行う（[クレートの構成](../06-impl/06-01-crate-structure.md)）。理由: 非同期の実行時（tokio など）を使わずにすみ、実行ファイルが小さくなる。試作を Benitoite の処理系にマージするときに、依存が増えない。

- ストリーミングの応答は、受け取ったバイト列を行に分け、Server-Sent Events の `event:` と `data:` の行を自前で読む。一つのイベントの大きさの上限は 16 MiB とし、超えたら誤りとして受け取りを止める。理由: 上限なく読むと、壊れた応答や悪意のある互換の口で、主のプロセスのメモリを使い尽くす。
- 時間の上限を、接続（10 秒）、応答のヘッダが届くまで（120 秒）、ストリームで次のデータが届かない時間（300 秒）、一つの要求の全体（30 分）に分ける。300 秒は Codex CLI の値である。`ureq` の時間の上限は、どれも段階ごとの合計の時間であり、データが届かない時間の上限を持たない（`timeout_recv_body` は本文の受け取りの合計の時間である。[ureq 3.4.2 の文書](https://docs.rs/ureq/latest/ureq/config/struct.ConfigBuilder.html)。2026-10-16 に確かめた）。このため、データが届かない時間は、ストリームのスレッドが最後にデータを受け取った時刻を記録し、エージェントのスレッドが見張って判定する。超えたら、その要求を打ち切るか切り離し（後述）、やり直せる誤り（ストリームの途中の切断）として扱う。全体の上限は `ureq` の要求の全体の上限で掛ける。
- 一つの提供者への HTTP の接続は、セッションの間使い回す。

HTTP の受け取りを途中で打ち切る方法（取り消しと、データが届かないときのため）は、`ureq` で確かめる（BH-010）。確かめた方法で足りなければ、ストリームを受け取るスレッドを切り離して結果を捨てる形にする。その場合、提供者の側では応答の生成が続き、利用料がかかりうる。切り離したスレッドは、要求の全体の上限で終わる。切り離したまま終わっていないスレッドの数の上限は、[エージェントのループ](../01-agent/01-01-agent-loop.md)の「取り消し」で定める。

### 費用の表示

モデルごとの単価を使って費用を画面に示すかは BH-035 で決める。単価は変わるので、実行ファイルに持つと古くなる。それまでは、トークンの量だけを示す。
