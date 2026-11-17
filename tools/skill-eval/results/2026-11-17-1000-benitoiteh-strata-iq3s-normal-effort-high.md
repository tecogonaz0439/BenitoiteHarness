# Skill の評価の記録

- 日時: 2026-11-17 10:00
- 処理系: benitoite 0.0.12
- 処理系 SHA-256: 6b26491a8014d9ca88e92caa93c2fee8cc8201098331212071dfdcced8d47e7d
- Skill: not installed (normal suite)
- ハーネス: benitoiteh-strata-iq3s-normal-effort-high（0.0.1）
- 提供者: {'kind': 'openai-chat', 'base_url': 'http://127.0.0.1:18081/v1', 'api_key': {'env': 'STRATA_API_KEY'}, 'context_window': 262144, 'capabilities': {'reasoning_effort': True}}
- モデル: qwen3.8-flash-next-iq3_s
- effort: high
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
| bugfix | 12 / 12 |
| feature | 12 / 12 |
| tests | 6 / 6 |

| 課題 | 成果物 | 確認不足 | bash の実行数 | 終了状態 | 変更したファイル | 判定 |
|---|---|---|---|---|---|---|
| normal_cli_exit | True | False | 4 | [0, 0, 0, 0] | tool.sh | verify=True, judge=True |
| normal_cli_exit | True | False | 4 | [0, 0, 0, 0] | tool.sh | verify=True, judge=True |
| normal_cli_exit | True | False | 4 | [0, 0, 0, 0] | tool.sh | verify=True, judge=True |
| normal_config_merge | True | False | 6 | [0, 0, 0, 0, 0, 0] | config.py | verify=True, judge=True |
| normal_config_merge | True | False | 5 | [0, 0, 0, 0, 0] | config.py | verify=True, judge=True |
| normal_config_merge | True | False | 3 | [0, 0, 0] | config.py | verify=True, judge=True |
| normal_csv_totals | True | False | 8 | [0, 0, 0, 0, 0, 0, 0, 0] | totals.py | verify=True, judge=True |
| normal_csv_totals | True | False | 6 | [0, 0, 0, 0, 0, 0] | totals.py | verify=True, judge=True |
| normal_csv_totals | True | False | 5 | [0, 0, 0, 0, 0] | totals.py | verify=True, judge=True |
| normal_jsonl_filter | True | False | 4 | [0, 0, 0, 0] | filter.py | verify=True, judge=True |
| normal_jsonl_filter | True | False | 6 | [0, 0, 0, 1, 0, 0] | filter.py | verify=True, judge=True |
| normal_jsonl_filter | True | False | 6 | [0, 0, 0, 0, 0, 0] | filter.py | verify=True, judge=True |
| normal_path_filter | True | False | 7 | [0, 0, 0, 0, 0, 0, 0] | collect.sh | verify=True, judge=True |
| normal_path_filter | True | False | 8 | [0, 0, 0, 0, 0, 0, 0, 0] | collect.sh | verify=True, judge=True |
| normal_path_filter | True | False | 14 | [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1] | collect.sh | verify=True, judge=True |
| normal_report_sort | True | False | 7 | [0, 0, 0, 0, 0, 0, 0] | report.py | verify=True, judge=True |
| normal_report_sort | True | False | 5 | [0, 0, 0, 0, 0] | report.py | verify=True, judge=True |
| normal_report_sort | True | False | 6 | [0, 0, 0, 0, 0, 0] | report.py | verify=True, judge=True |
| normal_stable_unique | True | False | 11 | [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0] | unique.py | verify=True, judge=True |
| normal_stable_unique | True | False | 10 | [0, 0, 0, 0, 0, 1, 0, 0, 0, 0] | unique.py | verify=True, judge=True |
| normal_stable_unique | True | False | 9 | [0, 0, 0, 0, 0, 0, 0, 1, 0] | unique.py | verify=True, judge=True |
| normal_test_manifest | True | False | 5 | [0, 0, 0, 1, 0] | manifest.py | verify=True, judge=True |
| normal_test_manifest | True | False | 7 | [0, 0, 0, 1, 2, 1, 0] | manifest.py | verify=True, judge=True |
| normal_test_manifest | True | False | 6 | [0, 0, 0, 1, 0, 0] | manifest.py | verify=True, judge=True |
| normal_test_ranges | True | False | 4 | [0, 0, 0, 0] | ranges.py | verify=True, judge=True |
| normal_test_ranges | True | False | 5 | [0, 0, 0, 0, 0] | ranges.py | verify=True, judge=True |
| normal_test_ranges | True | False | 4 | [0, 0, 0, 0] | ranges.py | verify=True, judge=True |
| normal_text_replace | True | False | 7 | [0, 0, 0, 0, 1, 0, 0] | replace.py | verify=True, judge=True |
| normal_text_replace | True | False | 5 | [0, 0, 0, 0, 0] | replace.py | verify=True, judge=True |
| normal_text_replace | True | False | 5 | [0, 0, 0, 0, 0] | replace.py | verify=True, judge=True |

| 課題 | 成功 / 試行 | 成功した試行の検査と修正の回数 | 時間の中央値 (秒) | トークンの中央値 | 上限で停止 |
|---|---|---|---|---|---|
| normal_cli_exit | 3 / 3 | - | 35.3 | 23662 | 0 |
| normal_config_merge | 3 / 3 | - | 57.4 | 42121 | 0 |
| normal_csv_totals | 3 / 3 | - | 81.9 | 35179 | 0 |
| normal_jsonl_filter | 3 / 3 | - | 86.1 | 38312 | 0 |
| normal_path_filter | 3 / 3 | - | 127.6 | 50997 | 0 |
| normal_report_sort | 3 / 3 | - | 61.2 | 41491 | 0 |
| normal_stable_unique | 3 / 3 | - | 213.1 | 78134 | 0 |
| normal_test_manifest | 3 / 3 | - | 88.6 | 44815 | 0 |
| normal_test_ranges | 3 / 3 | - | 43.4 | 26637 | 0 |
| normal_text_replace | 3 / 3 | - | 66.5 | 30060 | 0 |

費用の見積もりの合計: - USD（費用を取得できた試行の合計）

## 試行ごとの結果

| 課題 | 試行 | 成功 | 番 | 検査と修正 (書いた後の検査) | トークン (キャッシュ読み取り) | 時間 (秒) | 終わり方 | 費用 (USD) | 理由 |
|---|---|---|---|---|---|---|---|---|---|
| normal_cli_exit | 1 | yes | 7 | - (-) | 23662 (18841) | 33.8 | completed | - | ok |
| normal_cli_exit | 2 | yes | 6 | - (-) | 21196 (18072) | 38.3 | completed | - | ok |
| normal_cli_exit | 3 | yes | 7 | - (-) | 23928 (20882) | 35.3 | completed | - | ok |
| normal_config_merge | 1 | yes | 10 | - (-) | 42121 (36577) | 57.4 | completed | - | ok |
| normal_config_merge | 2 | yes | 10 | - (-) | 43393 (37437) | 62.4 | completed | - | ok |
| normal_config_merge | 3 | yes | 6 | - (-) | 27055 (21784) | 50.8 | completed | - | ok |
| normal_csv_totals | 1 | yes | 12 | - (-) | 62264 (52529) | 138.0 | completed | - | ok |
| normal_csv_totals | 2 | yes | 8 | - (-) | 35179 (28890) | 81.9 | completed | - | ok |
| normal_csv_totals | 3 | yes | 7 | - (-) | 31603 (25534) | 68.9 | completed | - | ok |
| normal_jsonl_filter | 1 | yes | 6 | - (-) | 25035 (20019) | 63.4 | completed | - | ok |
| normal_jsonl_filter | 2 | yes | 8 | - (-) | 38312 (31139) | 86.1 | completed | - | ok |
| normal_jsonl_filter | 3 | yes | 8 | - (-) | 39512 (31902) | 87.9 | completed | - | ok |
| normal_path_filter | 1 | yes | 9 | - (-) | 38856 (31479) | 111.3 | completed | - | ok |
| normal_path_filter | 2 | yes | 11 | - (-) | 50997 (42544) | 127.6 | completed | - | ok |
| normal_path_filter | 3 | yes | 19 | - (-) | 120821 (98009) | 383.2 | completed | - | ok |
| normal_report_sort | 1 | yes | 12 | - (-) | 54956 (48648) | 67.1 | completed | - | ok |
| normal_report_sort | 2 | yes | 7 | - (-) | 30400 (25188) | 55.3 | completed | - | ok |
| normal_report_sort | 3 | yes | 9 | - (-) | 41491 (35476) | 61.2 | completed | - | ok |
| normal_stable_unique | 1 | yes | 14 | - (-) | 78134 (63527) | 238.7 | completed | - | ok |
| normal_stable_unique | 2 | yes | 15 | - (-) | 100447 (84813) | 213.1 | completed | - | ok |
| normal_stable_unique | 3 | yes | 11 | - (-) | 56753 (46774) | 135.4 | completed | - | ok |
| normal_test_manifest | 1 | yes | 8 | - (-) | 36208 (30590) | 61.2 | completed | - | ok |
| normal_test_manifest | 2 | yes | 9 | - (-) | 45405 (36768) | 96.0 | completed | - | ok |
| normal_test_manifest | 3 | yes | 9 | - (-) | 44815 (37315) | 88.6 | completed | - | ok |
| normal_test_ranges | 1 | yes | 7 | - (-) | 26637 (22655) | 43.8 | completed | - | ok |
| normal_test_ranges | 2 | yes | 8 | - (-) | 30665 (27044) | 42.1 | completed | - | ok |
| normal_test_ranges | 3 | yes | 7 | - (-) | 26443 (22696) | 43.4 | completed | - | ok |
| normal_text_replace | 1 | yes | 10 | - (-) | 50841 (43450) | 73.0 | completed | - | ok |
| normal_text_replace | 2 | yes | 7 | - (-) | 29405 (24572) | 55.6 | completed | - | ok |
| normal_text_replace | 3 | yes | 7 | - (-) | 30060 (24653) | 66.5 | completed | - | ok |

## 診断のコードの集計

「解消」と「残った」は、そのコードが出た起動の次の check・run・test で、コードが消えたか残ったかの回数である。「最後に残った試行の数」は、判定のときに最後のスクリプトの check（テストの課題では test）でそのコードが出た試行の数である。警告（W）は diag::codes の全件を載せ、出なかったものも 0 とする。

### benitoiteh-strata-iq3s-normal-effort-high（qwen3.8-flash-next-iq3_s）

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
| benitoiteh-strata-iq3s-normal-effort-high | normal_cli_exit | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_cli_exit | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_cli_exit | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_config_merge | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_config_merge | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_config_merge | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_csv_totals | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_csv_totals | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_csv_totals | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_jsonl_filter | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_jsonl_filter | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_jsonl_filter | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_path_filter | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_path_filter | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_path_filter | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_report_sort | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_report_sort | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_report_sort | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_stable_unique | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_stable_unique | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_stable_unique | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_test_manifest | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_test_manifest | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_test_manifest | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_test_ranges | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_test_ranges | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_test_ranges | 3 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_text_replace | 1 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_text_replace | 2 | - | - |
| benitoiteh-strata-iq3s-normal-effort-high | normal_text_replace | 3 | - | - |
