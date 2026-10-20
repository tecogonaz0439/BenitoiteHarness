"""Public behavioral contract; run from the project directory."""

import json
import os
import subprocess
import sys
import tempfile

import unittest


def observe(case):
    with tempfile.TemporaryDirectory(dir='.') as directory:
        path = os.path.join(directory, 'lines.txt')
        with open(path, 'wb') as stream:
            stream.write(case['content'].encode('utf-8'))
        args = ['--ignore-case'] if case['ignore_case'] else []
        result = subprocess.run([sys.executable, '-B', 'unique.py'] + args + [path],
                                capture_output=True, timeout=5)
    return [result.returncode, result.stdout.decode('utf-8')]


CASES = [{'name': 'default', 'ignore_case': False, 'content': 'B\na\nB\nA', 'expected': [0, 'B\na\nA\n']},
 {'name': 'folded',
  'ignore_case': True,
  'content': 'Hello\nHELLO\nworld\nWorld\n',
  'expected': [0, 'Hello\nworld\n']},
 {'name': 'empty_lines_crlf',
  'ignore_case': True,
  'content': '\r\nX\r\n\r\nx',
  'expected': [0, '\nX\n']},
 {'name': 'empty', 'ignore_case': False, 'content': '', 'expected': [0, '']}]


class ContractTests(unittest.TestCase):
    def test_contract(self):
        for case in CASES:
            with self.subTest(case=case['name']):
                self.assertEqual(json.dumps(observe(case), sort_keys=True),
                                 json.dumps(case['expected'], sort_keys=True))


if __name__ == '__main__':
    unittest.main()
