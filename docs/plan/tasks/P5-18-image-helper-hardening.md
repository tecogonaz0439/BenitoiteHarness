# P5-18 画像の補助のプロセスの締め直しと、後始末の失敗で起動を止めないこと

## 目的

P5-15 のセキュリティのレビューの所見（low と info）を直す。設計書 01-03「画像のファイルの読み取り」の「サンドボックス」と、01-05「記録を残す期間」を、2026-10-19 に次のとおり改めた。

1. 画像の補助（`benitoiteh __image`）に書ける場所を持たせない。補助ごとの作業ディレクトリをなくす（Linux で、作業ディレクトリごとに `placeholders/` に錠のファイルが増えて消えないことも、これで解消する）。
2. 補助は子のプロセスを作れず（`fork` を許さず、`RLIMIT_NPROC` を 0）、ファイルを書けず（`RLIMIT_FSIZE` を 0）、コアのダンプを残さない（`RLIMIT_CORE` を 0）。
3. 補助は、標準出力に書き終えたら明示して `flush` し、失敗すれば 0 以外で終わる。
4. 起動のときの古いセッションの削除（`session::cleanup`）が失敗しても、起動を止めず、消せなかったことを示して続ける。

この作業で本物の LLM の提供者を呼ばない。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「画像のファイルの読み取り」（「サンドボックス」の項目）
- [セッション](../../design/01-agent/01-05-sessions.md)の「記録を残す期間」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「方針の要素」「Linux のファイルシステムの見え方」
- 既存のもの: `crates/benitoiteh/src/sandbox/restricted.rs`（`image_policy`）、`src/sandbox/policy.rs`（`PolicyConfig` の検証。書ける場所を空にできない規則）、`src/sandbox/macos.rs`（`(allow process-fork)` を出す所と、書ける場所の規則）、`src/sandbox/linux/`（書ける場所の bind、守る名前の錠）、`src/tools/read/image_process.rs`（`Workdir`、起動、後始末）、`src/tools/read/image_helper.rs`（出力、`RLIMIT_AS`）、`src/session/mod.rs`（`cleanup`）、`src/cli/exec.rs`（347 行あたりの `session::cleanup(...)?`）

## この作業が認める型の変更

- `PolicyConfig` に、書ける場所を持たない方針を表す形を加える変更（例: 書ける場所を空にしてよい印、または画像の補助のための専用の方針の型）。今の方針（書ける場所を空にできない）は、ほかの用途ではそのまま保つ。
- macOS の方針の生成で、`process-fork` を許すかを方針ごとに選べるようにする変更。

## 作るもの

### 1. 書ける場所のない方針

- 画像の補助の方針は、書ける場所を持たない。macOS では `file-write*` を一つも許さず、Linux では書ける場所の bind（と守る名前の錠と塞ぐ場所）を作らない。Landlock の書き込みの権利も与えない。
- 補助の作業ディレクトリ（`image-<乱数>`）を作らない。補助の作業ディレクトリ（`cwd`）は、補助が読めなくてもよい既存のディレクトリ（例: `/`）とし、補助はファイルを開かない。
- `cleanup` するものがなくなるので、`Workdir` とその後始末を除く。

### 2. 補助の資源の上限

- 補助の入口で、`RLIMIT_AS`（Linux だけ、今のまま）に加え、両方の OS で `RLIMIT_NPROC`・`RLIMIT_FSIZE`・`RLIMIT_CORE` を 0 にする（`rustix::process::setrlimit`。下げる値なので掛けられないことは想定しないが、失敗したら誤りで終わる）。
- macOS の画像の補助の方針では `(allow process-fork)` を出さない。ほかの方針は今のまま。

### 3. 出力の書き出し

- 補助の書き込みの後に `flush` を加え、失敗したら標準出力に追加で書かずに 1 で終わる（主のプロセスは、終了状態が 0 でない出力を捨てる）。

### 4. 後始末の失敗で起動を止めない

- `session::cleanup` は、個々のセッションのディレクトリと一時ディレクトリの削除に失敗しても、ほかの削除を続け、失敗したものの一覧を返す。起動は止めず、消せなかったセッションの ID と短い理由を画面（対話の画面の知らせと、`exec` の標準エラー出力の一行）に示す。記録の大きさの数え（10 GiB の上限）は、残ったものも数える。
- 削除の深さと件数に上限を掛ける必要はない（標準の `remove_dir_all` のままでよい）が、深い入れ子でスタックが溢れないよう、削除は今どおり主のスレッド（既定のスタックの大きさ）で行う。【要検証】深さ数千の入れ子で `remove_dir_all` がスタックを溢れさせるかは、レビューで macOS の 2 MiB のスタックで深さ 1000 で溢れたと報告された。主のスレッドのスタック（macOS 8 MiB）で深さ 3,000 を消せるかを試し、溢れるなら、削除を明示の大きいスタック（例: 64 MiB）のスレッドで行う。

## 触るファイル

- `crates/benitoiteh/src/sandbox/policy.rs`、`src/sandbox/restricted.rs`、`src/sandbox/macos.rs`、`src/sandbox/linux/`（要れば）
- `crates/benitoiteh/src/tools/read/image_process.rs`、`src/tools/read/image_helper.rs`
- `crates/benitoiteh/src/session/mod.rs`、`src/cli/exec.rs`、`src/ui/`（知らせの文言）
- テスト: `tests/sandbox_read_image.rs`、`tests/image_helper.rs`、`tests/sandbox_macos.rs`・`tests/sandbox_linux.rs`（方針の生成のテストがあれば）、`tests/session.rs`（後始末）

## テスト

- 画像の補助の方針: 補助の中から、どの場所にも書けないこと（`sandbox-probe` の書き込みで確かめる。macOS と Linux）。`fork` できないこと（macOS は方針で、両方の OS で `RLIMIT_NPROC` で）。縮める処理は今までどおり成功すること。
- Linux: 縮めても `placeholders/` に錠のファイルが増えないこと。
- 補助: 正しい入力で JSON と中身を書き、`flush` まで済ませて 0 で終わること（今のテストを保つ）。
- 後始末: 消せないもの（`chmod 000` の子ディレクトリなど）を含む期間切れのセッションがあっても、起動が成功し、消せなかったことが示され、ほかの期間切れのセッションは消されること。深さ 3,000 の入れ子を含む期間切れのセッションを消せること（スタックが溢れないこと）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。サンドボックスのテストは、オーケストレータが外で走らせる全体の検査で確かめる。
- 完了の報告に、画像の補助の macOS の方針の文（書き込みと `fork` の規則がないこと）を書く。

## 補足（2026-10-19、始める前の点検）

本節は、上の各節に優先する。行の番号は `main`（`a709cc6`）のもの。設計書 01-05「一時ディレクトリ」の削除を、再帰しない形に改めた（下の 6）。

1. **書ける場所のない方針の型**: 空の書ける場所を拒むのは `src/sandbox/policy.rs:101-103`（`EMPTY_WRITABLE`）だけで、`src/sandbox/tests.rs:76-78` がそれを確かめている（既定の形では保つ）。`PolicyConfig` に二つの欄を加える。`require_writable: bool`（`new()` で真、serde の既定も真）は、真なら書ける場所が空でないこと、偽なら空であることを求める。`allow_fork: bool`（`new()` で真、serde の既定は付けない）は、macOS で `(allow process-fork)` を出すかを決める。Linux の補助は方針を JSON で受けて検証し直す（`src/sandbox/linux.rs:76-96`、`deny_unknown_fields`）ので、両方の欄を serde で通す。`PolicyConfig::without_writable()` の形の作り方を加え、`image_policy(exe)`（`src/sandbox/restricted.rs:127-139`）はそれを使って `workdir` の引数をなくす。`writable` を辿る所（macOS の `seatbelt_profile` の 113〜132 行、Linux の `mounts::build` の 92〜113 行、Landlock の `filesystem.rs` の 47〜77・113〜115 行、`process.rs:58-66`、`policy.rs:153-158`）は空の一覧をそのまま扱える。書ける場所が空なら、`Store::prepare`（`mounts.rs:69`）を呼ばない。
2. **`/dev/null`**: 作るもの 1 の「`file-write*` を一つも許さず」「書き込みの権利も与えない」は、`/dev/null` への書き込みを除く（設計書 03-02 のとおり、どの方針でも許す。`macos.rs:61-63` と Landlock の規則）。
3. **macOS の `process-fork`**: `src/sandbox/macos.rs:46-49` の定数の見出しから `(allow process-fork)` を外し、`config.allow_fork` が真のときだけ出す（隣の注釈も一緒に移す）。補助の起動の鎖（`__session` の `exec`、`sandbox-exec`）は `fork` を要しない（点検で、`fork` を許さない方針の下で Rust の実行ファイルが起動し、`Command::spawn` が `EPERM` になることを確かめた）。
4. **資源の上限**: `rustix::process::setrlimit` の `Resource::Nproc`・`Fsize`・`Core` は、今の `process` の機能で両方の OS にある。`convert`（`image_helper.rs:62-65`）の先頭で、両方の OS で三つを 0 にする関数（例: `limit_resources()`）を呼ぶ。`limit_memory()` は Linux だけのまま。両方の OS で使う誤りの種類と `text` の定数（例: `limits`）を加える。Linux では `RLIMIT_NPROC` がスレッドも数えるので、上限を掛けた後にスレッドを作らないことを注釈に書く（今の補助はスレッドを作らない）。Linux では加えて、`__image` の中で `rustix::process::set_dumpable_behavior(NotDumpable)` を呼ぶ（`execve` で `__sandbox` の設定が戻るため。パイプの形の `core_pattern` には `RLIMIT_CORE` が効かない）。
   - `RLIMIT_FSIZE=0` の後、通常のファイルへの一バイト以上の書き込みは、`SIGXFSZ` でプロセスを終わらせる（標準エラー出力が通常のファイルのときも）。本番の標準エラー出力はパイプなのでよいが、上限を確かめる子のプロセスのテストは、`.status()` ではなく `.output()`（パイプ）で起動する。「ファイルを作れない」ことは確かめられない（空のファイルは作れる）。macOS で確かめた値: `setrlimit` は成功し、`fork` は `EAGAIN` になり、スレッドは作れ、パイプへの 4 MiB の書き込みは通る。
5. **作業ディレクトリをなくす所**: `image_process.rs:42-55`（`Workdir` の作成と後始末）、`launch` の `work` の引数（57〜66 行）と `cwd: work.into()`（81 行。`PathBuf::from("/")` にする。補助は作業ディレクトリを読まない）、`struct Workdir`（231〜255 行）と `DirBuilderExt` の import、`ImageHelperContext.work_parent`（`src/tools/mod.rs:416-417`。`src/cli/exec.rs:560` と `tests/support/image_resize.rs:72` で設定）、`image_policy` の呼び出し（`image_process.rs:66`、`tests/sandbox_read_image.rs:87`）。テストの直し:
   - `image_process.rs` のテストの `Fixture`（297、305〜313、392、483 行）は `Workdir` を使うので、テストだけの小さな構造体（`Drop` で消す）に替える。テストの直の起動の `.current_dir(work)`（97 行）は `/` か準備のディレクトリにする。
   - `cancellation_stops_running_helper_and_cleans_its_directory`（490〜516 行）は作業ディレクトリに `ready` を書いているので、`private` の外の絶対のパスに印を書く形に改め、名前を改める。`cleanup_does_not_follow_links_created_by_helper`（518〜534 行）は意味がなくなるので除く。`private` が空であることの確かめ（391、461 行）は「作業ディレクトリを作らない」として残す。
   - `tests/sandbox_read_image.rs` の `image_policy_denies_home_reads_and_network_connections`（51〜126 行）は、`probe-work` をやめて `image_policy(&probe)` と `cwd = /` にし、`sandbox-probe` の `write` でセッションの `sandbox/` の下、セッションの一時ディレクトリ、準備のディレクトリに書けないこと（両方の OS で終了状態 10）と、`exec` で子を起動できないこと（macOS だけ 10）を加える。Linux の `fork` できないことは、4. の上限の関数の単体のテストで確かめる。`failed_resize_does_not_store_or_record_and_removes_workdir`（128〜167 行）は名前を改める。Linux の錠のファイルは、縮める前と後の `placeholders` の項目の数を比べて確かめる。
6. **再帰しない削除**: 点検の試しで、標準の `remove_dir_all` は深さごとに約 2.2 KiB のスタックと記述子を一つ使い、macOS の主のスレッド（8 MiB）でも深さ 4,000 で溢れた（作業 4 の「主のスレッドで行えば十分」は取り消す）。シンボリックリンクを辿らず（`openat` の `NOFOLLOW`・`DIRECTORY`）、`unlinkat` で消し、再帰の呼び出しを使わない（ヒープの上の明示のスタックで辿る）削除の関数を、`rustix` で一つ作る。記述子の数の上限（`EMFILE`）で止まったときは、誤りとして返す（落ちない）。この関数を、`session::cleanup` の二つの削除と、セッションの終わりの一時ディレクトリの削除（`SessionState::drop`、`src/session/mod.rs:269-279`。対話の画面では、既定の 2 MiB のスタックの「agent」のスレッドで走る。`src/ui/tui/worker.rs:92`）の両方で使う。テストは、子のプロセスか小さなスタックのスレッドで、再帰ならスタックが溢れる深さ（例: 512 KiB のスタックで深さ 1,500。記述子の数の上限が足りなければ、子のプロセスの中で柔らかい上限を上げる）を消せることを確かめる。深さ 3,000 を作って消すのに macOS で約 45 秒かかったので、試しの深さは時間を見て選ぶ（10 秒以内を目安に）。
7. **後始末の失敗**: `session::cleanup`（`src/session/mod.rs:289-350`。`CleanupReport` は 281〜285 行）は、二つの根の `read_dir` の失敗だけを起動の誤りとし、項目ごとの誤り（`entry`、`symlink_metadata`、`inactive_lock`、`open_directory_at`、`expired`、削除）は、その項目の失敗として記録して続ける。`CleanupReport` に失敗の一覧（ID、一時ディレクトリかセッションか、`io::Error` の表示の文。パスは含めない）を加える。セッションのディレクトリを消すときは、`lock` のファイルを最後に消し、その後にディレクトリを消す（`lock` が先に消えると、次の起動で `inactive_lock` が `Busy` を返し、`expired` もディレクトリの更新の時刻で判定するので、残りが黙って残り続けるため）。一時ディレクトリは、錠を持ったまま消す（今のまま）。
8. **失敗の示し方**: 起動の準備の `diagnostics`（`exec.rs:360` の形の `diagnostics.push(...)`）に、失敗ごとに一行を加える。`exec` は標準エラー出力に（`exec.rs:734-736`）、対話の画面は起動の知らせに（`ui/tui/mod.rs:117`）示すので、`src/ui/` は変えない。文言は `src/cli/text.rs` か `src/session/text.rs` に置き、今と同じく `terminal_text` を通す。`benitoiteh sessions`（`src/cli/sessions.rs:38-43`）も `?` で止めているので、同じく失敗を `ExecOutput::diagnostic` で示して続ける。テストの呼び出し（`tests/session.rs:527`、575〜577 行）を合わせる。`chmod 000` のテストは、root では効かないので、`tests/session.rs:726` の形で root のときは飛ばす。
9. **記録の大きさの数え**: `storage_usage`（`src/session/quota.rs:24-49`）は後始末の後に残ったものを数えるので、変えなくてよい。
10. **触るファイルの訂正**: `src/ui/` は触らない。`src/cli/sessions.rs`、`src/session/text.rs`（か `src/cli/text.rs`）、`src/sandbox/tests.rs` を加える。
11. **完了の報告の方針の文**: `image_policy(...).seatbelt_profile().source` から作って示す（`process-fork`、`WRITE_`・`PROTECTED_`・通信の行がなく、`/dev/null` の書き込みの規則だけがあること）。
