# P5-08 S の段の決定を実装に反映する

## 目的

2026-10-19 に設計者が決めた、P5 の S の段の評価の結果による二つの変更を実装に反映する。書いた後の検査（BH-004）は既定で行う今の実装のままなので、変えない。

1. リリースのビルドの `opt-level` を `"s"` にする（BH-060。設計書 06-01「ビルドの設定」）。
2. `anthropic-messages` の effort の既定を `low` にする（BH-034。設計書 02-01 の Anthropic の Messages API の節）。`openai-responses` の既定は `high` のまま、`openai-chat` は今までどおり effort を要求に付けない。

この作業で本物の LLM の提供者を呼ばない。

## 読む設計書

- [クレートの構成](../../design/06-impl/06-01-crate-structure.md)の「ビルドの設定」
- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の Anthropic の Messages API と OpenAI の Responses API の節（effort の扱い）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「オプション」（`--effort`）と「設定ファイル」
- 既存のもの: ワークスペースの `Cargo.toml` の `[profile.release]`、`crates/benitoiteh/src/config/mod.rs`（`ProviderConfig.effort`、`default_effort`、`resolve_exec` の effort の解決、空の値の検査）、`src/provider/anthropic/request.rs`（`options.effort.unwrap_or("high")`）、`src/provider/openai_responses/request.rs`（`unwrap_or("medium")`）、`tests/config.rs`、`tests/provider_anthropic.rs`、`tests/provider_openai_responses.rs`

## この作業が認める型の変更

- `ProviderConfig.effort` を `Option<String>`（設定ファイルで省けば `None`）にしてよい。`ResolvedExec.effort` は `String` のままとし、`--effort`、提供者の設定の `effort`、アダプタの種類ごとの既定、の順で決める。
- アダプタの種類ごとの既定を返す関数を加えてよい（`anthropic-messages` は `low`、`openai-responses` は `high`、`openai-chat` は `high`（要求には付けないが、記録と表示のために値を持つ））。

## 作るもの

### 1. `opt-level`

- ワークスペースの `Cargo.toml` の `[profile.release]` の `opt-level = 3` を `opt-level = "s"` にする。ほかの欄（`lto`、`codegen-units`、`panic`、`strip`）は変えない。
- 完了の報告に、変えた後の `scripts/check.sh` の `size` の段の三つの対象の大きさを書く。

### 2. effort の既定

- 設定ファイルで `effort` を省いた提供者は、アダプタの種類ごとの既定を使う。`--effort` と設定の `effort` は今までどおり優先する。空の文字列を誤りとする今の検査は残す。
- `src/provider/anthropic/request.rs` の、値がないときの代わりの値を `"low"` にする（`ResolvedExec` から常に値が渡るので通らない道だが、既定と揃える）。コメントの「省略したときは high」も直す。
- `openai-responses` と `openai-chat` の振る舞いは変えない。
- `doctor`・`/status`・セッションの記録のヘッダに出る effort は、解決した後の値のままとする（変えない）。

## 触るファイル

- `Cargo.toml`（ワークスペースの根）
- `crates/benitoiteh/src/config/mod.rs`
- `crates/benitoiteh/src/provider/anthropic/request.rs`
- `crates/benitoiteh/tests/` の該当のテスト（`config.rs`、`provider_anthropic.rs` など）

## テスト

- 設定: `effort` を省いた `anthropic-messages` の提供者が `low` に、`openai-responses` の提供者が `high` に解決されること。設定の `effort` と `--effort` が既定より優先すること。空の値が今までどおり誤りになること。
- `anthropic-messages` の要求の `output_config.effort` が、既定のときに `low` であること（模擬の提供者で要求の本文を確かめる）。
- 既存のテストで既定の `high` を前提にしているものは、`anthropic-messages` なら `low` に直すか、`effort` を明示する形に直す。テストを消さない。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、三つの対象の実行ファイルの大きさ（変える前と後）と、effort の解決の例を書く。

## 補足（2026-10-19、始める前の点検）

本節は、上の各節に優先する。

1. effort は、`doctor`・`/status`・セッションの記録のヘッダのどれにも出ていない（`cli/doctor/mod.rs`、`ui/tui/mod.rs` の `Status`、`session/record.rs` の `Header` と `RecordedProvider`）。この作業でも出さない。「作るもの」の 2 の「出る effort は解決した後の値のまま」は、この意味に読み替える。
2. 再開（`resolve_resume`）は effort を記録から取らず、`resolve_exec` で今の `--effort`、提供者の設定、種類ごとの既定の順に決める。変更より前に既定で始めた `anthropic-messages` のセッションは、再開すると `low` になる。圧縮の要求（`agent/compaction/mod.rs`）も同じ値を使う。どちらもこのままとする。
3. プロジェクトの設定（`ProjectConfig`）は providers を受け付けないので、影響しない。
4. 種類ごとの既定は、`src/provider/types.rs` の `AdapterKind` に `default_effort(self) -> &'static str` として置く（`official_capabilities` と同じ場所）。「触るファイル」に `src/provider/types.rs` を加える。`config/mod.rs` の `default_effort()` と `#[serde(default = "default_effort")]` は消す（残すと使われない警告で検査が落ちる）。空の検査は `provider.effort.as_deref() == Some("")` で誤りとする。
5. `openai-chat` の既定 `high` は、`AgentConfig.effort` が `String` なので持つ値であり、要求には付けない（「この作業が認める型の変更」の「記録と表示のために」は誤り）。
6. `src/provider/openai_responses/request.rs` の `unwrap_or("medium")` は通らない道であり、変えない。
7. 直す既存のテストは二つ。`tests/config.rs` の 183 行あたり（`official.effort` は `None` になる）と、`tests/provider_anthropic.rs` の 390 行あたり（`effort: None` のときの期待を `"low"` にする）。effort を明示しているほかのテストは変えない。`tests/expected/` に effort はない。
8. 「既定のときに要求の `output_config.effort` が `low`」は、設定から要求までの通しで確かめる。`tests/exec.rs` の協定ごとに要求の本文を確かめるテスト（effort を省いた `support/exec_fixture.rs` の設定を使う）に、Anthropic の `/output_config/effort == "low"` と Responses の `/reasoning/effort == "high"` を加える。
9. 解決の順は `tests/config.rs` で確かめる。既存の `--effort high` を `anthropic-messages` に渡す検査は「オプションが既定より優先する」を兼ねる。加えるのは、effort を省いた `anthropic-messages` が `low`、`openai-responses` が `high` に解決されること、設定の `effort` が既定より優先すること、`effort = ""` が誤りになること。
10. 大きさの「変える前」は、`Cargo.toml` を変える前に、`cargo build --release -p benitoiteh`（ホスト）と、`--target aarch64-unknown-linux-musl`・`--target x86_64-unknown-linux-musl` で測って控える。「三つの対象」はこの三つである。
11. `tools/perf/measure.py` と `test_measure.py` は変えない。変更の後は、`measure.py` の `default` と `--opt-s` がどちらも `"s"` になる。`tools/perf/README.md` の `--opt-s` の段落に、「BH-060 は 2026-10-19 に `"s"` に決めた。今は既定も `"s"` なので、`--opt-s` は同じ設定どうしの比べになる」旨を一文加える（「触るファイル」に加える）。
12. 設計書は変えない（05-01 と 07-02 は、オーケストレータがこの作業の前に直した）。
