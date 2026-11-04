# 段 3: 通信の制限

> 2026-11-04 に、`docs/2026-11-02/` の `06-stage3.md` から本ディレクトリへ移した。本文は当時の記録のままとし、リンクだけを張り直した。

[04-oss-feature-review](../../2026-11-02/04-oss-feature-review.md)の「段 3: 通信の制限（BH-046、BH-040）」を、設計者の指示で進めた記録である。確かめは、[P5-42-cloud-linux-check](P5-42-cloud-linux-check.md)の環境（x86_64 の Linux、Alpine 3.22.7 の rootfs、root でない利用者）で行った。macOS での確かめは別途とする。

## 設計者の決定

2026-11-02 に、設計者が次のとおり決めた。

| 問い | 決定 |
|---|---|
| 範囲 | 通常モード（BH-046）と制限モード（BH-040）を同時に設計する |
| 許すドメインの決め方 | 設定の一覧に加えて、対話でも確認する（このコマンド・このセッション・断る） |
| 私的なアドレス | 許したドメインでも、私的なアドレスに解決されれば拒む |
| 通信の形 | `CONNECT` に加えて、平文の HTTP も中継する |

判断の前に、Benitoite `v0.0.12` の HTTP のクライアントが、プロキシを使わないように固定していることを確かめた（`crates/benitoite/src/builtins/funcs/http_client.rs:63` の `.proxy(None)`）。このため、制限モードは設計と処理系への要望までとし、実装は通常モードだけとした。

## 作ったもの

### 設定

`[normal] network` に、今までの `false`・`true` に加えて `"proxy"` を受け付ける。`[normal] allowed_domains` に、確認なしに通すドメインを書く（`web_fetch` と同じ規則で正規化する）。中継の方式は `workspace-write` でだけ効く。

### 中継の核（`crates/benitoiteh/src/sandbox/proxy.rs`、`proxy/http.rs`）

- `NetworkGate`: セッションで一つ。設定のドメイン、このセッションで許したドメイン、画面への確認の口を持つ。尋ねてよいのは、対話の起動で、承認の方針が `never` でないときだけ。
- `ProxyRun`: `bash` の呼び出しごと。このコマンドで許したドメインと、断った記録を持つ。
- 判断の順は、設定の一覧、このセッション、このコマンド、利用者への確認。同時の接続でも確認は一度だけ。
- 名前は一度だけ解決し、一つでも私的・特殊用途のアドレスがあればつながない（`web_fetch` の判定を使う）。IP アドレスの宛先、絶対形の `https://`、利用者の情報を含む URL も断る。
- `CONNECT` は中身を覗かずに渡す。平文の HTTP は、要求の行を通常の形にし、`Host` を確かめた宛先に合わせ、プロキシ向けのヘッダと接続を保つ指定を除き、`Connection: close` を付ける。
- 断った要求には、理由を本文にした 403 などを返す。macOS のための合言葉（`Proxy-Authorization` の Basic）も扱う。

依存のクレートは加えていない（標準のライブラリのソケットと、既にある `rustix` だけを使う）。

### Linux のつなぎ込み

- `PolicyConfig.network_proxy` を加えた。ネットワークの名前空間は今までどおり分け、seccomp はこの方式でだけ IP のソケットを許す（名前空間の中にはループバックしかない）。`allow_ip` とは並べない。
- 補助のプロセスは、目的のプログラムより先に、名前空間の中の `127.0.0.1:3128` で待ち受け、その記述子を制御の通り道で `SCM_RIGHTS` により親へ渡す（nono の方式）。
- 親は `PreparedSandbox::with_proxy` で、起動の後に記述子を受け取り、子を待ち終えるまで中継で受け付ける。

### `bash` と画面

- サンドボックスの中の `bash` の呼び出しごとに `ProxyRun` を作り、`HTTP_PROXY`・`HTTPS_PROXY`・`ALL_PROXY`（小文字も）を設定し、`NO_PROXY` を除く。
- 中継が断った宛先と理由を、道具の結果に添える。
- 画面の確認の口を門につなぎ、コマンドの実行の途中で確認を出せるようにした。新しい確認の種類 `Network` の答えは `[y]` このコマンドだけ、`[s]` このセッションの間、`[n]` 断る。

### macOS（確かめていない）

主のプロセスが呼び出しごとにループバックの空いたポートで待ち受け、方針の `allow_loopback_port` でそのポートへの接続だけを許す。待ち受けは手元のほかのプロセスからも届くので、セッションごとの乱数の合言葉を求める。`aarch64-apple-darwin` のコンパイルと lint は通るが、実機では動かしていない（BH-119）。

### 制限モード（設計だけ）

`Http.Connect` を確認したスクリプトの実行でも同じ中継を使う方針を、設計書 03-02 に書いた。処理系がプロキシに従うようになるまで今のままとし、処理系への要望の 7 として 04-01 に加えた（まだ依頼していない）。`Http.Listen` は中継で扱えないので今のまま。BH-040 は未決のまま、この方針を書き足した。

## テスト

| テスト | 確かめること |
|---|---|
| `src/sandbox/proxy/http.rs` の 2 件（追加） | 要求の行の二つの形と断る形、平文の HTTP の書き直し、合言葉の照合（RFC 4648 の例を含む） |
| `src/sandbox/proxy/tests.rs` の 4 件（追加） | 本物のソケットと手元の模擬の宛先で、`CONNECT` と平文の HTTP の中継、確認の三つの答えの範囲と記録、私的なアドレス・IP アドレス・`https://`・利用者の情報の拒否、合言葉、尋ねてはならない門 |
| `tests/sandbox_linux.rs` の `proxied_network_reaches_allowed_domains_only_through_the_relay`（追加） | 本物のサンドボックスで、許した名前は中継を通って外の手元の宛先へ届き、ほかの名前は 403 と記録になり、中からの直接の接続は失敗する |
| `tests/tools_bash.rs` の `proxied_bash_reaches_allowed_and_approved_domains_and_reports_denials`（追加） | 本物のサンドボックスの `bash` で、プロキシの変数、許したドメイン、尋ねて許したドメイン、尋ねて断ったドメインと、その記録が結果に載ること |
| `tests/sandbox_exec_restricted.rs` の `exec_proxy_setting_relays_bash_and_refuses_unlisted_domains_without_asking`（追加） | `exec` の入口で、設定が `bash` を中継の方式にし、非対話では尋ねずに断って理由を載せること |
| `tests/config.rs` の `normal_network_accepts_flags_and_proxy_and_normalizes_allowed_domains`（追加） | 設定の三つの形と、ドメインの正規化と拒否 |
| `tests/normal_setup.rs`、`tests/tui/interaction.rs`（行を足した） | 方針への写し方（`read-only` では効かない）、確認の画面のキー |

道具の結果に断った記録を添えない版では、`bash` のテストが失敗した。

## 限り

- プロキシの変数に従わないプログラム、名前の解決を自分で行うプログラム、UDP は、外へつながらずに失敗する。
- 許したドメインへの持ち出しは防げない（srt の README が挙げる限りと同じ）。
- 断ったことは、道具の結果に添えるが、サンドボックスの OS の拒否（出力からの推定）は今のままである。macOS の方針の拒否の記録から理由を返すこと（04 の段 3 の 3）は、まだ行っていない。
- 設計書 04-01 の、03-04 の見出しへの参照の切れ（「段 3」）も直した。
