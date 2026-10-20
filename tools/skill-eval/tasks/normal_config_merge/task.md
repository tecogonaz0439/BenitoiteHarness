# Repair recursive configuration merging

`python3 config.py DEFAULTS_JSON OVERRIDES_JSON` takes two file paths, each pointing
to a UTF-8 JSON object. It reads those files and
writes the merged JSON object followed by LF to stdout, exiting 0. JSON formatting
and object key order do not matter. Inputs are valid JSON objects.

Also preserve the importable `merge_config(defaults, overrides)` function. Merge
objects recursively only when both values are objects; otherwise replace the whole
value, including arrays. An absent key differs from `false`, `0`, `""`, and `null`.
Do not mutate either input. The result must not share mutable objects or arrays
with either input: changing a nested result later must leave both inputs unchanged.

Edit only `config.py`. Keep the tests and other files unchanged. Use only the Python
standard library and installed system tools; no downloads or network access.
Run `python3 -m unittest discover -s tests` from this project directory to check your change.
