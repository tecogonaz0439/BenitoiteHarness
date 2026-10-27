# Benitoite.Float

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Float.<name>`, for example `Float.toString`.

Functions on `Float`, the IEEE 754 double-precision type.

## Type

Type: `Float`. Canonical name: `Float64`. Displayed as `Float`.

Accepted aliases for this type: `Float`. These names denote the same type; they do not require a conversion.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function toString(x: Float) -> String
```

Returns the text representation of `x`.

```benitoite
public function parse(s: String) -> Option[Float]
```

Reads a decimal number. Returns `Option.None` when `s` is not a number.

```benitoite
public function truncate(x: Float) -> Option[Integer]
```

Rounds toward zero. Returns `Option.None` for NaN, infinities, and values out of the `Integer` range.

```benitoite
public function isNaN(x: Float) -> Boolean
```

Returns whether `x` is NaN.

```benitoite
public function absolute(x: Float) -> Float
```

Returns `x` with a positive sign.

```benitoite
public function floor(x: Float) -> Float
```

Returns the largest integral value not greater than `x`.

```benitoite
public function ceiling(x: Float) -> Float
```

Returns the smallest integral value not less than `x`.

```benitoite
public function round(x: Float) -> Float
```

Returns the nearest integral value, rounding halves away from zero.

```benitoite
public function squareRoot(x: Float) -> Float
```

Returns the square root. Negative numbers give NaN.
