# Benitoite.Integer

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Integer.<name>`, for example `Integer.toString`.

Functions on `Integer`, the 64-bit signed integer type.

For direct calls to this module's shift functions, local interval analysis reports W0404 only when the entire inferred interval for `n` is outside `[0,63]`.
The warning means that, if execution reaches the call and all arguments are evaluated successfully, the call stops the program at runtime.
Intersecting or unknown intervals and calls without normal argument values do not produce W0404. Runtime range checks remain; no warning does not mean safe.

W0404 applies only to direct arguments of standard shift functions, including the fixed-width and Byte modules and their aliases.
It does not apply to Process.exit, Http.Listen.listen, Http.Listen.respond, Http.Connect.send, or HTTP record fields: language handlers can replace these operations.

## Type

Type: `Integer`. Canonical name: `Integer64`. Displayed as `Integer`.

Accepted aliases for this type: `Integer`. These names denote the same type; they do not require a conversion.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function toString(n: Integer) -> String
```

Returns the decimal representation of `n`, with a leading `-` when negative.

```benitoite
public function parse(s: String) -> Option[Integer]
```

Reads a decimal integer. Returns `Option.None` when `s` is not an integer or is out of range.

```benitoite
public function toFloat(n: Integer) -> Float
```

Returns the `Float` nearest to `n`.

```benitoite
public function floorDivide(a: Integer, b: Integer) -> Integer
```

Divides and rounds the quotient toward negative infinity. Stops the program when `b` is 0.

```benitoite
public function floorModulo(a: Integer, b: Integer) -> Integer
```

Returns the remainder whose sign follows `b`. Stops the program when `b` is 0.

```benitoite
public function absolute(n: Integer) -> Integer
```

Returns the absolute value. Stops the program when `n` is the smallest `Integer`.

```benitoite
public function minimum(a: Integer, b: Integer) -> Integer
```

Returns the smaller of `a` and `b`.

```benitoite
public function maximum(a: Integer, b: Integer) -> Integer
```

Returns the larger of `a` and `b`.

```benitoite
public function bitwiseAnd(a: Integer, b: Integer) -> Integer
```

Returns the bitwise AND.

```benitoite
public function bitwiseOr(a: Integer, b: Integer) -> Integer
```

Returns the bitwise OR.

```benitoite
public function bitwiseExclusiveOr(a: Integer, b: Integer) -> Integer
```

Returns the bitwise exclusive OR.

```benitoite
public function bitwiseNot(a: Integer) -> Integer
```

Inverts all bits.

```benitoite
public function shiftLeft(a: Integer, n: Integer) -> Integer
```

Shifts left by `n` bits, filling the low bits with 0.

```benitoite
public function shiftRight(a: Integer, n: Integer) -> Integer
```

Shifts right by `n` bits, filling the high bits with the sign bit.

```benitoite
public function shiftRightUnsigned(a: Integer, n: Integer) -> Integer
```

Shifts right by `n` bits, filling the high bits with 0.
