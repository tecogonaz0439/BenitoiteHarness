# P5-26 秘密の入力の段 2 の残り

## 目的

2026-10-20 の点検で見つかった、秘密の入力の仲介（P5-20）の残りを直す。伏せ字の照合のために出力を留めすぎること、照合の値が呼び出しの後もメモリーに残ること、macOS の補助のディレクトリの名前からセッションを一つに決められないこと、掃除の失敗の文が二重になることの四つである。この作業で本物の LLM の提供者を呼ばない。

## 読む設計書

- [秘密の入力](../../design/03-safety/03-04-secrets.md)の「段 2: `benitoiteh` が秘密の入力を仲介する」（「補助」「秘密の扱い」）
- [セッション](../../design/01-agent/01-05-sessions.md)の「一時ディレクトリ」
- [P5-20](P5-20-askpass.md)の補足
- 既存のもの: `crates/benitoiteh/src/askpass/mod.rs`、`src/askpass/output.rs`、`src/askpass/tests.rs`、`src/tools/process_output.rs`、`src/tools/bash.rs`、`src/session/mod.rs`（`cleanup` の askpass の段）、`src/session/text.rs`、`tests/session.rs`

## 作るもの

### 1. 留める出力を区切りの手前までにする

設計書 03-04「秘密の扱い」の【方針】のとおり、ストリームごとに留める出力を、最も長い秘密の長さより 1 バイト短い分までとし、さらに、留めた中の最後の `\n`・`\r`・NUL までは留めずに渡す。照合の値は、これらのバイトを含む秘密には作らない。

### 2. 照合の値を消す

設計書 03-04「秘密の扱い」の【方針】のとおり、照合の値（長さ、SHA-256、rolling hash）を `zeroize` で上書きしてから手放し、呼び出しが終わったときに呼び出しの状態を手放す（次の呼び出しまで残さない）。`sha2` の機能 `zeroize` を有効にする。

### 3. 補助のディレクトリの `id`

設計書 03-04「補助」の【方針】のとおり、補助のディレクトリ（セッションごとの子）に、セッションの ID の全体を書いた `id` を置き、作り直しと後始末でそれを確かめる。作り直すときに `id` が自分のものでなければ、仲介を止める。`id` のない古い子は、後始末だけ今の対応表で扱い、作るときはほかのセッションのものとして扱う（消さずに仲介を止める）。

### 4. 掃除の失敗の文

掃除の失敗の文が「Could not clean up temporary directory unknown: Skipped cleanup of <パス>: …」のように二重になる形を直す。あわせて、`CleanupFailure` のコメントを実際の文に合わせる。

## テスト

- (a) 区切り（項目 1）: 標準出力と標準エラー出力に交互に一行ずつ書いた出力の、混ぜた並びの順が書いた順と同じであること。秘密より短い行も、改行まで届けばすぐに渡されること。`\r` と NUL も区切りになること。秘密が二回の読み取りにまたがっても伏せられること。
- (b) 読み取りの境目（項目 1）: 改行を含む出力を、読み取りの大きさ 1・4095・8192 で渡しても、伏せ字が同じになること。
- (c) 手放す（項目 2）: 呼び出しの守り（`CallGuard`）と伏せ字の処理（`Redactor`）を手放すと、呼び出しの状態も手放されること。二つの呼び出しを続けて始め、先の守りを後で手放しても、後の呼び出しの求めが画面に届くこと。
- (d) `id`（項目 3）: `id` が違うか、ないときに、作り直しが失敗して元の子が残ること。自分の `id` なら作り直せること。`id` がリンク・0644・FIFO・長すぎるものは、読まずに断ること。macOS で、乱数の部分が同じ二つの ID の子のうち、錠を持たないものだけが後始末で消えること。
- (e) 文（項目 4）: ID の分からない掃除の失敗の文に `unknown` が出ず、パスが一度だけ出ること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- `sha2` の機能 `zeroize` を有効にした前と後の、release の実行ファイルの大きさを測り、完了の報告に書く。`docs/plan/10-interfaces.md` の「依存のクレート」の `sha2` の行に、機能 `zeroize` と、その理由（設計書 03-04「秘密の扱い」）と、測った大きさの変化を書く。

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。行の番号は `main`（`564a32c`）の `crates/benitoiteh/` からの相対である。

1. **区切り（項目 1）**:
   - 今の箇所: `src/askpass/output.rs` の `Redactor::push`（62-）。留める量は 119-124 の `lookbehind`（最も長い秘密の長さ − 1）で、渡す量は 125-129 の `count`。出力の組み立ては `src/tools/process_output.rs` の `StreamOutput`（217-261）と混ぜた並び（128-146）、`src/tools/bash.rs` の 139-146 でストリームごとに `Redactor` を分け、163-175 で標準出力、標準エラー出力の順に最後を流す。読み手は `src/sandbox/run.rs` の 486-490。
   - 直し: `finish` でないとき、`count` を「`pending.len() − lookbehind`」と「`pending` の中の最後の `\n`・`\r`・NUL の位置 + 1」の大きい方にする。`SecretHash::new` は、秘密がこれらのバイトを含むとき `None` を返す形（`Option<SecretHash>`）にし、`None` なら照合の値を登録しない。入力の側は既にこれらのバイトを断っている（`src/askpass/mod.rs` の 643-648、`src/ui/tui/confirmation.rs` の 1035-1036 と 1075）ので、ふだんは `None` にならない。
   - 安全の論証: 秘密はこれらのバイトを含まないので、秘密と一致する並びはこれらのバイトをまたがない。最後の区切りまでのバイトは、そこで終わる一致もそこをまたぐ一致もないので、留めずに渡しても見逃さない。一致の印（`covered`）は区切りの後ろから付け直されるだけになる。
   - テストは `src/askpass/tests.rs` の `interleaved_stream_redaction_preserves_each_stream_secret`（603-655）の隣に置く。順の試しは `a\n`・`b\n`・`c\n` を標準出力、標準エラー出力、標準出力の順に一行ずつ渡し、混ぜた並びが `a\nb\nc\n` であることを確かめる。秘密より短い行は、長さ 32 の秘密に対する 21 バイトの行で確かめる。`redaction_covers_read_boundaries_before_the_output_tail_is_limited`（450-500）に、改行を含む入力を読み取りの大きさ 1・4095・8192 で渡す場合を加える。本物の `bash` で順を確かめるテストは置かない（書き込みの順は、子のプロセスの書き方と読み手のスレッドの順で揺れる）。
2. **照合の値を消す（項目 2）**:
   - 今の箇所: `SecretHash`（`src/askpass/output.rs` の 7-13。`Clone` と `Copy`）、`State.hashes: Vec<SecretHash>`（`src/askpass/mod.rs` の 79-85）、`push` が錠の中で `hashes` を毎回写す箇所（`output.rs` の 64-70）、`CallGuard` の `Drop`（`mod.rs` の 152-156。`active` を偽にするだけ）、`Broker.current` を次の `begin`（431-434）か `shutdown`（361-371）まで持ち続ける箇所。
   - 直し: `SecretHash` に `Default` を付け、`zeroize::DefaultIsZeroes` を実装する（`Copy` と `Default` が要る。全部の欄が 0 の値が既定になる）。`State.hashes` は `Zeroizing<Vec<SecretHash>>` とし、容量 3（一つの呼び出しで出せる求めの上限）で作って伸ばさない（伸ばすと古い領域に写しが残る）。`State` の `Default` は手で書く。`Redactor` は、自分の `Zeroizing<Vec<SecretHash>>` を持ち、`push` のたびに `State.hashes` から新しく増えた分だけを写す。`CallGuard` の `Drop` は、`stop()` の後に `Broker.current` の錠を取り、中身が自分の `call` と同じもの（`Arc::ptr_eq`）なら取り出して手放す。錠の順は、`shutdown` と同じく `current` → `state` とする（`stop()` は `state` だけを取るので、先に済ませて放す）。`CallGuard` は、そのために `current` への `Arc` を持つ。`State.hashes` を呼び出しの途中で空にはしない（空なら伏せ字を掛けずに渡す、という 71-74 の扱いがあるため）。
   - `Cargo.toml` の 37 行の `sha2` に `features = ["zeroize"]` を加える。`sha2` 0.11.0 の機能 `zeroize` は `digest/zeroize` を有効にし、`src/block_api.rs` の `Sha256VarCore` の `Drop` が状態を上書きする（手元の `~/.cargo/registry` の `sha2-0.11.0` で確かめた）。新しいクレートは加わらない（`zeroize` 1.9.1 は既に依存にある）。
   - テスト: `Arc::downgrade` で呼び出しの状態の弱い参照を取り、守りと伏せ字の処理を手放した後に `upgrade()` が `None` であることを確かめる。二つの `begin` を続け、先の守りを手放した後に、後の呼び出しの求めが画面の受け口（`take_requests`）に届くことを確かめる。上書きそのもの（手放した領域が 0 であること）は、安全な Rust では読めないので確かめない。
3. **`id`（項目 3）**:
   - 今の箇所: 子の名前は `helper_name`（`src/askpass/mod.rs` の 451-458。macOS では ID の乱数の部分 16 桁）。作り直しは `Broker::create` の 260-280（自分の 0700 の子なら消して作り直す）。掃除は `src/session/mod.rs` の 347-364（名前から ID への対応表。重なる名前は `None` にして残す）、419-423、442-463（askpass の段）。
   - 作る: 子を 0700 で作った直後、リンクとソケットを置く前に、`openat(child, "id", WRONLY | CREAT | EXCL | NOFOLLOW | CLOEXEC, 0o600)` で作り、ID の全体と `\n` を書く。どの OS でも置く。
   - 読む: `openat(child, "id", RDONLY | NOFOLLOW | NONBLOCK | CLOEXEC)` で開き（`NONBLOCK` は FIFO で止まらないため）、`fstat` で、通常のファイル、持ち主が自分、0600、リンクの数が 1、64 バイト以下を確かめてから読む。中身が `<ID>\n` の形で、ID として読めることを確かめる。
   - 作り直し: 子が既にあり、自分の持ち物の 0700 なら、`id` を読み、自分のセッションの ID と同じときだけ、今どおり消して作り直す。違うとき、ないとき、確かめに通らないときは、子を消さずに誤りを返す。`cli/exec.rs` の 567-575 が、その誤りで仲介を止めて `DISABLED` の文を示す（新しい誤りの種類を足すなら、示す文は `DISABLED` のままでよい）。
   - 掃除: askpass の段の項目は、子の `id` を読めればその ID で錠と記録を確かめる（名前の対応表を使わない）。`id` がない子は、今の対応表で扱う（前の版が作った子のため）。`id` があるのに確かめに通らない子は残す。
   - ID の全体を子の名前にしない理由（材料の計算）: macOS の一時の根の実のパスが 56 バイトのとき、`bha-<uid>/`（8）、区切り（1）、ID の全体（33）、`/sock`（5）で 103 バイトになり、Unix のソケットの名前の上限に届く。
   - 範囲の外: 記録を別のデータの置き場所に写して、同じ ID のセッションが二つの置き場所にある場合は扱わない。
   - テスト: (a) は `src/askpass/tests.rs` の 210-263 の作り直しの試しに倣う（Linux は `rerun_without_runtime`（80）で `$XDG_RUNTIME_DIR` を外して一時の根の下に作る）。(b) は `tests/session.rs` に、macOS だけで、乱数の部分が同じ二つの ID（日時の部分だけが違うもの）の子を `id` 付きで作り、片方の錠を持ったまま掃除して、錠のない方だけが消えることを確かめる。(c) の偽の `id` は、リンク、0644、FIFO、65 バイトの四つとする。持ち主の違う `id` は root でないと作れないので、root のときだけ確かめる（Linux のコンテナは root で走る）。0000 の試しは、root のときに飛ばす。短い根（`/tmp/bh-<16 桁>`）を使う試しは、macOS の `/tmp` が `/private/tmp` へのリンクなので、正規化した根で比べる。
   - 今のテスト: `tests/session.rs` の 685-738 と `shared_temporary_cleanup_preserves_foreign_live_helpers_and_unrecorded_helpers`（1994-）は、`id` のない子を作っている。古い子の扱い（対応表で消す）の試しとして残し、名前を変えずに、そのことをコメントに書く。
4. **掃除の失敗の文（項目 4）**:
   - 今の箇所: `CleanupFailure` のコメント（`src/session/mod.rs` の 307「誤りの文にファイルのパスは含めない」と 314）は、`text::cleanup_skipped`（`src/session/text.rs` の 64-68）がパスを含める実際（`mod.rs` の 341-345、380-385、391-395）と合わない。`text::cleanup_failed`（`text.rs` の 20-31）は、ID がないときに `unknown` と書き、続けて `Skipped cleanup of <パス>: …` を並べる。
   - 直し: コメントを「一時の根と askpass の親の失敗は、パスを含む文にする」の形に改める。`cleanup_failed` は、ID がないときは `reason` をそのまま返す。一覧の読み取りの誤り（`mod.rs` の 401-409）は、`reason` を `cleanup_skipped(<一覧を読んだ根>, …)` で包む（ID がなくてもどの根の誤りかが分かるように）。
   - 今のテスト: `tests/session.rs` の 2068-2072 と 2100-2101 は今の文を確かめている。新しい形に改め、`unknown` を含まないことと、パスが一度だけ出ることを確かめる。
