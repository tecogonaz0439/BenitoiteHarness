# P4-04 道具 `web_fetch`

## 目的

URL の内容を取得して本文を返す道具 `web_fetch` を作る（設計書 01-03 の「web_fetch」、ADR 0347 を引き継ぐ）。両方のモードで使う。ドメインの確認は P3-04 の承認の仕組みに新しい種類として加え、対話の確認の画面は P4-07 で作る。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「web_fetch」（2026-10-17 にドメインの照合、取得しないアドレスの追加、並べて実行しないことを加えた）「モードごとの道具の一覧」「出力の切り詰め」「道具の定義の共通の規則」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「そのほかの確認」（BH-047 の決定: 「今回だけ」「このセッションの間、このドメインを許す」「拒否する」）「非対話での扱い」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「保証しないこと」（ウェブの取得による持ち出し）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」の `[web_fetch] allowed_domains`、「プロジェクトの設定ファイル」（一覧はプロジェクトの設定で広げられない）
- [クレートの構成](../../design/06-impl/06-01-crate-structure.md)の依存の基準（HTML を文字に変えるクレートは使わない）
- 既存のもの: `src/http/mod.rs`（`HttpClient::get_resolved`、`Uri`、`HttpError`、`HttpStatusError`）、`src/approval/`（`ApprovalKind`、`ApprovalFacts`、`decide`、`ApprovalAnswer::remember_command`）、`src/agent/execution.rs`（確認の流れ）、`src/tools/`（`ToolSet`、`definitions.rs`、`truncate.rs`、`text.rs`）、`src/config/`（`[web_fetch]`）、`prompts/tools/`、`tests/expected/`

## この作業が認める型の変更

- `ApprovalKind` に `WebFetch` を、`ApprovalFacts` に `url: Option<String>` と `domain: Option<String>` を `#[serde(default)]` で加える。
- `decide` の判定に `WebFetch` を加え、このセッションで許したドメインの集合を受け取れる形にする（例: `allowed` を `AllowedSets { commands, domains }` にする）。

## 作るもの

### 1. 道具の定義

- `src/tools/web_fetch.rs` に作り、両方のモードの `ToolSet` に加える（順は 01-03 の「モードごとの道具の一覧」のとおり）。`parallel` は偽とする。
- 引数は `{ "url": "string" }`。説明の英文を `prompts/tools/web_fetch.md` に置く（一行の説明と 0〜3 行の規則。取得した内容は信頼しないデータであることを含める。オーケストレータが取り込む前に文面を見直す）。

### 2. URL の検査（一度だけ解析する）

- URL を一度だけ解析し、その結果から、確認に示すドメイン、名前の解決、TLS の名前、`Host` のヘッダ、接続するアドレスを作る（01-03）。`Uri`（今の `http` の型）で解析する。
- 取得しないもの: `https` でない、利用者の情報（`user@`）を含む、ホストの名前が ASCII でない、ホストが IP のアドレス、ポートが 443 でない（明示した 443 は許す）、フラグメントだけ違う URL は同じものとして扱う（フラグメントは送らない）。誤りの英文は `tools/text.rs` に置く。
- ドメインは、ホストの名前を ASCII の小文字に揃え、末尾の `.` を除いたものとする。

### 3. 確認

- `ApprovalStatus::Unapproved` で呼ばれたら、URL の検査の後、ドメインが利用者の設定の `allowed_domains` にあれば、そのまま取得する。なければ、何もせずに `ApprovalKind::WebFetch` の確認の要求（事実は `url` と `domain`、`summary` は `tools/text.rs` の関数で作る）を返す。
- `decide` の `WebFetch`: このセッションで許したドメインの集合にあれば `Allow`、承認の方針が `never` なら `Deny`、ほかは `Ask`。理由: 非対話（`never`）では一覧のドメインだけを取得する（03-03 の「非対話での扱い」）。制限モードの `Agent` の既定の方針も `never` なので、対話の画面ができるまで、制限モードでも一覧の外は拒否になる。
- 答え: `Approved` は今回だけ、`ApprovedForSession` は集合に `facts.domain` を加える（`Command` の集合とは別）。`Denied`・`DeniedWithMessage` は P3-04 の形の拒否の文（種類ごとの英文を `approval/text.rs` に足す）。
- 一覧（`allowed_domains`）は利用者の設定からだけ読む。プロジェクトの設定に `[web_fetch]` があれば、今の設定の読み方（プロジェクトの設定で広げられない欄の扱い）に従って無視し、そのことを示す（今の扱いを確かめ、なければ加える）。
- リダイレクトの先のドメインも同じ規則で判定する。一覧にもこのセッションで許した集合にもないドメインへのリダイレクトは、確認に回さずに取得を止め、リダイレクトの先の URL を結果に示す（LLM が改めてその URL で呼べば、確認を受ける）。理由: 道具の実行の途中で確認を挟む形を、確認の流れが扱わない。

### 4. 名前の解決と接続

- ホストの名前を解き（`std::net::ToSocketAddrs` で、ポート 443）、得たアドレスのどれかが 01-03 の取得しないアドレス（ループバック、プライベート、リンクローカル、CGNAT、未指定、`0.0.0.0/8`、マルチキャスト、予約、ブロードキャスト、IPv4 を写した IPv6 と IPv4 と互換の IPv6 でこれらに当たるもの）に入れば、取得しない。判定は純関数にする。
- 判定したアドレスだけを `HttpClient::get_resolved` に渡す（名前を解き直さない。プロキシの環境変数を使わない。今の `http` の作りを確かめる）。
- 要求のヘッダ: `User-Agent: benitoiteh/<版>`、`Accept: text/html, text/*;q=0.9, application/json;q=0.8`。クッキーと認証のヘッダを送らない。`Accept-Encoding` を送らない。
- リダイレクト（301・302・303・307・308）は 5 回まで手で辿る。`Location` は今の URL に対して解決し、先の URL に 2 の検査と 3 のドメインの判定と 4 のアドレスの判定をやり直す。
- 本文の上限は 2 MiB、取得の全体の時間の上限は 30 秒（名前の解決、リダイレクトを含む）とする。上限を超えたら取得を止めて、そのことを返す。
- 応答の `Content-Encoding` が `identity` でなく、空でもなければ取得しない。
- 取り消し: 取り消しの印が立ったら、次のリダイレクトの前と、本文を読み終えた後に止める（`get_resolved` の途中を打ち切る口がなければ、時間の上限に任せることをコメントに書く）。

### 5. 本文を文字にする

- 受け取る形式は `text/html`、そのほかの `text/*`、`application/json`（と `+json`）に限る。ほかは取得しない。`charset` が `utf-8` か指定なしのときは UTF-8 として読み（不正なバイトは置き換える）、ほかの `charset` は取得しない（理由を返す）。
- HTML は、自前の小さな変換で本文の文字にする（依存のクレートを使わない）。`script`・`style`・`noscript`・`template`・`svg`・`head` の中を除く。`p`・`div`・`br`・`tr`・`section`・`article` などの区切りで改行し、`h1`〜`h6` を `#` の数の見出しに、`li` を `- ` の行に、`pre` の中を改行と空白を保って残し、ほかの空白は詰める。`a` は文字だけを残す。文字の参照（`&amp;`・`&lt;`・`&gt;`・`&quot;`・`&#39;`・`&nbsp;`・数値の参照）を戻す。`<title>` があれば先頭に `# <title>` として置く。
- 結果は、先頭に `URL: <最後の URL>` と `Content-Type: <型>` の行、空の行、本文の順とし、本文は `read` と同じく先頭から 50 KiB で区切り、区切ったことを示す（印の英文は `read` のものに倣う）。
- 取得した内容は信頼しないデータとして扱う（道具の結果に入るだけで、確認やモードに効かない）。

### 6. ほかの道具との関係

- 巻き戻し（P4-03）の「戻せないもの」に `web_fetch` の呼び出しが入ることを、P4-03 の作りに合わせて確かめる（P4-03 が先に入っていなければ、この作業では何もしない）。

## 触るファイル

- `crates/benitoiteh/src/tools/`（`web_fetch.rs`（新）、HTML の変換（新。置き場所は任せる）、`mod.rs`、`definitions.rs`、`text.rs`）
- `crates/benitoiteh/prompts/tools/web_fetch.md`（新）
- `crates/benitoiteh/src/approval/`（`WebFetch` の種類、判定、拒否の文）
- `crates/benitoiteh/src/agent/`（このセッションで許したドメインの集合）
- `crates/benitoiteh/src/http/`（足りない口があれば。リダイレクトの応答の `Location` を読む口など）
- `crates/benitoiteh/src/config/`（`[web_fetch]` の読み方を確かめ、要れば直す）
- `crates/benitoiteh/src/session/`（`ApprovalFacts` の欄の記録）
- `crates/benitoiteh/tests/expected/`（`system-*.txt`、`tools-*.json` を作り直し、差分が `web_fetch` の分だけであること）、`tests/`（新しい `tests/web_fetch.rs` にしてよい）

## テスト

外のネットワークに出ない。名前の解決と接続は、`http` の差し替え（P0-03 の試験の仕組み）か、手元の TLS の模擬のサーバで確かめる。

- URL の検査: `http://`、`user@`、ASCII でないホスト、IP のアドレス、443 でないポートが拒否される。明示した `:443` は許す。
- アドレスの判定の純関数: 01-03 の各範囲と、IPv4 を写した IPv6（`::ffff:127.0.0.1` など）が拒否され、公開のアドレスが許される（安全の性質のテスト。07-01）。
- 名前の解決が私的なアドレスを返すと、一覧のドメインでも取得しない。判定したアドレスだけに接続する（解き直さない）。
- 確認:
  - 一覧のドメインは確認なしで取得する。
  - 一覧にないドメインは、`never` では確認せずに拒否し（`Approver` が呼ばれない）、台本の `Approver` の `on-request` では尋ねる。`ApprovedForSession` の後は同じドメインを尋ねず、別のドメインは尋ねる。`Approved` の後は次も尋ねる。
  - 一覧と照合するのは完全な一致で、下位のドメインは一覧に含まれない。
  - プロジェクトの設定の `[web_fetch]` で一覧を広げられない。
- リダイレクト: 5 回まで辿り、6 回目で止まる。`https` でない先、私的なアドレスの先、一覧にもこのセッションの集合にもないドメインの先で止まる。
- 本文: 2 MiB を超えると止まる。`Content-Encoding: gzip` の応答を取得しない。許さない形式（`image/png`）と、UTF-8 でない `charset` を取得しない。
- HTML の変換: 見出し、段落、箇条書き、`pre`、文字の参照、`script`・`style` の除去、`title` の扱いを表の形のテストで確かめる。50 KiB の区切りと印。
- 道具の定義の期待の出力が一致する。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、道具の説明と、`tools/text.rs`・`approval/text.rs` に足した英文の全文、HTML の変換の例（入力と出力）、加えた公開の関数と型のシグネチャ、実行ファイルの大きさの変化を書く。
