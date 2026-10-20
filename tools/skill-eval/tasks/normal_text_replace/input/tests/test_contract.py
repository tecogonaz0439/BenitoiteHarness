"""Public behavioral contract; run from the project directory."""

import json
import os
import subprocess
import sys
import tempfile

import unittest


def observe(case):
    with tempfile.TemporaryDirectory(prefix='replace ', dir='.') as directory:
        path = os.path.join(directory, 'text.txt')
        with open(path, 'wb') as stream:
            stream.write(case['content'].encode('utf-8'))
        args = ['--dry-run'] if case['dry_run'] else []
        result = subprocess.run([sys.executable, '-B', 'replace.py'] + args + [path, case['old'], case['new']],
                                capture_output=True, timeout=5)
        with open(path, 'rb') as stream:
            after = stream.read().decode('utf-8')
    return [result.returncode, result.stdout.decode('utf-8'), after]


CASES = [{'name': 'normal',
  'dry_run': False,
  'content': 'cat cat\n',
  'old': 'cat',
  'new': 'dog',
  'expected': [0, '', 'dog dog\n']},
 {'name': 'preview_japanese',
  'dry_run': True,
  'content': '猫と猫',
  'old': '猫',
  'new': '犬',
  'expected': [0, '犬と犬', '猫と猫']},
 {'name': 'no_match',
  'dry_run': True,
  'content': 'hello\n',
  'old': 'absent',
  'new': 'x',
  'expected': [0, 'hello\n', 'hello\n']},
 {'name': 'replacement_backslashes',
  'dry_run': False,
  'content': 'cost: X\n',
  'old': 'X',
  'new': 'C:\\temp \\g<0>',
  'expected': [0, '', 'cost: C:\\temp \\g<0>\n']}]


class ContractTests(unittest.TestCase):
    def test_contract(self):
        for case in CASES:
            with self.subTest(case=case['name']):
                self.assertEqual(json.dumps(observe(case), sort_keys=True),
                                 json.dumps(case['expected'], sort_keys=True))


if __name__ == '__main__':
    unittest.main()
