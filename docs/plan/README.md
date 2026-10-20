# 実装プラン

本書は、試作のハーネス `benitoiteh` の実装の作業の分け方と、各作業の文書をまとめる。設計書（[docs/design/](../design/README.md)）と本書だけを読めば、作業を実装できる粒度で書く（[ロードマップ](../design/00-overview/00-03-roadmap.md)の「進め方」）。

- 対象の段階: P0（準備）、P1（核）と、P2 からの作業。2026-10-17 から、Benitoite の `v0.0.7` を待つ間に、`v0.0.7` に依らない作業（P2 のサンドボックスの項目、P3、P4 の大部分）を先に加える（[ロードマップ](../design/00-overview/00-03-roadmap.md)の「段階」）。
- 実装する者: 作業ごとに Codex CLI（GPT-6.1-Sol、推論の度合い high）。作業の進め方は [00-common.md](00-common.md) に従う。
- 作業の間で共有する型と、モジュールの持ち主は [10-interfaces.md](10-interfaces.md) で定める。

## 作業の一覧

状態は、未着手・作業中・確認中・完了のどれかである。依存の欄の作業がすべて完了してから始める。

| ID | 作業 | 依存 | 状態 |
|---|---|---|---|
| [P0-01](tasks/P0-01-workspace.md) | ワークスペース、lint、検査の仕組み、空の実行ファイル | — | 完了 |
| [P0-02](tasks/P0-02-safe-fs.md) | 範囲の根から相対に開くファイルの操作（BH-093） | P0-01 | 完了 |
| [P0-03](tasks/P0-03-http.md) | HTTP と TLS の層、ストリームの打ち切りと名前の解決の差し替え（BH-010） | P0-01 | 完了 |
| [P0-04](tasks/P0-04-sandbox-macos.md) | サンドボックスの方針の型と macOS の Seatbelt の試作 | P0-02 | 完了 |
| [P0-05](tasks/P0-05-sandbox-linux.md) | Linux の補助のプロセスの試作（BH-043、BH-049） | P0-04 | 完了（P0-07 で作り直した） |
| [P0-07](tasks/P0-07-sandbox-linux-bwrap.md) | Linux の補助のプロセスを bubblewrap の形に作り直す（BH-049 の決定） | P0-05 | 完了 |
| [P0-06](tasks/P0-06-size-probe.md) | 要る機能を一通り持つ構成の大きさの測定（BH-062） | P1-10（P0-05 を除いて行う） | 完了 |
| [P1-01](tasks/P1-01-provider-common.md) | 提供者の共通の形と SSE の読み取り | P0-03 | 完了 |
| [P1-02](tasks/P1-02-anthropic.md) | 模擬の提供者と `anthropic-messages` のアダプタ | P1-01 | 完了 |
| [P1-03](tasks/P1-03-openai-chat.md) | `openai-chat` のアダプタ | P1-02 | 完了 |
| [P1-04](tasks/P1-04-config.md) | 設定ファイル、引数、データの置き場所、書ける場所の検査 | P0-02 | 完了 |
| [P1-05](tasks/P1-05-session.md) | セッションの記録と一時ディレクトリ | P1-01、P1-04 | 完了 |
| [P1-06](tasks/P1-06-tools-read.md) | 道具の枠組みと、読む道具（read・glob・grep） | P1-04、P1-05 | 完了 |
| [P1-07](tasks/P1-07-tools-write.md) | 書く道具（write・edit） | P1-06 | 完了 |
| [P1-08](tasks/P1-08-context.md) | システムプロンプト、道具の定義、環境の情報 | P1-07 | 完了 |
| [P1-09](tasks/P1-09-agent-loop.md) | エージェントのループ | P1-03、P1-08 | 完了 |
| [P1-10](tasks/P1-10-exec.md) | `exec` の入口と P1 の完了の確かめ | P1-09 | 完了 |
| [P1-11](tasks/P1-11-skill-comment.md) | 埋め込んだ SKILL.md の先頭の注釈を除く | P1-08 | 完了 |
| [P1-12](tasks/P1-12-gemini-compat.md) | `openai-chat` の Gemini の OpenAI 互換の口への対応 | P1-03 | 完了 |
| [P2-01](tasks/P2-01-process-run.md) | 子プロセスの実行と後始末（BH-090） | P0-07 | 完了 |
| [P2-02](tasks/P2-02-hardlinks.md) | サンドボックスの中のハードリンクの作成（BH-092） | P0-07 | 完了 |
| [P2-03](tasks/P2-03-linux-placeholders.md) | Linux の塞ぐ場所の錠と記録、入れ子の探索の時間（BH-095） | P2-01 | 完了 |
| [P2-04](tasks/P2-04-bwrap-checks.md) | bubblewrap の版と setuid の確かめ、特権の除去（BH-094 の一部） | P2-01 | 完了 |
| [P2-05](tasks/P2-05-processor-discovery.md) | 処理系の見つけ方と確かめ、問い合わせの起動 | P4-07、P4-08 | 完了 |
| [P2-06](tasks/P2-06-skill-runtime.md) | Skill の文書を処理系から得る、`benitoite_explain`・`benitoite_reference` | P2-05 | 完了 |
| [P2-07](tasks/P2-07-check-and-diagnostics.md) | 診断の受け渡し、`benitoite_check`、書いた後の検査、検査と修正の回数（BH-028 の測定） | P2-06 | 完了 |
| [P2-08](tasks/P2-08-run-and-confirm.md) | 実行の前の検査の報告、制限モードの方針、`benitoite_run`、対話の確認 | P2-07 | 完了 |
| [P2-09](tasks/P2-09-scratch.md) | 一時的なスクリプト `benitoite_scratch` | P2-08、P2-12 | 完了 |
| [P2-10](tasks/P2-10-effects-confirmation.md) | 対話の画面の、エフェクトの確認 | P2-09、P2-12、P4-07 | 完了 |
| [P2-11](tasks/P2-11-p2-completion.md) | P2 の完了の確かめ（制限モードの安全の性質のテストの一覧と結果） | P2-09、P2-10、P2-12 | 完了 |
| [P2-12](tasks/P2-12-test-and-allow.md) | `benitoite_test` と、非対話の `--allow` | P2-08 | 完了 |
| [P3-01](tasks/P3-01-project-instructions.md) | プロジェクトの指示のファイル（BH-013 の決定） | P1-10 | 完了 |
| [P3-02](tasks/P3-02-normal-setup.md) | 通常モードの起動の準備（サンドボックスの方針と子プロセスの環境） | P2-04 | 完了 |
| [P3-03](tasks/P3-03-openai-responses.md) | `openai-responses` のアダプタ（BH-030） | P1-10 | 完了 |
| [P3-04](tasks/P3-04-approval-policies.md) | 通常モードの承認の方針 | P3-02 | 完了 |
| [P3-05](tasks/P3-05-bash.md) | 通常モードの道具 `bash` | P3-02、P3-04 | 完了 |
| [P4-01](tasks/P4-01-compaction.md) | 圧縮 | P3-05 | 完了 |
| [P4-02](tasks/P4-02-resume.md) | 再開、セッションの一覧、記録の期間と大きさの上限 | P4-01 | 完了 |
| [P4-03](tasks/P4-03-rewind.md) | 巻き戻しの記録と戻す処理 | P4-02 | 完了 |
| [P4-04](tasks/P4-04-web-fetch.md) | 道具 `web_fetch` | P4-01 | 完了 |
| [P4-05](tasks/P4-05-doctor.md) | `doctor`（処理系の確かめを除く） | P4-02 | 完了 |
| [P4-06](tasks/P4-06-tui-output.md) | 対話の画面の土台と出力 | P4-05 | 完了 |
| [P4-07](tasks/P4-07-interaction.md) | 対話の操作と確認の画面 | P4-04、P4-06 | 完了 |
| [P4-08](tasks/P4-08-slash-commands.md) | `/` で始まるコマンドと、P4 の安全の性質のテストの一覧 | P4-03、P4-04、P4-07、P4-09 | 完了 |
| [P4-09](tasks/P4-09-safe-walk-search.md) | `glob`・`grep`・`read` を、リンクを辿らず上限のある読み方に移す | P4-03 | 完了 |
| [P4-10](tasks/P4-10-doctor-processor.md) | `doctor` の処理系の項目 | P2-06 | 完了 |
| [P4-11](tasks/P4-11-create-project.md) | 制限モードの道具 `create_project`（雛形、確認、プロジェクトの切り替え、`--project-parent`） | P4-08、P2-12 | 完了 |
| [P4-12](tasks/P4-12-doctor-wording.md) | `doctor` と起動の、プロジェクトの場所の誤りの文言 | P4-10、P4-11 | 完了 |
| [P5-01](tasks/P5-01-skill-eval-import.md) | 評価の道具（`v0.0.8` のタグから写した `tools/skill-eval/`）を本リポジトリで動かし、LLM を呼ばない確かめを検査に加える | — | 完了 |
| [P5-02](tasks/P5-02-benitoiteh-harness.md) | 評価の道具に `benitoiteh` を加え、集計を読み取り、模擬の提供者で 11 課題を通す | P5-01 | 完了 |
| [P5-03](tasks/P5-03-records-and-cost.md) | 評価の道具の記録の欄、子の環境、時間の上限、費用の見積もりと上限、本物の提供者の項目 | P5-02 | 完了 |
| [P5-04](tasks/P5-04-dsh-and-claude-code.md) | 評価の道具に DeepSeek Harness と Claude Code の項目を整える（集計の読み取り、隔離、キーの渡し方、利用者が加えたスキルの確かめ） | P5-03 | 完了 |
| [P5-05](tasks/P5-05-performance.md) | 性能の測定の道具（06-02 の目標の各項目を測るテストと集める道具、`opt-level = "s"` との比べ） | P5-04 | 完了 |
| [P5-06](tasks/P5-06-evaluation-switches.md) | 評価で決める選択肢を切り替える口（`restricted.after_write_check`、`read.line_numbers`、評価の道具の `config` と比べる項目） | P5-04 | 完了 |
| [P5-07](tasks/P5-07-antigravity.md) | 評価の道具に Antigravity の項目を整える（`accept-edits` とフックで `benitoite` だけを許す、出力と会話の記録の読み取り、利用者の設定の確かめ） | P5-04 | 完了 |
| [P5-08](tasks/P5-08-s-tier-decisions.md) | S の段の決定の反映（`opt-level = "s"`、`anthropic-messages` の effort の既定を `low`） | P5-05 | 完了 |
| [P5-09](tasks/P5-09-user-skills.md) | 利用者の Agent Skill を読む（`~/.agents/skills` とプロジェクトの `.agents/skills`、Skill のディレクトリの読み取りと制限モードの `.bnt`、記録と表示） | P5-08 | 完了 |
| [P5-10](tasks/P5-10-chat-reasoning-effort.md) | `openai-chat` の `reasoning_effort`（機能の表）と、`total_tokens` だけに含まれる推論のトークンの数え方 | P5-09 | 完了 |
| [P5-11](tasks/P5-11-image-reading.md) | 画像のファイルを読む（`read` の画像の判定と寸法の上限、`image_input` の機能、三つのアダプタの画像の受け渡し、`objects/` への記録と再開、文脈の見積もり） | P5-10 | 完了 |
| [P5-12](tasks/P5-12-image-request-limits.md) | 送る画像の上限（古い画像を外す 12 MiB／20 枚の線）と、断られたときに画像を外して一度だけ送り直すこと、再開のときの画像の確かめ直し | P5-11 | 完了 |
| [P5-13](tasks/P5-13-image-followups.md) | 画像の送り直しを HTTP の 400・413（ストリームの前）に限る、再生の一覧の引き方、再開の読み直しの重複の除去、負荷で揺れるテストの直し | P5-12 | 完了 |
| [P5-14](tasks/P5-14-image-resize.md) | 上限を超える PNG と JPEG を縮める（`image` の PNG と JPEG、4,000 万画素の上限、向きの直し、一度に一つ） | P5-13 | 完了 |
| [P5-15](tasks/P5-15-image-helper.md) | 画像を縮める処理を補助のプロセス（`benitoiteh __image`）に移し、OS のサンドボックスとメモリーと時間の上限を掛ける | P5-14 | 完了 |
| [P5-18](tasks/P5-18-image-helper-hardening.md) | 画像の補助に書ける場所を持たせない、`fork`・ファイルの書き込み・コアのダンプを止める、出力の `flush`、古いセッションの削除の失敗で起動を止めない | P5-15 | 完了 |
| [P5-16](tasks/P5-16-session-tree.md) | セッションの木の記録（節点の番号、枝を移る記録、道での組み立て、形式の版 2）と、枝を移る操作、`/rewind` の会話を戻す操作を枝の移動にする | P5-18 | 完了 |
| [P5-17](tasks/P5-17-tree-screen.md) | 対話の画面の `/tree`（木の行の並び、選んで移る、確認、依頼の文を入力の欄に戻す） | P5-16 | 完了 |
| [P5-19](tasks/P5-19-fork.md) | `/fork`（今の道を新しいセッションに写す。参照する `spill/`・`objects/` も写す） | P5-17 | 完了 |
| [P5-20](tasks/P5-20-askpass.md) | 秘密の入力の段 2（サンドボックスの外のコマンドの `SSH_ASKPASS`・`GIT_ASKPASS`・`SUDO_ASKPASS` の仲介） | P5-18 | 完了 |
| [P5-21](tasks/P5-21-chatgpt-login.md) | Sign in with ChatGPT のログイン・保存・更新・ログアウト（ID トークンの検証、BH-114） | P5-18 | 完了 |
| [P5-22](tasks/P5-22-chatgpt-requests.md) | Sign in with ChatGPT の要求（`auth = "chatgpt"` の提供者） | P5-21 | 完了 |
| [P5-23](tasks/P5-23-plugin-skills.md) | Agent Plugins のプラグインの Skill を読む（MCP は読まない） | P5-18 | 完了 |
| [P5-24](tasks/P5-24-plugin-skill-followups.md) | プラグインの Skill の上限の残り（示す文の大きさ、候補の上限で飛ばした名前、上限のテスト） | P5-23 | 完了 |
| [P5-25](tasks/P5-25-temporary-cleanup-followups.md) | 一時ディレクトリとセッションの削除の残り（記述子の 32 段、権限と印を変えられた一時ディレクトリ、墓標の名前、足した書ける場所と一時ディレクトリの根、記録と錠のある一時ディレクトリだけを消す） | P5-24 | 完了 |
| [P5-26](tasks/P5-26-askpass-followups.md) | 秘密の入力の段 2 の残り（区切りの手前までの伏せ字、照合の値を消す、補助のディレクトリの `id`、掃除の失敗の文） | P5-25 | 完了 |
| [P5-27](tasks/P5-27-chatgpt-followups.md) | Sign in with ChatGPT の残り（戻り先の `poll`、後片付けの誤り、408・429、壊れた更新の応答、失敗したログインの取り消し、取り消しと更新の錠、誤りの文） | P5-26 | 完了 |
| [P5-28](tasks/P5-28-image-helper-and-rewind-memory.md) | 画像の補助の seccomp によるプロセスの作成の禁止と、枝を移るときの記録の写しの除去 | P5-27 | 完了 |
| [P5-29](tasks/P5-29-completion-check.md) | M の段の完了の前の確かめ（BH-011）の促し、利用者だけの切り替えの口、評価の集計と条件の順の入れ替え（`--interleave`） | P5-28、P5-06 | 完了 |
| [P5-30](tasks/P5-30-compaction-evaluation.md) | M の段の圧縮（BH-014）の Rust の設定、境と残す量、記録とイベントの欄、ループのテスト | P5-29 | 完了 |
| [P5-31](tasks/P5-31-normal-mode-tasks.md) | 通常モードの判定の実行器とそのテスト（macOS のみ） | P5-29 | 完了 |
| [P5-32](tasks/P5-32-compaction-runner.md) | 圧縮の評価の道具（組の選択、段の実行、上限と費用、模擬、判定の Seatbelt、`check.sh`） | P5-30 | 完了 |
| [P5-33](tasks/P5-33-compaction-tasks.md) | 圧縮の 2 課題、資料の生成、全段の判定と否定の対照 | P5-32 | 未着手 |
| [P5-34](tasks/P5-34-normal-mode-evaluation.md) | 通常モードの評価の道具と試しの 2 課題（準備、判定、環境、記録、模擬） | P5-31、P5-32 | 未着手 |
| [P5-35](tasks/P5-35-normal-mode-tasks.md) | 通常モードの残りの 8 課題を加え、計 10 課題を揃える | P5-34 | 未着手 |

2026-10-20 の始める前の点検を受け、元の P5-30 を P5-30・P5-32・P5-33、元の P5-31 を P5-31・P5-34・P5-35 に分けた。P5-29 の後は P5-30 と P5-31 を並べて進められる。P5-32 の後は P5-33 と、P5-31 が完了した P5-34 を並べて進められる。各作業の補足を、その文書の上の各節より優先する。

並べて作業するときは、P5-31・P5-32・P5-34 がそれぞれ `scripts/check.sh` に加える検査を突き合わせて取り込む。P5-32 の完了は実際の長い課題に依存させず、一時的な課題で道具を確かめ、P5-33 で 2 課題の検査を必須にする。

P1 の作業は P0-04・P0-05（OS のサンドボックス）の結果に依らないので、P0-04・P0-05 が設計者の判断を待つ間に先に進めてよい。

P0-05 は、Apple の `container` の既定のカーネル（6.18.15）が Landlock を持たない（`CONFIG_SECURITY_LANDLOCK` が無効）ので、いったん止まった（2026-10-17）。2026-10-17 に、x86_64 の Linux のテストを設計者の常設の機械の podman で走らせる形にし（00-common の「検査」）、その機械で `sandbox_linux` を通して取り込んだ。Landlock で表せる範囲に狭めた形は、書ける場所に新しいファイルを作れない。2026-10-17 に、設計者が、守る場所の条件を Codex CLI の水準に改め、利用者が導入した bubblewrap でマウントを作る形に決めた（BH-049 を閉じた。設計書 08-04）。P0-07 で、この形に作り直した（2026-10-17。x86_64 の機械で `sandbox_linux` の 15 件が通った）。

P0 の各作業の確かめの結果（BH-010、BH-043、BH-049、BH-062、BH-093）は、オーケストレータが設計書と [未決事項](../design/open-issues.md) に反映する。Skill の取り込み（P0 の内容の一つ）は、オーケストレータが `assets/benitoite/v0.0.6/` に済ませた（`SOURCE.md`）。

2026-10-18 に、Benitoite の `v0.0.7`（2026-10-17 公開。コミット `63de89a2`）を受けて、処理系との連携の作業（group B: P2-05〜P2-12 と P4-10）を加えた。設計書は同日に `v0.0.7` に合わせて改めてある（04-01 ほか）。P2-05 は、`cli/exec.rs` の起動の流れと子プロセスの端末からの切り離し（`__session`）を P4-07 が変えるので、P4-07 の取り込みの後に始める。`create_project`（BH-006 の雛形が未決）は group B に含めない。BH-006 は 2026-10-18 に設計者が決めたので、P4-11 として加えた。テストに使う処理系は `v0.0.7` のタグからビルドしたもので、Linux の静的リンクの処理系（x86_64 と aarch64 の musl）は、P2-05 の前にオーケストレータが用意する（P2-05 の「前提」）。

2026-10-18 に、始める前の点検で、元の P2-08（`benitoite_run`・`benitoite_test`・非対話の確認）を、P2-08（報告、方針、`benitoite_run`、対話の確認）と P2-12（`benitoite_test`、非対話の `--allow`、続けての拒否での停止）に分けた。P2-09 は非対話の範囲と停止に `benitoite_scratch` を加えるので P2-12 の後に行い、作業の順は番号の順と違う（P2-08、P2-12、P2-09、P2-10、P2-11）。

2026-10-18 に、設計者が P5（評価）の組み立てを決めた（設計書 07-02「比べる相手」「評価の規模と費用の上限」）。P5 の作業は、LLM を呼ばない準備（P5-01 評価の道具を動かす、P5-02 `benitoiteh` を評価の対象に加え模擬の提供者で通す、P5-03 記録と費用の上限と本物の提供者の項目、P5-04 DeepSeek Harness と Claude Code の項目、P5-05 性能の測定、P5-06 選択肢の切り替えの口）を先に行い、LLM を呼ぶ走行は、まとまりごとに設計者の了承を得てから行う。P5-02 以降の作業の文書は、前の作業を取り込んだ後に書く。評価の道具は、オーケストレータが `v0.0.8` のタグから `tools/skill-eval/` に写した（`SOURCE.md`）。

P1 で作らず後の段階に回すもの: ストリームの途中で読み取りの道具を先に始めること（01-01 の「道具の実行」の 5。P2 の後に効果を測る作業として加える）、圧縮（P4）、対話の確認（P4）。

P5-32 は、BH-014 の有料の走行の前に、既存の Benitoite と圧縮の課題の判定へ macOS の Seatbelt を掛ける。P5-33 の後の有料の走行は、P5-34 にこの保護を先送りしない。Codex 内では方針の組み立てと段の実行器までを検査し、実際の拒否と正しい解の判定はオーケストレータが外の macOS の全体の検査で確かめる。
