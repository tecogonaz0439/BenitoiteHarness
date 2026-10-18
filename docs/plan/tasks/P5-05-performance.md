# P5-05 性能の測定の道具

## 目的

設計書 06-02「目標」の各項目を、同じ条件で測り直せる道具を作る。測定そのもの（本番の数値）は、オーケストレータがサンドボックスの外で走らせ、結果を `docs/plan/results/P5-perf.md` に書く。この作業では、道具を作り、短い通しで動くことを確かめる。BH-060（`opt-level`）の判断に要る、`opt-level = "s"` との比べも同じ道具で行えるようにする。

この作業で本物の LLM の提供者を呼ばない。製品のコード（`crates/benitoiteh/src/`）を変えない。

## 読む設計書

- [性能](../../design/06-impl/06-02-performance.md)（「目標」「測り方」）
- [クレートの構成](../../design/06-impl/06-01-crate-structure.md)の `[profile.release]` の記述、[未決事項](../../design/open-issues.md)の BH-060
- [テスト](../../design/07-quality/07-01-testing.md)の「模擬の提供者」
- 既存のもの:
  - 大きさの測定: `docs/plan/tasks/P0-06-size-probe.md`（「結果」の書き方を雛形にする）、`scripts/check.sh` の `size` の段
  - 模擬の提供者: `crates/benitoiteh/tests/support/mock_provider.rs`（`Delivery::Chunks`、`requests()`）、`tests/support/exec_fixture.rs`（`Protocol`、`Fixture::new`・`Fixture::restricted`）
  - pty: `tests/support/tty.rs`、`tests/tui/pty.rs`（`"Sandbox:"` を入力を受けられる合図にしている）
  - 検索の測定: `tests/tools_search_measure.rs`
  - 測定の手順の例: `tools/bench-write-check.sh`、`examples/bench_write_check.rs`

## この作業が認める型の変更

- テストの支えの模擬の提供者（`tests/support/mock_provider.rs`）に、要求を受けた時刻（`std::time::Instant`）と、`Delivery::Chunks` の各塊を書き終えた時刻を記録する欄と、それを読む関数を加えてよい。既存のテストの振る舞いは変えない。
- 製品のコードの型は変えない。測定に製品のコードの変更（準備が終わった印、時刻のログなど）が要ると分かったら、00-common.md の「止まるとき」に従って止まる。

## 作るもの

### 1. 測定のテスト（`crates/benitoiteh/tests/perf_measure.rs`）

- どのテストも `#[ignore]` を付け、`cargo test --release -p benitoiteh --test perf_measure -- --ignored --nocapture --test-threads=1` で走らせる手動の測定とする。
- 測る実行ファイルは `env!("CARGO_BIN_EXE_benitoiteh")` とし、環境変数 `BENITOITEH_PERF_BINARY` があればそれを使う（`opt-level = "s"` のビルドを測るため）。
- 繰り返しの回数は環境変数 `BENITOITEH_PERF_RUNS`（既定 10、最小 1）で決める。各測定の前に一回、結果に含めない温めの走行を行う。
- 各テストは、測った値を一行に一つの JSON で、`PERF ` を前に付けて標準出力に書く。形は `{"item": "<項目>", "unit": "ms" か "KiB", "samples": [...], "note": "<条件>"}` とする。
- 設定、状態、一時ディレクトリは、どのテストも一時ディレクトリに向ける（利用者の `~/.config/benitoiteh` を読まない）。

測る項目は次のとおりである。

| `item` | 測り方 | 06-02 の目標 |
|---|---|---|
| `version_ms` | `benitoiteh --version` を起動してから終わるまで | 5 ms |
| `exec_first_request_ms` | `Fixture::new` の形（通常モード、処理系なし）で `exec` を起動してから、模擬の提供者が最初の要求を受けるまで | 30 ms（処理系の起動を除く） |
| `exec_first_request_restricted_ms` | `Fixture::restricted` の形（本物の処理系。`BENITOITEH_TEST_BENITOITE`）で同じもの。参考の値とし、`note` に処理系の三回の起動（`--version=json`・`skill list`・`skill show`）を含むことを書く | 参考 |
| `tui_ready_ms` | pty で対話の画面を起動してから、出力に `Sandbox:` とプロンプトの `> ` の両方が現れるまで。制限モード、本物の処理系 | 150 ms |
| `tui_idle_rss_kib` | `tui_ready_ms` の後に 1 秒待ち、`ps -o rss= -p <pid>` で読む | 20 MiB |
| `session_200_max_rss_kib` | 模擬の提供者で 199 回の道具の呼び出し（作業ディレクトリの 2 KiB 程度のファイルの `read`）と最後の文の応答を返し、`exec --max-turns 200` を通常モード（処理系なし。子プロセスを起動しない）で走らせ、`/usr/bin/time -l` の `maximum resident set size` を読む。回数は 3 回でよい | 100 MiB |
| `stream_delta_ms` | pty の対話の画面で、模擬の提供者が文の差分を 100 ms の間隔で 20 回流す。各差分に固有の印（例 `M01`〜`M20`）を含め、模擬の提供者が塊を書き終えた時刻から、その印が pty の出力に現れるまでを測る | 16 ms |
| `grep_10k_ms` | 1 万のファイル（16 個のディレクトリに 625 個ずつ。各 1 KiB 程度。うち 100 個に一致する行を含める）の木で、`grep` の道具を一回呼ぶ時間。木を作った直後の一回目と、その後の繰り返しを分けて書く（`note` に合計の大きさと除外の数を書く） | 1 秒 |

- `grep_10k_ms` は、既存の `tests/tools_search_measure.rs` に加えるか、その呼び方を写す。どちらでも、`PERF` の行を出す。
- 時刻は `std::time::Instant` で測る。pty の出力は、非ブロックで読み、待つ間は 1 ms 以下の間隔で調べる（測る値を荒くしない）。
- `stream_delta_ms` について: 対話の画面は、処理中に `crossterm::event::poll` で最大 33 ms 待つ（`src/ui/tui/runtime.rs` の 217 行目あたり）ので、目標を満たさない見込みがある。測った値をそのまま書く（直すのは別の作業とする）。
- macOS だけで走らせる。ほかの環境では、テストの先頭で理由を書いて飛ばす。

### 2. 集める道具（`tools/perf/measure.py`）

- python3 3.9 の標準ライブラリだけで書く。
- 引数: `--benitoite <処理系>`（必須）、`--runs N`（既定 10）、`--out <Markdown のファイル>`（既定は標準出力）、`--opt-s`（`opt-level = "s"` のビルドも測る）。
- 行うこと:
  1. 環境を集める: `rustc -V`、`git rev-parse HEAD`、`Cargo.lock` の SHA-256、`sw_vers`、`sysctl -n machdep.cpu.brand_string`、処理系の版と SHA-256。
  2. `cargo build --release -p benitoiteh` を行い、`target/release/benitoiteh` の大きさ（バイトと MiB）を書く。
  3. `cargo test --release -p benitoiteh --test perf_measure -- --ignored --nocapture --test-threads=1` を、`BENITOITEH_TEST_BENITOITE` と `BENITOITEH_PERF_RUNS` を設定して走らせ、`PERF ` の行を集める（`tools_search_measure` に置いたときは、そのテストも走らせる）。
  4. `--opt-s` のときは、同じことを `--config 'profile.release.opt-level="s"'` と `--target-dir target/perf-opt-s` で行う（`Cargo.toml` は変えない）。
  5. 項目ごとに、回数、中央値、最大値、目標、目標を満たしたか、を表にした Markdown を書く。`--opt-s` のときは二つの列を並べる。
- `PERF` の行が欠けた項目は、表に「測れなかった」と書き、終了状態を 1 にする。
- `tools/perf/test_measure.py`: 中央値と最大値、`PERF` の行の読み取り（壊れた行、欠けた項目）、表の組み立てを、決まった入力で確かめる。

### 3. 検査への組み込み

- `scripts/check.sh` に、`python3 -I -m unittest discover -s tools/perf` を加える（測定そのものは走らせない。時間がかかり、値がゆらぐため）。
- `perf_measure.rs` は `#[ignore]` なので、通常の `cargo test` ではコンパイルだけされる。

### 4. 文書

- `tools/perf/README.md` に、走らせ方、各項目の測り方と条件（06-02 の目標との違い。例えば `tui_ready_ms` は処理系の三回の起動を含み、`exec_first_request_ms` は処理系なしの通常モードで測ること）、結果を `docs/plan/results/P5-perf.md` に書くことを書く。

## 触るファイル

- `crates/benitoiteh/tests/perf_measure.rs`（新規）
- `crates/benitoiteh/tests/support/mock_provider.rs`（時刻の記録を加える）
- `crates/benitoiteh/tests/tools_search_measure.rs`（`grep_10k_ms` をここに置くとき）
- `tools/perf/measure.py`、`tools/perf/test_measure.py`、`tools/perf/README.md`（新規）
- `scripts/check.sh`

## テスト

- `tools/perf/test_measure.py`（「作るもの」の 2）。
- `perf_measure.rs` の各測定を、`BENITOITEH_PERF_RUNS=1` で一度ずつ走らせ、`PERF` の行が出ることを確かめる（Codex のサンドボックスの中で pty やループバックが使えないものは、飛ばしたことを完了の報告に書く）。
- 模擬の提供者の時刻の欄を加えた後も、既存のテストが通ること。

## 完了条件

- `scripts/check.sh --inside-codex` が通る（`BENITOITEH_TEST_BENITOITE` を設定して）。
- 完了の報告に、`BENITOITEH_PERF_RUNS=1` で走らせた `PERF` の行（走らせられたもの）と、`measure.py` が作った表の例を書く。値はサンドボックスの中のものであり、本番の値ではないと明記する。

## 補足（2026-10-19、始める前の点検）

本節は、上の各節に優先する。

1. **模擬の提供者に足すものは、使われない警告を止める**: `.cargo/config.toml` が `warnings = "deny"` であり、`agent_loop.rs`・`exec.rs`・`provider_anthropic.rs`・`provider_openai_chat.rs`・`provider_openai_responses.rs` は `#[allow(dead_code)]` なしで `mod support;` を取り込む。`mock_provider.rs` に足す欄とメソッドには、一つずつ `#[allow(dead_code)]` を付ける。既存のテストのクレートは変えない。足すものは、`RecordedRequest` の `accepted_at`（接続を受け付けた直後）と `received_at`（要求を読み終えた時刻）、台本ごとに塊を書き終えた時刻の列を返す関数とする。
2. **`perf_measure.rs` の取り込み方**: `#[allow(dead_code)] mod support;`、`#[allow(dead_code, unused_imports)] #[path = "support/exec_fixture.rs"] mod fixture;`、`#[allow(dead_code)] #[path = "support/tty.rs"] mod tty;` の形とする。`support` の名前は変えない（`exec_fixture.rs` が `super::support` を参照する）。`exec_fixture.rs` と `tty.rs` は変えない。
3. **実行ファイルの指定**: `Fixture::command` は `CARGO_BIN_EXE_benitoiteh` に固定しているので使わない。同じ作業ディレクトリと環境変数（`HOME`・`XDG_CONFIG_HOME`・`XDG_STATE_HOME`・`TMPDIR`・`BENITOITEH_EXEC_TEST_KEY`）と、通常モードでは `--no-sandbox` を与える `Command` を自分で組み立てる。走行ごとに新しい一時ディレクトリを作る（溜まったセッションが起動の掃除を重くしないように）。
4. **`exec_first_request_ms` は専用の待ち受けで測る**: 模擬の提供者は受け付けを 5 ms ごとにしか調べない（`TICK`）ので、30 ms の目標に対して誤差が大きい。`perf_measure.rs` の中に、ブロックする `accept` で一本だけ受け付ける待ち受けを作り、`spawn` の直前の時刻から受け付けた直後の時刻までを項目の値とする。要求を読み終えた時刻までの値は `note` に添える。設定は `Fixture::new`（制限モードの項目は `Fixture::restricted`）と同じ内容を自分で書く。応答は `Protocol::Chat` の最後の応答（`Connection: close`）とする。
5. **`session_200_max_rss_kib` の台本と単位**: 作業ディレクトリに 2 KiB 程度の別々のファイル `f001.txt`〜`f199.txt` を置き、`Protocol::Chat` で `call_id` の違う `read` の呼び出しを 199 個と、最後の文の応答を一つ並べる（同じ引数を続けると 3 回目から注意の文が付く）。設定に `context_window` を足さない（自動の圧縮が働き、台本が狂う）。macOS の `/usr/bin/time -l` の `maximum resident set size` はバイトなので、1024 で割って KiB にする。回数は `min(BENITOITEH_PERF_RUNS, 3)` とする。終了状態が 0 であることと、要求がちょうど 200 個あることを確かめる。待ち時間の上限は 120 秒とする。
6. **擬似端末の読み方**: `Pty::interact` と `capture` は 5 ms ずつ眠るので使わない。`Pty::input()` の複製（非ブロック）を 1 ms 以下の間隔で読む。子は `process_group(0)` で直接起動し、終わりに `/quit\r` を送り、終わらなければグループごと止めて回収する。
7. **`tui_ready_ms` と `tui_idle_rss_kib`**: 一つのテストで両方の行を出す。制限モードで起動し（`--mode`・`--no-sandbox`・`--sandbox`・`--approval` は付けない）、設定の `[benitoite] path` に本物の処理系を書き、`base_url = 'http://127.0.0.1:1/v1'` とする（起動時は要求を送らないので、模擬の提供者を立てない）。合図は、出力の全体に `Sandbox:` と `"> "` の両方が初めて揃った時刻とする。合図の前に子が終わったら、出力を添えて失敗させる。`note` に、処理系の三回の起動（`--version=json`・`skill list`・`skill show`）をどれも `sandbox-exec` の中で行うことを書く。RSS は `ps -o rss= -p <child.id()>`（KiB）で読む。
8. **`stream_delta_ms` の組み立て**: 通常モードで `--sandbox danger-full-access --no-sandbox --approval never` を付け、`env_clear` と `PATH=""` で起動する（`tests/tui/pty.rs` と同じ形）。`Sandbox:` が見えた後に `b"Stream the markers\r"` を書く。応答は Chat の差分の事象 20 個で、内容は `"M01\n"`〜`"M20\n"` とする。どれも同じバイト長なので、`Delivery::Chunks { size: <一つの事象の長さ>, delay: 100 ms }` で一つの塊に事象が一つ入る。標本は、i 番目の塊を書き終えた時刻から、`M{i:02}` が出力に初めて現れた時刻までとし、一回の走行で 20 個とする。
9. **`grep` は `perf_measure.rs` に置き、項目を二つに分ける**: `tools_search_measure.rs` は変えない（「触るファイル」から除く）。一致する行は 100 ファイルに 1 行ずつ、合計ちょうど 100 行にする（101 件目で走査を止めるため）。結果に打ち切りの文（`Search stopped` など）がないことを確かめる。既定の除外に当たるファイルは置かず、`note` に `excluded=0` と合計のバイト数を書く。木を作った直後の一回を `grep_10k_first_ms`（標本 1 個、温めの走行に含めない）、その後の繰り返しを `grep_10k_ms` とする。`note` に「作成の直後（ページキャッシュに残る）」と書く。どちらも目標は 1 秒とする。
10. **`opt-level = "s"` との比べと `BENITOITEH_PERF_BINARY`**: `"s"` の比べは、`--config 'profile.release.opt-level="s"' --target-dir target/perf-opt-s` を付けた `cargo build` と `cargo test` で行う（`grep` はテストのプロセスの中で測るので、環境変数だけでは比べられない）。`measure.py` は、どちらの構成でも `cargo build --offline --locked --release -p benitoiteh` の直後に、実行ファイルを `target/perf/benitoiteh-<構成>` へ写し、大きさと SHA-256 を記録し、写したファイルを `BENITOITEH_PERF_BINARY` に渡す（`cargo test` が同じ出力先を作り直しても、大きさと速さを同じ実行ファイルで測るため）。`cargo` は、シェルを通さずに引数の列で起動する。
11. **`PERF` の行の読み取り**: テストは `PERF` の行の前に改行を一つ出す。`measure.py` は、行の中のどこにあっても `PERF {` を探し、そこから行の終わりまでを JSON として読む（libtest が `test 名前 ... ` を改行なしで先に書くため）。時間は ms の小数、メモリは KiB の整数とする。`note` に測った実行ファイルのパスを含める。
12. **判定と表**: 判定は中央値で行い、最大値が目標を超えたときはそのことを書き添える。目標は KiB に直して比べる（20 MiB = 20480 KiB、100 MiB = 102400 KiB）。`exec_first_request_restricted_ms` は判定しない。`cargo test` が失敗しても、集めた行で表を作り、終了状態を 1 にする。環境の記録に、`[profile.release]` の値、`--config` による差し替え、`git status --porcelain` が空かどうかを加える。
13. **回数**: `BENITOITEH_PERF_RUNS` が 0 か数でないときは、理由を書いてテストを失敗させる。温めの走行は、`session_200_max_rss_kib` と `grep_10k_first_ms` を除く全項目で一回行う。
14. **Codex の中で測れないもの**: 127.0.0.1 の待ち受けが `EPERM` になる場合は、待ち受けを使う項目が測れない。入れ子のサンドボックスが使えないので、制限モードの項目（`tui_ready_ms`・`tui_idle_rss_kib`・`exec_first_request_restricted_ms`）も測れない。これらは理由を添えた `Err` でテストを失敗させ、Codex を見分ける分岐は作らない。完了の報告に、項目ごとに走らせた結果を書く。
15. **lint**: `perf_measure.rs` にも、ワークスペースの clippy の lint（`arithmetic_side_effects`・`indexing_slicing`・`unwrap_used` など）が、Linux の二つの対象を含めて掛かる。`saturating_*`・`.get()`・`?` で書く。`/usr/bin/time` と `ps` は、実行時に macOS かを見て使う（Linux でもコンパイルが通るように）。各テストには、`tools_search_measure.rs` と同じ形で、測定の道具であり時間の閾値で判定しないという作成時の関門のコメントを付ける。
16. **`check.sh`**: `skill-eval` の段の後に `begin_stage perf-tools` を設け、`python3 -I -m unittest discover -s tools/perf -p 'test_*.py'` を走らせる（`--inside-codex` でも走らせる）。`test_measure.py` は、`test_skill_eval.py` と同じく、自分のディレクトリを `sys.path` の先頭に入れてから `measure` を取り込む。
