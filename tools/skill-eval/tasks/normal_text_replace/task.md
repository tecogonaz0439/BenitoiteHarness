# Add a dry-run text replacement option

Extend `python3 replace.py PATH OLD NEW` with optional `--dry-run` before PATH.
OLD is nonempty. Replace every non-overlapping literal occurrence in the UTF-8 file;
OLD and NEW are single arguments and may contain spaces or Japanese text.
NEW is inserted literally, including backslashes. In normal
mode update the file and write no stdout. In dry-run mode print the complete updated
content to stdout and leave the original bytes unchanged. Both modes exit 0.

Preserve all unaffected content, CRLF versus LF, and the presence or absence of the
final newline. Do not add a newline to a preview. No match keeps the file unchanged
and previews the original content. File I/O and argument errors are outside this task.

Edit only `replace.py`. Keep the tests and other files unchanged. Use only the Python
standard library and installed system tools; no downloads or network access.
Run `python3 -m unittest discover -s tests` from this project directory to check your change.
