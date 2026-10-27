# Benitoite.Character

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Character.<name>`, for example `Character.toInteger`.

Functions on `Character`, a Unicode scalar value.

```benitoite
public function toInteger(c: Character) -> Integer
```

Returns the scalar value of `c`.

```benitoite
public function fromInteger(n: Integer) -> Option[Character]
```

Returns the character with scalar value `n`, or `Option.None` when `n` is not a scalar value.

```benitoite
public function toString(c: Character) -> String
```

Returns the string made of `c` alone.

```benitoite
public function isASCIIDigit(c: Character) -> Boolean
```

Returns whether `c` is one of '0' to '9'.

```benitoite
public function isASCIIWhitespace(c: Character) -> Boolean
```

Returns whether `c` is an ASCII space, tab, line feed, or carriage return.

```benitoite
public function isAlphabetic(c: Character) -> Boolean
```

Returns whether `c` has the Unicode Alphabetic property.

```benitoite
public function isNumeric(c: Character) -> Boolean
```

Returns whether the Unicode general category of `c` is Nd, Nl, or No.

```benitoite
public function isWhitespace(c: Character) -> Boolean
```

Returns whether `c` has the Unicode White_Space property.

```benitoite
public function isUppercase(c: Character) -> Boolean
```

Returns whether `c` has the Unicode Uppercase property.

```benitoite
public function isLowercase(c: Character) -> Boolean
```

Returns whether `c` has the Unicode Lowercase property.

```benitoite
public function toUppercase(c: Character) -> String
```

Returns `c` in uppercase. One character may become several, as 'ß' becomes "SS".

```benitoite
public function toLowercase(c: Character) -> String
```

Returns `c` in lowercase. One character may become several.
