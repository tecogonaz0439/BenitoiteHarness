# P0-05 Linux の補助のプロセスの試作（BH-043、BH-049）

## 目的

Linux で子プロセスにサンドボックスを掛ける補助のプロセス（`benitoiteh __sandbox`）を試作し、P0-04 と同じ確かめを Linux（Apple の `container` の中）で行う。あわせて、次の二つを確かめる。

- BH-043: Landlock の実行の権利が、許した静的リンクのプログラムだけを起動させ、ほかの実行ファイル（動的リンクのローダを経るものを含む）を拒むか。`container` の中で Landlock と seccomp が使えるか。
- BH-049: 読ませない場所と、書ける場所の中の守る場所（`.git`・`.benitoiteh`）を、どう実現するか。

## 読む設計書

- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)（全体。特に「サンドボックスの掛け方」の Linux の手順、「方針の要素」の seccomp の規則、「制限モードで実行を許すプログラム」の静的リンクの規則）
- [セキュリティモデル](../../design/03-safety/03-01-security-model.md)の「Linux での保証の範囲」
- [クレートの構成](../../design/06-impl/06-01-crate-structure.md)の「lint と実装の規約」（名前空間の関数と `unsafe`）
- [未決事項](../../design/open-issues.md)の BH-043、BH-049、BH-091
- P0-04 で作った `sandbox` と `sandbox-probe` のソース

## 作るもの

1. `cli` の入口で、`__sandbox` を、ほかの処理より前に見分ける（03-02 の手順 1）。`benitoiteh __sandbox <方針の JSON> -- <プログラム> <引数...>`。
2. `crates/benitoiteh/src/sandbox/linux.rs` ほか（`#[cfg(target_os = "linux")]`）。03-02 の手順 2〜7 を行う。
   - 2: 標準入出力のほかに開いた記述子がないことを、`/proc/self/fd` の一覧で確かめる（一覧を読むために開いた記述子は除く）。あれば失敗とする。
   - 3: 名前空間は、この作業では使わない（後述の「確かめること」の 3 で、使えるかと `unsafe` の要否だけを調べる）。
   - 4: `no_new_privs` を設定する（`rustix` の関数）。
   - 5: Landlock の規則を掛ける（`landlock` クレート）。必ず要る ABI を V3（V1 の実行・読み・書き・作成と削除の権利に、V2 の `Refer`（ディレクトリをまたぐ付け替えとリンク）と V3 の `Truncate` を加えたもの）とし、`CompatLevel::HardRequirement` で掛ける。V3 が使えなければ、部分的に掛けずに失敗とする（03-02 の「サンドボックスを掛けられないとき」）。それより新しい権利（V5 の `IoctlDev`、V6 の abstract のソケットの `Scope`、V9 の `ResolveUnix` など）は、使えるときだけ重ねて掛ける。読ませない場所と守る場所は、後述の「確かめること」の 2 の結果で採る形で表す。
   - 6: seccomp のフィルタを掛ける（`seccompiler` クレート）。03-02 の「方針の要素」の規則のとおりとする: IP 以外のソケットの作成を常に拒む、IP の通信を許さないときは IP のソケットの作成と `connect`・`bind`・`listen`・`accept`（`accept4` を含む）も拒む、`socketpair` は `AF_UNIX` で種類が `SOCK_STREAM` か `SOCK_SEQPACKET`（`SOCK_CLOEXEC`・`SOCK_NONBLOCK` を除いて比べる）だけを許す、`ptrace`・`process_vm_readv`・`process_vm_writev`・`io_uring_setup`・`io_uring_enter`・`io_uring_register` を拒む、補助のプロセスの CPU の種類の番号の表だけを許し、x86_64 では i386 の互換の呼び出しと x32 の番号を拒む。拒むときの戻りは `EPERM` とする。`seccompiler` 0.5.0 について、点検で次のことが分かっている（`seccompiler-0.5.0/src/backend/bpf.rs` ほか）。CPU の種類が違う呼び出しは `SECCOMP_RET_KILL_PROCESS` で終わり、`EPERM` にならない。これは強制終了でよい。x32 の呼び出しは同じ `AUDIT_ARCH_X86_64` を持つので、ライブラリの CPU の種類の検査では拒まれない。x32 を拒むには、公開されている `sock_filter` の命令で `nr & 0x40000000`（`__X32_SYSCALL_BIT`）を調べる命令をフィルタの先頭に置く（安全なコードで書ける）。引数の比べ方はマスクした値が等しいか（`MaskedEq`）だけで、マスク付きの「等しくない」はないので、`socketpair` の種類は、許す 8 通りの値（2 種類と二つの印の組み合わせ）を並べて表す。呼び出しの番号と `AF_*`・`SOCK_*` の値は、`libc` の定数を使う（10-interfaces）。
   - 7: 目的のプログラムを `exec` する（`std::os::unix::process::CommandExt::exec`）。
   - どの段階で失敗しても `exec` せず、決まった終了状態（値は任せる。`Process.Exit` で偽れることは 03-02 のとおり扱う）と、標準エラー出力の一行で失敗を示す。
3. `SandboxCommand` から、Linux では `<benitoiteh の実行ファイル> __sandbox <JSON> -- ...` の `std::process::Command` を作る。`benitoiteh` の実行ファイルの場所は、呼び出す側が与える（テストでは `CARGO_BIN_EXE_benitoiteh`）。
4. `crates/benitoiteh/tests/sandbox_linux.rs`（`#![cfg(target_os = "linux")]`）。P0-04 の「確かめること」の 1〜6 と同じ項目を、`sandbox-probe` で確かめる。加えて次のものを確かめる。
   - x86_64 で、i386 の互換の呼び出しと x32 の番号でソケットを作れないこと。試す方法（`sandbox-probe` に操作を足すなど）に `unsafe` が要るなら、試さずに、フィルタの規則が番号の表と CPU の種類を調べることを、フィルタの作りのユニットテストで確かめ、報告に書く。
   - 補助のプロセスの各段階を失敗させたとき（`sandbox-probe` の `spawn-with-fd` で余分な記述子を渡す、壊れた方針の JSON、Landlock を掛けられない指定）、目的のプログラムが動かないこと。Landlock を掛けられない指定は、失敗させる方向にだけ働く欄（求める ABI の版を上げる欄など）で作り、規則を外す方向の欄を作らない。
   - `io_uring_setup` が拒否されること（試す方法に `unsafe` が要るなら、上と同じく扱う）。

## 触るファイル

- `crates/benitoiteh/src/cli/mod.rs`（`__sandbox` の見分け）
- `crates/benitoiteh/src/sandbox/`（P0-04 で作ったもの）
- `crates/benitoiteh/Cargo.toml`（Linux だけの依存に `landlock`・`seccompiler` を加える。`rustix` の機能を足す）
- `crates/sandbox-probe/src/main.rs`（操作を足すとき）
- `scripts/check.sh`（後述の x86_64 の扱い）

## 確かめること

テストと調べた結果を、完了の報告の「確かめたこと」に書く。Codex の中では Linux のテストを走らせられないので、テストを書き、Linux の二つの対象でビルドと clippy を通すところまでとする。走らせた結果は、オーケストレータが `scripts/check.sh` の `linux` の段（`container` の中）で得て、`codex exec resume` で伝える。伝えた結果を見て直すことも、この作業に含む。

1. BH-043: Landlock の実行の権利を、`sandbox-probe`（静的リンク）だけに与えた方針で、次のものがどうなるかを確かめるテストを書く。`container` のイメージ（`alpine:3.22`）の `/bin/sh`（`busybox`、`ld-musl` を経る動的リンク）、`ld-musl` の実行ファイル（`/lib/ld-musl-*.so.1`）を直接起動すること。動的リンクのプログラムが、ローダに実行の権利がないことで拒まれるか。ローダにも権利を与えたら、ローダを経て任意のプログラムを動かせるか（設計書 03-02 の懸念のとおりか）。
2. BH-049: Landlock で表せる範囲で、読ませない場所と守る場所をどう表すかを調べ、試作で採る形を決めて実装する。設計書の条件（読ませない場所は読み取りが失敗する、守る場所はまだないときの作成・付け替え・ファイルとしての作成も拒む）を満たせない点は、満たせないと書く。候補の例: 書ける場所の根のディレクトリには作成の権利を与えず、根の直下のディレクトリ（守る名前を除く）ごとに書く権利を与える形。この形で、根の直下に新しいファイルを作れなくなるなどの制約が出るなら、それも書く。読ませない場所は、読む権利を「全体から除いた形」で与えるため、根から辿って、読ませない場所を含まない部分木に分けて権利を与える形が考えられる。
3. 名前空間を使う形（ユーザとマウントの名前空間）について、`rustix` 1.1.5 の `unshare` に当たる関数が `unsafe` かを、ソースで確かめて書く。`container` の中で特権を持たないユーザの名前空間を作れるかは、`unsafe` なしに試せるなら試し、試せなければ試さない。名前空間の形は実装しない。
4. `container` の中のカーネルの版、Landlock の ABI の版、seccomp を使えるか。テストの出力に書く。

## テスト

上の「作るもの」の 4 のとおり。ほかに、seccomp のフィルタを作る関数の出力（規則の表）を、ユニットテストで確かめてよい（P0-04 の方針の文の確かめと同じ扱い）。

## x86_64 の扱い

`container --arch amd64` は Rosetta で x86_64 の実行ファイルを動かす。点検で、Rosetta の下では、プロセスに `/run/rosetta/rosetta` などの記述子が開いていることが分かった（`ls -l /proc/self/fd` で 3〜6 番）。このため補助のプロセスの手順 2 は必ず失敗する。Rosetta は呼び出しを aarch64 のカーネルに訳して出すので、seccomp の CPU の種類の検査と、i386・x32 の確かめも意味を持たない見込みである。

そこで、`scripts/check.sh` の `linux` の段は、x86_64 の対象では `tests/sandbox_*.rs` を除き、除いたことと理由（Rosetta の下では確かめられない）を示す。x86_64 の Linux での確かめは、本物の x86_64 の環境で後に行う項目として報告に書く。x86_64 のフィルタの作り（i386・x32 の拒否）は、ユニットテストで規則の表を確かめる。

## 完了条件

- `scripts/check.sh --inside-codex` が通る。
- オーケストレータが、サンドボックスの外で `scripts/check.sh` を走らせ、`linux` の段の `sandbox_linux` が aarch64 の対象で通る。BH-043・BH-049 の結果として、Landlock が使えないことや、設計書の条件を満たせないことが分かったときは、その結果を確かめたテスト（期待を結果に合わせたもの）が通ればよい。その場合、オーケストレータが設計者と相談して、Linux の制限モードを試作の範囲に残すかを決める（00-03 の P0 の完了条件）。
