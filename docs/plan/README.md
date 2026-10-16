# 実装プラン

本書は、試作のハーネス `benitoiteh` の実装の作業の分け方と、各作業の文書をまとめる。設計書（[docs/design/](../design/README.md)）と本書だけを読めば、作業を実装できる粒度で書く（[ロードマップ](../design/00-overview/00-03-roadmap.md)の「進め方」）。

- 対象の段階: P0（準備）と P1（核）。P2 より後の作業は、P1 を終えてから加える。
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
| [P0-05](tasks/P0-05-sandbox-linux.md) | Linux の補助のプロセスの試作（BH-043、BH-049） | P0-04 | 完了（P0-07 で作り直す） |
| [P0-07](tasks/P0-07-sandbox-linux-bwrap.md) | Linux の補助のプロセスを bubblewrap の形に作り直す（BH-049 の決定） | P0-05 | 未着手 |
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

P1 の作業は P0-04・P0-05（OS のサンドボックス）の結果に依らないので、P0-04・P0-05 が設計者の判断を待つ間に先に進めてよい。

P0-05 は、Apple の `container` の既定のカーネル（6.18.15）が Landlock を持たない（`CONFIG_SECURITY_LANDLOCK` が無効）ので、いったん止まった（2026-10-17）。2026-10-18 に、x86_64 の Linux のテストを設計者の常設の機械の podman で走らせる形にし（00-common の「検査」）、その機械で `sandbox_linux` を通して取り込んだ。Landlock で表せる範囲に狭めた形は、書ける場所に新しいファイルを作れない。2026-10-18 に、設計者が、守る場所の条件を Codex CLI の水準に改め、利用者が導入した bubblewrap でマウントを作る形に決めた（BH-049 を閉じた。設計書 08-04）。P0-07 で、この形に作り直す。

P0 の各作業の確かめの結果（BH-010、BH-043、BH-049、BH-062、BH-093）は、オーケストレータが設計書と [未決事項](../design/open-issues.md) に反映する。Skill の取り込み（P0 の内容の一つ）は、オーケストレータが `assets/benitoite/v0.0.6/` に済ませた（`SOURCE.md`）。

P1 で作らず後の段階に回すもの: ストリームの途中で読み取りの道具を先に始めること（01-01 の「道具の実行」の 5。P2 の後に効果を測る作業として加える）、圧縮（P4）、対話の確認（P4）。
