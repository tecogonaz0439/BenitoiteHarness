# Skill の評価の記録

- 日時: 2026-10-19 10:40
- 処理系: benitoite 0.0.7
- 処理系 SHA-256: ee8eaf0eb4e624be571b14198e28f1078fb6c6791c42b88ab77a015ea0e8f46e
- Skill: 0.0.7 (sha256 20b2c3a1533c)
- ハーネス: antigravity-gemini（1.4.2）
- 提供者: {}
- モデル: gemini-3.8-flash-high
- effort: high
- config: -
- max_turns: -
- max_run_tokens: -
- HARNESS_TIMEOUT: 1860
- 除いた環境変数: CLAUDECODE, CLAUDE_CODE_CHILD_SESSION, CLAUDE_CODE_ENTRYPOINT, CLAUDE_CODE_EXECPATH, CLAUDE_CODE_MESSAGING_SOCKET, CLAUDE_CODE_MESSAGING_TOKEN, CLAUDE_CODE_SESSION_ATTENDED, CLAUDE_CODE_SESSION_ID, CLAUDE_EFFORT, CLAUDE_PID
- CLAUDE_CONFIG_DIR: not set
- 試行の回数: 3
- 生の記録: <work-root>（リポジトリの外）
- Antigravity の利用者設定の確認: passed
- Antigravity の同梱の Skill: ['agy-customizations', 'antigravity_guide', 'automation', 'generative_ui', 'migrate-workflows', 'permissioned-github', 'plugin', 'run-workflow', 'ui-plugin-navigation']

| 課題 | 成功 / 試行 | 成功した試行の検査と修正の回数 | 時間の中央値 (秒) | トークンの中央値 | 上限で停止 |
|---|---|---|---|---|---|
| csv_report | 3 / 3 | 2, 2, 5 | 299.1 | 1.45121e+06 | 0 |
| dir_walk | 3 / 3 | 6, 4, 2 | 346.3 | 2.406e+06 | 0 |
| error_handling | 3 / 3 | 2, 2, 2 | 244.4 | 1.75482e+06 | 0 |
| file_process | 3 / 3 | 2, 2, 2 | 200.2 | 1.20487e+06 | 0 |
| http_client | 3 / 3 | 2, 2, 2 | 251.9 | 1.4166e+06 | 0 |
| json_convert | 3 / 3 | 3, 2, 2 | 272.1 | 1.30695e+06 | 0 |
| json_summary | 3 / 3 | 2, 8, 2 | 357.0 | 1.44551e+06 | 0 |
| process_run | 3 / 3 | 2, 2, 2 | 197.5 | 1.11208e+06 | 0 |
| process_shell | 3 / 3 | 2, 2, 2 | 304.2 | 1.01282e+06 | 0 |
| regex_text | 3 / 3 | 2, 2, 2 | 333.8 | 1.43032e+06 | 0 |
| write_tests | 3 / 3 | 1, 1, 1 | 287.0 | 1.33334e+06 | 0 |

費用の見積もりの合計: - USD（費用を取得できた試行の合計）

## 試行ごとの結果

| 課題 | 試行 | 成功 | 番 | 検査と修正 (書いた後の検査) | トークン (キャッシュ読み取り) | 時間 (秒) | 終わり方 | 費用 (USD) | 理由 |
|---|---|---|---|---|---|---|---|---|---|
| csv_report | 1 | yes | 24 | 2 | 1451213 (1268773) | 325.1 | completed | - | ok |
| csv_report | 2 | yes | 25 | 2 | 1368467 (1179836) | 281.9 | completed | - | ok |
| csv_report | 3 | yes | 31 | 5 | 2019248 (1823002) | 299.1 | completed | - | ok; over the 2M token cap |
| dir_walk | 1 | yes | 34 | 6 | 2405999 (2197046) | 336.9 | completed | - | ok; over the 2M token cap |
| dir_walk | 2 | yes | 30 | 4 | 2376834 (2165789) | 346.3 | completed | - | ok; over the 2M token cap |
| dir_walk | 3 | yes | 40 | 2 | 2612521 (2393214) | 375.2 | completed | - | ok; over the 2M token cap |
| error_handling | 1 | yes | 31 | 2 | 1754824 (1563884) | 259.2 | completed | - | ok |
| error_handling | 2 | yes | 25 | 2 | 1580652 (1404931) | 235.3 | completed | - | ok |
| error_handling | 3 | yes | 27 | 2 | 2061975 (1863038) | 244.4 | completed | - | ok; over the 2M token cap |
| file_process | 1 | yes | 20 | 2 | 1070307 (904083) | 200.2 | completed | - | ok |
| file_process | 2 | yes | 23 | 2 | 1204873 (1001613) | 198.3 | completed | - | ok |
| file_process | 3 | yes | 25 | 2 | 1639504 (1445377) | 218.7 | completed | - | ok |
| http_client | 1 | yes | 25 | 2 | 1416598 (1223607) | 251.9 | completed | - | ok |
| http_client | 2 | yes | 26 | 2 | 1632908 (1362506) | 337.5 | completed | - | ok |
| http_client | 3 | yes | 24 | 2 | 1386686 (1207424) | 248.9 | completed | - | ok |
| json_convert | 1 | yes | 25 | 3 | 1525804 (1348920) | 270.9 | completed | - | ok |
| json_convert | 2 | yes | 20 | 2 | 1087667 (923489) | 272.1 | completed | - | ok |
| json_convert | 3 | yes | 23 | 2 | 1306954 (1131984) | 319.5 | completed | - | ok |
| json_summary | 1 | yes | 22 | 2 | 1445510 (1261524) | 357.0 | completed | - | ok |
| json_summary | 2 | yes | 37 | 8 | 2347410 (2130173) | 388.5 | completed | - | ok; over the 2M token cap |
| json_summary | 3 | yes | 22 | 2 | 1137886 (947751) | 335.1 | completed | - | ok |
| process_run | 1 | yes | 21 | 2 | 1112084 (939164) | 223.1 | completed | - | ok |
| process_run | 2 | yes | 20 | 2 | 1025760 (862999) | 171.0 | completed | - | ok |
| process_run | 3 | yes | 26 | 2 | 1157939 (1012606) | 197.5 | completed | - | ok |
| process_shell | 1 | yes | 18 | 2 | 1007782 (815244) | 304.2 | completed | - | ok |
| process_shell | 2 | yes | 24 | 2 | 1454053 (1251120) | 317.1 | completed | - | ok |
| process_shell | 3 | yes | 18 | 2 | 1012819 (797687) | 256.6 | completed | - | ok |
| regex_text | 1 | yes | 23 | 2 | 1430317 (1199874) | 333.8 | completed | - | ok |
| regex_text | 2 | yes | 24 | 2 | 1846548 (1646138) | 387.6 | completed | - | ok |
| regex_text | 3 | yes | 24 | 2 | 1392189 (1036189) | 305.1 | completed | - | ok |
| write_tests | 1 | yes | 24 | 1 | 1333345 (1135008) | 287.0 | completed | - | ok |
| write_tests | 2 | yes | 28 | 1 | 1850014 (1630491) | 411.4 | completed | - | ok |
| write_tests | 3 | yes | 22 | 1 | 1180277 (989401) | 263.2 | completed | - | ok |

## 診断のコードの集計

「解消」と「残った」は、そのコードが出た起動の次の check・run・test で、コードが消えたか残ったかの回数である。「最後に残った試行の数」は、判定のときに最後のスクリプトの check（テストの課題では test）でそのコードが出た試行の数である。警告（W）は diag::codes の全件を載せ、出なかったものも 0 とする。

### antigravity-gemini（gemini-3.8-flash-high）

| コード | 出た回数 | 出た試行の数 | 解消 | 残った | 解消率 | 最後に残った試行の数 |
|---|---|---|---|---|---|---|
| W0301 | 0 | 0 | 0 | 0 | - | 0 |
| W0401 | 0 | 0 | 0 | 0 | - | 0 |
| W0402 | 0 | 0 | 0 | 0 | - | 0 |
| W0403 | 0 | 0 | 0 | 0 | - | 0 |
| W0501 | 0 | 0 | 0 | 0 | - | 0 |
| E0201 | 2 | 1 | 1 | 0 | 100% | 0 |
| E0338 | 1 | 1 | 1 | 0 | 100% | 0 |

### 試行ごとの最初の check のコードと最後の警告

| ハーネス | 課題 | 試行 | 最初の check | 最後の check の警告 |
|---|---|---|---|---|
| antigravity-gemini | csv_report | 1 | - | - |
| antigravity-gemini | csv_report | 2 | - | - |
| antigravity-gemini | csv_report | 3 | - | - |
| antigravity-gemini | dir_walk | 1 | - | - |
| antigravity-gemini | dir_walk | 2 | - | - |
| antigravity-gemini | dir_walk | 3 | - | - |
| antigravity-gemini | error_handling | 1 | - | - |
| antigravity-gemini | error_handling | 2 | - | - |
| antigravity-gemini | error_handling | 3 | - | - |
| antigravity-gemini | file_process | 1 | - | - |
| antigravity-gemini | file_process | 2 | - | - |
| antigravity-gemini | file_process | 3 | - | - |
| antigravity-gemini | http_client | 1 | - | - |
| antigravity-gemini | http_client | 2 | - | - |
| antigravity-gemini | http_client | 3 | - | - |
| antigravity-gemini | json_convert | 1 | E0338 | - |
| antigravity-gemini | json_convert | 2 | - | - |
| antigravity-gemini | json_convert | 3 | - | - |
| antigravity-gemini | json_summary | 1 | - | - |
| antigravity-gemini | json_summary | 2 | - | - |
| antigravity-gemini | json_summary | 3 | - | - |
| antigravity-gemini | process_run | 1 | - | - |
| antigravity-gemini | process_run | 2 | - | - |
| antigravity-gemini | process_run | 3 | - | - |
| antigravity-gemini | process_shell | 1 | - | - |
| antigravity-gemini | process_shell | 2 | - | - |
| antigravity-gemini | process_shell | 3 | - | - |
| antigravity-gemini | regex_text | 1 | - | - |
| antigravity-gemini | regex_text | 2 | - | - |
| antigravity-gemini | regex_text | 3 | - | - |
| antigravity-gemini | write_tests | 1 | - | - |
| antigravity-gemini | write_tests | 2 | - | - |
| antigravity-gemini | write_tests | 3 | - | - |
