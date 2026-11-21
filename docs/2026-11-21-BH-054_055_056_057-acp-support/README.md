# BH-054〜057 ACP（Agent Client Protocol）と T3Code 連携の対応

## 目的と範囲

BenitoiteHarness（`benitoiteh`）に、エディタや外部クライアント（T3Code を主たる連携対象とする）から標準入出力（JSON-RPC 2.0 over stdio、改行区切り）で呼び出すための `benitoiteh acp [オプション]` サブコマンドを加える改造の記録である。扱う未決事項は [docs/design/open-issues.md](../design/open-issues.md) の `BH-054`・`BH-055`・`BH-056`・`BH-057` の 4 項目である。

- `BH-054`: ACP の対応で、道具とコマンドの実行を `benitoiteh` の側で行うか、クライアントの `fs/*`・`terminal/*` に委ねるか
- `BH-055`: T3Code の `provider-acp`（Custom ACP providers）に対する `benitoiteh acp` の実機検証項目
- `BH-056`: ACP のクレート（`agent-client-protocol` または `agent-client-protocol-schema`）を依存に加えるか、`serde_json` で必要なメッセージの型だけを定義するか
- `BH-057`: ACP の通信の中継（`[normal] network = "proxy"`）の未許可ドメインの確認と、`secrets.askpass` の扱い

設計の正は [docs/design/05-interface/05-03-acp.md](../design/05-interface/05-03-acp.md) および関連する各章（[00-01](../design/00-overview/00-01-goals.md)、[00-02](../design/00-overview/00-02-architecture.md)、[00-03](../design/00-overview/00-03-roadmap.md)、[00-04](../design/00-overview/00-04-glossary.md)、[03-02](../design/03-safety/03-02-sandbox.md)、[03-03](../design/03-safety/03-03-approval.md)、[05-01](../design/05-interface/05-01-cli.md)、[05-02](../design/05-interface/05-02-tui.md)、[06-01](../design/06-implementation/06-01-crate-structure.md)、[07-01](../design/07-quality/07-01-testing.md)）に反映済みであり、2 回の外部レビュー（ChatGPT Pro、[第 1 回・第 2 回記録](../design/08-appendix/reviews/2026-11-21-chatgpt-pro/README.md)）の指摘をすべて取り込んでいる。

このディレクトリには、改造の概要、設計の判断とその理由（代替案と却下理由）、実装プラン、および実装を担う Codex への共通依頼文をまとめる。

## 状態

2026-11-21 時点で、設計書の改訂、2 回の外部レビュー（指摘 01〜12 および残件 R01〜R06）の反映、および本ディレクトリ内の実装プラン（P5-53〜P5-56 の 4 作業）の作成を完了した。**Rust の本体コード（`crates/benitoiteh/`）の実装はまだ始めておらず、設計者が本ディレクトリのプラン文書を確認して了承した後に着手する。**

開始時の基準コミット（`gamou` ブランチの先頭）は `f051cb6`（P5-52 完了時点）であり、設計とプランの作業は `design/acp-support` ブランチで行っている。参照した外部の一次資料のコミットは次の 3 点に固定している。

| 対象 | リポジトリ | 固定したコミット |
|---|---|---|
| ACP の仕様と JSON スキーマ | `agentclientprotocol/agent-client-protocol` | `5ce539f03d2750e7f58a3ffdfab6919c2d04445c` |
| ACP の Rust SDK | `agentclientprotocol/rust-sdk` | `8ac65cf6b0b3371ae4d8231760cb91dad99e92ef` |
| T3Code | `pingdotgg/t3code` | `d5f8a7573a3817d8c20f7491e7672443355c4083` |

## 文書の一覧

| 文書 | 内容 |
|---|---|
| [README.md](README.md) | 本書。改造の目的と範囲、状態、文書と作業の一覧、残っていること |
| [decisions.md](decisions.md) | 設計の判断（D1〜D8）とその理由、検討した代替案と却下理由、外部レビュー（第 1 回・第 2 回）の指摘と処置の対応 |
| [plan.md](plan.md) | 改造の実装プラン本体。前提と依存、共通の関門、段とレビューの区切り、作業 P5-53〜P5-56 の詳細、検査の割り当て |
| [implementation-request.md](implementation-request.md) | 各作業（P5-53〜P5-56）を Codex に依頼するときに共通で渡す指示、契約、環境差、検査、完了報告の形式 |
| [../design/05-interface/05-03-acp.md](../design/05-interface/05-03-acp.md) | ACP（エディタとの連携）の設計書（正） |
| [../design/08-appendix/reviews/2026-11-21-chatgpt-pro/README.md](../design/08-appendix/reviews/2026-11-21-chatgpt-pro/README.md) | 2026-11-21 の ChatGPT Pro による外部レビュー（第 1 回・第 2 回）の依頼文と回答の記録 |

## 作業の一覧

全体の実装プラン（[docs/plan/README.md](../plan/README.md)）の通し番号に合わせ、本改造の作業を P5-53〜P5-56 の 4 作業に分ける。各作業の詳細は [plan.md](plan.md)（および [docs/plan/tasks/](../plan/tasks/) の各ファイル）に定める。

| ID | 作業 | 依存 | 段・レビュー | 状態 |
|---|---|---|---|---|
| [P5-53](plan.md#p5-53-子プロセスの環境からの-acp-クライアントの-mcp-資格情報の強制除外と通信の中継の終了と確認応答の直列化) | 子プロセスの環境からの ACP クライアントの MCP 資格情報（`T3_ACP_MCP_*`・`T3CODE_*`）の強制除外と、通信の中継（`ProxyRun`）の終了と確認応答の直列化 | P5-52 | 段 1（基盤と安全境界、G1） | 完了 |
| [P5-54](plan.md#p5-54-起動準備の方針の-5-分離呼び出し単位の一回限りのエフェクト許可終了理由の分類と画像バイト列の縮小入口) | 起動・準備の方針の 5 分離、呼び出し単位の一回限りのエフェクト許可、終了理由の分類と画像バイト列の縮小入口 | P5-53 | 段 1（基盤と安全境界、G1） | 完了 |
| [P5-55](plan.md#p5-55-benitoiteh-acp-のサブコマンドacp-版-1-の-json-rpc-層acpapprover終了手順) | `benitoiteh acp` のサブコマンド、ACP 版 1 の JSON-RPC 層、`AcpApprover`、取り消し・EOF・出力障害の 5 段の終了手順 | P5-54 | 段 2（ACP 本体と安全検証、G2） | 未着手 |
| [P5-56](plan.md#p5-56-benitoiteh-acp-の安全の性質の統合テストと-t3code-実機検証bh-055) | `benitoiteh acp` の安全の性質の統合テスト（本物のサンドボックスと処理系）と T3Code 実機検証（BH-055） | P5-55 | 段 2（ACP 本体と安全検証、G2） | 未着手 |

## 残っていること

1. 設計者が本ディレクトリのプラン（[README.md](README.md)、[decisions.md](decisions.md)、[plan.md](plan.md)、[implementation-request.md](implementation-request.md)）を確認し、実装への着手を了承すること。
2. 了承の後、P5-53 → P5-54（段 1、レビュー G1）→ P5-55 → P5-56（段 2、レビュー G2）の順に実装と検査（Codex 内の `scripts/check.sh --inside-codex` と、外の macOS・aarch64 Linux・x86_64 Linux での `scripts/check.sh` 全体）を進めること。
3. P5-56 で実機の T3Code に `benitoiteh acp` を登録して `BH-055` の 4 項目を確かめ、結果を `docs/plan/results/P5-56-acp.md` と [docs/design/open-issues.md](../design/open-issues.md) に反映して `BH-055` を閉じること。
