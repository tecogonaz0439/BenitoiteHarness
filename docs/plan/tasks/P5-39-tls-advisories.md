# P5-39 TLS の依存の更新による勧告の解消

- 状態: 完了
- 依存: P5-38

## 目的

[P5-38](P5-38-advisories.md)の「取り込みの条件」にある、別の作業での依存の更新を行う。`rustls` と `rustls-webpki` を修正した版へ更新し、P5-38 の勧告の検査で挙がった 5 件を解消する。併せて `webpki-root-certs` を更新し、検査の対象に含まれない勧告の版も `Cargo.lock` に残さない。

### 背景

2026-10-25 に、オーケストレータが最新の勧告のデータベースで検査し直し、P5-38 と同じ 5 件だけが出ることを確かめた。これは作業の依頼時に受け取った結果であり、作業文書の執筆時に再実行した結果ではない。

| クレート | 更新前 | 更新後 | 対応する勧告 |
|---|---|---|---|
| `rustls` | 0.23.45 | 0.23.46 | [RUSTSEC-2026-0346](https://rustsec.org/advisories/RUSTSEC-2026-0346.html)、[0347](https://rustsec.org/advisories/RUSTSEC-2026-0347.html)、[0348](https://rustsec.org/advisories/RUSTSEC-2026-0348.html)、[0349](https://rustsec.org/advisories/RUSTSEC-2026-0349.html) |
| `rustls-webpki` | 0.103.15 | 0.103.16 | [RUSTSEC-2026-0382](https://rustsec.org/advisories/RUSTSEC-2026-0382.html) |
| `webpki-root-certs` | 1.0.9 | 1.0.10 | [RUSTSEC-2026-0383](https://rustsec.org/advisories/RUSTSEC-2026-0383.html) |

上の修正した版は、執筆時（2026-10-25）に RustSec の各勧告で確認した。`webpki-root-certs` は `rustls-platform-verifier` 0.7.1 が wasm32 向けに組み込む推移的な依存であり、現行の `deny.toml` の `[graph] targets` には含まれない。このため 0383 は上の 5 件に数えず、更新後の版を `Cargo.lock` で別に確かめる。

## 読む文書

- `AGENTS.md`、[作業の進め方](../00-common.md)、[作業の間の約束](../10-interfaces.md)の「依存のクレート」。
- [P5-38](P5-38-advisories.md)全体。特に「テスト」「完了条件」「取り込みの条件」。
- [クレートの構成](../../design/06-impl/06-01-crate-structure.md)の「依存の基準」、[性能](../../design/06-impl/06-02-performance.md)の「測り方」、[テスト](../../design/07-quality/07-01-testing.md)の「処理系の用意」「完了の前に通す検査」。
- [P5-14 の大きさの記録](../results/P5-14-image-resize-size.md)。新しい測定の記録の形を揃える。
- `Cargo.toml`、`crates/benitoiteh/Cargo.toml`、`Cargo.lock`、`.cargo/config.toml`、`deny.toml`、`scripts/check.sh` の `deny`・`size` の段、`crates/benitoiteh/src/http/mod.rs` の TLS の設定。
- 上の RustSec の勧告と、後述の crates.io の公開の版、手元のクレートのソース。
- 処理系のリポジトリの `v0.0.11` の `crates/benitoite/Cargo.toml` と `Cargo.lock`。`git -C ../Benitoite show v0.0.11:<パス>` でタグの時点のものを読む。オーケストレータが指定した、タグの付いていない更新のコミットは、版の選び方の参考として読むだけにする。

## 版の選び方と試作への影響

### 公開の版と処理系との差

今回使う版は、crates.io の公開の版である [`rustls` 0.23.46](https://crates.io/crates/rustls/0.23.46)、[`rustls-webpki` 0.103.16](https://crates.io/crates/rustls-webpki/0.103.16)、[`webpki-root-certs` 1.0.10](https://crates.io/crates/webpki-root-certs/1.0.10) から選んだ。オーケストレータが取得済みのレジストリのソースの `Cargo.toml` でも、各版を確認した。更新ではレジストリの依存を使い、処理系のファイルを取り込まない。

処理系のリリースのタグ `v0.0.11`（コミット `6ecc9996747db03801004ebee6c38a5283b590a7`）は、まだ更新前の版である。執筆時に [`crates/benitoite/Cargo.toml`](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.11/crates/benitoite/Cargo.toml) の `rustls =0.23.45` と、[`Cargo.lock`](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.11/Cargo.lock) の上の三つの更新前の版を確認した。処理系で進めている同じ更新は版の選び方の参考にとどめ、タグの付いていないコミットへ公開の文書からリンクしない。

`10-interfaces.md` の「処理系と同じクレートは同じ版にする」という原則に対し、今回は勧告を解消するため、リリース版より先に `rustls` の版を上げる。同文書の原則の直後に「ただし、依存の勧告を解消するために、処理系のリリースより先に版を上げることがある（P5-39 の `rustls`）。」と例外を 1 文で加える。機能の指定と TLS の暗号の実装は変えない。この個別の指示に従って進め、一般の原則との違いを完了の報告にも残す。

### RUSTSEC-2026-0382 と OS ごとの検証

RUSTSEC-2026-0382 は、eMudhra の emSign の中間 CA の秘密鍵の漏洩を背景とする。漏洩した鍵に対応する中間 CA を含む証明書の連なりを、設定された信頼の基点によっては受け入れる問題であり、`rustls-webpki` 0.103.16 は該当する公開鍵を含む連なりを拒否する。出典は [RustSec の勧告](https://rustsec.org/advisories/RUSTSEC-2026-0382.html)である。

`benitoiteh` の HTTP の設定は、`ureq::tls::RootCerts::PlatformVerifier` を使う。手元の `rustls-platform-verifier` 0.7.1 のソースで、次の違いを確認した。

| 対象 | 検証の仕組みと今回の更新との関係 |
|---|---|
| Linux | `rustls-native-certs` で OS の CA の束を読み、WebPKI で証明書の連なりを検証する。更新後は `rustls-webpki` 0.103.16 の中間 CA の公開鍵の拒否の一覧が検証に適用される。 |
| macOS | OS の Security.framework に証明書の連なりの検証を任せる。この検証での拒否を `rustls-webpki` の版の更新だけで保証するものではない。 |

出典のリポジトリは `rustls/rustls-platform-verifier`、0.7.1 の `.cargo_vcs_info.json` にあるコミットは `252e25161a91af476cbea620e29d277592d27ec7` である。手元のレジストリの `rustls-platform-verifier-0.7.1/Cargo.toml` の target ごとの依存と、同じコミットの [`src/verification/mod.rs`](https://github.com/rustls/rustls-platform-verifier/blob/252e25161a91af476cbea620e29d277592d27ec7/rustls-platform-verifier/src/verification/mod.rs)、[`others.rs`](https://github.com/rustls/rustls-platform-verifier/blob/252e25161a91af476cbea620e29d277592d27ec7/rustls-platform-verifier/src/verification/others.rs) の `load_native_certs`・`WebPkiServerVerifier`、[`apple.rs`](https://github.com/rustls/rustls-platform-verifier/blob/252e25161a91af476cbea620e29d277592d27ec7/rustls-platform-verifier/src/verification/apple.rs) の `SecTrust`・`evaluate_with_error` への呼び出しを根拠とする。これはソースでの確認であり、漏洩した鍵を使う証明書による接続の再現や、一般の証明書の失効確認を確かめた結果ではない。

## 作るものと手順

1. **依存の更新前に**、`scripts/check.sh` の `size` の段と同じ方法で、ホスト、`aarch64-unknown-linux-musl`、`x86_64-unknown-linux-musl` の release の実行ファイルの大きさを測り、更新前の値を保存する。P5-38 のコミット `a559e95` の依存を基準とし、古いビルドの出力を測るだけで済ませない。
   - `CARGO_NET_OFFLINE=true` を指定し、ホストは `cargo build --release -p benitoiteh`、Linux はそれぞれ `cargo build --release -p benitoiteh --target <対象>` を使う。
   - `target_dir=${CARGO_TARGET_DIR:-target}` とし、ホストは `$target_dir/release/benitoiteh`、Linux は `$target_dir/<対象>/release/benitoiteh` を `wc -c` でバイト単位で測る。スクリプトと同じ release の設定を使い、測定のために追加の strip や設定の変更を行わない。
2. `crates/benitoiteh/Cargo.toml` の `rustls` の版だけを `=0.23.46` にする。`default-features = false` と機能 `std`・`tls12` を保つ。取得済みのクレートを使い、次を順に走らせる。

   ```sh
   cargo update --offline -p rustls --precise 0.23.46
   cargo update --offline -p rustls-webpki --precise 0.103.16
   cargo update --offline -p webpki-root-certs --precise 1.0.10
   ```

   `rustls-webpki` と `webpki-root-certs` は推移的な依存のままとし、直接の依存に加えない。ほかのクレートの版、依存の機能、暗号の実装、`deny.toml` は変えない。`Cargo.lock` の差分がこの三つの版と checksum の更新に限られることを確かめる。ほかの更新が生じたら、その状態で進めず報告する。
3. `docs/plan/10-interfaces.md` の「依存のクレート」の原則の直後に、上の例外の 1 文を加える。依存の表の `rustls` の行の版を `=0.23.46` とし、備考を「機能 `std`・`tls12` は Benitoite と同じ。版は勧告に合わせて 0.23.46 に上げた（P5-39）」とする。表のほかの行は変えない。
4. 更新前と同じ三つの対象を、同じ環境、ツールチェーン、機能、release の設定でオフラインでビルドし、大きさを測る。`docs/plan/results/P5-39-tls-advisories-size.md` を新しく作り、P5-14 の記録と同じく「測り方」「結果」を置く。
   - 測定の日付、環境、Rust の版、更新前のコミット、更新後はそのコミットに本作業の依存の更新を加えた未コミットの状態であること、三つの版、機能、ビルドの設定とコマンドを書く。
   - 結果の表は「対象」「更新前」「更新後」「増減」とし、三つの対象のバイト数、符号付きの差、増減の割合を記録する。測定値から分かる大きさの変化だけを書き、速度やメモリーの改善を主張しない。
5. P5-38 の作業文書の状態と、`docs/plan/README.md` の P5-38 の行を「完了」にする。P5-38 の「テスト」に `### 確かめた結果` を加え、次節の受け取った結果を記録する。過去の検査の結果と今回の実行の結果を区別し、P5-38 の「取り込みの条件」は保つ。
6. 「確かめること」を実施する。完了の報告に、測定の記録、実際に通した検査、外へ残した検査、処理系のリリース版との違いと、二つの作業をまとめて取り込む条件を書く。

## P5-38 に記録する結果

次の結果は、オーケストレータが 2026-10-24 に、P5-38 の変更を加えたコミット前の作業ツリーで、サンドボックスの外で確かめたものである。cargo-deny は 0.20.2。P5-39 の実装者が再実行した結果として書かない。

1. `cargo deny check licenses advisories` は終了状態 1 で終わり、RUSTSEC-2026-0346、0347、0348、0349、0382 の 5 件を挙げた（`advisories FAILED, licenses ok`）。`scripts/check.sh` 全体は、`processors`・`fmt`・`clippy`・`test`・`skill-eval`・`perf-tools`・`linux` の段を通った後、`FAILED: deny (exit 1)` で終わった。x86_64 の Linux は、試験の機械を指定しなかったためビルドとリンクまでとし、aarch64 の Linux の Landlock を要するテストは、既存の扱いどおり走らせなかった。
2. 勧告のデータベースを取れない状態を二つ試した。どちらも、今の `deny.toml` に `[advisories]` の `db-path` を一時の場所にした試験用の写しを、`cargo deny --config <写し> check ...` で使い、通常の保存済みのデータベースを変えなかった。
   - (a) `db-urls` を存在しないホストにし、空の保存先から取得させた。`failed to fetch advisory database` で終了状態 1 となった。
   - (b) 保存済みのデータベースの写しを保存先に置き、`https_proxy` を接続できない宛先にして更新させた。`failed to fetch latest changes` で終了状態 1 となった。

どちらも、古いデータベースやライセンスだけの検査で通ることはなかった。`scripts/check.sh` は `set -e` で cargo-deny の非 0 を `deny` の段の失敗として扱う（1 の結果）。この二つの直接のコマンドの結果と、1 のスクリプト全体の結果を区別して記録する。

## 触るファイル

実装で変更するファイルは次のとおりである。

- `crates/benitoiteh/Cargo.toml`（`rustls` の版のみ）
- `Cargo.lock`（上の三つのクレートのみ）
- `docs/plan/10-interfaces.md`（「依存のクレート」の原則の直後の例外の 1 文と、依存の表の `rustls` の行）
- `docs/plan/results/P5-39-tls-advisories-size.md`（新）
- `docs/plan/tasks/P5-38-advisories.md`（状態と「テスト」の「確かめた結果」の追記）
- `docs/plan/README.md`（P5-38 の状態）

本作業文書と一覧の P5-39 の状態の更新はオーケストレータが行う。`scripts/check.sh`、`deny.toml`、設計書、処理系のファイルは変更しない。本物の LLM を呼ばず、利用者の設定・状態、キーチェーン、トークン、`~/.ssh` を読まない。

## 確かめること

- `Cargo.toml` の `rustls` が `=0.23.46` であり、機能の指定に差分がないこと。
- `Cargo.lock` の `rustls` が 0.23.46、`rustls-webpki` が 0.103.16、`webpki-root-certs` が 1.0.10 であり、更新前の版が残っていないこと。ほかのパッケージの追加・削除・版の変更がないこと。
- `10-interfaces.md` の「依存のクレート」の原則の直後に例外の 1 文があり、`rustls` の行が manifest と一致し、処理系のリリース版と版も同じであるかのように書いていないこと。依存の表のほかの行に差分がないこと。
- 三つの対象で更新前と更新後を測り、比較の条件と増減を結果の文書に記録したこと。
- P5-38 の状態を両方の文書で「完了」にし、2026-10-24 の二つの確かめの結果と、その検査で走らせなかった範囲を記録したこと。
- `CARGO_NET_OFFLINE=true scripts/check.sh --inside-codex` が通ること。勧告の検査を走らせなかったことが、最後の `Skipped with --inside-codex:` の一覧に出ること。
- `deny.toml` の対象や勧告の除外を変えず、検査を通すための例外を加えていないこと。

## テスト

新しい Rust のテストは加えない。依存の更新による既存の契約への影響は、`scripts/check.sh --inside-codex` に含まれる既存のテストとビルドで確かめる。クレート内部の修正を写すテストや、公開のサーバーへ接続するテストは加えない。

`cargo deny --offline check advisories` は完了条件にしない。理由: Codex の中では勧告のデータベースを更新できず、オフラインの検査が通っても最新の勧告との照合にはならない。更新後の勧告の解消は、オーケストレータがサンドボックスの外でデータベースを更新して確かめる。

## 完了条件

- 指定した三つのクレートだけが指定の版へ更新され、機能の指定、TLS の暗号の実装、検査の対象と勧告の扱いを保っている。
- `10-interfaces.md` の指定の行の更新、三つの対象の大きさの記録、P5-38 の状態と結果の記録がそろっている。
- `scripts/check.sh --inside-codex` がオフラインで通る。
- 外の勧告の検査と OS のサンドボックス・Linux の実行の検査を、オーケストレータが行うものとして完了の報告に残している。

## 取り込みの条件

オーケストレータは、サンドボックスの外で `scripts/check.sh` 全体を走らせる。勧告のデータベースを取得・更新した `cargo deny check licenses advisories` と、`BENITOITEH_X86_64_HOST` に試験の機械を指定した x86_64 の Linux のテストを含める。aarch64 の Landlock を要するテストは既存の扱いどおり走らせず、Landlock は x86_64 の試験環境で確かめる。

全体が通った後に、P5-38 と P5-39 をまとめて `gamou` に取り込む。新しい勧告が出た場合は結果を記録してオーケストレータへ報告し、この作業の範囲を広げたり、除外して検査を通したりしない。二つの作業の完了だけで、外の全検査を通す条件を省かない。
