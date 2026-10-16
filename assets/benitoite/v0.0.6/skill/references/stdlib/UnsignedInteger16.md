# Benitoite.UnsignedInteger16

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `UnsignedInteger16.<name>`, for example `UnsignedInteger16.fromInteger`.

Functions on `UnsignedInteger16`, the integers from 0 to 65535.

## Type

Type: `UnsignedInteger16`. Canonical name: `UnsignedInteger16`. Displayed as `UnsignedInteger16`.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function fromInteger(n: Integer) -> Option[UnsignedInteger16]
```

Converts from `Integer`. Returns `Option.None` when out of range.

```benitoite
public function toInteger(n: UnsignedInteger16) -> Integer
```

Converts exactly to `Integer`.

```benitoite
public function parse(s: String) -> Option[UnsignedInteger16]
```

Reads a decimal integer without a minus sign, whitespace, prefixes, underscores, or extra leading zeros. Returns `Option.None` when invalid or out of range.

```benitoite
public function toString(n: UnsignedInteger16) -> String
```

Returns the decimal representation.

```benitoite
public function toFloat(n: UnsignedInteger16) -> Float
```

Converts exactly to `Float`.

```benitoite
public function absolute(n: UnsignedInteger16) -> UnsignedInteger16
```

Returns the original value.

```benitoite
public function minimum(a: UnsignedInteger16, b: UnsignedInteger16) -> UnsignedInteger16
```

Returns the smaller of `a` and `b`.

```benitoite
public function maximum(a: UnsignedInteger16, b: UnsignedInteger16) -> UnsignedInteger16
```

Returns the larger of `a` and `b`.

```benitoite
public function floorDivide(a: UnsignedInteger16, b: UnsignedInteger16) -> UnsignedInteger16
```

Divides and rounds the quotient toward negative infinity. Stops the program when the divisor is 0.

```benitoite
public function floorModulo(a: UnsignedInteger16, b: UnsignedInteger16) -> UnsignedInteger16
```

Returns a remainder that is zero or has the sign of the divisor. Stops the program when the divisor is 0.

```benitoite
public function bitwiseAnd(a: UnsignedInteger16, b: UnsignedInteger16) -> UnsignedInteger16
```

Returns the bitwise AND of `a` and `b`.

```benitoite
public function bitwiseOr(a: UnsignedInteger16, b: UnsignedInteger16) -> UnsignedInteger16
```

Returns the bitwise OR of `a` and `b`.

```benitoite
public function bitwiseExclusiveOr(a: UnsignedInteger16, b: UnsignedInteger16) -> UnsignedInteger16
```

Returns the bitwise exclusive OR of `a` and `b`.

```benitoite
public function bitwiseNot(a: UnsignedInteger16) -> UnsignedInteger16
```

Inverts all bits within the width of this type.

```benitoite
public function shiftLeft(a: UnsignedInteger16, n: Integer) -> UnsignedInteger16
```

Shifts left, discarding bits beyond the width and filling low bits with 0. Stops the program unless the shift count is between 0 and 15.

```benitoite
public function shiftRight(a: UnsignedInteger16, n: Integer) -> UnsignedInteger16
```

Shifts right, filling high bits with 0. Stops the program unless the shift count is between 0 and 15.
