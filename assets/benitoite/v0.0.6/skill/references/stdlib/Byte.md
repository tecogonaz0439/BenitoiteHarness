# Benitoite.Byte

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Byte.<name>`, for example `Byte.fromInteger`.

Functions on `Byte`, the integers from 0 to 255.

## Type

Type: `Byte`. Canonical name: `UnsignedInteger8`. Displayed as `Byte`.

Accepted aliases for this type: `Byte`. These names denote the same type; they do not require a conversion.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function fromInteger(n: Integer) -> Option[Byte]
```

Returns the `Byte` equal to `n`, or `Option.None` when `n` is not between 0 and 255.

```benitoite
public function toInteger(b: Byte) -> Integer
```

Returns the value of `b` as an `Integer`.

```benitoite
public function toString(b: Byte) -> String
```

Returns the decimal representation of `b`.

```benitoite
public function bitwiseAnd(a: Byte, b: Byte) -> Byte
```

Returns the bitwise AND.

```benitoite
public function bitwiseOr(a: Byte, b: Byte) -> Byte
```

Returns the bitwise OR.

```benitoite
public function bitwiseExclusiveOr(a: Byte, b: Byte) -> Byte
```

Returns the bitwise exclusive OR.

```benitoite
public function bitwiseNot(a: Byte) -> Byte
```

Inverts all 8 bits.

```benitoite
public function shiftLeft(a: Byte, n: Integer) -> Byte
```

Shifts left by `n` bits, dropping the bits that leave the 8 bits.

```benitoite
public function shiftRight(a: Byte, n: Integer) -> Byte
```

Shifts right by `n` bits, filling the high bits with 0.
