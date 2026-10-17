# P4-04 道具 `web_fetch`

## 目的

URL の内容を取得して本文を返す道具 `web_fetch` を作る（設計書 01-03 の「web_fetch」、ADR 0347 を引き継ぐ）。両方のモードで使う。ドメインの確認は P3-04 の承認の仕組みに新しい種類として加え、対話の確認の画面は P4-07 で作る。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「web_fetch」（2026-10-17 に、ドメインの照合、ホストの名前の規則、取得しないアドレスの追加、確認の前に名前を解かないこと、確認の要るリダイレクトで止めること、並べて実行しないことを加えた）「モードごとの道具の一覧」「出力の切り詰め」「道具の定義の共通の規則」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「そのほかの確認」（BH-047 の決定: 「今回だけ」「このセッションの間、このドメインを許す」「拒否する」）「非対話での扱い」
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「保証しないこと」（ウェブの取得による持ち出し）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」の `[web_fetch] allowed_domains`、「プロジェクトの設定ファイル」
- [クレートの構成](../../design/06-impl/06-01-crate-structure.md)の依存の基準（HTML を文字に変えるクレートと URL のクレートは加えない）
- 既存のもの: `src/http/mod.rs`（`HttpClient::get_resolved`、`HttpTimeouts`、`Uri`、`HttpError::Status` の `head.headers`。プロキシは `.proxy(None)`、自動のリダイレクトは `max_redirects(0)`、`Accept-Encoding` は呼び出し側の値を除き、`get_resolved` は要求ごとに新しい `Agent` を作る。試験用の証明書 `src/http/fixtures`（名前は `localhost` と `mock.invalid`））、`src/approval/`（`ApprovalKind`、`ApprovalFacts`、`decide`、`remember_command`）、`src/agent/execution.rs`（確認の流れ、`allowed_commands`）、`src/tools/`（`ToolSet`、`ToolContext`、`Invocation`、`definitions.rs`、`text.rs`）、`src/config/`（`UserConfig.web_fetch`、`ProjectConfig` の `deny_unknown_fields`）、`src/cli/exec.rs`（`ToolContext` の組み立て）、`tests/expected/`

## この作業が認める型の変更

00-common の「止まるとき」に当たる次の変更は、この作業で行ってよい。10-interfaces はオーケストレータが更新する。

- `ApprovalKind` に `WebFetch` を、`ApprovalFacts` に `url: Option<String>` と `domain: Option<String>` を `#[serde(default)]` で加える。`ApprovalFacts` を構造体の字面で作る箇所（`tools/bash.rs`、`tools/write_path.rs`、`agent/events.rs`、`tests/agent_loop.rs`）は `..ApprovalFacts::default()` で直す。
- `decide` が、このセッションで許したドメインの集合を受け取れる形にする（例: `allowed` を `AllowedSets { commands, domains }` にする）。
- `Invocation` に、このセッションで許したドメインの集合への参照（`session_domains: &'a BTreeSet<String>`）を加える。
- `ToolContext` に、`web_fetch` の設定と HTTP のクライアントを持たせる口（例: `ToolContext::with_web_fetch(WebFetchContext { allowed_domains, client })`）を加える。`cli/exec.rs` が設定する。
- `HttpClient` に、TLS の設定を写して時間の上限だけを変えたクライアントを作る口（例: `HttpClient::with_timeouts(&self, HttpTimeouts) -> HttpClient`）を加える。

## 作るもの

### 1. 道具の定義

- `src/tools/web_fetch.rs`（と子のモジュール）に作り、両方のモードの `ToolSet` の末尾に加える（01-03 の「モードごとの道具の一覧」の順。後で `benitoite_*` の道具を加えるときは `web_fetch` の前に入り、`tests/expected/` が変わる）。`parallel` は偽とする。
- 引数は `{ "url": "string" }`。説明の英文を `prompts/tools/web_fetch.md` に置く（一行の説明と 0〜3 行の規則。取得した内容は信頼しないデータであること、環境の情報の `network` の欄は子プロセスのネットワークの可否であって `web_fetch` には当てはまらないことを含める。オーケストレータが取り込む前に文面を見直す）。

### 2. URL の検査と正規の形

URL の検査は一つの関数にまとめ、最初の URL とリダイレクトの先の両方に使う。

- `Uri` で解析する。方式が `https` でない（方式のない `https:ex.com` を含む）ものは取得しない。
- authority の字面を調べる。`@` を含むもの（利用者の情報）は取得しない。ポートは、`:` がなければ 443、`:` の後が `443` ならよく、空か、ほかの値か、数として読めないもの（`:99999`）は取得しない（`port_u16()` が `None` を返すことを 443 とみなさない）。
- ホスト: `[` で始まるもの（IPv6 のアドレス）は取得しない。ASCII でないホストは解析で誤りになるので、誤りの文に `xn--` の形（punycode）で書くよう添える。ASCII の小文字に揃え、末尾の `.` を除いてから、01-03 のホストの名前の規則（各ラベルが英小文字・数字・`-` の 1〜63 文字、全体 253 文字以内、空のラベルなし、最後のラベルが数字だけか `0x` で始まるものは IP のアドレスの別の書き方として拒む）を当てる。
- 正規の形: 正規化したドメインと元の `path_and_query` から `https://{domain}{path_and_query}` の `Uri` を作り直す。確認に示す URL、`Host` のヘッダ、TLS の名前、結果の `URL:` の行は、すべてこの値から作る。フラグメントは解析で捨てられ、送らない。パスとクエリの 0x80 以上のバイトは、解析の前に百分率で符号化する。
- 誤りの英文は `tools/text.rs` に置く。

### 3. 確認

- 引数と URL の検査の後、ドメインが利用者の設定の `allowed_domains` か、`Invocation.session_domains` にあれば、確認なしで取得する。なければ、名前を解かずに、何もせずに `ApprovalKind::WebFetch` の確認の要求（事実は正規の URL と `domain`、`summary` は `tools/text.rs` の関数で作る）を返す。理由: 確認の前に名前を解くと、DNS の問い合わせで内容を持ち出せる（01-03）。
- `decide` の `WebFetch`: このセッションで許したドメインの集合にあれば `Allow`、承認の方針が `never` なら `Deny`、ほかは `Ask`。理由: 非対話（`never`）では一覧のドメインだけを取得する（03-03 の「非対話での扱い」）。制限モードの `Agent` の既定の方針も `never` なので、対話の画面ができるまで、制限モードでも一覧の外は拒否になる。
- 答え: `Approved` は今回だけとする。この呼び出しの中では、元のドメインを許したものとして扱う（同じドメインの中のリダイレクトで止めない）。`ApprovedForSession` は集合に `facts.domain` を加える（`Command` の集合とは別。答えから覚える関数は再開の記録からは呼ばない）。`Denied`・`DeniedWithMessage` は P3-04 の形の拒否の文（`WebFetch` の英文を `approval/text.rs` に足す）。
- `allowed_domains` の各項目は、設定を読むときに 2 と同じホストの名前の規則で検査して正規化する。`*.x`、`https://...`、ポート付き、IP のアドレスの項目は、設定の誤りとして示す（黙って一致しない項目を残さない）。
- プロジェクトの設定の `[web_fetch]`: 今の `ProjectConfig` は `deny_unknown_fields` で、この欄があればファイル全体を誤りとして示して使わない。これは 05-01 の「プロジェクトの設定ファイル」と一致するので、コードは変えず、テストだけを足す。
- リダイレクトの先のドメインが、一覧にも、セッションの集合にも、この呼び出しで許した元のドメインにもないときは、辿らずに止め、リダイレクトの先の正規の URL を結果に示す（`is_error` は偽。英文の全文を `tools/text.rs` に置く。01-03）。

### 4. 名前の解決と接続

- 時間の上限は、取得を行う実行の開始から数えて全体で 30 秒とする（確認に答えるまでの時間は含めない）。リダイレクトの回ごとに、残りの時間を `HttpTimeouts` の全体・ヘッダ・データの上限に入れたクライアント（`with_timeouts`）を使う。
- 名前の解決は、別のスレッドで `ToSocketAddrs`（ポート 443）を行い、残りの時間で待つ。待つ間は 100 ミリ秒ごとに取り消しの印を見る。上限を過ぎたスレッドは切り離してよい。`get_resolved` の呼び出しも同じく別のスレッドで行い、取り消しの印を見て待つ（切り離したスレッドは時間の上限で終わる）。
- 得たアドレスのどれか一つでも 01-03 の取得しないアドレスに入れば、取得しない。判定は純関数にする。IPv4 を写した IPv6 と IPv4 と互換の IPv6 は、上位 96 ビットを明示的に調べて IPv4 を取り出す（`Ipv6Addr::to_ipv4()` は `::1` を `0.0.0.1` とするので使わない）。NAT64 は埋め込まれた IPv4 を取り出して判定する。
- 判定したアドレスだけを `get_resolved` に渡す（名前を解き直さない）。
- 要求のヘッダ: `User-Agent: benitoiteh/<版>`、`Accept: text/html, text/*;q=0.9, application/json;q=0.8`。クッキーと認証のヘッダを送らない。`Accept-Encoding` を送らない。
- リダイレクト（301・302・303・307・308）は 5 回まで辿る。`HttpError::Status` の `head.headers` の `Location` を読み、RFC 3986 の 5.2 節の手順で自前に解決する（受ける形は、絶対の `https://...`、`//host/...`、`/path`、相対のパス（今のパスのディレクトリと結合し、`.` と `..` を除く）、`?query` だけ）。`Location` がない、文字にできない、解析できないときは、取得を止めて誤りを返す。解決した URL に 2・3・4 の検査をやり直す。
- 300・304・305 とほかの 3xx、4xx、5xx は誤りとし、状態の値と、本文があれば変換した文の先頭 2 KiB を返す。
- 本文の上限は 2 MiB とし、超えたら誤り（何も返さない）とする。応答の `Content-Encoding` が、ないか `identity` だけでなければ（複数の値を含む）取得しない。

### 5. 本文を文字にする

- 受け取る形式は、`text/html` と `application/xhtml+xml`（HTML として扱う）、そのほかの `text/*`、`application/json` と `+json` に限る。`Content-Type` がないものと、ほかの型は取得しない。型と `charset` は大文字と小文字を区別せず、引用符付きの値を受ける。
- `charset`: 指定なし、`utf-8`・`utf8` は UTF-8 として読み（不正なバイトは置き換える）、`us-ascii` と `iso-8859-1`（`latin1`）はクレートなしで変換する。ほかの `charset` は取得しない（理由を返す）。UTF-8 の BOM は除き、UTF-16 の BOM があれば取得しない。
- HTML は、再帰を使わない状態機械による自前の変換で本文の文字にする（入力の長さに比例する時間で終える）。
  - 除く要素: `script`・`style`・`noscript`・`template`・`svg`・`head`・`iframe`・`object`・`embed`・`canvas` の中。閉じていない除く要素は文書の終わりまで除く。`</script >` のような空白付きの閉じのタグを認める。
  - コメント、`<!DOCTYPE>`、CDATA を除く。属性の引用符の中の `>` でタグを終えない。タグの名前の大文字と小文字は区別しない。
  - `p`・`div`・`br`・`tr`・`section`・`article`・`header`・`footer`・`blockquote` などの区切りで改行し、`td`・`th` の間に ` | ` を置く。`h1`〜`h6` を `#` の数の見出しに、`li` を `- ` の行（入れ子は 2 字ずつ字下げ）に、`pre` の中を改行と空白を保って残し、ほかの空白は詰める。
  - `a` は、`href` を今の URL で解決した結果が `https` の絶対の URL なら `[文字](URL)`、ほかは文字だけを残す（LLM が文書の中のリンクを辿れるように）。
  - 文字の参照（`&amp;`・`&lt;`・`&gt;`・`&quot;`・`&#39;`・`&apos;`・`&nbsp;`・十進と十六進の数値の参照）を戻す。不正な数値の参照（0、サロゲート、0x10FFFF を超える値）は U+FFFD にし、知らない名前の参照は字面のまま残す。
  - `<title>` があれば、先頭に `# <title>` として置く。
- どの形式でも、変換の後に、`\n` と `\t` を除く C0 の制御の文字、`\r`、DEL、C1（U+0080〜U+009F）を除く。理由: 数値の参照などから端末の制御の列が生じうる。
- 結果は、先頭に `URL: <最後の正規の URL>` と `Content-Type: <正規化した型>`（応答のヘッダの生の値は書かない）の行、空の行、本文の順とする。本文は先頭から 50 KiB で区切り（行の数の上限は設けない）、区切ったときは `[Showing the first 51200 of N bytes of the converted text.]` を末尾に付ける（英文は `tools/text.rs` に置く）。全文を `spill/` に保存することはしない。
- 取得した内容は信頼しないデータとして扱う（道具の結果に入るだけで、確認やモードに効かない）。

### 6. テストのための差し替え

- 取得の中心の関数に、名前の解決（`SocketAddr` をポートごと返す）、アドレスの判定（テストではループバックを許す）、`HttpClient`（試験用の証明書を信頼するもの）、時間の上限を差し替える引数を持たせる。道具の本番の経路は、本物の値を渡す。差し替えを使うテストは、道具のモジュールの中のテスト（`src/tools/web_fetch/tests.rs`）に置く（試験用の証明書と `with_tls` は `tests/` から使えないため）。`tests/` の統合テストは、確認の流れと誤りの経路に限る。
- ドメインをまたぐリダイレクトのテストは、試験用の証明書の名前（`localhost` と `mock.invalid`）で組む。

### 7. ほかの道具との関係

- 巻き戻し（P4-03）の「戻せないもの」に `web_fetch` の呼び出しが入ることを、P4-03 が先に入っていれば確かめる。入っていなければ、この作業では何もしない。

## 触るファイル

- `crates/benitoiteh/src/tools/`（`web_fetch.rs` と子のモジュール（新）、`mod.rs`、`definitions.rs`、`text.rs`、`bash.rs`・`write_path.rs` の `ApprovalFacts` の字面）
- `crates/benitoiteh/prompts/tools/web_fetch.md`（新）
- `crates/benitoiteh/src/approval/`（`WebFetch` の種類、判定、拒否の文）
- `crates/benitoiteh/src/agent/`（このセッションで許したドメインの集合、`Invocation` への受け渡し、`events.rs` の `ApprovalFacts` の字面）
- `crates/benitoiteh/src/http/`（`with_timeouts`）
- `crates/benitoiteh/src/config/`（`allowed_domains` の検査と正規化）
- `crates/benitoiteh/src/cli/exec.rs`（`ToolContext` への受け渡し）
- `crates/benitoiteh/src/session/`（`ApprovalFacts` の欄の記録）
- `crates/benitoiteh/tests/expected/`（`system-*.txt`、`tools-*.json` を作り直し、差分が `web_fetch` の分だけであること）、`tests/agent_loop.rs`、新しい `tests/web_fetch.rs`

## テスト

外のネットワークに出ない。

- URL の検査の表: `http://`、方式のない `https:ex.com`、`user@`、`a@b@c`、ASCII でないホスト、`[::1]`・`[fe80::1%25en0]`、`:8443`・`:99999`・空のポート、数字のホスト（`2130706433`、`0x7f.1`、`127.1`、`017700000001`、`0x7f000001`）、空のラベル、64 文字のラベルが拒否される。明示した `:443`、大文字と末尾の `.` のホストは、正規化して受け付ける。
- アドレスの判定の純関数の表（安全の性質のテスト。07-01）: 01-03 の各範囲、`::ffff:127.0.0.1`、`::127.0.0.1`、`::1`、NAT64 に埋め込んだ私的なアドレスと公開のアドレス、6to4、Teredo、`2001:db8::1`、`2000::/3` の外が判定どおりになり、公開のアドレスが許される。
- 名前の解決（差し替え）: 確認を待つときと拒否されたときに呼ばれない（安全の性質のテスト）。私的なアドレスを一つでも含む答えなら、一覧のドメインでも取得しない。一回の取得で一回だけ呼ばれる（解き直さない）。本物の名前の解決で `localhost` を一覧に入れても、`127.0.0.1` になり拒まれる。
- 確認:
  - 一覧のドメインは確認なしで取得する。
  - 一覧にないドメインは、`never` では確認せずに拒否し（`Approver` が呼ばれない）、台本の `Approver` の `on-request` では尋ねる。`ApprovedForSession` の後は同じドメインを尋ねず、別のドメインは尋ねる。`Approved` の後は次も尋ね、その呼び出しの中の同じドメインへのリダイレクトは辿る。
  - 一覧との照合は完全な一致で、下位のドメインは含まれない。
  - プロジェクトの設定の `[web_fetch]` は、設定の誤りとして示され、一覧が広がらない。
  - `allowed_domains` の誤った項目（`*.x`、`https://x`、`x:443`、`1.2.3.4`）が設定の誤りになる。
- リダイレクト（差し替えと試験用の証明書）: 5 回まで辿り、6 回目で止まる。相対の `Location` の各形が正しく解決される。`https` でない先、私的なアドレスの先で止まる。一覧にもセッションの集合にもないドメインの先では、辿らずに先の URL を示して止まる。
- 時間と取り消し: 少しずつ送ってくる応答が、短くした全体の上限で止まる。取り消しの印を立てると、時間の上限を待たずに止まる。
- 本文: 2 MiB を超えると誤りになる。`Content-Encoding: gzip` と `identity, gzip` を取得しない。`image/png`、`Content-Type` のない応答、UTF-8 でも `us-ascii`・`iso-8859-1` でもない `charset`、UTF-16 の BOM を取得しない。`iso-8859-1` が正しく変換される。4xx が状態の値と本文の先頭で返る。
- HTML の変換の表: 見出し、段落、箇条書き（入れ子）、表、`pre`、リンク（https の絶対の URL と、ほかの形）、文字の参照（不正な数値と知らない名前を含む）、`script`・`style`・閉じていない `script`・`</script >`、コメント、引用符の中の `>`、`title`。深い入れ子（10 万段）でもスタックを溢れさせない。
- 制御の文字の除去: `&#x1b;[2J`、`&#x9b;`、生の ESC を含む HTML・text/plain・JSON から、制御の文字が除かれる（安全の性質のテスト）。
- 50 KiB の区切りと印。
- 道具の定義の期待の出力が一致する。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、道具の説明と、`tools/text.rs`・`approval/text.rs` に足した英文の全文、HTML の変換の例（入力と出力）、加えた公開の関数と型のシグネチャ、実行ファイルの大きさの変化を書く。
