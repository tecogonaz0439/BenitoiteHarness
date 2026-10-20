"""Keep fixed expectations in the parent; execute artifacts only in children."""

import json
import subprocess
import sys

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
  'value': {'files': ['lib/a.txt', '日本語/文書.md', '..notes', 'a..b'], 'extra': 1},
  'expected': [0, {'returned': ['lib/a.txt', '日本語/文書.md', '..notes', 'a..b']}]},
 {'name': 'duplicate',
  'value': {'files': ['x/y', 'a', 'x/y']},
  'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'absolute',
  'value': {'files': ['/etc/config']},
  'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'parent', 'value': {'files': ['a/b/../c']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'empty_path', 'value': {'files': ['']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'array_type', 'value': {'files': {'x': 1}}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'null_array', 'value': {'files': None}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'missing', 'value': {}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'root_type', 'value': [], 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'element_type', 'value': {'files': ['a', 7]}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'trailing_separator',
  'value': {'files': ['a/']},
  'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'dot_component',
  'value': {'files': ['a/./b']},
  'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'backslash', 'value': {'files': ['x\\y']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'nul', 'value': {'files': ['z\x00']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'empty', 'value': {'files': []}, 'expected': [0, {'returned': []}]},
 {'name': 'leading_parent',
  'value': {'files': ['../a']},
  'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'parent_only', 'value': {'files': ['..']}, 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'dot_only', 'value': {'files': ['.']}, 'expected': [0, {'raised': 'ValueError'}]}]


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
