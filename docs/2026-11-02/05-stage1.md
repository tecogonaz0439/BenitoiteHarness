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
