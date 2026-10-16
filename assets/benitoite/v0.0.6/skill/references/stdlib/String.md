# Benitoite.String

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `String.<name>`, for example `String.byteLength`.

Functions on `String`, a sequence of Unicode scalar values.

```benitoite
public function byteLength(s: String) -> Integer
```

Returns the number of bytes in the UTF-8 encoding of `s`.

```benitoite
public function byteSlice(s: String, start: Integer, stop: Integer) -> Option[String]
```

Returns the part from byte position `start` to `stop`, or `Option.None` when the positions are not valid.

```benitoite
public function characterCount(s: String) -> Integer
```

Returns the number of scalar values in `s`.

```benitoite
public function characterAt(s: String, i: Integer) -> Option[Character]
```

Returns the scalar value at character position `i`, or `Option.None` when `i` is out of range.

```benitoite
public function characterSlice(s: String, start: Integer, stop: Integer) -> Option[String]
```

Returns the part from character position `start` to `stop`, or `Option.None` when the positions are not valid.

```benitoite
public function isEmpty(s: String) -> Boolean
```

Returns whether `s` is the empty string.

```benitoite
public function contains(s: String, part: String) -> Boolean
```

Returns whether `s` contains `part`.

```benitoite
public function startsWith(s: String, prefix: String) -> Boolean
```

Returns whether `s` starts with `prefix`.

```benitoite
public function endsWith(s: String, suffix: String) -> Boolean
```

Returns whether `s` ends with `suffix`.

```benitoite
public function byteIndexOf(s: String, part: String) -> Option[Integer]
```

Returns the byte position of the first `part` in `s`, or `Option.None` when it does not appear.

```benitoite
public function split(s: String, separator: String) -> List[String]
```

Splits `s` at each `separator`. An empty separator splits into single characters.

```benitoite
public function lines(s: String) -> List[String]
```

Splits `s` into lines at LF, dropping a CR before each LF.

```benitoite
public function join(xs: List[String], separator: String) -> String
```

Joins the strings of `xs`, putting `separator` between them.

```benitoite
public function trim(s: String) -> String
```

Removes ASCII whitespace from both ends.

```benitoite
public function replace(s: String, old: String, new: String) -> String
```

Replaces every non-overlapping `old` in `s` with `new`, from the start.

```benitoite
public function repeat(s: String, n: Integer) -> String
```

Repeats `s` `n` times. Returns the empty string when `n` is 0 or less.

```benitoite
public function characters(s: String) -> List[Character]
```

Returns the scalar values of `s` in order.

```benitoite
public function fromCharacters(cs: List[Character]) -> String
```

Joins the characters of `cs` into a string.

```benitoite
public function toUppercase(s: String) -> String
```

Returns `s` with each character changed by `Character.toUppercase`.

```benitoite
public function toLowercase(s: String) -> String
```

Returns `s` in lowercase. A capital sigma at the end of a word becomes the final form.

```benitoite
public function toUTF8(s: String) -> Bytes
```

Returns the UTF-8 encoding of `s`.

```benitoite
public function fromUTF8(b: Bytes) -> Option[String]
```

Reads `b` as UTF-8. Returns `Option.None` when `b` is not valid UTF-8. A leading BOM is kept.
