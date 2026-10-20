# 共通の作業手順

このディレクトリには、個人の設定に依存しない手順を置く。担当への依頼では、作業ごとの範囲と完了条件を具体化する。

- [実装作業の共通依頼文](implementation-request.md): オーケストレータの確認手順と、担当へ渡す情報。
- [作業文書の雛形](change-plan-template.md): `docs/plan/tasks/` の作業を書くときの構成。
- [文書構成の対応表](repository-layout.md): Benitoite の構成との対応と、今回行わない移動の案。

実装者が守る手順は [作業の進め方](../plan/00-common.md)、検査の基準は [テスト](../design/07-quality/07-01-testing.md#完了の前に通す検査)を正とする。依頼文と雛形は、これらと `scripts/check.sh` の変更に合わせて保守する。
