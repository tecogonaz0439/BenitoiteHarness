"""検査が新しく作った askpass の空の親だけを片付ける（設計書 07-01）。"""

import json
import os
from pathlib import Path
import stat
import sys


def prepare(root: Path, record: Path) -> None:
    """既存の親を再利用し、新しく作れた場合だけ実体を記録する。"""
    parent = root.resolve() / f"bha-{os.geteuid()}"
    identity = None
    try:
        parent.mkdir(mode=0o700)
    except FileExistsError:
        pass
    else:
        metadata = parent.lstat()
        identity = [metadata.st_dev, metadata.st_ino]
    record.write_text(json.dumps({"parent": str(parent), "identity": identity}))


def cleanup(record: Path) -> None:
    """実体・所有者・権限が一致し、空の場合だけ削除する。"""
    state = json.loads(record.read_text())
    if state["identity"] is None:
        return
    parent = Path(state["parent"])
    metadata = parent.lstat()
    if (
        [metadata.st_dev, metadata.st_ino] != state["identity"]
        or not stat.S_ISDIR(metadata.st_mode)
        or metadata.st_uid != os.geteuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        raise OSError("Refusing to remove a changed askpass test directory.")
    # rmdir は中身が残る場合に失敗する。残った補助を再帰的に消して隠さない。
    parent.rmdir()


def main() -> None:
    """検査スクリプトから準備または後始末を呼ぶ。"""
    action, record_name = sys.argv[1:]
    record = Path(record_name)
    if action == "prepare":
        prepare(Path("/tmp"), record)
    elif action == "cleanup":
        try:
            cleanup(record)
        finally:
            record.unlink()
    else:
        raise ValueError("Expected prepare or cleanup.")


if __name__ == "__main__":
    main()
