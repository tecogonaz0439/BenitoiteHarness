"""Keep fixed expectations in the parent; execute artifacts only in children."""

import json
import os
import subprocess
import sys
import tempfile


def observe(case):
    with tempfile.TemporaryDirectory(dir='.') as directory:
        path = os.path.join(directory, 'records.json')
        with open(path, 'w', encoding='utf-8') as stream:
            json.dump(case['rows'], stream)
        args = ['--descending'] if case['descending'] else []
        result = subprocess.run([sys.executable, '-B', 'report.py'] + args + [path, case['key']],
                                capture_output=True, timeout=5)
    value = json.loads(result.stdout) if result.stdout else None
    if result.stdout and not result.stdout.endswith(b'\n'):
        raise ValueError('missing final LF')
    return [result.returncode, value, result.stderr.decode('utf-8')]


CASES = [{'name': 'descending_ties',
  'descending': True,
  'key': 'score',
  'rows': [{'score': 2, 'id': 'x'},
           {'score': 9, 'id': 'y'},
           {'score': 2, 'id': 'z'},
           {'score': -1, 'id': 'w'}],
  'expected': [0,
               [{'score': 9, 'id': 'y'},
                {'score': 2, 'id': 'x'},
                {'score': 2, 'id': 'z'},
                {'score': -1, 'id': 'w'}],
               '']},
 {'name': 'ascending',
  'descending': False,
  'key': 'n',
  'rows': [{'n': 12}, {'n': -2}, {'n': 3}],
  'expected': [0, [{'n': -2}, {'n': 3}, {'n': 12}], '']},
 {'name': 'strings',
  'descending': True,
  'key': 's',
  'rows': [{'s': 'Beta'}, {'s': 'alpha'}, {'s': 'Gamma'}],
  'expected': [0, [{'s': 'alpha'}, {'s': 'Gamma'}, {'s': 'Beta'}], '']},
 {'name': 'ascending_ties',
  'descending': False,
  'key': 's',
  'rows': [{'s': 'z', 'id': 1}, {'s': 'a', 'id': 2}, {'s': 'a', 'id': 3}],
  'expected': [0, [{'s': 'a', 'id': 2}, {'s': 'a', 'id': 3}, {'s': 'z', 'id': 1}], '']},
 {'name': 'empty', 'descending': True, 'key': 'any', 'rows': [], 'expected': [0, [], '']},
 {'name': 'missing',
  'descending': False,
  'key': 'score',
  'rows': [{}, {'score': 3}],
  'expected': [2, None, 'missing key: score\n']}]


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
