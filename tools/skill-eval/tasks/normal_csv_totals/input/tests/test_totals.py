"""Public CLI checks; run from the project directory."""
import os
import subprocess
import sys
import tempfile
import unittest


class TotalsTests(unittest.TestCase):
    def test_exact_cents_and_order(self):
        with tempfile.TemporaryDirectory(dir='.') as directory:
            path = os.path.join(directory, 'input.csv')
            with open(path, 'w', encoding='utf-8') as stream:
                stream.write('item,amount\npear,0.29\napple,0.10\napple,0.20\n')
            result = subprocess.run([sys.executable, 'totals.py', path], capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')
        self.assertEqual(result.stdout, 'apple,0.30\npear,0.29\n')

    def test_empty(self):
        with tempfile.TemporaryDirectory(dir='.') as directory:
            path = os.path.join(directory, 'empty.csv')
            with open(path, 'w'):
                pass
            result = subprocess.run([sys.executable, 'totals.py', path], capture_output=True, text=True, timeout=5)
        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, '', ''))
