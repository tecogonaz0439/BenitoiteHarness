# P5-50 後回しにしたレビューの指摘のうち、直すものと限界として書くもの

- 状態: 着手
- 依存: P5-49

## 目的

2026-11-04 の安全のレビューで後回しにした指摘（[P5-47 の記録](../results/P5-47-macos-checks.md)の「直さなかったレビューの指摘」）のうち、次の四つを直し、二つを設計書の限界として書く。

| 指摘 | 扱い |
|---|---|
| macOS の gitdir の指す先 | 直す。根の `.git` ファイルの `gitdir:` が書ける場所の中を指すとき、macOS でも指す先に書かせない |
| 読ませないホームの場所の追加 | 直す。資格情報を平文のファイルに置く場所のうち、一次資料で確かめたものだけを加える |
| 制限モードの秘密の名前のリンク | 直す。読めないディレクトリは飛ばす。リンクの先がプロジェクトか一時の場所を含むときは、起動を止め、そのリンクを示す |
| 外部のエディタと通知のコマンド | 直す。提供者のキー（設定で参照したものを含む）を環境から除いて起動する |
| `.agents` のリンクでの起動の失敗 | 限界として書く。理由と、失敗の文で示すことを設計書に書く |
| `escalate` の環境 | 限界として書く。`PATH` のほかの探す変数を除かない理由を設計書に書く |

残りの二つのうち、秘密に見える環境変数の名前は P5-49 で置き換えた。Linux のまだない入れ子の `.agents` は P5-51 で扱う。

## 経緯

2026-11-05 に設計者が、後回しの指摘を推奨のとおりに扱うと決め、作業をオーケストレータ（Claude Code）に任せた。読ませないホームの場所は、確実に秘密を持つものだけを加えると決めた。

## 読む文書

- `AGENTS.md`、[作業の進め方](../00-common.md)。
- 設計書 03-02「Linux のファイルシステムの見え方」「.agents の守り」「読ませないホームの場所」「制限モードの方針」「通常モードの方針」、03-01「保証しないこと」、05-02「入力の操作」「通知」。
- `.claude/skills/test-audit/SKILL.md`。

## 作るもの

1. **macOS の gitdir の指す先**（`src/sandbox/`）
   - 根の `.git` ファイルの読み方（`gitdir:` の行、空と改行を含む値の拒否）を、Linux の `mounts.rs` から OS に共通の関数へ移す。
   - macOS の起動のときに、書ける場所の根ごとに `.git` ファイルの指す先の実のパスを求め、書ける場所の中にあれば、その下への書き込みを Seatbelt の規則で拒む。
2. **読ませないホームの場所**（`src/cli/mod.rs` の `DENIED_HOME`）に、次の場所を加える。`~/.cargo/credentials.toml`・`~/.cargo/credentials`・`~/.pypirc`・`~/.gem/credentials`・`~/.config/git/credentials`・`~/.config/gh/hosts.yml`・`~/.codex/auth.json`・`~/.claude/.credentials.json`・`~/.gemini/oauth_creds.json`。
3. **制限モードの秘密の名前の探索**（`src/sandbox/restricted.rs`）
   - 権限がなくて読めないディレクトリは、誤りにせず飛ばす。
   - 秘密の名前のリンクの先が、プロジェクトか一時の場所と同じか、その祖先であれば、そのリンクのパスを示す誤りで起動を止める。
4. **外部のエディタと通知のコマンド**（`src/ui/tui/`、`src/cli/tui.rs`、`src/cli/exec.rs`）: 起動のときに、`PROVIDER_KEY_ENV_VARS` と設定で参照したキーの変数を環境から除く。
5. **設計書**: 03-02 の各節と 05-02 に、上の決定と、限界として扱う二つを書く。

## テスト

`test-audit` の作成時の関門に従い、既存のテストに行を足す。

- macOS の gitdir: `tests/sandbox_macos.rs` の `writing_is_confined_and_protected_names_cannot_be_created_or_replaced` に、`gitdir:` の指す先に書けず、ほかの場所には書ける行を足す。
- 読ませないホームの場所: `tests/sandbox_exec_restricted.rs` の `redirected_xdg_keeps_default_home_paths_unreadable_from_normal_bash` の一覧に、加えた場所を足す。
- 秘密の名前の探索: `tests/sandbox_benitoite_run.rs` の `restricted_scripts_cannot_read_project_secret_names` の近くの行として、読めないディレクトリがあっても動くこと、プロジェクトの親を指す `.ssh` のリンクがあるとそのリンクを示して止まることを確かめる。
- エディタと通知: `tests/tui/pty.rs` の `terminal_confirmation_notification_and_external_editor_work_end_to_end` で、エディタと通知のコマンドが提供者のキーを受け取らないことを確かめる。

## 完了の条件

- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが、外で `scripts/check.sh` の全体（`BENITOITEH_X86_64_HOST` を指定）を通す。
- 本書の「確かめた結果」に、行ったことと、外に残した検査を書く。

## 作ったもの

- 根の `.git` ファイルの読み方を `sandbox::policy::root_gitdir` にまとめ、Linux の `mounts.rs` と macOS で使う。macOS は起動のときに指す先の実のパスを求め、書ける場所の中にあれば `(deny file-write* (subpath ...))` を、書ける場所を許す規則の後に足す。
- `DENIED_HOME` に、資格情報を平文のファイルに置く九つの場所を加えた。一次資料は設計書 03-02「読ませないホームの場所」に書いた。`~/.claude.json` は、資格情報を持つことを確かめられなかったので加えていない。
- 制限モードの秘密の名前の探索は、権限で読めないディレクトリを飛ばす。リンクの先がプロジェクトか一時の場所を含むときは、`SandboxError::SecretLink`（そのリンクのパスを示す）で止める。
- 起動の準備（`exec::Prepared`）に提供者のキーの名前の一覧を加え、対話の画面が外部のエディタと通知のコマンドを起動するときに環境から除く。
- 設計書 03-02 の「.agents の守り」「読ませないホームの場所」「制限モードの方針」「通常モードの方針」と、05-02 の「入力の操作」「通知」を改めた。

## 確かめた結果

| テスト | 足した行 |
|---|---|
| `tests/sandbox_macos.rs` の `writing_is_confined_and_protected_names_cannot_be_created_or_replaced` | `gitdir: metadata` のとき、`metadata` の中のファイルにも新しいファイルにも書けず、ほかの場所には書ける |
| `tests/sandbox_exec_restricted.rs` の `redirected_xdg_keeps_default_home_paths_unreadable_from_normal_bash` | 加えた九つの場所のうち八つ（`~/.cargo/credentials` を除く）を通常モードの `bash` が読めず、同じディレクトリの `~/.config/gh/config.yml` は読める |
| `tests/sandbox_benitoite_run.rs` の `restricted_scripts_cannot_read_project_secret_names` | 権限 000 のディレクトリがあってもスクリプトが動く。プロジェクトの親を指す `.aws` のリンクがあると、そのリンクを示して止まる |
| `tests/tui/pty.rs` の `terminal_confirmation_notification_and_external_editor_work_end_to_end` | 外部のエディタと通知のコマンドが、`OPENAI_API_KEY` と設定で参照したキーの変数を受け取らない |

壊した版で、次のように確かめた。

| 壊した箇所 | 結果 |
|---|---|
| macOS の gitdir の規則を足さない | `gitdir:` の行で失敗した（書き込みが通った） |
| 読めないディレクトリを誤りのままにする | 権限 000 の行で失敗した（`Permission denied` で起動しなかった） |
| 秘密の名前のリンクの先を確かめない | リンクの行で失敗した（別の誤り「A denied path must not contain a writable root」で止まり、リンクを示さなかった） |
| エディタの環境から提供者のキーを除かない | エディタの行で失敗した |
| 通知のコマンドの環境から提供者のキーを除かない | 通知の行で失敗した |

macOS で `scripts/check.sh --inside-codex` が終了状態 0 で通った。
