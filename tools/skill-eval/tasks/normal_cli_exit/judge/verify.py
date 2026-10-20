"""Keep fixed expectations in the parent; execute artifacts only in children."""

import json
import subprocess
import sys


def observe(case):
    result = subprocess.run(['/bin/sh', 'tool.sh'] + case['args'], capture_output=True, timeout=5)
    return [result.returncode, result.stdout.decode('utf-8'), result.stderr.decode('utf-8')]


CASES = [{'name': 'spaces', 'args': ['greet', 'Lin Q'], 'expected': [0, 'hello Lin Q\n', '']},
 {'name': 'literal', 'args': ['greet', '-n * %s \\t'], 'expected': [0, 'hello -n * %s \\t\n', '']},
 {'name': 'empty_name', 'args': ['greet', ''], 'expected': [0, 'hello \n', '']},
 {'name': 'missing', 'args': [], 'expected': [2, '', 'usage: tool.sh greet <name>\n']},
 {'name': 'missing_name', 'args': ['greet'], 'expected': [2, '', 'usage: tool.sh greet <name>\n']},
 {'name': 'extra',
  'args': ['greet', 'Bo', 'x'],
  'expected': [2, '', 'usage: tool.sh greet <name>\n']},
 {'name': 'unknown', 'args': ['bye', 'Lin'], 'expected': [2, '', 'usage: tool.sh greet <name>\n']}]


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
