# benitoiteh の性能の測定

[設計書 06-02](../../docs/design/06-impl/06-02-performance.md)の大きさ・起動時間・メモリ・描画・検索の目標を、同じ条件で測り直す道具である。macOS で使う。本物の LLM の提供者は呼ばず、HTTP の模擬の提供者を使う。

## 走らせ方

リポジトリのルートで、次のように実行する。Python 3.9 以降の標準ライブラリと、オフラインでビルドできる Rust の環境が必要である。`--benitoite` には本物の処理系（現在の検証対象は v0.0.7）を指定する。

```sh
python3 -I tools/perf/measure.py \
  --benitoite /absolute/path/to/benitoite \
  --out docs/plan/results/P5-perf.md
```

`--runs N` の既定は 10、最小は 1 である。`--out` を省くと Markdown を標準出力へ書く。ビルドと測定のログは標準エラー出力へ書く。短い通しの確認には `--runs 1` を使う。本番の数値は、オーケストレータが Codex のサンドボックスの外で測り、`docs/plan/results/P5-perf.md` に記録する。

BH-060 の判断のために `opt-level = "s"` と比べるときは `--opt-s` を付ける。

```sh
python3 -I tools/perf/measure.py \
  --benitoite /absolute/path/to/benitoite --opt-s \
  --out docs/plan/results/P5-perf.md
```

どちらの構成も `cargo build --offline --locked --release -p benitoiteh` でビルドする。`opt-s` の build と test の両方に、`--config 'profile.release.opt-level="s"' --target-dir target/perf-opt-s` を加える。`Cargo.toml` は変えない。build の直後に実行ファイルを `target/perf/benitoiteh-default` または `target/perf/benitoiteh-opt-s` へ写し、その大きさと SHA-256 を記録する。測定の子プロセスは、この写しを起動する。検索は、それぞれの構成のテストの実行ファイルの中で測る。

Rust の手動測定だけを走らせる場合は、次のようにする。各走行が子プロセスの `HOME`、`XDG_CONFIG_HOME`、`XDG_STATE_HOME`、`TMPDIR` を新しい一時ディレクトリへ向け、終わったら消す。集計ツール自身も、処理系の版の問い合わせを含めてこれらを隔離する。

```sh
BENITOITEH_TEST_BENITOITE=/absolute/path/to/benitoite \
BENITOITEH_PERF_RUNS=1 \
cargo test --offline --locked --release -p benitoiteh --test perf_measure \
  -- --ignored --nocapture --test-threads=1
```

`BENITOITEH_PERF_BINARY` で、測る `benitoiteh` の絶対パスを指定できる。省くと Cargo がテスト用に用意した実行ファイルを使う。`BENITOITEH_PERF_RUNS` が 0 または数でないときは失敗する。Linux では、各測定の先頭で macOS 専用であることを表示して飛ばす。

## 項目と条件

時間は `Instant`、端末の読み取りは非ブロックで測る。端末のポーリング間隔は 1 ms である。`session_200_max_rss_kib` と `grep_10k_first_ms` を除き、各項目で一回の温めの走行を行い、その値を標本から除く。

| 項目 | 測る範囲と条件 | 目標 |
|---|---|---|
| 実行ファイルの大きさ | 記号を除いた release の写し。バイト、MiB、SHA-256 を記録する。処理系の大きさは環境の欄へ別に書く | 8 MiB |
| `version_ms` | `--version` を起動する直前から終了まで | 5 ms |
| `exec_first_request_ms` | 通常モード、処理系なし、`--no-sandbox`。起動直前から専用のブロッキング accept の直後まで。要求の本文を読み終えるまでの時間も note に書く | 30 ms |
| `exec_first_request_restricted_ms` | 制限モード、本物の処理系。`--version=json`、`skill list`、`skill show` の三回を `sandbox-exec` の中で起動する時間を含む | 参考（判定なし） |
| `tui_ready_ms` | 制限モードで起動し、端末の出力に `Sandbox:` と `> ` が初めて揃うまで。上記の処理系の三回の起動を含む。提供者への要求は送らない | 150 ms |
| `tui_idle_rss_kib` | 上記の準備が終わってから 1 秒待ち、子の PID に対して `ps -o rss= -p` で読む | 20480 KiB（20 MiB） |
| `session_200_max_rss_kib` | 通常モード、処理系なし。別々の約 2 KiB のファイルへの `read` 199 回と最後の応答。`exec --max-turns 200` の成功と要求数 200 を確認する。道具の子プロセスは起動しない。`/usr/bin/time -l` の最大 RSS のバイトを 1024 で割る。回数は `min(runs, 3)`、温めなし、期限 120 秒 | 102400 KiB（100 MiB） |
| `stream_delta_ms` | 通常モード、処理系なし、`danger-full-access`、`--no-sandbox`、`approval never`。模擬の提供者が 100 ms ごとに `M01`〜`M20` を改行付きで送る。各塊を書き終えた時刻から、その印が端末へ初めて現れるまで。一走行で 20 標本 | 16 ms |
| `grep_10k_first_ms` | 16 ディレクトリ × 625 ファイル、合計 10000 ファイル × 1024 バイト、100 ファイルに一致する行を一行ずつ置く。作成直後の最初の一回。標本は一個 | 1000 ms |
| `grep_10k_ms` | 同じ木で、最初の走査と温めの一回の後に繰り返す | 1000 ms |

`exec_first_request_ms` は処理系の起動を除いた通常モードの値である。一方、対話の画面と制限モードの起動の値には、設計書の目標から除くとしている処理系の起動時間が含まれる。この差を引き算で補正しない。ストリーミングでは、対話の画面のイベント待ち（最大 33 ms）を含む値をそのまま記録する。目標を超えても、この道具から製品を変更しない。

検索の木には既定の除外に当たるファイルを置かず、`excluded=0` と合計バイト数を note に記録する。一致は合計ちょうど 100 行とし、検索結果に打ち切りがないことを確認する。最初の走査も、作成したファイルがページキャッシュに残る条件である。OS のキャッシュを消した測定ではない。

## 集計と失敗の扱い

Rust の測定は、`PERF ` に続けて一行の JSON を出す。欄は `item`、`unit`、`samples`、`note` である。時間の単位は ms、メモリは整数の KiB とし、note に実行ファイルのパスを書く。集計ツールは libtest の表示が行頭に付いていても読み取る。

報告には、環境（Rust、コミット、作業ツリーが変更されているか、Cargo.lock の SHA-256、macOS、CPU、処理系の版と SHA-256）、release の設定とその差し替え、測定条件、生の標本を残す。表では回数、中央値、最大値、目標、中央値による判定を示す。最大値が目標を超えた場合も書き添える。`--opt-s` では二つの構成の列を並べる。

欠けた項目は「測れなかった」と書き、終了状態を 1 にする。壊れた PERF の行や `cargo test` の失敗でも、取得できた標本と失敗理由を報告に残す。目標の未達だけでは終了状態を 1 にしない。

Codex のサンドボックス内では、ループバックの待ち受けや入れ子の `sandbox-exec` が使えず、該当する測定は理由付きで失敗する。環境を見分けて成功扱いにする分岐は設けていない。サンドボックス内で取得した値は、本番の性能値として扱わない。

## 道具の検査

```sh
python3 -I -m unittest discover -s tools/perf -p 'test_*.py'
```

`scripts/check.sh` の `perf-tools` の段でも走る。通常の `cargo test` は Rust の測定コードをコンパイルするが、`#[ignore]` の測定は実行しない。性能の閾値で通常の検査を判定しない。
