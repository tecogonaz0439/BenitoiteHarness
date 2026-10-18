# P1-12 `openai-chat` の Gemini の OpenAI 互換の口への対応

## 目的

2026-10-18 に設計者の手元で、Gemini 3.8 Flash を `openai-chat`（接続先 `https://generativelanguage.googleapis.com/v1beta/openai`）で使うと、最初の要求で `Provider stream disconnected before completion` になった。オーケストレータが `curl` で生の応答を確かめると、ストリームは `data: [DONE]` まで正しく届いており、次の違いが原因だった（設計書 02-01「OpenAI の Chat Completions API」に 2026-10-18 に加えた【方針】）。

- 道具の呼び出しを、`index` のない一つの断片（`id`・`type`・`function.name`・`function.arguments` が揃ったもの）で送る。今の組み立て器は `index` がないと `InvalidResponse` にし、それが `StreamDisconnected` として示された。
- 道具を呼んだ応答の `finish_reason` が `stop` である。
- 推論の印を `delta.extra_content.google.thought_signature`（文だけの応答）と `tool_calls[].extra_content.google.thought_signature`（道具の呼び出し）に入れて返す。今の組み立て器は捨てるので、次の要求で送り返せない。
- `usage` を差分ごとに、その時点までの量として繰り返す。

これを直し、Gemini の OpenAI 互換の口で道具を使う一連の処理が通るようにする。OpenAI と DeepSeek などの既存の振る舞いは変えない。

## 読む設計書

- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「共通の形」「OpenAI の Chat Completions API」（2026-10-18 の【方針】）「提供者ごとの機能の表」
- 既存のもの: `src/provider/openai_chat/assembly.rs`（`OpenAiChatAssembler`、`Call`、`end_calls`、`finish`）、`src/provider/openai_chat/request.rs`（同じ送り先の元の発言の送り返し）、`src/provider/openai_chat.rs`（誤りの対応）、`src/provider/mod.rs`（`ProviderError`）、既存の `openai_chat` のテストと模擬の提供者（`tests/support/mock_provider.rs`）

## この作業が認める型の変更

- `ProviderError` に、読めない応答を表す種類（例: `InvalidResponse`）を加え、`OpenAiChatError::InvalidResponse` と SSE の読み取りの誤りをそれに対応させる。英文は `The provider returned a response that benitoiteh could not read.` の形とする。ほかのアダプタ（`openai-responses`・`anthropic`）の `InvalidResponse` も同じ種類に対応させてよい（ストリームが本当に途中で終わったときは `StreamDisconnected` のまま）。`ProviderError` を網羅する `match` を直す。

## 作るもの

1. `index` のない `tool_calls` の要素は、完全な呼び出しとして、届いた順に新しい番号（既存の最大の番号の次）で受け取る。`index` のある要素は今までどおり番号ごとに断片をつなぐ。一つの応答の中で両方が混ざったら読めない応答とする。
2. 道具の呼び出しの要素の `extra_content`（オブジェクト）を、その呼び出しの元の項目（`raw_calls` の要素）に保つ。差分（`delta`）の `extra_content` は、元の発言（`raw` のオブジェクト）の `extra_content` に保つ（複数の差分に現れたら、後の値で欄ごとに上書きする）。値は解釈せず、そのまま保つ。オブジェクトでなければ読めない応答とする。
3. 同じ送り先へ送り返すとき（`request.rs` の `same` の枝）は、元の発言をそのまま送るので、2 で保った `extra_content` も付く。別の送り先へは付けない（今の別の送り先の枝のとおり）。
4. `finish_reason` が `stop` で、`[DONE]` まで届き、道具の呼び出しが一つ以上揃っているときは、`FinishReason::ToolCalls` とする。`length` などは今までどおり。
5. `usage` は今の上書きの合わせ方のままでよい（最後の値が残る）。差分ごとに `StreamEvent::Usage` を出すのも今のままでよい。
6. 読めない応答の誤りの文を 1 の英文にする（「型の変更」の節）。

## 触るファイル

- `crates/benitoiteh/src/provider/openai_chat/`（`assembly.rs`、`request.rs`、テスト）、`crates/benitoiteh/src/provider/openai_chat.rs`、`crates/benitoiteh/src/provider/mod.rs`（`ProviderError`）と、その文の置き場所、`ProviderError` を網羅する所
- 必要なら `openai_responses.rs`・`anthropic.rs`（読めない応答の対応だけ）

## テスト

スキル `test-audit` の作成時の関門に従う。応答の写しは、2026-10-18 に Gemini から受け取った形（推論の印の値は短い偽の文字列に替える）を使う。

- 組み立て器の単体のテスト: `index` のない道具の呼び出しが一つと二つのとき、呼び出しが届いた順に組み立てられること。`finish_reason: "stop"` と `[DONE]` で `ToolCalls` になること。`extra_content` が元の項目に保たれること。`index` の有無が混ざったときと、`extra_content` がオブジェクトでないときは読めない応答になること。`[DONE]` の前に切れたら今までどおり `StreamDisconnected` になること。
- 要求の単体のテスト: 同じ送り先へ送り返す要求で、道具の呼び出しの要素と発言に `extra_content` がそのまま付くこと。別の送り先へは付かないこと。
- ループの結合のテスト（模擬の提供者）: Gemini の形の応答で、道具の呼び出し → 結果 → 次の要求（推論の印を送り返す）→ 完了の一連の処理が通ること。模擬の提供者で、二回目の要求の本文に推論の印が含まれることを確かめる。
- 既存の OpenAI の形（`index` のある断片、`tool_calls` の `finish_reason`）のテストがそのまま通ること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、変えた英文と、Gemini の形の応答の扱いの要約を書く。本物の Gemini での確かめは、取り込んだ後にオーケストレータが設計者の了承を得て行う。
