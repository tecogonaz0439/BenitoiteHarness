"""Public behavioral contract; run from the project directory."""

import json
import subprocess
import sys

import unittest


def observe(case):
    result = subprocess.run(['/bin/sh', 'tool.sh'] + case['args'], capture_output=True, timeout=5)
    return [result.returncode, result.stdout.decode('utf-8'), result.stderr.decode('utf-8')]


CASES = [{'name': 'greet', 'args': ['greet', 'Ada'], 'expected': [0, 'hello Ada\n', '']},
 {'name': 'spaces', 'args': ['greet', 'Ada Lovelace'], 'expected': [0, 'hello Ada Lovelace\n', '']},
 {'name': 'missing', 'args': [], 'expected': [2, '', 'usage: tool.sh greet <name>\n']},
 {'name': 'missing_name', 'args': ['greet'], 'expected': [2, '', 'usage: tool.sh greet <name>\n']},
 {'name': 'extra',
  'args': ['greet', 'Ada', 'extra'],
  'expected': [2, '', 'usage: tool.sh greet <name>\n']},
 {'name': 'unknown', 'args': ['bye', 'Ada'], 'expected': [2, '', 'usage: tool.sh greet <name>\n']},
 {'name': 'literal', 'args': ['greet', '-n * %s \\t'], 'expected': [0, 'hello -n * %s \\t\n', '']}]


class ContractTests(unittest.TestCase):
    def test_contract(self):
        for case in CASES:
            with self.subTest(case=case['name']):
                self.assertEqual(json.dumps(observe(case), sort_keys=True),
                                 json.dumps(case['expected'], sort_keys=True))


if __name__ == '__main__':
    unittest.main()
