# P5-05 性能の測定の結果（2026-10-19）

設計書 06-02「目標」の各項目を、`tools/perf/measure.py`（P5-05）で、サンドボックスの外の手元の機械（Apple M4 の Mac mini、macOS 27.0.1）で測った。今の設定（`opt-level = 3`）と、大きさを優先する設定（`opt-level = "s"`、BH-060）を並べた。測ったのはオーケストレータで、コミット `069c0fc` の時点である。

## 要点

| 項目 | 目標 | `opt-level = 3` | `opt-level = "s"` |
|---|---:|---:|---:|
| 実行ファイルの大きさ | 8 MiB | 5.72 MiB（5,999,872 バイト） | 4.68 MiB（4,911,408 バイト） |
| `--version` の時間 | 5 ms | 3.5 ms | 3.8 ms |
| `exec` から最初の要求まで（処理系なし） | 30 ms | 6.4 ms | 6.6 ms |
| 同（制限モード、処理系の三回の起動を含む。参考） | — | 50.3 ms | 50.1 ms |
| 対話の画面が入力を受けられるまで | 150 ms | 79.6 ms | 80.7 ms |
| 待っている間のメモリ | 20 MiB | 10.0 MiB | 9.6 MiB |
| 200 番のセッションの最大のメモリ | 100 MiB | 15.9 MiB | 15.8 MiB |
| ストリーミングの差分から表示まで | 16 ms | **17.9 ms（未達）** | **17.6 ms（未達）** |
| `grep` で 1 万のファイル（一回目 / 繰り返し） | 1 秒 | 256 ms / 192 ms | 213 ms / 210 ms |

値はどれも中央値である（大きさを除く）。

- **ストリーミングの表示だけが目標に届かなかった。** 標本（200 個）は 1〜36 ms にほぼ一様に散らばり、最大値は約 36 ms だった。対話の画面は、処理中にキーの入力を最大 33 ms 待ち、その待ちの間に届いたエージェントの事象を、待ちが終わってから描く（`src/ui/tui/runtime.rs` の 217 行目、`src/cli/tui.rs` の 33 行目）。標本の散らばりは、この待ちの長さと合う。直すには、エージェントの事象が届いたときに待ちを起こす作り（事象とキーの入力を一つの待ちで受ける、など）が要る（BH-106）。
- **ほかの項目は、どちらの設定でも目標を満たした。** `--version` の最大値（7.0 ms）は、温めの後の最初の数回のものである。
- **`opt-level = "s"`**: 実行ファイルは 18.1% 小さくなった（P0-06 の試しの 18.0% とほぼ同じ）。時間とメモリの差は、`grep` の繰り返し（192 ms と 210 ms、約 9% 遅い）を除いて、ゆらぎの範囲だった。`grep` の一回目は逆に `"s"` のほうが速く、一回だけの標本なので差とは見なさない。
- **制限モードの起動**: 処理系の三回の起動（`--version=json`・`skill list`・`skill show`。どれも `sandbox-exec` の中）で、約 44 ms がかかる（処理系なしの 6.4 ms との差）。対話の画面の 79.6 ms の半分以上を占める。

## 条件

- 時間は 10 回（200 番のセッションは 3 回、ストリーミングは 10 回 × 20 個）の中央値と最大値。各項目で温めの走行を一回行い、標本から除いた。
- 模擬の提供者（ループバックの HTTP。TLS なし）を使い、本物の提供者は呼んでいない（設計書 06-02「測り方」）。最初の要求までの時間は、TLS の接続の確立を含まない。
- 200 番のセッションは、2 KiB のファイルを 199 回読む筋書きであり、大きな内容を読むセッションのメモリはこれより大きくなりうる。
- `grep` の一回目は木を作った直後であり、ファイルシステムのキャッシュは温かい。

以下は `measure.py` が書いた報告そのものである（手元の絶対パスを `<repo>` と `~` に置き換えた）。

---


測定値はこの実行環境の値である。判定は中央値で行う。

## 環境

| 項目 | 値 |
|---|---|
| UTC | 2026-10-19T02:01:14.904161+00:00 |
| Rust | rustc 1.98.1 (48a229cea 2026-09-01) |
| commit | 069c0fc8bcedc1f2bf30e7071c56eadd72e0faa2 |
| macOS | ProductName:		macOS<br>ProductVersion:		27.0.1<br>BuildVersion:		26A434 |
| CPU | Apple M4 |
| benitoite version | {"build":{"commit":null,"dirty":null,"provenance":{"commit":"unknown","dirty":"notCollected"},"target":"aarch64-apple-darwin"},"formats":{"checkReport":[1],"diagnostics":[1],"effectLimit":[1],"loaderEnvironment":[1],"skillDocument":[1],"skillList":[1],"testCheckReport":[1],"testResults":[1],"version":[1]},"kind":"version","loaderEnvironment":{"deniedPrefixes":["DYLD_","LD_"],"errorKind":"InvalidInput","inherited":"unchanged","mode":"always","nameComparison":"asciiCaseInsensitive","schemaVersion":1},"name":"benitoite","schemaVersion":1,"unicodeVersion":"17.0.0","version":"0.0.7"} |
| working tree clean | True |
| Cargo.lock SHA-256 | 32b5f03a8090ca4e739f77744a8d420331be8bc8ddc4c93d91c34921a7a9cf87 |
| benitoite path | <repo>/work/benitoite-v0.0.7/target/release/benitoite |
| benitoite bytes | 30097536 |
| benitoite SHA-256 | ee8eaf0eb4e624be571b14198e28f1078fb6c6791c42b88ab77a015ea0e8f46e |
| [profile.release] | opt-level = 3<br>lto = "fat"<br>codegen-units = 1<br>panic = "abort"<br>strip = "symbols" |
| features | Cargo default features; crates/benitoiteh/Cargo.toml at the recorded commit and working tree |
| cargo target | host; CARGO_TARGET_DIR and CARGO_BUILD_TARGET removed |
| runs | 10 |
| --config override | profile.release.opt-level="s" (opt-s only) |

## 測定値

| 項目 | 単位 | 目標 | default 回数 | default 中央値 | default 最大値 | default 判定 | opt-s 回数 | opt-s 中央値 | opt-s 最大値 | opt-s 判定 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| binary_size_mib | MiB | 8 | 1 | 5.722 | 5.722 | 達成 | 1 | 4.684 | 4.684 | 達成 |
| version_ms | ms | 5 | 10 | 3.492 | 7.015 | 達成（最大値は目標超過） | 10 | 3.789 | 7.054 | 達成（最大値は目標超過） |
| exec_first_request_ms | ms | 30 | 10 | 6.356 | 13.728 | 達成 | 10 | 6.577 | 13.325 | 達成 |
| exec_first_request_restricted_ms | ms | 参考 | 10 | 50.272 | 51.767 | 参考（判定なし） | 10 | 50.097 | 50.613 | 参考（判定なし） |
| tui_ready_ms | ms | 150 | 10 | 79.590 | 84.424 | 達成 | 10 | 80.701 | 83.281 | 達成 |
| tui_idle_rss_kib | KiB | 20480 | 10 | 10216.000 | 10304.000 | 達成 | 10 | 9832.000 | 9904.000 | 達成 |
| session_200_max_rss_kib | KiB | 102400 | 3 | 16320.000 | 16400.000 | 達成 | 3 | 16176.000 | 16224.000 | 達成 |
| stream_delta_ms | ms | 16 | 200 | 17.864 | 36.463 | 未達（最大値は目標超過） | 200 | 17.604 | 36.568 | 未達（最大値は目標超過） |
| grep_10k_first_ms | ms | 1000 | 1 | 255.735 | 255.735 | 達成 | 1 | 213.222 | 213.222 | 達成 |
| grep_10k_ms | ms | 1000 | 10 | 192.450 | 196.518 | 達成 | 10 | 210.373 | 254.333 | 達成 |

## 条件と失敗

### default

- `binary_size_mib`: bytes=5999872; SHA-256=073c1a9a639a71b0cbb843c4159055f7f2f10fdb99aa0afeba1cfda036d2263a; binary=<repo>/target/perf/benitoiteh-default; --config=(none)
- `exec_first_request_ms`: spawn to blocking accept; normal; no interpreter; --no-sandbox; request fully received ms=[13.802999999999999, 9.910166, 7.574167, 7.15275, 6.497917, 6.271583, 6.2531669999999995, 6.2888340000000005, 5.705834, 5.733458000000001]; one warm-up; binary=<repo>/target/perf/benitoiteh-default
- `exec_first_request_restricted_ms`: spawn to blocking accept; restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; request fully received ms=[50.728916999999996, 50.31875, 50.616708, 50.37325, 50.022083, 50.391833, 49.933917, 50.105625, 50.123124999999995, 51.83575]; one warm-up; binary=<repo>/target/perf/benitoiteh-default
- `grep_10k_first_ms`: 10000 files, 16 directories x 625; total_bytes=10240000; excluded=0; exactly 100 matching lines; 作成の直後（ページキャッシュに残る）; grep runs inside test binary=<repo>/target/release/deps/perf_measure-65e42736d0e932ee; binary=<repo>/target/perf/benitoiteh-default
- `grep_10k_ms`: 10000 files, 16 directories x 625; total_bytes=10240000; excluded=0; exactly 100 matching lines; 作成の直後（ページキャッシュに残る）; grep runs inside test binary=<repo>/target/release/deps/perf_measure-65e42736d0e932ee; repeated after first traversal and one warm-up; binary=<repo>/target/perf/benitoiteh-default
- `session_200_max_rss_kib`: normal; no interpreter or tool child processes; 199 distinct 2 KiB reads + final response; exactly 200 requests; /usr/bin/time -l bytes / 1024; no warm-up; at most three runs; binary=<repo>/target/perf/benitoiteh-default
- `stream_delta_ms`: normal; no interpreter; danger-full-access, --no-sandbox, approval never; 20 unique newline markers per run at 100 ms intervals; one warm-up run; pty polling <= 1 ms; binary=<repo>/target/perf/benitoiteh-default
- `tui_ready_ms`: restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; ready=Sandbox: and > ; one warm-up; binary=<repo>/target/perf/benitoiteh-default
- `tui_idle_rss_kib`: restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; ready=Sandbox: and > ; one warm-up; ps -o rss= one second after ready; binary=<repo>/target/perf/benitoiteh-default
- `version_ms`: spawn to exit; one warm-up; fresh isolated directories per run; binary=<repo>/target/perf/benitoiteh-default

標本（PERF の JSON）:

```jsonl
{"item": "binary_size_mib", "unit": "MiB", "samples": [5.721923828125], "note": "bytes=5999872; SHA-256=073c1a9a639a71b0cbb843c4159055f7f2f10fdb99aa0afeba1cfda036d2263a; binary=<repo>/target/perf/benitoiteh-default; --config=(none)"}
{"item": "exec_first_request_ms", "note": "spawn to blocking accept; normal; no interpreter; --no-sandbox; request fully received ms=[13.802999999999999, 9.910166, 7.574167, 7.15275, 6.497917, 6.271583, 6.2531669999999995, 6.2888340000000005, 5.705834, 5.733458000000001]; one warm-up; binary=<repo>/target/perf/benitoiteh-default", "samples": [13.728167, 9.830833, 7.528709, 7.107417000000001, 6.454167, 6.236583, 6.2191670000000006, 6.2568340000000005, 5.667333999999999, 5.69575], "unit": "ms"}
{"item": "exec_first_request_restricted_ms", "note": "spawn to blocking accept; restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; request fully received ms=[50.728916999999996, 50.31875, 50.616708, 50.37325, 50.022083, 50.391833, 49.933917, 50.105625, 50.123124999999995, 51.83575]; one warm-up; binary=<repo>/target/perf/benitoiteh-default", "samples": [50.658500000000004, 50.248833, 50.545208, 50.294375, 49.943541, 50.321708, 49.862334000000004, 50.04025, 50.056416999999996, 51.76675], "unit": "ms"}
{"item": "grep_10k_first_ms", "note": "10000 files, 16 directories x 625; total_bytes=10240000; excluded=0; exactly 100 matching lines; 作成の直後（ページキャッシュに残る）; grep runs inside test binary=<repo>/target/release/deps/perf_measure-65e42736d0e932ee; binary=<repo>/target/perf/benitoiteh-default", "samples": [255.73537499999998], "unit": "ms"}
{"item": "grep_10k_ms", "note": "10000 files, 16 directories x 625; total_bytes=10240000; excluded=0; exactly 100 matching lines; 作成の直後（ページキャッシュに残る）; grep runs inside test binary=<repo>/target/release/deps/perf_measure-65e42736d0e932ee; repeated after first traversal and one warm-up; binary=<repo>/target/perf/benitoiteh-default", "samples": [196.51770900000002, 195.023834, 192.506834, 193.9285, 191.980042, 191.57125, 190.08279100000001, 192.392958, 195.20375, 191.51262499999999], "unit": "ms"}
{"item": "session_200_max_rss_kib", "note": "normal; no interpreter or tool child processes; 199 distinct 2 KiB reads + final response; exactly 200 requests; /usr/bin/time -l bytes / 1024; no warm-up; at most three runs; binary=<repo>/target/perf/benitoiteh-default", "samples": [16192, 16320, 16400], "unit": "KiB"}
{"item": "stream_delta_ms", "note": "normal; no interpreter; danger-full-access, --no-sandbox, approval never; 20 unique newline markers per run at 100 ms intervals; one warm-up run; pty polling <= 1 ms; binary=<repo>/target/perf/benitoiteh-default", "samples": [28.911083, 32.140125, 35.709708, 3.062417, 3.878667, 7.660084, 10.609957999999999, 13.569291000000002, 18.442, 18.189541, 20.457459, 22.540708, 24.632667, 28.796875, 33.264833, 1.467416, 2.618292, 4.28, 9.920958, 11.904875, 32.492541, 35.512665999999996, 3.9515000000000002, 7.133417000000001, 9.797542, 10.483208000000001, 10.860042, 12.2395, 14.954749999999999, 18.143542, 20.628332999999998, 26.189125, 29.411417, 31.645417000000002, 34.961416, 34.152083, 35.295708000000005, 4.5745, 3.526542, 8.931375000000001, 34.384583000000006, 36.286, 4.279916, 6.468083, 7.969208, 8.773292, 12.460834, 15.544749999999999, 15.404292, 15.304292, 17.083292, 17.492666, 22.271833, 24.471041999999997, 27.810458999999998, 29.893875, 32.47475, 1.923416, 3.174792, 4.944375, 29.653624999999998, 30.618583, 35.432041999999996, 2.788708, 2.423708, 4.904292, 5.136583, 6.6302080000000005, 8.586083, 10.876415999999999, 13.877667, 15.950042000000002, 17.881999999999998, 20.965042, 22.094208000000002, 27.28325, 30.168833, 33.028375, 1.642375, 5.379375, 31.730166999999998, 31.850084000000003, 1.8731659999999999, 5.984625, 8.191624999999998, 10.121667, 13.220457999999999, 17.979875, 21.271709, 20.468041, 21.435667, 24.347542, 25.026125, 27.519125, 31.719042, 35.788916, 2.45675, 5.984500000000001, 7.843125, 12.202, 27.788166, 30.033, 32.028417000000005, 33.877334000000005, 3.018459, 3.616875, 8.6385, 10.5495, 9.660458, 13.764667, 16.066667, 18.223125, 19.302792, 23.720833, 23.980709, 27.469458, 28.539291000000002, 32.655667, 36.463042, 2.6272919999999997, 28.769917, 34.585625, 2.639292, 4.528083, 8.378166, 9.336333, 13.313709, 16.101375, 20.352917, 22.7205, 23.974375, 26.843417000000002, 31.727292, 34.843958, 3.005375, 6.455334, 8.014375000000001, 11.604708, 14.944416, 15.237957999999999, 30.5155, 34.278707999999995, 34.74775, 3.760084, 3.5270420000000002, 6.015208, 7.216832999999999, 11.354375, 13.420958, 13.753459000000001, 16.888125, 20.415875, 21.484916, 24.2215, 25.214625, 29.527500000000003, 30.999208, 33.854166, 33.414667, 34.536542000000004, 31.984375000000004, 35.414208, 4.542916, 6.9222909999999995, 8.807666000000001, 10.324290999999999, 11.69075, 13.807125, 15.211875, 17.84625, 22.315834, 26.99525, 31.056167, 30.975375, 31.440584, 31.838207999999998, 35.475209, 4.277375, 6.4545840000000005, 7.6006670000000005, 29.95375, 32.099416, 34.897084, 3.9465000000000003, 7.969041, 9.834584, 12.306708, 11.271542, 12.123083, 16.835958, 20.038332999999998, 21.119584, 26.040292, 26.978458, 28.7, 29.043, 31.824875000000002, 34.611958, 35.202999999999996, 4.702875], "unit": "ms"}
{"item": "tui_ready_ms", "note": "restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; ready=Sandbox: and > ; one warm-up; binary=<repo>/target/perf/benitoiteh-default", "samples": [84.4245, 79.58295799999999, 83.350583, 81.196, 78.70075, 81.323208, 78.01712500000001, 79.597209, 75.842209, 78.52850000000001], "unit": "ms"}
{"item": "tui_idle_rss_kib", "note": "restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; ready=Sandbox: and > ; one warm-up; ps -o rss= one second after ready; binary=<repo>/target/perf/benitoiteh-default", "samples": [10208, 10192, 10304, 10256, 10096, 10112, 10224, 10176, 10272, 10272], "unit": "KiB"}
{"item": "version_ms", "note": "spawn to exit; one warm-up; fresh isolated directories per run; binary=<repo>/target/perf/benitoiteh-default", "samples": [7.015375, 5.511458999999999, 4.795458, 4.222375, 3.566291, 3.418, 3.4008749999999996, 3.3414159999999997, 3.1870000000000003, 3.029333], "unit": "ms"}
```

### opt-s

- `binary_size_mib`: bytes=4911408; SHA-256=f9e77e236b6e1292a8f5648b03945f8d78a78f517a95a1ff7b9aeba67145d63e; binary=<repo>/target/perf/benitoiteh-opt-s; --config=profile.release.opt-level="s"
- `exec_first_request_ms`: spawn to blocking accept; normal; no interpreter; --no-sandbox; request fully received ms=[13.3925, 9.588332999999999, 8.173083, 7.100916, 6.752208, 6.331207999999999, 6.491917, 5.941333, 5.929167, 5.978625]; one warm-up; binary=<repo>/target/perf/benitoiteh-opt-s
- `exec_first_request_restricted_ms`: spawn to blocking accept; restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; request fully received ms=[50.102833, 50.091417, 50.235667, 50.686375, 50.312208999999996, 50.020042000000004, 50.117208000000005, 50.278959, 50.409, 50.014583]; one warm-up; binary=<repo>/target/perf/benitoiteh-opt-s
- `grep_10k_first_ms`: 10000 files, 16 directories x 625; total_bytes=10240000; excluded=0; exactly 100 matching lines; 作成の直後（ページキャッシュに残る）; grep runs inside test binary=<repo>/target/perf-opt-s/release/deps/perf_measure-f600e0cc62dca617; binary=<repo>/target/perf/benitoiteh-opt-s
- `grep_10k_ms`: 10000 files, 16 directories x 625; total_bytes=10240000; excluded=0; exactly 100 matching lines; 作成の直後（ページキャッシュに残る）; grep runs inside test binary=<repo>/target/perf-opt-s/release/deps/perf_measure-f600e0cc62dca617; repeated after first traversal and one warm-up; binary=<repo>/target/perf/benitoiteh-opt-s
- `session_200_max_rss_kib`: normal; no interpreter or tool child processes; 199 distinct 2 KiB reads + final response; exactly 200 requests; /usr/bin/time -l bytes / 1024; no warm-up; at most three runs; binary=<repo>/target/perf/benitoiteh-opt-s
- `stream_delta_ms`: normal; no interpreter; danger-full-access, --no-sandbox, approval never; 20 unique newline markers per run at 100 ms intervals; one warm-up run; pty polling <= 1 ms; binary=<repo>/target/perf/benitoiteh-opt-s
- `tui_ready_ms`: restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; ready=Sandbox: and > ; one warm-up; binary=<repo>/target/perf/benitoiteh-opt-s
- `tui_idle_rss_kib`: restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; ready=Sandbox: and > ; one warm-up; ps -o rss= one second after ready; binary=<repo>/target/perf/benitoiteh-opt-s
- `version_ms`: spawn to exit; one warm-up; fresh isolated directories per run; binary=<repo>/target/perf/benitoiteh-opt-s

標本（PERF の JSON）:

```jsonl
{"item": "binary_size_mib", "unit": "MiB", "samples": [4.6838836669921875], "note": "bytes=4911408; SHA-256=f9e77e236b6e1292a8f5648b03945f8d78a78f517a95a1ff7b9aeba67145d63e; binary=<repo>/target/perf/benitoiteh-opt-s; --config=profile.release.opt-level=\"s\""}
{"item": "exec_first_request_ms", "note": "spawn to blocking accept; normal; no interpreter; --no-sandbox; request fully received ms=[13.3925, 9.588332999999999, 8.173083, 7.100916, 6.752208, 6.331207999999999, 6.491917, 5.941333, 5.929167, 5.978625]; one warm-up; binary=<repo>/target/perf/benitoiteh-opt-s", "samples": [13.325292000000001, 9.526833, 8.125667, 7.066916, 6.6965829999999995, 6.291667, 6.458417, 5.908583, 5.894875, 5.945333], "unit": "ms"}
{"item": "exec_first_request_restricted_ms", "note": "spawn to blocking accept; restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; request fully received ms=[50.102833, 50.091417, 50.235667, 50.686375, 50.312208999999996, 50.020042000000004, 50.117208000000005, 50.278959, 50.409, 50.014583]; one warm-up; binary=<repo>/target/perf/benitoiteh-opt-s", "samples": [50.027417, 50.018667, 50.165834, 50.612708, 50.246542, 49.950874999999996, 50.028041, 50.194666999999995, 50.342040999999995, 49.947167], "unit": "ms"}
{"item": "grep_10k_first_ms", "note": "10000 files, 16 directories x 625; total_bytes=10240000; excluded=0; exactly 100 matching lines; 作成の直後（ページキャッシュに残る）; grep runs inside test binary=<repo>/target/perf-opt-s/release/deps/perf_measure-f600e0cc62dca617; binary=<repo>/target/perf/benitoiteh-opt-s", "samples": [213.221875], "unit": "ms"}
{"item": "grep_10k_ms", "note": "10000 files, 16 directories x 625; total_bytes=10240000; excluded=0; exactly 100 matching lines; 作成の直後（ページキャッシュに残る）; grep runs inside test binary=<repo>/target/perf-opt-s/release/deps/perf_measure-f600e0cc62dca617; repeated after first traversal and one warm-up; binary=<repo>/target/perf/benitoiteh-opt-s", "samples": [210.266208, 208.90075, 211.191375, 210.005833, 254.33337499999996, 227.30420900000001, 211.357833, 209.255375, 210.480416, 206.243542], "unit": "ms"}
{"item": "session_200_max_rss_kib", "note": "normal; no interpreter or tool child processes; 199 distinct 2 KiB reads + final response; exactly 200 requests; /usr/bin/time -l bytes / 1024; no warm-up; at most three runs; binary=<repo>/target/perf/benitoiteh-opt-s", "samples": [16176, 16016, 16224], "unit": "KiB"}
{"item": "stream_delta_ms", "note": "normal; no interpreter; danger-full-access, --no-sandbox, approval never; 20 unique newline markers per run at 100 ms intervals; one warm-up run; pty polling <= 1 ms; binary=<repo>/target/perf/benitoiteh-opt-s", "samples": [32.987667, 36.037625, 2.6589579999999997, 1.970834, 3.470791, 8.535167, 11.384625, 12.5115, 13.883208, 15.724167000000001, 18.635790999999998, 19.582042, 25.177042, 24.099458000000002, 26.106333, 31.158667, 34.623042, 3.171208, 6.396917, 7.432875, 31.896375, 36.272125, 4.399833, 7.898917, 12.225083, 13.039208, 14.554292, 18.527124999999998, 23.124125, 26.428208, 27.385875000000002, 27.674417, 29.794999999999998, 34.331875, 1.6792090000000002, 5.64225, 10.56, 13.654417, 16.177917, 18.87525, 33.429666, 1.9772499999999997, 6.135666, 9.666417000000001, 11.463084, 14.69025, 17.759875, 20.925042, 21.436417, 24.810458, 27.260165999999998, 29.253500000000003, 32.910459, 4.118708, 7.654042, 8.100958, 13.701166, 16.544875, 21.283625, 22.15075, 30.062792, 30.461458, 31.621457999999997, 36.040542, 4.166708, 6.82125, 6.939584, 8.475416, 13.840708, 17.312292, 16.709792, 19.684666, 22.278916, 23.210041, 27.806250000000002, 31.552625, 34.958957999999996, 3.0083330000000004, 6.091584, 9.722167, 30.379458000000003, 33.123082999999994, 2.18775, 6.703834, 10.068125, 12.27975, 13.981375, 17.159457999999997, 19.290666, 22.416208, 24.843375, 23.794083, 27.457167000000002, 27.180167, 30.550458, 34.358624999999996, 2.229625, 5.48575, 9.857792, 11.497666, 32.741042, 32.675999999999995, 2.214209, 3.820917, 3.20025, 9.1535, 14.363166999999999, 18.583542, 23.726625000000002, 25.134125, 28.235, 27.42425, 29.421416999999998, 30.7075, 32.625084, 2.2552090000000002, 4.5516250000000005, 7.765458000000001, 8.707958999999999, 12.302166, 32.014125, 32.342833, 1.240416, 3.246167, 8.763375, 10.6105, 14.048209, 16.34, 18.073292000000002, 22.471667, 26.484625, 31.097917, 32.935625, 33.127667, 35.483292, 3.374875, 7.581917, 12.449625000000001, 16.672542, 18.776167, 30.392374999999998, 33.689334, 35.050375, 2.930958, 4.555791, 7.01475, 8.825166000000001, 10.254624999999999, 14.370167, 17.448375, 19.775666, 21.546042, 23.406917, 27.803250000000002, 27.875, 32.457208, 35.142167, 2.530875, 6.717542, 7.733333, 29.836499999999997, 31.098209, 34.098000000000006, 2.256125, 6.124917, 8.164, 13.972708, 15.493875000000001, 19.554291000000003, 24.343916, 27.116999999999997, 31.868333999999997, 34.965917, 2.833666, 4.578666, 6.148916, 9.052707999999999, 9.970542, 14.034916999999998, 19.160916, 30.200167, 33.411625, 36.567958999999995, 35.278792, 2.700416, 6.383792, 9.184167, 7.661417, 9.378917000000001, 13.49, 17.173875000000002, 21.481584, 24.315292, 24.057959, 27.885707999999997, 29.472292, 30.1835, 33.111833, 36.417666000000004, 3.014708], "unit": "ms"}
{"item": "tui_ready_ms", "note": "restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; ready=Sandbox: and > ; one warm-up; binary=<repo>/target/perf/benitoiteh-opt-s", "samples": [79.39579099999999, 83.280833, 79.75391699999999, 81.765167, 78.800709, 75.272667, 81.923833, 82.629042, 61.943666, 81.647125], "unit": "ms"}
{"item": "tui_idle_rss_kib", "note": "restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; ready=Sandbox: and > ; one warm-up; ps -o rss= one second after ready; binary=<repo>/target/perf/benitoiteh-opt-s", "samples": [9840, 9824, 9792, 9888, 9824, 9824, 9904, 9808, 9840, 9872], "unit": "KiB"}
{"item": "version_ms", "note": "spawn to exit; one warm-up; fresh isolated directories per run; binary=<repo>/target/perf/benitoiteh-opt-s", "samples": [7.053833, 5.729709000000001, 4.832000000000001, 4.428, 4.073459, 3.5037089999999997, 3.355542, 3.346875, 3.149458, 3.12925], "unit": "ms"}
```

