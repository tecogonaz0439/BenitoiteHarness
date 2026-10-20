# P5-28 画像の補助のプロセスの作成の禁止と、枝を移るときの記録の写し

## 目的

2026-10-20 の点検で見つかった二つの残りを直す。一つは、Linux で root として動かすと、画像を縮める補助のプロセス（`benitoiteh __image`）の `RLIMIT_NPROC` の 0 が効かず、補助が子のプロセスを作れることである。もう一つは、`/rewind` の会話を戻す操作と枝の移動で、セッションの記録の全体を何度も写し、大きなセッションでメモリーを多く使うことである。この作業で本物の LLM の提供者を呼ばない。

## 読む設計書

- [道具](../../design/01-agent/01-03-tools.md)の「画像のファイルの読み取り」（「サンドボックス」）
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「サンドボックスの掛け方」「方針の要素」（seccomp）
- [セッション](../../design/01-agent/01-05-sessions.md)の「再開」「巻き戻し」「セッションの木」
- 既存のもの: `crates/benitoiteh/src/tools/read/image_helper.rs`、`src/sandbox/linux/filter.rs`、`src/sandbox/linux.rs`、`tests/sandbox_read_image.rs`、`src/agent/rewind.rs`、`src/session/resume.rs`、`src/session/replay.rs`、`src/agent/mod.rs`

## 作るもの

### 1. 画像の補助で、プロセスの作成とプログラムの起動を拒む

設計書 01-03「画像のファイルの読み取り」の【方針】のとおり、Linux では、補助は解く前に seccomp の絞りを自分に加え、`clone`・`clone3`（x86_64 では `fork`・`vfork` も）と `execve`・`execveat` を `EPERM` で拒む。macOS は今のまま（Seatbelt の方針と `RLIMIT_NPROC`）とする。

### 2. 枝を移るときに記録を写さない

`/rewind` の会話を戻す操作と枝の移動で、読み込んだ記録（`Vec<Record>`）を、会話の組み立てに写さずに移す。結果の会話と記録に書く内容は変えない。

## テスト

- (a) 補助（項目 1）: 今の `resource_limits_deny_children_and_file_data_but_allow_pipes` の、Linux の root で子の起動の確かめを飛ばす扱いを除く。Linux では子の起動が `EPERM` で、macOS では `EAGAIN` で失敗することを確かめる。`tests/sandbox_read_image.rs` が今どおり通る（補助がスレッドを作らずに縮められる）こと。
- (b) 記録（項目 2）: 既存の巻き戻し・枝の移動・再開のテスト（`tests/rewind.rs`、`src/agent/resume_tests.rs` など）が、期待を変えずに通ること。メモリーの量を確かめるテストは置かない（下の補足 3 の測定を完了の報告に書く）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、項目 2 の前と後の最大の常駐の大きさの測定の結果と、測り方を書く。

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。行の番号は `main`（`564a32c`）の `crates/benitoiteh/` からの相対である。

1. **補助の seccomp（項目 1）**:
   - 今の箇所: `src/tools/read/image_helper.rs` の `limit_resources`（51-71。`RLIMIT_NPROC`・`RLIMIT_FSIZE`・`RLIMIT_CORE` を 0）、`convert`（85-）の初めで `limit_resources` と `limit_memory` を呼ぶ（86-89）。テストの `resource_limits_deny_children_and_file_data_but_allow_pipes`（266-）は、Linux の root では子の起動の確かめを飛ばしている（286-296。「Linux は特権のある root に NPROC を強制しない」）。
   - 事実: Linux のカーネルは、`copy_process` で、実の利用者が root（`INIT_USER`）であるか、`CAP_SYS_RESOURCE` か `CAP_SYS_ADMIN` を持つときに `RLIMIT_NPROC` の超過を断らない（torvalds/linux の `v6.18` の `kernel/fork.c`）。Linux のコンテナ（aarch64 の `container`）のテストは root で走るので、今のテストはそこで確かめを飛ばしている。
   - 直し: `src/sandbox/linux/filter.rs` に、プロセスを作る呼び出しとプログラムを起動する呼び出しだけを拒む絞りを作る関数（例 `pub(crate) fn compile_no_process() -> Result<BpfProgram>`）を足す。拒む呼び出しは `libc::SYS_clone`・`SYS_clone3`・`SYS_execve`・`SYS_execveat`、x86_64 では加えて `SYS_fork`・`SYS_vfork`（aarch64 の番号の表は `fork`・`vfork` を持たない）。一致しないものは許し、一致したものは `SeccompAction::Errno(libc::EPERM as u32)` とする（今の `compile` の 79-84 と同じ形）。x86_64 の x32 の番号を先に拒む前置き（今の `compile` の 90- の `prefix`）も同じく付ける（前置きを関数に切り出して両方で使う）。
   - 掛け方: 補助の中で、`limit_resources` の後、入力を読む前に、`rustix::thread::set_no_new_privs(true)` を呼んでから `seccompiler::apply_filter` で掛ける（`src/sandbox/linux.rs` の 101-105 と同じ順。`__sandbox` が既に `no_new_privs` を立てていても、補助を単独で起動するテストのために立て直す）。掛けられなければ、`HelperError::Limits` で終える。関数は `limit_processes()` などの名で `limit_resources` と分け、テストの子からも呼べるようにする。
   - 補助はスレッドを作らない前提で作ってある（`limit_resources` の 53-54 のコメント。`image` は `default-features = false` で `png` と `jpeg` だけを有効にしている。`Cargo.toml` の 25）。`clone` を拒むとスレッドも作れないので、縮める処理が通ることを `tests/sandbox_read_image.rs` で確かめる。
   - テスト: 286-296 の root の判定を除き、子の起動（`std::process::Command::output`）の誤りが、Linux では `Errno::PERM`、macOS では今どおり `Errno::AGAIN` であることを確かめる。子のテストのプロセスで `limit_processes()` も呼ぶ。seccomp を確かめる試しは、OS のサンドボックスを要しないので `tests/sandbox_*.rs` に移さなくてよい（Linux の段で走る）。
2. **記録の写し（項目 2）**:
   - 今の箇所: `src/agent/rewind.rs` の会話を戻す操作（252-253 で全部を読み、`SessionTree::from_records(&records)`）と `records_for_move`（302-315。306-307 で全部を読む）、`move_branch`（317-）の 341 の `ResumeState::from_records_with_tree(&records, tree)`。`from_records_with_tree`（`src/session/resume.rs` の 43-）は `&[Record]` を受け、`replay::replay_records`（`src/session/replay.rs` の 288-325）が道の項目を写す（`message.clone()`、`response.clone()`、`result.clone()`、`compacted.clone()`）。`fill_missing`（327-）は、会話の道具の結果を全部写して `known` の表を作る（328-336）。`resume.rs` の 211-221 は、記録にない結果を `missing_results` に写す。`src/agent/mod.rs` の 388-398 は、再開のときに記録を読み直して `CheckpointRecorder::new` に渡す。
   - 直し: `from_records_with_tree` を、`Vec<Record>` を値で受ける形にする（`from_records` と `read` は、読んだ記録を移して呼ぶ）。`replay_records` も値で受け、道の番号の項目を `std::mem::replace` か `std::mem::take` で取り出して会話に入れる（取り出した後の記録は使わない）。`fill_missing` は、道具の結果を写す代わりに、ID から会話の中の位置への表を作る。`move_branch` の 341 は、`records` をそのまま移す（341 より後で `records` を使わないことは確かめた）。会話を戻す操作の側も同じく移す。
   - `from_records_with_tree` の前に記録から取る値（`irreversible` の 329 など）は、移す前に済ませる。`resume_tests.rs` の 669-673 など、記録を後で使う呼び出し側は、`clone()` を渡すか、使う値を先に取る。
   - `resume.rs` の 211-221 と `agent/mod.rs` の 388-398 は、測って差が大きいときだけ直す（前者は記録に書き足す内容なので、写しをなくすには形を変える要がある）。直さなかったときは、測った値と理由を報告の「残したこと」に書く。
3. **測り方（項目 2）**: 道具の結果を合わせて 200 MB 程度にした合成のセッションを作る（`tests/rewind.rs` の記録の作り方に倣い、テストの外で一度作ってよい）。macOS の `/usr/bin/time -l` で、最大の常駐の大きさ（`maximum resident set size`）を、変える前と後で比べる。再開は `benitoiteh exec --resume <ID>` を模擬の提供者で走らせて測る。枝の移動は、対話の画面を経ずに呼べる入口（`tests/rewind.rs` と同じ呼び方）を、`#[ignore]` のテストか小さな測りの道具にして、子のプロセスで走らせて測る。測りの道具を残すなら、`tools/` でなく、テストの `#[ignore]` として置く（[テスト](../../design/07-quality/07-01-testing.md)の原則と `test-audit` に従い、後で要らなくなるものは残さない）。
