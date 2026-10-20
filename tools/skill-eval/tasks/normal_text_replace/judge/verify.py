"""Keep fixed expectations in the parent; execute artifacts only in children."""

import json
import os
import subprocess
import sys
import tempfile


def observe(case):
    with tempfile.TemporaryDirectory(prefix='replace ', dir='.') as directory:
        path = os.path.join(directory, 'text.txt')
        with open(path, 'wb') as stream:
            stream.write(case['content'].encode('utf-8'))
        args = ['--dry-run'] if case['dry_run'] else []
        result = subprocess.run([sys.executable, '-B', 'replace.py'] + args + [path, case['old'], case['new']],
                                capture_output=True, timeout=5)
        with open(path, 'rb') as stream:
            after = stream.read().decode('utf-8')
    return [result.returncode, result.stdout.decode('utf-8'), after]


CASES = [{'name': 'preview',
  'dry_run': True,
  'content': 'one one\n',
  'old': 'one',
  'new': 'two',
  'expected': [0, 'two two\n', 'one one\n']},
 {'name': 'normal',
  'dry_run': False,
  'content': 'red red',
  'old': 'red',
  'new': '青',
  'expected': [0, '', '青 青']},
 {'name': 'no_match',
  'dry_run': False,
  'content': ' unchanged\n',
  'old': 'none',
  'new': 'x',
  'expected': [0, '', ' unchanged\n']},
 {'name': 'literal',
  'dry_run': True,
  'content': 'a.b a?b\r\n',
  'old': 'a.b',
  'new': 'X',
  'expected': [0, 'X a?b\r\n', 'a.b a?b\r\n']},
 {'name': 'delete',
  'dry_run': True,
  'content': '猫の猫',
  'old': '猫',
  'new': '',
  'expected': [0, 'の', '猫の猫']},
 {'name': 'normal_crlf',
  'dry_run': False,
  'content': 'cat\r\ncat\r\n',
  'old': 'cat',
  'new': 'dog',
  'expected': [0, '', 'dog\r\ndog\r\n']},
 {'name': 'replacement_backslashes',
  'dry_run': False,
  'content': 'path: X\n',
  'old': 'X',
  'new': 'C:\\temp \\g<0>',
  'expected': [0, '', 'path: C:\\temp \\g<0>\n']}]


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
