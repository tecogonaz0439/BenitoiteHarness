# Benitoite.Json.Codec

Status: unofficial. Import it with `import Benitoite.Unofficial.Json.Codec`. When the module becomes standard, the import name changes to `import Benitoite.Json.Codec`.

Refer to its declarations as `Codec.<name>`, for example `Codec.PathPart`.

Pure, explicit JSON conversions. Failures contain relative paths, never input payloads.

```benitoite
public data PathPart
  Field(String)
  Index(Integer)
  Key(String)
end data
```

One step from the root of a converted value.

```benitoite
@extensible
public record ConversionError
  path: List[PathPart]
  expected: String
  found: String
  reason: String
end record
```

A conversion failure. `path` runs from the outermost container to the failure.

```benitoite
public function kind(value: Json.Value) -> String
```

Returns the JSON kind without including any input content.

```benitoite
public function error(expected: String, found: String, reason: String) -> ConversionError
```

Builds a relative conversion failure without copying input payloads.

```benitoite
public function prepend(part: PathPart, error: ConversionError) -> ConversionError
```

Adds an outer path component to a relative failure.

```benitoite
public function decodeObject(value: Json.Value) -> Result[Map[String, Json.Value], ConversionError]
```

Requires an object before decoding any record fields.

```benitoite
public function decodeField[T](members: Map[String, Json.Value], key: String, decode: function(value: Json.Value) -> Result[T, ConversionError], decodeMissing: function() -> Result[T, ConversionError]) -> Result[T, ConversionError]
```

Decodes a present field or uses an explicit missing-field conversion, adding its field path.

```benitoite
public function missing[T]() -> Result[T, ConversionError]
```

Fails for a required missing field; the caller adds its field path.

```benitoite
public function rejectExtra(members: Map[String, Json.Value], keys: List[String]) -> Result[Unit, ConversionError]
```

Rejects the first unexpected object key in key order, after all fields succeeded.
Uses a Key path so Show quotes and escapes the input key.

```benitoite
public function encodeString(value: String) -> Result[Json.Value, ConversionError]
```

Encodes a String without implicit numeric conversion.

```benitoite
public function decodeString(value: Json.Value) -> Result[String, ConversionError]
```

Decodes only the JSON string kind.

```benitoite
public function encodeBoolean(value: Boolean) -> Result[Json.Value, ConversionError]
```

Encodes a Boolean without implicit numeric conversion.

```benitoite
public function decodeBoolean(value: Json.Value) -> Result[Boolean, ConversionError]
```

Decodes only the JSON boolean kind.

```benitoite
public function encodeInteger(value: Integer) -> Result[Json.Value, ConversionError]
```

Encodes a Integer without implicit numeric conversion.

```benitoite
public function decodeInteger(value: Json.Value) -> Result[Integer, ConversionError]
```

Decodes only the JSON integer kind.

```benitoite
public function encodeFloat(value: Float) -> Result[Json.Value, ConversionError]
```

Encodes a finite Float without implicit numeric conversion.

```benitoite
public function decodeFloat(value: Json.Value) -> Result[Float, ConversionError]
```

Decodes only the JSON float kind; rejects NaN and infinity.

```benitoite
public function encodeOption[T](value: Option[T], encode: function(value: T) -> Result[Json.Value, ConversionError]) -> Result[Json.Value, ConversionError]
```

Encodes None as null, or Some using the supplied conversion.

```benitoite
public function decodeOption[T](value: Json.Value, decode: function(value: Json.Value) -> Result[T, ConversionError]) -> Result[Option[T], ConversionError]
```

Decodes null as None, or a non-null value using the supplied conversion.

```benitoite
public function encodeList[T](value: List[T], encode: function(value: T) -> Result[Json.Value, ConversionError]) -> Result[Json.Value, ConversionError]
```

Encodes in index order and adds the first failing index to the path.

```benitoite
public function decodeList[T](value: Json.Value, decode: function(value: Json.Value) -> Result[T, ConversionError]) -> Result[List[T], ConversionError]
```

Decodes in index order and adds the first failing index to the path.

```benitoite
public function encodeMap[T](value: Map[String, T], encode: function(value: T) -> Result[Json.Value, ConversionError]) -> Result[Json.Value, ConversionError]
```

Encodes a String-keyed map in key order, adding the first failing key to the path.

```benitoite
public function decodeMap[T](value: Json.Value, decode: function(value: Json.Value) -> Result[T, ConversionError]) -> Result[Map[String, T], ConversionError]
```

Decodes a String-keyed map in key order, adding the first failing key to the path.
