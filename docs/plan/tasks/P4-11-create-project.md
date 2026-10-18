# P4-11 制限モードの道具 `create_project`

## 目的

制限モードの道具 `create_project` を作る（設計書 01-03 の「create_project（制限モード）」、Benitoite の ADR 0344 の決定 2、ADR 0345 の決定 4）。利用者の確認を受けて、まだないディレクトリと雛形の `main.bnt` を作り、作ったディレクトリをセッションのプロジェクトに切り替える。前のプロジェクトは、道具の読み書きの範囲、OS のサンドボックス、処理系の問い合わせから外す。非対話では `--project-parent` の直下にだけ作る。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「create_project（制限モード）」（雛形は 2026-10-18 に決定）「道具の定義の共通の規則」「モードごとの道具の一覧」
- [利用者の確認](../../design/03-safety/03-03-approval.md)の「そのほかの確認」「非対話での扱い」「確認の共通の規則」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「書ける場所の検査」（2026-10-18 に `.git`・`.benitoiteh` の要素の拒否を加えた）「制限モードの方針」
- [文脈の管理](../../design/01-agent/01-04-context.md)の「環境の情報」
- [セッション](../../design/01-agent/01-05-sessions.md)の「再開」「巻き戻し」
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「オプション」の `--project-parent`
- [対話の画面](../../design/05-interface/05-02-tui.md)の「確認の画面」
- [テスト](../../design/07-quality/07-01-testing.md)の「安全の性質のテスト」
- 既存のもの: `cli/args.rs`（`--project-parent` は解釈だけあり、`DeferredOptions` と `ArgsError::Unavailable` を返す）、`cli/exec.rs`（`prepare`、`SessionOverrides`）、`approval/`、`agent/execution.rs`（非対話の `Effects` の扱い）、`agent/mod.rs`（`environment_changes`、`changes_since_rendered`）、`tools/mod.rs`（`ToolContext`、`ConfirmKey`）、`tools/exclude.rs`、`safe_fs/mod.rs`（`create_dir`、`create_new`、`open_dir`）、`config/writable.rs`、`rewind/mod.rs`（`CheckpointRecorder::new` は `Record::ProjectDirChanged` を無視している）、`benitoite/query.rs`（`QueryContext.locations.project`）、`session/`（`Record::ProjectDirChanged`、`continue_session` は対応済み）、`ui/tui/`（`confirmation.rs`、`worker.rs` の `status`）、`docs/plan/results/P4-safety.md`

## この作業が認める型の変更

00-common の「止まるとき」に当たる次の変更は、この作業で行ってよい。10-interfaces はオーケストレータが更新する。

- `ToolId::CreateProject`。`ApprovalKind::CreateProject`（`serde` の名前は `create_project`）。事実は `facts.path` に、作るディレクトリの絶対の実のパスを入れる。`ApprovalKind` を網羅する `match` をすべて直す。
- `decide`: `never` なら拒否、ほかは尋ねる。`decide_in_mode` は、制限モードでこの種類を許す側に加える。
- `AgentConfig.project_parent: Option<PathBuf>`。`ExecArgs.project_parent`（`DeferredOptions` と `--project-parent` の `ArgsError::Unavailable` は除く）。
- `AgentEvent::ProjectDirChanged { project_dir }`（記録は既存の `Record::ProjectDirChanged`）。
- `ToolContext` に、作成の文脈（データの置き場所と実行ファイル。例: `with_project_creation`）、確認の前に `call_id` で覚えた候補と作った根の置き場所（`pub(crate)`）、切り替えの関数（`pub(crate)`）を加える。
- `ExcludeRules::rebased(&self, project)`、`Environment::with_project(..)`、`CheckpointRecorder::restart(..)`、`Processor::set_project(..)`（`QueryContext` の作業ディレクトリを内側で変えられる形にする）。
- `safe_fs`: 新しい名前のディレクトリを作り、リンクを辿らずに開き、辿る性質の `ScopeRoot` で返す口。`create_new` に権限を指定できる形。
- `config::writable`: 作る前の候補（実の親と名前）に同じ規則を当てる関数（規則の本体を共有する）と、`.git`・`.benitoiteh` の要素の拒否。

## 作るもの

1. **引数**: `exec` の制限モードだけで `--project-parent <dir>` を受け、実のパスにしてディレクトリであることを確かめる。対話の形と通常モードでは使い方の誤り（終了状態 2）にする。`cli/text.rs` の `HELP` の `--allow` を「まだ使えない」とする古い記述を直し、`--allow` と `--project-parent` を `exec` だけのオプションとして載せる。
2. **道具の定義**: `{ "path": "string" }` を `benitoite_reference` と `web_fetch` の間に置く（制限モードだけ。処理系がないときの `ToolSet::without_benitoite` でも残す）。並べない。説明は `prompts/tools/create_project.md`（新）に置き、`tools/text.rs` から `prompt_parts(include_str!(..))` で読む。説明の英文は完了の報告に全文を書き、オーケストレータが見直す。
3. **パスの検査**（確認の前と、呼び直しの両方で行う）: パスは 01-03 の共通の規則どおり今のプロジェクトから解く（絶対パスも受ける）。空の要素、`.`、`..`、`~` で始まる形（ヒントを付ける）、制御の文字を含む名前、UTF-8 でない実のパスを拒む。親は既存を要し、作らない。候補は「開いた親の実のパス＋最後の名前」とし、リンクを辿らずにまだないことを確かめる。書ける場所の検査の規則（03-02。`.git`・`.benitoiteh` の要素を含む）を候補に当てる。
4. **確認**: 道具は確認の要求（`ApprovalKind::CreateProject`、`facts.path`）を返し、候補を `call_id` で覚える。呼び直し（`Approved`）では、同じ `call_id` の候補を検査し直し、変わっていれば作らずに誤りを返す。非対話では、`Agent::approve` が `AgentConfig.project_parent` と候補の実の親を比べ（直下だけ）、一致すれば `execute_approved` を呼び、一致しなければ方針による拒否の文を返す。指定がなければ `never` で拒否する（確認の受け手は呼ばない）。作成はいつも `Approved` の呼び直しだけで起きるようにする。確認の画面は作るパスだけを事実に示し、答えのキーは `y`・`n`・`m` とする（`a`・`s` は受けない）。キーの文に、作ってそこへ切り替えることを書く。英文（要約、方針による拒否、利用者による拒否）は `approval/text.rs` に置く。
5. **作成**: 親の記述子から `mkdirat`（0755、umask に従う）で作り、リンクを辿らずに開いて、持ち主が自分であること、中が空であること、記述子から得た実のパスが候補と一致することを確かめる。雛形の `main.bnt` を `O_CREAT|O_EXCL|O_NOFOLLOW`（0644）で書く。中身は 01-03 の決定のとおり（末尾に改行を一つ）。途中で失敗したら、作ったものを消して誤りを返す。書いた後の検査は行わない（01-03）。
6. **切り替え**（`Agent` が、道具の結果を記録する前に、次の順で行う）: `ProjectDirChanged` のイベントを出して記録する → `ToolContext` を切り替える（プロジェクトの `ScopeRoot`、読む規則の `rebased`、読んだ版の一括の消去、`RunContext.locations.project`、`Processor::set_project`）→ 巻き戻しの記録を `restart` で新しい根から始め直す → `confirmed_effects`・`check_failures`・`check_limit` を消す（`session_domains` は残す）→ 環境の情報を `with_project` で変え、`environment_changes` を設定する（次の依頼の前の `<environment>` で伝える）。道具の結果の文には、新しい絶対パスと、前のプロジェクトを読み書きできなくなったことを書く。
7. **巻き戻しの記録**: `CheckpointRecorder::new` は、記録の中の最後の `ProjectDirChanged` より前の時点を捨てる（番号と世代の最大は保つ）。再開した後に、前のプロジェクトの時点を新しい根に戻さないためである。
8. **画面と出力**: 確認の題と答えのキーを足す。対話の画面の働き手は、イベントで `status.project` を更新し、切り替えの後の `/new` が新しいプロジェクトで始まるようにする（01-03）。画面と `exec` の出力に、プロジェクトが変わったことを一行で示す。入力の履歴のファイルは起動のときのプロジェクトのままでよい。
9. **期待のファイル**: `tests/expected/tools-restricted.json` と `system-restricted.txt` を作り直し、差分が `create_project` の分だけであることを確かめる。
10. **結果の一覧**: `docs/plan/results/P4-safety.md` に、`create_project` の行（確かめるテストと環境）を加える。

## 触るファイル

- `crates/benitoiteh/src/`: `tools/`（`create_project.rs`（新）、`mod.rs`、`definitions.rs`、`text.rs`、`exclude.rs`、`ApprovalKind` を網羅する所）、`prompts/tools/create_project.md`（新）、`approval/`（`mod.rs`、`text.rs`）、`agent/`（`mod.rs`、`execution.rs`、`events.rs`）、`safe_fs/mod.rs`、`config/writable.rs`、`config/text.rs`、`rewind/mod.rs`、`benitoite/query.rs`、`benitoite/mod.rs`、`context/environment.rs`、`cli/`（`args.rs`、`text.rs`、`exec.rs`、`exec/tests.rs`）、`ui/`（`mod.rs`、`text.rs`、`tui/confirmation.rs`、`tui/text.rs`、`tui/mod.rs`、`tui/worker.rs`）
- `crates/benitoiteh/tests/`: `expected/`、`args.rs`、`agent_loop/`、`tools_create_project.rs`（新）、`tui/interaction.rs`、`tui/commands.rs`、`rewind.rs`、`sandbox_exec_restricted.rs`、`tools_benitoite_test.rs`（網羅の `match`）
- `docs/plan/results/P4-safety.md`（行を足すだけ）

## テスト

スキル `test-audit` の作成時の関門に従い、既存のテストと重ねない。

- **道具（C で走る）**: 既存のディレクトリ・ファイル・リンク・宙に浮いたリンクを指す名前、親がない、規則に当たる候補（ホーム、`~/.config` の下、データの置き場所、一時ディレクトリの下、`.git` の要素、`.GIT`）、制御の文字、`~/x`、UTF-8 でないものでは、何も作らない。確認なしでは作らない。作ったディレクトリと `main.bnt` の中身と権限が正しい。
- **ループ（C）**: 承認、拒否、メッセージ付きの拒否、取り消し。確認と呼び直しの間に親のリンクを差し替えると作らない。切り替えの後に、相対のパスが新しいプロジェクトで解けること、前のプロジェクトが範囲の外になること、読んだ版と確認したエフェクトが消えること、次の依頼に `<project_directory>` と `<writable_roots>` の変化が入ること、`ProjectDirChanged` が道具の結果より前に記録されること。非対話で、指定がなければ方針による拒否になり確認の受け手が呼ばれないこと、直下なら作ること、孫と外は拒むこと。
- **巻き戻しと再開（C）**: 変更の前の時点を一覧に出さず、戻せないこと。再開すると新しいプロジェクトで開き、環境の変化を補うこと。
- **対話の画面（C）**: 題、パスの事実（改行が `\x0a`）、`a`・`s` を受けないこと、収まらないときは `y` を受けないこと、イベントの後の `/new` が新しいプロジェクトを使うこと。
- **`exec` の準備（C、`QueryLauncher::TestDirect`）**: `--project-parent` の受理と誤り（対話の形、通常モード、ないディレクトリ）。
- **雛形（C、処理系を直接起動）**: 雛形が本物の処理系（`BENITOITEH_TEST_BENITOITE`）の `check` と `run --allow-effects=Console.Write` を通り、`hello, world.` を出すこと。
- **サンドボックス（M/X。Codex の中では走らないので、オーケストレータが確かめる）**: 制限モードの `exec --project-parent` で作り、新しいプロジェクトで `benitoite_check` と `benitoite_run` が通ること。`File.Write` を確認したスクリプトが前のプロジェクトに書けないこと。`--continue` を新しいディレクトリから行うと選ばれ、前のディレクトリからは選ばれないこと。
- **期待のファイル**: 一致すること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、足した英文の全文（道具の説明、結果、誤り、承認、画面）、確認の画面の写し、切り替えの順序、加えた公開の型とシグネチャ、`P4-safety.md` に足した行、実行ファイルの大きさの変化を書く。
- オーケストレータが M/A/X で `scripts/check.sh` を走らせ、P4 の安全の性質のテストをすべて通し直す。
