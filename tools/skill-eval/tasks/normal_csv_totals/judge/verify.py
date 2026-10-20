"""Compare child CLI output against fixed independent expectations."""
import json
import os
import subprocess
import sys
import tempfile


def main():
    cases = []
    for name, content, expected in [
        ('fractional', 'item,amount\nzinc,0.57\nberry,0.10\nberry,0.20\n', 'berry,0.30\nzinc,0.57\n'),
        ('negative', 'item,amount\nrefund,-1.15\nrefund,0.29\nother,2.01\n', 'other,2.01\nrefund,-0.86\n'),
        ('repeated', 'item,amount\nz,10.11\na,0.01\nz,0.29\na,-0.01\n', 'a,0.00\nz,10.40\n'),
        ('empty', '', ''),
        ('header', 'item,amount\n', ''),
    ]:
        try:
            with tempfile.TemporaryDirectory(dir='.') as directory:
                path = os.path.join(directory, 'case.csv')
                with open(path, 'w', encoding='utf-8') as stream:
                    stream.write(content)
                result = subprocess.run([sys.executable, 'totals.py', path], capture_output=True, text=True, timeout=5)
            success = (result.returncode, result.stdout, result.stderr) == (0, expected, '')
            reason = 'ok' if success else 'CLI output or exit status differs'
        except (OSError, subprocess.TimeoutExpired):
            success, reason = False, 'CLI did not complete'
        cases.append(dict(name=name, success=success, reason=reason))
    print(json.dumps(dict(cases=cases)))
    return 0 if all(case['success'] for case in cases) else 1


if __name__ == '__main__':
    sys.exit(main())
