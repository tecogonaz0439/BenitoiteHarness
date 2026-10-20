# Add case-insensitive stable line deduplication

Extend `python3 unique.py INPUT` with optional `--ignore-case` before INPUT. Read a
UTF-8 file and emit each distinct line once, in order of its first occurrence.
Without the flag compare case-sensitively. With it compare using Python `str.casefold()`;
retain the first line's original spelling. Spaces are significant, including leading
and trailing spaces. Inputs use LF or CRLF line endings; treat both as separators.

An empty input emits nothing. Empty lines count as a value; a final separator does
not create an extra empty line. Always emit one LF after each retained line, including
an unterminated final input line. Exit 0. Other separators, I/O and argument errors
are outside this task.

Edit only `unique.py`. Keep the tests and other files unchanged. Use only the Python
standard library and installed system tools; no downloads or network access.
Run `python3 -m unittest discover -s tests` from this project directory to check your change.
