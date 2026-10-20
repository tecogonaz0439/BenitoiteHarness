# Fix manifest validation to pass the tests

Repair `validate_manifest(manifest)` in `manifest.py` so the original public tests
pass. Input is a JSON value. Require an object with a `files` array of unique,
nonempty relative POSIX path strings. Return a new list in input order containing
those paths, without modifying the input. Other object fields are allowed.

Reject absolute paths (leading `/`), any `..` or `.` component, empty components
(including `//` or a trailing `/`), backslashes and NUL characters. Reject duplicates
by exact string equality, a missing `files`, a non-array `files`, and non-string
entries. An empty array is valid; ordinary names such as `..notes`, spaces and
Japanese text are valid. Do not inspect the filesystem or normalize paths.
Raise `ValueError` for invalid input; its message is unspecified. This task requires
a source correction, not changes to the tests. The module has no CLI.

Edit only `manifest.py`. Keep the tests and other files unchanged. Use only the Python
standard library and installed system tools; no downloads or network access.
Run `python3 -m unittest discover -s tests` from this project directory to check your change.
