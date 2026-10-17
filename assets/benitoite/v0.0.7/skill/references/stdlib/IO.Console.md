# Benitoite.IO.Console

Status: unofficial. Import it with `import Benitoite.Unofficial.IO.Console`. When the module becomes standard, the import name changes to `import Benitoite.IO.Console`.

Refer to its declarations as `Console.<name>`, for example `Console.Write`.

Standard input, standard output, and standard error.

```benitoite
public effect Write
  /// Writes `text` to standard output.
  function write(text: String) -> Unit
  /// Writes `text` and a line feed to standard output.
  function writeLine(text: String) -> Unit
  /// Writes `text` to standard error.
  function writeError(text: String) -> Unit
  /// Writes `text` and a line feed to standard error.
  function writeErrorLine(text: String) -> Unit
end effect
```

Writing to standard output and standard error.

```benitoite
public effect Read
  /// Reads one line. Returns `Option.None` at the end of the input.
  function readLine() -> Result[Option[String], IOError]
  /// Reads the rest of the input.
  function readAll() -> Result[String, IOError]
  /// Reads the rest of the input and splits it into lines.
  function readAllLines() -> Result[List[String], IOError]
  /// Reads the rest of the input as bytes.
  function readAllBytes() -> Result[Bytes, IOError]
end effect
```

Reading standard input.
