# Benitoite.Float32

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Float32.<name>`, for example `Float32.fromFloat`.

Conversions and functions on `Float32`, the IEEE 754 single-precision type.

## Type

Type: `Float32`. Canonical name: `Float32`. Displayed as `Float32`.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function fromFloat(n: Float) -> Option[Float32]
```

Narrows with round-to-nearest, ties-to-even. Returns `Option.None` when a finite value becomes infinite, and `Option.Some` for existing infinities and NaN. Preserves the sign of zero.

```benitoite
public function toFloat(n: Float32) -> Float
```

Widens to `Float`, preserving finite values exactly and the sign of zero.

```benitoite
public function parse(s: String) -> Option[Float32]
```

Reads the same decimal syntax as `Float` directly at single precision. Returns `Option.None` for invalid syntax, infinities, and NaN.

```benitoite
public function toString(n: Float32) -> String
```

Returns the shortest decimal representation that round-trips to single precision. Uses the same spelling as `Float` for negative zero, NaN, Infinity, and -Infinity.

```benitoite
public function truncate(n: Float32) -> Option[Integer]
```

Rounds toward zero. Returns `Option.None` for NaN, infinities, and values out of the `Integer` range.

```benitoite
public function isNaN(n: Float32) -> Boolean
```

Returns whether `n` is NaN.

```benitoite
public function isInfinite(n: Float32) -> Boolean
```

Returns whether `n` is a positive or negative infinity.

```benitoite
public function absolute(n: Float32) -> Float32
```

Returns `n` with a positive sign.

```benitoite
public function floor(n: Float32) -> Float32
```

Returns the largest integral value not greater than `n`.

```benitoite
public function ceiling(n: Float32) -> Float32
```

Returns the smallest integral value not less than `n`.

```benitoite
public function round(n: Float32) -> Float32
```

Returns the nearest integral value, rounding halves away from zero.

```benitoite
public function squareRoot(n: Float32) -> Float32
```

Returns the square root. Negative numbers give NaN.
