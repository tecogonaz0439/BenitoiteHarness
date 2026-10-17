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
| [P2-01](tasks/P2-01-process-run.md) | 子プロセスの実行と後始末（BH-090） | P0-07 | 完了 |
| [P2-02](tasks/P2-02-hardlinks.md) | サンドボックスの中のハードリンクの作成（BH-092） | P0-07 | 完了 |
| [P2-03](tasks/P2-03-linux-placeholders.md) | Linux の塞ぐ場所の錠と記録、入れ子の探索の時間（BH-095） | P2-01 | 完了 |
| [P2-04](tasks/P2-04-bwrap-checks.md) | bubblewrap の版と setuid の確かめ、特権の除去（BH-094 の一部） | P2-01 | 完了 |
| [P2-05](tasks/P2-05-processor-discovery.md) | 処理系の見つけ方と確かめ、問い合わせの起動 | P4-07、P4-08 | 未着手 |
| [P2-06](tasks/P2-06-skill-runtime.md) | Skill の文書を処理系から得る、`benitoite_explain`・`benitoite_reference` | P2-05 | 未着手 |
| [P2-07](tasks/P2-07-check-and-diagnostics.md) | 診断の受け渡し、`benitoite_check`、書いた後の検査、検査と修正の回数（BH-028 の測定） | P2-06 | 未着手 |
| [P2-08](tasks/P2-08-run-and-confirm.md) | 実行の前の検査の報告、制限モードの方針、`benitoite_run`、対話の確認 | P2-07 | 未着手 |
| [P2-09](tasks/P2-09-scratch.md) | 一時的なスクリプト `benitoite_scratch` | P2-08、P2-12 | 未着手 |
| [P2-10](tasks/P2-10-effects-confirmation.md) | 対話の画面の、エフェクトの確認 | P2-09、P2-12、P4-07 | 未着手 |
| [P2-11](tasks/P2-11-p2-completion.md) | P2 の完了の確かめ（制限モードの安全の性質のテストの一覧と結果） | P2-09、P2-10、P2-12 | 未着手 |
| [P2-12](tasks/P2-12-test-and-allow.md) | `benitoite_test` と、非対話の `--allow` | P2-08 | 未着手 |
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
| [P4-08](tasks/P4-08-slash-commands.md) | `/` で始まるコマンドと、P4 の安全の性質のテストの一覧 | P4-03、P4-04、P4-07、P4-09 | 未着手 |
| [P4-09](tasks/P4-09-safe-walk-search.md) | `glob`・`grep`・`read` を、リンクを辿らず上限のある読み方に移す | P4-03 | 完了 |
| [P4-10](tasks/P4-10-doctor-processor.md) | `doctor` の処理系の項目 | P2-06 | 未着手 |

P1 の作業は P0-04・P0-05（OS のサンドボックス）の結果に依らないので、P0-04・P0-05 が設計者の判断を待つ間に先に進めてよい。

P0-05 は、Apple の `container` の既定のカーネル（6.18.15）が Landlock を持たない（`CONFIG_SECURITY_LANDLOCK` が無効）ので、いったん止まった（2026-10-17）。2026-10-17 に、x86_64 の Linux のテストを設計者の常設の機械の podman で走らせる形にし（00-common の「検査」）、その機械で `sandbox_linux` を通して取り込んだ。Landlock で表せる範囲に狭めた形は、書ける場所に新しいファイルを作れない。2026-10-17 に、設計者が、守る場所の条件を Codex CLI の水準に改め、利用者が導入した bubblewrap でマウントを作る形に決めた（BH-049 を閉じた。設計書 08-04）。P0-07 で、この形に作り直した（2026-10-17。x86_64 の機械で `sandbox_linux` の 15 件が通った）。

P0 の各作業の確かめの結果（BH-010、BH-043、BH-049、BH-062、BH-093）は、オーケストレータが設計書と [未決事項](../design/open-issues.md) に反映する。Skill の取り込み（P0 の内容の一つ）は、オーケストレータが `assets/benitoite/v0.0.6/` に済ませた（`SOURCE.md`）。

2026-10-18 に、Benitoite の `v0.0.7`（2026-10-17 公開。コミット `63de89a2`）を受けて、処理系との連携の作業（group B: P2-05〜P2-12 と P4-10）を加えた。設計書は同日に `v0.0.7` に合わせて改めてある（04-01 ほか）。P2-05 は、`cli/exec.rs` の起動の流れと子プロセスの端末からの切り離し（`__session`）を P4-07 が変えるので、P4-07 の取り込みの後に始める。`create_project`（BH-006 の雛形が未決）は group B に含めない。テストに使う処理系は `v0.0.7` のタグからビルドしたもので、Linux の静的リンクの処理系（x86_64 と aarch64 の musl）は、P2-05 の前にオーケストレータが用意する（P2-05 の「前提」）。

2026-10-18 に、始める前の点検で、元の P2-08（`benitoite_run`・`benitoite_test`・非対話の確認）を、P2-08（報告、方針、`benitoite_run`、対話の確認）と P2-12（`benitoite_test`、非対話の `--allow`、続けての拒否での停止）に分けた。P2-09 は非対話の範囲と停止に `benitoite_scratch` を加えるので P2-12 の後に行い、作業の順は番号の順と違う（P2-08、P2-12、P2-09、P2-10、P2-11）。

P1 で作らず後の段階に回すもの: ストリームの途中で読み取りの道具を先に始めること（01-01 の「道具の実行」の 5。P2 の後に効果を測る作業として加える）、圧縮（P4）、対話の確認（P4）。
