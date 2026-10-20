"""Public behavioral contract; run from the project directory."""

import json
import subprocess
import sys

import unittest

CASE_CODE = """import json, sys
from manifest import validate_manifest
try:
    result = {'returned': validate_manifest(json.load(sys.stdin))}
except ValueError:
    result = {'raised': 'ValueError'}
print(json.dumps(result))
"""


def observe(case):
    code = CASE_CODE
    if case.get('detached'):
        code = """import json, sys
from manifest import validate_manifest
value = json.load(sys.stdin)
result = validate_manifest(value)
before = list(result)
result.append('changed.txt')
print(json.dumps([before, value]))
"""
    result = subprocess.run([sys.executable, '-B', '-c', code],
                            input=json.dumps(case['value']).encode('utf-8'), capture_output=True, timeout=5)
    return [result.returncode, json.loads(result.stdout)]


CASES = [{'name': 'detached_inputs',
  'detached': True,
  'value': {'files': ['a/b']},
  'expected': [0, [['a/b'], {'files': ['a/b']}]]},
 {'name': 'nested',
  'value': {'files': ['src/main.py', 'docs/read me.md']},
  'expected': [0, {'returned': ['src/main.py', 'docs/read me.md']}]},
 {'name': 'empty_list', 'value': {'files': []}, 'expected': [0, {'returned': []}]},
 {'name': 'array_type', 'value': {'files': 'a.py'}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'duplicate', 'value': {'files': ['a', 'a']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'absolute', 'value': {'files': ['/a']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'parent', 'value': {'files': ['x/../y']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'empty_path', 'value': {'files': ['']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'dot', 'value': {'files': ['./a']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'separator', 'value': {'files': ['a//b']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'backslash', 'value': {'files': ['a\\b']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'nul', 'value': {'files': ['a\x00b']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'leading_parent',
  'value': {'files': ['../a']},
  'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'parent_only', 'value': {'files': ['..']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'dot_only', 'value': {'files': ['.']}, 'expected': [0, {'raised': 'ValueError'}]}]


class ContractTests(unittest.TestCase):
    def test_contract(self):
        for case in CASES:
            with self.subTest(case=case['name']):
                self.assertEqual(json.dumps(observe(case), sort_keys=True),
                                 json.dumps(case['expected'], sort_keys=True))


if __name__ == '__main__':
    unittest.main()
