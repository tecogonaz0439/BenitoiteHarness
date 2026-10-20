"""Keep fixed expectations in the parent; execute artifacts only in children."""

import json
import os
import subprocess
import sys
import tempfile


def observe(case):
    with tempfile.TemporaryDirectory(dir='.') as directory:
        path = os.path.join(directory, 'lines.txt')
        with open(path, 'wb') as stream:
            stream.write(case['content'].encode('utf-8'))
        args = ['--ignore-case'] if case['ignore_case'] else []
        result = subprocess.run([sys.executable, '-B', 'unique.py'] + args + [path],
                                capture_output=True, timeout=5)
    return [result.returncode, result.stdout.decode('utf-8')]


CASES = [{'name': 'casefold',
  'ignore_case': True,
  'content': 'Straße\nSTRASSE\nZ\nz',
  'expected': [0, 'Straße\nZ\n']},
 {'name': 'default', 'ignore_case': False, 'content': 'q\nQ\nq\nA\n', 'expected': [0, 'q\nQ\nA\n']},
 {'name': 'empty_lines', 'ignore_case': True, 'content': '\n\n', 'expected': [0, '\n']},
 {'name': 'crlf',
  'ignore_case': False,
  'content': ' x \r\ny\r\n x \r\nend',
  'expected': [0, ' x \ny\nend\n']},
 {'name': 'empty', 'ignore_case': True, 'content': '', 'expected': [0, '']}]


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
