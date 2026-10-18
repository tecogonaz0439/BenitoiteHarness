# Write tests for a settings reader

Write a Benitoite test file `settings_test.bnt` (run it with `benitoite test settings_test.bnt`; it has no `main`). It must contain these two functions and tests for them.

- `parseSetting(line: String) -> Option[Pair[String, String]]`: removes spaces at both ends of `line`. An empty line or a line starting with `#` gives `Option.None`. Otherwise the line is split at the first `=`: the key is the part before it and the value is everything after it (it may contain more `=`), both with spaces removed at both ends. A line without `=`, or with an empty key, gives `Option.None`.
- `loadSettings(path: String) -> Result[Map[String, String], String] uses File.Read`: reads the file with `File.readLines` and returns a map of all settings parsed by `parseSetting`. A read error becomes `Result.Error` with the error message.

Write at least 4 tests, and all of them must pass:

- at least 3 tests of `parseSetting` (a normal line, a value that contains `=`, a comment or blank line);
- at least 1 test of `loadSettings` that replaces `File.readLines` with a handler (`handle ... with case File.readLines(path) -> ...`), so that the test does not read a real file.
