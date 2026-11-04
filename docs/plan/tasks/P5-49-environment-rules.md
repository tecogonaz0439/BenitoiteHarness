# P5-49 子プロセスの環境の規則（allow・deny・set）

- 状態: 完了
- 依存: P5-48

## 目的

子プロセスの環境から秘密を除く仕組みを、名前の語の判定と通す名前の一覧（`[secrets] pass_env`）の組から、利用者が書く行の並び（`[environment] rules`）を上から順に当てる形に改める（設計書 03-02「子プロセスの環境」の「環境の規則」「規則の後に必ず当てる変更」）。

## 経緯

2026-11-04 の安全のレビューは、秘密に見える名前の判定の漏れ（`MYSQL_PWD`・`*_PAT` など）を挙げた（[P5-47 の記録](../results/P5-47-macos-checks.md)の「直さなかったレビューの指摘」）。設計者は、名前の判定が経験則にとどまることを踏まえ、語を足す代わりに、nono の通す一覧・除く一覧・決まった値の設定を元に、行の順で決める規則の形にすると決めた。同じ日に、次のことも決めた。

- 規則の後に、提供者のキーを除き、`benitoiteh` の変数を設定する。この変更は規則で崩せない。
- 値の形で秘密を見分ける判定は見送る。
- `rules` を書かないときは `rules = ["allow *"]` と同じに扱う。2026-11-02 から 11-04 までの既定（秘密に見える名前の変数を除く）は改め、提供者のキーのほかはすべて受け継ぐ。秘密に見える名前を除く規則は、設定の例として設計書に示す。
- nono の `env_credentials` に当たる仕組みは将来対応とする（BH-033）。

## 読む文書

- `AGENTS.md`、[作業の進め方](../00-common.md)、[作業の間の約束](../10-interfaces.md)の `config::child_environment` の項。
- 設計書 03-02「子プロセスの環境」（「環境の規則」「規則の後に必ず当てる変更」とローダの変数の段落）、03-01 の R5・N3、04-01「処理系の起動」、05-01「設定ファイル」の `[environment]` の項と「doctor」、07-01「安全の性質のテスト」。
- `.claude/skills/test-audit/SKILL.md`。

## 作るもの

1. **規則の型と評価**（`src/config/environment.rs`）
   - 規則の一行を表す型（`Allow(パターン)`・`Deny(パターン)`・`Set(名前, 値)`）と、行の並びの型（例: `EnvironmentRules`）を作る。既定は `allow *` の一行とする。今の `SECRET_NAME_PARTS` と `secret_like` は、ほかに使う所がなければ取り除く。
   - パターンの照合は、`*` だけを長さ 0 以上の任意の並びとし、ASCII の大文字と小文字を区別しない。`*` を含まないパターンは全体の一致とする。正規表現のクレートは加えない。
   - 評価は、空の環境から行を順に当てる。`allow` は親の環境のうち合うものを親の値で入れ（すでにある名前は置き換え）、`deny` は今の環境から合うものを除き、`set` は値を入れる。
   - `child_environment` の引数 `passed: &BTreeSet<String>` を、規則の並びに替える。規則を当てた後に、今の `remark_child_environment` の処理（提供者のキーの除去、印の除去と設定）を当てる順は変えない。設定で参照した提供者のキー（`referenced_keys`）も、規則の後に除く。
   - 制限モードの処理系の起動（`src/tools/benitoite_process.rs` の `remark_child_environment` の使い方）は変えない。
2. **設定**（`src/config/mod.rs`、`src/config/text.rs`）
   - `[environment]` の表と `rules`（文字列の配列）を加え、読み込みのときに規則の型へ変える。書かなければ `allow *` の一行とする。
   - 設計書 05-01 の誤りの条件をすべて誤りとして起動を止める。条件は、知らない語、形の誤り、空のパターン、`set` で提供者のキーか `benitoiteh` が設定する名前を書く、`*` のない `allow` で提供者のキーを書く、である。
   - プロジェクトの設定ファイルの `[environment]` は誤りとする。
   - `[secrets]` の `pass_env` を取り除く。書かれていたら、`[environment] rules` への書き換えを促す文を添えた誤りとする。
   - 文言は英語で `text` にまとめる。
3. **呼び出し元**（`src/cli/exec.rs` の 3 か所、`src/cli/doctor/processor.rs`）は、`pass_env` の代わりに規則を渡す。
4. **doctor**（`src/cli/doctor/`）に、設計書 05-01「doctor」の項目を加える。
   - 使っている規則（利用者の規則か既定か）と、子に渡る変数の名前の一覧を示す。一覧は、規則と必ず当てる変更の後のもので、値は示さない。
   - その行を除いても結果が変わらない行を「注意」として示す。終了状態は変えない。
5. **文書**: `docs/plan/10-interfaces.md` の `config::child_environment` の項を新しい引数に改める。設計書は本作業の前に書いてあるので、実装と食い違う点があれば作業を止めて報告する。

## 触るファイル

`crates/benitoiteh/src/config/environment.rs`・`mod.rs`・`text.rs`、`src/cli/exec.rs`、`src/cli/doctor/`、`child_environment` を呼ぶテストと準備（`tests/support/bash.rs` など。`git grep -n 'child_environment('` で探す）、`tests/config.rs`、`docs/plan/10-interfaces.md`、本書、`docs/plan/README.md`。依存のクレートは加えない。

## テスト

`test-audit` の作成時の関門に従う。同じ契約を層ごとに重ねない。

- **規則の評価**（持ち主の境界は `child_environment`）: 表の形のテストを一つ置く。次の行を並べる。
  - 規則を書かなければ、提供者のキーのほかはすべて残る。
  - 設計書の例の規則で、秘密に見える名前が除かれ、ほかが残る。小文字の名前を含む。
  - 先頭に `allow *` がなければ受け継がない。
  - 最後の `allow *` が前の `deny` を打ち消す。
  - `set` の後の `deny <名前>` と `deny *` が `set` を打ち消す。
  - `set` の後の `allow` が、親にある名前だけを親の値に置き換える。
  - `*` が先頭・途中・末尾にあるパターン、`*` のない完全一致。
  - `allow *` でも提供者のキーと設定で参照したキーが残らず、`TMPDIR` などの `benitoiteh` の変数が上書きされない。
- **設定の誤り**（`tests/config.rs`）: 誤りの条件ごとの行を、既存の表の形のテストに足す。条件は、知らない語、形の誤り、空のパターン、`set` の禁じた名前、`*` のない `allow` の提供者のキー、プロジェクトの設定の `[environment]`、`pass_env` である。
- **本物の子プロセス**: 既存の `tests/sandbox_exec_restricted.rs` の、通常モードの `bash` が秘密を受け取らないことを確かめるテストを、規則を書いた設定で確かめる形に改める（既定ではもう秘密に見える名前を除かないので、今の期待のままでは失敗する。`git grep -n 'secret' crates/benitoiteh/tests` で、既定の除外を前提にしたテストを探して同じく改める）。`allow` で通した名前が本物の `bash` に届き、`deny` した名前が届かないことを確かめる。新しいテストのファイルは作らない。
- **doctor**: 効いていない行を「注意」として示すことと、値を出さないことを、既存の doctor のテストに足す。

壊した版で、次のように失敗することを確かめ、結果に記録する。

| 壊し方 | 失敗するべき行 |
|---|---|
| 行の順を無視して `deny` を優先する | 最後の `allow *` の行 |
| 大小文字を区別する | 小文字の名前の行 |
| 規則の後の提供者のキーの除去を外す | `allow *` で提供者のキーが残らない行 |

## 完了の条件

- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが、外で `scripts/check.sh` の全体（`BENITOITEH_X86_64_HOST` を指定）を通す。
- 本書の「確かめた結果」に、行ったことと、外に残した検査を書く。

## 作ったもの

作業はオーケストレータ（Claude Code）が行った（2026-11-05 の設計者の指示）。

- `src/config/environment.rs` に `EnvironmentRule`（`Allow`・`Deny`・`Set`）と `EnvironmentRules`（既定は `allow *` の一行）を置き、`SECRET_NAME_PARTS` と `secret_like` を取り除いた。`child_environment` は規則を当てた後に、設定で参照した提供者のキーを除き、`remark_child_environment` を当てる。`ineffective_rules` は、一行ずつ除いて `child_environment` の結果（名前と値）を比べ、変わらない行を返す。
- 設定に `[environment] rules` を加え、`UserConfig::parse` で規則の型へ変える。誤りの文は、値を含めず、行の位置（1 から数える）と理由だけを示す。`set` で書けない名前の判定と、`*` のない `allow` の提供者のキーの判定は、ASCII の大文字と小文字を区別しない（パターンの照合と揃えた）。秘密の入力の補助の変数の一覧は `askpass::ENVIRONMENT_NAMES` にまとめ、補助の変数を消す処理と共有した。
- `[secrets] pass_env` を取り除いた。書かれていたら、`environment.rules` への書き換えを促す文で起動を止める。
- doctor に `environment.rules`（使う規則。`set` の値は示さない）、`environment.names`（子に渡る名前）、効いていない行ごとの `environment.rules` の「注意」を加えた。

## 確かめた結果

テストは次のとおりである。新しいテストの関数は作らず、既存のテストの表に行を足すか、既存のテストを改めた。過去の結果の文書が名前で引くので、テストの名前は変えていない。

| テスト | 守る契約 |
|---|---|
| `tests/config.rs` の `child_process_environment_removes_all_provider_keys_and_replaces_inherited_markers` | 「テスト」の節の規則の評価の行を表にし、設定の読み込みから本物の `/usr/bin/env` までで確かめる。どの行でも、提供者のキーが残らず、`TMPDIR` などが上書きされる |
| `tests/config.rs` の `askpass_is_enabled_by_default_and_only_user_config_can_disable_it` | 誤りの条件ごとの行（知らない語、形の誤り、空のパターン、`set` の禁じた名前、`*` のない `allow` の提供者のキー）、`pass_env` の誤りの文、プロジェクトの設定の `[environment]` と `pass_env` を拒むこと、誤りの文が値を含まないこと |
| `tests/sandbox_exec_restricted.rs` の `redirected_xdg_keeps_default_home_paths_unreadable_from_normal_bash` | exec が利用者の規則を本物の `bash` に渡すこと（`allow GH_TOKEN` で通した名前が届き、`deny *TOKEN*` した `GITHUB_TOKEN` が届かない） |
| `tests/doctor.rs` の `credentials_and_url_secrets_never_reach_either_output_stream` | doctor が受け継いだ値と `set` の値を出さず、名前の一覧から設定で参照した提供者のキーを除き、効いていない 3 行だけを「注意」として示すこと |

`git grep -n 'secret' crates/benitoiteh/tests` で探した限り、既定の除外（秘密に見える名前を除く）を前提にしたテストは、上の `tests/config.rs` の一つだけだった。

壊した版で、次のように確かめた。

| 壊した箇所 | 結果 |
|---|---|
| 行の順を無視して `deny` を最後に当てる | 最後の `allow *` の行で失敗した |
| パターンの照合で大小文字を区別する | 例の規則の行（小文字の `client_secret_file`）で失敗した |
| 規則の後の提供者のキーの除去（固定の一覧と設定で参照したもの）を外す | 規則を書かない行（`allow *` と同じ）で失敗した |
| exec の通常モードの `bash` に、利用者の規則の代わりに既定の規則を渡す | `redirected_xdg_keeps_default_home_paths_unreadable_from_normal_bash` で失敗した |

macOS で `scripts/check.sh --inside-codex` が終了状態 0 で通った。

全体の検査（`scripts/check.sh`、`BENITOITEH_X86_64_HOST` を指定。コミット `394daad`）は終了状態 0 だった。テストの関数を足していないので、件数は P5-48 と同じである。

| 対象 | 結果 |
|---|---|
| macOS | 819 件が通り、失敗 0、無視 14。評価の道具の Python のテスト 118 件も通った |
| aarch64 の Linux（Apple の `container`） | 721 件が通り、失敗 0、無視 13（Landlock を要する `sandbox_*.rs` は飛ばす） |
| x86_64 の Linux（試験の機械の podman） | 815 件が通り、失敗 0、無視 16 |
