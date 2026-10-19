# P5-10 openai-chat の reasoning_effort と推論のトークンの数え方

## 目的

設計書 02-01 の「OpenAI の Chat Completions API」と「提供者ごとの機能の表」に 2026-10-19 に加えた二つの決定を実装する。

1. 機能の表の `reasoning_effort`（既定は偽）が真のときだけ、`openai-chat` の要求に `reasoning_effort` を付け、値を解決した effort とする。
2. `completion_tokens_details.reasoning_tokens` がなく、`total_tokens` が `prompt_tokens` と `completion_tokens` の和より大きいときは、その差を推論のトークンとし、共通の形の出力の量に足す（Google の Gemini の OpenAI 互換の口は、推論のトークンを `total_tokens` にだけ含める）。

評価の道具の `benitoiteh-gemini` の項目で、この機能を真にする。この作業で本物の LLM の提供者を呼ばない。

## 読む設計書

- [提供者の層](../../design/02-providers/02-01-provider-layer.md)の「共通の形」「OpenAI の Chat Completions API」「提供者ごとの機能の表」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「設定ファイル」（`capabilities` の欄）
- 既存のもの: `crates/benitoiteh/src/provider/types.rs`（`Capabilities`、`Usage`、`official_capabilities`）、`src/config/mod.rs`（`CapabilityOverrides` と、その解決）、`src/provider/openai_chat/request.rs`、`src/provider/openai_chat/assembly.rs`（395〜405 行あたりの使用量の読み取り）、`src/cli/doctor/`（機能の表示があれば）、`tests/provider_openai_chat.rs`、`tests/config.rs`、`tools/skill-eval/harnesses.json`・`skill_eval.py`（提供者の `capabilities` を設定に書く所）・`test_skill_eval.py`・`SOURCE.md`

## この作業が認める型の変更

- `Capabilities` に `reasoning_effort: bool`（既定 `false`）、`CapabilityOverrides` に `reasoning_effort: Option<bool>` を加える。`official_capabilities` のどのアダプタでも偽とする。
- `openai_chat/request.rs` の要求の組み立ての引数に、機能の表の値を渡す形の変更を認める。

## 作るもの

### 1. reasoning_effort

- 設定ファイルの `providers.<名前>.capabilities.reasoning_effort = true` を受け付ける（プロジェクトの設定ファイルは providers を受け付けないので、今のまま）。
- `openai-chat` は、機能が真のとき、要求の本文に `"reasoning_effort": "<解決した effort>"` を付ける。偽のときは付けない（今の振る舞い）。値は検査せずにそのまま送る（提供者が誤りを返す。設計書のとおり、値の一覧は提供者ごとに違う）。
- `anthropic-messages` と `openai-responses` では、この機能を使わない（設定で真にしても振る舞いは変えない）。
- 機能を表示する箇所（`doctor` など）があれば、ほかの機能と同じ形で示す。

### 2. 推論のトークン

- `openai-chat` の使用量の読み取りで、`completion_tokens_details.reasoning_tokens` がなく、`total_tokens`・`prompt_tokens`・`completion_tokens` がそろい、`total_tokens` が `prompt_tokens + completion_tokens` より大きいときは、差を `reasoning_tokens` とし、`output_tokens` を `completion_tokens + 差` とする。加算は溢れない形で行う。
- `reasoning_tokens` が報告されているとき（OpenAI と DeepSeek の形。`completion_tokens` に推論を含む）と、差が 0 以下のときは、今の読み方のままとする。
- `raw` には提供者の値をそのまま残す。

### 3. 評価の道具

- `tools/skill-eval/harnesses.json` の `benitoiteh-gemini` の `provider.capabilities` に `"reasoning_effort": true` を加える。`skill_eval.py` がこの値を `benitoiteh` の設定ファイルに書けるかを確かめ、書けなければ書けるようにする（ほかの機能と同じ扱い）。`SOURCE.md` の「写した後に変えた点」に一行加える。ほかの項目は変えない。

## 触るファイル

- `crates/benitoiteh/src/provider/types.rs`、`src/config/mod.rs`、`src/provider/openai_chat/request.rs`、`src/provider/openai_chat/assembly.rs`（要れば `src/agent/transport.rs` と `src/cli/doctor/`）
- `crates/benitoiteh/tests/provider_openai_chat.rs`、`tests/config.rs`（要れば `tests/doctor.rs`）
- `tools/skill-eval/harnesses.json`、`skill_eval.py`、`test_skill_eval.py`、`SOURCE.md`

## テスト

- 設定: `capabilities.reasoning_effort` の既定が偽、真を受け付けること。
- 要求: 機能が真のとき、`openai-chat` の要求に `reasoning_effort` が解決した effort で付くこと。偽のときは付かないこと（既存の `reasoning_effort` が無いことの検査を保つ）。
- 使用量: Gemini の形（`{"prompt_tokens":36,"completion_tokens":5,"total_tokens":290}`）で、`reasoning_tokens` が 249、`output_tokens` が 254 になること。`reasoning_tokens` が報告される形と、`total_tokens` が和に等しい形と、`total_tokens` がない形では、今の値のままであること。
- 評価の道具: `benitoiteh-gemini` の生成する設定に `reasoning_effort = true` が入ること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、機能が真のときの要求の本文の例と、Gemini の形の使用量を読んだ結果を書く。

## 補足（2026-10-19、始める前の点検）

本節は、上の各節に優先する。

1. **型と解決**: `Capabilities.reasoning_effort` は `Default` の導出で偽になる。`official_capabilities` の式は `..Capabilities::default()` のままでよい。`CapabilityOverrides` に `reasoning_effort: Option<bool>` を足し、`resolved_capabilities` の上書きの並びに同じ形で加える（忘れると欄が拒否されるか、黙って無視される）。src と tests の構造体の式はどれも `..default()` を使うので直す所はない。機能を表示する箇所（doctor、`/status`）と記録には機能の欄がないので、変えない。
2. **要求の組み立て**: アダプタは `self.capabilities` を持ち、`transport.rs` は既に `RequestOptions.effort` に解決した effort を渡している。引数の形と `transport.rs` は変えない（「この作業が認める型の変更」の 2 項目めは要らない）。`build_request` で、`self.capabilities.reasoning_effort` が真で `options.effort` が `Some` のときだけ `reasoning_effort` を付ける。`None` なら付けない。道具が無い要求でも消さない。圧縮の要求にも同じく付く。`RequestOptions.effort` の注釈（`openai_chat/request.rs` の 19 行あたり）と `AdapterKind::default_effort` の注釈（`types.rs` の 309 行あたり）を、この振る舞いに合わせて改める。
3. **使用量**: `normalize_usage` で、`completion_tokens_details.reasoning_tokens` が数として読めず、`prompt_tokens`・`completion_tokens`・`total_tokens` が数として読め、`prompt_tokens.checked_add(completion_tokens)` が `total_tokens` より小さいときだけ、`reasoning_tokens = total − 和`、`output_tokens = total − prompt` とする。和が溢れるときは今の読み方とする。断片の `usage` は合わせた全体から作り直す形（`extend`）なので、途中と最後に二度来ても差は一度だけ数える。集計（`Summary` は推論の欄を持たない）、`max_run_tokens`、記録、圧縮の `add_usage` は変えない。
4. **使用量のテストの場所**: `tests/provider_openai_chat/gemini.rs` の `assembler()` と `feed()` を使う。途中の断片で一度（例: completion 1、total 37）、最後に `{"prompt_tokens":36,"completion_tokens":5,"total_tokens":290}` を流し、`output_tokens` が 254、`reasoning_tokens` が 249、`raw` が最後の提供者の値と等しいことを確かめる。報告がある形、和に等しい形、`total_tokens` が無い形は、既存のテストの値が変わらないことで確かめる。
5. **要求のテスト**: `tests/provider_openai_chat.rs` の `build_request` のテストの `cases` に機能の列を足し、真の場合は `body["reasoning_effort"] == "high"`、偽の場合は今の `is_none` を確かめる。
6. **設定のテスト**: `tests/config.rs` の `capabilities_use_official_defaults_only_at_the_exact_official_destination` に、本家と互換の口の両方で既定が偽であることと、`capabilities = { reasoning_effort = true }` が真に解決されることを足す。
7. **評価の道具**: `benitoiteh_config` は `provider.capabilities` を鍵の一覧なしにそのまま TOML の inline table に書くので、`skill_eval.py` は変えない。`harnesses.json` の `benitoiteh-gemini` の `"capabilities": {}` を `{"reasoning_effort": true}` にする。`test_skill_eval.py` の `test_real_provider_configs_reference_keys_and_declare_prices` の近くで、生成した設定に `capabilities = { reasoning_effort = true }` が入ることを確かめる。`SOURCE.md` には `### P5-10 openai-chat の reasoning_effort` の見出しを新しく作って一行加える。
8. **触るファイルの訂正**: `src/agent/transport.rs`・`src/cli/doctor/`・`tests/doctor.rs`・`skill_eval.py` は触らない。`tests/provider_openai_chat/gemini.rs` を加える。
