"""Public behavioral contract; run from the project directory."""

import json
import os
import subprocess
import sys
import tempfile

import unittest


def observe(case):
    with tempfile.TemporaryDirectory(dir='.') as directory:
        path = os.path.join(directory, 'input.jsonl')
        with open(path, 'w', encoding='utf-8') as stream:
            stream.write(case['content'])
        result = subprocess.run([sys.executable, '-B', 'filter.py', '--min-score', case['minimum'], path],
                                capture_output=True, timeout=5)
    rows = [json.loads(line) for line in result.stdout.splitlines()]
    if result.stdout and not result.stdout.endswith(b'\n'):
        raise ValueError('missing final LF')
    return [result.returncode, rows, result.stderr.decode('utf-8')]


CASES = [{'name': 'threshold',
  'minimum': '0',
  'content': '{"id":"a","score":0}\n{"id":"b","score":-1}\n{"id":"c"}\n{"id":"d","score":1.5}\n',
  'expected': [0, [{'id': 'a', 'score': 0}, {'id': 'd', 'score': 1.5}], '']},
 {'name': 'empty', 'minimum': '2', 'content': '', 'expected': [0, [], '']},
 {'name': 'invalid',
  'minimum': '0',
  'content': '{"score":1}\nno json\n',
  'expected': [2, [], 'invalid JSON on line 2\n']},
 {'name': 'blank_line',
  'minimum': '0',
  'content': '{"score":5}\n \n',
  'expected': [2, [], 'invalid JSON on line 2\n']}]


class ContractTests(unittest.TestCase):
    def test_contract(self):
        for case in CASES:
            with self.subTest(case=case['name']):
                self.assertEqual(json.dumps(observe(case), sort_keys=True),
                                 json.dumps(case['expected'], sort_keys=True))


if __name__ == '__main__':
    unittest.main()
