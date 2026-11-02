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

## 読ませないホームの場所

04 の案の 2 に当たる。

### 直し方

`crates/benitoiteh/src/cli/mod.rs` の読ませない場所の候補（`exec` と `doctor` が共有する）に、nono の既定の拒否の組から、資格情報、パスワードの保管庫、シェルの履歴、ブラウザのプロファイルを加えた。一覧は設計書 03-02 の「読ませないホームの場所」に書いた。候補のうち、ないものは方針を作るときに飛ばし、シンボリックリンクは正規化して指す先を塞ぐ（今までの扱いのまま）。

nono の `deny_shell_configs` のうち、シェルの起動の設定（`~/.bashrc`・`~/.profile` など）は採らず、`~/.env` と `~/.envrc` だけを採った。起動の設定を読めないと、ログインのシェルなどが失敗し、使い勝手への影響が大きいからである。採るかは、評価で影響を測ってから設計者が決める。

### 行わなかったこと

04 の案の 2 は、道具の「読まないファイル」（プロジェクトの中の `.env*` と鍵の形の名前）を `bash` からも読めなくすることを含んでいた。これは BH-100（【未決】）と同じ判断であり、`.env` を読む正当なコマンドが動かなくなる代償があるので、設計者の判断を待つ。設計書 01-03 は、通常モードの `bash` でこれらを読めることを、保証の外と書いている。

### 確かめ

読めない設定のファイルの扱いを、root でない利用者で確かめた。npm 10.9.9 は、読めない `~/.npmrc` があっても `npm config get registry` が通った。Python の標準の `netrc.netrc()` は `PermissionError` を投げる。macOS での影響（`~/Library/Keychains` を読ませないことが TLS の検証などに及ぼす影響）は確かめていない。

### テスト

`tests/sandbox_exec_restricted.rs` の `redirected_xdg_keeps_default_home_paths_unreadable_from_normal_bash` に行を足した。`benitoiteh exec` を通常モードの `workspace-write` で起動し、模擬の提供者が `bash` に HOME の秘密を `cat` させ、どれも読めないことと、公開のファイルを読めてプロジェクトに書けることを確かめる。足した行は、資格情報（`~/.ssh/id_ed25519`・`~/.kube/config`・`~/.config/gcloud/credentials.db`・`~/.netrc`・`~/.git-credentials`・`~/.npmrc`・`~/.env`）、保管庫（`~/.local/share/keyrings/login.keyring`）、履歴（`~/.bash_history`）、ブラウザ（`~/.config/google-chrome/Default/Cookies`）であり、ディレクトリとファイルの両方を含む。実装の一覧から写さず、例を選んだ。

作成時の関門の答えは次のとおりである。

1. 守る契約: 通常モードの `bash` は、HOME の下の資格情報などを読めない。
2. 捕まえる退行: 候補の一覧から組が抜ける、ファイルの候補とディレクトリの候補の扱いの違い。
3. 既存のテストとの違い: 同じテストが `benitoiteh` 自身の設定とデータだけを確かめていた。同じ境界に行を足し、新しいテストは加えていない。
4. 本番のコードに差し込み口を加えていない。

直す前の一覧ではこのテストが失敗し、直した後は通った。

## 外で動かすコマンドの PATH

04 の案の 3 に当たる。

### 確かめたこと

通常モードの `bash` の `escalate` は、承認を受けた一回だけ、サンドボックスの外で、継いだ `PATH` のまま動いていた（`crates/benitoiteh/src/tools/bash.rs`）。`PATH` に書ける場所（プロジェクトの `bin` や `node_modules/.bin`、相対の項目）があれば、サンドボックスの中のコマンドがそこに偽の `git` を置き、利用者が承認した `git push` の外での実行に拾わせられる。直す前のコードでは、新しいテストで、プロジェクトの `bin` に置いた偽のプログラムが外で動いた。

秘密の入力の仲介の補助（`SSH_ASKPASS`・`GIT_ASKPASS`・`SUDO_ASKPASS`）は、書ける場所の外のディレクトリに置いた、`benitoiteh` の実行ファイルへの絶対のリンクで指す（`crates/benitoiteh/src/askpass/mod.rs` の `Broker::create` と `environment`）。`PATH` に依らないので、変えていない。

### 直し方

`escalate` の一回（方針があって、サンドボックスを外す一回）の `PATH` から、書ける場所に届く項目を除く。項目を根から一段ずつ辿り、次のどれかに当たれば除く。

- 通るディレクトリが書ける場所の中にある。
- 通るディレクトリが書ける場所の根と同じもの（デバイスと inode が一致する）である。大小文字を区別しないファイルシステムの綴りの違いや、別の場所の bind マウントから根に入る道を捕まえる。
- 途中のシンボリックリンクが書ける場所の中にある。リンクは指す先も同じに辿る（nono の `crates/nono/src/broker_path.rs` と同じ考え方）。
- 相対の項目（空の項目を含む）である。作業ディレクトリに依るからである。
- 調べられない（リンクを読めない、段の数が 40 を超えるなど）。

サンドボックスの中で動かすコマンドの `PATH` は変えない。`danger-full-access` は、もともとサンドボックスがないので変えない。

### 代償

外で動かすコマンドは、プロジェクトの中のプログラム（`node_modules/.bin` の道具など）を名前だけでは起動できない。必要なら、承認の画面に出るように、パスで書く。

### テスト

`tests/tools_bash.rs` に `escalated_bash_path_skips_entries_that_reach_writable_roots` を加えた。`PATH` に、書ける場所の中の項目、書ける場所を指すリンク、途中の段がリンクの項目、まだない書ける場所の項目、相対の項目、空の項目、信頼する場所を並べ、`escalate` の一回では信頼する場所と `/bin`・`/usr/bin` だけが残って本物のプログラムが動くこと、サンドボックスの中の `PATH` は変わらないことを、本物の子プロセスで確かめる。準備（`tests/support/bash.rs`）に、`PATH` を差し替える `Fixture::with_path` を足した。

作成時の関門の答えは次のとおりである。

1. 守る契約: 承認を受けて外で動かすコマンドは、サンドボックスの中から書ける場所のプログラムを、名前で拾わない。
2. 捕まえる退行: 除き忘れ、リンクを辿らない、相対の項目を残す。
3. 既存のテストとの違い: 既存の `escalate` のテストは、環境の秘密と印だけを確かめていた。
4. 本番のコードに差し込み口を加えていない。`Fixture::with_path` はテストの準備の中だけにある。

直す前と後で、次のように確かめた。

| コード | 結果 |
|---|---|
| 除かない版 | 失敗した |
| リンクを辿らない版 | 失敗した |
| 相対の項目を残す版 | 失敗した |
| まだない場所の分岐で判定しない版 | 通った。最後の判定と重なっていたので、その分岐の判定を消した |
| 直した後 | 通った |

デバイスと inode で比べる部分は、Linux の名前の比べと同じ結果になるので、このテストでは区別できない。macOS の大小文字を区別しないファイルシステムでの確かめは、macOS での確かめとあわせて行う。

## 主のプロセスの守り

04 の案の 4 に当たる。

### 確かめたこと

`benitoiteh` は、サンドボックスの補助と画像の補助だけをダンプできない形にしていた（`crates/benitoiteh/src/sandbox/linux.rs`、`src/tools/read/image_helper.rs`）。主のプロセスはダンプできるままだった。主のプロセスの環境には提供者のキーがある。サンドボックスの外で動くコマンド（`escalate` の一回と `danger-full-access`）は同じ利用者で動くので、`/proc/$PPID/environ` から提供者のキーを読めた。直す前のコードでは、新しいテストでこの読み取りが通った。

### 直し方

Linux では、主のプロセスの入口（`crates/benitoiteh/src/cli/mod.rs` の `main`）で、補助の入口を分けた後、ほかの処理より先に `PR_SET_DUMPABLE` を 0 にする。できなければ起動しない。ダンプできない形は `execve` で起動したプログラムには継がれないので、利用者のプログラムのコアのダンプは変わらない。コアのダンプの上限（`RLIMIT_CORE`）は子に継がれるので使わなかった。

04 の案は「コアのダンプと ptrace の接続を止める」としていた。Linux では、ダンプできない形にすることで、`/proc/<pid>/environ`・`mem` の読み取りと ptrace の接続の両方を、同じ利用者のほかのプロセスに対して拒む。

macOS では手当てしていない。同じ利用者のほかのプロセスが主のプロセスの環境や記憶を読めるかと、`unsafe` を書かずに防ぐ方法を、BH-118（【要検証】）とした。

### テスト

`tests/sandbox_exec_restricted.rs` に、Linux だけの `unsandboxed_bash_cannot_read_the_main_process_environment` を加えた。`benitoiteh exec` を通常モードの `danger-full-access` で起動し、模擬の提供者が `bash` に `/proc/$PPID/comm` と `/proc/$PPID/environ` を読ませる。親が `benitoiteh` であること（別の理由で通らないため）、環境を読めないこと、提供者のキーが結果に出ないことを確かめる。

作成時の関門の答えは次のとおりである。

1. 守る契約: サンドボックスの外で動くコマンドも、主のプロセスの環境（提供者のキー）を読めない。
2. 捕まえる退行: 主のプロセスの守りの抜け、入口の分け方を変えて守りの前に処理が走る形。
3. 既存のテストとの違い: 既存のテストは、子の環境から提供者のキーを除くことだけを確かめていた。親の環境から読む道は確かめていなかった。
4. 本番のコードに差し込み口を加えていない。

直す前のコードではこのテストが失敗し（親の環境を読めた）、直した後は通った。

## .agents の守り

04 の案の 6 に当たる。設計者が次の三つを決めた（2026-11-02）。

- 守る範囲は `.agents` の全体とする（スキルのほか、ほかのハーネスが読む `hooks.json` などの設定も含む）。
- Benitoite のスクリプトが `.agents` に書くときは、LLM が道具の引数 `write_agents` で求め、利用者が一回ごとに確認する。確認は覚えない。
- 非対話（`exec`）では、新しいオプション `--allow-agents-write` を付けたときだけ許す。

設計者は、プロジェクトのスキルを直せなくなると、Benitoite のスクリプトでスキルを作れなくなり、試作の目的の一つを果たせなくなると指摘した。そこで、守ったうえで、承認で書けるようにした。

### 直し方

| 場所 | 変更 |
|---|---|
| `src/sandbox/policy.rs` | 守る名前に `.agents` を加えた。サンドボックスの中からは、根の直下の `.agents` にあってもなくても書けず、深いところのすでにある `.agents` にも書けない |
| `src/sandbox/restricted.rs`、`src/tools/benitoite_process.rs` | `RestrictedAccess` に `write_agents` を加え、真で `File.Write` があるときだけ、その一回の方針の守る名前から `.agents` を外す |
| `src/tools/benitoite_run.rs`、`benitoite_test.rs`、`definitions.rs`、`text.rs` | `benitoite_run`・`benitoite_test`・`benitoite_scratch` に引数 `write_agents` を加えた。真のときは、前に同じエフェクトを確認していても毎回確認し、`File.Write` がなければ確認の前に断る。`benitoite_scratch` は確認なしの範囲に収まっても確認する |
| `src/tools/write_path.rs` | `write`・`edit` は、`.agents` の下に書くたびに確認を求める（制限モードでも、`.bnt` だけに書けることは変わらない） |
| `src/approval/` | 確認の事実に `agents_write` を加え、要約に `.agents` への書き込みを示す。制限モードでも、`.agents` の `.bnt` への書く道具の確認を通す |
| `src/agent/execution.rs`、`src/cli/args.rs`、`src/cli/exec.rs` | `exec --allow-agents-write` を加えた。非対話では、付けたときだけ `.agents` への書き込みを許し、付けないときは理由を道具の結果で返す。対話の形では使い方の誤りとする |
| `src/ui/tui/confirmation.rs` | 確認の画面に、この一回だけ `.agents` に書くことを別の行で示す |

通常モードの `bash` からは、`.agents` に書くコマンドを `escalate` の承認を受けて外で動かす（新しい仕組みは足していない）。

### テスト

| テスト | 確かめること |
|---|---|
| `sandbox_benitoite_run.rs` の `agents_is_written_only_by_runs_confirmed_with_write_agents`（追加） | 本物の処理系とサンドボックスで、`File.Write` を確認したスクリプトも `.agents` に書けないこと、`write_agents` を付けると前の確認があっても確認を求め、承認の後に書けること、`File.Write` のないスクリプトの `write_agents` は確認の前に断ること |
| `sandbox_benitoite_test.rs` の `tests_write_agents_only_after_a_write_agents_confirmation`（追加） | テストの実行の同じ契約。`benitoite_test` は確認の判定を別に持つので、別に確かめる |
| `tools_write.rs` の `restricted_writes_to_agents_scripts_need_one_time_approval`（追加） | 制限モードの書く道具が、`.agents` の `.bnt` に確認を求め、承認の後に書けること、`.bnt` でないファイルは確認の前に拒むこと |
| `tools_write.rs` の `normal_mode_requests_approval_before_any_mutation_and_accepts_trusted_approval`（行を足した） | 通常モードの書く道具が、`.agents` と `sub/.AGENTS` の下に確認を求め、確認の事実に `agents_write` を載せること |
| `sandbox_exec_restricted.rs` の `exec_writes_agents_only_with_allow_agents_write`（追加） | `exec` の入口で、`--allow-agents-write` を付けたときだけ書けること、付けないときは理由を返すこと |
| `sandbox_linux.rs` の `root_protection_blocks_creation_writing_removal_and_replacement`（行を足した） | 本物の Linux のサンドボックスで、根の直下の `.agents` を、作る・書く・消す・付け替えることができないこと |
| `args.rs` の `exec_arguments_reject_duplicates_invalid_values_and_unavailable_options`（行を足した） | `--allow-agents-write` を受け付け、値の付いた形・重複・対話の形を拒むこと |

既存のテストのうち、守る名前の一覧と引数列の期待（`normal_setup.rs`、`src/sandbox/linux/mounts/tests.rs`）を `.agents` に合わせて直した。LLM に見せる道具の定義が変わったので、期待のファイル（`tests/expected/tools-restricted.json`、`tools-restricted-read-plain.json`）を `examples/update_context_expected.rs` で作り直し、差分が三つの道具の `write_agents` の説明の追加だけであることを確かめた。

一部を壊した版で、次のように確かめた。

| 壊した箇所 | 失敗したテスト |
|---|---|
| 守る名前から `.agents` を外す | `agents_is_written_only…`、`root_protection…` |
| `write_agents` の実行で確認を省く（`benitoite_run`） | `agents_is_written_only…` |
| `write_agents` の実行で確認を省く（`benitoite_test`） | `tests_write_agents_only…` |
| 承認した一回でも方針から `.agents` を外さない | `agents_is_written_only…` |
| 非対話の判定を外す | `exec_writes_agents_only…` |
| 制限モードの書く道具が `.agents` を確かめない | `restricted_writes_to_agents…` |
| 通常モードの書く道具が `.agents` を確かめない | `normal_mode_requests_approval…` |

`benitoite_scratch` の `write_agents` は `benitoite_run` と同じ処理を通るので、別のテストを加えていない。macOS の Seatbelt で `.agents` を守ることは、macOS での確かめとあわせて確かめる。

### 代償

- LLM が `.agents` に書くたびに確認が出る。非対話では `--allow-agents-write` が要る。
- 通常モードの `bash` から `.agents` に書くには、`escalate` で外で動かす。
- 子プロセスを起動するたびに、根の直下にまだない `.agents` を塞ぐ空のディレクトリが一時的にでき、子の終了の後に消える（`.git` と `.benitoiteh` と同じ）。

## 判断待ちの 3 件の決定

設計者が 2026-11-02 に、次のとおり決めた。

| 件 | 決定 |
|---|---|
| BH-100（プロジェクトの中の `.env*`） | 制限モードだけを塞ぐ。通常モードの `bash` は塞がない |
| シェルの起動の設定 | `~/.zshenv` を除いて読ませない |
| BH-118（macOS の主のプロセス） | macOS の実機で確かめてから対策を決める |

### プロジェクトの中の秘密の名前（BH-100）

`crates/benitoiteh/src/sandbox/restricted.rs` の `restricted_policy` で、プロジェクトの中の、道具が読まない名前（`.env`・`.env.*`（例と見本を除く）、鍵と証明書の形、`.ssh`・`.gnupg`・`.aws`・`.netrc`・`.git-credentials`）のファイルとディレクトリを、読ませない場所に加える。名前の判定は道具と同じ関数（`src/tools/exclude.rs` の `secret_component`）を使う。

- 探す深さは、入れ子の守る場所と同じく根から 3 段（`a/b/.env` まで）とし、`.git` は辿らない。
- 名前がシンボリックリンクなら、指す先を塞ぐ。通常のファイルとディレクトリでない先は塞がない。
- Benitoite のソース（`.env.bnt` など）は塞がない。最初の版は塞いでいたが、全体のテストで、`tests/.env.bnt` のテストが動かなくなり、処理系の「読めない」という診断にそのパスが載ることが分かった（`sandbox_benitoite_test.rs::excluded_test_names_values_output_and_diagnostics_never_reach_results_or_spill`、`sandbox_exec_restricted.rs::restricted_check_limit_emits_full_safe_diagnostics_and_flat_summary` が失敗した）。このようなソースは、取り込みの確認と、診断を件数だけ示す今までの規則で扱う。スクリプトが `File.Read` で `.env.bnt` を読むことは防がない。
- 利用者が `read.exclude` で足した規則は、まだ加えていない。
- 問い合わせ・実行・テスト・一時的なスクリプトの、制限モードのどの起動にも効く。

問いの選択肢では「macOS はどの深さも」と示したが、macOS も Linux と同じく探す形にした。Seatbelt の正規表現の規則は、macOS で確かめられないまま入れると、壊れた規則で制限モードの起動がすべて止まるおそれがあるからである。macOS での確かめのときに、正規表現の形に改めるかを決める。

代償として、`.env` を読む正当なスクリプトは制限モードでは動かず、塞いだファイルには書けない。子プロセスが動いている間に作られたファイルと、4 段より深いファイルは塞がない。

テストは、`tests/sandbox_benitoite_run.rs` に `restricted_scripts_cannot_read_project_secret_names` を加えた。本物の処理系とサンドボックスで、`File.Read` を確認したスクリプトが `.env`・`config/.env.local`・`a/b/id_ed25519`・`keys/server.pem` を読めず（`PermissionDenied`）、`.env.example`・`notes.txt`・`.env.bnt` と 4 段目の `a/b/c/.env` は読めることを確かめる。探さない版と、根の直下だけを探す版では、このテストが失敗した。

### シェルの起動の設定

読ませないホームの場所に、`~/.bashrc`・`~/.bash_profile`・`~/.bash_login`・`~/.bash_logout`・`~/.profile`・`~/.zshrc`・`~/.zprofile`・`~/.zlogin`・`~/.zlogout`・`~/.config/fish` を加えた。`~/.zshenv` は、zsh が対話でない起動でも読み、`PATH` の設定に使われるので、読めるままにした。

判断の前に、root でない利用者で確かめた（Alpine 3.22.7 の bash と dash）。

- `bash -c`: 起動の設定を読まないので、影響はなかった。
- `bash -l`・`bash -i`: `Permission denied` の警告を出して続いた。
- `sh -l`（dash）: 警告なしで続いた。

前の節で「ログインのシェルが失敗する」としたのは誤りで、実際は警告を出して続き、その設定の `PATH` などが効かなくなる。

テストは、`redirected_xdg_keeps_default_home_paths_unreadable_from_normal_bash` に `~/.bashrc`・`~/.profile`・`~/.zshrc`・`~/.config/fish/config.fish` の行と、`~/.zshenv` が読めることの行を足した。`~/.zshrc` を外した版と、`~/.zshenv` も読ませない版で、このテストが失敗した。

### macOS の主のプロセス（BH-118）

BH-118 は【要検証】のまま残し、設計者の決定を書き足した。macOS で確かめるときに、同じ利用者のほかのプロセスから主のプロセスの環境（`KERN_PROCARGS2` など）と記憶を読めるかを試し、対策を決める。
