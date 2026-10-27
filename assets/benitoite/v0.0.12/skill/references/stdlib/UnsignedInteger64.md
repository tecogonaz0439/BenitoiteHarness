# Benitoite.UnsignedInteger64

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `UnsignedInteger64.<name>`, for example `UnsignedInteger64.fromInteger`.

Conversions and functions over the full range of `UnsignedInteger64`.

For direct calls to this module's shift functions, local interval analysis reports W0404 only when the entire inferred interval for `n` is outside `[0,63]`.
The warning means that, if execution reaches the call and all arguments are evaluated successfully, the call stops the program at runtime.
Intersecting or unknown intervals and calls without normal argument values do not produce W0404. Runtime range checks remain; no warning does not mean safe.

## Type

Type: `UnsignedInteger64`. Canonical name: `UnsignedInteger64`. Displayed as `UnsignedInteger64`.

Accepted aliases for this type: `UnsignedInteger`. These names denote the same type; they do not require a conversion.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function fromInteger(n: Integer) -> Option[UnsignedInteger64]
```

Converts from `Integer`. Returns `Option.None` for negative values.

```benitoite
public function toInteger(n: UnsignedInteger64) -> Option[Integer]
```

Converts to `Integer`. Returns `Option.None` when out of range.

```benitoite
public function parse(s: String) -> Option[UnsignedInteger64]
```

Reads a decimal integer without a minus sign, whitespace, prefixes, underscores, or extra leading zeros. Returns `Option.None` when invalid or out of range.

```benitoite
public function toString(n: UnsignedInteger64) -> String
```

Returns the decimal representation.

```benitoite
public function toFloat(n: UnsignedInteger64) -> Float
```

Returns the nearest `Float`, rounding to binary64 precision.

```benitoite
public function toDecimal(n: UnsignedInteger64) -> Decimal
```

Converts exactly to `Decimal` over the full range of this type.

```benitoite
public function fromDecimal(x: Decimal) -> Option[UnsignedInteger64]
```

Converts a mathematically integral `Decimal` in range, including values such as `1.0m`. Returns `Option.None` otherwise.

```benitoite
public function absolute(n: UnsignedInteger64) -> UnsignedInteger64
```

Returns the original value.

```benitoite
public function minimum(a: UnsignedInteger64, b: UnsignedInteger64) -> UnsignedInteger64
```

Returns the smaller of `a` and `b`.

```benitoite
public function maximum(a: UnsignedInteger64, b: UnsignedInteger64) -> UnsignedInteger64
```

Returns the larger of `a` and `b`.

```benitoite
public function floorDivide(a: UnsignedInteger64, b: UnsignedInteger64) -> UnsignedInteger64
```

Divides and rounds the quotient toward negative infinity. Stops the program when the divisor is 0.

```benitoite
public function floorModulo(a: UnsignedInteger64, b: UnsignedInteger64) -> UnsignedInteger64
```

Returns the remainder. Stops the program when the divisor is 0.

```benitoite
public function bitwiseAnd(a: UnsignedInteger64, b: UnsignedInteger64) -> UnsignedInteger64
```

Returns the bitwise AND of `a` and `b`.

```benitoite
public function bitwiseOr(a: UnsignedInteger64, b: UnsignedInteger64) -> UnsignedInteger64
```

Returns the bitwise OR of `a` and `b`.

```benitoite
public function bitwiseExclusiveOr(a: UnsignedInteger64, b: UnsignedInteger64) -> UnsignedInteger64
```

Returns the bitwise exclusive OR of `a` and `b`.

```benitoite
public function bitwiseNot(a: UnsignedInteger64) -> UnsignedInteger64
```

Inverts all bits within the width of this type.

```benitoite
public function shiftLeft(a: UnsignedInteger64, n: Integer) -> UnsignedInteger64
```

Shifts left, discarding bits beyond the width and filling low bits with 0. Stops the program unless the shift count is between 0 and 63.

```benitoite
public function shiftRight(a: UnsignedInteger64, n: Integer) -> UnsignedInteger64
```

Shifts right, filling high bits with 0. Stops the program unless the shift count is between 0 and 63.
