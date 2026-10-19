#!/usr/bin/env python3
"""測定結果の読み取りと報告の契約を確かめる（実装プラン P5-05）。"""

import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import measure


# 作成時の関門: 集計の入口で、欠測を達成と誤認する退行と単位の混同を防ぐ。
# 製品の性能テストと重ならず、製品コードに差し込み口を加えない。
class ReportTests(unittest.TestCase):
    def test_median_maximum_and_goals(self):
        for samples, target, expected in [
                ([1, 4, 2], 3, (3, 2, 4, '達成（最大値は目標超過）')),
                ([4, 2], 3, (2, 3, 4, '達成（最大値は目標超過）')),
                ([5], 3, (1, 5, 5, '未達（最大値は目標超過）')),
                ([30], None, (1, 30, 30, '参考（判定なし）')),
                ([20480], 20480, (1, 20480, 20480, '達成'))]:
            with self.subTest(samples=samples, target=target):
                self.assertEqual(measure.summary(samples, target), expected)

    def test_libtest_prefix_invalid_records_and_missing_items(self):
        valid = {'item': 'version_ms', 'unit': 'ms', 'samples': [1.5, 2.5], 'note': 'isolated'}
        records, errors = measure.parse_perf('test version ... PERF ' + json.dumps(valid) + '\nok\n')
        self.assertEqual(records, {'version_ms': valid})
        self.assertEqual(errors, [])
        self.assertNotIn('version_ms', measure.missing_items(records))
        self.assertIn('stream_delta_ms', measure.missing_items(records))
        for bad in ['{', '[]', '{"item":"unknown"}',
                    json.dumps(dict(valid, unit='KiB')), json.dumps(dict(valid, samples=[])),
                    json.dumps(dict(valid, samples=[True])), json.dumps(dict(valid, samples=[-1])),
                    json.dumps(dict(valid, samples=[float('nan')])),
                    json.dumps(dict(valid, samples=[float('inf')])),
                    json.dumps(dict(valid, item='tui_idle_rss_kib', unit='KiB', samples=[1.5])),
                    json.dumps(dict(valid, note=None))]:
            with self.subTest(bad=bad):
                records, errors = measure.parse_perf('PERF ' + bad)
                self.assertEqual(records, {})
                self.assertEqual(len(errors), 1)
        records, errors = measure.parse_perf(('PERF ' + json.dumps(valid) + '\n') * 2)
        self.assertEqual(records, {'version_ms': valid})
        self.assertEqual(len(errors), 1)

    def test_comparison_table_and_notes(self):
        configs = [
            {'name': 'default', 'records': {
                'version_ms': {'samples': [1, 3, 9], 'note': 'a|b\nc'},
                'tui_idle_rss_kib': {'samples': [20481], 'note': 'rss'},
                'exec_first_request_restricted_ms': {'samples': [999], 'note': 'reference'}}, 'errors': []},
            {'name': 'opt-s', 'records': {}, 'errors': ['cargo test exited 101']}]
        output = measure.report({'commit': 'abc'}, configs)
        self.assertIn('| version_ms | ms | 5 | 3 | 3.000 | 9.000 | 達成（最大値は目標超過） | 0 | 測れなかった', output)
        self.assertIn('| tui_idle_rss_kib | KiB | 20480 | 1 | 20481.000 | 20481.000 | 未達', output)
        self.assertIn('参考（判定なし）', output)
        self.assertIn('opt-s 中央値', output)
        self.assertIn('a\\|b<br>c', output)
        self.assertIn('cargo test exited 101', output)

    def test_opt_s_commands_build_and_test_the_same_profile(self):
        for action in ('build', 'test'):
            with self.subTest(action=action):
                args = measure.cargo_command(action, True)
                self.assertEqual(args[:8], ['cargo', action, '--offline', '--locked', '--release', '-p', 'benitoiteh', '--config'])
                self.assertEqual(args[8:11], ['profile.release.opt-level="s"', '--target-dir', 'target/perf-opt-s'])
        self.assertEqual(measure.cargo_command('test', True)[11:], [
            '--test', 'perf_measure', '--', '--ignored', '--nocapture', '--test-threads=1'])

    def test_failed_cargo_test_preserves_samples_and_measures_snapshot(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'target/release').mkdir(parents=True)
            (root / 'target/release/benitoiteh').write_bytes(b'original binary')
            observed = []

            def execute(args, cwd, env):
                observed.append((args, dict(env)))
                if args[1] == 'build':
                    return mock.Mock(returncode=0, stdout='')
                snapshot = Path(env['BENITOITEH_PERF_BINARY'])
                (root / 'target/release/benitoiteh').write_bytes(b'test build overwrites output')
                self.assertEqual(snapshot.read_bytes(), b'original binary')
                return mock.Mock(returncode=101, stdout='PERF {"item":"version_ms","unit":"ms","samples":[2.5],"note":"test"}\n')

            with mock.patch.object(measure, 'execute', side_effect=execute), mock.patch('sys.stderr'):
                result = measure.measure_configuration(root, Path('/processor path'), 1, {}, False)
            self.assertEqual(result['records']['version_ms']['samples'], [2.5])
            self.assertIn('cargo test exited 101', result['errors'])
            self.assertTrue(any('stream_delta_ms' in error for error in result['errors']))
            self.assertIn('bytes=15;', result['records']['binary_size_mib']['note'])
            self.assertEqual(observed[1][1]['BENITOITEH_TEST_BENITOITE'], '/processor path')
            self.assertEqual(observed[1][1]['BENITOITEH_PERF_RUNS'], '1')

    def test_invalid_runs(self):
        for value in ('0', '-1', 'no', '1.5'):
            with self.subTest(value=value), self.assertRaises(measure.argparse.ArgumentTypeError):
                measure.positive(value)


if __name__ == '__main__':
    unittest.main()
