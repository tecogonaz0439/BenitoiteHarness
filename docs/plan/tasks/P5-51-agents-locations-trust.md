# P5-51 信頼した置き場所を覚え、置き場所が増えたら尋ね直す

- 状態: 完了
- 依存: P5-50

## 目的

Linux のサンドボックスの中のコマンドは、まだない入れ子の `.agents`（`pkg/.agents/skills/x` など）を作れる。プロジェクトの信頼は探す範囲の根ごとに覚えるので、根を信頼した利用者が後で `pkg` から起動すると、LLM が作った Skill を尋ねずに読む（[P5-47 の記録](../results/P5-47-macos-checks.md)の「直さなかったレビューの指摘」）。信頼して覚えるときに、信頼を要した置き場所の一覧も覚え、置き場所が増えたら尋ね直す（設計書 03-01「プロジェクトの信頼」の「置き場所が増えたとき」）。

## 経緯

2026-11-05 に設計者が、置き場所の一覧を覚える案（レビューの推奨の (a)）に決め、設計と実装をオーケストレータ（Claude Code）に任せた。ほかの案は、Linux で入れ子の `.agents` を作らせないもの（マウントでは前もって表せない。設計書 03-01「Linux での保証の範囲」）と、信頼をディレクトリごとにするもの（根の下のディレクトリから起動するたびに尋ねる）だった。

## 読む文書

- `AGENTS.md`、[作業の進め方](../00-common.md)。
- 設計書 03-01「プロジェクトの信頼」、03-02「.agents の守り」、01-04「利用者の Agent Skill」「Agent Plugins のプラグイン」、05-02「利用者の定義するコマンド」。
- `.claude/skills/test-audit/SKILL.md`。

## 作るもの

1. **置き場所の一覧**（`src/context/skills.rs`）: 信頼を要するかを見る `project_resources` が、範囲の中の各ディレクトリの `.agents/skills`・`.agents/plugins`（読む設定のとき）・`.agents/prompts` を、範囲の根からの相対のパスとして集める。`model` のあるプロジェクトの設定ファイルは、`exec` が足す。
2. **決定の保存**（`src/config/trust.rs`）: 決定の記録に `locations`（相対のパスの文字列の一覧）を加える。前の版の記録（`locations` がない）は、空の一覧として読む。UTF-8 でない置き場所は保存せず、いつも増えたものとして扱う。
3. **決定の使い方**（`src/cli/exec.rs` の `decide_trust`）
   - 保存した決定が信頼で、今の置き場所がすべて一覧にあれば、尋ねずに信頼する。
   - 一覧にない置き場所があれば、増えた置き場所を確認の要求に入れて尋ね直す。信頼して覚える答えなら、一覧に今の置き場所を足して保存する。
   - 保存した決定が信頼しないなら、今までどおり尋ねない。
   - 非対話の起動で増えた置き場所があれば、信頼せず、診断に増えた置き場所を示す。
4. **確認の画面**（`src/ui/tui/confirmation.rs`、`src/cli/tui.rs`）: 増えた置き場所があれば、その一覧を事実の一行として示す。パスは制御の文字を見える形に変えて示す。
5. **文書**: 設計書は本作業の前に書いた（03-01「プロジェクトの信頼」、03-02「.agents の守り」）。

## テスト

`test-audit` の作成時の関門に従う。

- `src/cli/exec/tests.rs` の `project_skills_and_model_follow_the_trust_decision` に、根を信頼して覚えた後に、入れ子の `pkg/.agents/skills` を作って `pkg` から起動すると尋ね直し、増えた置き場所が要求に入ること、答えを覚えた後は尋ねないこと、非対話の起動では信頼せずに増えた置き場所を示すことを足す。
- `src/config/trust/tests.rs` に、`locations` のない前の版の記録を読めることを足す。

## 完了の条件

- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが、外で `scripts/check.sh` の全体（`BENITOITEH_X86_64_HOST` を指定）を通す。
- 本書の「確かめた結果」に、行ったことと、外に残した検査を書く。

## 作ったもの

- `context::skills::project_resources` が、範囲の中の `.agents/skills`・`.agents/plugins`（読む設定のとき）・`.agents/prompts` を、範囲の根からの相対のパスとして集める。雛形の有無もここで見るので、`context::prompts::project_has_prompts`（テストのほかに呼ぶ所がなくなった）を取り除いた。
- `exec` は、`model` のあるプロジェクトの設定ファイルを置き場所に足す。設定ファイルは起動したディレクトリのものだけを読むので、`pkg` から起動すると `pkg/.benitoiteh/config.toml` になる。
- `config::trust::Decision`（信頼の真偽と置き場所の一覧）を保存する。記録の `locations` は省略でき、前の版の記録は空の一覧として読む。
- `decide_trust` は、保存した信頼の一覧にない置き場所を `TrustRequest::added` に入れて尋ね直す。信頼して覚える答えなら、前の一覧に今の置き場所を足して保存する。信頼しない答えなら、増えた置き場所を診断に示す。信頼しない決定には一覧を保存しない。
- 対話の画面の信頼の確認に、増えた置き場所の行（`New since trusted`）を加えた。

## 確かめた結果

| テスト | 足した行 |
|---|---|
| `src/cli/exec/tests.rs` の `project_skills_and_model_follow_the_trust_decision` | 一覧のない前の版の決定は尋ね直し、増えた置き場所に `.agents/skills` と `.benitoiteh/config.toml` が入る。覚えた後は、根からも、置き場所のない `pkg` からも尋ねない。`pkg/.agents/skills` を作ると尋ね直し、信頼しない答えでは読まずに診断で示す。信頼して覚えた後は、`pkg` からも根からも尋ねない |
| `src/config/trust/tests.rs` の `decisions_are_kept_per_root_and_rejected_when_tampered` | 置き場所の一覧を保存して読み戻せる。`locations` のない前の版の記録を空の一覧として読む |
| `src/ui/tui/tests.rs` の `trust_confirmation_maps_keys_and_rejects_early_or_hidden_trust` | 増えた置き場所の行を示し、置き場所の名前の制御の文字を端末に送らない |

壊した版で、次のように確かめた。

| 壊した箇所 | 結果 |
|---|---|
| 保存した決定が信頼なら、置き場所によらず信頼する | 一覧のない前の版の決定で尋ね直す行で失敗した |
| 信頼して覚えるときに、前の一覧に足さず今の置き場所だけを保存する | `pkg` で覚えた後に根から起動する行で失敗した（尋ね直した） |
| 増えた置き場所を、制御の文字を変えずに事実の行へ入れる | 失敗しなかった。画面を描く処理が、事実の行の制御の文字をもともと見える形に変えるためである。テストの行は、どちらの層で変えても、端末に制御の文字を送らないことを確かめる |

macOS で `scripts/check.sh --inside-codex` が終了状態 0 で通った。

全体の検査（`scripts/check.sh`、`BENITOITEH_X86_64_HOST` を指定。コミット `aae1782`）は終了状態 0 だった。テストの関数を足していないので、件数は P5-50 と同じである。

| 対象 | 結果 |
|---|---|
| macOS | 819 件が通り、失敗 0、無視 14。評価の道具の Python のテスト 118 件も通った |
| aarch64 の Linux（Apple の `container`） | 721 件が通り、失敗 0、無視 13（Landlock を要する `sandbox_*.rs` は飛ばす） |
| x86_64 の Linux（試験の機械の podman） | 815 件が通り、失敗 0、無視 16 |
