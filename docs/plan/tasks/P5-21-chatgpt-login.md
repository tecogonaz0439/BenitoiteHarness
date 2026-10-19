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
