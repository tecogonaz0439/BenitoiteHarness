# Read a list of files and report the ones that cannot be read

Write a Benitoite script `main.bnt` that reads `files.txt` in the current directory. Each line is the path of a file to read.

For each path, in order, print one line:

- `<path>: <lines> lines` when the file can be read;
- `<path>: not found` when it does not exist;
- `<path>: cannot read` when reading it fails for any other reason (for example, the path is a directory).

The script must not stop at the first failure. At the end, print:

```text
read <files read> of <paths in files.txt> files, <total lines> lines
```

The script must exit with code 0 even when some files cannot be read. If `files.txt` itself cannot be read, `main` must return an error.
