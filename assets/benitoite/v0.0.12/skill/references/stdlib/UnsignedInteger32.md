# Benitoite.UnsignedInteger32

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `UnsignedInteger32.<name>`, for example `UnsignedInteger32.fromInteger`.

Functions on `UnsignedInteger32`, the integers from 0 to 4294967295.

For direct calls to this module's shift functions, local interval analysis reports W0404 only when the entire inferred interval for `n` is outside `[0,31]`.
The warning means that, if execution reaches the call and all arguments are evaluated successfully, the call stops the program at runtime.
Intersecting or unknown intervals and calls without normal argument values do not produce W0404. Runtime range checks remain; no warning does not mean safe.

## Type

Type: `UnsignedInteger32`. Canonical name: `UnsignedInteger32`. Displayed as `UnsignedInteger32`.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function fromInteger(n: Integer) -> Option[UnsignedInteger32]
```

Converts from `Integer`. Returns `Option.None` when out of range.

```benitoite
public function toInteger(n: UnsignedInteger32) -> Integer
```

Converts exactly to `Integer`.

```benitoite
public function parse(s: String) -> Option[UnsignedInteger32]
```

Reads a decimal integer without a minus sign, whitespace, prefixes, underscores, or extra leading zeros. Returns `Option.None` when invalid or out of range.

```benitoite
public function toString(n: UnsignedInteger32) -> String
```

Returns the decimal representation.

```benitoite
public function toFloat(n: UnsignedInteger32) -> Float
```

Converts exactly to `Float`.

```benitoite
public function absolute(n: UnsignedInteger32) -> UnsignedInteger32
```

Returns the original value.

```benitoite
public function minimum(a: UnsignedInteger32, b: UnsignedInteger32) -> UnsignedInteger32
```

Returns the smaller of `a` and `b`.

```benitoite
public function maximum(a: UnsignedInteger32, b: UnsignedInteger32) -> UnsignedInteger32
```

Returns the larger of `a` and `b`.

```benitoite
public function floorDivide(a: UnsignedInteger32, b: UnsignedInteger32) -> UnsignedInteger32
```

Divides and rounds the quotient toward negative infinity. Stops the program when the divisor is 0.

```benitoite
public function floorModulo(a: UnsignedInteger32, b: UnsignedInteger32) -> UnsignedInteger32
```

Returns a remainder that is zero or has the sign of the divisor. Stops the program when the divisor is 0.

```benitoite
public function bitwiseAnd(a: UnsignedInteger32, b: UnsignedInteger32) -> UnsignedInteger32
```

Returns the bitwise AND of `a` and `b`.

```benitoite
public function bitwiseOr(a: UnsignedInteger32, b: UnsignedInteger32) -> UnsignedInteger32
```

Returns the bitwise OR of `a` and `b`.

```benitoite
public function bitwiseExclusiveOr(a: UnsignedInteger32, b: UnsignedInteger32) -> UnsignedInteger32
```

Returns the bitwise exclusive OR of `a` and `b`.

```benitoite
public function bitwiseNot(a: UnsignedInteger32) -> UnsignedInteger32
```

Inverts all bits within the width of this type.

```benitoite
public function shiftLeft(a: UnsignedInteger32, n: Integer) -> UnsignedInteger32
```

Shifts left, discarding bits beyond the width and filling low bits with 0. Stops the program unless the shift count is between 0 and 31.

```benitoite
public function shiftRight(a: UnsignedInteger32, n: Integer) -> UnsignedInteger32
```

Shifts right, filling high bits with 0. Stops the program unless the shift count is between 0 and 31.
