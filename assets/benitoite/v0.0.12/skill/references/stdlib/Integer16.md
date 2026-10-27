# Benitoite.Integer16

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Integer16.<name>`, for example `Integer16.fromInteger`.

Functions on `Integer16`, the integers from -32768 to 32767.

For direct calls to this module's shift functions, local interval analysis reports W0404 only when the entire inferred interval for `n` is outside `[0,15]`.
The warning means that, if execution reaches the call and all arguments are evaluated successfully, the call stops the program at runtime.
Intersecting or unknown intervals and calls without normal argument values do not produce W0404. Runtime range checks remain; no warning does not mean safe.

## Type

Type: `Integer16`. Canonical name: `Integer16`. Displayed as `Integer16`.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function fromInteger(n: Integer) -> Option[Integer16]
```

Converts from `Integer`. Returns `Option.None` when out of range.

```benitoite
public function toInteger(n: Integer16) -> Integer
```

Converts exactly to `Integer`.

```benitoite
public function parse(s: String) -> Option[Integer16]
```

Reads a decimal integer without whitespace, prefixes, underscores, or extra leading zeros. Returns `Option.None` when invalid or out of range.

```benitoite
public function toString(n: Integer16) -> String
```

Returns the decimal representation, with a leading `-` when negative.

```benitoite
public function toFloat(n: Integer16) -> Float
```

Converts exactly to `Float`.

```benitoite
public function absolute(n: Integer16) -> Integer16
```

Returns the absolute value. Stops the program for the smallest value of this type.

```benitoite
public function minimum(a: Integer16, b: Integer16) -> Integer16
```

Returns the smaller of `a` and `b`.

```benitoite
public function maximum(a: Integer16, b: Integer16) -> Integer16
```

Returns the larger of `a` and `b`.

```benitoite
public function floorDivide(a: Integer16, b: Integer16) -> Integer16
```

Divides and rounds the quotient toward negative infinity. Stops the program when the divisor is 0 or the smallest value is divided by -1.

```benitoite
public function floorModulo(a: Integer16, b: Integer16) -> Integer16
```

Returns a remainder that is zero or has the sign of the divisor. Stops the program when the divisor is 0.

```benitoite
public function bitwiseAnd(a: Integer16, b: Integer16) -> Integer16
```

Returns the bitwise AND of `a` and `b`.

```benitoite
public function bitwiseOr(a: Integer16, b: Integer16) -> Integer16
```

Returns the bitwise OR of `a` and `b`.

```benitoite
public function bitwiseExclusiveOr(a: Integer16, b: Integer16) -> Integer16
```

Returns the bitwise exclusive OR of `a` and `b`.

```benitoite
public function bitwiseNot(a: Integer16) -> Integer16
```

Inverts all bits within the width of this type.

```benitoite
public function shiftLeft(a: Integer16, n: Integer) -> Integer16
```

Shifts left, discarding bits beyond the width and filling low bits with 0. Stops the program unless the shift count is between 0 and 15.

```benitoite
public function shiftRight(a: Integer16, n: Integer) -> Integer16
```

Shifts right, filling high bits with the sign bit. Stops the program unless the shift count is between 0 and 15.

```benitoite
public function shiftRightUnsigned(a: Integer16, n: Integer) -> Integer16
```

Shifts right, filling high bits with 0. Stops the program unless the shift count is between 0 and 15.
