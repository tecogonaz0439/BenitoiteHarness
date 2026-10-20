# Repair greeting CLI errors

Run `/bin/sh tool.sh greet NAME`. Exactly these two arguments print `hello NAME`
followed by LF to stdout and exit 0, with empty stderr. NAME is a single argument;
preserve spaces, an empty string, and literal punctuation without glob expansion.

Missing arguments, extra arguments, and an unknown subcommand must exit 2, write
exactly `usage: tool.sh greet <name>` followed by LF to stderr, and leave stdout empty.

Edit only `tool.sh`. Keep the tests and other files unchanged. Use only the Python
standard library and installed system tools; no downloads or network access.
Run `python3 -m unittest discover -s tests` from this project directory to check your change.
