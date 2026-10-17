Check a Benitoite script, or the main.bnt of a directory, without running it.
Set `tests` to true to check a file, or every .bnt file under a directory, without requiring `main` and without running tests (like `benitoite test --check`).
A file passed directly resolves imports from its own directory; to check a module that imports others by names from the project root, pass the project directory with `tests` set to true.
