"""Keep fixed expectations in the parent; execute artifacts only in children."""

import json
import subprocess
import sys

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
 {'name': 'touching', 'value': [-5, -2, -2, 0], 'expected': [0, {'returned': False}]},
 {'name': 'containment', 'value': [-8, 10, -1, 2], 'expected': [0, {'returned': True}]},
 {'name': 'negative', 'value': [-7, -3, -6, -1], 'expected': [0, {'returned': True}]},
 {'name': 'equal', 'value': [2, 5, 2, 5], 'expected': [0, {'returned': True}]},
 {'name': 'empty_inside', 'value': [3, 7, 5, 5], 'expected': [0, {'returned': False}]},
 {'name': 'empty_both', 'value': [4, 4, 4, 4], 'expected': [0, {'returned': False}]},
 {'name': 'disjoint', 'value': [8, 10, 1, 7], 'expected': [0, {'returned': False}]},
 {'name': 'symmetric', 'value': [3, 6, 1, 4], 'expected': [0, {'returned': True}]},
 {'name': 'reverse_a', 'value': [5, 0, 1, 2], 'expected': [0, {'raised': 'ValueError'}]},
 {'name': 'reverse_b', 'value': [-2, 0, 7, 3], 'expected': [0, {'raised': 'ValueError'}]}]


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
