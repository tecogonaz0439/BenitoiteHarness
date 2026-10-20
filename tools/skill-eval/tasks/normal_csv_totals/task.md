# Repair CSV totals

Fix `totals.py`. You may edit only that file. Keep the original tests and inputs.
Use the Python standard library and installed local tools; do not install packages.

Run `python3 totals.py <csv-path>` with exactly one input path. Nonempty CSV input
has the header `item,amount`; amounts have at most two decimal places and may be
negative. Sum amounts exactly by item, without losing cents to binary floating
point. Item names are nonempty ASCII words. Output one `item,amount` CSV row per
item, sorted by item name, with exactly two decimal places, no header, and a final
newline for each row. Use `0.00` for a zero total. Empty input (an empty file or a
header alone) succeeds with no output. Valid input exits with status 0 and no
stderr. Invalid input and argument errors are outside this task.

The local check is `python3 -m unittest discover -s tests`. Run it after your fix.
Generated `__pycache__/` and `tests/__pycache__/` files are allowed. Do not add project instructions,
skills, plugins, tests, or other files.
