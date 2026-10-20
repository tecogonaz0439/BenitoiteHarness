# Fix half-open interval overlap to pass the tests

Repair `overlaps(start_a, end_a, start_b, end_b)` in `ranges.py` so the original
public tests pass. Arguments are integers representing half-open intervals
`[start_a, end_a)` and `[start_b, end_b)`. Return a bool: true exactly when their
intersection has positive length. Touching endpoints and empty intervals do not
overlap. Containment and equal nonempty intervals overlap; negative endpoints work.

Raise `ValueError` if either start is greater than its end, even if the other
interval is empty or disjoint. The exception message is unspecified. This task
requires a source correction, not changes to the tests. The module has no CLI.

Edit only `ranges.py`. Keep the tests and other files unchanged. Use only the Python
standard library and installed system tools; no downloads or network access.
Run `python3 -m unittest discover -s tests` from this project directory to check your change.
