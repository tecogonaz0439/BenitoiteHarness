# Skill の評価の記録

- 日時: 2026-11-17 04:20
- 処理系: benitoite 0.0.12
- 処理系 SHA-256: 6b26491a8014d9ca88e92caa93c2fee8cc8201098331212071dfdcced8d47e7d
- Skill: 0.0.12 (sha256 f86d1ae3ebf9)
- ハーネス: benitoiteh-strata-iq3s-effort-medium（0.0.1）
- 提供者: {'kind': 'openai-chat', 'base_url': 'http://127.0.0.1:18081/v1', 'api_key': {'env': 'STRATA_API_KEY'}, 'context_window': 262144, 'capabilities': {'reasoning_effort': True}}
- モデル: qwen3.8-flash-next-iq3_s
- effort: medium
- config: -
- max_turns: 50
- max_run_tokens: 2000000
- HARNESS_TIMEOUT: 1800
- 除いた環境変数: OLDPWD, PWD
- CLAUDE_CONFIG_DIR: not set
- 試行の回数: 3
- 生の記録: <work-root>（リポジトリの外）
- completion_check_sha256: 65e20f220a2700e09b7ae6b7f8e2861c6b9d9fbf73bc05c80b420511317e216e
- benitoiteh_version: 0.0.1
- system_prompt_sha256: 85efb17128081963d5bef31a47d02abf0e32cf65a4d47e396a2606eccd7846c5
- tools_sha256: ae9cccf64a96dc2632c08c6f6335b73e664ca11305868e5ec9a44b46c881ff3c
- 条件の順: sequential

| 課題 | 成功 / 試行 | 成功した試行の検査と修正の回数 | 時間の中央値 (秒) | トークンの中央値 | 上限で停止 |
|---|---|---|---|---|---|
| csv_report | 3 / 3 | 1, 1, 1 | 175.6 | 345147 | 0 |
| dir_walk | 3 / 3 | 4, 8, 5 | 505.2 | 780972 | 0 |
| error_handling | 3 / 3 | 2, 3, 1 | 167.8 | 382600 | 0 |
| file_process | 3 / 3 | 1, 1, 1 | 103.2 | 234731 | 0 |
| http_client | 3 / 3 | 1, 1, 1 | 350.5 | 554652 | 0 |
| json_convert | 2 / 3 | 1, 2 | 268.6 | 693626 | 0 |
| json_summary | 3 / 3 | 1, 1, 1 | 115.5 | 172083 | 0 |
| process_run | 3 / 3 | 2, 2, 2 | 76.1 | 225269 | 0 |
| process_shell | 3 / 3 | 5, 3, 4 | 136.7 | 260033 | 0 |
| regex_text | 3 / 3 | 1, 2, 2 | 352.1 | 610217 | 0 |
| write_tests | 3 / 3 | 2, 1, 2 | 485.9 | 771202 | 0 |

費用の見積もりの合計: - USD（費用を取得できた試行の合計）

## 試行ごとの結果

| 課題 | 試行 | 成功 | 番 | 検査と修正 (書いた後の検査) | トークン (キャッシュ読み取り) | 時間 (秒) | 終わり方 | 費用 (USD) | 理由 |
|---|---|---|---|---|---|---|---|---|---|
| csv_report | 1 | yes | 12 | 1 (5) | 345147 (305451) | 175.6 | completed | - | ok |
| csv_report | 2 | yes | 18 | 1 (3) | 440729 (405905) | 331.7 | completed | - | ok |
| csv_report | 3 | yes | 9 | 1 (1) | 210532 (185691) | 133.7 | completed | - | ok |
| dir_walk | 1 | yes | 21 | 4 (5) | 489111 (460589) | 319.6 | completed | - | ok |
| dir_walk | 2 | yes | 30 | 8 (8) | 970194 (924212) | 505.2 | completed | - | ok |
| dir_walk | 3 | yes | 26 | 5 (3) | 780972 (720848) | 828.9 | completed | - | ok |
| error_handling | 1 | yes | 15 | 2 (6) | 308772 (289926) | 157.2 | completed | - | ok |
| error_handling | 2 | yes | 18 | 3 (4) | 382600 (362511) | 167.8 | completed | - | ok |
| error_handling | 3 | yes | 25 | 1 (11) | 545451 (519070) | 299.5 | completed | - | ok |
| file_process | 1 | yes | 14 | 1 (2) | 254464 (242456) | 90.8 | completed | - | ok |
| file_process | 2 | yes | 13 | 1 (1) | 234731 (220967) | 163.0 | completed | - | ok |
| file_process | 3 | yes | 11 | 1 (2) | 193421 (181991) | 103.2 | completed | - | ok |
| http_client | 1 | yes | 27 | 1 (9) | 814153 (772187) | 350.5 | completed | - | ok |
| http_client | 2 | yes | 8 | 1 (1) | 217687 (190142) | 188.4 | completed | - | ok |
| http_client | 3 | yes | 21 | 1 (9) | 554652 (511732) | 475.4 | completed | - | ok |
| json_convert | 1 | no | 22 | 0 (10) | 518062 (491843) | 256.4 | check_fix_limit | - | check failed (exit 2): function parameter names do not match; check_fix_limit: The run stopped because checks of main.bnt failed 10 times without passing (limit: 10; change it with --max-check-fix or limits.max_check_fix). |
| json_convert | 2 | yes | 27 | 1 (9) | 663021 (631516) | 304.6 | completed | - | ok |
| json_convert | 3 | yes | 24 | 2 (13) | 724230 (693316) | 232.7 | completed | - | ok |
| json_summary | 1 | yes | 8 | 1 (1) | 200163 (177879) | 139.8 | completed | - | ok |
| json_summary | 2 | yes | 7 | 1 (1) | 170776 (150670) | 88.7 | completed | - | ok |
| json_summary | 3 | yes | 7 | 1 (1) | 172083 (150641) | 115.5 | completed | - | ok |
| process_run | 1 | yes | 9 | 2 (2) | 225306 (206997) | 59.6 | completed | - | ok |
| process_run | 2 | yes | 9 | 2 (2) | 220087 (200277) | 77.3 | completed | - | ok |
| process_run | 3 | yes | 12 | 2 (3) | 225269 (213511) | 76.1 | completed | - | ok |
| process_shell | 1 | yes | 15 | 5 (4) | 305974 (290385) | 136.7 | completed | - | ok |
| process_shell | 2 | yes | 10 | 3 (3) | 165862 (155563) | 111.8 | completed | - | ok |
| process_shell | 3 | yes | 12 | 4 (2) | 260033 (242200) | 149.2 | completed | - | ok |
| regex_text | 1 | yes | 23 | 1 (6) | 623316 (580927) | 481.6 | completed | - | ok |
| regex_text | 2 | yes | 25 | 2 (10) | 610217 (578729) | 352.1 | completed | - | ok |
| regex_text | 3 | yes | 9 | 2 (3) | 231192 (209473) | 127.1 | completed | - | ok |
| write_tests | 1 | yes | 22 | 2 (4) | 771202 (711251) | 485.9 | completed | - | ok |
| write_tests | 2 | yes | 16 | 1 (2) | 352798 (324827) | 344.1 | completed | - | ok |
| write_tests | 3 | yes | 32 | 2 (8) | 971054 (920031) | 558.8 | completed | - | ok |

## 診断のコードの集計

「解消」と「残った」は、そのコードが出た起動の次の check・run・test で、コードが消えたか残ったかの回数である。「最後に残った試行の数」は、判定のときに最後のスクリプトの check（テストの課題では test）でそのコードが出た試行の数である。警告（W）は diag::codes の全件を載せ、出なかったものも 0 とする。

### benitoiteh-strata-iq3s-effort-medium（qwen3.8-flash-next-iq3_s）

| コード | 出た回数 | 出た試行の数 | 解消 | 残った | 解消率 | 最後に残った試行の数 |
|---|---|---|---|---|---|---|
| W0301 | 0 | 0 | 0 | 0 | - | 0 |
| W0401 | 0 | 0 | 0 | 0 | - | 0 |
| W0402 | 0 | 0 | 0 | 0 | - | 0 |
| W0403 | 0 | 0 | 0 | 0 | - | 0 |
| W0501 | 0 | 0 | 0 | 0 | - | 0 |
| E0201 | 45 | 12 | 13 | 6 | 68% | 0 |
| E0203 | 22 | 4 | 4 | 2 | 67% | 0 |
| E0211 | 11 | 2 | 2 | 1 | 67% | 0 |
| E0213 | 1 | 1 | 1 | 0 | 100% | 0 |
| E0214 | 4 | 3 | 3 | 1 | 75% | 0 |
| E0217 | 1 | 1 | 1 | 0 | 100% | 0 |
| E0225 | 1 | 1 | 1 | 0 | 100% | 0 |
| E0230 | 1 | 1 | 1 | 0 | 100% | 0 |
| E0254 | 7 | 3 | 3 | 2 | 60% | 0 |
| E0301 | 10 | 5 | 5 | 1 | 83% | 0 |
| E0302 | 2 | 1 | 1 | 0 | 100% | 0 |
| E0303 | 5 | 2 | 2 | 0 | 100% | 0 |
| E0331 | 2 | 1 | 1 | 0 | 100% | 0 |
| E0332 | 2 | 2 | 2 | 0 | 100% | 0 |
| E0338 | 8 | 3 | 4 | 3 | 57% | 0 |
| E0401 | 29 | 8 | 9 | 7 | 56% | 0 |
| E0402 | 1 | 1 | 1 | 0 | 100% | 0 |
| E0405 | 1 | 1 | 1 | 0 | 100% | 0 |
| E0407 | 5 | 3 | 3 | 1 | 75% | 0 |
| E0415 | 1 | 1 | 1 | 0 | 100% | 0 |
| E0416 | 4 | 2 | 2 | 0 | 100% | 0 |
| E0422 | 5 | 4 | 4 | 1 | 80% | 0 |
| E0438 | 2 | 1 | 1 | 1 | 50% | 0 |
| E0441 | 9 | 3 | 4 | 2 | 67% | 0 |
| E0442 | 3 | 1 | 1 | 1 | 50% | 0 |
| E0449 | 8 | 1 | 1 | 1 | 50% | 0 |
| E0450 | 9 | 1 | 1 | 2 | 33% | 0 |
| E0451 | 4 | 3 | 3 | 0 | 100% | 0 |
| E0454 | 62 | 20 | 26 | 17 | 60% | 1 |
| E0456 | 1 | 1 | 1 | 0 | 100% | 0 |
| E0501 | 5 | 3 | 3 | 2 | 60% | 0 |
| E0601 | 1 | 1 | 1 | 0 | 100% | 0 |
| E0602 | 1 | 1 | 1 | 0 | 100% | 0 |
| E0604 | 2 | 1 | 1 | 0 | 100% | 0 |

### 試行ごとの最初の check のコードと最後の警告

| ハーネス | 課題 | 試行 | 最初の check | 最後の check の警告 |
|---|---|---|---|---|
| benitoiteh-strata-iq3s-effort-medium | csv_report | 1 | E0201, E0201, E0201, E0203, E0203, E0203, E0203, E0203, E0203, E0203 | - |
| benitoiteh-strata-iq3s-effort-medium | csv_report | 2 | E0201, E0217, E0201 | - |
| benitoiteh-strata-iq3s-effort-medium | csv_report | 3 | - | - |
| benitoiteh-strata-iq3s-effort-medium | dir_walk | 1 | E0454, E0501, E0454, E0454 | - |
| benitoiteh-strata-iq3s-effort-medium | dir_walk | 2 | E0211 | - |
| benitoiteh-strata-iq3s-effort-medium | dir_walk | 3 | E0401, E0454, E0407, E0454, E0407 | - |
| benitoiteh-strata-iq3s-effort-medium | error_handling | 1 | E0254, E0201, E0201, E0201, E0254, E0201, E0211, E0201, E0211, E0211, E0211, E0211 | - |
| benitoiteh-strata-iq3s-effort-medium | error_handling | 2 | E0454 | - |
| benitoiteh-strata-iq3s-effort-medium | error_handling | 3 | E0201, E0201 | - |
| benitoiteh-strata-iq3s-effort-medium | file_process | 1 | E0454, E0416, E0416, E0454, E0416, E0407 | - |
| benitoiteh-strata-iq3s-effort-medium | file_process | 2 | - | - |
| benitoiteh-strata-iq3s-effort-medium | file_process | 3 | E0416, E0454 | - |
| benitoiteh-strata-iq3s-effort-medium | http_client | 1 | E0201, E0201, E0201, E0201, E0201 | - |
| benitoiteh-strata-iq3s-effort-medium | http_client | 2 | - | - |
| benitoiteh-strata-iq3s-effort-medium | http_client | 3 | E0201 | - |
| benitoiteh-strata-iq3s-effort-medium | json_convert | 1 | E0454, E0454 | - |
| benitoiteh-strata-iq3s-effort-medium | json_convert | 2 | E0201, E0201, E0201, E0201, E0201, E0201, E0201, E0214 | - |
| benitoiteh-strata-iq3s-effort-medium | json_convert | 3 | E0331, E0331 | - |
| benitoiteh-strata-iq3s-effort-medium | json_summary | 1 | - | - |
| benitoiteh-strata-iq3s-effort-medium | json_summary | 2 | - | - |
| benitoiteh-strata-iq3s-effort-medium | json_summary | 3 | - | - |
| benitoiteh-strata-iq3s-effort-medium | process_run | 1 | - | - |
| benitoiteh-strata-iq3s-effort-medium | process_run | 2 | - | - |
| benitoiteh-strata-iq3s-effort-medium | process_run | 3 | E0454 | - |
| benitoiteh-strata-iq3s-effort-medium | process_shell | 1 | E0451 | - |
| benitoiteh-strata-iq3s-effort-medium | process_shell | 2 | E0442, E0442 | - |
| benitoiteh-strata-iq3s-effort-medium | process_shell | 3 | - | - |
| benitoiteh-strata-iq3s-effort-medium | regex_text | 1 | E0201 | - |
| benitoiteh-strata-iq3s-effort-medium | regex_text | 2 | E0201, E0201, E0214 | - |
| benitoiteh-strata-iq3s-effort-medium | regex_text | 3 | E0401 | - |
| benitoiteh-strata-iq3s-effort-medium | write_tests | 1 | E0441 | - |
| benitoiteh-strata-iq3s-effort-medium | write_tests | 2 | E0201 | - |
| benitoiteh-strata-iq3s-effort-medium | write_tests | 3 | E0214 | - |
