# P5-38 依存のクレートの勧告の検査

- 状態: 完了
- 依存: P0-01

## 目的

`scripts/check.sh` に依存のクレートの勧告の検査を加え、勧告に該当する版を含む `Cargo.lock` が、ライセンスの検査だけで通ることを防ぐ。依存の版の更新は別の作業とし、この作業では `Cargo.toml` と `Cargo.lock` を変えない。

### 背景

2026-10-24 に、オーケストレータが、eMudhra の中間 CA の鍵の漏洩（2026-10-17 に報告）に起因する RUSTSEC-2026-0382（別名 CVE-2026-111943、`rustls-webpki` 0.103.16 で修正）を確認した。その際、サンドボックスの外で cargo-deny 0.20.2 の `cargo deny check advisories` を走らせ、現行の `Cargo.lock` について次の 5 件で失敗することを確かめた。これは作業の依頼時に受け取った結果であり、作業文書の執筆時に再実行した結果ではない。

| クレートと版 | 勧告 | 修正した版 |
|---|---|---|
| `rustls` 0.23.45 | RUSTSEC-2026-0346、RUSTSEC-2026-0347、RUSTSEC-2026-0348、RUSTSEC-2026-0349 | 0.23.46 |
| `rustls-webpki` 0.103.15 | RUSTSEC-2026-0382 | 0.103.16 |

同じ確認では、`webpki-root-certs` 1.0.9 の RUSTSEC-2026-0383 は報告されなかった。`rustls-platform-verifier` 0.7.1 が wasm32 のときだけ組み込む依存であり、現行の `deny.toml` の `[graph] targets` の対象には含まれない。この作業では検査の対象を変えず、この 1 件を失敗の期待値に加えない。

この作業の実装後は、依存を更新するまで、サンドボックスの外の `scripts/check.sh` が `deny` の段で失敗する。その状態で取り込まず、後述の「取り込みの条件」を満たしてから取り込む。

## 読む文書

- `AGENTS.md`、[作業の進め方](../00-common.md)、[作業の間の約束](../10-interfaces.md)の「ファイルの配置」「依存のクレート」。
- [テスト](../../design/07-quality/07-01-testing.md)の「処理系の用意」「完了の前に通す検査」。
- [クレートの構成](../../design/06-impl/06-01-crate-structure.md)の「前提」「依存の基準」。
- `scripts/check.sh` 全体。特に `deny` の段、`finish()` と `trap finish EXIT`、`x86_64_skipped` などの除いた検査の表示。
- `deny.toml`、`Cargo.toml`、`Cargo.lock`。後の二つは現行の依存を確かめるために読み、変更しない。
- 後述の cargo-deny 0.20.2 の一次資料。
- 処理系のリポジトリ（既定は `../Benitoite`）の `v0.0.11` の `scripts/check.sh` と `deny.toml`。`git -C ../Benitoite show v0.0.11:scripts/check.sh` と `git -C ../Benitoite show v0.0.11:deny.toml` で、タグの時点のものを読む。

## 確認した既定と処理系との差

### cargo-deny 0.20.2 の勧告の扱い

`[advisories]` の表は加えず、cargo-deny 0.20.2 の既定を採用する。理由: 表を省略しても勧告の検査は動き、脆弱性を誤りとして検出するという今回の目的を満たす。既定と同じ設定を重ねて書く必要はない。

手元の `cargo-deny-0.20.2` のソースと `.cargo_vcs_info.json` で、次の既定を確認した。出典のリポジトリは `EmbarkStudios/cargo-deny`、コミットは `bca0dde53651ee946720e4540b5ce2610bec8f06` である。

| 種類 | 既定の扱い |
|---|---|
| 脆弱性 | `ignore` で除外していなければ誤りとして検査を失敗させる。`vulnerability` による水準の指定は廃止されている。 |
| `unmaintained` | `"all"`。検査対象のすべての依存について、該当する勧告を誤りとする。 |
| `unsound` | `"workspace"`。ワークスペースのクレートの直接依存に該当する勧告を誤りとする。間接依存だけに該当するものはこの既定では報告しない。 |
| `yanked` | `"warn"`。レジストリから取り下げられた版を警告し、それだけでは検査を失敗させない。取り下げの検査自体は有効である。 |

設定の既定は [`src/advisories/cfg.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/bca0dde53651ee946720e4540b5ce2610bec8f06/src/advisories/cfg.rs) の `Config::default` と `Deserialize`、対象の絞り方は [`src/advisories.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/bca0dde53651ee946720e4540b5ce2610bec8f06/src/advisories.rs)、診断の水準は [`src/advisories/diags.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/bca0dde53651ee946720e4540b5ce2610bec8f06/src/advisories/diags.rs) の `diag_for_advisory` による。表を省略した場合の動作は、[公式の設定の文書](https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html)にも記載がある。版を特定した判断は、上のコミットのソースを根拠とする。

勧告のデータベースの既定の保存先は `$CARGO_HOME/advisory-dbs`（通常は `~/.cargo/advisory-dbs`）、取得先は RustSec の `advisory-db` である。オンラインの検査では取得・更新の失敗を誤りとして返す。出典は同じコミットの [`src/advisories/helpers/db.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/bca0dde53651ee946720e4540b5ce2610bec8f06/src/advisories/helpers/db.rs) の `AdvisoryDb::load` と [`src/cargo-deny/check.rs`](https://github.com/EmbarkStudios/cargo-deny/blob/bca0dde53651ee946720e4540b5ce2610bec8f06/src/cargo-deny/check.rs) のデータベースの読み込みである。

勧告の除外（`ignore`）は置かない。将来除外を加えるときは、勧告の ID、除外する理由、除外を外す条件を設定と併せて記録する。検査を通すためだけに今回の 5 件を除外しない。

### Benitoite v0.0.11 との比較

作業文書の執筆時に、処理系の `v0.0.11`（コミット `6ecc9996747db03801004ebee6c38a5283b590a7`）のファイルを `git show` で確認した。[`scripts/check.sh`](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.11/scripts/check.sh) の `check_licenses()` は `cargo deny check licenses` だけを走らせ、勧告の検査を持たない。[`deny.toml`](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.11/deny.toml) には `[licenses]` と `[graph]` があり、`[advisories]` はない。本プロジェクトが処理系より先に勧告の検査を加えることになる。処理系のファイルは変更しない。

## 作るものと手順

1. `scripts/check.sh` の `deny` の段を、実行場所に応じて次のようにする。
   - サンドボックスの外では `cargo deny check licenses advisories` を走らせ、勧告のデータベースをネットワークから取得・更新する。取得できないときも失敗させ、ライセンスだけの検査への切り替えや、失敗を成功に変える処理を加えない。理由: 飛ばすと勧告を確かめないまま検査が通る。07-01「処理系の用意」で、処理系がないときにテストを飛ばさないのと同じ考えである。
   - `--inside-codex` では、現行の `cargo deny --offline check licenses` だけを走らせる。理由: 勧告のデータベースは通常 `~/.cargo` の下にあり、Codex のサンドボックスから更新できない。
   - `finish()` の `Skipped with --inside-codex:` の一覧に、勧告の検査を走らせなかったことと、サンドボックスの外で更新が必要なことを英語で示す。例えば `deny: advisories (requires advisory database updates outside Codex)` とする。既存の `EXIT` の処理に合わせ、検査が途中で失敗してもこの一覧を示す。
   - 既存の `stage` と終了状態の扱いを保ち、勧告やデータベースの取得で失敗したときは `FAILED: deny` を示して非 0 で終わる。`All checks passed.` を示さず、後の `size` の段へ進まない。
2. `deny.toml` の先頭のコメントを「Benitoite v0.0.6（既存のコミットの表記）と同じライセンスの許可の一覧」と分かる形に直す。理由: 元の版への参照が `[licenses]` の許可の一覧についての記述だと明確にする。`[licenses]` と `[graph]` の値は変えず、上の判断のとおり `[advisories]` と `ignore` は加えない。
3. 次の文書を実装と同じ変更で直す。
   - 07-01「完了の前に通す検査」の 4 を `cargo deny check licenses advisories` とする。直後に「理由: ライセンスの検査だけでは、依存のクレートの既知の脆弱性を検出できない。勧告のデータベースを取得できないときも失敗させ、勧告を確かめないまま検査が通ることを防ぐ。」のように、理由を 1〜3 文で添える。
   - 06-01「依存の基準」の付近に、本プロジェクトではライセンスと勧告を `cargo-deny` で確かめることを別の文で書き、07-01「完了の前に通す検査」を参照する。「前提」の Benitoite の事実（執筆時点の 12 行目）は書き換えない。
   - `docs/plan/10-interfaces.md` の `deny.toml` の行を、例えば「依存のライセンスと勧告の検査の設定（勧告は cargo-deny の既定）」とし、ファイルの役割を揃える。
4. 下の「確かめること」を実施し、サンドボックスの外の確かめをオーケストレータへ残す。完了の報告には、実際に確かめた範囲、残した確かめ、依存の更新前は取り込めないことを書く。

## 触るファイル

実装で変更するファイルは次のとおりである。

- `scripts/check.sh`
- `deny.toml`（先頭のコメントのみ）
- `docs/design/07-quality/07-01-testing.md`
- `docs/design/06-impl/06-01-crate-structure.md`
- `docs/plan/10-interfaces.md`

`Cargo.toml` と `Cargo.lock` は変更しない。依存の更新と、それに伴う版の記述の更新は別の作業で行う。本作業文書と一覧の状態の更新はオーケストレータが行う。

## 確かめること

- `bash -n scripts/check.sh` が通ること。
- `scripts/check.sh --inside-codex` が通り、最後の `Skipped with --inside-codex:` の一覧に勧告の検査を走らせなかったことが出ること。`deny` の段でライセンスだけをオフラインで検査し、勧告のデータベースを更新しないこと。
- サンドボックスの外の分岐に `--offline` を付けず、`cargo deny check licenses advisories` を走らせること。既存の `CARGO_NET_OFFLINE` の指定も含めて取得を妨げていないことは、オーケストレータが外で確認する。
- `Cargo.toml`、`Cargo.lock`、`[licenses]` の許可の一覧、`[graph] targets` に差分がないこと。
- 設計書の検査のコマンド、理由、`deny.toml` の役割の説明が、スクリプトと一致すること。Benitoite も勧告を検査するかのように書き換えていないこと。

## テスト

新しい Rust のテストは加えない。検査スクリプトの契約は上の実行結果で確かめる。次の二つはオーケストレータがサンドボックスの外で行い、コマンド、cargo-deny の版、終了状態、失敗した段と理由を記録する。

1. 依存の更新前の `Cargo.lock` で、`cargo deny check licenses advisories` が背景の 5 件を挙げて失敗し、`scripts/check.sh` も `deny` の段で失敗すること。データベースの更新で別の勧告が増えていたら、追加分も失敗の理由として記録する。
2. 勧告のデータベースを取得・更新できない状態で、勧告の検査が非 0 で終わり、`scripts/check.sh` が `deny` の段で失敗すること。試験用の一時の設定とデータベースの保存先を使い、更新先への通信を失敗させるなどして、通常の保存済みデータベースを壊さずに確かめる。勧告による失敗と取得の失敗を区別し、古いデータベースやライセンスだけの検査に切り替えて通していないことを確認する。

### 確かめた結果

2026-10-24 に、オーケストレータが、P5-38 の変更を加えたコミット前の作業ツリーで、サンドボックスの外で確かめた。cargo-deny は 0.20.2。以下はオーケストレータから受け取った結果であり、P5-39 の実装者が再実行した結果ではない。

1. `cargo deny check licenses advisories` は終了状態 1 で終わり、RUSTSEC-2026-0346、0347、0348、0349、0382 の 5 件を挙げた（`advisories FAILED, licenses ok`）。`scripts/check.sh` 全体は、`processors`・`fmt`・`clippy`・`test`・`skill-eval`・`perf-tools`・`linux` の段を通った後、`FAILED: deny (exit 1)` で終わった。x86_64 の Linux は、試験の機械を指定しなかったためビルドとリンクまでとし、aarch64 の Linux の Landlock を要するテストは、既存の扱いどおり走らせなかった。
2. 勧告のデータベースを取れない状態を二つ試した。どちらも、今の `deny.toml` に `[advisories]` の `db-path` を一時の場所にした試験用の写しを、`cargo deny --config <写し> check ...` で使い、通常の保存済みのデータベースを変えなかった。
   - (a) `db-urls` を存在しないホストにし、空の保存先から取得させた。`failed to fetch advisory database` で終了状態 1 となった。
   - (b) 保存済みのデータベースの写しを保存先に置き、`https_proxy` を接続できない宛先にして更新させた。`failed to fetch latest changes` で終了状態 1 となった。

どちらも、古いデータベースやライセンスだけの検査で通ることはなかった。`scripts/check.sh` は `set -e` で cargo-deny の非 0 を `deny` の段の失敗として扱う（1 の結果）。

## 完了条件

- 「作るものと手順」の変更がそろい、`bash -n scripts/check.sh` と `scripts/check.sh --inside-codex` が通る。
- `--inside-codex` の最後の表示に、勧告の検査を走らせなかったことが出る。
- 依存の版を変えず、勧告の除外も加えていない。
- サンドボックスの外の二つの確かめを、オーケストレータが行うものとして完了の報告に残す。

## 取り込みの条件

この作業の完了だけでは取り込まない。依存の更新前に `deny` の段が背景の 5 件で失敗することは、勧告の検出の確かめとして記録し、検査を通した結果としては扱わない。

オーケストレータは、上のサンドボックスの外の二つの確かめを行い、別の作業で依存の版を更新して勧告を解消した後に、本作業を取り込む。取り込みの前に、依存の更新を含む状態でサンドボックスの外の `scripts/check.sh` 全体を通す。共通の「取り込む前にすべての検査を通す」という条件を、この作業のために緩めない。
