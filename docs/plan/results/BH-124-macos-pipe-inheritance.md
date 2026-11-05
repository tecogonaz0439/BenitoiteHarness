# BH-124 macOS で、別の糸が作ったパイプを子が継ぐことの調べ

2026-11-05 に、設計者の指示で、オーケストレータ（Claude Code）が調べた。きっかけは、[P5-37](../tasks/P5-37-post-release-followups.md)で時々失敗するテスト（`queries_bound_output_timeout_and_propagate_cancellation`）の原因として推定した仕組みである。

## 仕組み

Rust の標準のライブラリ（手元の 1.98.1。`library/std/src/sys/pipe/unix.rs` の `pipe`）は、Linux・FreeBSD などでは `pipe2(O_CLOEXEC)` で、close-on-exec の印を付けたパイプを一度に作る。macOS はこの一覧になく、`pipe()` で作った後に、両端へ一つずつ印を付ける。作ってから印を付けるまでの間に、同じプロセスの別の糸が子を起動すると、その子は印のない記述子を継ぐ。`Command` の標準入出力のパイプは、`spawn` の中で作られる。

継いだ子がパイプの書き口を持つ間、そのパイプの読み手は終わり（EOF）を得られない。継いだ子は、別の起動の出力に書き込むこともできる。macOS の Seatbelt は記述子を閉じないので（設計書 03-02「方針の要素」の 2026-10-17 の確かめ）、子がサンドボックスの中にいても、継いだ記述子は使える。

## 再現

次のプログラムで確かめた。一つの糸が、標準入出力を閉じた `/bin/sleep 2` を起動し続け、主の糸が、標準出力をパイプにした短い子の出力を終わりまで読む（`Command::output`）。継がれていれば、読みは `sleep` が終わるまで待つ。

```rust
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let rounds: usize = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(2000);
    let stop = Arc::new(AtomicBool::new(false));
    let spawner = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let mut children = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                if let Ok(child) = Command::new("/bin/sleep").arg("2")
                    .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
                {
                    children.push(child);
                }
                if children.len() > 200 {
                    for mut child in children.drain(..100) {
                        let _ = child.wait();
                    }
                }
            }
            for mut child in children {
                let _ = child.wait();
            }
        })
    };
    let (mut slow, mut worst) = (0, Duration::ZERO);
    for _ in 0..rounds {
        let start = Instant::now();
        let program = if cfg!(target_os = "linux") { "/bin/true" } else { "/usr/bin/true" };
        let _ = Command::new(program).stdout(Stdio::piped()).stderr(Stdio::piped()).output();
        worst = worst.max(start.elapsed());
        if start.elapsed() > Duration::from_millis(500) {
            slow += 1;
        }
    }
    stop.store(true, Ordering::Relaxed);
    let _ = spawner.join();
    println!("rounds={rounds} slow(>500ms)={slow} worst={worst:?}");
}
```

| 環境 | 結果 |
|---|---|
| macOS 27.2（Apple シリコン）、release、33,000 回 | 4 回が 2 秒（`sleep` の長さ）待った |
| aarch64 の Linux（Apple の `container` の Alpine 3.22、musl）、release、30,000 回 | 待った回はなかった。最も長い読みは 4 ミリ秒未満だった |

P5-37 のテストの失敗も、同じ仕組みで説明できる。同じテストのプロセスの中で、別のテストが 30 秒残る子（`sandbox-probe` の `descendant-child`）を起動したときに、出力の閉じを待つ処理が 2 秒の上限に達し、`PossiblyRemaining` と判定した。テストの失敗の値は、その後の再現で得ていない。

## 製品で起こりうる場面

`benitoiteh` の中で、子の起動と記述子の作成が別の糸で重なりうるのは、次の場面である（2026-11-05 のコードで確かめた）。

| 場面 | 起動するもの | 起こりうること |
|---|---|---|
| 並行の道具の呼び出し（最大 8 本。`read`・`glob`・`grep`・`benitoite_check`・`benitoite_explain`・`benitoite_reference`） | 画像の縮小の補助、処理系の検査と問い合わせ（どちらもサンドボックスの中） | 一つの呼び出しの出力のパイプを、同時に起動した別の補助か処理系が継ぐ。継いだ側が終わるまで、出力の終わりの待ちが延び、上限（2 秒）を超えると残りがあるものとして誤りになる。継ぐのは処理系か補助（信頼するプログラム）であり、スクリプトは動かさない |
| 対話の画面の糸と、エージェントの糸 | 通知のコマンド、外部のエディタ（利用者が設定したもの） | 確認を開いたときと一連の処理を終えたときに、エージェントの糸は子を起動していないので、今の流れでは重ならない |
| HTTP の要求の糸、トークンの更新の糸、中継と秘密の入力の補助の受け付けの糸 | なし（ソケットを作る） | 標準のライブラリは、macOS ではソケットの印も作った後に付ける。今の流れでは、`bash` などの子の起動と重ならない。並行の道具の呼び出しの間に、これらの糸がソケットを作るかは確かめていない |

`bash` は並行の道具ではなく、LLM の書いたコマンドを動かす子が、別の糸の記述子を継ぐ場面は、今の流れでは見つからなかった。ただし、糸の重なりを防ぐ仕組みはないので、後の変更（`bash` を並行にする、通信を道具の実行と重ねる）で、LLM の動かす子が、提供者への接続のソケットや別の道具の出力を継ぐ道筋になりうる。

## 直し方の案

| 案 | 内容 | 費用と限り |
|---|---|---|
| (a) 起動の錠 | `benitoiteh` のすべての子の起動（`Command::spawn` とその仲間）を、プロセスに一つの錠で一つずつにする。パイプは `spawn` の中で作られるので、子の起動どうしの重なりは塞がる | `unsafe` は要らない。錠を持つのは起動の呼び出しの間（数ミリ秒）だけで、子が動く間は持たない。ソケットを作る別の糸との重なりは塞がらない |
| (b) (a) に加え、ソケットを作る所も同じ錠の読みの側で囲む | 子の起動は錠の書きの側、`benitoiteh` のコードが記述子を作る所（接続、受け付け、待ち受け）は読みの側で持つ | HTTP のクレートの中の接続など、自分のコードの外で作るものは囲めない。囲む所が多く、漏れを確かめにくい |
| (c) 子の側で記述子を閉じる | 補助（`__session`）が、標準入出力のほかの記述子を閉じてから目的のプログラムを起動する | 番号から記述子を扱うには `unsafe`（`BorrowedFd::borrow_raw` など）か C の関数の呼び出しが要り、実装の規約（`unsafe` を書かない）に反する |
| (d) 並行の道具を、子を起動しないものに限る | `benitoite_check` などを並行から外す | 並行で速くしていた分が失われる。対話の画面の糸などとの重なりは残る |

推奨は (a) である。今の製品で重なりうる場面（並行の道具の呼び出し）を、規約の範囲で塞げる。ソケットとの重なりは、今の流れでは子の起動と重ならないことを記録し、(b) は、通信を道具の実行と重ねる変更をするときに改めて検討する。

## 直したこと

2026-11-05 に設計者が (a) に決め、オーケストレータが実装した。

- `sandbox::spawn` に、プロセスに一つの錠を持って子を起動する `spawn` と、起動だけを錠で囲んで終わりを待つ `output`・`status` を置いた。
- macOS で通る起動（実行器の `RunCommand::Command`、資格情報のコマンド、通知のコマンド、外部のエディタ、ログインのブラウザ、`log show`）を、この関数に替えた。`bash`、処理系の問い合わせと実行、画像の縮小の補助は、どれも実行器を通る。Linux だけの起動（bubblewrap とその補助の中）は替えていない。
- 設計書 03-02「方針の要素」と 06-01「lint と実装の規約」に書いた。

上の再現のプログラムの二つの起動を同じ形の錠で囲み、macOS 27.2 で比べた。

| 形 | 待った回 |
|---|---|
| 錠なし（80,000 回） | 6 回（どれも 2 秒） |
| 錠あり（80,000 回） | 0 回。最も長い読みは 47 ミリ秒（錠を待つ分） |

錠が抜けたことを、テストの中で決まった形で捕まえることはできない。起こる割合が 1 万回に 1 回ほどで、テストの中の起動の回数では見逃すからである。新しい起動を加えるときは、06-01 の決まりで `sandbox::spawn` を通す。

macOS で `scripts/check.sh --inside-codex` が終了状態 0 で通った。全体の検査（`scripts/check.sh`、`BENITOITEH_X86_64_HOST` を指定。コミット `c9538e4`）も終了状態 0 で、macOS は 821 件、aarch64 の Linux は 721 件、x86_64 の Linux は 815 件が通り、失敗は 0 だった。
