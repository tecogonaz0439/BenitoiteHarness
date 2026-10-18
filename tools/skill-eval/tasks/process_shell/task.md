# Search a log with a shell pipeline

`app.log` in the current directory has lines of the form `<date> <level> <component> <message...>`, separated by single spaces.

Write a Benitoite script `main.bnt` that runs shell command lines (POSIX `sh`) to:

1. Find the components of the lines whose level is `ERROR`, without duplicates, sorted with `LC_ALL=C sort -u`. Use one pipeline with `grep`, `cut` and `sort`. Print each component on its own line, then `components with errors: <count>`.
2. Check whether any line has the level `FATAL` with `grep -q`. Print `FATAL lines: found` when grep's exit code is 0; otherwise print `FATAL lines: none (grep exit code <code>)`.

Expected output shape:

```text
<component>
...
components with errors: <count>
FATAL lines: none (grep exit code 1)
```
