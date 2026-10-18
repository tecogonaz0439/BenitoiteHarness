# P4-12 `doctor` と起動の、プロジェクトの場所の誤りの文言

## 目的

2026-10-18 に設計者が `~/.config` の中で `benitoiteh doctor` を実行したとき、実行したディレクトリがプロジェクトとして扱われ、書ける場所の検査（設計書 03-02「書ける場所の検査」）で拒まれた。安全の規則は意図どおりに働いたが、示した文言が原因を伝えなかった。

- 処理系の四つの項目（`benitoite.*`）の説明が、`writable root is inside a home dot directory or macOS Library: /Users/tecogonaz/.config (/Users/tecogonaz)` で、処理系の項目なのに書ける場所の文だけが出た。
- 要約が `Restricted mode sandbox: available (checked with benitoiteh itself; Benitoite was not found or failed validation).` で、処理系は見つからなかったのではなく、確かめなかった。
- `writable` の項目の文が、どのディレクトリがプロジェクトとして選ばれたか（実行したディレクトリか `--project` か）と、どうすればよいかを示さない。

この文言を直す。安全の規則と、項目の状態と終了状態は変えない。

## 読む設計書

- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「doctor」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「書ける場所の検査」
- 既存のもの: `src/cli/doctor/`（`mod.rs`、`text.rs`、`processor.rs`、`probe.rs`）、`src/config/writable.rs`（`WritableError`、`WritableRule`）、`src/config/text.rs`、`src/cli/exec.rs`（起動のときの書ける場所の検査の誤り）、`tests/doctor.rs`

## この作業が認める型の変更

なし。`WritableError` の `Display` は変えず、呼ぶ側で文を組み立てる（拒まれたパスがプロジェクトのディレクトリかを `path` で見分ける）。

## 作るもの

1. `doctor` で、プロジェクトのディレクトリが書ける場所の検査に当たったときは、`writable` の項目の説明を次の形にする（英文の案。`text.rs` に置く）。
   - `The project directory <path> (<source>) cannot be used: <WritableError の文>. Run doctor in a project directory or pass --project <dir>.`
   - `<source>` は、`--project` を指定したときは `from --project`、ほかは `the current directory`。
   - 当たったのが通常モードの設定で足した書ける場所のときは、今の文のまま（どの場所かは文に含まれる）。
2. 処理系の四つの項目が、プロジェクトのディレクトリの誤りのために確かめられないときは、`unchecked` のまま、説明を `Requires a usable project directory; see the writable item.` にする。利用者の設定が読めないときの `CONFIG_REQUIRED` と、`Probe` を作れないときの文は今のままとする。
3. 要約: 処理系を確かめなかった（プロジェクトのディレクトリか設定の誤り）ときの文を、処理系が見つからないか形の確かめを通らなかったときの文と分ける。
   - 確かめなかったとき: `Restricted mode sandbox: available (checked with benitoiteh itself; Benitoite was not checked).`
   - 見つからないか形が通らないとき: 今の `RESTRICTED_FALLBACK_OK` のまま。
   - `Summary.restricted_sandbox` の真偽は変えない。
4. 起動（`exec` と対話の形）で、プロジェクトのディレクトリが書ける場所の検査に当たったときの誤りの文にも、1 と同じく `The project directory <path> (<source>) cannot be used: ...` の前置きを付け、`--project <dir>` で別の場所を選べることを示す。終了状態は変えない。

## 触るファイル

- `crates/benitoiteh/src/cli/doctor/`（`mod.rs`、`text.rs`、必要なら `processor.rs`）
- `crates/benitoiteh/src/cli/exec.rs`、`crates/benitoiteh/src/cli/text.rs`
- `crates/benitoiteh/tests/doctor.rs`、起動の誤りの文を確かめる既存のテスト（`tests/exec.rs` か `src/cli/exec/tests.rs`）

## テスト

- `tests/doctor.rs`: 偽のホームの下の `.config` の中で `doctor` を実行し、`writable` の説明が前置きと `the current directory` を含むこと、処理系の四つの項目が `unchecked` で `Requires a usable project directory` を含むこと、要約が `Benitoite was not checked` を含むこと、終了状態が 1 であることを確かめる。`--project` で同じ場所を指したときは `from --project` になること。既存の、処理系が見つからないときの要約の期待は変えない。
- 起動: 同じ場所で `exec` を起動したときの誤りの文が前置きを含み、終了状態が今と同じであること。
- 期待の文は、パスの字面に偶然に一致しない語（`project directory`、`current directory` など）で確かめる。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、変えた英文の全文と、`~/.config` に当たる場所での `doctor` の出力の例を書く。
