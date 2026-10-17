# P2-03 Linux の塞ぐ場所の錠と記録、入れ子の探索の時間（BH-095）

## 目的

Linux の bubblewrap の形で、塞ぐために作らせた空のディレクトリ（書ける場所の根の直下のまだない `.git` と `.benitoiteh`）の後始末を、並んで動く子プロセスと `benitoiteh` の強制終了に耐える形にする（設計書 03-02 の「Linux のファイルシステムの見え方」、2026-10-17 の方針）。あわせて、すでにある入れ子の守る場所を探す時間を測る（BH-095）。

## 読む設計書

- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「Linux のファイルシステムの見え方」（塞ぐ場所の後始末、入れ子の守る場所）
- [CLI と設定ファイル](../../design/05-interface/05-01-cli.md)の「データの置き場所」
- [未決事項](../../design/open-issues.md)の BH-095
- 既存のもの: `crates/benitoiteh/src/sandbox/linux/mounts.rs`（`MountPlan`、`Cleanup`、`build`、`scan`、`check_links`）、`linux/process.rs`（`PreparedSandbox`、`SandboxChild`、`Drop`、`finish_cleanup`）、`linux/availability.rs`、`sandbox/text.rs`、`config/paths.rs`（`data_dir`）、`tests/sandbox_linux.rs`（`cleanup_waits_for_exit…` などの、`SandboxStdin::Piped` と `take_stdin` で子を進めるテスト）、`crates/sandbox-probe/`

## 作るもの

### 1. 錠と記録のディレクトリ

- `SandboxCommand::prepare` の形を `prepare(&self, helper: &Path, scratch: &Path, placeholders: &Path)` に改める。`placeholders` は錠と記録を置くディレクトリである。呼び出す側（P3 以降の `cli`）は `data_dir.join("placeholders")` を渡す。
- `prepare` は、`placeholders` がなければ `DirBuilder::new().recursive(true).mode(0o700)` で作り、次を確かめる。満たさなければ `SandboxError::InvalidPolicy` を返す（文は `text` に置く）。
  - リンクを辿らないメタデータで、シンボリックリンクでない、本物のディレクトリである。
  - 持ち主が実行している利用者（実効の UID）で、権限が `mode & 0o077 == 0` である。緩い権限は直さずに誤りとする。
  - 正規の形（`canonicalize`）にしたパスが、どの書ける場所の中にもない（既存の `WRITABLE_COVER`・`WRITABLE_HELPER` と同じ確かめ）。理由: サンドボックスの中から錠のファイルを消すか差し替えられると、別のプロセスが違うファイルに錠を掛け、動いているサンドボックスのマウントの場所を消せてしまう。
- `probe_linux(helper, scratch)` の形は変えない。中で `scratch` の下に一時の 700 のディレクトリ（例: `.benitoiteh-probe-<乱数>-placeholders`）を作って `prepare` に渡し、終わったら消す。理由: 確かめに使う書ける場所は新しく作った一時のディレクトリなので、ほかのプロセスとの競合はない。
- 錠と記録の操作の誤りは、`SandboxError::Setup { stage: text::PLACEHOLDERS, detail }`（名前は任せる）で返し、子プロセスを起動しない。

### 2. 錠

- 対象は、書ける場所の根の直下の守る名前の場所すべて（`root.join(name)`）とする。**まだないものも、すでにあるものも**同じく扱う。理由: B が、A の bubblewrap が作った空の `.benitoiteh` をすでにあるものとして `--ro-bind` で重ねたとき、B が錠を持たなければ、A の後始末がそのディレクトリを消し、B の塞ぎが外れる。`scan` が見つけた入れ子の場所に錠を掛けるかは任せる（設計書は、新しい入れ子の場所を保証しない）。
- 名前: 場所の正規の形の絶対パスのバイト列（`as_os_str().as_bytes()`）の SHA-256 の小文字の 16 進を `<hash>` とし、錠のファイルは `<hash>.lock` とする。
- 子プロセスごと、場所ごとに、錠のファイルを `O_CLOEXEC` で新しく開く（`try_clone` や記述子の複製を使わない。`flock` の錠は開いたファイルの記述ごとに掛かるため）。共有の錠は待つ形の `LockShared` で、その場所があるかを調べる前に掛ける。錠のファイルは `Cleanup` が持ち、子プロセスを回収するまで持つ。
- 排他の錠は `NonBlockingLockExclusive` だけで取り、取れるまで待たない。
- `flock` の掛け直し（共有から排他）が失敗すると共有の錠が外れていることがある（`flock(2)`）。後始末は失敗したら錠を外して終わるので害はないが、失敗の後に共有の錠が残っている前提のコードにしない。
- 起動した後に、回収せずに `SandboxChild` が落とされたとき（`Drop`）は、錠を外さない。錠のファイルをプロセスの終わりまで生かしておく（`std::mem::forget`、またはプロセスの中の `static` の `Mutex<Vec<File>>` に移す）。理由: 錠を外すと、ほかのプロセスが、まだ動いているかもしれないサンドボックスのマウントの場所を消せる。

### 3. 記録

- まだない場所を塞ぐと決めたら、bubblewrap を起動する前に、記録のファイル `<hash>.path` を置く。中身は場所の正規の形の絶対パスのバイト列とする。書き方は、同じディレクトリの `<hash>.path.<乱数>.tmp` を `create_new` で作って書き、名前を付け替える（二つのプロセスが同じ記録を同時に書きうる）。

### 4. 後始末（子プロセスを回収した後、`Cleanup::finish`）

場所ごとに次のとおりとする。

1. 共有の錠を排他の錠（`NonBlockingLockExclusive`）に掛け直す。取れなければ、消さず、記録も残し、錠を外して終える。
2. 取れたら、この子が塞ぐために作らせた場所か、`<hash>.path` の記録がある場所について、`rmdir` を呼ぶ。理由（記録がある場所も消す）: B が A の作ったディレクトリを重ねたとき、A は B が動いている間は消せない。B が最後に終わったときに、記録を手がかりに消す。
3. `rmdir` の結果が成功か、`NotFound`・`DirectoryNotEmpty`・`NotADirectory` なら、記録を消す。ほかの誤りなら記録を残し、誤りを返す。`rmdir` は空でないディレクトリとシンボリックリンクを消さないので、利用者が後でその名前で作ったものは消えない。
4. 錠を外す。錠のファイルは消さない（消すと、ほかのプロセスが同じ名前の別のファイルに錠を掛けうる）。

### 5. 強制終了の残りの片付け（`prepare` の初め）

- `placeholders` の `*.path` を調べ、それぞれについて、錠のファイルを開いて `NonBlockingLockExclusive` を試す。取れたものは、記録の中身を読み、絶対パスでない、読めない、ハッシュがファイルの名前と合わないときは、記録だけを消す。合うときは、4 の 2〜3 と同じく `rmdir` を呼んで記録を消す。取れなかったものは何もしない。
- この片付けで開いた錠の記述子は、2 の共有の錠を掛ける前にすべて閉じる。理由: 同じプロセスの中で、別の記述で排他の錠を持ったまま共有の錠を待つと、自分で止まる。

### 6. 入れ子の探索の時間

- 無視するテスト（`#[ignore]`）として `tests/sandbox_linux.rs` に作る。次の二つの木で `prepare` を 20 回呼び（起動せずに落とす。残った記録は次の呼び出しが片付ける）、最初の一回の時間と、20 回の中央値を、テストの出力に書く。
  - 項目の少ない木（ディレクトリ 10、ファイル 100）。
  - 合成の大きな木（深さ 3 まで、各段の各ディレクトリに 30 のディレクトリと 30 のファイル。辿る項目の数は、木の作り方から 30·60 + 900·60 + 60 = 55,860 と求める）。
- 項目の数を数えるための数え口を、本番のコードに足さない。テストのコードにも lint（`arithmetic_side_effects`、`indexing_slicing` など）が掛かることに注意する。
- 木を置いたファイルシステムの種類（`/proc/self/mountinfo` の該当の行など）も出力に書く。rootless の podman の `/tmp` は overlay であり、ホストのファイルシステムとは時間が違いうる。

### 7. `sandbox-probe` の操作

並んで動く子のテストのために、準備ができたことを標準出力に書き、標準入力から一行を読んでから、続く引数の操作を行う操作（例: `wait-line <操作> <引数…>`）を加える。

## 触るファイル

- `crates/benitoiteh/src/sandbox/linux/`（`mounts.rs`、`mounts/tests.rs`、`process.rs`、`availability.rs`）、`sandbox/text.rs`
- `crates/sandbox-probe/src/`（`main.rs`、`text.rs`）
- `crates/benitoiteh/tests/sandbox_linux.rs`（`prepare` の呼び出しは `f.root.join("placeholders")` を渡す形に直す。書ける場所の外である）

`sha2` と `rustix::fs::flock`（機能 `fs`）は、すでに使える。

## テスト

`tests/sandbox_linux.rs` に加える。並んで動く子は、`SandboxStdin::Piped` と `take_stdin` で進める。

- **順 (a)**: A と B を、どちらも起動する前に準備する（どちらも、まだない `.benitoiteh` を tmpfs で塞ぐ）。A を終わらせて後始末した後も、B の中から `.benitoiteh` の下に書けず、付け替えもできない。B が終わった後に、空のディレクトリと記録が残らない。
- **順 (b)**: A が準備できたと書いた後に B を準備する（B は A の作った空のディレクトリを `--ro-bind` で重ねる）。A を終わらせて後始末した後も、B の中から書けない。B が終わった後に、空のディレクトリと記録が残らない。
- **錠がないときの外れ（無視するテスト）**: 順 (a) と同じく並べ、A の後始末の代わりに、テストが錠を使わずに空のディレクトリを `rmdir` する。その後に、B の中から `.benitoiteh/x` を作れることを確かめる（錠が要る理由の確かめ。結果はオーケストレータが設計書に書く）。
- **強制終了の残り**: 記録と空のディレクトリを手で作っておき、次の `prepare` が片付ける。空でないディレクトリ、ファイル、シンボリックリンクは消さず、記録だけを消す。ほかの記述が共有の錠を持っている間は、片付けない。中身が絶対パスでない記録、ハッシュが合わない記録は、記録だけを消す。
- **`placeholders` の検査**: 書ける場所の中にあるとき、シンボリックリンクのとき、権限が 700 より緩いときに `InvalidPolicy` になる。ないときは 700 で作られる。
- 既存の、塞ぐために作らせた空のディレクトリが子の終了の後に消えるテストが通る。

ユニットテスト（`mounts/tests.rs`）では、記録の名前（ハッシュ）と、記録の中身の検査の境界を確かめる。

## 確かめること

- 錠を使わずに消したときに B の塞ぎが外れるか（無視するテスト。オーケストレータが走らせる）。
- 探索の時間（二つの木の最初の一回と中央値、ファイルシステムの種類。オーケストレータが `scripts/remote-linux-run.sh <テストの実行ファイル> --ignored --exact <名前> --nocapture` で走らせる）。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- 完了の報告に、`prepare` の新しい形と、`probe_linux` の扱いを書く（オーケストレータが 10-interfaces を更新する）。
