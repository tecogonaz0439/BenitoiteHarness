"""Keep fixed expectations in the parent; execute artifacts only in children."""

import json
import os
import subprocess
import sys
import tempfile


def observe(case):
    with tempfile.TemporaryDirectory(dir='.') as directory:
        path = os.path.join(directory, 'input.jsonl')
        with open(path, 'w', encoding='utf-8') as stream:
            stream.write(case['content'])
        result = subprocess.run([sys.executable, '-B', 'filter.py', '--min-score', case['minimum'], path],
                                capture_output=True, timeout=5)
    rows = [json.loads(line) for line in result.stdout.splitlines()]
    if result.stdout and not result.stdout.endswith(b'\n'):
        raise ValueError('missing final LF')
    return [result.returncode, rows, result.stderr.decode('utf-8')]


CASES = [{'name': 'equality',
  'minimum': '2.5',
  'content': '{"id":"z","score":2.5}\n{"id":"a","score":3}\n{"score":2}\n',
  'expected': [0, [{'id': 'z', 'score': 2.5}, {'id': 'a', 'score': 3}], '']},
 {'name': 'zero_and_types',
  'minimum': '0',
  'content': '{"score":0}\n{}\n{"score":false}\n{"score":"4"}\n{"score":null}\n',
  'expected': [0, [{'score': 0}], '']},
 {'name': 'negative',
  'minimum': '-2',
  'content': '{"score":-2}\n{"score":-3}\n',
  'expected': [0, [{'score': -2}], '']},
 {'name': 'empty', 'minimum': '0', 'content': '', 'expected': [0, [], '']},
 {'name': 'invalid_line',
  'minimum': '0',
  'content': '{}\n{}\n{bad\n',
  'expected': [2, [], 'invalid JSON on line 3\n']},
 {'name': 'blank_line',
  'minimum': '0',
  'content': '{"score":5}\n \n',
  'expected': [2, [], 'invalid JSON on line 2\n']}]


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
