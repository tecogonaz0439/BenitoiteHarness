# P5-21 Sign in with ChatGPT のログイン・保存・更新・ログアウト

## 目的

設計書 02-01「Sign in with ChatGPT」と 05-01「login と logout」の【決定】のうち、許可を得て保存し、更新し、取り消す部分を実装する。`benitoiteh login chatgpt` と `benitoiteh logout chatgpt` を加える。要求の側（`auth = "chatgpt"` の提供者で Responses API を呼ぶ）は次の作業（P5-22）で行う。

この作業で本物の OpenAI の口を呼ばない。テストは、OpenAI の認可の口・トークンの口・JWKS・取り消しの口を模した手元の HTTP の口（今の模擬の提供者と同じ作り）で行う。本物の口でのログインの確かめは、取り込んだ後に設計者が行う（設計者のアカウントを使う）。

## 読む設計書

- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「Sign in with ChatGPT」「認証の情報」「HTTP の通信」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「コマンドラインの形」「login と logout」「データの置き場所」
- 既存のもの: `crates/benitoiteh/src/http/`（`ureq` の要求、時間の上限）、`src/cli/`（サブコマンドの解釈）、`src/config/`（データの置き場所）、`src/session/`（0700・0600 の作り方、一時ファイルと名前の付け替え）、`src/provider/images.rs`（base64 の符号化。base64url の解読を加える）、`graviola` の `signing::rsa::VerifyingKey`（依存の木にある版）

## この作業が認める依存

- `graviola` を直の依存に加える（版は今の `Cargo.lock` の `=0.4.1`。`rustls-graviola` が既に使う版と同じにする）。【要検証】BH-114: JWKS の `n`・`e` から PKCS#1 の DER を組み立てて `VerifyingKey::from_pkcs1_der`（か同じ働きの口）で鍵を作り、`verify_pkcs1_sha256` で確かめられること。できなければ作業を止めて報告する（ほかの依存を加えない）。`docs/plan/10-interfaces.md` の依存の表への追加はオーケストレータが行う。

## 作るもの

### 1. ログイン（`benitoiteh login chatgpt`）

- 保存のディレクトリ `$XDG_STATE_HOME/benitoiteh/auth/`（0700）から、発行された `client_id` と `ext_agent_host_id` を読む。`ext_agent_host_id` がなければ、`urn:uuid:` の形の乱数（UUID の版 4 の形）を作って保存する。
- PKCE（S256。検証の値は 32 バイト以上の乱数を base64url）、`state`（32 バイトの乱数）、`nonce` を作る。
- `127.0.0.1` に、OS が選ぶ番号で待ち受ける。`GET /auth/callback` だけを受け、`Host` の値が `127.0.0.1:<番号>` であることを確かめる。ほかの道は 404 を返す。待つ時間の上限は 10 分とし、Ctrl-C で取り消せる。
- 認可の URL（設計書の欄）を、macOS は `/usr/bin/open`、Linux は `xdg-open` で開き、開けなければ URL を標準エラー出力に示す。URL と取り消しの方法は、どちらでも示す。
- 戻りの `state` を時間の一定な比べ方で確かめ、違えば捨てて待ち続ける。`error=access_denied` なら終える。戻りの `client_id` が保存した ID と違えば断る（初めてのときは、戻りの ID を保存する）。
- トークンの口に、フォームの形で code を交換する（時間の上限を掛け、応答の時間切れの後に送り直さない。code は一度しか使えないため）。
- `openid-configuration` から `jwks_uri` を得て JWKS を取り、ID トークンの `kid` の鍵で RS256 の署名を確かめ、`iss`（`https://auth.openai.com`）、`aud`（発行された `client_id`）、`exp`（今より後）、`nonce` を確かめる。得た許可の `scope` に `chatgpt.tokens.use.direct` がなければ断る。
- 保存する（2.）。成功したら、ID トークンのメールの宛先など表示用の情報だけを示し、トークンを示さない。

### 2. 保存

- `chatgpt.json` に、`client_id`、`ext_agent_host_id`、アクセストークン、リフレッシュトークン、アクセストークンの期限、`earliest_refresh_at`、表示用の情報を書く。0600 の一時ファイルに書いて `fsync` し、名前を替える。ディレクトリは 0700 で、所有者と権限を確かめる（違えば断る）。
- トークンは、診断、ログ、セッションの記録、`Debug` の表示に出さない（型の `Debug` を手で書いて伏せる）。

### 3. 更新（P5-22 から使う関数）

- 「使えるアクセストークンを返す」関数を作る。期限まで 60 秒以上あり、`earliest_refresh_at` より前なら、そのまま返す。そうでなければ、`auth/` の錠のファイル（`flock` の排他）を取り、保存を読み直し、ほかのプロセスが既に更新していればそれを返す。まだなら、フォームの形の更新の要求（`grant_type=refresh_token`、`client_id`、`refresh_token`、`resource`）を送り、受け取ったトークンをすぐに保存してから返す。更新が `invalid_grant`・`refresh_token_reused`・`refresh_token_expired`・`refresh_token_invalidated` で断られたら、トークンを消し、`benitoiteh login chatgpt` をやり直すよう示す誤りを返す。
- 401 を受けたときに一度だけ強制して更新する口も設ける（P5-22 が使う）。

### 4. ログアウト（`benitoiteh logout chatgpt`）

- `openid-configuration` の `revocation_endpoint` に、`token`（リフレッシュトークン）、`token_type_hint=refresh_token`、`client_id` を 10 秒の上限で送る（空の 200 を成功とする）。結果によらずトークンを消し、`client_id` と `ext_agent_host_id` は残す。取り消しを確かめられなければ、そのことを示す。

## 触るファイル

- 新しい `crates/benitoiteh/src/auth/`（ログイン、保存、更新、JWT と JWKS の検証）
- `crates/benitoiteh/src/cli/`（`login`・`logout` のサブコマンド、文言）
- `crates/benitoiteh/src/provider/images.rs` か共通の場所（base64url の解読）
- `crates/benitoiteh/Cargo.toml`、`Cargo.lock`
- テスト: 新しい `tests/auth_chatgpt.rs`（模した口で）、JWT の検証の単体のテスト（試し用の RSA の鍵で署名した ID トークン。鍵はテストの中で `graviola` の署名の口で作るか、固定の試しの鍵を使う）

## テスト

- ログイン: 模した口で、認可の URL の欄、戻りの受け付け（`state` の違い、`Host` の違い、ほかの道、`client_id` の違い、`access_denied`）、code の交換、ID トークンの検証（署名、`iss`、`aud`、`exp`、`nonce` のそれぞれの誤り）、`scope` の欠け、保存の権限。
- 保存: 0600・0700 で、名前の付け替えで書くこと。トークンが `Debug` と誤りの文に出ないこと。
- 更新: 期限の前は送らないこと、期限の近くで送り、受け取ったトークンを保存すること、二つのプロセス（か二つのスレッドと錠）が同時に更新しても一度しか送らないこと、永久の誤りでトークンを消すこと。
- ログアウト: 取り消しの要求の形、失敗しても消すこと、`client_id` と `ext_agent_host_id` を残すこと。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、BH-114 の確かめの結果（`graviola` で RS256 を確かめた形）と、実行ファイルの大きさの前と後を書く。

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。行の番号は `main`（`2320000`）のもの。P5-20 を取り込んだ後に始めるので、`src/cli/mod.rs`・`src/config/mod.rs` の行の番号は目安とする。

1. **コマンド**: `src/cli/mod.rs` の `run()` の `sessions` の分岐（78 行目付近）の後に `login`・`logout` を加え、新しい `src/cli/login.rs`（`sessions.rs` の `run(args) -> Result<u8, CliError>` と同じ形）に置く。続く引数がちょうど `chatgpt` の一つでなければ使い方の誤り（終了状態 2）。`CliError` に認証の誤りを加え、終了状態は、成功 0、拒否・検証の失敗・通信・時間切れ・保存の権限の誤り 1、使い方の誤り 2、Ctrl-C と SIGTERM 130 とする。`HELP`（`src/cli/text.rs:3-9`）に二つのコマンドを足し、認証の文言は新しい `src/auth/text.rs` にまとめる。
2. **HTTP**: ストリームでないフォームの POST がないので、`HttpClient::post_form(uri, headers, body, limit) -> Result<HttpResponse, HttpError>` を加える（`request()` に Content-Type の引数を足し、要求ごとに新しい接続を作る）。リダイレクトは今のまま辿らない。ureq は自動で送り直さないので、code を二度送らない。時間の上限は、login と更新が接続 10 秒・頭 30 秒・合計 60 秒、logout はすべて 10 秒。本文の上限は、トークンの応答と openid-configuration が 64 KiB、JWKS が 256 KiB、ID トークンが 16 KiB（ちょうど三つの部分でなければ断る）。JSON は型付きの構造体で読み、知らない欄は無視する。フォームと URL の符号化は自前で書く（RFC 3986 の非予約の文字だけをそのまま出す。戻りの解読では `+` を空白にする）。
3. **暗号（BH-114）**: `graviola = { version = "=0.4.1", default-features = false }` を直の依存に加える。`graviola::signing::rsa::VerifyingKey::from_pkcs1_der` と `verify_pkcs1_sha256(signature, message)`（`message` は `header.payload` の ASCII。関数の中で SHA-256 を取る）を使う。`n`・`e` から PKCS#1 の DER（`30 len (02 len n)(02 len e)`。INTEGER は先頭の 0x00 を落とし、最上位のビットが立てば 0x00 を一つ足す。長さは最短の形）を自前で組み立てる。署名の長さが法の長さ（先頭の 0 を落とした `n` のバイト数）と等しいことを自前で確かめる（graviola は確かめない）。SHA-256 は今の `sha2`、乱数は今の `getrandom::fill`。base64url（パディングなし。解読は `=` と字母の外の文字を断る）は新しい `src/auth/base64url.rs` に置き、`images.rs` は変えない。時間の一定な比較は自前で書く（長さを確かめてから XOR を畳む）。lint（`indexing_slicing`・`arithmetic_side_effects` など）に合わせ、`push`・`extend`・`checked_*` で書く。
4. **保存**: `DataPaths` に `auth/` の場所を加えてよい（`src/config/paths.rs`）。作り方は `src/ui/tui/history.rs`（ディレクトリの記述子からの `openat`、`NOFOLLOW`、所有者・0600・`nlink == 1` の確かめ、`flock`、一時ファイル・`sync_all`・`renameat`、失敗したら一時ファイルを消す）と `src/session/mod.rs` の `private_metadata`・`create_file`・`sessions_root` に倣う。一時ファイルは作った後に `fchmod` で 0600 にし、名前を替えた後にディレクトリにも `sync_all` を掛ける。錠のファイルは `auth/chatgpt.lock`。読むファイルは 64 KiB を上限にする。トークンは `Debug` で伏せる型（`config/credentials.rs:132-141` の `ApiKey` に倣う。`Display` を持たせない）に包む。P5-20 で `zeroize` が入っていれば、トークンの型に使ってよい（ほかの依存は加えない）。
   - `chatgpt.json` の形: `{version: 1, client_id, ext_agent_host_id, account: {email, subject} | null, tokens: {access_token, refresh_token, expires_at, earliest_refresh_at | null, scopes: []} | null, saved_at}`。時刻は Unix の秒。login・更新・logout の書き込みは、どれも錠の中で行う。保存の型と保存の関数は公開する（P5-22 のテストが模した保存を書くため）。
5. **更新の条件**（上の 3. の「期限まで 60 秒以上あり、`earliest_refresh_at` より前なら、そのまま返す。そうでなければ」を、設計書 02-01「更新」に合わせて読み替える）: 期限まで 60 秒を切っていて、かつ今が `earliest_refresh_at` 以降（値がなければ制限なし）のときだけ更新する。それ以外は保存のトークンを返す。401 を受けて強制する更新は `earliest_refresh_at` を無視し、拒まれたアクセストークンを引数で受けて、錠を取った後に保存の値が違えば（ほかのプロセスが更新済み）それを返す。`earliest_refresh_at` は、整数なら Unix の秒、文字列なら RFC 3339 の `...Z` の形として読み、ほかは `None` とする。保存は Unix の秒。
   - 永久の誤り（トークンを消し、login のやり直しを示す）: `invalid_grant`・`invalid_refresh_token`・`token_expired`・`refresh_token_expired`・`refresh_token_invalidated`・`refresh_token_reused`。`invalid_client` はトークンを残して、やり直せない誤りとする。通信の失敗と 5xx はトークンを消さない。更新は取り消しを受けず、終えて保存する。更新の応答に `scope` か `refresh_token` がなければ前の値を保ち、更新の応答の `id_token` は使わない。
6. **ログインの細部**:
   - 認可の URL の欄は、`client_id`、`response_type=code`、`redirect_uri`、`scope`（空白は `%20`）、`resource`、`state`、`nonce`、`code_challenge`、`code_challenge_method=S256`、`ext_agent_host_id` の順とし、初めて（`client_id` が `dynamic_agent_client`）のときだけ `agent_name_hint=benitoiteh` を足す。`id_token_hint`・`login_hint`・`prompt` は送らない。
   - verifier・state・nonce は、32 バイトの乱数の base64url（43 文字）。`ext_agent_host_id` は UUID の版 4（第 6 バイトを `(b & 0x0f) | 0x40`、第 8 バイトを `(b & 0x3f) | 0x80`）に `urn:uuid:` を前に付け、ブラウザを開く前に保存する。
   - 呼ぶ順: openid-configuration の GET（ブラウザを開く前）、ブラウザ、トークンの POST、JWKS の GET。authorize と token の URL は設計書の固定の値を使い、openid-configuration からは `jwks_uri` と `revocation_endpoint` だけを使う（本番の値では `https` を求める）。openid-configuration の `issuer` の一致も確かめる。
   - 戻りの受け付け: `GET` 以外は 405、`/auth/callback` 以外は 404。`Host` が一つだけで `127.0.0.1:<番号>` と完全に一致しなければ 400。欄の重なり・解読の誤り・`state` がないか違うときは 400 を返して待ち続ける。接続ごとに読む時間の上限 5 秒、頭 8 KiB。`state` が正しければ、すぐに 200 と端末に戻る旨の短い文を返し、待ち受けを閉じる。`error` があれば終える（示すのは `[a-z0-9_]` で 64 文字以内の符号だけで、`error_description` は示さない）。
   - `client_id`: 戻りの値は `oaiapp_` で始まり、`[A-Za-z0-9_-]` で 128 文字以内でなければ断る。初めてなのに戻りに `client_id` がなければ断る。2 回目以降に戻りに `client_id` がなければ保存した値を使う。発行された ID は、`state` を確かめた時点で保存する（後で失敗しても二つ目の登録を作らないため）。
   - トークンの応答: `token_type` があれば Bearer（大文字と小文字を区別しない）に限り、`expires_in` は正の整数、`scope` と `refresh_token` は必須。
   - ID トークン: ヘッダの `alg` は `RS256` に限り、`kid` は必須、`crit` があれば断る。JWK は `kty=RSA` に限り、`use`・`alg` はあれば `sig`・`RS256` に限る。`iss` は `https://auth.openai.com` と完全に一致。`aud` は文字列か、`client_id` を含む配列（二つ以上なら `azp == client_id`）。`exp > 今`（ずれの猶予は置かない）。`sub` は空でない文字列。`nonce` は完全に一致。
   - `scope` に `chatgpt.tokens.use.direct` がなければ断り、トークンを保存しない（保存した `client_id` は残す）。文言で、ChatGPT のプランの利用を許してから login をやり直すよう示す。
   - 保存した `account.subject` があって新しい `sub` と違えば断り、別のアカウントを使うには `auth/chatgpt.json` を消すよう示す。
   - 表示: 待つ間は `Continue with ChatGPT`、URL、`Press Ctrl-C to cancel.` を標準エラー出力に示す。成功は `Signed in to ChatGPT as <email>.`（`ui::terminal_text` を通す。メールの宛先がなければ `Signed in to ChatGPT.`）。
7. **ブラウザと Ctrl-C**: macOS は `/usr/bin/open`、Linux は `xdg-open`（`PATH` から）を、標準入出力を null にし、親の環境から提供者のキーの変数（`src/config/mod.rs:24` の `PROVIDER_KEY_ENV_VARS`）を除いて `spawn` する。終わりを待つのは別のスレッドで行う（`xdg-open` が戻らないことがあるため）。Ctrl-C は `src/cli/signals.rs` の `Signals::new()` の印を CLI の側で作って渡し、待つ間は accept と 50 ms の休みを繰り返して印と期限（10 分）を見る。code の交換を始めたら打ち切らない。
8. **logout**: 保存がないかトークンがなければ `Not signed in to ChatGPT.` と示して 0 で終える（通信しない）。そうでなければ、錠を取り、openid-configuration と取り消しをそれぞれ 10 秒の上限で行い、`tokens` と `account` を消し、`client_id` と `ext_agent_host_id` を残す。取り消しを確かめられなければ、消したうえで、確かめられなかったことと ChatGPT の設定の画面で接続を切れることを示し、1 で終える。
9. **テスト**:
   - 口の差し替えは、公開の構造体（例: `auth::Endpoints { authorize, token, discovery, issuer }`）を引数で渡す形にする。CLI だけが本番の値（例: `Endpoints::official()`）を使う。環境の変数、設定の欄、`cfg(test)` での差し替えは設けない。テストの `Endpoints` だけが `http://127.0.0.1` を許す。
   - `login` は、URL を開く関数、待つ時間の上限、今の時刻を引数で受ける。テストは代わりの関数で URL の欄を確かめ、`TcpStream` で戻り先に GET を送る（`Host`・`state`・道の誤りもここで作る）。
   - 模擬の口は `tests/support/mock_provider.rs` の `MockProvider` を使う。`Content-Length` がない要求（GET）とフォームの本文を受けられるよう、`Content-Length` がなければ 0 とし、生の本文の欄を加え、JSON でない本文は `Value::Null` にする。
   - 試しの鍵は `graviola::signing::rsa::SigningKey::generate(KeySize::Rsa2048)` で作り（`OnceLock` で使い回す）、`sign_pkcs1_sha256` で署名する。公開鍵の `to_pkcs1_der` の出力と、自前の DER の組み立てが返すバイト列が一致することを単体のテストで確かめる（BH-114 の証拠）。
   - P5-22 で `doctor` に示す処理は、この作業では触らない。
