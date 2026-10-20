"""Public behavioral contract; run from the project directory."""

import json
import os
import subprocess
import sys
import tempfile

import unittest


def observe(case):
    if case.get('api'):
        code = """import json, sys
from config import merge_config
x, y = json.load(sys.stdin)
r = merge_config(x, y)
before = json.loads(json.dumps(r))
r['left']['values'].append(99)
r['right']['values'].append(88)
print(json.dumps([before, x, y]))
"""
        if case.get('overlap'):
            code = """import json, sys
from config import merge_config
x, y = json.load(sys.stdin)
r = merge_config(x, y)
print(json.dumps([r, x, y]))
"""
        result = subprocess.run([sys.executable, '-B', '-c', code],
                                input=json.dumps(case['values']).encode('utf-8'),
                                capture_output=True, timeout=5)
    else:
        with tempfile.TemporaryDirectory(dir='.') as directory:
            paths = [os.path.join(directory, name) for name in ('defaults.json', 'overrides.json')]
            for path, value in zip(paths, case['values']):
                with open(path, 'w', encoding='utf-8') as stream:
                    json.dump(value, stream)
            result = subprocess.run([sys.executable, '-B', 'config.py'] + paths,
                                    capture_output=True, timeout=5)
    if result.stdout and not result.stdout.endswith(b'\n'):
        raise ValueError('missing final LF')
    return [result.returncode, json.loads(result.stdout)]


CASES = [{'name': 'recursive_and_falsy',
  'values': [{'ui': {'color': 'blue', 'size': 3},
              'enabled': True,
              'count': 8,
              'label': 'old',
              'optional': 4},
             {'ui': {'size': 0}, 'enabled': False, 'count': 0, 'label': '', 'optional': None}],
  'expected': [0,
               {'ui': {'color': 'blue', 'size': 0},
                'enabled': False,
                'count': 0,
                'label': '',
                'optional': None}]},
 {'name': 'replace_non_dictionary',
  'values': [{'x': {'a': 1}, 'y': [1, 2]}, {'x': [], 'y': {'z': 3}}],
  'expected': [0, {'x': [], 'y': {'z': 3}}]},
 {'name': 'detached_inputs',
  'api': True,
  'values': [{'left': {'values': [1]}}, {'right': {'values': [2]}}],
  'expected': [0,
               [{'left': {'values': [1]}, 'right': {'values': [2]}},
                {'left': {'values': [1]}},
                {'right': {'values': [2]}}]]},
 {'name': 'overlapping_inputs',
  'api': True,
  'overlap': True,
  'values': [{'s': {'a': {'x': 1}}}, {'s': {'a': {'y': 2}}}],
  'expected': [0,
               [{'s': {'a': {'x': 1, 'y': 2}}}, {'s': {'a': {'x': 1}}}, {'s': {'a': {'y': 2}}}]]}]


class ContractTests(unittest.TestCase):
    def test_contract(self):
        for case in CASES:
            with self.subTest(case=case['name']):
                self.assertEqual(json.dumps(observe(case), sort_keys=True),
                                 json.dumps(case['expected'], sort_keys=True))


if __name__ == '__main__':
    unittest.main()
