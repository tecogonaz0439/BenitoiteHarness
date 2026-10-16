# P1-05 セッションの記録と一時ディレクトリ

## 目的

セッションのディレクトリ、記録のファイル（`session.jsonl`）、`spill/`、一時ディレクトリ、ロックのファイル、記録を残す期間の掃除を作る。再開に要る記録の形（形式の版 1）を決め、記録から会話を組み立て直す関数を作る（`--resume` の入口は P4 で作る）。

## 読む設計書

- [セッション](../../design/01-agent/01-05-sessions.md)の「セッションの記録」「LLM を使ったテストのための記録」「再開」「一時ディレクトリ」「記録を残す期間」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「データの置き場所」
- [エージェントのループ](../../design/01-agent/01-01-agent-loop.md)の「イベント」「取り消し」
- [道具](../../design/01-agent/01-03-tools.md)の「出力の切り詰め」（`spill/` の部分）
- P1-01 の `provider` の型、P1-04 の `config` のデータの置き場所、P0-02 の `safe_fs`

## 作るもの

`crates/benitoiteh/src/session/`。

1. セッションの ID: 開始の時刻（UTC、`YYYYMMDDTHHMMSSZ`）と、`getrandom` で得た 8 バイトを 16 進にしたものを `-` でつなぐ（時刻の順に並ぶ。05-01）。時刻の整形は自前で行う（日時のクレートを使わない。06-01）。
2. セッションのディレクトリ `<sessions>/<ID>/`（0700）。中に `session.jsonl`（0600）、`spill/`（0700）、ロックのファイル `lock`（0600）を作る。ロックのファイルにファイルのロック（`rustix::fs::flock` の排他、待たない）を掛け、セッションの間持ち続ける。データのディレクトリの中だけを扱う処理として、道具の操作と分ける（01-03 の「道具の定義の共通の規則」）。
3. 一時ディレクトリ `<tmp>/benitoiteh-<ID>/`（0700）。排他に作り（すでにあれば誤り）、作った後に、所有者が自分で権限が 0700 であることを、リンクを辿らずに確かめる（Linux の `/tmp` は利用者の間で共有されるため）。セッションの終わりに消す。起動のときに、`benitoiteh-<ID>` の形の名前の一時ディレクトリのうち、対応するセッションのディレクトリのロックが取れるもの（動いていない）と、セッションのディレクトリがないものを消す。消すときは、シンボリックリンクを辿らない削除（`std::fs::remove_dir_all`）を使う（01-05 の「一時ディレクトリ」）。
4. 記録を残す期間: 起動のときに、`retention_days`（既定 30）より古いセッションのディレクトリ（ID の時刻で判定）のうち、ロックが取れるものを消す。
5. 記録の行の型 `Record`（`serde`、`"type"` の欄で種類を分ける）。形式の版は 1 とする。行の種類と欄は次のとおりとする。欄を足すのはよいが、名前を変えるときは報告に書く。
   - `header`: `format`（1）、`session_id`、`started_at`、`benitoiteh_version`、`mode`、`project_dir`、`provider`（設定の名前、`base_url`、`model`、`adapter`）、`benitoite`（パスと版。P1 では `null`）、`skill_version`、`system_prompt_sha256`、`tools_sha256`。
   - `user`: 利用者の発言（依頼、操縦）と、`kind`（`request`・`steer`・`environment`）。
   - `assistant`: `AssistantTurn`（P1-01。`RawItems` を含む）と番の番号。
   - `tool_start`: 内部の ID、提供者の ID、道具の名前、引数。子プロセスを起動する道具と書く道具だけが、実行を始める前に書く。
   - `tool_result`: `ToolResult`（P1-01）と、`spill/` に書いた場合はその場所。
   - `approval`: 確認の要求と結果（P1 では使わないが型を置く）。
   - `retry`、`error`、`turn_end`（使ったトークンの量）、`session_end`（終わり方）、`summary`（後述）。
   - `project_dir_changed`、`rewind`、`compaction`（後の段階で使う。型だけを置く）。
   - ループのイベント（P1-09 の `AgentEvent`）は、次の対応で記録に写す。`session` は `agent` より下の層なので、`Record` は `AgentEvent` を包まず、自分の型で持つ。

| `AgentEvent` | `Record` |
|---|---|
| `session_start` | `session_start`（`notice` などのイベントの欄。ヘッダとは別の行） |
| `turn_start` | `turn_start` |
| `text_delta`・`reasoning_delta` | 書かない（応答は `assistant` の行にまとめる） |
| `tool_call` | `tool_call`（応答の `assistant` の行の後に、検証の結果とともに） |
| `approval_request`・`approval_result` | `approval`（要求と結果を一行ずつ。`phase` の欄で分ける） |
| `tool_result` | `tool_result` |
| `turn_end` | `turn_end` |
| `compaction` | `compaction` |
| `retry`・`error`・`session_end` | 同じ名前の行 |

6. 記録の書き手 `SessionWriter`: 一行ずつ JSON にして追記し、行ごとに `flush` する（`sync_data` までは求めない）。一行が 16 MiB を超えるときは、大きな欄（道具の結果の文など）を `spill/` のファイルに書き、行にはその場所を書く。API キーと HTTP のヘッダは型に持たないので書けない形にする。
7. `spill/` の書き手: 一つの出力について 16 MiB まで書き、超えた分は書かずに、書いた量と捨てた量をファイルの末尾に書く（01-03 の「出力の切り詰め」）。ファイルの名前は、道具の呼び出しの内部の ID から作る。
8. 記録の読み手と、会話の組み立て直し `replay`: `session.jsonl` を読み、最後の行が途中で切れていれば捨てる。`user`・`assistant`・`tool_result` から `Conversation` を作る。`tool_start` があって `tool_result` がない呼び出しには、`Tool call was interrupted before its result was recorded. Its effects are unknown; check the files before retrying.` を、どちらもない呼び出しには `Tool call was cancelled by the user.` を、結果として補う（01-05 の「再開」）。最後の `compaction` があれば、その後から組み立てる（P1 では `compaction` の行は書かれないので、型の扱いだけでよい）。
9. 集計 `summary`（記録では `session_end` の後の最後の行）（01-05 の「LLM を使ったテストのための記録」）: 番の数、道具ごとの呼び出しの回数、検査と修正の回数、確認の結果の数、トークンの量（入力、キャッシュから読んだ量、キャッシュに書いた量、出力）、かかった時間（ミリ秒）、終わり方。集計を数える型を置き、P1-09 のループが数を足し、P1-10 の `exec` が最後に書く。

## 触るファイル

- `crates/benitoiteh/src/lib.rs`（`pub mod session;`）
- `crates/benitoiteh/Cargo.toml`（`getrandom` を加える。`rustix` の機能を足す）

## テスト

- 記録を書いて `replay` で読むと、会話が同じになる（`RawItems` の順序と ID を含む）。
- 最後の行が途中で切れた記録を読める。`tool_start` だけの呼び出しと、呼び出しだけの応答に、決まった結果が補われる。
- 16 MiB を超える行が `spill/` に移る（テストでは上限を小さくできる形にしてよい）。`spill/` の 16 MiB の上限と、捨てた量の記録。
- ディレクトリとファイルの権限（0700・0600）。
- 動いているセッション（ロックを持つ）の一時ディレクトリを消さず、動いていないものを消す。一時ディレクトリの中のホームを指すシンボリックリンクを辿らずに消す。
- 古いセッションのディレクトリを消す。
- ID が時刻の順に並び、形が決まった形である。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、記録の行の型の一覧と欄を書く（オーケストレータが設計書 01-05 に「記録の形式の版 1」として反映する）。
