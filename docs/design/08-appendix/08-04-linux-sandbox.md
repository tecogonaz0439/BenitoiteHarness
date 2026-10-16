# Linux のサンドボックスの検討の記録

本付録は、Linux で、読ませない場所と、書ける場所の中の守る場所（`.git` と `.benitoiteh`）をどう実現するか（BH-049）の検討を記録する。2026-10-18 に設計者が決めた。決めた内容は、[OS のサンドボックス](../03-safety/03-02-sandbox.md)の「サンドボックスの掛け方」「Linux のファイルシステムの見え方」と、[セキュリティモデル](../03-safety/03-01-security-model.md)の「Linux での保証の範囲」に書く。

## 元の条件と候補

2026-10-16 の設計書は、Linux の形が満たすべき条件を次のとおり置いていた。

- 読ませない場所は、読み取りそのものが失敗する形で隠す。
- 守る場所は、書ける場所の下のどの深さでも、まだないときに作ること、ほかの名前に付け替えて同じ名前で作り直すこと、`.git` をファイル（`gitdir:` の行）として作ることも拒む。名前は ASCII の大文字と小文字を区別せずに比べる。

候補は、ユーザとマウントの名前空間で見える木を組み直す形と、Landlock で表せる範囲に規則を狭める形だった。試作は、bubblewrap が外部のプログラムであることを理由に、Landlock と seccomp だけを使う方針にしていた。名前空間を自分で作るには、`rustix` の `unsafe` の関数（`unshare_unsafe`）が要る。

macOS の Seatbelt は、正規表現の規則でこの条件を満たせる。2026-10-17 に、試作の P0-04 で確かめた（03-02 の「方針の要素」）。

## Landlock だけの形の試作の結果

試作の P0-05 で Landlock だけの形を作った。2026-10-18 に、x86_64 の Linux 7.2.9（Landlock の ABI の版 10）の rootless の podman の中で、テストで確かめた。分かったことは次のとおりである。

- **読ませない場所**: 根から辿って、読ませない場所を含まない部分木ごとに読み取りの権利を与える形で隠せた。ただし、読ませない場所の祖先のディレクトリは、一覧を読めなくなる。
- **守る場所**: 書ける場所の中で、すでにある通常のファイルを更新する権利だけを与える形でしか守れない。ディレクトリにファイルを作る権利を与えると、その下のどの深さにも、どの名前でも作れるので、まだない `.git` を作ることも許してしまう。この形では、書ける場所（一時ディレクトリを含む）に新しいファイルとディレクトリを作ること、消すこと、一時のファイルに書いてから名前を付け替えることができない。規則は、すでにあるファイルの一つずつに足すので、ファイルの多いプロジェクトでは規則の数も多くなる。
- **見つかった誤り**: 試作の最初の実装は、読み取りの規則に `landlock` クレートの `AccessFs::from_read` を使っていた。この関数は実行の権利も含むので、読める場所のプログラムを、実行を許すプログラムの一覧になくても起動できた。読み取りの規則には実行の権利を含めないと、03-02 に定めた。

この形では、制限モードで `File.Write` を確認したスクリプトが新しい出力のファイルを作れない。通常モードのシェルのコマンド（`mkdir`、出力を作るコンパイラ、`npm install`、一時のファイルに書いてから付け替えて保存するプログラム）も動かない。サンドボックスとして実質使えないと判断した。

この制約は、試作の作り方の誤りでなく、Landlock の規則の性質から来る。Landlock の規則は許可だけを並べる形であり、一つの規則はディレクトリの下全体に及ぶ。名前で例外を作ることはできない（[Landlock の文書](https://docs.kernel.org/userspace-api/landlock.html)）。

## 条件の見直し

設計者は、守る場所の条件そのものが強すぎないかを問うた。検討した点は次のとおりである。

- `.git` を守る理由は、hooks と `config`（`core.hooksPath`、`core.fsmonitor` など）を書き込まれると、利用者が後で git を使ったときに、サンドボックスの外でプログラムが動くことである。`.git` の中身は `git diff` と `git status` に出ないので、利用者が見直しで気付けない。この理由は、すでにある `.git` を守ることを強く支える。
- 同じ種類の危険は `.git` の外にもある。サンドボックスの中のプログラムがプロジェクトに書ければ、`Makefile`、`package.json` の scripts、`build.rs`、`.cargo/config.toml` の runner、`.envrc`、エディタの設定など、利用者が後で走らせるものに仕掛けられる。`.git` だけを、どの深さでも、まだなくても守っても、「後でサンドボックスの外で動かされる」危険の全体は塞げない。
- 新しく作られた入れ子の `sub/.git` は、外側のリポジトリの `git status` に追跡していないディレクトリとして現れる。
- 制限モードでは、スクリプトがプロジェクトに書けるのは、利用者が `File.Write` を確認したときだけである。
- 次節のとおり、「どの深さでも」「まだなくても」「大文字と小文字を区別せずに」の三つを同時に満たすハーネスは、調べた中になかった。

## ほかのハーネスの作り

次のコミットのソースを、2026-10-18 に浅い複製で読んで確かめた。ビルドも実行もしていない。

| ハーネス | リポジトリ | コミット |
|---|---|---|
| Codex CLI | <https://github.com/openai/codex> | `859bbc87fc80c18a72e08b181e301ddb4f8dabea`（2026-10-16） |
| Claude Code の sandbox-runtime | <https://github.com/anthropic-experimental/sandbox-runtime> | `626a59671ee2be556d70a290a8036224be996193`（2026-10-16） |
| Gemini CLI | <https://github.com/google-gemini/gemini-cli> | `ed96faedfd5571f055c44f2c9509d62321d1138d`（2026-10-16） |
| DeepSeek Harness | <https://github.com/deepseek-ai/deepseek-harness> | `1517a4dbca0a2789ff1d41ba439600f8547d23fa`（2026-10-16） |

Claude Code のサンドボックスの文書（<https://code.claude.com/docs/en/sandboxing>）も、2026-10-18 に読んだ。

| ハーネス | Linux の仕組み | まだない `.git` | 入れ子の `.git` | 名前の比べ方 |
|---|---|---|---|---|
| Codex CLI | bubblewrap と seccomp | 書ける場所の根の直下で塞ぐ | 守らない | 大文字と小文字を区別する |
| Claude Code の sandbox-runtime | bubblewrap と seccomp | 塞がない（`git init` を妨げないため） | すでにあるものを、深さ 3 まで探して守る | 区別しない |
| Gemini CLI | bubblewrap（道具ごとのサンドボックス。既定では使わない） | 実行の前に空の `.git` を作ってから、読み取りだけにする | 守らない | — |
| DeepSeek Harness | bubblewrap か Landlock | 守らない | 守らない | — |

### Codex CLI

- **仕組み**: bubblewrap が名前空間とファイルシステムの見え方を作る。その中で起動し直した自分が、`no_new_privs` と seccomp を掛ける（`codex-rs/linux-sandbox/src/bwrap.rs`）。
- **bubblewrap の入手**: `PATH` の上で作業ディレクトリの外にある `bwrap` を使い、なければ同梱の `codex-resources/bwrap` を使って、起動のときに注意を示す（`codex-rs/linux-sandbox/README.md`）。
- **Landlock**: 隠した指定（`--use-legacy-landlock`）でだけ使う。ファイルシステムを制限する方針では、`filesystem-restricted execution requires bubblewrap to isolate app-server sockets` として拒む（`codex-rs/linux-sandbox/src/linux_run_main.rs`）。古い経路の Landlock の規則には、守る場所の除外がない。
- **守る名前と比べ方**: 守る名前は `.git`・`.agents`・`.codex`・`.aws` で、書ける場所ごとに根の直下だけを守る。名前は完全一致で比べる（`codex-rs/protocol/src/permissions.rs` の `PROTECTED_METADATA_PATH_NAMES`）。
- **すでにあるとき**: 読み取りだけで重ねる（`--ro-bind`）。`.git` が `gitdir:` の行のファイルなら、指す先も重ねる。
- **まだないとき**: `--perms 555 --tmpfs <場所> --remount-ro <場所>` で、読み取りだけの空のディレクトリをマウントする。サンドボックスの外に作られたマウントの場所は、実行の後に、空のまま残っていれば消す。並んで動くサンドボックスのために、作った場所の一覧をロック付きのファイルで持つ（`codex-rs/linux-sandbox/src/linux_run_main.rs`）。
- **書ける場所の中のシンボリックリンク**: 守る場所に至るパスにあれば、サンドボックスを作らずに失敗とする。リンクの今の指す先を覆っても、中のプロセスがリンクを差し替えられるからである（`codex-rs/linux-sandbox/src/bwrap.rs`）。
- **読ませない場所**: ディレクトリは権限 000 の tmpfs で、ファイルは権限 000 の空のファイル（`--ro-bind-data`）で覆う。

### Claude Code の sandbox-runtime

- **仕組み**: bubblewrap と、自前の seccomp の補助を使う。補助は、Unix ドメインのソケットの作成と `io_uring` を拒み、名前空間を作る呼び出しと `mount` も拒む（`vendor/seccomp-src/`、`README.md`）。README は、Linux の書き込みの拒否はすべて読み取りだけの bind mount であり、マウントはそれを作った名前空間の中でだけ守るので、中のコマンドに名前空間を作らせないと説明する。
- **Landlock**: 使わない。
- **bubblewrap がないとき**: 誤りとして示す（`src/linux-sandbox-utils.ts`）。Claude Code の文書は、`failIfUnavailable` を指定すると、bubblewrap などがないときに起動しないと説明する。
- **既定で拒む場所**: シェルの起動のファイル、`.gitconfig`、`.vscode`、`.idea` などである（`src/sandbox-utils.ts`）。`.git` は、git の操作のために書ける必要があるとして一覧から除き、中の `hooks` と `config` だけを拒む。
- **まだない `.git`**: 塞がない。ソースの注釈は、`.git` にマウントすると `git init` で作れなくなるからとする（`src/linux-sandbox-utils.ts`）。Claude Code の文書は、実行の間に作業ディレクトリの根に現れた `HEAD`・`objects`・`refs`（bare のリポジトリの形）を消すと説明する。この処理は公開のソースにはない。
- **入れ子のリポジトリ**: `HEAD` を持つものを ripgrep で深さ 3（設定で 1〜10）まで探し、大文字と小文字を区別せずに比べる。
- **付け替え**: 守る場所と書ける場所の根の間のディレクトリを、それ自身にマウントして、付け替えと削除を拒む（`src/linux-sandbox-utils.ts`）。

### Gemini CLI

- **仕組み**: 主な隔離は、プロセス全体を動かすコンテナ（docker・podman・gVisor・lxc）か、macOS の Seatbelt である。道具ごとの bubblewrap のサンドボックス（`security.toolSandboxing`）もあるが、既定では使わない。
- **Landlock**: 使わない。
- **seccomp**: `ptrace` と、CPU の種類の違う呼び出しだけを拒む。
- **守るファイル**: `.git` と `.gitignore`・`.geminiignore` を「守るファイル」とする。まだなければ、実行の前に実際に作ってから、読み取りだけで重ねる（`packages/core/src/sandbox/linux/LinuxSandboxManager.ts` の `ensureGovernanceFilesExist`）。守るのは作業の根の直下だけである。
- **git のコマンドでの例外**: コマンドの文字列に `git` を含むときは、`.git` の全体を書けるようにする（同じファイル）。

### DeepSeek Harness

- **仕組み**: bubblewrap を試し、使えなければ静的リンクの Landlock の起動の補助（ABI の版 5 まで）を使い、どちらも使えなければ実行を拒む（`packages/sandbox/sandbox-local/src/index.ts`）。
- **限る範囲**: ファイルの書き込みだけである。`.git` を守る仕組み、読ませない場所、ネットワークの制限はない。文書もそう説明する（`packages/shell/bash-sandbox/README.md`）。

### まとめ

調べた中に、書ける場所の中の守る場所を Landlock で表すハーネスはなかった。Landlock を使うのは、DeepSeek Harness の代わりの経路と、Codex CLI の古い経路だけである。Codex CLI は、守る場所のある方針では Landlock を使わない。

## 検討した形

| 形 | 守れる範囲 | 代償 | 扱い |
|---|---|---|---|
| Landlock だけ（P0-05 の形） | 元の条件 | 書ける場所に新しいファイルを作れない | 採らない |
| Landlock だけで、根の直下の子ごとに権利を与える（P0-05 の作業の文書の候補） | 根の直下の守る場所 | 根の直下に新しいファイルを作れない。入れ子の `.git` は作れる | 採らない |
| `benitoiteh` が名前空間を作る | Codex CLI と同じ水準 | `unsafe` の例外が要る。bubblewrap の一部を作り直すことになる | 採らない |
| 利用者が導入した bubblewrap | Codex CLI と同じ水準 | 利用者の導入が要る。なければ Linux の制限モードは動かない | 採る |
| bubblewrap を同梱する | Codex CLI と同じ水準 | 配るものと版の管理が増える。試作の間は配布物を作らない | 採らない。試作の後に配るなら、改めて検討する |
| Linux の制限モードを試作の範囲から外す | — | Linux で制限モードを確かめられない | 採らない |

## 決めたこと

2026-10-18 に、設計者が、守る場所の条件を Codex CLI の水準に合わせると決めた。決めたことは次のとおりである。

- **Linux の条件**: 書ける場所の根の直下の `.git` と `.benitoiteh` は、あってもなくても守る。それより深い場所は、子プロセスを起動するときにすでにあるものだけを守る。探す深さは、Claude Code の sandbox-runtime の既定に倣って 3 段とする。
- **macOS の条件**: 今の Seatbelt の規則（どの深さでも、まだなくても、大文字と小文字を区別せずに）を保つ。
- **道具の規則**: LLM の道具（`write`・`edit`）は、どの OS でも、どの深さの守る場所にも書かない。
- **Linux の仕組み**: 利用者が導入した bubblewrap でマウントを作る。そのうえで、Landlock で起動できるプログラムを限り、seccomp でソケットなどを拒む。

この決定は、Linux の守る場所の性質を macOS より狭めるものである。狭めた範囲は、03-01 の「Linux での保証の範囲」に書き、`doctor` と制限モードの起動のときの表示で示す。
