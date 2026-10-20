"""Run each artifact in a child; keep expectations in this trusted parent."""
import json
import os
import subprocess
import sys
import tempfile


def main():
    cases = []
    for name, files, directories, expected in [
        ('spaces', ['two words.txt', 'a.txt', 'skip.md'], ['folder.txt'], 'a.txt\ntwo words.txt\n'),
        ('hidden', ['.secret.txt', '.txt', 'UPPER.TXT', 'z.txt'], [], '.secret.txt\n.txt\nz.txt\n'),
        ('nested', ['b.txt', 'sub/child.txt'], ['sub'], 'b.txt\n'),
        ('empty', [], [], ''),
        ('links', ['target.txt'], [], 'target.txt\n'),
    ]:
        try:
            with tempfile.TemporaryDirectory(prefix='case ', dir='.') as directory:
                for child in directories:
                    os.mkdir(os.path.join(directory, child))
                for child in files:
                    with open(os.path.join(directory, child), 'w'):
                        pass
                if name == 'links':
                    os.symlink('target.txt', os.path.join(directory, 'alias.txt'))
                result = subprocess.run(['/bin/sh', 'collect.sh', directory], capture_output=True, text=True, timeout=5)
            success = (result.returncode, result.stdout, result.stderr) == (0, expected, '')
            reason = 'ok' if success else 'CLI output or exit status differs'
        except (OSError, subprocess.TimeoutExpired):
            success, reason = False, 'CLI did not complete'
        cases.append(dict(name=name, success=success, reason=reason))
    print(json.dumps(dict(cases=cases)))
    return 0 if all(case['success'] for case in cases) else 1


if __name__ == '__main__':
    sys.exit(main())
