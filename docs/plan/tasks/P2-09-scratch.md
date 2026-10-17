# P2-09 一時的なスクリプト `benitoite_scratch`

## 目的

制限モードの道具 `benitoite_scratch` を作る。LLM が渡したコードを一時ディレクトリに新しい名前のファイルとして書き、実行の前の検査の報告を得て、エフェクトが確認なしの範囲に収まれば、読める範囲を限った OS のサンドボックスの中で確認なしで実行する。範囲を超えれば `benitoite_run` と同じく確認を受ける（設計書 01-03 の「benitoite_scratch（制限モード）」、03-03 の「一時的なスクリプト」）。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「benitoite_scratch（制限モード）」「benitoite_run（制限モード）」「出力の切り詰め」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「一時的なスクリプト」（既定の範囲、利用者の設定で加えられるものと加えられないもの、プロジェクトの設定では変えられないこと、確認なしで実行するときの読める範囲と書ける場所）「制限モードの確認」「非対話での扱い」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「制限モードの方針」「方針の要素」
- [セッション](../../design/01-agent/01-05-sessions.md)の「一時ディレクトリ」「巻き戻し」（記録する時点）
- 既存のもの: P2-08 の `benitoite_run`（報告、方針、実行、結果、`ApprovalKind::Effects` の確認の判定、`ConfirmKey`）、P2-12 の非対話の判定（ループの `approve`）と続けての拒否の数、`src/config/mod.rs`（`RestrictedConfig.scratch_auto_effects` と名前の検査）、`src/sandbox/policy.rs`（`ReadAccess::Roots`）、`src/session/`（一時ディレクトリ）

## この作業が認める型の変更

- `ToolId` に `BenitoiteScratch` を加える。並べて実行しない。
- P2-12 の非対話の判定に、確認なしの範囲を渡す口を加える（今は `benitoite_run`・`benitoite_test` の空の範囲だけ）。
- 巻き戻しの時点の理由は、`CheckpointReason` に `BenitoiteScratch` を加えるか、P2-08 の `BenitoiteRun` を使うかをこの作業で決め、報告に書く。
- P2-12 の続けての拒否の停止の英文を、`The run stopped because benitoite_run, benitoite_test or benitoite_scratch was refused 3 times in a row: the effects were outside --allow.` に改める。

## 作るもの

- 引数は `{ "code": "string", "args": ["string (optional)"], "stdin": "string (optional)" }`。
- `code` を、セッションの一時ディレクトリに `scratch-<n>.bnt`（`n` はセッションの中で 1 から増やす。すでにある名前には書かず、次の番号にする）として、リンクを辿らずに新しく作って書く。書いたファイルのパスを結果に含める。
- 報告（`check --report=json`）を得て、エフェクト（`Assert.Check` を除く）が確認なしの範囲に収まるかを決める。範囲は、既定（エフェクトなし、`Console.Write`、`State`、`Process.Exit`）と、利用者の設定の `restricted.scratch_auto_effects` の和とする。プロジェクトの設定では変えられない（今の `ProjectConfig` が `restricted` を持たないことをテストで確かめる）。
- 収まるとき: 確認なしで実行する。方針は、03-03 のとおり、読める範囲をプロジェクトのディレクトリ、一時ディレクトリ、処理系の実行ファイル、処理系の動作に要る OS の場所に限り（`ReadAccess::Roots`）、書ける場所は一時ディレクトリだけとする。ネットワークとプログラムの起動は、P2-08 の方針の規則のとおり（範囲の中では `Http.*` と `Process.Run` は含まれないので、どちらも許さない）。処理系の動作に要る OS の場所は、macOS と Linux のそれぞれで、処理系の `run` が動く最小の組を試して決め、報告に一覧と理由を書く（例: macOS の `/usr/lib`、`/System/Library` の一部。Linux の静的リンクの処理系では要らない見込み）。
- 収まらないとき: `benitoite_run` と同じ確認（`ApprovalKind::Effects`。確かめの単位は書いたファイルの実のパスの `ConfirmKey::Run`）を受け、許されたら、P2-08 のとおり報告を得直して確認した範囲と照らし、P2-08 の方針（読める範囲は既定）で実行する。非対話では、P2-12 の判定で、確認なしの範囲と `--allow` の和に収まれば実行し、超えれば P2-12 の英文を返して、続けての拒否の数に入れる。
- 実行は P2-08 の `benitoite_run` の部品を使う（`--allow-effects` に報告の集合、出力の扱い、時間の上限 120 秒、止め方、結果の形、`processor_launches` の数え方）。巻き戻しの時点を実行の直前に記録する。戻せないものの一覧に `benitoite_scratch` の呼び出しを、`benitoite_run` と同じ選び方で含める。
- 一時的なスクリプトの根は一時ディレクトリなので、プロジェクトのモジュールを取り込めないことを、道具の説明に書く。道具の説明の英文を `prompts/tools/benitoite_scratch.md` に置く（案を報告に書き、オーケストレータが取り込む前に見直す）。
- 制限モードの `ToolSet` に、01-03 の表の順（`benitoite_test` の後）で加える。期待のファイルを作り直す。

- `/new`（P4-08）は、安全に関わる値を今のセッションの値に固定する（`SessionOverrides`）。この作業で `restricted.scratch_auto_effects` を使い始めるので、`/new` ではこの値も固定し、読み直した設定で置き換えない。テストを加える。

## 触るファイル

- `crates/benitoiteh/src/tools/`（`benitoite_scratch.rs`（新）、`mod.rs`、`definitions.rs`、`text.rs`）
- `crates/benitoiteh/prompts/tools/benitoite_scratch.md`（新）
- `crates/benitoiteh/src/sandbox/`（読める範囲を限る方針）
- `crates/benitoiteh/src/approval/`、`src/agent/`（範囲の受け渡し、停止の英文、`rewind.rs`）
- `crates/benitoiteh/src/rewind/types.rs`（`CheckpointReason` を加えるとき）
- `crates/benitoiteh/tests/expected/`、新しいテスト（サンドボックスを要するものは `tests/sandbox_*.rs`）

## テスト

- 範囲: `Console.Write` だけのスクリプトが確認なしで動くこと。`File.Read` は既定で確認を求め、利用者の設定で加えると確認なしになること。`File.Write`・`Process.Run`・`Http.Connect` を `scratch_auto_effects` に書くと設定の誤りになること（今の検査の確かめ）。
- 読める範囲（安全の性質のテスト。07-01 の「読まないファイル」の考えに当たる）: 確認なしで動く `File.Read` のスクリプト（設定で範囲に加えたもの）が、プロジェクトのディレクトリと一時ディレクトリの外（ホームディレクトリの下に置いたファイル）を読めないこと。プロジェクトに書けないこと。macOS と Linux で。
- 名前: 二度呼ぶと `scratch-1.bnt` と `scratch-2.bnt` になること。一時ディレクトリにあらかじめ `scratch-1.bnt` へのシンボリックリンクを置いても、リンクの先に書かないこと。
- 範囲を超えたときの確認と、非対話での扱い。
- プロジェクトのモジュールを取り込むコードが、検査の誤りになること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、道具の説明の英文、足した英文の全文、OS ごとの読める範囲の一覧と決めた理由、実行ファイルの大きさの変化を書く。
