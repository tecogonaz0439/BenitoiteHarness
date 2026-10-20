"""Public behavioral contract; run from the project directory."""

import json
import subprocess
import sys

import unittest

CASE_CODE = """import json, sys
from ranges import overlaps
try:
    result = {'returned': overlaps(*json.load(sys.stdin))}
except ValueError:
    result = {'raised': 'ValueError'}
print(json.dumps(result))
"""


def observe(case):
    code = CASE_CODE
    result = subprocess.run([sys.executable, '-B', '-c', code],
                            input=json.dumps(case['value']).encode('utf-8'), capture_output=True, timeout=5)
    return [result.returncode, json.loads(result.stdout)]


CASES = [{'name': 'reversed_with_empty', 'value': [9, 3, 5, 5], 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'touching', 'value': [1, 3, 3, 5], 'expected': [0, {'returned': False}]},
 {'name': 'overlap', 'value': [1, 4, 3, 5], 'expected': [0, {'returned': True}]},
 {'name': 'empty', 'value': [2, 2, 1, 4], 'expected': [0, {'returned': False}]},
 {'name': 'reverse_a', 'value': [4, 1, 0, 3], 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'reverse_b', 'value': [0, 3, 4, 1], 'expected': [0, {'raised': 'ValueError'}]}]


class ContractTests(unittest.TestCase):
    def test_contract(self):
        for case in CASES:
            with self.subTest(case=case['name']):
                self.assertEqual(json.dumps(observe(case), sort_keys=True),
                                 json.dumps(case['expected'], sort_keys=True))


if __name__ == '__main__':
    unittest.main()
