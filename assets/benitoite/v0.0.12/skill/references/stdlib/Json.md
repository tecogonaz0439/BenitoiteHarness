# Benitoite.Json

Status: unofficial. Import it with `import Benitoite.Unofficial.Json`. When the module becomes standard, the import name changes to `import Benitoite.Json`.

Refer to its declarations as `Json.<name>`, for example `Json.Value`.

JSON values, parsing, and writing.

```benitoite
public data Value
  Null
  Boolean(Boolean)
  Integer(Integer)
  Float(Float)
  String(String)
  Array(List[Value])
  Object(Map[String, Value])
end data
```

A JSON value. Object members are kept in the order of their keys.

```benitoite
@extensible
public record ParseError
  line: Integer
  column: Integer
  message: String
end record
```

Where and why parsing failed. `line` and `column` count from 1; `column` counts characters.

```benitoite
public function parse(text: String) -> Result[Value, ParseError]
```

Reads `text` as JSON. Spaces before and after the value are allowed.

```benitoite
public function stringify(value: Value) -> String
```

Writes the value without spaces. Members are written in the order of their keys.

```benitoite
public function stringifyPretty(value: Value) -> String
```

Writes the value with line breaks and two spaces of indentation per level.

```benitoite
public function get(value: Value, k: String) -> Option[Value]
```

Returns the member `k` of an object, or `Option.None` for other values and missing members.

```benitoite
public function at(value: Value, index: Integer) -> Option[Value]
```

Returns the element at `index` of an array, or `Option.None` for other values and positions out of range.

```benitoite
public function asString(value: Value) -> Option[String]
```

Returns the content of `Value.String`.

```benitoite
public function asInteger(value: Value) -> Option[Integer]
```

Returns the content of `Value.Integer`.

```benitoite
public function asFloat(value: Value) -> Option[Float]
```

Returns the content of `Value.Float`, or the content of `Value.Integer` converted to `Float`.

```benitoite
public function asBoolean(value: Value) -> Option[Boolean]
```

Returns the content of `Value.Boolean`.

```benitoite
public function asArray(value: Value) -> Option[List[Value]]
```

Returns the content of `Value.Array`.

```benitoite
public function asObject(value: Value) -> Option[Map[String, Value]]
```

Returns the content of `Value.Object`.
