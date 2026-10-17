# Benitoite.Integer8

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Integer8.<name>`, for example `Integer8.fromInteger`.

Functions on `Integer8`, the integers from -128 to 127.

## Type

Type: `Integer8`. Canonical name: `Integer8`. Displayed as `Integer8`.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function fromInteger(n: Integer) -> Option[Integer8]
```

Converts from `Integer`. Returns `Option.None` when out of range.

```benitoite
public function toInteger(n: Integer8) -> Integer
```

Converts exactly to `Integer`.

```benitoite
public function parse(s: String) -> Option[Integer8]
```

Reads a decimal integer without whitespace, prefixes, underscores, or extra leading zeros. Returns `Option.None` when invalid or out of range.

```benitoite
public function toString(n: Integer8) -> String
```

Returns the decimal representation, with a leading `-` when negative.

```benitoite
public function toFloat(n: Integer8) -> Float
```

Converts exactly to `Float`.

```benitoite
public function absolute(n: Integer8) -> Integer8
```

Returns the absolute value. Stops the program for the smallest value of this type.

```benitoite
public function minimum(a: Integer8, b: Integer8) -> Integer8
```

Returns the smaller of `a` and `b`.

```benitoite
public function maximum(a: Integer8, b: Integer8) -> Integer8
```

Returns the larger of `a` and `b`.

```benitoite
public function floorDivide(a: Integer8, b: Integer8) -> Integer8
```

Divides and rounds the quotient toward negative infinity. Stops the program when the divisor is 0 or the smallest value is divided by -1.

```benitoite
public function floorModulo(a: Integer8, b: Integer8) -> Integer8
```

Returns a remainder that is zero or has the sign of the divisor. Stops the program when the divisor is 0.

```benitoite
public function bitwiseAnd(a: Integer8, b: Integer8) -> Integer8
```

Returns the bitwise AND of `a` and `b`.

```benitoite
public function bitwiseOr(a: Integer8, b: Integer8) -> Integer8
```

Returns the bitwise OR of `a` and `b`.

```benitoite
public function bitwiseExclusiveOr(a: Integer8, b: Integer8) -> Integer8
```

Returns the bitwise exclusive OR of `a` and `b`.

```benitoite
public function bitwiseNot(a: Integer8) -> Integer8
```

Inverts all bits within the width of this type.

```benitoite
public function shiftLeft(a: Integer8, n: Integer) -> Integer8
```

Shifts left, discarding bits beyond the width and filling low bits with 0. Stops the program unless the shift count is between 0 and 7.

```benitoite
public function shiftRight(a: Integer8, n: Integer) -> Integer8
```

Shifts right, filling high bits with the sign bit. Stops the program unless the shift count is between 0 and 7.

```benitoite
public function shiftRightUnsigned(a: Integer8, n: Integer) -> Integer8
```

Shifts right, filling high bits with 0. Stops the program unless the shift count is between 0 and 7.
