# 実装の共通依頼文（BH-054〜057 ACP 対応）

[docs/workflow/implementation-request.md](../workflow/implementation-request.md) を、本改造（[plan.md](plan.md) の P5-53〜P5-56）向けに具体化した共通依頼文である。オーケストレータが Codex CLI に各作業を依頼するときは、冒頭で作業 ID と対象節を指定し、以下の規則をそのまま適用する。

---

## 依頼の雛形

```text
docs/2026-11-21-BH-054_055_056_057-acp-support/plan.md の作業 <作業 ID>（または docs/plan/tasks/<作業ファイル>.md）を実装してください。

作業を始める前に、次を読んでください。
- AGENTS.md
- docs/plan/00-common.md
- docs/plan/10-interfaces.md
- docs/2026-11-21-BH-054_055_056_057-acp-support/decisions.md
- docs/2026-11-21-BH-054_055_056_057-acp-support/plan.md の <作業 ID> の節と、その「読む設計書」に挙げた章
- .claude/skills/test-audit/SKILL.md
```

## 守る契約

- `AGENTS.md` と [docs/plan/00-common.md](../plan/00-common.md) の規則（コーディング規約、クレートの構成、利用者に示す文の置き場所 `text.rs`、秘密と個人情報の禁止）を守る。
- 本番のコード（`src/`、テストを除く）では `unwrap`・`expect`・添字アクセス `[]` を使わない。`unsafe` は `sandbox/` と `process.rs` の既存の箇所以外には書かない。
- 新しい外部の依存クレートは加えない（`BH-056` の決定）。
- 実装プラン（[plan.md](plan.md)）の該当作業の「この作業が認める型の変更」に書いた範囲を超えて、ほかの作業と共有する型や公開シグネチャを勝手に変えない。変更が必要な場合は、理由を添えて報告し、止まる。
- 既存のテストを削除・変更・無効化（`#[ignore]` を付ける、条件を緩めるなど）しない。

## テストの作成

テストを加えるときは、`.claude/skills/test-audit/SKILL.md` の「1. 作成時の関門」を通す。

1. そのテストが守る **観測可能な契約** を一文で言えるようにする。
2. そのテストが捕まえる **現実的な退行** を言えるようにする。
3. 既存のテスト（同じファイルと、上位の統合テスト）を読み、すでに同じ契約が守られていないか確かめる。
4. テストのためだけに本番の可視性（`pub`）やフックを足さない。
5. 実装の該当箇所を一時的に壊して、加えたテストが失敗することを確かめ、すぐに戻す。

## 実行の環境の差

- Codex のサンドボックスの中では、macOS の `sandbox-exec`（入れ子の Seatbelt）と Linux のコンテナ（`container` / `podman`）は動かない。
- そのため、Codex の中では必ず `scripts/check.sh --inside-codex` を走らせる。
- `crates/benitoiteh/tests/sandbox_*.rs`（P5-56 で加える `sandbox_acp.rs` を含む）と Linux のテストは、Codex の作業が終わった後にオーケストレータが外で `scripts/check.sh`（macOS、aarch64 Linux、`BENITOITEH_X86_64_HOST` が指す x86_64 Linux）を走らせて確かめる。

## 完了の報告

作業を終えたら、コミットはせずに（オーケストレータが外の検査を通してからコミットする）、次の項目を報告する。

1. **変更したファイルの一覧と、それぞれの変更の要点**
2. **加えたテストの一覧と、それぞれが守る契約・壊して失敗を確かめた箇所**
3. **実行した検査のコマンドと結果**（`scripts/check.sh --inside-codex`、および P5-55 では `cargo build --release -p benitoiteh` の実行ファイルの大きさ）
4. **残った懸念や、外の環境（macOS の Seatbelt、x86_64 Linux、T3Code 実機）で特に確かめるべき点**
