# Skill の評価の記録

- 日時: 2026-10-18 21:35
- 処理系: benitoite 0.0.7
- 処理系 SHA-256: ee8eaf0eb4e624be571b14198e28f1078fb6c6791c42b88ab77a015ea0e8f46e
- Skill: 0.0.7 (sha256 20b2c3a1533c)
- ハーネス: deepseek-harness（0.2.1-rc.2）
- 提供者: {}
- モデル: deepseek-flash
- effort: high
- max_turns: -
- max_run_tokens: -
- HARNESS_TIMEOUT: 1800
- 除いた環境変数: CLAUDECODE, CLAUDE_CODE_CHILD_SESSION, CLAUDE_CODE_ENTRYPOINT, CLAUDE_CODE_EXECPATH, CLAUDE_CODE_MESSAGING_SOCKET, CLAUDE_CODE_MESSAGING_TOKEN, CLAUDE_CODE_SESSION_ATTENDED, CLAUDE_CODE_SESSION_ID, CLAUDE_EFFORT, CLAUDE_PID
- CLAUDE_CONFIG_DIR: not set
- 試行の回数: 3
- 生の記録: <work-root>/deepseek-harness（リポジトリの外）
- 使用量の範囲: parent session only; usage excludes subagents

| 課題 | 成功 / 試行 | 成功した試行の検査と修正の回数 | 時間の中央値 (秒) | トークンの中央値 | 上限で停止 |
|---|---|---|---|---|---|
| csv_report | 3 / 3 | 5, 5, 4 | 39.9 | 438368 | 0 |
| dir_walk | 3 / 3 | 4, 3, 6 | 51.3 | 500874 | 0 |
| error_handling | 3 / 3 | 4, 4, 4 | 30.2 | 334727 | 0 |
| file_process | 3 / 3 | 3, 3, 3 | 22.9 | 230753 | 0 |
| http_client | 3 / 3 | 3, 3, 4 | 36.2 | 335929 | 0 |
| json_convert | 3 / 3 | 4, 3, 5 | 33.3 | 447399 | 0 |
| json_summary | 3 / 3 | 5, 4, 7 | 38.1 | 373995 | 0 |
| process_run | 3 / 3 | 3, 3, 3 | 27.7 | 244035 | 0 |
| process_shell | 3 / 3 | 3, 3, 3 | 29.0 | 230063 | 0 |
| regex_text | 3 / 3 | 3, 3, 4 | 30.1 | 305106 | 0 |
| write_tests | 3 / 3 | 3, 4, 3 | 36.0 | 366485 | 0 |

費用の見積もりの合計: 0.569097 USD（費用を取得できた試行の合計）

## 試行ごとの結果

| 課題 | 試行 | 成功 | 番 | 検査と修正 (書いた後の検査) | トークン (キャッシュ読み取り) | 時間 (秒) | 終わり方 | 費用 (USD) | 理由 |
|---|---|---|---|---|---|---|---|---|---|
| csv_report | 1 | yes | 15 | 5 | 477215 (436352) | 41.6 | completed | 0.020788 | ok |
| csv_report | 2 | yes | 13 | 5 | 420215 (380416) | 39.9 | completed | 0.020491 | ok |
| csv_report | 3 | yes | 14 | 4 | 438368 (400640) | 36.1 | completed | 0.018583 | ok |
| dir_walk | 1 | yes | 16 | 4 | 500874 (459648) | 50.3 | completed | 0.022435 | ok |
| dir_walk | 2 | yes | 14 | 3 | 490623 (442752) | 51.3 | completed | 0.024452 | ok |
| dir_walk | 3 | yes | 16 | 6 | 543384 (501504) | 54.2 | completed | 0.023476 | ok |
| error_handling | 1 | yes | 11 | 4 | 298083 (265984) | 30.2 | completed | 0.015080 | ok |
| error_handling | 2 | yes | 14 | 4 | 380281 (347392) | 31.8 | completed | 0.015302 | ok |
| error_handling | 3 | yes | 12 | 4 | 334727 (300928) | 29.6 | completed | 0.015499 | ok |
| file_process | 1 | yes | 9 | 3 | 230753 (197760) | 22.7 | completed | 0.013812 | ok |
| file_process | 2 | yes | 12 | 3 | 324817 (294016) | 25.6 | completed | 0.013568 | ok |
| file_process | 3 | yes | 9 | 3 | 221031 (189824) | 22.9 | completed | 0.013379 | ok |
| http_client | 1 | yes | 13 | 3 | 470459 (422528) | 51.7 | completed | 0.025725 | ok |
| http_client | 2 | yes | 10 | 3 | 279621 (244608) | 31.9 | completed | 0.016677 | ok |
| http_client | 3 | yes | 11 | 4 | 335929 (295936) | 36.2 | completed | 0.019626 | ok |
| json_convert | 1 | yes | 14 | 4 | 447399 (409216) | 33.3 | completed | 0.018690 | ok |
| json_convert | 2 | yes | 9 | 3 | 269660 (231424) | 34.3 | completed | 0.019103 | ok |
| json_convert | 3 | yes | 14 | 5 | 449390 (411392) | 30.8 | completed | 0.018369 | ok |
| json_summary | 1 | yes | 14 | 5 | 437051 (399488) | 38.1 | completed | 0.019066 | ok |
| json_summary | 2 | yes | 10 | 4 | 273342 (240768) | 28.3 | completed | 0.015627 | ok |
| json_summary | 3 | yes | 12 | 7 | 373995 (336512) | 42.0 | completed | 0.019853 | ok |
| process_run | 1 | yes | 9 | 3 | 238136 (206592) | 20.0 | completed | 0.012928 | ok |
| process_run | 2 | yes | 13 | 3 | 244035 (220416) | 30.3 | completed | 0.012265 | ok |
| process_run | 3 | yes | 12 | 3 | 325063 (292608) | 27.7 | completed | 0.014584 | ok |
| process_shell | 1 | yes | 11 | 3 | 212945 (189440) | 29.0 | completed | 0.011956 | ok |
| process_shell | 2 | yes | 9 | 3 | 230063 (199808) | 21.3 | completed | 0.012749 | ok |
| process_shell | 3 | yes | 11 | 3 | 280078 (248448) | 29.3 | completed | 0.014757 | ok |
| regex_text | 1 | yes | 11 | 3 | 305106 (271744) | 30.1 | completed | 0.015488 | ok |
| regex_text | 2 | yes | 10 | 3 | 268360 (236288) | 25.2 | completed | 0.014339 | ok |
| regex_text | 3 | yes | 13 | 4 | 354763 (321536) | 30.3 | completed | 0.015348 | ok |
| write_tests | 1 | yes | 12 | 3 | 366485 (327040) | 36.0 | completed | 0.019113 | ok |
| write_tests | 2 | yes | 11 | 4 | 326908 (292224) | 33.8 | completed | 0.016896 | ok |
| write_tests | 3 | yes | 14 | 3 | 432196 (392704) | 37.4 | completed | 0.019074 | ok |

## 診断のコードの集計

「解消」と「残った」は、そのコードが出た起動の次の check・run・test で、コードが消えたか残ったかの回数である。「最後に残った試行の数」は、判定のときに最後のスクリプトの check（テストの課題では test）でそのコードが出た試行の数である。警告（W）は diag::codes の全件を載せ、出なかったものも 0 とする。

### deepseek-harness（deepseek-flash）

| コード | 出た回数 | 出た試行の数 | 解消 | 残った | 解消率 | 最後に残った試行の数 |
|---|---|---|---|---|---|---|
| W0301 | 0 | 0 | 0 | 0 | - | 0 |
| W0401 | 0 | 0 | 0 | 0 | - | 0 |
| W0402 | 0 | 0 | 0 | 0 | - | 0 |
| W0403 | 0 | 0 | 0 | 0 | - | 0 |
| W0501 | 0 | 0 | 0 | 0 | - | 0 |

### 試行ごとの最初の check のコードと最後の警告

| ハーネス | 課題 | 試行 | 最初の check | 最後の check の警告 |
|---|---|---|---|---|
| deepseek-harness | csv_report | 1 | - | - |
| deepseek-harness | csv_report | 2 | - | - |
| deepseek-harness | csv_report | 3 | - | - |
| deepseek-harness | dir_walk | 1 | - | - |
| deepseek-harness | dir_walk | 2 | - | - |
| deepseek-harness | dir_walk | 3 | - | - |
| deepseek-harness | error_handling | 1 | - | - |
| deepseek-harness | error_handling | 2 | - | - |
| deepseek-harness | error_handling | 3 | - | - |
| deepseek-harness | file_process | 1 | - | - |
| deepseek-harness | file_process | 2 | - | - |
| deepseek-harness | file_process | 3 | - | - |
| deepseek-harness | http_client | 1 | - | - |
| deepseek-harness | http_client | 2 | - | - |
| deepseek-harness | http_client | 3 | - | - |
| deepseek-harness | json_convert | 1 | - | - |
| deepseek-harness | json_convert | 2 | - | - |
| deepseek-harness | json_convert | 3 | - | - |
| deepseek-harness | json_summary | 1 | - | - |
| deepseek-harness | json_summary | 2 | - | - |
| deepseek-harness | json_summary | 3 | - | - |
| deepseek-harness | process_run | 1 | - | - |
| deepseek-harness | process_run | 2 | - | - |
| deepseek-harness | process_run | 3 | - | - |
| deepseek-harness | process_shell | 1 | - | - |
| deepseek-harness | process_shell | 2 | - | - |
| deepseek-harness | process_shell | 3 | - | - |
| deepseek-harness | regex_text | 1 | - | - |
| deepseek-harness | regex_text | 2 | - | - |
| deepseek-harness | regex_text | 3 | - | - |
| deepseek-harness | write_tests | 1 | - | - |
| deepseek-harness | write_tests | 2 | - | - |
| deepseek-harness | write_tests | 3 | - | - |
