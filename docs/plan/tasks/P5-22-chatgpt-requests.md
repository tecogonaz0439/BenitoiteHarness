# P5-22 Sign in with ChatGPT の要求（`auth = "chatgpt"` の提供者）

## 目的

設計書 02-01「Sign in with ChatGPT」の「要求」「誤り」と、05-01「login と logout」の設定の形を実装する。`kind = "openai-responses"` と `auth = "chatgpt"` の提供者で、P5-21 の保存したトークンを使って `https://api.openai.com/v1/responses` を呼ぶ。この作業で本物の OpenAI の口を呼ばない（模擬の提供者で確かめる）。道具の定義の形を本物の口が受け付けるか（BH-115）は、取り込んだ後に設計者の了承を得て確かめる。

## 読む設計書

- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「Sign in with ChatGPT」「OpenAI の Responses API」「認証の情報」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」「login と logout」
- 既存のもの: `crates/benitoiteh/src/provider/openai_responses/`（要求の組み立て、誤りの分類）、`src/config/mod.rs`（提供者の設定、`api_key`）、`src/agent/transport.rs`（アダプタの作成、キーの渡し方）、P5-21 の `src/auth/`（使えるアクセストークンを返す関数、強制して更新する口）

## 作るもの

### 1. 設定

- 提供者の設定に `auth = "chatgpt"` を受け付ける。`kind` は `openai-responses` だけを許し、`base_url` と `api_key` を書けば設定の誤りとする。送り先は `https://api.openai.com/v1` に固定する。
- `doctor` は、`auth = "chatgpt"` の提供者について、保存した許可があるか（と表示用の情報）を示し、トークンを示さない。

### 2. 要求

- 要求のたびに、P5-21 の関数で使えるアクセストークンを得て、`Authorization: Bearer` に入れる（API キーの代わり）。トークンは要求の組み立ての外で扱い、記録と診断に出さない。
- 要求の本文は今の `openai-responses` のアダプタで作り、`auth = "chatgpt"` のときは次のとおり変える。設計書の、受け付けない欄（`background`、`conversation`、`max_output_tokens`、`max_tool_calls`、`metadata`、`moderation`、`prompt`、`prompt_cache_retention`、`safety_identifier`、`temperature`、`top_logprobs`、`top_p`、`truncation`、`user`、`previous_response_id` など）を送らない。システムプロンプトは `instructions` で送り、`role: "system"` の項目を送らない。`store: false`、`stream: true` とする。今のアダプタが送る欄のうち、上の一覧に当たるものを完了の報告に書く。
- 401 を受けたら、一度だけ強制して更新し、同じ要求を送り直す。更新が永久の誤りなら、ログインのやり直しを示して止める。

### 3. 誤り

- `subscription_sharing_usage_limit_exceeded`（429）は、やり直す誤りにせず止め、利用の上限に達したことと `https://chatgpt.com/settings/usage` を示す。`subscription_sharing_user_not_eligible`（403）と `subscription_sharing_unsupported_capability`（400）も、やり直さずに止める（誤りの文は、今の提供者の誤りの形に合わせる）。ほかの誤りの扱いは今の `openai-responses` と同じ。

## 触るファイル

- `crates/benitoiteh/src/config/mod.rs`、`src/provider/openai_responses/`、`src/agent/transport.rs`、`src/cli/doctor/`
- テスト: `tests/provider_openai_responses.rs`、`tests/config.rs`、`tests/doctor.rs`（模擬の提供者と、P5-21 の模した保存）

## テスト

- 設定: `auth = "chatgpt"` を受け付け、`base_url`・`api_key` との組み合わせと `kind` の違いを断ること。
- 要求: 模擬の提供者に届いた要求の本文が、受け付けない欄を含まず、`instructions` を持ち、`role: "system"` の項目を持たず、`store: false` であること。`Authorization` に保存したトークンが入ること。
- 401 で一度だけ更新して送り直すこと。二度目の 401 で止めること。
- 三つの `subscription_sharing_*` の誤りで、やり直さずに止め、決まった文を示すこと。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、`auth = "chatgpt"` のときの要求の本文の例を書く。
