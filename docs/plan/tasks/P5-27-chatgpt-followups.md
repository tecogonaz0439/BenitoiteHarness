# P5-27 Sign in with ChatGPT の残り

## 目的

2026-10-20 の点検で見つかった、Sign in with ChatGPT（P5-21、P5-22）の残りを直す。ループバックの戻り先の待ち受け、後片付けの誤りが元の誤りを隠すこと、408・429 の扱い、更新の応答が壊れていたときのリフレッシュトークン、トークンを受け取った後のログインの失敗、要求の取り消しと更新の錠、誤りの文に残るトークンの一部の七つである。この作業で本物の LLM の提供者と OpenAI の口を呼ばない（模擬のサーバだけを使う）。

## 読む設計書

- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「Sign in with ChatGPT」（「ログイン」「更新」「誤り」「ログアウト」）
- [P5-21](P5-21-chatgpt-login.md)と [P5-22](P5-22-chatgpt-requests.md)の補足
- 既存のもの: `crates/benitoiteh/src/auth/mod.rs`、`src/auth/callback.rs`、`src/auth/store.rs`、`src/provider/openai_responses.rs`、`src/provider/openai_responses/authentication.rs`、`tests/auth_chatgpt.rs`、`tests/provider_openai_responses.rs`、`tests/support/mock_provider.rs`

## 作るもの

### 1. 戻り先の待ち受けを一つのスレッドの `poll` にする

設計書 02-01「ログイン」の【方針】のとおり、待ち受けのソケットと接続を `rustix::event::poll` でまとめて待つ。未完の接続は 64 まで保ち、超えたら最も古いものを閉じる。一つの接続は 5 秒・8 KiB までで要求の頭を読む。完全な戻りを受けたら、200 を書いて書く側を閉じ、残りの接続を閉じて返す。`rustix` に機能 `event` を加える（新しいクレートは加わらない）。

### 2. 後片付けの誤りで元の誤りを隠さない

ログインの失敗の後の `reset_unconfirmed_client` と、断られた更新の後のトークンを消す保存が失敗しても、元の誤りを返す（設計書 02-01「誤り」の【方針】）。

### 3. 408 と 429 で確かめていない `client_id` を残す

code の交換の前後のログインの失敗のうち、408・429・5xx の応答では、確かめていない `client_id` を `dynamic_agent_client` に戻さない（設計書 02-01「ログイン」）。

### 4. 更新の応答が確かめに通らないときのリフレッシュトークン

設計書 02-01「更新」の【方針】のとおり、2xx の応答が確かめに通らなくても、空でないリフレッシュトークンを読めれば保存してから誤りを返す。アクセストークンと期限は前の値を残し、`earliest_refresh_at` は消す。

### 5. トークンを受け取った後のログインの失敗で取り消しを送る

設計書 02-01「ログイン」の【決定】のとおり、code の交換でトークンを受け取った後にログインが失敗したら、受け取ったリフレッシュトークンの取り消しを、ログアウトと同じ形で一度だけ送り、結果によらず元の誤りを返す。結果は利用者に示さない。保存に同じ `client_id` のトークンがあり、同じアカウントのものか、受け取ったトークンのアカウントを確かめられなかったときは送らない。

### 6. 要求の取り消しと更新の錠

設計書 02-01「更新」の【方針】のとおり、更新の錠を待つ間とトークンの口へ送る前に、取り消しを受ける。送った後は保存まで待つ（最長 60 秒。画面には示さない）。

### 7. `auth = "chatgpt"` の誤りの文

設計書 02-01「誤り」の【方針】のとおり、`auth = "chatgpt"` の提供者の誤りでは、提供者の `message` を示さず、`code` と `type` は伏せた後に `^[a-z0-9_.]{1,64}$` に合うときだけ、`Retry-After` は数字だけのときだけ示す。API キーの提供者は今のままとする。

## テスト

- (a) 待ち受け（項目 1）: 戻りの要求を二つに分けて送っても 200 になること。頭を送り終えない接続を 16 個と 100 個開いた後に、正しい戻りを送ると 200 になること。完全な戻りの後に、残りの接続が閉じられること。
- (b) 後片付け（項目 2）: 保存のファイルを読めない形（0644）にしたうえで、ログインと更新を失敗させると、元の誤りが返ること。
- (c) 408・429（項目 3）: 今の表のテストに 408 と 429 を加え、確かめていない `client_id` が残ること。
- (d) 壊れた更新の応答（項目 4）: 200 で `refresh_token` だけを含む応答のあと、誤りが返り、保存に新しいリフレッシュトークンと前のアクセストークンがあり、次の更新が新しいリフレッシュトークンを送ること。
- (e) 取り消し（項目 5）: ID トークンの確かめの前に失敗させると、模擬のサーバに取り消しの要求が一度だけ届き、元の誤りが返ること。保存に同じ `client_id` で同じアカウントのトークンがあるとき、受け取ったトークンのアカウントを確かめられないときは、取り消しが届かないこと。アカウントが違うとき（`AuthError::Account`）は届くこと。取り消しが 500 でも、返る誤りと示す文が変わらないこと。
- (f) 取り消しと錠（項目 6）: ほかのスレッドが更新の錠を持ったまま、要求のストリームを取り消して捨てると、すぐに返ること。トークンの口へ送った後に取り消しても、受け取ったトークンが保存されること。
- (g) 誤りの文（項目 7）: 提供者の `message` と `code` にトークンの一部（先頭の 10 文字など）を含めても、どの誤りの文にも出ないこと。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- `rustix` の機能 `event` を加えた前と後の、release の実行ファイルの大きさを測り、完了の報告に書く。`docs/plan/10-interfaces.md` の「依存のクレート」の `rustix` の行に、機能 `event` と、その理由（設計書 02-01「ログイン」）と、測った大きさの変化を書く。

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。行の番号は `main`（`564a32c`）の `crates/benitoiteh/` からの相対である。

1. **待ち受け（項目 1）**:
   - 今の箇所: `src/auth/callback.rs`。`MAX_READERS = 4`（11）、`HEAD_LIMIT = 8192`（12）、`wait`（23-117）の中で、受けた接続を非ブロックで一度読み（62-83）、未完なら最も古い読み手を閉じて（85-90）、接続ごとにスレッドを起こす（101-115）。一つの接続を読む時間は 1 秒（95-97）で、P5-21 の補足（5 秒・8 KiB）と違う。
   - 直し: スレッドを使わない。待ち受けのソケットと接続を非ブロックにし、`rustix::event::poll` に、待ち受けのソケットと各接続の `PollFd`（読めるか）を渡して待つ。待つ時間は、全体の残り、最も早い接続の期限、取り消しを見る間隔（50 ミリ秒）の最小とする。起きたら、先に読める接続を `read_head` で読み、次に待ち受けから `WouldBlock` まで受ける（今の `accepted` と `transient_accept` を使う）。未完の接続が 64 を超えたら、最も古いものを閉じる。接続ごとの期限は受けてから 5 秒とする。誤りの接続には今どおり `reply` で状態を返して閉じる。
   - 完了の順（今の、閉じる処理と完了の競合（46-48、85-90、104、212-220）を消す）: 完全な戻りを読んだら、その接続をブロックに戻して `reply(200)` を書き、`shutdown(Write)` してから返す。残りの接続は `wait` から返るときに手放して閉じる。待ち受けのソケットは、呼ぶ側（`src/auth/mod.rs` の 385 の `drop(listener)`）が閉じる。
   - `Cargo.toml` の 38 行の `rustix` の機能に `"event"` を加える（`rustix` 1.1.5 の機能。新しいクレートは加わらない）。
   - テスト: `tests/auth_chatgpt.rs` の `security_idle_connections_do_not_block_a_callback`（884-）と `rereview_complete_callback_is_accepted_after_four_or_eight_idle_connections`（1355-）に倣う。数を 16 と 100 にした場合を加え、分けて送る場合は、頭の前半を送ってから少し待ち、後半を送る。5 秒の期限そのものを待つテストは重いので置かない（期限の計算を小さな関数にして単体のテストで確かめてよい）。
2. **後片付け（項目 2）**:
   - 今の箇所: `login` の 336-341（`reset_unconfirmed_client(...)?` の `?` が元の誤りを捨てる）、`refresh` の 584-588（`LoginRequired` のときの `locked.save(&saved)?`）。
   - 直し: どちらも後片付けの結果を捨て（`let _cleanup = …;`）、元の誤りを返す。
   - テスト: 保存のファイル（`chatgpt.json`）を 0644 にすると、`Store` の確かめで読み書きが失敗する（`tests/auth_chatgpt.rs` の 699 と 720 の使い方）。始めから 0644 にすると最初の読み取りで落ちるので、トークンの口の応答を `Delivery::Controlled`（`tests/support/mock_provider.rs` の 25-28）で止め、要求が届いた（`entered`）後に 0644 にしてから応答を放す（`release`）。ログインは交換を 400 の `invalid_request` で断らせ（確かめていない `client_id` を戻す後片付けが走る）、更新は `invalid_grant` で断らせる（トークンを消す保存が走る）。それぞれ、返る誤りが `Storage` でなく元の誤りであることを確かめる。権限の確かめは持ち主によらないので、root で走る Linux のコンテナでも通る。
3. **408・429（項目 3）**: `impl From<HttpError> for LoginFailure` の 232 を `matches!(status.head.status, 408 | 429 | 500..=599)` にする。テストは `tests/auth_chatgpt.rs` の `security_unconfirmed_client_recovers_but_successful_exchange_pins_client`（934-）の表に 408 と 429 を加え、998 あたりの確かめ（残る ID）を同じく通す。
4. **壊れた更新の応答（項目 4）**:
   - 今の箇所: `exchange`（284-298）は応答を `TokenResponse` として読み、読めなければ `Validation` にする。`TokenResponse::tokens`（141-172）の確かめで落ちても、`refresh`（592）は保存せずに返す。
   - 直し: 更新の側だけで、2xx の応答の中身（`Zeroizing` のバイト列）を受け取り、`TokenResponse` として読めないか `tokens()` で落ちたときに、`{"refresh_token": "<文字列>"}` だけを読む小さな型で読み直す。空でなければ、保存の `tokens.refresh_token` を替え、`earliest_refresh_at` を `None` にして保存し、`Validation` を返す。アクセストークンと `expires_at` は替えない。ログインの交換は変えない（受け取った値を保存する前に失敗するのは項目 5 で扱う）。
5. **ログインの失敗で取り消し（項目 5）**:
   - 今の箇所: `login_attempt` の交換（430-440）の後の失敗（441-491。確かめた印の保存、JWKS の取得、`jwt::verify`、`tokens()`、許可の範囲、アカウントの違い、最後の保存）。取り消しは `logout`（611-652）の 623-642 の中にしかない。
   - 直し: 取り消しの要求を、`logout` から関数に切り出す（例 `fn revoke(endpoints, client_id, refresh_token) -> bool`。`client(true)`、`discovery`、`revocation_endpoint`、フォームの `token`・`token_type_hint=refresh_token`・`client_id`）。`LoginFailure` に、交換の後に失敗したときの受け取ったリフレッシュトークン（`Option<Secret>`）とアカウントの `sub`（`Option<String>`。`jwt::verify` が通っていれば）を持たせる。`login` の誤りの枝で、保存の錠を取り、次のどちらかなら送らない: 保存に `tokens` があり、`client_id` が同じで、保存の `account.subject` が受け取った `sub` と同じとき、または受け取った `sub` が分からないとき。それ以外は、錠を持ったまま一度だけ送り、結果を捨てる（判定と送りを一つの錠の中で行うのは、ログアウトと同じ形にし、間にほかのプロセスが同じアカウントで保存するのを防ぐため）。その後、項目 2 の後片付けを行い、元の誤りを返す。応答にリフレッシュトークンがなければ送らない（`tokens()` が `Validation` で落ちる場合。アクセストークンは 1 時間で切れる）。
   - 取り消しが同じ許可のほかのトークンに及ぶかは、設計書の BH-117 で【要検証】とした。この作業では確かめない。
   - テスト: 模擬のサーバの応答の並びで、交換を 200 で通し、JWKS を 500 にする（`sub` が分からない場合）か、別の鍵で署名した ID トークンにする。保存に前のトークンがない場合は取り消しが届き、同じ `client_id` のトークンがある場合は届かないこと。保存に別のアカウントのトークンがある場合（`AuthError::Account`）は届くこと。取り消しの口が 500 を返しても、`login` の誤りが `Account` のままであること。届いた取り消しのフォームの `token` が受け取ったリフレッシュトークンであることを確かめる。
6. **取り消しと錠（項目 6）**:
   - 今の箇所: `Resend` の `Drop`（`src/provider/openai_responses.rs` の 237-246）は、更新のスレッド（177-178 と 435-437 で起こす）の終わりを待つ。更新は `Store::lock`（`src/auth/store.rs` の 124-140。132 の `flock(LockExclusive)`）で錠を限りなく待つ。
   - 直し: `Resend` に取り消しの印（`Arc<AtomicBool>`）を持たせ、`ChatGptTokens::access_token`・`refresh`（`src/provider/openai_responses/authentication.rs` の 35-43）を経て `auth::access_token`・`force_refresh` に渡す。`auth` の側は、印を受ける錠の取り方（例 `Store::lock_cancellable(&AtomicBool)`。`flock(LockExclusiveNonBlocking)` を、`EWOULDBLOCK` の間 10〜50 ミリ秒の間を空けて繰り返し、その合間に印を見て、立っていれば `AuthError::Cancelled`）を足し、`refresh` は、錠を取った後とトークンの口へ送る直前に印を見る。送った後は印を見ない。取り消しの印は、ストリームの取り消し（`ResponsesCancellation`）と `Resend` の `Drop` で、`join` の前に立てる。ログインとログアウトの錠は今のまま（印を受けない）とする。
   - テスト: 錠を別のスレッドで持ったまま、要求のストリームを作って取り消して捨て、捨てる処理が短い時間（例えば 1 秒以内）で返ることを確かめる。送った後の取り消しは、模擬のサーバのトークンの口の応答を遅らせ（`Delivery`）、その間に取り消し、捨てた後に保存のリフレッシュトークンが替わっていることを確かめる。
7. **誤りの文（項目 7）**:
   - 今の箇所: `OpenAiResponsesError::redact`（77-100）は、トークンの全体と一致する並びだけを置き換える。`poll` の誤りの枝（306-322）が今のトークンと断られたトークンで伏せる。誤りの詳しい内容は 519-532 で読み、`message` を残す。
   - 直し: `auth = "chatgpt"` の要求（`Authentication::Chatgpt` で作ったストリーム）では、伏せた後に、`details.message` を `None` にし、`code` と `error_type` は `^[a-z0-9_.]{1,64}$` に合わなければ `None` にし、`retry_after` は ASCII の数字だけでなければ `None` にする。正規表現のクレートは使わず、バイトの判定で書く。`subscription_sharing_*` の判定（533 あたり）は、この絞りの前の値で行う。
   - テスト: `tests/provider_openai_responses.rs` の `chatgpt_resend_errors_redact_both_rejected_and_current_tokens`（1555-）を改める。1586-1588 の期待（`message` に伏せ字が入ること）を、`message` が示されないことに替え、`message` と `code` にトークンの先頭の 10 文字を含めた応答で、誤りの `Display` と `Debug` のどちらにも出ないことを確かめる。
8. **示す文**: 取り消しの結果（項目 5）と、送った後の待ち（項目 6）は、画面にも記録にも示さない（2026-10-20 に設計者が決めた）。新しい文言は要らない。
