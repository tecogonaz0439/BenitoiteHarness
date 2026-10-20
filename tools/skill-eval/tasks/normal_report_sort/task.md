# Add stable descending report sorting

Extend `python3 report.py INPUT_JSON KEY` with optional `--descending` before INPUT_JSON.
The input is a UTF-8 JSON array of objects. Write the sorted array as JSON followed
by LF and exit 0 with empty stderr. Without the flag sort ascending; with it sort
descending. Equal key values must retain their input order in both directions.

Each present key has either a finite number or a string, with a consistent type
within the array. Compare numbers numerically and strings by Python's ordinary
case-sensitive order. Preserve every record and field. Empty arrays produce `[]`.
JSON spacing and object key order do not matter. If any record lacks KEY, emit no
stdout, write exactly `missing key: KEY` plus LF to stderr and exit 2. Other invalid
input and I/O errors are outside this task.

Edit only `report.py`. Keep the tests and other files unchanged. Use only the Python
standard library and installed system tools; no downloads or network access.
Run `python3 -m unittest discover -s tests` from this project directory to check your change.
