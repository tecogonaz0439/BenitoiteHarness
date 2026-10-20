# Implement a JSONL score filter

Implement `python3 filter.py --min-score NUMBER INPUT_JSONL`. Read UTF-8 JSONL
objects and write objects with a numeric `score >= NUMBER` as JSONL in their original
order. Exclude missing scores, strings, booleans and null. Each emitted object ends
with LF; JSON spacing and object key order do not matter. Empty input produces no
output and exits 0. Thresholds and numeric scores are finite integers or floats.

A malformed JSON line (including a blank line) must produce exactly
`invalid JSON on line N` plus LF on stderr, using a one-based line number, exit 2,
and produce no stdout, even if earlier rows matched. Valid JSON lines are objects;
other shapes and file I/O errors are outside this task. Success exits 0 with empty stderr.

Edit only `filter.py`. Keep the tests and other files unchanged. Use only the Python
standard library and installed system tools; no downloads or network access.
Run `python3 -m unittest discover -s tests` from this project directory to check your change.
