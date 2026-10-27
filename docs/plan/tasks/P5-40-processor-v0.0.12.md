# P5-40 処理系 v0.0.12 への更新

- 状態: 完了
- 依存: P5-39

## 目的

設計の前提の版、Skill の写し、システムプロンプトの対応の説明を確かめた版、テストと検査と評価に使う処理系を、`v0.0.7` から `v0.0.12` に改める。2026-10-27 に公開された `v0.0.12`（コミット `d0678ddb7e82da77715ea2e01ef974d06d41814b`）を以後使うという、同日の設計者の指示に従う。[Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)の「取り込む文書と版」にある、設計者の指示による取り込み直しと、対応の説明の見直しに当たる。

本物の LLM を呼ぶ評価は行わない。評価の前提と手順の版を改めても、過去の評価を `v0.0.12` で走らせ直したことにはしない。更新後の有料の評価は、使う提供者・モデル・回数・見込みの費用について設計者の了承を得て、別に行う。

### 背景と受け取った結果

次の実行ファイルと測定と依存の照合は、オーケストレータが 2026-10-27 に確かめ、作業の依頼時に渡したものである。作業文書の執筆者や実装者が再実行した結果として書かない。

- `v0.0.12` のアーカイブから、macOS と `aarch64-unknown-linux-musl`・`x86_64-unknown-linux-musl` の処理系をビルドした。macOS の `target/release/benitoite` は 32,091,952 バイトである。Linux は `target-linux/<対象>/release/benitoite` にある。実際に使う三つのパスは実装の依頼で示す。
- 手元の macOS（Apple Silicon）で、同じ条件の 30 回の中央値を測った。`check --report=json` は標準のモジュールを含めて 44 のソースを読む小さなスクリプトを使った。

  | 問い合わせ・検査 | v0.0.7 | v0.0.12 |
  |---|---|---|
  | `--version=json` | 5.8 ミリ秒 | 5.8 ミリ秒 |
  | `skill show --format=json SKILL.md` | 5.6 ミリ秒 | 6.5 ミリ秒 |
  | `check --report=json` | 13.6 ミリ秒 | 16.6 ミリ秒 |

  LLM の応答の時間と比べて小さいという BH-020 の判断は変わらない。2026-10-18 の測定とは別の測定として、日付、版、条件を添えて 04-01 に加える。古い測定は残す。
- `docs/plan/10-interfaces.md` の直接の依存のうち、処理系と共通の `ureq`、`rustls`、`rustls-graviola`、`graviola`、`rustix`、`serde`、`serde_json`、`getrandom`、`sha2`、`regex`、`signal-hook`、`libc`、`unicode-width`、`zeroize` は、すべて `v0.0.12` の `Cargo.lock` にある版と同じである。依存の版と機能は変えない。

## 読む文書

- `AGENTS.md`、[作業の進め方](../00-common.md)、[作業の間の約束](../10-interfaces.md)。特に依存、処理系、確認に関する節。
- [P5-39](P5-39-tls-advisories.md)。依存の原則に加えた例外と、取り込みの条件。
- [道具](../../design/01-agent/01-03-tools.md)の「書いた後の検査（制限モード）」「benitoite_run（制限モード）」「benitoite_test（制限モード）」「benitoite_scratch（制限モード）」「benitoite_explain と benitoite_reference」、[文脈の管理](../../design/01-agent/01-04-context.md)の「システムプロンプトの構成」「Benitoite の Skill の置き方」。
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「制限モードの方針」、[利用者の確認](../../design/03-safety/03-03-approval.md)の「制限モードの確認」「一時的なスクリプト」「非対話での扱い」。
- [Benitoite の処理系との連携](../../design/04-benitoite/04-01-benitoite-integration.md)全体、[テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」「LLM に見せる文の固定」「処理系の用意」「完了の前に通す検査」、[評価](../../design/07-quality/07-02-evaluation.md)の「評価の課題」と評価の条件を固定する箇所。
- `.claude/skills/test-audit/SKILL.md`。テストを加える・変える前に、作成時の関門に答える。
- 後述の「触るファイル」に挙げる既存のソース、プロンプト、テスト、評価と測定の道具の説明。
- 処理系の `v0.0.12` の `CHANGELOG.md` の 0.0.8〜0.0.12、`docs/reference/benitoite.md`、`crates/benitoite/skill/SKILL.md` と `references/inspection.md`。必要な標準ライブラリと例は同じ Skill の文書で確かめる。

処理系の資料は、オーケストレータが `git archive v0.0.12` を展開した 主の作業ツリーの `work/benitoite-v0.0.12/` を読む（実際のパスは依頼で示す）。以下の「アーカイブ」はこの場所を指す。処理系のリポジトリ（既定は `../Benitoite`）を使うときも、`git show v0.0.12:<パス>` などでタグの時点を読む。どちらも読み取り専用とし、処理系の作業ツリーからファイルを写さない。

## エフェクトと報告の形式の差

### 原子のエフェクトの棚卸し

執筆時にアーカイブのリファレンスと Skill を読み、`checkReport` と `testCheckReport` の `expanded` に出る原子の名前を、現在の `crates/benitoiteh/src/benitoite/effects.rs` の `Effect` と比べた。

| 分類 | 名前 | 今の Effect との差 |
|---|---|---|
| IO.All が展開するもの | `Clock.Time`、`Console.Read`、`Console.Write`、`File.Read`、`File.Write`、`Process.Environment`、`Process.Exit`、`Process.Run`、`Random.Generate`、`State` | 同じ 10 個 |
| IO.All に含まれない既存のもの | `Http.Connect`、`Http.Listen`、`Assert.Check` | 同じ 3 個 |
| 加えるもの | `Log` | 1 個追加。合計 14 個 |

`Assert.Check` はテストの契約と許す集合に残すが、`main` には書けない（E0415）。`IO.All` は展開前の総称であり、原子の名前として受け付けない。標準の `Failure.Fail[E]` を入口で処理しないと E0506 となり、`Fail` は `IO.All`、報告の `expanded`、`--allow-effects` の名前から除かれる。`Fail` や任意の利用者定義のエフェクトを `Effect` に加えない。

出典は、Benitoite の同じタグ・コミットの次のファイルと節である。

- [`docs/reference/benitoite.md`](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.12/docs/reference/benitoite.md)の「2. The command line」の「Inspection reports」「Effect limits before execution」と「10. Effects」。14 の原子の名前、`IO.All` の展開、`Fail` の除外を定める。
- [`crates/benitoite/skill/SKILL.md`](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.12/crates/benitoite/skill/SKILL.md)の「Before running」の表と「Checked values and local builders」。`Log` が `Console.Write` と `IO.All` から独立していること、既定の標準エラー出力と受け手のエフェクトを説明する。
- [`crates/benitoite/skill/references/inspection.md`](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.12/crates/benitoite/skill/references/inspection.md)の Effects オブジェクトと `--allow-effects` の説明。`Log` を原子の名前として扱う。
- [`CHANGELOG.md`](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.12/CHANGELOG.md)の 0.0.8「Checking and harness interfaces」と 0.0.10「Builders, logging, and checked values」。`Fail` の除外と `Log` の追加の経緯。

今の `report/decode.rs` の `Effects` は、`Effect::from_name` が知らない名前を不正な報告として扱う。このため `uses Log` を宣言するだけでも今は実行を断る。追加後も、ほかの知らない名前を空の集合や許可済みの集合として扱わない。

### Log の確認と許可の案

以下は【方針】であり、実装前にオーケストレータが点検する。既存の `Console.Write` の分類に揃えるが、名前と宣言の上限は別に保つ。

| 項目 | 案 | 根拠 |
|---|---|---|
| `benitoite_run` と `benitoite_test` の対話の確認 | 初回と、前に確認した範囲から `Log` が増えたときに確認する。画面に表示し、増えたかの判定にも数える | 03-03「制限モードの確認」。`Assert.Check` だけを除く今の分類を保つ |
| `benitoite_scratch` の確認なしの範囲 | 既定は変えない（エフェクトなし、`Console.Write`、`State`、`Process.Exit`）。`Log` を含む一時スクリプトは、確認なしの範囲に収まらないものとして今の規則どおり扱う。利用者が `restricted.scratch_auto_effects` に `Log` を書けば加えられる | 03-03「一時的なスクリプト」、01-03「benitoite_scratch（制限モード）」。確認なしで動く範囲は安全の性質に関わる既定なので、今回の更新では広げない |
| OS のサンドボックス | `Log` だけでは、プロジェクトへの書き込み、IP の通信、追加のプログラムの起動を許さない。標準エラー出力は既存の子プロセスの出力として受ける。確認なしの一時スクリプトは今の狭い読み取り範囲を保つ | 03-02「制限モードの方針」、03-03「一時的なスクリプト」、01-03「出力の切り詰め」 |
| 非対話の `--allow` | 原子の名前 `Log` を受け付け、報告の集合に含まれるときは `--allow-effects` にそのまま渡す。通常の `run`・`test` に一時スクリプトの既定を自動で引き継がせない | 03-03「非対話での扱い」、01-03 の各実行の道具。許す集合の作り方を変えず名前を増やす |
| 利用者の `restricted.scratch_auto_effects` | `Log` を受け付け、明示したときだけ確認なしの範囲に加える。プロジェクトの設定からは変えられない | 03-03「一時的なスクリプト」。利用者が選ぶ追加の範囲として扱う |
| 画面の説明 | `structured log events; independent of Console.Write and IO.All` | v0.0.12 の Skill「Before running」の表から字句を変えずに写す |

理由: v0.0.12 の Skill「Checked values and local builders」とリファレンス「Structured logging and source locations」「File and JSON log sinks」では、未処理の `Log` は既定で Info 以上を標準エラー出力に書く。ファイルなどへの出力は、利用者が定義した受け手が持つ `File.Write` などのエフェクトとして別に残る。`Log` を加えることで、残った `File.Write` の確認や OS の書き込みの制限を省かない。ログが無効な水準でも宣言の上限から `Log` を除かない。

この分類を、03-03 の上の三つの節、03-02「制限モードの方針」、01-03 の三つの実行の道具の該当箇所にも反映する。既存の見出しは改名しない。本作業は `Effect::Log` の追加と、それを受け付ける設定・許可・表示の変更を明示して許す。`10-interfaces.md` の `Effect` の説明も 14 個へ改めるが、既存の型のシグネチャは変えない。

### intervalAnalysis と文書の例のテスト

`v0.0.12` は、報告と各対象に `intervalAnalysis` を加えた。主な値は `basis: "localIntervals"` と `coverage: "all" | "some" | "none"` であり、型とエフェクトの検査・契約の取り出しが完全かを表す `complete` とは別である。`all` を受け入れの条件にせず、`all` や警告がないことを安全や実行の成功の証明にしない。未知の値、未対応の構文、分析の予算切れだけを理由に `uses` を広げさせない。出典は CHANGELOG の 0.0.12、リファレンス「Interval analysis coverage」、Skill「Before running」、`references/inspection.md` の「Local interval analysis」である。

【方針】この更新では、報告の受け入れの規則と、使わない追加の欄を読み飛ばす既存のデコーダの扱いを保つ。`intervalAnalysis` を実行の許可に使わず、LLM に分析範囲を読んだという説明もしない。局所の警告は通常の検査の診断で読み、Skill の例と分析の境界に従って説明する。システムプロンプトの対応の説明には、道具が報告を処理し、この分析の要約は返さないことを明記する。分析範囲の詳しい表示を新しい道具や引数として加えることは、この更新に含めない。理由: 追加の欄は報告形式の版と受け入れの規則を変えておらず、エフェクトの確認と OS の方針にも用いない。

【方針】`benitoite_test` と `benitoite_check(tests=true)` は `--doc` を付けない。報告も実行の結果も、今の通常のテストの形式 1 を使う。理由: `--doc` を付けると、文書の例を検査と実行の両方で同じ対象に加え、`testCheckReport`・`testResults` の形式 2 と `docExample`・`docSummary` を扱う必要がある。今の道具の契約は通常のテストであり、検査だけ・実行だけに例を加える変更は対象と確認の集合をずらす。Skill の手順の対応と道具の説明に、文書の例のテストには対応しないことを書く。出典は CHANGELOG の 0.0.9、Skill「Workflow for tests」、リファレンス「Documentation examples」と `references/inspection.md` の文書の例の説明である。

## 作るものと手順

1. アーカイブのタグとコミットを確かめ、上の棚卸しと報告の形式の差を読み直す。実装の依頼で示す macOS の処理系の `--version=json` も確かめる。形式の対応をリリース番号の大小で代用せず、要る `formats` の版を確かめる今の起動の規則を保つ。
2. アーカイブの `crates/benitoite/skill/` の全体を、`assets/benitoite/v0.0.12/skill/` にそのまま写す。`assets/benitoite/v0.0.12/SOURCE.md` に、元のリポジトリ、タグ、完全なコミット、実際に写した日、元と先のパス、ファイルの数と合計のバイト数を書く。執筆時にアーカイブで数えた値は 71 ファイル、441,389 バイトである。実装時にも数え、`SOURCE.md` 自身をこの集計に含めない。全ファイルの相対パスとバイト列をアーカイブと比べ、写した本文を手で直さない。
3. 指定された処理系で `skill show --format=json SKILL.md` を実行し、既存の Skill の受け入れの規則（名前、長さ、SHA-256 など）を満たすことを確かめる。JSON の `text` の UTF-8 のバイト列と、写しの `SKILL.md` の `{{benitoite-version}}` を `0.0.12` に置き換えたバイト列を比較する。改行と末尾の改行も含め、一致を結果に記録する。`skill install` は使わない。確認後に古い `assets/benitoite/v0.0.7/` 全体を消す。古い写しは git の履歴に残る。
4. 上の `Log` の案に従い、`Effect` の名前の読み取りと綴り、利用者の設定の検証、`--allow`、確認と TUI の説明を揃える。一時スクリプトの既定を保ち、OS の権限を `Log` によって広げない。`Assert.Check` の特例と、知らない名前を拒む規則を保つ。設計書の該当箇所と `10-interfaces.md` を同じ変更で揃える。
5. `crates/benitoiteh/prompts/` のシステムプロンプトと Benitoite の道具の説明を、写した `SKILL.md` と `references/inspection.md` に照らして見直す。特に `benitoite-mapping.md`、`mode-restricted.md`、`preamble-restricted.md`、`benitoite-index.md` と `tools/benitoite*.md` を読む。次の対応が食い違う箇所だけを直し、言語の説明を Skill と重ねて書かない。

   | 見直す変化 | 対応で確かめること |
   |---|---|
   | W0401・W0402・W0404 の局所の区間の警告 | 到達と引数の評価が成功した場合の失敗という条件を保つ。診断の説明は `benitoite_explain` と Skill の例へ案内する |
   | `intervalAnalysis` | 前節の方針どおり、報告の受け入れと分析範囲の説明を分ける。LLM が報告を直接読めるという対応にしない |
   | `test --doc` | 検査と実行の両方で使わないことを明記し、普通のテストとの対応を保つ |
   | ファイルの単位の `uses` | 関数の行だけで許可を決めず、報告の展開した宣言の上限を使うことと整合する |
   | `Log` | 独立した名前、受け手に残るエフェクト、一時スクリプトの既定の説明を揃える |
   | `Failure.toResult`、`block ... end block` | 新しい言語の手順を妨げる古い説明がないかを確かめる。専用の道具や言語の構文の読み取りは加えない |

   `docs/design/01-agent/01-04-context.md` に載せた対応の英文も、変更後のプロンプトと揃える。直した箇所と、見直して直さなかった箇所の理由を、実装後に本作業文書の「確かめた結果」に記録する。
6. `PROMPT_VERIFIED_VERSION` を `"0.0.12"` にする。テストと example の `language "0.0.7"`、正常な版の期待値、指定を求める文言、Skill の写しのパス、TUI の表の出典のコメントを更新する。`git grep -n '0\.0\.7'` で漏れを探す。新旧の版の不一致をわざと試すテストは、警告を出す版と出さない版の区別を保つ。固定の入力や過去の記録の版を、すべて機械的に置き換えない。
7. `examples/update_context_expected.rs` を使い、v0.0.12 の Skill と対応の説明を含むシステムプロンプトの期待値、変更した道具の定義の期待値を更新する。差分を読んで、Skill の変更と意図した対応の修正だけであることを確かめる。期待値の更新だけを正しさの根拠にしない。
8. 次節の分類に従って、設計書と運用の説明の現在の前提を改める。04-01 には、今回の実行ファイルの大きさと起動の時間を受け取った結果として追記する。古い測定と機能の追加の経緯は残す。
9. `10-interfaces.md` の依存の原則の版 `v0.0.6` を `v0.0.12` にする。P5-39 の例外の文は「P5-39 では勧告を解消するため処理系のリリースより先に `rustls` を更新したが、`v0.0.12` では処理系も同じ版である。」という経緯に改め、`rustls` の備考と `libc` の現在の照合先も揃える。表の版と manifest と lockfile は変えない。
10. 04-01「処理系への要望」の 7〜9 と CHANGELOG の 0.0.8〜0.0.12 を突き合わせる。【方針】今回の変化は既存の報告の追加情報と新しい言語の機能なので、新しい要望の番号は足さない。既存の要望の達成を示す記述は見つかっていないため、BH-028・BH-029・BH-105 を閉じない。関わる変化の要点（`Fail` の除外、`Log`、ファイルの単位の宣言、`--doc`、`intervalAnalysis`）は 04-01 の仕様の該当箇所に書く。要望の節への追記を見送った理由と、突き合わせた結果は本作業文書に残す。解消を主張するなら、CHANGELOG に記述がないだけで判断せず、一次資料か再現で確かめる。
11. 「テスト」「確かめること」を実施し、本作業文書の「確かめた結果」に、Codex の中で通した検査と外へ残した検査を分けて記録する。コミットしない。

### 版の参照の分類

`git grep -n 'v0\.0\.7'` と、接頭辞のない数も含む `git grep -n '0\.0\.7'` を変更の前後に走らせる。残した参照は、結果の節で次の分類ごとにファイルと節を挙げる。同じファイルに複数の分類がある場合は文や段落で分ける。検索の一致が 0 件になることを完了条件にしない。

| 分類 | 今回の扱い | 主な該当箇所 |
|---|---|---|
| 現在の設計の前提、対応を確かめた版、検査と今後の評価に使う版 | v0.0.12 に改める。引用先の内容と見出しをアーカイブで確かめてリンクも更新する | 設計書 README の冒頭と「Benitoite へのリンク」、00-01 の「最新」の記述、01-04 の固定のテストと対応の版、04-01 の前提と対応の版、07-01 の処理系と写し、07-02 の今後の評価の条件、10-interfaces の現在の契約、tools の使い方 |
| 現在の仕様を支える通常の設計書・事前検討へのリンク | 設計書 README の規則に合わせ v0.0.12 のタグへ更新する。リンク先の実在と根拠を確認する | 用語集、道具、モード、提供者、安全、CLI、依存の根拠にある通常の出典。依頼や測定を記録する文中のリンクは次の分類で扱う |
| 旧版での機能の追加、過去の日付の検証と測定、決定の経緯 | 元の版とタグを残す。新しい版で確かめた結果は別の文で加える | 04-01 の v0.0.7 の機能の列挙と「処理系への要望」の依頼の記録、2026-10-18 の測定、01-03 の動作を確かめた記録、03-03 の macOS の根拠、00-03 の進め方の変更、閉じた未決事項 |
| 過去の評価の結果と取り込み元 | 元の版・ハッシュ・タグを残す。今後の実行例だけ更新する | `tools/skill-eval/results/`、設計書の過去の評価の記録、07-02 の 2026-10-18 の同条件での比較の決定、SOURCE.md の v0.0.8 の取り込み元と v0.0.7 との差、警告の一覧などを取り出した当時の出典 |
| 古い作業と外部のレビューの記録 | 変更しない | `docs/plan/tasks/` の P5-39 以前、`docs/plan/results/` の既存の結果、`docs/plan/README.md` の過去の経緯の段落、外部のレビューの依頼・結果とそれに基づく出典 |
| ソース・テストの現在の前提と、旧版を意図した入力 | 実際の v0.0.12 を使う部分を更新する。旧版の警告や固定の模擬データは契約を読んで選ぶ | 処理系の指定を求める文、`language`、版の期待値、写しのパス。模擬 Skill や不一致を試す入力は、その目的を保つ |

07-02 の「評価の課題」は、今後 `v0.0.12` を使うという 2026-10-27 の指示を現在の決定として書き、評価の道具を `v0.0.8` から取り込んだ事実を残す。2026-10-18 に `v0.0.7` を採った理由は過去の経緯として日付を付けて残す。通常モードなどの今後の評価の条件も揃える。過去の比較と今回の更新後の評価を同じ結果として読める文にしない。

`tools/skill-eval/README.md`・`SOURCE.md` の `--benitoite` の例と現在の前提、`skill_eval.py` の処理系の指定を求める文、`tools/perf/README.md` の現在の検証対象を更新する。SOURCE.md に残す古い Skill のパスは、当時の出典として v0.0.7 のタグの URL にするか、履歴上のパスであることを明示し、消したローカルの写しを現在の参照先として案内しない。

### macOS の一時スクリプトの方針

`sandbox/restricted.rs` の `scratch_policy` が使う OS の場所とメタデータの組は、オーケストレータの `v0.0.7`・macOS 27.0.1 の実測を根拠にしている。Codex の中では `sandbox-exec` を重ねられないため、`v0.0.12` でも同じ組で足りるかは確かめられない。

このコメントの版は、内側の検査が通っただけでは書き換えない。オーケストレータが外の全体の検査で、同じ狭い範囲で一時スクリプトと時間帯の情報を読む処理などが動くことを確かめ、足りなければ検査を失敗として返す。確認なしに読める範囲を広げて通さない。確認後にだけ、確かめた日付・OS・版に合わせてコメントを更新し、その結果を本作業文書に記録する。

## 触るファイル

以下は、点検後の実装で変更する範囲である。作業文書の執筆時点では、本作業文書と `docs/plan/README.md` の P5-40 の行だけを変更する。

- `assets/benitoite/v0.0.12/skill/` と `assets/benitoite/v0.0.12/SOURCE.md`（新）、`assets/benitoite/v0.0.7/`（削除）。
- `crates/benitoiteh/src/benitoite/effects.rs`、`src/benitoite/mod.rs`、`src/config/mod.rs`、`src/ui/tui/text.rs`。パスの先頭の `crates/benitoiteh/` は、以下の `src/`・`tests/`・`examples/` でも同じである。
- `src/cli/doctor/processor.rs` と `src/cli/exec/tests.rs` のテストと指定を求める文、`src/ui/tui/tests.rs` の版の期待値。`src/sandbox/restricted.rs` は上の外での検証後のコメントだけ。
- `prompts/benitoite-mapping.md`、`prompts/mode-restricted.md`、`prompts/preamble-restricted.md`、`prompts/benitoite-index.md`、`prompts/tools/benitoite*.md`（見直した結果、対応が食い違う箇所だけ）。
- `examples/update_context_expected.rs`、`tests/context.rs`、`tests/expected/` のシステムプロンプトと道具の定義、`tests/support/processor.rs`、`tests/tui/interaction.rs`。
- `tests/benitoite_report.rs`、`tests/tools_benitoite_run.rs`、`tests/tools_benitoite_test.rs`、`tests/tools_benitoite_scratch.rs`、`tests/agent_loop/allow.rs`、`tests/args.rs`、`tests/config.rs`。既存の契約を拡張し、同じ契約を層ごとに重ねて加えない。
- `tests/sandbox_benitoite.rs`、`tests/sandbox_exec_restricted.rs`、`tests/sandbox_benitoite_run.rs`、`tests/sandbox_benitoite_test.rs`、`tests/sandbox_benitoite_scratch.rs`（版の期待値と、外で確かめる Log の実行・権限の契約）。
- `scripts/check.sh`（処理系の指定を求める版の文言だけ）。
- `docs/design/README.md`、`docs/design/00-overview/00-01-goals.md`、`00-04-glossary.md`、`01-agent/01-01-agent-loop.md`、`01-02-modes.md`、`01-03-tools.md`、`01-04-context.md`、`01-05-sessions.md`、`02-providers/02-01-provider-layer.md`、`03-safety/03-01-security-model.md`、`03-02-sandbox.md`、`03-03-approval.md`、`04-benitoite/04-01-benitoite-integration.md`、`05-interface/05-01-cli.md`、`06-impl/06-01-crate-structure.md`、`07-quality/07-01-testing.md`、`07-02-evaluation.md`、`08-appendix/08-02-pre-study-mapping.md`。最初の二つ以降の省略した先頭は `docs/design/` である。現在の前提・出典のリンクと、本作業で指定した Log・報告・プロンプトの対応に限る。
- `docs/plan/10-interfaces.md`（現在の版、写しのパス、Effect の名前の数、依存の原則と備考）、`tools/skill-eval/README.md`、`tools/skill-eval/SOURCE.md`、`tools/skill-eval/skill_eval.py`（指定を求める版の文言だけ）、`tools/perf/README.md`。
- 本作業文書の「確かめた結果」。本作業と一覧の状態の更新はオーケストレータが行う。

`Cargo.toml`・`Cargo.lock`・`deny.toml`、処理系のファイル、P5-39 以前の作業文書、既存の測定・評価・外部のレビューの記録は変更しない。本物の LLM を呼ばず、利用者の設定・状態、キーチェーン、トークン、`~/.ssh` を読まない。

## 確かめること

- 取り込んだ全ファイルがタグのアーカイブと一致し、`SOURCE.md` の版・コミット・日付・パス・ファイル数・バイト数が正しいこと。`skill show` の本文とのバイト列の一致を、実際に使う処理系で確かめたこと。
- 原子の名前が棚卸しした 14 個に揃い、`Log` を報告・許可・設定・表示で扱えること。`IO.All`・`Fail` と知らない名前の拒否を保つこと。
- `Log` を一時スクリプトの既定に加えず、利用者の設定に書いた場合だけ加えること。`Log` 単独で OS の権限を広げず、受け手に残った `File.Write` などは既存の確認と方針で別に扱うこと。一時スクリプトの既定が通常の `run`・`test` へ引き継がれないこと。
- `intervalAnalysis` の追加された実際の報告を受け付け、`coverage` を `complete` と取り違えないこと。`--doc` を検査にも実行にも足していないこと。
- プロンプトと道具の説明、設計書に載せた対応の英文、固定の期待値が一致すること。見直したが直さなかった箇所にも理由を記録したこと。
- `git grep` の一致を現在の前提と過去の事実に分類し、消した写しを現在の参照先にする箇所が残っていないこと。過去の作業・評価の版とタグを保っていること。
- `PROMPT_VERIFIED_VERSION`、検査の案内、テストと example、設計と評価と測定の現在の前提が `0.0.12` に揃っていること。依存の版と機能に差分がないこと。
- 04-01 に受け取った測定の日付・条件・値を追記し、古い測定と混ぜていないこと。要望への追記の判断と、macOS の未検証の範囲を結果に記録したこと。
- `BENITOITEH_TEST_BENITOITE` に指定された v0.0.12 の macOS の実行ファイルを設定して、`CARGO_NET_OFFLINE=true scripts/check.sh --inside-codex` が通ること。最後の `Skipped with --inside-codex:` に出た範囲をそのまま報告すること。

## テスト

版の置き換えだけを確かめる新しいテストは加えない。既存のテストの固定の入力と期待値を更新し、変更する契約は次の境界で確かめる。実装の前に既存の該当するテストを読み、作成時の関門の四つの答えを結果に記録する。

| 契約 | 主に確かめる境界と退行 |
|---|---|
| `Log` を持つ報告を使える | 本物の v0.0.12 と道具の実行器で、関数の `uses Log` とファイルの単位の `uses Log` を扱う。未知のエフェクトとして拒む退行を、修正前に同じ理由で失敗させる。通常のテストでも Log と Assert.Check の集合を落とさない |
| 確認を飛ばせない | ループの既存の許可のテストを拡張し、通常の run・test は許可なしの Log を断り、`--allow Log` で許す。一時スクリプトも既定では断り、利用者の設定に Log を加えた場合だけ確認なしで動く。その設定を run・test に引き継がない。確認後に増えた File.Write を Log と一緒に自動で許さない |
| 設定と画面の契約 | 設定と引数の既存の検証で Log を受け付け、TUI の既存の説明の照合で v0.0.12 の Skill の Log の行と一致させる。名前の変換だけを全層で繰り返し試さない |
| 報告の受け入れを保つ | 実際の報告の追加の欄で失敗しないことを確認する。既存の報告の表形式のテストに、`complete=true`・`coverage=some` を受け付け、`complete=false`・`coverage=all` は受け付けない場合を加える。壊れた報告と知らない名前を拒む既存の確かめを残す |
| 受け手の残ったエフェクトと OS の制限 | 外の sandbox の既存の道具のテストを拡張する。既定の Log は標準エラー出力に出るが、Log だけの許可で File.Write の受け手は動かない。別に File.Write を許しても、OS の書ける場所の外への出力は拒まれる。設定の文字列だけで拒否を確かめたことにしない |
| LLM に見せる文 | 既存の文脈と道具の定義の固定のテスト。更新した期待値と実際の差を読み、Skill と対応の説明の変更を確認する |

OS のサンドボックスを要する実行は `tests/sandbox_*.rs` に置き、Codex の中で直接の起動へ差し替えて通したことにしない。`--doc` の新しい結果の読み取り、局所の区間分析の実装、処理系内部の言語のテストは加えない。

## 完了条件

- v0.0.12 の Skill の写しと出典、実際の処理系との本文の一致、対応を点検したプロンプトと固定の期待値がそろっている。
- Log の名前・確認・設定・非対話の許可・TUI の説明と、設計書の分類が一致し、未知の名前を拒む規則と OS の権限の制限を保つ。
- 現在の版を v0.0.12 に揃え、過去の事実の版とタグを保ち、依存の版と機能を変えていない。
- `BENITOITEH_TEST_BENITOITE` に v0.0.12 の macOS の処理系を設定して、`scripts/check.sh --inside-codex` が通る。
- 本作業文書の「確かめた結果」に、実施した検査、プロンプトの見直し、版の参照の分類、要望への追記の判断、外へ残した検査が記録されている。

## 取り込みの条件

オーケストレータは、サンドボックスの外で v0.0.12 の三つの処理系を使い、`scripts/check.sh` 全体を走らせる。macOS の実行ファイルは `BENITOITEH_TEST_BENITOITE`、Linux の二つは `BENITOITEH_TEST_BENITOITE_AARCH64` と `BENITOITEH_TEST_BENITOITE_X86_64` に指定する。OS のサンドボックス、aarch64 のコンテナ、`BENITOITEH_X86_64_HOST` に指定した x86_64 の試験の機械での実行、最新のデータベースによる勧告の検査を含める。aarch64 の Landlock を要するテストは既存の扱いどおり走らせず、Landlock は x86_64 の試験環境で確かめる。

macOS の一時スクリプトの狭い読み取り範囲も、この全体の検査で確かめる。失敗したら結果を返して修正を依頼し、確認が済むまでコメントの根拠の版を更新しない。全体が通り、外で確かめた結果を記録してから `gamou` に取り込む。内側の完了だけで取り込みの条件を省かない。

## 確かめた結果

2026-10-27 に、点検の補足を先に反映して実装した。`Log` を確認なしの既定に加えず、利用者の `restricted.scratch_auto_effects` に書いた場合だけ加える。作業と一覧の状態はオーケストレータが更新するため、未着手の表記を保っている。

### Skill と処理系の照合

指定された macOS の処理系の `--version=json` は `version: "0.0.12"`、`schemaVersion: 1`、`kind: "version"` を返した。要る七つの形式はすべて版 1 を含み、`testCheckReport`・`testResults` は `[1,2]` であった。`build.commit` と `build.dirty` は `null` なので、コミットは自己申告から確定せず、リポジトリの `v0.0.12^{commit}` の `d0678ddb7e82da77715ea2e01ef974d06d41814b` と、依頼で指定されたタグのアーカイブを根拠とした。

アーカイブの Skill 全体の相対パスとバイト列をコピー先と比較した。71 ファイル、441,389 バイトで一致し、`SOURCE.md` に 2026-10-27 の取り込みとして記録した。`skill show --format=json SKILL.md` の本文は 40,107 バイト、SHA-256 は `6ac3fead480e53bb90ff95a8a7222bd845cda56fe4cce5b7e2e7a94bf8e9b6a1` である。名前、形式の版、長さ、ハッシュを照合し、写しの版の置換後のバイト列と、末尾の改行を含めて一致した。`skill install` は使っていない。

### プロンプトと道具の対応の点検

| 対象 | 結果と理由 |
|---|---|
| `benitoite-mapping.md` | `intervalAnalysis` の要約を返さないこと、警告の有無を安全と読み替えず、不完全な分析だけを理由にエフェクトを広げないこと、`--doc` を使わないことを追記した。新しい Skill が LLM に求める報告の読み方を、道具の実際の機能に合わせた |
| `tools/benitoite_check.md`・`benitoite_test.md` | 文書の例を検査と実行に含めないことを明記した。形式 2 を検査側だけ・実行側だけに足さず、形式 1 の対象の一致を保った |
| `tools/benitoite_scratch.md` | `Log` は既定に含まれず、利用者が設定で加えられることと、受け手の出力は別の許可を要することを追記した。既定の三つの名前は変えなかった |
| `mode-restricted.md`・`preamble-restricted.md` | 変更なし。シェルを持たない制限と作業の前置きは新しい言語の機能に依らず、矛盾がなかった |
| `benitoite-index.md`・参照と診断の説明の道具 | 変更なし。起動した処理系から Skill の本文と参照を得る手順と名前は同じであり、新しい構文やモジュールの説明は Skill に含まれる |
| `tools/benitoite_run.md` | 変更なし。関数のソースから許可を推定せず、処理系の報告の集合を使う既存の説明が、ファイルの単位の `uses` と `Log` にも当てはまる |
| `Failure.toResult`・`block ... end block` | 追加の対応なし。言語の構文をハーネスで読まず、処理系の検査・実行と Skill の説明を使う既存の道具で扱える |

01-04 の対応の説明もプロンプトに揃えた。局所の W0401・W0402・W0404 は、通常の検査の診断と `benitoite_explain`、Skill の例で説明する。`intervalAnalysis` を保持・表示する型や新しい道具は加えていない。文脈の期待値を example で更新し、差分は新しい Skill の内容、対応の説明、道具の説明の変更であることを確認した。道具の説明の本文はシステムプロンプトの道具の節に入り、道具の定義の JSON の形と引数には差分がない。

### テストの作成時の関門

| 問い | 答え |
|---|---|
| 守る観察可能な契約 | Log の報告を使えること、確認と非対話の許可を飛ばせないこと、設定で許した範囲が後の道具に残らないこと、受け手の書く場所を OS が限ること、報告の完全さと分析範囲を混同しないこと |
| 捕まえる退行 | `Effect::from_name` が Log を拒む、Log を既定で許す、Log の設定を run・test に引き継ぐ、Log と一緒に File.Write を許す、`coverage` で失敗した報告を受け入れる、画面から Log の説明が落ちる退行 |
| 既存のテストで足りない理由と境界 | 従来の入力には Log がなかったため、道具・許可・設定・画面の既存の表や入力に加えた。新しい二つの sandbox のテストは、設定後の実行と後続の道具の許可、受け手の出力に対する実際の OS の拒否を扱う。内側の確認の事実のテストだけでは、これらの実行時の性質を確かめられない |
| 本番にテスト専用の口が要るか | 要らない。既存の Fixture、道具の実行器、ループ、模擬の提供者、本物の処理系を使い、公開の範囲や注入の引数を増やしていない |

修正前に、`tools_benitoite_run::restricted_facts_cover_current_and_previously_confirmed_effects` に Log の行を加えて実行し、`the inspection report contains invalid or unknown effect names` で失敗することを確認した。修正後の結果は下の検査に記録する。OS のテストは内側で実行を代替せず外へ残した。ファイルへ出す受け手のスクリプトは、外での実行前の準備として同じ文字列を本物の `check --report=json` で静的に検査し、終了状態 0、診断なし、展開した上限が `File.Write,Log,Process.Exit` であることを確認した。

### 版の参照の分類の結果

`git grep -n 'v0\.0\.7'` と `git grep -n '0\.0\.7'` で変更の前後を調べた。

| 分類 | 更新・維持した箇所 |
|---|---|
| 現在の前提 | 設計書 README の版とリンクの規則、00-01 の最新の処理系、01-04 の固定のテストと対応の版、04-01 の前提と写し、07-01 のテストの処理系、07-02 の今後の評価、10-interfaces の版・パス・名前の数、評価と性能の道具の使い方を更新した |
| 現在の根拠のリンク | 通常の設計と事前検討の出典を v0.0.12 に更新し、アーカイブの同じパスの実在を確認した。過去の日付に確かめた事実の出典は維持した |
| 過去の機能・実測 | 01-03 の 2026-10-18 の検査と雛形、01-04 の旧 Skill の大きさ、03-02・03-03 の実測の根拠、04-01 の v0.0.7 の機能と検証と依頼の記録を残した。今回受け取った大きさと起動の時間は日付と条件を分けて追記した |
| 過去の評価と出典 | 07-02 の 2026-10-18 の比較の決定、SOURCE.md の v0.0.8 の取り込み元と v0.0.7 との差、警告の一覧の元、過去の模擬での確かめを残した。消した Skill のパスは当時のタグの URL にした |
| 記録として維持する文書 | P5-39 以前の作業文書、既存の results、ロードマップと作業一覧の過去の経緯、閉じた未決事項、外部のレビューの記録は変更していない |
| ソースとテスト | 現在の版の期待値、language の宣言、案内の文、写しのパスとコメントを更新した。doctor の不一致の入力 0.0.8 と、評価の模擬 Skill の 0.0.7 は目的を保って残した。sandbox/restricted.rs の実測のコメントと skill_eval.py の agent_parent の出典も旧版の事実として残した |

依存の manifest、lockfile、機能、勧告の扱いは変更していない。04-01「処理系への要望」の 7〜9 と CHANGELOG の 0.0.8〜0.0.12 を照合したが、今回の変化は言語の機能と報告の追加情報であり、新しい要望を足さなかった。既存の根の指定・モジュール検査の共有、表示のパス、秘密の入力の要望が解消したという記述も見つからず、BH-028・BH-029・BH-105 は閉じていない。

### 実施した検査と外へ残した検査

次の環境を指定し、`CARGO_NET_OFFLINE=true scripts/check.sh --inside-codex` は終了状態 0、`All checks passed.` で通った。

```text
BENITOITEH_TEST_BENITOITE=<主の作業ツリー>/work/benitoite-v0.0.12/target/release/benitoite
```

`processors`、`fmt`、ホストと Linux の二つの対象の `clippy`、Codex 内で走る `test`、LLM を呼ばない `skill-eval` と `perf-tools`、Linux の二つの対象のビルドとリンク、ライセンスの `deny`、三つの release の `size` の段を通った。修正前に失敗した Log の確認のテストも、この全体の検査で通った。最後に `cargo fmt --all -- --check` と `git diff --check` も通した。

`size` が出した値は macOS aarch64 が 5,706,560 バイト、aarch64 Linux musl が 6,234,912 バイト、x86_64 Linux musl が 7,502,576 バイトであった。これは今回の benitoiteh のビルドの値であり、「背景と受け取った結果」の処理系の大きさと時間とは別である。更新前と同条件の比較は行っておらず、改善を主張しない。

最後のスクリプトの一覧は次のとおりであった。

```text
Skipped with --inside-codex:
  deny: advisories (requires advisory database updates outside Codex)
  crates/benitoiteh/tests/sandbox_benitoite.rs
  crates/benitoiteh/tests/sandbox_benitoite_run.rs
  crates/benitoiteh/tests/sandbox_benitoite_scratch.rs
  crates/benitoiteh/tests/sandbox_benitoite_test.rs
  crates/benitoiteh/tests/sandbox_exec_restricted.rs
  crates/benitoiteh/tests/sandbox_linux.rs
  crates/benitoiteh/tests/sandbox_macos.rs
  crates/benitoiteh/tests/sandbox_read_image.rs
  crates/benitoiteh/tests/sandbox_rewind.rs
  crates/benitoiteh/tests/sandbox_skill_eval_judge.rs
  linux: container tests for aarch64-unknown-linux-musl
  linux: container tests for x86_64-unknown-linux-musl
  skill-eval: requires_sandbox tests (individual names above), test_judge_sandbox.RuntimeTests OS tests, all Benitoite/compaction self-test and dry-run passes (needs sandbox-exec outside Codex)
  skill-eval: benitoiteh-mock, benitoiteh-mock-completion-check and benitoiteh-mock-compact passes (needs loopback and the benitoiteh sandbox outside Codex)
  skill-eval: --self-test --suite normal, --dry-run --suite normal, benitoiteh-mock-normal; NormalTests.test_self_test_and_dry_run_normal_tasks, NormalTests.test_self_test_rejects_broken_controls, NormalTests.test_mock_normal_read_write_bash_for_all_tasks (needs the macOS sandbox outside Codex)
```

Python の検査で個別に飛ばされた名前も次に残す。いずれも本物の LLM を使う検査ではなく、OS のサンドボックスなどを要するために外へ残したものである。

- `test_judge_sandbox.RuntimeTests.test_home_secrets_are_denied_and_absent_from_stdout_and_error_reasons`
- `test_judge_sandbox.RuntimeTests.test_os_refuses_outside_writes_unlisted_exec_and_other_loopback`
- `test_judge_sandbox.RuntimeTests.test_other_ipv6_loopback_is_refused_before_connecting`
- `test_judge_sandbox.RuntimeTests.test_special_judgment_paths_work_with_the_real_policy_parser`
- `test_judge_sandbox.RuntimeTests.test_stdout_and_error_values_are_escaped_and_bounded_in_reasons`
- `test_judge_sandbox.RuntimeTests.test_test_command_is_also_sandboxed_and_shell_helpers_are_allowed`
- `test_skill_eval.AgyTests.test_agy_settings_and_versions_stop_evaluation_and_preserve_records`
- `test_skill_eval.AgyTests.test_fake_agy_trial_hooks_integrity_records_and_environment`
- `test_skill_eval.AgyTests.test_gate_coverage_compares_tool_counts_and_requires_transcript`
- `test_skill_eval.AgyTests.test_security_permission_failure_keeps_trial_result`
- `test_skill_eval.AgyTests.test_security_trial_metadata_and_protected_workspace_changes`
- `test_skill_eval.AgyTests.test_security_wrapper_requires_command_allow_and_created_names_are_private`
- `test_skill_eval.CompactionTaskTests.test_real_task_references_negative_controls_and_dry_run`
- `test_skill_eval.CompactionTaskTests.test_real_tasks_retain_multiple_read_results_and_distinguish_settings`
- `test_skill_eval.ExternalTrialTests.test_claude_cost_is_separate_and_synced_names_are_recorded`
- `test_skill_eval.ExternalTrialTests.test_dsh_missing_usage_stops_cost_budget_without_changing_verdict`
- `test_skill_eval.ExternalTrialTests.test_dsh_preparation_key_once_cost_and_records`
- `test_skill_eval.ExternalTrialTests.test_key_command_failures_hide_output_and_dry_run_skips_preflight`
- `test_skill_eval.ExternalTrialTests.test_key_leak_is_flagged_without_publishing_value`
- `test_skill_eval.ExternalTrialTests.test_leak_scan_covers_tmp_and_agents_home_without_following_links`
- `test_skill_eval.ExternalTrialTests.test_modified_processor_copy_is_rejected_and_judgment_uses_original`
- `test_skill_eval.ExternalTrialTests.test_subagent_and_workflow_cost_is_unknown_and_stops_budget`
- `test_skill_eval.ExternalTrialTests.test_timeout_overrides_completed_output_and_zero_exit`
- `test_skill_eval.ExternalTrialTests.test_unreadable_summary_keeps_wrapper_verdict_and_continues`
- `test_skill_eval.ExternalTrialTests.test_unscannable_file_does_not_discard_trial`
- `test_skill_eval.NormalTests.test_mock_normal_read_write_bash_for_all_tasks`
- `test_skill_eval.NormalTests.test_self_test_and_dry_run_normal_tasks`
- `test_skill_eval.NormalTests.test_self_test_rejects_broken_controls`
- `test_skill_eval.ProcessorTests.test_reference_effects_and_attempt_limit`
- `test_skill_eval.RecordsAndCostTests.test_benitoiteh_preserves_parent_home_for_execution_and_version_queries`
- `test_skill_eval.RecordsAndCostTests.test_judged_script_cannot_read_parent_session_or_provider_key`
- `test_skill_eval.RecordsAndCostTests.test_unreadable_session_fails_only_one_trial_and_retains_cost`
- `test_skill_eval.StagedTests.test_budget_before_first_launch_and_cost_only_unknown_and_runner_failure`
- `test_skill_eval.StagedTests.test_cases_arguments_json_and_named_negative_controls_and_dry_run`
- `test_skill_eval.StagedTests.test_judgment_does_not_inherit_parent_environment`
- `test_skill_eval.StagedTests.test_negative_controls_require_output_or_file_mismatch`
- `test_skill_eval.StagedTests.test_real_resume_compaction_and_session_records`
- `test_skill_eval.StagedTests.test_repo_paths_are_removed_from_harness_env_and_warn_on_session_evidence`
- `test_skill_eval.StagedTests.test_reserve_comparison_does_not_require_retention`
- `test_skill_eval.StagedTests.test_resume_material_order_limits_single_record_and_event_turns`
- `test_skill_eval.StagedTests.test_stage_stops_and_partial_cost_never_double_counted`
- `test_skill_eval.StagedTests.test_time_attempt_limits_and_comparison_failures_keep_artifact_verdict`

macOS の一時スクリプトの狭い読み取り範囲、OS の実際の拒否、Linux の実行、勧告の検査は「取り込みの条件」に従いオーケストレータへ残す。初回の Codex 内の検査の時点では、scratch_policy のコメントの根拠の版は変更していなかった。外での確認を受けた更新は次節に記録する。本物の LLM は呼んでいない。

### 外の検査を受けた圧縮の模擬の調整（2026-10-27）

オーケストレータから、macOS 27 と指定した三つの v0.0.12 の処理系、x86_64 の試験の機械を指定して、サンドボックスの外で全体の検査を走らせた結果を受け取った。`processors`・`fmt`・`clippy`・OS のサンドボックスを含む `test` は通った。`skill-eval` の 118 件中、`CompactionTaskTests.test_real_tasks_retain_multiple_read_results_and_distinguish_settings` の一件が、base と reserve の最初の圧縮がともに段 2 となり、`2 < 2` が成り立たず失敗した。`skill-eval` は終了状態 1 で止まり、その後の `perf-tools`・`linux`・`deny`・`size` は走っていない。これは受け取った結果であり、Codex 内で再現した走行として扱わない。

#### 原因とオフラインでの計算

`agent/compaction/mod.rs` の `total_bytes` と `context/mod.rs` の `estimate_tokens`、P5-33 と P5-36 の計算の規則を読み直した。再開時は最後の提供者の使用量がなく、システムプロンプト、空白なしの道具の JSON、会話の項目の UTF-8 の量の合計を 3 で切り上げて割る。模擬の `input_tokens=1000` は道具を使う途中の判定には使うが、段の再開直後の全量による判定を下げることはできない。

HEAD の旧版の固定の期待値と現在の期待値を比較すると、システムプロンプトが 31,390 B から 45,824 B に増えていた。道具の空白なしの JSON はどちらも 5,038 B なので、固定部分は 36,428 B から 50,862 B、差は 14,434 B である。増加は Skill の本文だけでなく、P5-40 で点検した対応と道具の説明も含む。

`mock_provider.py` の筋書きは、各段の全資料を一度ずつ読み、reference を書き、実行し、短い完了の文を返す。`read.rs` と `tools/text.rs::numbered` に従い、各行に幅 6 の行番号、タブ、改行を付けて結果の量を数えた。資料の各行は 2,000 文字以下で、一ファイルの結果も読み取りの上限に届かず、切り詰めはない。段 1 の読み取り結果は JSON が 99,555 B、走査が 99,006 B である。

段の依頼、最初の共通の依頼、資料とスクリプトの読み取りの引数、reference を書く引数、実行の引数、完了の文も数えた。段 2 では reference の読み直しも含めた。環境の情報、write と run の結果、次の段の依頼は含めず、圧縮前の会話を保持した場合の下限として次の値を得た。これはセッションの実測値ではない。

| 課題 | 段 1 後の会話の確認した部分 | 段 2 の入口の下限（旧版の固定部分） | 段 2 の入口の下限（新版の固定部分） | 段 2 後の会話の確認した部分 | 段 3 の入口の下限（新版、まだ圧縮しない条件） |
|---|---:|---:|---:|---:|---:|
| compact_json_summary | 103,779 B | 46,736 トークン | 51,547 トークン | 209,097 B | 86,653 トークン |
| compact_dir_walk | 103,135 B | 46,521 トークン | 51,333 トークン | 212,951 B | 87,938 トークン |

従来の窓 65,536、出力の上限 8,192 では、base の境は 40,960、reserve は 49,152 である。新版では、数えた部分だけで段 2 の入口が両方の境を超える。固定部分の増加が、予備の違いを同じ段の圧縮にしてしまうことを計算でも確認した。

#### 変更した模擬の設定と課題の意図

変更はこの通しのテスト内だけに限った。課題の資料、段の依頼、生成の引数とハッシュ、解と否定の対照、判定、`harnesses.json`、有料の評価の設定は変えていない。07-02 が定める有料の比較の窓 65,536 と出力の上限 8,192 も保っている。

| 設定 | 変更前 | 変更後 | 理由 |
|---|---:|---:|---|
| このテストの全三条件の context_window | 65,536 | 73,728 | 新しい固定部分を含め、段 2 の量を base と reserve の境の間に戻す |
| base・reserve の keep_tokens | 20,000 | 16,384 | 旧設定は窓の四分の一に制限されて実効値が 16,384 だった。窓を広げても、その実効値と既存の検査の上限を変えない |
| base・keep の reserve_tokens | 16,384 | 16,384 | 変更なし |
| reserve の reserve_tokens | 8,192 | 8,192 | 変更なし |
| keep の keep_tokens | 8,000 | 8,000 | 変更なし |
| max_output_tokens・mock_input_tokens | 8,192・1,000 | 8,192・1,000 | 変更なし |

変更後の base・keep の境は 49,152、reserve の境は 57,344 となる。段 2 の確認した量は二つの境の間にあり、高い境までの残りは JSON が 17,391 B、走査が 18,035 B である。ここには未計上の環境の情報、write と run の結果、次の依頼が入る。これらを含む実際の量が高い境を超えないことと、最初の圧縮が base で段 2、reserve で段 3 に分かれることは、外での通しで確かめる。まだ圧縮していない reserve の段 3 の下限は、高い境を十分超える。

P5-33 は、低い模擬の使用量と実際の複数の読み取りを使い、再開の境での圧縮、複数の項目の保持、設定の差を確かめる作業である。P5-36 が扱った、本物の LLM の読み方と出力量に依存する課題の長さの調整とは分け、この決定的な筋書きのテストの窓を合わせ直した。素材を変えると過去の評価と課題の条件が変わり、保留中の P5-36 にも関わるため、この修正では選ばなかった。

テストの作成時の関門も確認した。守る契約は、予備だけの違いが最初の圧縮の段の差として現れることと、実際の複数の読み取り結果を残すことである。固定の文脈が増えて両条件とも同じ境で圧縮する退行は、この通しのテストが持つ。最初の圧縮の段の厳密な大小、読み取りの件数・内容、保持した項目数と見積もりの大小などのアサーションは変えていない。変更前後の Python の AST でも、同テストの 16 個のアサーションがすべて同じことを確認した。新しいテストや本番の差し込み口は加えていない。外での失敗の報告を受けた修正であり、修正後の外での再走行は未実施である。

#### macOS の狭い読み取り範囲の確認

同じ外の検査で、次の件数が通ったとオーケストレータから報告を受けた。

| 対象 | 通った件数 |
|---|---:|
| sandbox_benitoite_scratch | 6 |
| sandbox_benitoite_run | 13 |
| sandbox_benitoite_test | 9 |
| sandbox_benitoite | 2 |
| sandbox_exec_restricted | 15 |
| sandbox_macos | 21 |

`sandbox_benitoite_scratch` の `automatic_file_reads_are_confined_to_project_and_temporary_roots` は、File.Read を自動の範囲に加えた本物の一時スクリプトで、プロジェクトと一時ディレクトリは読め、外のファイルは PermissionDenied となることを確かめる。`optional_automatic_clock_random_and_environment_effects_work_in_the_narrow_policy` は同じ狭い方針で時計・乱数・環境の操作を実行し、時間帯の値を同じ処理系の通常の値と比較する。テストの本文と、道具が `scratch_policy` を使うことを読み、今回の 6 件がこの二つを含むことを確認した。

そのため `sandbox/restricted.rs` のコメントに、旧版の実測の根拠を残したまま、2026-10-27 に v0.0.12・macOS 27 でも同じ組で通ったことを追記した。方針の値や読み取りの範囲は変更していない。これは受け取った外での結果に基づく更新であり、Codex 内で OS の拒否を確かめたという記録ではない。

03-03「非対話での扱い」の「同じ14 個」を「同じ 14 個」に直した。この補足で変更したファイルは `tools/skill-eval/test_skill_eval.py`、`crates/benitoiteh/src/sandbox/restricted.rs` のコメント、03-03 の空白、本作業文書だけである。

#### 修正後の検査

指定された v0.0.12 の macOS の処理系を設定して、`CARGO_NET_OFFLINE=true scripts/check.sh --inside-codex` は終了状態 0、`All checks passed.` で通った。内側で走るテスト、評価と性能の道具、Linux 二対象のビルドとリンク、ライセンス、三対象の release のビルドを含む。最後の除外の一覧は初回の内側の検査と同じであり、問題になった圧縮の通しは今回も走っていない。圧縮の通しと、外の全体の再検査はオーケストレータへ残す。前回の外の検査で走っていない後続の段も、通過したものとして扱わない。

### 二回目の外の検査での Linux の拒否の判定（2026-10-27）

オーケストレータから、macOS 27、指定の三つの v0.0.12 の処理系、`BENITOITEH_X86_64_HOST` を使った二回目の全体の検査の結果を受け取った。`processors`・`fmt`・`clippy`・`test`・`skill-eval`・`perf-tools` は通った。`skill-eval` は、前回直した圧縮の試験を含む 118 件が通った。続く `linux` の段で、x86_64 Linux（podman、Landlock あり）の `log_file_sinks_require_their_own_effects_and_respect_os_write_scope` が失敗し、終了状態 101 で止まった。同じ試験ファイルのほかの 12 件は通った。後続の `deny` と `size` は走っていない。

失敗した書き込みは OS に拒まれ、`outside.log` も作られていなかった。Linux では、書ける場所の外を bubblewrap が読み取り専用でマウントするため、この実行の拒否は `EROFS`（os error 30）となった。試験が使っていた `permission_errno` は `EACCES` と `EPERM` だけを受け付けるため、拒否の判定で失敗していた。

`sandbox_read_image.rs` の既存の書き方に合わせ、この試験の該当のアサーションだけに、Linux では `rustix::io::Errno::ROFS` の番号も受け付ける条件と、その理由のコメントを加えた。`output.is_error` と拒否の番号の両方を必要とし、別の実行の誤りを通さない。`permission_errno` 本体とほかの三つの呼び出しは変更していない。外のファイルがないことと、許された場所への同じ書き込みが成功することの確認も残した。既存の試験の OS の拒否の判定を直すものであり、試験や本番のコードは追加していない。

指定の v0.0.12 の macOS の処理系を `BENITOITEH_TEST_BENITOITE` に設定し、`CARGO_NET_OFFLINE=true scripts/check.sh --inside-codex` は終了状態 0、`All checks passed.` で通った。記録は `work/P5-40-erofs-check-inside-codex.log` に置いた。この試験を含む OS のサンドボックスと Linux のコンテナでの実行は内側の検査から除かれるため、修正した条件での x86_64 Linux の再実行と、外の全体の検査の続きはオーケストレータへ残す。この補足で変更したファイルは `crates/benitoiteh/tests/sandbox_benitoite_run.rs` と本作業文書だけである。

### 変更したファイル

- `assets/benitoite/v0.0.12/skill/`（新。71 ファイル）と `assets/benitoite/v0.0.12/SOURCE.md`（新）。`assets/benitoite/v0.0.7/` は全体を削除した。
- `crates/benitoiteh/examples/update_context_expected.rs`
- `crates/benitoiteh/prompts/benitoite-mapping.md`
- `crates/benitoiteh/prompts/tools/benitoite_check.md`
- `crates/benitoiteh/prompts/tools/benitoite_scratch.md`
- `crates/benitoiteh/prompts/tools/benitoite_test.md`
- `crates/benitoiteh/src/benitoite/effects.rs`
- `crates/benitoiteh/src/benitoite/mod.rs`
- `crates/benitoiteh/src/cli/doctor/processor.rs`
- `crates/benitoiteh/src/cli/exec/tests.rs`
- `crates/benitoiteh/src/config/mod.rs`
- `crates/benitoiteh/src/sandbox/restricted.rs`（外での確認を受けたコメント）。
- `crates/benitoiteh/src/ui/tui/tests.rs`
- `crates/benitoiteh/src/ui/tui/text.rs`
- `crates/benitoiteh/tests/agent_loop/allow.rs`
- `crates/benitoiteh/tests/args.rs`
- `crates/benitoiteh/tests/benitoite_report.rs`
- `crates/benitoiteh/tests/config.rs`
- `crates/benitoiteh/tests/context.rs`
- `crates/benitoiteh/tests/expected/system-normal.txt`
- `crates/benitoiteh/tests/expected/system-restricted-read-plain.txt`
- `crates/benitoiteh/tests/expected/system-restricted.txt`
- `crates/benitoiteh/tests/sandbox_benitoite.rs`
- `crates/benitoiteh/tests/sandbox_benitoite_run.rs`
- `crates/benitoiteh/tests/sandbox_benitoite_scratch.rs`
- `crates/benitoiteh/tests/sandbox_exec_restricted.rs`
- `crates/benitoiteh/tests/support/processor.rs`
- `crates/benitoiteh/tests/tools_benitoite_run.rs`
- `crates/benitoiteh/tests/tools_benitoite_scratch.rs`
- `crates/benitoiteh/tests/tools_benitoite_test.rs`
- `crates/benitoiteh/tests/tui/interaction.rs`
- `docs/design/00-overview/00-01-goals.md`
- `docs/design/00-overview/00-04-glossary.md`
- `docs/design/01-agent/01-01-agent-loop.md`
- `docs/design/01-agent/01-02-modes.md`
- `docs/design/01-agent/01-03-tools.md`
- `docs/design/01-agent/01-04-context.md`
- `docs/design/01-agent/01-05-sessions.md`
- `docs/design/02-providers/02-01-provider-layer.md`
- `docs/design/03-safety/03-01-security-model.md`
- `docs/design/03-safety/03-02-sandbox.md`
- `docs/design/03-safety/03-03-approval.md`
- `docs/design/04-benitoite/04-01-benitoite-integration.md`
- `docs/design/05-interface/05-01-cli.md`
- `docs/design/06-impl/06-01-crate-structure.md`
- `docs/design/07-quality/07-01-testing.md`
- `docs/design/07-quality/07-02-evaluation.md`
- `docs/design/08-appendix/08-02-pre-study-mapping.md`
- `docs/design/README.md`
- `docs/plan/10-interfaces.md`
- `docs/plan/README.md`
- `scripts/check.sh`
- `tools/perf/README.md`
- `tools/skill-eval/README.md`
- `tools/skill-eval/SOURCE.md`
- `tools/skill-eval/skill_eval.py`
- `tools/skill-eval/test_skill_eval.py`（外での失敗を受けた模擬の設定）。
- `docs/plan/tasks/P5-40-processor-v0.0.12.md`（新。補足と本結果を記録）。

### 設計者に判断を求める候補

`Log` を一時スクリプトの確認なしの既定に加える案を、今回の実装から外した。理由: 既定の出力は標準エラー出力だけであり、受け手の出力は `File.Write` などの別のエフェクトとして残るが、確認なしで動く既定を広げるかは設計者が決める。今回の実装は既定を保ち、利用者の明示的な設定だけを受け付ける。
