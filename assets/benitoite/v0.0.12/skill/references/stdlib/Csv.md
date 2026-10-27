# Benitoite.Csv

Status: unofficial. Import it with `import Benitoite.Unofficial.Csv`. When the module becomes standard, the import name changes to `import Benitoite.Csv`.

Refer to its declarations as `Csv.<name>`, for example `Csv.ParseError`.

Reading and writing CSV as in RFC 4180. Every field is a `String`.

```benitoite
@extensible
public record ParseError
  line: Integer
  message: String
end record
```

Why parsing failed. `line` is the line where the record with the error starts, counting from 1.

```benitoite
public function parse(text: String) -> Result[List[List[String]], ParseError]
```

Reads `text` as rows of fields separated by commas.

```benitoite
public function parseWith(text: String, delimiter: Character) -> Result[List[List[String]], ParseError]
```

Reads `text` as rows of fields separated by `delimiter`. `delimiter` must be an ASCII character other than `"`, CR, and LF.

```benitoite
public function parseWithHeader(text: String) -> Result[List[Map[String, String]], ParseError]
```

Reads the first row as the header and each other row as a map from header names to fields.

```benitoite
public function format(rows: List[List[String]]) -> String
```

Writes the rows as CSV, ending each row with a line feed.

```benitoite
public function formatWith(rows: List[List[String]], delimiter: Character) -> String
```

Writes the rows as CSV with `delimiter` between fields. `delimiter` must be an ASCII character other than `"`, CR, and LF.
