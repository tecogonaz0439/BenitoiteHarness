# Benitoite.UnsignedInteger

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `UnsignedInteger.<name>`, for example `UnsignedInteger.fromInteger`.

Functions on `UnsignedInteger`, an alias of `UnsignedInteger64`.

## Type

Type: `UnsignedInteger`. Canonical name: `UnsignedInteger64`. Displayed as `UnsignedInteger64`.

Accepted aliases for this type: `UnsignedInteger`. These names denote the same type; they do not require a conversion.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function fromInteger(n: Integer) -> Option[UnsignedInteger]
```

Converts from `Integer`. Returns `Option.None` for negative values.

```benitoite
public function toInteger(n: UnsignedInteger) -> Option[Integer]
```

Converts to `Integer`. Returns `Option.None` when out of range.

```benitoite
public function parse(s: String) -> Option[UnsignedInteger]
```

Reads a decimal integer without a minus sign, whitespace, prefixes, underscores, or extra leading zeros. Returns `Option.None` when invalid or out of range.

```benitoite
public function toString(n: UnsignedInteger) -> String
```

Returns the decimal representation.

```benitoite
public function toFloat(n: UnsignedInteger) -> Float
```

Returns the nearest `Float`, rounding to binary64 precision.

```benitoite
public function toDecimal(n: UnsignedInteger) -> Decimal
```

Converts exactly to `Decimal` over the full range of this type.

```benitoite
public function fromDecimal(x: Decimal) -> Option[UnsignedInteger]
```

Converts a mathematically integral `Decimal` in range, including values such as `1.0m`. Returns `Option.None` otherwise.

```benitoite
public function absolute(n: UnsignedInteger) -> UnsignedInteger
```

Returns the original value.

```benitoite
public function minimum(a: UnsignedInteger, b: UnsignedInteger) -> UnsignedInteger
```

Returns the smaller of `a` and `b`.

```benitoite
public function maximum(a: UnsignedInteger, b: UnsignedInteger) -> UnsignedInteger
```

Returns the larger of `a` and `b`.

```benitoite
public function floorDivide(a: UnsignedInteger, b: UnsignedInteger) -> UnsignedInteger
```

Divides and rounds the quotient toward negative infinity. Stops the program when the divisor is 0.

```benitoite
public function floorModulo(a: UnsignedInteger, b: UnsignedInteger) -> UnsignedInteger
```

Returns the remainder. Stops the program when the divisor is 0.

```benitoite
public function bitwiseAnd(a: UnsignedInteger, b: UnsignedInteger) -> UnsignedInteger
```

Calls `UnsignedInteger64.bitwiseAnd` with the same result and stopping conditions.

```benitoite
public function bitwiseOr(a: UnsignedInteger, b: UnsignedInteger) -> UnsignedInteger
```

Calls `UnsignedInteger64.bitwiseOr` with the same result and stopping conditions.

```benitoite
public function bitwiseExclusiveOr(a: UnsignedInteger, b: UnsignedInteger) -> UnsignedInteger
```

Calls `UnsignedInteger64.bitwiseExclusiveOr` with the same result and stopping conditions.

```benitoite
public function bitwiseNot(a: UnsignedInteger) -> UnsignedInteger
```

Calls `UnsignedInteger64.bitwiseNot` with the same result and stopping conditions.

```benitoite
public function shiftLeft(a: UnsignedInteger, n: Integer) -> UnsignedInteger
```

Calls `UnsignedInteger64.shiftLeft` with the same result and stopping conditions.

```benitoite
public function shiftRight(a: UnsignedInteger, n: Integer) -> UnsignedInteger
```

Calls `UnsignedInteger64.shiftRight` with the same result and stopping conditions.
