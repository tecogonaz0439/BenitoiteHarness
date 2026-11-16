# Skill の評価の記録

- 日時: 2026-11-17 04:50
- 処理系: benitoite 0.0.12
- 処理系 SHA-256: 6b26491a8014d9ca88e92caa93c2fee8cc8201098331212071dfdcced8d47e7d
- Skill: not installed (normal suite)
- ハーネス: benitoiteh-strata-iq3s-normal-effort-medium（0.0.1）
- 提供者: {'kind': 'openai-chat', 'base_url': 'http://127.0.0.1:18081/v1', 'api_key': {'env': 'STRATA_API_KEY'}, 'context_window': 262144, 'capabilities': {'reasoning_effort': True}}
- モデル: qwen3.8-flash-next-iq3_s
- effort: medium
- config: {"agent": {"completion_check": false}}
- max_turns: 50
- max_run_tokens: 2000000
- HARNESS_TIMEOUT: 1800
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
| bugfix | 11 / 12 |
| feature | 12 / 12 |
| tests | 6 / 6 |

| 課題 | 成果物 | 確認不足 | bash の実行数 | 終了状態 | 変更したファイル | 判定 |
|---|---|---|---|---|---|---|
| normal_cli_exit | True | False | 3 | [0, 0, 0] | tool.sh | verify=True, judge=True |
| normal_cli_exit | True | False | 4 | [0, 0, 0, 0] | tool.sh | verify=True, judge=True |
| normal_cli_exit | True | False | 4 | [0, 0, 0, 0] | tool.sh | verify=True, judge=True |
| normal_config_merge | True | False | 5 | [0, 0, 0, 0, 0] | config.py | verify=True, judge=True |
| normal_config_merge | True | False | 4 | [0, 0, 1, 0] | config.py | verify=True, judge=True |
| normal_config_merge | True | False | 4 | [0, 0, 1, 0] | config.py | verify=True, judge=True |
| normal_csv_totals | True | False | 8 | [0, 0, 0, 0, 0, 0, 0, 0] | totals.py | verify=True, judge=True |
| normal_csv_totals | True | False | 6 | [0, 0, 0, 0, 0, 0] | totals.py | verify=True, judge=True |
| normal_csv_totals | True | False | 3 | [0, 1, 0] | totals.py | verify=True, judge=True |
| normal_jsonl_filter | True | False | 5 | [0, 0, 0, 0, 0] | filter.py | verify=True, judge=True |
| normal_jsonl_filter | True | False | 6 | [0, 0, 0, 0, 0, 0] | filter.py | verify=True, judge=True |
| normal_jsonl_filter | True | False | 3 | [0, 0, 0] | filter.py | verify=True, judge=True |
| normal_path_filter | True | False | 5 | [0, 0, 0, 0, 0] | collect.sh | verify=True, judge=True |
| normal_path_filter | False | False | 3 | [0, 0, 0] | collect.sh | verify=False, judge=False |
| normal_path_filter | True | False | 5 | [0, 0, 0, 0, 0] | collect.sh | verify=True, judge=True |
| normal_report_sort | True | False | 3 | [0, 0, 0] | report.py | verify=True, judge=True |
| normal_report_sort | True | False | 6 | [0, 0, 0, 0, 0, 0] | report.py | verify=True, judge=True |
| normal_report_sort | True | False | 5 | [0, 0, 0, 0, 0] | report.py | verify=True, judge=True |
| normal_stable_unique | True | False | 4 | [0, 0, 0, 0] | unique.py | verify=True, judge=True |
| normal_stable_unique | True | False | 5 | [0, 0, 0, 0, 0] | unique.py | verify=True, judge=True |
| normal_stable_unique | True | False | 5 | [0, 0, 0, 0, 0] | unique.py | verify=True, judge=True |
| normal_test_manifest | True | False | 2 | [0, 0] | manifest.py | verify=True, judge=True |
| normal_test_manifest | True | False | 3 | [0, 0, 0] | manifest.py | verify=True, judge=True |
| normal_test_manifest | True | False | 3 | [0, 0, 0] | manifest.py | verify=True, judge=True |
| normal_test_ranges | True | False | 2 | [0, 0] | ranges.py | verify=True, judge=True |
| normal_test_ranges | True | False | 3 | [0, 0, 0] | ranges.py | verify=True, judge=True |
| normal_test_ranges | True | False | 3 | [0, 0, 0] | ranges.py | verify=True, judge=True |
| normal_text_replace | True | False | 4 | [0, 0, 1, 0] | replace.py | verify=True, judge=True |
| normal_text_replace | True | False | 5 | [0, 0, 0, 0, 0] | replace.py | verify=True, judge=True |
| normal_text_replace | True | False | 4 | [0, 0, 0, 0] | replace.py | verify=True, judge=True |

| 課題 | 成功 / 試行 | 成功した試行の検査と修正の回数 | 時間の中央値 (秒) | トークンの中央値 | 上限で停止 |
|---|---|---|---|---|---|
| normal_cli_exit | 3 / 3 | - | 33.5 | 20570 | 0 |
| normal_config_merge | 3 / 3 | - | 40.0 | 29503 | 0 |
| normal_csv_totals | 3 / 3 | - | 87.8 | 40640 | 0 |
| normal_jsonl_filter | 3 / 3 | - | 68.2 | 34367 | 0 |
| normal_path_filter | 2 / 3 | - | 75.4 | 28809 | 0 |
| normal_report_sort | 3 / 3 | - | 56.4 | 39350 | 0 |
| normal_stable_unique | 3 / 3 | - | 82.0 | 29202 | 0 |
| normal_test_manifest | 3 / 3 | - | 38.1 | 23502 | 0 |
| normal_test_ranges | 3 / 3 | - | 29.8 | 17511 | 0 |
| normal_text_replace | 3 / 3 | - | 55.7 | 28609 | 0 |

費用の見積もりの合計: - USD（費用を取得できた試行の合計）

## 試行ごとの結果

| 課題 | 試行 | 成功 | 番 | 検査と修正 (書いた後の検査) | トークン (キャッシュ読み取り) | 時間 (秒) | 終わり方 | 費用 (USD) | 理由 |
|---|---|---|---|---|---|---|---|---|---|
| normal_cli_exit | 1 | yes | 5 | - (-) | 17058 (12030) | 44.0 | completed | - | ok |
| normal_cli_exit | 2 | yes | 6 | - (-) | 21086 (18152) | 33.5 | completed | - | ok |
| normal_cli_exit | 3 | yes | 6 | - (-) | 20570 (17762) | 32.1 | completed | - | ok |
| normal_config_merge | 1 | yes | 7 | - (-) | 29503 (25250) | 40.0 | completed | - | ok |
| normal_config_merge | 2 | yes | 8 | - (-) | 36156 (31115) | 48.6 | completed | - | ok |
| normal_config_merge | 3 | yes | 6 | - (-) | 25102 (20697) | 39.7 | completed | - | ok |
| normal_csv_totals | 1 | yes | 12 | - (-) | 56338 (47310) | 135.8 | completed | - | ok |
| normal_csv_totals | 2 | yes | 9 | - (-) | 40640 (33651) | 87.8 | completed | - | ok |
| normal_csv_totals | 3 | yes | 6 | - (-) | 22193 (18321) | 45.9 | completed | - | ok |
| normal_jsonl_filter | 1 | yes | 7 | - (-) | 30446 (24374) | 74.4 | completed | - | ok |
| normal_jsonl_filter | 2 | yes | 8 | - (-) | 34367 (29061) | 65.0 | completed | - | ok |
| normal_jsonl_filter | 3 | yes | 8 | - (-) | 35233 (29649) | 68.2 | completed | - | ok |
| normal_path_filter | 1 | yes | 7 | - (-) | 27019 (22385) | 64.3 | completed | - | ok |
| normal_path_filter | 2 | no | 7 | - (-) | 25472 (20807) | 72.8 | completed | - | artifact checks failed |
| normal_path_filter | 3 | yes | 8 | - (-) | 30599 (25331) | 86.6 | completed | - | ok |
| normal_report_sort | 1 | yes | 5 | - (-) | 18582 (15418) | 28.7 | completed | - | ok |
| normal_report_sort | 2 | yes | 9 | - (-) | 39350 (33815) | 58.8 | completed | - | ok |
| normal_report_sort | 3 | yes | 9 | - (-) | 40524 (34990) | 56.4 | completed | - | ok |
| normal_stable_unique | 1 | yes | 8 | - (-) | 36261 (29196) | 102.3 | completed | - | ok |
| normal_stable_unique | 2 | yes | 7 | - (-) | 29202 (23545) | 82.0 | completed | - | ok |
| normal_stable_unique | 3 | yes | 7 | - (-) | 28181 (23416) | 62.4 | completed | - | ok |
| normal_test_manifest | 1 | yes | 7 | - (-) | 26110 (22779) | 28.6 | completed | - | ok |
| normal_test_manifest | 2 | yes | 5 | - (-) | 19407 (15614) | 43.2 | completed | - | ok |
| normal_test_manifest | 3 | yes | 6 | - (-) | 23502 (19478) | 38.1 | completed | - | ok |
| normal_test_ranges | 1 | yes | 5 | - (-) | 17511 (14754) | 30.2 | completed | - | ok |
| normal_test_ranges | 2 | yes | 5 | - (-) | 16691 (14356) | 23.4 | completed | - | ok |
| normal_test_ranges | 3 | yes | 7 | - (-) | 24274 (21478) | 29.8 | completed | - | ok |
| normal_text_replace | 1 | yes | 7 | - (-) | 28609 (23950) | 52.3 | completed | - | ok |
| normal_text_replace | 2 | yes | 8 | - (-) | 39574 (31036) | 121.4 | completed | - | ok |
| normal_text_replace | 3 | yes | 6 | - (-) | 24447 (19785) | 55.7 | completed | - | ok |

## 診断のコードの集計

「解消」と「残った」は、そのコードが出た起動の次の check・run・test で、コードが消えたか残ったかの回数である。「最後に残った試行の数」は、判定のときに最後のスクリプトの check（テストの課題では test）でそのコードが出た試行の数である。警告（W）は diag::codes の全件を載せ、出なかったものも 0 とする。

### benitoiteh-strata-iq3s-normal-effort-medium（qwen3.8-flash-next-iq3_s）

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
| benitoiteh-strata-iq3s-normal-effort-medium | normal_cli_exit | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_cli_exit | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_cli_exit | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_config_merge | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_config_merge | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_config_merge | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_csv_totals | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_csv_totals | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_csv_totals | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_jsonl_filter | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_jsonl_filter | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_jsonl_filter | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_path_filter | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_path_filter | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_path_filter | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_report_sort | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_report_sort | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_report_sort | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_stable_unique | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_stable_unique | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_stable_unique | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_test_manifest | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_test_manifest | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_test_manifest | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_test_ranges | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_test_ranges | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_test_ranges | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_text_replace | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_text_replace | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-medium | normal_text_replace | 3 | - | - |
