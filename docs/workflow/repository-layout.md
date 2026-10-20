# 文書構成の対応表

2026-10-21 の設計者の依頼により、Benitoite の最上位と `docs/` の構成を手本として、BenitoiteHarness の文書の置き場所を整理した。共通の運用手順を `docs/workflow/` に置き、非公開の運用情報は gitignore のディレクトリへ移した。設計の決定、未決事項の区分、実装プランの時期は変えない。

以下の「現在」は今回の整理後の場所を指す。「案」は設計者の判断を待つものであり、移動・改名の実施や時期を決めるものではない。

## 最上位の対応

| 手本のパス | このリポジトリの現在のパス | 扱い |
|---|---|---|
| `AGENTS.md` | `AGENTS.md` | 維持。任意の非公開の補足を先に読む規則、ディレクトリの表、スキルの運用を合わせた |
| `README.md` | `README.md` | 維持。文書の入口とセキュリティの報告方針へのリンクを加えた |
| `SECURITY.md` | `SECURITY.md`（新設） | 試作の安全の境界と、秘密を公開せずに報告する手順を案内する。個人の連絡先は記載しない |
| `CHANGELOG.md` | なし | 案。配布と版の運用を決めるときに新設を検討する。今回、過去の変更を遡って一覧にはしない |
| `LICENSE-APACHE`・`LICENSE-MIT` | 同名 | 維持 |
| `Cargo.toml`・`Cargo.lock`・`deny.toml`・`rust-toolchain.toml` | 同名 | 維持。コードとビルドの設定は変えない |
| `.gitignore` | 同名 | 非公開の運用文書とスキル、スキルへのリンクを除く規則を加えた。未決事項を移す判断は行わず、`docs/todo/` は新設しない |
| `.cargo/` | 同名 | 維持。ビルドと対象ごとの実行の設定は変えない |
| `.claude/skills/`・`.agents/skills/` | 同名 | 公開の `git-workflow` と `test-audit` の配置とリンクを維持する。非公開のスキルは下記の配置へ分ける |
| `assets/` | `assets/benitoite/` | 維持。リリースから取り込んだ文書と版の管理を続ける |
| `crates/`・`scripts/`・`tools/` | 同名 | 維持。プロジェクト固有の中身は処理系に合わせて移動しない |
| `formal/`・`fuzz/` | なし | 今回は新設しない。対応する実装と検査の範囲を追加する決定は行わない |
| `reference/`・`work/`・`target/` | 同名 | 維持。参照用・作業用・生成物の扱いを変えない |
| `private/AGENTS.md` | 同名（新設、gitignore） | 非公開の運用の補足を置く。公開文書からリンクしない |
| `private/skills/orchestration/` | 同名（移動先、gitignore） | 公開されていたスキルを移す。一般の依頼手順は `docs/workflow/` に置く |
| `.claude/skills/orchestration` | 同名（非公開のスキルへの相対のリンク、gitignore） | 公開のスキルの実体を置く構成から切り替える。`.agents/skills/` 側のリンクも gitignore で除く |

## docs の対応

| 手本のパス | このリポジトリの現在のパス | 扱い |
|---|---|---|
| `docs/design/` | `docs/design/` | 維持。章番号とファイル名を変えない |
| `docs/workflow/implementation-request.md` | 同名（新設） | Codex への共通依頼文、依頼前の準備、検査・レビュー・取り込みの一般の手順を置く |
| `docs/workflow/change-plan-template.md` | 同名（新設） | 試作の作業文書の雛形を置く。処理系の改造ごとの計画の構成をそのまま導入しない |
| `docs/archive/` | なし。過去の実装の文書は `docs/plan/`、外部レビューの写しは `docs/design/08-appendix/reviews/` | 案。試作の後の判断に合わせ、完了したプランと設計書の写しの保管場所を検討する。今回は移動しない |
| `docs/YYYY-MM-DD-…/` | `docs/plan/tasks/`・`docs/plan/results/` | 維持。試作の一続きのプランとして扱い、改造ごとのディレクトリへの分割は案にとどめる |
| `docs/todo/`（非公開） | `docs/design/open-issues.md`、公開後の所見は `docs/plan/tasks/P5-37-post-release-followups.md` | 維持。既存の区分と時期を変えない。未実装の内容と未決事項を移す先は、既存の規約どおり試作の後の判断と合わせて決める |
| `docs/reference/` | 独立の利用者向けリファレンスはなし。CLI の設計は `docs/design/05-interface/`、道具の使い方は `tools/*/README.md` | 案。利用者向けの文書を整えるときに新設を検討する。設計書の章をそのまま移さない |
| `docs/ja/` | なし。設計書と実装プランは日本語 | 案。英語の利用者向け文書を正とする場合に、その訳の置き場所として検討する |
| `docs/release-0.1.0/`（gitignore） | なし | 今回は新設しない。試作の配布物とリリースの範囲の判断を先取りしない |

文書の入口として [docs/README.md](../README.md)を加えた。実装プランの共通手順は [00-common.md](../plan/00-common.md)に保ち、オーケストレータへ渡す共通依頼文と役割を分ける。

## 今回行わない移動

`docs/plan/` のアーカイブ化、作業ごとの日付付きディレクトリへの分割、`docs/design/` の章の移動は行わない。これらは多数のリンクと、コードのコメントからの参照を変えるため、設計者が試作の後の扱いを決めてから、目次と参照をまとめて更新する作業として計画する。

`docs/todo/` の新設と未決事項の移動も行わない。未実装の内容をどこへ置くかは [設計の段階と試作の後](../../AGENTS.md#設計の段階と試作の後)の規約に従い、既存の `BH-nnn` と作業の状態を維持する。
