# 出典一覧と確認の範囲

確認日: **2026-10-16**。対象設計とBenitoiteの参照は固定コミット／タグです。外部のAPI文書や `latest` / `main` は閲覧時点の内容であり、将来も同じである保証はありません。引用した機能が、対象OS・特定モデルで実際に利用できることまでは、この文書の閲覧だけでは確定しません。

本報告の `[S-K01]` 等は、各Markdown末尾の参照定義から一次資料へ直接移動できます。以下に用途と適用範囲をまとめます。

## 1. 対象設計

- [リポジトリの対象コミット](https://github.com/tecogonaz0439/BenitoiteHarness/tree/09dde24f92ec4768f9288e1b0448ced0f921ec08)
- [AGENTS.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/AGENTS.md) — 規則とレビューの観点
- [docs/design/README.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/README.md) — 目次と凡例
- [docs/design/open-issues.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/open-issues.md) — BHの一覧

以下の22章を読み合わせました。表の名称は内容を表す案内であり、原文見出しの引用ではありません。

| 章 | 内容 |
|---|---|
| [docs/design/00-overview/00-01-goals.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/00-overview/00-01-goals.md) | 目的と設計原則 |
| [docs/design/00-overview/00-02-architecture.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/00-overview/00-02-architecture.md) | 全体の構成 |
| [docs/design/00-overview/00-03-roadmap.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/00-overview/00-03-roadmap.md) | ロードマップ |
| [docs/design/00-overview/00-04-glossary.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/00-overview/00-04-glossary.md) | 用語 |
| [docs/design/01-agent/01-01-agent-loop.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/01-agent/01-01-agent-loop.md) | エージェントのループ |
| [docs/design/01-agent/01-02-modes.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/01-agent/01-02-modes.md) | モード |
| [docs/design/01-agent/01-03-tools.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/01-agent/01-03-tools.md) | 道具 |
| [docs/design/01-agent/01-04-context.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/01-agent/01-04-context.md) | 文脈の管理 |
| [docs/design/01-agent/01-05-sessions.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/01-agent/01-05-sessions.md) | セッション |
| [docs/design/02-providers/02-01-provider-layer.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/02-providers/02-01-provider-layer.md) | 提供者の層 |
| [docs/design/03-safety/03-01-security-model.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/03-safety/03-01-security-model.md) | セキュリティモデル |
| [docs/design/03-safety/03-02-sandbox.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/03-safety/03-02-sandbox.md) | OSのサンドボックス |
| [docs/design/03-safety/03-03-approval.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/03-safety/03-03-approval.md) | 利用者の確認 |
| [docs/design/04-benitoite/04-01-benitoite-integration.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/04-benitoite/04-01-benitoite-integration.md) | Benitoiteの処理系との連携 |
| [docs/design/05-interface/05-01-cli.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/05-interface/05-01-cli.md) | CLIと設定ファイル |
| [docs/design/05-interface/05-02-tui.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/05-interface/05-02-tui.md) | 対話の画面 |
| [docs/design/06-impl/06-01-crate-structure.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/06-impl/06-01-crate-structure.md) | クレートの構成 |
| [docs/design/06-impl/06-02-performance.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/06-impl/06-02-performance.md) | 性能 |
| [docs/design/07-quality/07-01-testing.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/07-quality/07-01-testing.md) | テスト |
| [docs/design/07-quality/07-02-evaluation.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/07-quality/07-02-evaluation.md) | 評価 |
| [docs/design/08-appendix/08-01-prior-art.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/08-appendix/08-01-prior-art.md) | 先行事例 |
| [docs/design/08-appendix/08-02-pre-study-mapping.md](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/08-appendix/08-02-pre-study-mapping.md) | 事前検討との対応 |

事前検討との違いは、00-01と08-02に書かれた承認済みの差分を前提としました。非公開TODOの内容は、公開設計に書かれた要点以上を推測していません。

## 2. OS・隔離・端末

### S-K01: Linuxカーネル: LandlockユーザーAPI

URL: <https://docs.kernel.org/userspace-api/landlock.html>

現行のユーザーAPI文書。正の権利、既存FD、ABI、Unix scope等を確認。ABI 9のpathname Unix、ABI 11のno_new_privs同時設定などの文書記載は、実際の対象VMの対応を証明しない。配布済みkernelとの対応版を推測していない。

### S-K02: Linuxカーネル: seccomp BPF

URL: <https://docs.kernel.org/userspace-api/seccomp_filter.html>

seccompの導入条件、継承、引数検査の制約を確認。汎用のサンドボックス全体を提供するという意味では使っていない。

### S-K03: Linux v6.12: Landlockのfile-openフック

URL: <https://github.com/torvalds/linux/blob/v6.12/security/landlock/fs.c>

v6.12へ固定。実行形式を開く際のLandlockのフックを照合。最新kernel全体の監査ではない。

### S-K04: Linux v6.12: do_open_execat

URL: <https://github.com/torvalds/linux/blob/v6.12/fs/exec.c>

v6.12へ固定。実行対象を開く経路の確認。

### S-K05: Linux v6.12: ELFのPT_INTERP処理

URL: <https://github.com/torvalds/linux/blob/v6.12/fs/binfmt_elf.c>

v6.12へ固定。PT_INTERPとローダを開く経路の確認。

### S-K06: Linux v6.12: Landlockファイルシステム自己試験

URL: <https://github.com/torvalds/linux/blob/v6.12/tools/testing/selftests/landlock/fs_test.c>

v6.12へ固定。link/rename等の自己試験の参照。今回このテストを実行したわけではない。

### S-K07: Linux man-pages: unix(7)

URL: <https://man7.org/linux/man-pages/man7/unix.7.html>

Linux man-pagesプロジェクトによるUnixソケットの仕様。stream/datagram、明示宛先、FD授受の根拠。

### S-K08: Linuxカーネル: Landlock内部設計

URL: <https://docs.kernel.org/6.2/security/landlock.html>

6.2版の内部設計。パス階層とinodeに関わるモデルの背景。現在ABIの機能一覧の代わりには使っていない。

### S-K09: bubblewrap: README / Sandbox security / Limitations

URL: <https://github.com/containers/bubblewrap/blob/main/README.md>

公式リポジトリのREADME。名前空間による構成、ポリシーやIPC・FDに残る注意点の根拠。main参照であり、先行事例の特定コミットを再監査した意味ではない。

### S-K10: Linux man-pages: ld.so(8)

URL: <https://man7.org/linux/man-pages/man8/ld.so.8.html>

Linux man-pagesの動的ローダの文書。直接起動と環境変数の扱いの根拠。

### S-M01: Apple: App Sandbox

URL: <https://developer.apple.com/documentation/security/app-sandbox>

AppleのApp Sandboxの説明。sandbox-execと同じものとして推薦するための資料ではない。

### S-M02: Apple dyld: 環境変数の読み取り

URL: <https://github.com/apple-oss-distributions/dyld/blob/main/dyld/DyldProcessConfig.cpp>

Appleが公開するdyldソース。DYLD系の環境入力処理の照合。main参照であり、特定のmacOSビルドの実挙動を断定していない。

### S-T01: xterm: Control Sequences

URL: <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html>

xtermの制御列仕様。端末への生出力を信頼された確認画面へ混ぜない根拠。全端末で同じ機能が有効とはしていない。


## 3. Benitoite v0.0.5

### S-B01: Benitoite v0.0.5: 診断エンジン

URL: <https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/docs/design/02-impl/02-10-diagnostics.md>

v0.0.5に固定。JSON診断と位置情報の形式を確認。

### S-B02: Benitoite v0.0.5: IO.Console

URL: <https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/crates/benitoite/skill/references/stdlib/IO.Console.md>

v0.0.5に固定。Console.Writeによる標準エラー出力の根拠。

### S-B03: Benitoite v0.0.5: IO.Process

URL: <https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/crates/benitoite/skill/references/stdlib/IO.Process.md>

v0.0.5に固定。Process.Command、environment、shell等の公開APIの根拠。

### S-B04: Benitoite v0.0.5: IO.File

URL: <https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/crates/benitoite/skill/references/stdlib/IO.File.md>

v0.0.5に固定。ファイルの書込APIの説明。hardlinkの実機試験を行ったという根拠ではない。

### S-B05: Benitoite v0.0.5: CLI

URL: <https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/docs/design/06-tooling/06-01-cli.md>

v0.0.5に固定。check/run/test/fmt/skill/package、テスト発見、終了状態の照合。

### S-B06: Benitoite v0.0.5: エフェクト

URL: <https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/docs/design/01-spec/01-07-effects.md>

v0.0.5に固定。エフェクトの規則とmainの前提。独自の未確認文法を使う回避例は作っていない。

### S-B07: Benitoite v0.0.5: SECURITY.md

URL: <https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/SECURITY.md>

v0.0.5に固定。処理系と外部の隔離の責務分担。


## 4. OpenAI

### S-O01: OpenAI: Responsesへの移行

URL: <https://developers.openai.com/api/docs/guides/migrate-to-responses>

OpenAI公式の現行移行ガイド。ResponsesとChat Completionsの保存・推論の扱いを確認。条件のある記載を全アカウントへ一般化しない。

### S-O02: OpenAI公式Python SDK: ResponseCreateParams

URL: <https://github.com/openai/openai-python/blob/main/src/openai/types/responses/response_create_params.py>

OpenAI公式SDKの生成された入力型。store/include/instructions等の入力の裏付け。main参照なので固定版のSDK試験の代わりではない。

### S-O03: OpenAI: Function calling

URL: <https://developers.openai.com/api/docs/guides/function-calling>

OpenAI公式の道具呼出しガイド。strict schema、call_id、出力列の保持の根拠。

### S-O04: OpenAI: Streaming Responses

URL: <https://developers.openai.com/api/docs/guides/streaming-responses>

OpenAI公式のストリーミングガイド。型付きイベントと完了状態の根拠。

### S-O05: OpenAI: Prompt caching

URL: <https://developers.openai.com/api/docs/guides/prompt-caching>

OpenAI公式のキャッシュガイド。キーと使用量の扱いを確認。料金・モデル別の最適値を測ったものではない。

### S-O06: OpenAI: Conversation state

URL: <https://developers.openai.com/api/docs/guides/conversation-state>

OpenAI公式の会話状態ガイド。ローカル履歴を使う構成の照合。


## 5. Anthropic

### S-A01: Anthropic: Preserved thinking

URL: <https://platform.claude.com/docs/en/build-with-claude/preserved-thinking>

Anthropic公式の現行preserved thinking文書。prefix/accountへの結び付きと変更時の扱い。適用条件は同文書に従う。

### S-A02: Anthropic: Streaming Messages

URL: <https://platform.claude.com/docs/en/build-with-claude/streaming>

Anthropic公式のストリーム仕様。block index、signature delta、error等の根拠。

### S-A03: Anthropic: Fine-grained tool streaming

URL: <https://platform.claude.com/docs/en/agents-and-tools/tool-use/fine-grained-tool-streaming>

Anthropic公式のfine-grained tool streaming。eager入力と不完全JSONの扱いを確認。

### S-A04: Anthropic: Tool reference

URL: <https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-reference>

Anthropic公式の道具仕様。strict等の入力の確認。全互換APIの対応を証明するものではない。

### S-A05: Anthropic: Prompt caching

URL: <https://platform.claude.com/docs/en/build-with-claude/prompt-caching>

Anthropic公式のキャッシュ文書。区切り、block探索、usage、期間の扱い。実際の費用低減は未測定。

### S-A06: Anthropic: Compaction

URL: <https://platform.claude.com/docs/en/build-with-claude/compaction>

Anthropic公式のサーバ側compaction。将来の比較候補であり、試作へ必須とするものではない。


## 6. DeepSeek

### S-D01: DeepSeek: Thinking Mode

URL: <https://api-docs.deepseek.com/guides/thinking_mode/>

DeepSeek公式の現行Thinking Mode。toolsを送る場合の過去reasoning_contentの再送と欠落時のエラーを確認。

### S-D02: DeepSeek: Anthropic API互換

URL: <https://api-docs.deepseek.com/guides/anthropic_api/>

DeepSeek公式のAnthropic互換文書。無視・未対応の欄等を確認。本家の機能を無条件に継承できない根拠。


## 7. Rustの依存候補

### S-R01: rustix 1.1.5: unshare

URL: <https://docs.rs/rustix/latest/rustix/thread/fn.unshare.html>

閲覧時の表示版はrustix 1.1.5。unshareの非推奨理由を確認。将来のlatestや別版に同じAPIがあるとは仮定しない。

### S-R02: rustix 1.1.5: unshare_unsafe

URL: <https://docs.rs/rustix/latest/rustix/thread/fn.unshare_unsafe.html>

同じ表示版のunshare_unsafe。汎用APIの安全性条件を確認。すべての名前空間処理が必ず自前unsafeを要求するとはしていない。

### S-R03: ureq 3.4.2: ConfigBuilderのタイムアウト

URL: <https://docs.rs/ureq/latest/ureq/config/struct.ConfigBuilder.html>

閲覧時の表示版はureq 3.4.2。各timeoutの区間を確認。別スレッドからの取消を実証したものではない。

## 8. 未確認のまま残したもの

| 項目 | 今回の限界 | 次の確認 |
|---|---|---|
| sandbox-execの将来 | 廃止の時期・同等な公式代替を確定する一次資料は得られていない | 対象macOSごとの起動・子孫・Mach・file-mapの実機試験 |
| Apple container内のLandlock/seccomp | 利用可能性を実行で確認していない | 対象kernel/LSM/ABI、user namespace、seccomp導入の観測 |
| 既存hardlinkやFD経由exec | 設計上の経路と一次資料を分けたが、対象環境では未再現 | ダミーfixtureによる試験。成功・拒否・未対応を分けて記録 |
| 最新APIの実接続 | 公式文書まで。課金を伴う通信は行っていない | 対象モデル・endpoint・資格情報を固定した承認済み試験 |
| ビルドサイズ・起動・RAM | 値を測っていない | 同一featuresと固定入力でP0/P5に測定 |
| 非公開TODO | ユーザーが指定したとおり閲覧不可 | 公開設計にある要点を前提とする。必要なら設計者側で照合 |
| 先行ハーネスの詳細 | 08-01の固定コミットの調査結果を参照。三件の全ソースの独立監査ではない | 再利用する実装が決まった時点で、その固定ソースを個別に監査 |
| 日本語の記述用Skill | AGENTSで言及されたSkillの本文は、この実行環境で取得できなかった | 技術指摘と意味の曖昧さに限定し、独自の文章好みを指摘しない |

検証できなかったものを理由に全体の設計を否定するのではなく、満たすべき契約と試験を本報告に分けて示しました。合格条件として列挙した項目は、すべて今後の試験案です。
