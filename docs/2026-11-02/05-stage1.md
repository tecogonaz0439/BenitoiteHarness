# 段 1: 安全の穴を塞ぐ

[04](04-oss-feature-review.md)の「段 1: 安全の穴を塞ぐ」を、設計者の指示で進めた記録である。段 1 は本番のコードを変える。確かめは、[01](01-cloud-linux-check.md)の環境（x86_64 の Linux、Alpine 3.22.7 の rootfs、root でない利用者）で行った。macOS での確かめは別途とする。

## 入れ子の守る場所の付け替え

04 の案の 5 に当たる。

### 確かめたこと

`crates/benitoiteh/tests/sandbox_linux.rs` に、否定のテスト `existing_nested_metadata_ancestors_cannot_be_renamed_away` を加え、`benitoiteh` の実際のサンドボックス（bubblewrap、Landlock、seccomp）で、すでにある入れ子の守る場所の祖先を付け替えられるかを試した。

直す前のコードでは、`a/.git` があるとき、サンドボックスの中から `a` を `moved` へ付け替えられた（`rename: ok`）。付け替えた後の `a/.git` は、まだない入れ子の守る場所なので、保証の外として作れる（既存のテスト `uncreated_nested_and_deep_metadata_and_uncreated_uppercase_root_are_outside_guarantee`）。すでにある入れ子のリポジトリを、hooks を仕込んだリポジトリに置き換えられることになる。根の `.git` が書ける場所の中の `gitdir:` の先（`meta/repo`）を指すときも、その親 `meta` を付け替えられた。

04 では、原因を書ける根に許す Landlock の `Refer` と見たが、違った。同じ親の中の付け替えには `Refer` は関わらない。原因は、守る場所の祖先がマウントの場所でないことである。

### 直し方

`crates/benitoiteh/src/sandbox/linux/mounts.rs` で、入れ子の守る場所と gitdir の指す先について、書ける場所の根との間にある祖先を、書ける形のまま自分自身へ重ねる（`--bind <祖先> <祖先>`）。親を子より先に、守る場所の読み取りだけの重ねより先に並べる。書ける場所の根（ほかの書ける場所の根を含む）は、もともとマウントの場所なので重ねない。Claude Code の sandbox-runtime の `ancestorPinArgs` と同じ考え方である。sandbox-runtime は祖先を読み取りだけで重ねるが、`benitoiteh` は祖先の中にこれまでどおり書けるよう、書ける形で重ねる。

祖先をマウントの場所にすると、祖先そのものの付け替えは `EBUSY` で、祖先の中から外の親へ移す付け替えは `EXDEV` で拒否される。テストは、終了状態に加えて、拒否がこのどちらかであることを確かめる。終了状態 11 はほかの誤りでも返るからである。

### 代償

入れ子のリポジトリを含む祖先の中と外の間の付け替えは、書ける場所の中でも `EXDEV` で失敗する。`mv` は写して消す形に切り替えるが、`rename(2)` だけを使うプログラムは失敗する。入れ子のリポジトリ（サブモジュールなど）が根から 3 段までにあるときだけ起こる。安全の性質を弱めないため受け入れ、設計書 03-02 の「Linux のファイルシステムの見え方」の「守る場所の祖先」に【方針】として書いた。

### テストと確かめ

| 変更 | 内容 |
|---|---|
| `tests/sandbox_linux.rs` | 否定のテストを加えた。`a/.git` の親 `a`、`a/b/.git` の祖先 `a` と `a/b`、gitdir の指す先 `meta/repo` の親 `meta` を、同じ親の中とディレクトリをまたぐ先へ付け替えられないこと、留めを外せないこと、祖先の中に書けることを確かめる |
| `src/sandbox/linux/mounts/tests.rs` | 引数列の契約のテスト `mount_order_keeps_masks_after_writable_roots` の期待値に、祖先の留め（`a`、`a/b`）を加えた |

作成時の関門の答えは次のとおりである。

1. 守る契約: すでにある入れ子の守る場所は、子プロセスが祖先を付け替えても、その位置で守られる。
2. 捕まえる退行: 祖先を留めない、gitdir の祖先を留め忘れる、留めを守る場所より後に並べる。
3. 既存のテストとの違い: 既存の `gitdir_targets_and_existing_nested_metadata_are_read_only` は守る場所そのものの付け替えだけを試し、祖先は試していなかった。
4. 本番のコードに差し込み口を加えていない。

直す前と後で、次のように確かめた。

| コード | 結果 |
|---|---|
| 直す前 | `a` の付け替えが通り、テストが失敗した |
| gitdir の祖先だけを留めない版 | `meta` の付け替えが通り、テストが失敗した |
| 直した後 | 通った |

## 子プロセスの環境

04 の案の 1 に当たる。

### 直し方

`crates/benitoiteh/src/config/environment.rs` の `child_environment` で、提供者のキーに加えて、秘密に見える名前の変数を除く。名前が `KEY`・`SECRET`・`TOKEN`・`PASSWORD`・`PASSWD`・`CREDENTIAL` のどれかを含むもの（ASCII の大文字と小文字を区別しない）とする。Codex CLI の既定の除外（`*KEY*`・`*SECRET*`・`*TOKEN*`）に、パスワードと資格情報の語を足した。Codex CLI はこの除外を既定では使わないが、`benitoiteh` は既定で使う。

利用者の設定ファイルの `[secrets] pass_env` に書いた名前（完全に一致するもの）は除かない。空の名前、`=` か NUL を含む名前、提供者のキーの変数（設定で参照したものを含む）を書いたら、設定の誤りとして起動を止める。プロジェクトの設定ファイルは `[secrets]` を受け付けないので、信頼しないリポジトリから秘密を通させることはできない。

制限モードの実行（`crates/benitoiteh/src/tools/benitoite_process.rs`）は、起動のときに作った環境を受け取り、方針の印を付け直すために `child_environment` を二度目に呼んでいた。二度目に秘密の名前を除くと、利用者が通した名前まで落ちるので、印の付け直しだけを `remark_child_environment` に分けた。提供者のキーは、ここでも念のため改めて除く。

### 04 の案から変えたこと

04 の案は、`LD_PRELOAD`・`DYLD_*`・`NODE_OPTIONS`・`PYTHONPATH` などを必ず除くとしていたが、採らなかった。親の環境は利用者が設定したものであり、`benitoiteh` の信頼の前提では信頼する側にある。nono は親の環境も信頼しない前提に立つので、これらを必ず除く（`crates/nono-cli/src/exec_strategy/env_sanitization.rs` の `is_dangerous_env_var`、コミット `c657265f65457efbd0b4e9e2cf246bdeb45cbe22`）。LLM が子の環境にこれらを設定する道筋（Benitoite の `Process.Command` の `environment`）は、処理系が `LD_`・`DYLD_` で始まる名前を拒む（BH-026）。除くと、利用者の `GOFLAGS` や `GEM_HOME` などの設定が効かなくなる。

nono は、継いだ環境の秘密の名前を既定では除かない（既定はすべて通す。`docs/cli/features/environment.mdx` の「Security Properties」）。`benitoiteh` の既定は、nono と Codex CLI の既定より狭い。

### 代償

`GITHUB_TOKEN`・`NPM_TOKEN`・`AWS_ACCESS_KEY_ID` などに頼るコマンドは、サンドボックスの中でも外（`escalate`）でも、利用者が `pass_env` に書くまで認証に失敗する。秘密を名前に表さない変数（URL に埋め込んだパスワードなど）は除けない。

### テストと確かめ

| 変更 | 内容 |
|---|---|
| `tests/config.rs` の `child_process_environment_removes_all_provider_keys_and_replaces_inherited_markers` | 語ごとの例（大小文字の違いと、語が名前の途中にある形を含む）が除かれること、`pass_env` の名前と `SSH_AUTH_SOCK` が残ることを、実際の子（`/usr/bin/env`）に渡して確かめる行を足した。過去の結果の文書が名前で引くので、名前は変えていない |
| `tests/config.rs` の `askpass_is_enabled_by_default_and_only_user_config_can_disable_it` | `pass_env` に提供者のキー、設定で参照したキー、空の名前、`=` を含む名前を書くと誤りになること、プロジェクトの設定では受け付けないことを足した |
| ほかの呼び出し元のテストと例 | 引数を一つ足した（通す名前は空） |

作成時の関門の答えは次のとおりである。

1. 守る契約: 子プロセスの環境に、利用者が通すと決めていない秘密に見える名前の変数が届かない。提供者のキーは、利用者の設定でも通せない。
2. 捕まえる退行: 判定の語の漏れ、大小文字の扱い、通す名前の無視、通す名前の検証の漏れ。
3. 既存のテストとの違い: 既存の行は提供者のキーだけを確かめていた。同じ持ち主の境界（環境を作る公開の関数と、設定の解釈）に行を足し、新しいテストは加えていない。
4. 本番のコードに差し込み口を加えていない。`remark_child_environment` は本番の呼び出し元（制限モードの実行）が使う。

直す前と後で、次のように確かめた。

| コード | 結果 |
|---|---|
| 秘密の名前の判定がいつも偽を返す版 | 環境のテストが失敗した |
| `pass_env` の検証を外した版 | 設定のテストが失敗した |
| 直した後 | 通った |
