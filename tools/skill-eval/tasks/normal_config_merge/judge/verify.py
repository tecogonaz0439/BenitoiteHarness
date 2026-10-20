"""Keep fixed expectations in the parent; execute artifacts only in children."""

import json
import os
import subprocess
import sys
import tempfile


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


CASES = [{'name': 'nested',
  'values': [{'service': {'host': 'local', 'limits': {'cpu': 2, 'memory': 8}}},
             {'service': {'limits': {'cpu': 4}}}],
  'expected': [0, {'service': {'host': 'local', 'limits': {'cpu': 4, 'memory': 8}}}]},
 {'name': 'falsy',
  'values': [{'a': 7, 'b': True, 'c': 'abc', 'd': [1]}, {'a': 0, 'b': False, 'c': '', 'd': None}],
  'expected': [0, {'a': 0, 'b': False, 'c': '', 'd': None}]},
 {'name': 'replace',
  'values': [{'a': [1, 2], 'b': {'x': 1}, 'c': 2}, {'a': [3], 'b': None, 'c': {'q': 0}}],
  'expected': [0, {'a': [3], 'b': None, 'c': {'q': 0}}]},
 {'name': 'empty', 'values': [{}, {}], 'expected': [0, {}]},
 {'name': 'immutability',
  'api': True,
  'values': [{'left': {'values': [4, 5]}}, {'right': {'values': [6, 7]}}],
  'expected': [0,
               [{'left': {'values': [4, 5]}, 'right': {'values': [6, 7]}},
                {'left': {'values': [4, 5]}},
                {'right': {'values': [6, 7]}}]]},
 {'name': 'overlapping_inputs',
  'api': True,
  'overlap': True,
  'values': [{'s': {'a': {'x': 3}}}, {'s': {'a': {'y': 4}}}],
  'expected': [0,
               [{'s': {'a': {'x': 3, 'y': 4}}}, {'s': {'a': {'x': 3}}}, {'s': {'a': {'y': 4}}}]]}]


def main():
    results = []
    for case in CASES:
        try:
            success = (json.dumps(observe(case), sort_keys=True)
                       == json.dumps(case['expected'], sort_keys=True))
            reason = 'ok' if success else 'output, file contents or exit status differs'
        except (OSError, ValueError, subprocess.TimeoutExpired):
            success, reason = False, 'artifact did not produce a valid result'
        results.append(dict(name=case['name'], success=success, reason=reason))
    print(json.dumps(dict(cases=results)))
    return 0 if all(result['success'] for result in results) else 1


if __name__ == '__main__':
    sys.exit(main())
