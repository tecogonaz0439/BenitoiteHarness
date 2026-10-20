# Repair direct text-file collection

Fix `collect.sh`. You may edit only that file. Keep all original tests and inputs.
Run it as `/bin/sh collect.sh <directory>` with exactly one existing directory.
Print the basenames of its direct regular files whose names end in `.txt`, one
name per line, sorted in byte order (`LC_ALL=C`), each with a final newline.
Include hidden matching files. Preserve spaces in names. Exclude directories,
symbolic links, nested files, and other extensions (including `.TXT`). An empty
directory succeeds with no output. Valid input exits with status 0 and no stderr.
Names containing newlines, invalid directories, and argument errors are outside
this task. Paths and filenames in this task are ASCII.

Use only installed local tools; do not install packages or access the Internet.
The local check is `python3 -m unittest discover -s tests`. Run it after your fix.
Generated `__pycache__/` and `tests/__pycache__/` files are allowed. Do not add project instructions,
skills, plugins, tests, or other files.
