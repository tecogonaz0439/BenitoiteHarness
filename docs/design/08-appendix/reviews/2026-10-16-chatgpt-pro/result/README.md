# BenitoiteHarness 設計レビュー

**対象コミット:** `09dde24f92ec4768f9288e1b0448ced0f921ec08`  
**レビュー日:** 2026-10-16  
**対象の処理系:** Benitoite `v0.0.5`  
**対象リポジトリ:** [BenitoiteHarnessの指定コミット](https://github.com/tecogonaz0439/BenitoiteHarness/tree/09dde24f92ec4768f9288e1b0448ced0f921ec08)

## 結論

**Rustの雛形とP0の実現性検証には進めます。ただし、安全境界と履歴形式に関わる設計判断を固定しないまま、制限モード本体の実装へ進むことは勧めません。**

三層に分ける方向、Benitoiteを外部プロセスとして起動する構成、小さな道具集合、安定した文脈を作る方針は維持してよい設計です。一方、Linuxの負のパス制約と外部IPC、親の代理操作、承認と子孫の寿命、提供者の元の履歴を失わない形式を先に決める必要があります。

指摘は **33件** です。

| 重さ | 件数 | 番号 |
|---|---:|---|
| 実装の前に直すべき | 16 | 01〜16 |
| 実装の中で直せばよい | 15 | 17〜31 |
| 好みの範囲 | 2 | 32〜33 |

「実装の前」は、該当する安全境界やデータモデルを実装プランとして固定する前、という意味です。すべての作業を止める要求ではありません。後の機能に関するものは、その機能を実装する段階までに判断できれば足ります。

## 文書の構成

| ファイル | 内容 |
|---|---|
| [01-findings.md](01-findings.md) | **本体。重大度順の33件**。各指摘に場所・問題点・重さ・修正案・根拠・BH番号・合格条件を記載 |
| [02-security.md](02-security.md) | 三層の保証、22の攻撃経路、ダミーを使う試験手順、BH-043/049、macOS/Linuxの検証ゲート |
| [03-api-and-interpreter.md](03-api-and-interpreter.md) | BH-030/031の公式文書での確認、損失のないAPI履歴、圧縮、BH-022〜026の優先順位とJSON案 |
| [04-implementation-and-evaluation.md](04-implementation-and-evaluation.md) | 章間の整合、実装契約、P0〜P6の完了条件、道具の比較、サイズ・速度・メモリ、BHへの意見一覧 |
| [05-sources.md](05-sources.md) | 対象文書と一次資料のURL、適用範囲、確認できなかったこと |

最初に本体の指摘01〜16を読み、Linuxの方式決定には02、処理系への依頼には03、実装プランには04を使う構成です。補足内の番号は章・試験の番号であり、新しい指摘番号ではありません。

## 優先して直す事項

### 親の代理操作と承認画面

診断JSONのpathからソースを読む処理には、共通の読取制約との接続を明記すべきです。`Console.Write` だけで診断らしい文字列を出せるため、子の標準エラーを親の権限の根拠にはできません。ただし、00-02には親にも道具の規則を適用する総則があるので、**設計全体が無条件に秘密ファイルの読み取りを許すと断定する指摘ではありません**。具体手順を総則へ結び付ける不足として扱っています。指摘01、05、06を参照してください。[S-B01] [S-B02]

### Linuxの完全な制限モード

Landlockとseccompは有力ですが、書ける木の内側にある保護対象や、読ませない場所をどう実現するかはBH-049のまま残っています。外部Unix IPCはIPネットワークの有無と分けて制限する必要があります。最新のカーネル文書の機能と、実際の対象VMの能力も分けて判定してください。指摘02〜04、07、09、16と02-security.mdに方式・試験をまとめました。[S-K01] [S-K02] [S-K07]

### 提供者の履歴と圧縮

本文・思考・道具一覧を共通化するだけでなく、元のblock順、wire ID、opaque項目の由来を保存する必要があります。特に、DeepSeekの推論モードで `tools` を含む要求では、過去の推論文の再送が必要です。一律に削除する圧縮は修正すべきです。指摘13〜15と03-api-and-interpreter.mdを参照してください。[S-D01] [S-A01] [S-O03]

### 処理系への5件の要望

BH-022/023を最優先、BH-025を共通基盤、BH-026を多層防御、BH-024を配布・取得の改善として扱う案です。独立した大機能を追加するより、検査結果へ形式の版、対象の同一性、完全性、依存ソースの台帳を加えることを勧めます。JSONやCLIの例はすべて**新規提案**であり、v0.0.5に存在する機能として記載していません。

## 検証の範囲と確かさ

目的と設計原則を先に読み、README・AGENTS・未決事項と全22章を読み合わせました。Benitoite v0.0.5の公開された診断・標準API・CLI・エフェクトの記述を照合し、OSとAPIの論点は一次資料で確認しました。

一方、次のことは行っていません。

- 実装済み `benitoiteh` に対する侵入試験、ビルド、実行性能の測定、Apple containerやmacOSでのサンドボックス試験。
- 実APIへの要求送信、課金の発生、モデルの応答による相互接続試験。
- 非公開のBenitoite `docs/todo` の閲覧、先行ハーネス三件の全ソースの独立した再監査。

そのため、報告の「確認済み」は、文書の不整合または一次資料の記載を確認したという意味です。**具体的な攻撃経路でも、実装で再現したと書いてはいません。** 初期状態にhardlinkがある場合、対応する許可プログラムがある場合などは条件を明記しています。OS構成・APIモデルごとの実証が必要なものには、合格条件を付けました。

設計者が承認済みのサーバモードなしの構成や、事前検討との差分を、単なる不一致として再提案していません。また、承認された `File.Write` による範囲内の非 `.bnt` 書き込み、`Process.Run` で別のBenitoiteを動かす既述の例外を、新しい抜け道として数えていません。

## 全体の所見

大枠は実装に値する設計です。**P0の実験と骨組みは進め、BH-049・BH-043・外部IPC・子孫の寿命を制限モードの公開条件にする**ことを勧めます。親の共通ファイル操作と、損失のない提供者履歴を先に固定すれば、後続の機能を安全に積み上げやすくなります。8MiBや道具の細かな値は、その安全性を維持した候補どうしを測って決めるのがよいでしょう。

---

## この文書の外部出典

[S-A01]: https://platform.claude.com/docs/en/build-with-claude/preserved-thinking "Anthropic: Preserved thinking"
[S-B01]: https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/docs/design/02-impl/02-10-diagnostics.md "Benitoite v0.0.5: 診断エンジン"
[S-B02]: https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/crates/benitoite/skill/references/stdlib/IO.Console.md "Benitoite v0.0.5: IO.Console"
[S-D01]: https://api-docs.deepseek.com/guides/thinking_mode/ "DeepSeek: Thinking Mode"
[S-K01]: https://docs.kernel.org/userspace-api/landlock.html "Linuxカーネル: LandlockユーザーAPI"
[S-K02]: https://docs.kernel.org/userspace-api/seccomp_filter.html "Linuxカーネル: seccomp BPF"
[S-K07]: https://man7.org/linux/man-pages/man7/unix.7.html "Linux man-pages: unix(7)"
[S-O03]: https://developers.openai.com/api/docs/guides/function-calling "OpenAI: Function calling"

各出典の確認日・適用範囲は [出典一覧](05-sources.md) を参照してください。

