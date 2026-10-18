# Count lines of Markdown files in a directory tree

Write a Benitoite script `main.bnt` that looks at every entry under the directory `docs` in the current directory, at any depth.

For every regular file whose name ends with `.md` (directories whose names end with `.md` do not count), count its lines. Print one line per file, with its path relative to `docs` (using `/`), sorted by that path:

```text
<relative path>: <lines> lines
```

Then print the summary line:

```text
markdown files: <number of files>, lines: <total lines>
```
