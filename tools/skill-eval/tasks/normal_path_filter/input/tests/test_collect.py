"""Public CLI checks; names containing newlines are out of scope."""
import os
import subprocess
import tempfile
import unittest


class CollectionTests(unittest.TestCase):
    def test_spaces_and_regular_files(self):
        with tempfile.TemporaryDirectory(dir='.') as directory:
            for name in ('z.txt', 'a space.txt', 'skip.csv'):
                with open(os.path.join(directory, name), 'w'):
                    pass
            os.mkdir(os.path.join(directory, 'folder.txt'))
            result = subprocess.run(['/bin/sh', 'collect.sh', directory], capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')
        self.assertEqual(result.stdout, 'a space.txt\nz.txt\n')

    def test_empty(self):
        with tempfile.TemporaryDirectory(dir='.') as directory:
            result = subprocess.run(['/bin/sh', 'collect.sh', directory], capture_output=True, text=True, timeout=5)
        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, '', ''))
