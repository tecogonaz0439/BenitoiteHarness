"""Public behavioral contract; run from the project directory."""

import json
import os
import subprocess
import sys
import tempfile

import unittest


def observe(case):
    with tempfile.TemporaryDirectory(dir='.') as directory:
        path = os.path.join(directory, 'records.json')
        with open(path, 'w', encoding='utf-8') as stream:
            json.dump(case['rows'], stream)
        args = ['--descending'] if case['descending'] else []
        result = subprocess.run([sys.executable, '-B', 'report.py'] + args + [path, case['key']],
                                capture_output=True, timeout=5)
    value = json.loads(result.stdout) if result.stdout else None
    if result.stdout and not result.stdout.endswith(b'\n'):
        raise ValueError('missing final LF')
    return [result.returncode, value, result.stderr.decode('utf-8')]


CASES = [{'name': 'ascending',
  'descending': False,
  'key': 'n',
  'rows': [{'n': 2}, {'n': 10}, {'n': 1}],
  'expected': [0, [{'n': 1}, {'n': 2}, {'n': 10}], '']},
 {'name': 'descending_stable',
  'descending': True,
  'key': 'n',
  'rows': [{'n': 1, 'id': 'a'}, {'n': 2, 'id': 'b'}, {'n': 1, 'id': 'c'}],
  'expected': [0, [{'n': 2, 'id': 'b'}, {'n': 1, 'id': 'a'}, {'n': 1, 'id': 'c'}], '']},
 {'name': 'strings',
  'descending': True,
  'key': 'name',
  'rows': [{'name': 'a'}, {'name': 'z'}],
  'expected': [0, [{'name': 'z'}, {'name': 'a'}], '']},
 {'name': 'empty', 'descending': False, 'key': 'n', 'rows': [], 'expected': [0, [], '']},
 {'name': 'missing',
  'descending': True,
  'key': 'n',
  'rows': [{'n': 1}, {}],
  'expected': [2, None, 'missing key: n\n']}]


class ContractTests(unittest.TestCase):
    def test_contract(self):
        for case in CASES:
            with self.subTest(case=case['name']):
                self.assertEqual(json.dumps(observe(case), sort_keys=True),
                                 json.dumps(case['expected'], sort_keys=True))


if __name__ == '__main__':
    unittest.main()
