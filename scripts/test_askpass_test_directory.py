"""検査の後始末が、作った空の親だけを消す契約を実ファイルで確かめる。"""

import os
from pathlib import Path
import tempfile
import unittest

from askpass_test_directory import cleanup, prepare


class AskpassTestDirectoryTests(unittest.TestCase):
    def test_only_the_created_unchanged_empty_parent_is_removed(self):
        for case in ("created", "existing", "nonempty", "replaced", "symlink", "mode"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                parent = root / f"bha-{os.geteuid()}"
                record = root / "record.json"
                if case == "existing":
                    parent.mkdir(mode=0o700)
                prepare(root, record)
                if case == "nonempty":
                    (parent / "keep").write_bytes(b"keep")
                elif case in ("replaced", "symlink"):
                    # 元を残して、同じ inode が再利用される可能性を避ける。
                    saved = root / "saved"
                    parent.rename(saved)
                    if case == "symlink":
                        parent.symlink_to(saved, target_is_directory=True)
                    else:
                        parent.mkdir(mode=0o700)
                elif case == "mode":
                    parent.chmod(0o755)
                if case in ("created", "existing"):
                    cleanup(record)
                    self.assertEqual(parent.exists(), case == "existing")
                else:
                    with self.assertRaises(OSError):
                        cleanup(record)
                    self.assertTrue(parent.exists())
                    if case == "nonempty":
                        self.assertEqual((parent / "keep").read_bytes(), b"keep")


if __name__ == "__main__":
    unittest.main()
