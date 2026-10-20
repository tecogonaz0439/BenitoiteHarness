# P5-22 Sign in with ChatGPT の要求（`auth = "chatgpt"` の提供者）

## 目的

設計書 02-01「Sign in with ChatGPT」の「要求」「誤り」と、05-01「login と logout」の設定の形を実装する。`kind = "openai-responses"` と `auth = "chatgpt"` の提供者で、P5-21 の保存したトークンを使って `https://api.openai.com/v1/responses` を呼ぶ。この作業で本物の OpenAI の口を呼ばない（模擬の提供者で確かめる）。道具の定義は、2026-10-20 に設計者の了承を得た本物の要求で、最上位の `function` の形のまま受け付けられることを確かめた（設計書 02-01「Sign in with ChatGPT」の「要求」）。

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

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。行の番号は `main`（`e15f50b`）のもの。P5-21 の型と関数の名前（例: `auth::Store`、保存の型、使えるトークンを返す関数、強制して更新する口、`Endpoints`）は作業中に見た仮のものなので、P5-21 を取り込んだ `main` の実際の名前に合わせる。

1. **設定**（`src/config/mod.rs`）: `ProviderConfig`（134-144 行。`deny_unknown_fields`）に `auth: Option<ProviderAuth>`（値 `"chatgpt"` だけの列挙）を加える。検証は `UserConfig::parse` の提供者の繰り返し（404-414 行）で行い、`kind` が `openai-responses` でない、`base_url` がある、`api_key` がある、のどれかなら、値を含めない文（`config/text.rs`）で誤りとする。送り先は `official_base_url()`（152-157 行）の `https://api.openai.com/v1` のままでよく、機能は本家の Responses の機能（`provider/types.rs:394-401`）になる。`ProjectConfig` は `auth` を書けない（`deny_unknown_fields`）ので、断られることだけをテストで確かめる。`ResolvedExec`（538-550 行）に `auth` を加え、`resolve_exec`（472-497 行）で写す。`AgentConfig` には欄を足さない（構造体を直書きするテストが多いため）。
2. **起動**（`src/cli/exec.rs` 301-306 行付近）: `auth = chatgpt` なら、P5-21 の保存を開いて読む（通信しない）。保存がないかトークンがなければ、`Not signed in to ChatGPT. Run benitoiteh login chatgpt.` の形の文で起動を止める（終了状態 2）。`Agent::new`・`Agent::resume` の引数は変えず、作った後にアダプタの認証を差し替える組み立ての関数（例: `Agent::with_chatgpt_tokens(self, source) -> Result<Self, AgentError>`。Responses のアダプタ以外では誤り）を呼ぶ。対話の画面も同じ起動の処理を通る。保存のファイルは、サンドボックスの拒む場所（`cli/mod.rs:122-126` の `data_dir`）と道具の除く場所（`tools/exclude.rs:37`）に入っている。
3. **要求の本文**（`provider/openai_responses/request.rs:79-107`）: 今送る欄は `model`、`instructions`、`input`、`tools`、`tool_choice`、`parallel_tool_calls`、`store: false`、`stream: true`、`include: ["reasoning.encrypted_content"]`、`reasoning`、条件付きの `max_output_tokens`（94-96 行）と `prompt_cache_key`（97-99 行）である。受け付けない欄の一覧に当たるのは `max_output_tokens` だけなので、`auth = chatgpt` のときは送らない（`capabilities.max_output_tokens` は圧縮の判定に使うので、設定では断らない）。システムプロンプトは既に `instructions` で送り、`role: "system"` の項目は作らない（確かめのテストだけを加える）。ほかの欄はそのまま送る。道具の定義を最上位の `function` の形のまま送れることは、上の「目的」に記した本物の要求で確かめた。
4. **認証のヘッダ**: 今は `build_request`（`request.rs:71-77`）が `Bearer` を入れ、作った要求を `retry.rs:18-25`・`58-105` のやり直しで使い回す。`auth = chatgpt` のときは `build_request` ではヘッダを付けず、`OpenAiResponsesAdapter::stream`（`provider/openai_responses.rs:143-161`）の中で、送るたびに使えるトークンを得て、要求の頭の写しに `set_sensitive(true)` で `authorization` を入れる。アダプタの認証の持ち方を `api_key: String`（122 行目）から、API キーか ChatGPT の許可かの列挙に替える（`Send`、`Debug` は値を出さない）。伏せる処理（71-94 行の `redact`）は、その要求に使ったトークンの文字列で行う。
5. **401**: 今は 401 も 403 も `NonRetryableKind::Authentication`（`openai_responses.rs:301-304`）。送り直しは `retry.rs` を変えずに、アダプタの `stream()` の中で行う（401 は応答の頭で返るので、出力の課金は起きていない）。401 なら、拒まれたトークンを渡して強制の更新を一度だけ呼び、新しいトークンで一度だけ送り直す。二度目の 401、または更新が永久の誤りなら、新しい `NonRetryableKind::LoginRequired` で止める。強制の更新が通信の失敗なら `Authentication`、送る前の更新が通信の失敗なら `Retryable { retry_after_secs: None }` とする。403 では更新しない。401 を見分けるため、`OpenAiResponsesError::Provider` に `status: Option<u16>` を足す。
6. **誤りの分類**（`api_error`、`openai_responses.rs:282-339`）: `details` を作った直後、301 行目より前で、`error.code` と `error.type` の両方を見て、三つの符号を状態によらず（ストリームの中の誤りは `status = None` で来る。`assembly.rs:107-115`）分類する。`NonRetryableKind`（`provider/types.rs:268-279`）に次を加え、表示の文（303-310 行）と定数（`provider/text.rs`）を加える。どれも `RequestRejected` と別なので、画像を外す送り直しは起きない。

   | 種類 | 文 |
   |---|---|
   | `SubscriptionUsageLimit` | `ChatGPT plan usage limit reached. See https://chatgpt.com/settings/usage.` |
   | `SubscriptionNotEligible` | `This ChatGPT account is not eligible for ChatGPT plan usage.` |
   | `SubscriptionUnsupported` | `ChatGPT plan usage does not support this request.` |
   | `LoginRequired` | `ChatGPT sign-in is no longer valid. Run benitoiteh login chatgpt.` |

7. **doctor**（`cli/doctor/mod.rs` の `providers()` 276-332 行、`credential()` 334-355 行）: `auth = chatgpt` の提供者は、`credential()` の代わりに保存の状態を示す。doctor はファイルやディレクトリを作らない（P5-21 の保存を開く関数が `auth/` を作るなら呼ばず、`symlink_metadata` で有無を先に確かめ、あるときだけ開く。必要なら「あるときだけ開く」関数を足す。`tests/doctor.rs:395` を壊さないため）。許可がなければ `Not signed in to ChatGPT; run benitoiteh login chatgpt.`（状態は今のキーと同じく、既定の提供者なら使えない、ほかは注意）。保存が壊れているか権限が違えば使えない。許可があれば確かめたとし、メールの宛先（`ui::terminal_text` を通す）、アクセストークンの期限の UTC（過ぎていれば次の要求で更新する旨）、`chatgpt.tokens.use.direct` の有無を示す。トークンは値も長さも示さない。doctor は更新しない（通信しない）。
8. **テスト**: アダプタは `tests/provider_openai_responses.rs`（145 行目からの形。`MockProvider::start`、`server.requests()`）に倣い、`Destination.base_url` に模擬の口を与えて作る。本文（`max_output_tokens` がない、`instructions` がある、`role: "system"` がない、`store: false`・`stream: true`、`authorization` が保存のトークン）、401 の一度だけの更新（401、トークンの口の 200、SSE の順。二度目の `Bearer` が新しいトークン）、二度目の 401（要求がちょうど三つで `LoginRequired`）、三つの符号（頭の誤りとストリームの中の誤りの両方。763 行目からの表に加える）を確かめる。偽の保存は、P5-21 の公開の保存の関数で、試しの `XDG_STATE_HOME/benitoiteh/auth/chatgpt.json` に書く。設定（`tests/config.rs`）、doctor（`tests/doctor.rs` の 226 行目の表の形。保存がないときにファイルが増えないこと、文と JSON にトークンが出ないこと）、exec（保存がないと終了状態 2 で、要求を送らないこと）も確かめる。
9. **決め方**: モデルの名前は確かめない。ログインしていなければ起動の時点で止め、要求の途中で許可が無効になったら `LoginRequired`（終了状態 1）とする。非対話の `exec` でも更新してよい（親のプロセスが、サンドボックスの外で行う）。複数のプロセスの更新は P5-21 の錠に任せる。API キーの提供者から `auth = chatgpt` に切り替えたセッションの再開は、この作業では扱わない。完了の報告に、`auth = chatgpt` のときの要求の本文の例を書く。

10. **実装で改めた点**（2026-10-20、取り込むときに追記）: `HttpClient::post_json()` は応答の頭を待たずに戻る（頭を待つ間も取り消せるようにするため）ので、401 は `stream()` が戻った後に分かる。そこで、上の 5. の「アダプタの `stream()` の中で」を、「`OpenAiResponsesStream` の側で、最初の事象を渡す前の 401 に限って」と改めた。送る前のトークンの取得も、取り消せるよう別のスレッドで行う。`retry.rs` と HTTP の層は変えていない。
