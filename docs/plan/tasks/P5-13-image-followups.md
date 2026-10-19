# P5-13 画像の送り直しの条件と再開の読み直しの直し、揺れるテストの直し

## 目的

P5-12 のセキュリティのレビューの所見（low）三つと、P5-12 の全体の検査で一度落ちた揺れるテストを直す。設計の変更はない。この作業で本物の LLM の提供者を呼ばない。

1. **画像を外して送り直す条件**: 今は `NonRetryable { reason: InvalidRequest }` なら送り直すので、ストリームの途中の誤り（HTTP の状態の番号がない誤り。Chat の途中の `{"error": ...}` を含む）でも、課金された要求をもう一度送り、画像の拒否でないのに画像を外したことを確定しうる。設計書 02-01「送る画像の上限」のとおり、提供者が HTTP の状態 400 か 413 で要求を断った（応答の本文のストリームを受け取る前の）ときに限る。
2. **再生の計算量**: `provider::images::remove_images` は、ID の一覧を線形に探す（76 行の `ids.contains`）。書き換えた記録に大きな一覧を置くと、再開が二乗に重くなる。
3. **再開の読み直しの量**: `Session::restore_images` は、画像ごとに中身を読み、ハッシュを計算し、写しを作る。同じハッシュを何度も参照する記録や、画像の多い会話で、送らない画像まで読む。
4. **揺れるテスト**: `src/benitoite/tests.rs` の `queries_bound_output_timeout_and_propagate_cancellation` の `overflow` の場合は、16 MiB を超える出力を既定の時間の上限の中で書き切ることを前提にしており、機械に負荷が掛かると `TimedOut` になって落ちる。

## 読む設計書

- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「送る画像の上限」
- [セッション](../../design/01-agent/01-05-sessions.md)の画像を `objects/` に置く段落
- 既存のもの: `crates/benitoiteh/src/provider/images.rs`（`remove_images`、`ImagePlan`、`prepare`）、`src/provider/types.rs`（`ProviderError`、`NonRetryableKind`）、三つのアダプタの誤りの分類（`provider/anthropic.rs` の `api_error`・`classify_http`、`provider/openai_chat.rs`、`provider/openai_responses.rs`、各 `assembly.rs` の途中の誤り）、`src/agent/retry.rs`、`src/agent/compaction/mod.rs`（430〜470 行あたり）、`src/agent/mod.rs`（`request_and_receive`）、`src/session/objects.rs`（`restore_images`）、`src/session/replay.rs`、`src/benitoite/tests.rs`

## この作業が認める型の変更

- 「HTTP の状態 400・413 で、ストリームの前に断られた」ことを、ループが見分けられるようにする変更。例: `NonRetryableKind` に新しい値（例: `RequestRejected`）を加え、`classify_http` の 400・413 をそれにする。利用者に示す誤りの文と、`exec` の出力や記録に出る誤りの種類の文字列（`invalid_request` など）は、今と同じにする（新しい値も `InvalidRequest` と同じ文と文字列にする）。
- `restore_images` の中の作りの変更。

## 作るもの

### 1. 送り直しの条件

- 画像を外して送り直すのは、画像を送った要求が、HTTP の状態 400 か 413 で、応答のストリームを受け取る前に断られたときだけとする（一つの番の要求と、続ける形の圧縮の要求の両方）。ストリームの途中の誤り、HTTP の状態の番号がない誤り、`benitoiteh` の中の誤り（要求の組み立て、取り消し、アダプタの食い違い）では送り直さない。
- 400 のうち、今 `ContextOverflow`・`Authentication`・`Quota` に分類しているものは、今のまま（送り直さない）。
- それ以外の誤りの扱い（止める、やり直す、圧縮する）は変えない。

### 2. 再生の計算量

- `remove_images` は、ID の一覧を一度 `HashSet<&str>`（か `BTreeSet`）にしてから引く。
- `replay_records` は、空の一覧の `ImagesRemoved` を読み飛ばす（会話を辿らない）。

### 3. 再開の読み直し

- 同じハッシュは一度だけ読み、確かめ、`Arc<[u8]>` を共有する（`restore_images` の中の `HashMap<String, Arc<[u8]>>` など）。ハッシュが同じで記録の MIME の型か寸法が違う画像は、今と同じに除く。
- 会話の新しい方から読み、読んだ画像（重複を一枚と数える）が 20 枚か合計 12 MiB（`provider::images` の上限の定数）に達したら、それより古い画像は読まない。読まない画像は `images` に残したまま `data` を `None` とし、断りの文を足さない（アダプタが今の「中身がない」の断りの文を付ける）。理由: 送る画像の上限の規則で送るのは、多くとも新しい方の 20 枚・12 MiB に収まる。
- `image_input` が偽なら読まない、は今のまま。

### 4. 揺れるテスト

- `overflow` の場合の `QueryLimits` の時間の上限を、出力を書き切るのに十分な長さ（例: 60 秒）に明示し、負荷で `TimedOut` にならないようにする。確かめる中身（16 MiB を超えたら `OutputLimit`）は変えない。

## 触るファイル

- `crates/benitoiteh/src/provider/types.rs`、`src/provider/text.rs`（要れば）、`src/provider/anthropic.rs`、`src/provider/openai_chat.rs`、`src/provider/openai_responses.rs`、`src/provider/images.rs`
- `crates/benitoiteh/src/agent/retry.rs`、`src/agent/mod.rs`、`src/agent/compaction/mod.rs`（要れば `src/agent/events.rs`、`src/ui/` の誤りの種類の対応）
- `crates/benitoiteh/src/session/objects.rs`、`src/session/replay.rs`
- `crates/benitoiteh/src/benitoite/tests.rs`
- テスト: `tests/agent_loop/images.rs`、`tests/provider_*.rs`（分類のテストがある所）、`src/agent/resume_tests.rs`、`src/provider/images.rs` の単体のテスト

## テスト

- 送り直し: 模擬の提供者が、画像を含む要求に HTTP 400 と 413 を返すと送り直すこと（既存のテストを保つ）。ストリームを 200 で始めてから途中で誤りの塊を返すと、送り直さずに今の誤りで終わり、記録に `images_removed` がないこと（Chat と Anthropic で）。誤りの文と種類の文字列が今と同じであること。
- 再生: 大きな一覧（例: 10 万の ID）の `ImagesRemoved` と空の一覧の `ImagesRemoved` を含む記録の再生が、外すべき結果だけを外すこと（時間の上限は掛けなくてよい）。
- 再開: 同じハッシュを参照する三つの結果が、一度の読み取りで同じ中身を共有すること（`Arc::ptr_eq`）。21 枚の画像の会話を再開すると、古い 1 枚の `data` が `None` で、断りの文が足されないこと。12 MiB を超える分の古い画像も同じであること（`read_object` と `restore_images` は記録の `bytes` と読んだ長さの一致を確かめるので、実際にその長さの中身で作る。例: 見出しの後を埋めた 3.5 MiB の GIF を 4 枚）。
- 揺れるテスト: 時間の上限を明示した形で通ること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、400・413 とストリームの途中の誤りを見分ける方法を書く。

## 補足（2026-10-19、始める前の点検）

本節は、上の各節に優先する。設計書 02-01「送る画像の上限」の送り直しの条件を、「HTTP の状態 400 か 413 で、応答のストリームを受け取る前に断られたとき（文脈の超過、認証、残高と利用の上限に分類したものを除く）」に改めた（目的の「設計の変更はない」は、この一文を除く）。

1. **再開の読み直しは、新しい方から 20 枚・12 MiB に限る形をとらない**（作るもの 3 の二つ目の項目を取り消す）。理由: `plan` は中身のない画像を数えないので、読まない画像があると、再開の前と後で外す画像と断りの文が変わり、設計書の「再開しても同じになる」に反する。会話の巻き戻しの後にも食い違う。同じハッシュを一度だけ読み、確かめ、`Arc<[u8]>` を共有することだけを行い、すべての画像を今までどおり読む。中身の長さ・MIME の型・寸法の照合は、参照ごとに、貯めた結果に対して行う。読めなかったハッシュの結果も貯め、同じハッシュを読み直さない。テストの「21 枚」「12 MiB を超える分」は行わない。共有のテストは、同じハッシュを参照する三つの結果で `Arc::ptr_eq` を確かめる。
2. **見分け方**: `HttpError::Status` は、応答のヘッダの前（2xx でないとき）にだけ作られ（`http/stream.rs` の 320〜323 行）、各アダプタの `classify_http` だけが `api_error(.., Some(status), ..)` に渡す（`anthropic.rs:323`、`openai_chat.rs:314`、`openai_responses.rs:337`）。ストリームの途中の誤りは `api_error(&data, None, None)` を通る（各 `assembly.rs`）。ヘッダの後の HTTP の誤りは `StreamDisconnected` になる。`NonRetryableKind` に新しい値（例: `RequestRejected`）を加え、各 `api_error` の最後の `else`（今 `InvalidRequest` を返す所）で、`matches!(status, Some(400 | 413))` のときだけ新しい値にする（`anthropic.rs:304-315`、`openai_chat.rs:306-310`、`openai_responses.rs:329-332`）。前の枝（Anthropic の 400 の "prompt is too long" → `ContextOverflow`、認証の型 → `Authentication`、Chat と Responses の `context_length_exceeded`、Responses の `is_quota` → `Quota`）は今のまま先に判定する。`benitoiteh` の中の誤り（各 `classification()` の `InvalidRequest|Cancelled`、`transport.rs:139-145`）は `InvalidRequest` のままとする。
3. **新しい値の扱い**: 網羅の照合は `provider/types.rs` の 302〜307 行の `Display` だけで、新しい値も `text::INVALID_REQUEST` を返す。`AgentError::kind()` は提供者の誤りをすべて `"provider"` とするので変わらない。`NonRetryableKind` の serde の形は、やり直せる誤りの `Retry` の記録にしか現れないので、新しい値の名前は既定（`request_rejected`）でよい（`invalid_request` に改名しない）。作るもの・「この作業が認める型の変更」の「`invalid_request` などの誤りの種類の文字列」は存在しないので、テストは誤りの種類 `"provider"` と文 `Invalid provider request` が今と同じであることを確かめる。
4. **送り直しの判定の場所**: `request_and_receive` は `src/agent/retry.rs` の 13〜51 行にある（判定は 26〜33 行）。続ける形の圧縮は `src/agent/compaction/mod.rs` の 433〜441 行。両方の照合の形を、新しい値だけに改める。
5. **直す既存のテスト**: `tests/provider_anthropic.rs` の 636〜643 行（HTTP 400 → 新しい値。途中の誤りの 698〜701 行は `InvalidRequest` のまま）、`tests/provider_openai_chat.rs` の 644〜665 行（400 の二つ → 新しい値）、`tests/provider_openai_responses.rs` の 767〜769 行と 795〜801 行あたり（`Some(400)` → 新しい値。`(None, "bad_request")` の 832 行と 873〜875 行は `InvalidRequest` のまま）。Responses の 400 に残高の符号がある場合（`Quota` のまま）のテストを一つ加える。
6. **再生**: `remove_images`（`provider/images.rs` の 66〜85 行）で一覧を一度 `HashSet<&str>` にする。`session/replay.rs` の 124〜126 行で空の一覧を読み飛ばす。大きな一覧のテストは、`provider/images.rs` の `mod tests` で `remove_images` に 10 万の ID を渡す形でよい（再生のテストは要らない）。
7. **揺れるテスト**: `QueryLimits::default()` は時間の上限 10 秒・猶予 10 秒である。`overflow` と `diagnostics` の場合に `timeout: Duration::from_secs(60)` を明示する。`ok` の場合（今は 100 ミリ秒の `limits`）も既定か長い上限にする。100 ミリ秒は `timeout` の場合だけに使う。
8. **途中の誤りのテスト**: Chat は `tests/agent_loop.rs` の 995〜1031 行（`refusal_and_stream_errors_do_not_execute_received_calls`）の形（`[DONE]` の代わりに `data: {"error":{"code":"invalid_request"}}`）、Anthropic は `event: message_start` の後に `event: error\ndata: {"type":"error","error":{"type":"invalid_request_error","message":"bad"}}` の形（型は `invalid_request_error` にする。`overloaded_error` などは状態の番号がないとやり直せる誤りになる）。`ResponseScript::sse(body)` は 200 で返す。
