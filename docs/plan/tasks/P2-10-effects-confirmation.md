# P2-10 対話の画面の、エフェクトの確認

## 目的

制限モードの実行の前の確認（`ApprovalKind::Effects`（P2-08）と `ApprovalKind::TestEffects`（P2-12））を、対話の画面（P4-07 の確認の画面）で、設計書 03-03 の「制限モードの確認」が定める事実をすべて示す形にする。示すのは、スクリプトのパスと増えたエフェクト、エフェクトごとの説明、OS のサンドボックスが許す範囲、`Process.Run` を含むときの許すプログラムと注意、シェルの参照の注意である。題、答えのキー、対象のパス、エフェクトの一覧、増えたエフェクト、シェルの参照の行は P2-08 と P2-12 で作ってあるので、この作業では、エフェクトの説明、サンドボックスの範囲、許すプログラムと注意を加え、行の並びを整える。

## 読む設計書

- [利用者の確認](../../design/03-safety/03-03-approval.md)の「確認の共通の規則」（LLM の説明と事実の分け方、事実を答えのキーの直前に置くこと、収まらないときに許す答えを受け付けないこと（2026-10-18））「制限モードの確認」
- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「実行の前の検査の報告」（`Assert.Check` を示さないこと、`processShellReference`）
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「制限モードの方針」「子プロセスの後始末」（macOS で、グループを抜けたプロセスを止められないこと）
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「保証しないこと」（`Process.Run` を許すと、処理系で別のスクリプトを起動できること）
- [対話の画面](../../design/05-interface/05-02-tui.md)の「確認の画面」「出力の示し方」（目に見えない文字、幅の数え方）
- [未決事項](../../design/open-issues.md)の BH-053（2026-10-17 の決定: 試作は英語だけ）
- 既存のもの: P4-07 の確認の画面（`src/ui/tui/confirmation.rs`、事実の行の作り方、答えのキー、`fits`、`ApprovalPrompt`）、P2-08 の `ApprovalKind::Effects` と `ApprovalFacts` の欄（`targets`・`effects`・`added_effects`・`shell_reference`）と事実の行、P2-12 の `ApprovalKind::TestEffects`、`sandbox::restricted_policy`、`assets/benitoite/v0.0.7/skill/SKILL.md` の「Before running」の表

## この作業が認める型の変更

- `ApprovalFacts` に、サンドボックスの範囲（書ける場所の一覧、IP の通信の可否）と、許すプログラムの一覧の欄を `#[serde(default)]` で加える。P2-08・P2-12・P2-09 の確認の要求を作る箇所で埋める。

## 作るもの

- 確認の画面の題と事実の行（`Effects` の種類）。並びは次のとおりとし、どれも `benitoiteh` が作る事実として示す（LLM の説明は事実より前）。
  1. スクリプトのパス（テストでは対象のパスの一覧）。P2-08 の `Targets` の行。
  2. エフェクトの一覧。P2-08 の `Effects` と `Added effects` の行を、次の形に改める。前に確認したものがあれば、増えたものを先に、印（例: `+`）を付けて示す。`Assert.Check` は示さない。各エフェクトに、同梱の Skill の「Before running」の表の説明の英文（`Console.Write` なら `writes to standard output and standard error`）を添える。説明の英文は、`v0.0.7` の `SKILL.md` の表から写して `approval/text.rs`（か `ui/tui/text.rs`）に置き、写した元の版を注記に書く。
  3. OS のサンドボックスが許す範囲: 書ける場所の一覧、ネットワークの可否。
  4. `Process.Run` を含むとき: 許すプログラムの一覧（処理系と利用者の設定のもの）、一覧にないプログラムの起動は OS のサンドボックスが拒むこと、`Process.Run` を許すと処理系で別のスクリプトを起動してほかのエフェクトもサンドボックスの範囲で使えるようになること。macOS で処理系のほかのプログラムを許しているときは、そのプログラムがプロセスグループを抜けて起動したプロセスを道具の終わりに止められないこと。
  5. 報告がシェルの参照を `"present"` と示し、`Process.Run` を含むとき: スクリプトが `Process.shell` を参照していることと、制限モードではシェルの起動を OS のサンドボックスが拒むこと。P2-08 の `Shell` の行を、この位置に置く。
- 答えのキーは、P2-08 と P2-12 のとおり、許す・拒否する・言付けを添えて拒否する、の三つとする（「このセッションの間」の答えは作らない）。
- 3 と 4 の行の英文（見出しと注意の文）は、全文を `text` に置き、報告に書く。オーケストレータが取り込む前に文面を見直す。
- 03-03 の利用者の言語の訳は示さない。理由: 試作は英語だけとする（BH-053 の 2026-10-17 の決定）。03-03 の該当の記述とこの決定の食い違いは、オーケストレータが設計書で直す。
- 画面に収まらないときの扱い（許す答えを受け付けない）は、P4-07 の規則をそのまま当てる。事実の行が多い（許すプログラムの一覧が長いなど）ときも、題・すべての事実・答えのキーが収まらなければ許さない。
- 記録: 確認の要求の記録に、加えた欄を書く。

## 触るファイル

- `crates/benitoiteh/src/ui/tui/`（`confirmation.rs`、`text.rs`）
- `crates/benitoiteh/src/approval/`（事実の欄、`text.rs`）
- `crates/benitoiteh/src/tools/benitoite_run.rs`・`benitoite_test.rs`・`benitoite_scratch.rs`（事実を埋める箇所。三つの道具が確認の要求を作る部品を共有していれば、その部品だけ）
- 関係するテスト

## テスト

- 端末なしの画面のテストで、各事実の行が並びどおりに出ること。増えたエフェクトの印。`Assert.Check` が出ないこと。`Process.Run` の有無で 4 の注意が出ること・出ないこと。シェルの参照の `"present"` と `"absent"`。
- 説明の英文が、`assets/benitoite/v0.0.7/skill/SKILL.md` の表の各行と一致すること（写しの確かめ。期待値を対象の処理で作らない）。
- 許すプログラムの一覧が長く画面に収まらないとき、許す答えを受け付けないこと。
- 確認の画面の前に押したキー、端末の制御の列を含むパスで、確認が進まず、画面が書き換わらないこと（安全の性質のテスト。07-01。P4-07 のテストの形を `Effects` の種類にも当てる）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、画面の例（各場合）、足した英文の全文、実行ファイルの大きさの変化を書く。

## P2-08〜P2-12 の実装の後の補足（2026-10-18）

P2-08、P2-12、P2-09、P4-07 を取り込んだ後に、この文書を実装に突き合わせた。上の記述と食い違うところは、この節を正とする。

### 既にあるもの（作り直さない）

- 題（`ui/tui/text.rs` の `approval_title`。`╭─ Run Benitoite script?`・`╭─ Run Benitoite tests?`）。`a`・`s` は `confirmation.rs` の `confirmation_input` が `Command`・`WebFetch` に限っており、`Effects`・`TestEffects` で受けないことと、先行入力と 300 ミリ秒の守りは `tests/tui/interaction.rs` の `approval_gate_and_key_table_have_no_implicit_or_modified_answer` が確かめている。テストの 4 のうちキーの部分は加えない。
- 事実の行（`confirmation.rs` の `Confirmation::facts` の `Effects | TestEffects` の枝）: `Targets`、`Effects`、`Added effects`、`Imports files that benitoiteh does not read`（P2-12 の `unread_imports`）、`Shell`。
- `Assert.Check` は道具の側で除いてから `facts.effects` と `added_effects` を作る（`benitoite_run.rs`・`benitoite_test.rs`）。画面の側で除く処理とそのテストは要らない。
- `shell_reference` は、道具が `processShellReference` が `"present"` で `Process.Run` を含むときだけ真にする。
- 収まらないときに許す答えを受けない扱い（`fits`）と制御の文字の変換は、種類によらない共有の部品で働く。手を加えない。
- `benitoite_scratch` の確認は `benitoite_run::execute_script` を通る（`ApprovalKind::Effects`、`Targets` は一時ファイルの実の絶対パス、`added_effects` は作らない）。事実を埋める箇所は `benitoite_run.rs` の `execute_script` と `benitoite_test.rs` の `execute` の二つであり、`benitoite_scratch.rs` は触らない。一時的なスクリプトの題と示し方は今のままとする。
- 「03-03 の利用者の言語の訳は示さない…」の項は済んだ（03-03 は英語だけと書いている）。作業から除く。

### 残る作業

- `ApprovalFacts` に `restricted: Option<RestrictedFacts>` を `#[serde(default, skip_serializing_if = "Option::is_none")]` で加える。`RestrictedFacts { writable: Vec<PathBuf>, network: bool, programs: Vec<PathBuf> }`（`Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize`）。ほかの種類の facts の JSON を変えない。
- 二つの道具で、`context.run` があるときに、`benitoite::effects::restricted_access(&集合, &run.processor, &run.allowed_programs)` と `run.locations` から埋める。共有の関数は `tools/benitoite_process.rs` に置く。`writable` は `write_project` なら `[project, temporary]`、そうでなければ `[temporary]`、`network` は `allow_ip`、`programs` は `programs`。集合は、前に確認した集合と今回の報告の集合（`Assert.Check` を除く）の和とする。理由: 承認すると確認した集合は和になり、呼び直しは二度目の報告の集合で実行するので、一度目になかった前の確認のエフェクトも含みうる。今回の集合だけで作ると、実際の範囲が画面より広くなりうる。`context.run` がないとき（道具のテストの `Fixture::new()`）は `None` とし、画面はその区画を出さない。
- エフェクトの説明: `ui/tui/text.rs` に `effect_description(Effect) -> &'static str` を網羅の `match` で置く。英文は `assets/benitoite/v0.0.7/skill/SKILL.md` の「Before running」の表から写し、注記に版（`v0.0.7`）を書く。`Process.Run` は末尾の ` (name them)`（エージェントへの指示）を除いて `starts external commands` とする。`IO.All` と `Assert.Check` の行は使わない。説明は、端末が狭くても省かない（収まらなければ、今の `fits` の規則で許す答えを受けない）。
- 事実の並び（`Effects | TestEffects`。LLM の説明は今のとおり事実より前）:
  1. `Targets`（今のまま）。
  2. 読まないファイルの取り込みの件数（今の行を `Targets` のすぐ後に移す）。
  3. `Effects`: 一行に一つ、`<印> <名前>  <説明>`。前の確認があるときは増えたものを先に `+` を付けて並べ、残りは今の綴りの順とする（印のない行は空白二つで揃える）。`Added effects` の行は廃する。空なら `none`。
  4. `Sandbox`: 書ける場所と IP の通信の可否。
  5. `Process.Run` を含むときだけ `Programs`: 許すプログラムの一覧と注意。macOS（`cfg!(target_os = "macos")`）で `programs` に処理系のほかのものがあるときだけ、プロセスグループの注意を加える。
  6. `Shell`（今の行）。
- 見出しは事実ごとに一行を使う今の `fact` の形を保ち、説明の行と注意の行を一つの事実の値（`\n` で繋ぐ）にまとめる。
- 答えのキーの文（`Effects`・`TestEffects` だけ）を `[y] run and allow these effects for this session  [n] decline  [m] decline with a message` に改める（設計書 05-02「確認の画面」）。
- 英文の案（取り込む前にオーケストレータが見直す）:
  - `Sandbox` の値: `can write: <project> and the session temporary directory` か `can write: only the session temporary directory`、改行して `network: allowed` か `network: blocked`。
  - `Programs` の値: 許すプログラムの実のパス（一行に一つ）の後に、`The OS sandbox blocks starting any other program.` と `Allowing Process.Run lets the script start benitoite on another script, which can use any operation the OS sandbox allows, including effects not listed above.`
  - macOS の注意: `On macOS, processes that an allowed program starts in a new session or process group are not stopped when the tool ends.`
- テスト（`tests/tui/interaction.rs`。今のテストを広げ、新しい関数は最小にする）:
  - `effect_confirmation_displays_all_facts_but_not_source_hashes_and_must_fit` を、新しい並び、`+` の印、`Added effects` の廃止、キーの文に合わせて直し、`Process.Run` の有無で `Programs` の区画が出ること・出ないこと、macOS の注意の有無（`cfg!` で期待を分ける）を加える。
  - `oversized_facts_disable_approval_keep_decline_and_never_overflow_the_screen` の表に、許すプログラムが多い `Effects` の場合と、制御の列と改行を含む `Targets` の場合を加える。
  - 写しの一致: 本物の `SKILL.md` の表の行を読み（`env!("CARGO_MANIFEST_DIR")` から辿る。実行ファイルには埋め込まない）、12 のエフェクトを持つ確認を幅の広い端末（例: 300×60）で描き、各行の説明の英文がそのまま出ることを確かめる（`Process.Run` は ` (name them)` を除いて比べる）。期待値を `effect_description` から作らない。
  - 収まり: `Console.Write, File.Read, File.Write, Process.Exit` に `Sandbox` を持つ確認が 80×24 で許せること。
  - `tests/tools_benitoite_run.rs`・`tools_benitoite_test.rs`: `Fixture::sandbox()` を呼んで、`File.Write`・`Http.Connect`・`Process.Run` の有無で `facts.restricted` の三つの欄が変わることと、前の確認との和で作ることを、本物の報告で確かめる（実行の前に返るので Codex の中で動く。`BENITOITEH_TEST_BENITOITE` が要る）。
- 完了の報告に、`IO.All` と `Process.Run` を持つスクリプトの確認が許せる最小の端末の大きさ（80 桁と 120 桁で）を書く。
