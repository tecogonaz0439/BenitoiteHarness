# Skill の評価の記録

- 日時: 2026-11-17 01:34
- 処理系: benitoite 0.0.12
- 処理系 SHA-256: 6b26491a8014d9ca88e92caa93c2fee8cc8201098331212071dfdcced8d47e7d
- Skill: not installed (normal suite)
- ハーネス: benitoiteh-strata-iq3s-normal（0.0.1）
- 提供者: {'kind': 'openai-chat', 'base_url': 'http://127.0.0.1:18081/v1', 'api_key': {'env': 'STRATA_API_KEY'}, 'context_window': 262144, 'capabilities': {'reasoning_effort': True}}
- モデル: qwen3.8-flash-next-iq3_s
- effort: low
- config: {"agent": {"completion_check": false}}
- max_turns: 50
- max_run_tokens: 2000000
- HARNESS_TIMEOUT: 900
- 除いた環境変数: OLDPWD, PWD
- CLAUDE_CONFIG_DIR: not set
- 試行の回数: 3
- 生の記録: <work-root>（リポジトリの外）
- completion_check_sha256: 65e20f220a2700e09b7ae6b7f8e2861c6b9d9fbf73bc05c80b420511317e216e
- benitoiteh_version: 0.0.1
- system_prompt_sha256: 6cedf30bb23c44cacc45aecc03454d710262aea31e50cacddc41ac2720b667e2
- tools_sha256: b6981cc2c2ad271ce663557906df90accf5ff8e563272e912b1e346e43f098af
- 条件の順: sequential

## 通常モード

| 種別 | 成功 / 試行 |
|---|---|
| bugfix | 12 / 12 |
| feature | 12 / 12 |
| tests | 6 / 6 |

| 課題 | 成果物 | 確認不足 | bash の実行数 | 終了状態 | 変更したファイル | 判定 |
|---|---|---|---|---|---|---|
| normal_cli_exit | True | False | 3 | [0, 0, 0] | tool.sh | verify=True, judge=True |
| normal_cli_exit | True | False | 4 | [0, 0, 0, 0] | tool.sh | verify=True, judge=True |
| normal_cli_exit | True | False | 3 | [0, 0, 0] | tool.sh | verify=True, judge=True |
| normal_config_merge | True | False | 4 | [0, 0, 1, 0] | config.py | verify=True, judge=True |
| normal_config_merge | True | False | 5 | [0, 0, 0, 1, 0] | config.py | verify=True, judge=True |
| normal_config_merge | True | False | 6 | [0, 0, 0, 1, 0, 0] | config.py | verify=True, judge=True |
| normal_csv_totals | True | False | 5 | [0, 0, 0, 0, 0] | totals.py | verify=True, judge=True |
| normal_csv_totals | True | False | 5 | [0, 0, 0, 0, 0] | totals.py | verify=True, judge=True |
| normal_csv_totals | True | False | 4 | [0, 0, 0, 0] | totals.py | verify=True, judge=True |
| normal_jsonl_filter | True | False | 6 | [0, 0, 0, 0, 0, 0] | filter.py | verify=True, judge=True |
| normal_jsonl_filter | True | False | 6 | [0, 0, 0, 1, 0, 0] | filter.py | verify=True, judge=True |
| normal_jsonl_filter | True | False | 5 | [0, 0, 0, 0, 0] | filter.py | verify=True, judge=True |
| normal_path_filter | True | False | 4 | [0, 0, 0, 0] | collect.sh | verify=True, judge=True |
| normal_path_filter | True | False | 7 | [0, 0, 0, 0, 0, 0, 0] | collect.sh | verify=True, judge=True |
| normal_path_filter | True | False | 7 | [0, 0, 0, 0, 0, 0, 0] | collect.sh | verify=True, judge=True |
| normal_report_sort | True | False | 5 | [0, 0, 0, 0, 0] | report.py | verify=True, judge=True |
| normal_report_sort | True | False | 3 | [0, 0, 0] | report.py | verify=True, judge=True |
| normal_report_sort | True | False | 2 | [0, 0] | report.py | verify=True, judge=True |
| normal_stable_unique | True | False | 5 | [0, 0, 0, 0, 0] | unique.py | verify=True, judge=True |
| normal_stable_unique | True | False | 6 | [0, 0, 0, 0, 0, 0] | unique.py | verify=True, judge=True |
| normal_stable_unique | True | False | 4 | [0, 0, 0, 0] | unique.py | verify=True, judge=True |
| normal_test_manifest | True | False | 3 | [0, 0, 0] | manifest.py | verify=True, judge=True |
| normal_test_manifest | True | False | 4 | [0, 0, 0, 0] | manifest.py | verify=True, judge=True |
| normal_test_manifest | True | False | 3 | [0, 0, 0] | manifest.py | verify=True, judge=True |
| normal_test_ranges | True | False | 4 | [0, 0, 0, 0] | ranges.py | verify=True, judge=True |
| normal_test_ranges | True | False | 4 | [0, 0, 0, 0] | ranges.py | verify=True, judge=True |
| normal_test_ranges | True | False | 2 | [0, 0] | ranges.py | verify=True, judge=True |
| normal_text_replace | True | False | 4 | [0, 0, 1, 0] | replace.py | verify=True, judge=True |
| normal_text_replace | True | False | 5 | [0, 0, 0, 1, 0] | replace.py | verify=True, judge=True |
| normal_text_replace | True | False | 5 | [0, 0, 0, 1, 0] | replace.py | verify=True, judge=True |

| 課題 | 成功 / 試行 | 成功した試行の検査と修正の回数 | 時間の中央値 (秒) | トークンの中央値 | 上限で停止 |
|---|---|---|---|---|---|
| normal_cli_exit | 3 / 3 | - | 28.5 | 17003 | 0 |
| normal_config_merge | 3 / 3 | - | 50.4 | 31143 | 0 |
| normal_csv_totals | 3 / 3 | - | 62.5 | 28944 | 0 |
| normal_jsonl_filter | 3 / 3 | - | 71.1 | 36551 | 0 |
| normal_path_filter | 3 / 3 | - | 107.9 | 44870 | 0 |
| normal_report_sort | 3 / 3 | - | 33.9 | 28638 | 0 |
| normal_stable_unique | 3 / 3 | - | 101.1 | 40891 | 0 |
| normal_test_manifest | 3 / 3 | - | 53.7 | 24902 | 0 |
| normal_test_ranges | 3 / 3 | - | 27.9 | 21447 | 0 |
| normal_text_replace | 3 / 3 | - | 54.3 | 34051 | 0 |

費用の見積もりの合計: - USD（費用を取得できた試行の合計）

## 試行ごとの結果

| 課題 | 試行 | 成功 | 番 | 検査と修正 (書いた後の検査) | トークン (キャッシュ読み取り) | 時間 (秒) | 終わり方 | 費用 (USD) | 理由 |
|---|---|---|---|---|---|---|---|---|---|
| normal_cli_exit | 1 | yes | 5 | - (-) | 16914 (12367) | 28.5 | completed | - | ok |
| normal_cli_exit | 2 | yes | 6 | - (-) | 20928 (18188) | 29.3 | completed | - | ok |
| normal_cli_exit | 3 | yes | 5 | - (-) | 17003 (14443) | 26.9 | completed | - | ok |
| normal_config_merge | 1 | yes | 7 | - (-) | 31143 (26217) | 50.4 | completed | - | ok |
| normal_config_merge | 2 | yes | 7 | - (-) | 30451 (25672) | 47.1 | completed | - | ok |
| normal_config_merge | 3 | yes | 9 | - (-) | 42125 (36426) | 58.9 | completed | - | ok |
| normal_csv_totals | 1 | yes | 9 | - (-) | 42862 (35601) | 81.6 | completed | - | ok |
| normal_csv_totals | 2 | yes | 7 | - (-) | 28944 (24021) | 62.5 | completed | - | ok |
| normal_csv_totals | 3 | yes | 7 | - (-) | 26366 (22298) | 54.2 | completed | - | ok |
| normal_jsonl_filter | 1 | yes | 10 | - (-) | 44487 (36886) | 100.3 | completed | - | ok |
| normal_jsonl_filter | 2 | yes | 8 | - (-) | 36551 (30163) | 71.1 | completed | - | ok |
| normal_jsonl_filter | 3 | yes | 8 | - (-) | 35075 (29693) | 65.9 | completed | - | ok |
| normal_path_filter | 1 | yes | 7 | - (-) | 26959 (22511) | 58.5 | completed | - | ok |
| normal_path_filter | 2 | yes | 10 | - (-) | 45514 (35784) | 161.2 | completed | - | ok |
| normal_path_filter | 3 | yes | 11 | - (-) | 44870 (38047) | 107.9 | completed | - | ok |
| normal_report_sort | 1 | yes | 8 | - (-) | 33714 (29096) | 45.0 | completed | - | ok |
| normal_report_sort | 2 | yes | 8 | - (-) | 28638 (25426) | 33.9 | completed | - | ok |
| normal_report_sort | 3 | yes | 6 | - (-) | 23108 (19888) | 28.8 | completed | - | ok |
| normal_stable_unique | 1 | yes | 7 | - (-) | 28113 (23278) | 62.5 | completed | - | ok |
| normal_stable_unique | 2 | yes | 9 | - (-) | 40891 (33282) | 115.8 | completed | - | ok |
| normal_stable_unique | 3 | yes | 10 | - (-) | 41875 (35129) | 101.1 | completed | - | ok |
| normal_test_manifest | 1 | yes | 6 | - (-) | 24902 (20363) | 53.7 | completed | - | ok |
| normal_test_manifest | 2 | yes | 6 | - (-) | 23933 (20073) | 39.5 | completed | - | ok |
| normal_test_manifest | 3 | yes | 7 | - (-) | 31055 (25781) | 62.7 | completed | - | ok |
| normal_test_ranges | 1 | yes | 6 | - (-) | 21447 (18600) | 27.9 | completed | - | ok |
| normal_test_ranges | 2 | yes | 8 | - (-) | 28790 (25628) | 36.0 | completed | - | ok |
| normal_test_ranges | 3 | yes | 5 | - (-) | 17267 (14661) | 27.2 | completed | - | ok |
| normal_text_replace | 1 | yes | 8 | - (-) | 34051 (28186) | 54.3 | completed | - | ok |
| normal_text_replace | 2 | yes | 7 | - (-) | 28263 (24182) | 42.7 | completed | - | ok |
| normal_text_replace | 3 | yes | 10 | - (-) | 40104 (34878) | 61.9 | completed | - | ok |

## 診断のコードの集計

「解消」と「残った」は、そのコードが出た起動の次の check・run・test で、コードが消えたか残ったかの回数である。「最後に残った試行の数」は、判定のときに最後のスクリプトの check（テストの課題では test）でそのコードが出た試行の数である。警告（W）は diag::codes の全件を載せ、出なかったものも 0 とする。

### benitoiteh-strata-iq3s-normal（qwen3.8-flash-next-iq3_s）

no diagnostic codes were found in benitoiteh tool results

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
| benitoiteh-strata-iq3s-normal | normal_cli_exit | 1 | - | - |
| benitoiteh-strata-iq3s-normal | normal_cli_exit | 2 | - | - |
| benitoiteh-strata-iq3s-normal | normal_cli_exit | 3 | - | - |
| benitoiteh-strata-iq3s-normal | normal_config_merge | 1 | - | - |
| benitoiteh-strata-iq3s-normal | normal_config_merge | 2 | - | - |
| benitoiteh-strata-iq3s-normal | normal_config_merge | 3 | - | - |
| benitoiteh-strata-iq3s-normal | normal_csv_totals | 1 | - | - |
| benitoiteh-strata-iq3s-normal | normal_csv_totals | 2 | - | - |
| benitoiteh-strata-iq3s-normal | normal_csv_totals | 3 | - | - |
| benitoiteh-strata-iq3s-normal | normal_jsonl_filter | 1 | - | - |
| benitoiteh-strata-iq3s-normal | normal_jsonl_filter | 2 | - | - |
| benitoiteh-strata-iq3s-normal | normal_jsonl_filter | 3 | - | - |
| benitoiteh-strata-iq3s-normal | normal_path_filter | 1 | - | - |
| benitoiteh-strata-iq3s-normal | normal_path_filter | 2 | - | - |
| benitoiteh-strata-iq3s-normal | normal_path_filter | 3 | - | - |
| benitoiteh-strata-iq3s-normal | normal_report_sort | 1 | - | - |
| benitoiteh-strata-iq3s-normal | normal_report_sort | 2 | - | - |
| benitoiteh-strata-iq3s-normal | normal_report_sort | 3 | - | - |
| benitoiteh-strata-iq3s-normal | normal_stable_unique | 1 | - | - |
| benitoiteh-strata-iq3s-normal | normal_stable_unique | 2 | - | - |
| benitoiteh-strata-iq3s-normal | normal_stable_unique | 3 | - | - |
| benitoiteh-strata-iq3s-normal | normal_test_manifest | 1 | - | - |
| benitoiteh-strata-iq3s-normal | normal_test_manifest | 2 | - | - |
| benitoiteh-strata-iq3s-normal | normal_test_manifest | 3 | - | - |
| benitoiteh-strata-iq3s-normal | normal_test_ranges | 1 | - | - |
| benitoiteh-strata-iq3s-normal | normal_test_ranges | 2 | - | - |
| benitoiteh-strata-iq3s-normal | normal_test_ranges | 3 | - | - |
| benitoiteh-strata-iq3s-normal | normal_text_replace | 1 | - | - |
| benitoiteh-strata-iq3s-normal | normal_text_replace | 2 | - | - |
| benitoiteh-strata-iq3s-normal | normal_text_replace | 3 | - | - |
