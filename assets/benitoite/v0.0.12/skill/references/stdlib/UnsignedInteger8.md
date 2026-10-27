# Benitoite.UnsignedInteger8

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `UnsignedInteger8.<name>`, for example `UnsignedInteger8.fromInteger`.

Functions on `UnsignedInteger8`, the same type as `Byte`, exposed under the canonical module name.

For direct calls to this module's shift functions, local interval analysis reports W0404 only when the entire inferred interval for `n` is outside `[0,7]`.
The warning means that, if execution reaches the call and all arguments are evaluated successfully, the call stops the program at runtime.
Intersecting or unknown intervals and calls without normal argument values do not produce W0404. Runtime range checks remain; no warning does not mean safe.

## Type

Type: `UnsignedInteger8`. Canonical name: `UnsignedInteger8`. Displayed as `Byte`.

Accepted aliases for this type: `Byte`. These names denote the same type; they do not require a conversion.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function fromInteger(n: Integer) -> Option[UnsignedInteger8]
```

Calls `Byte.fromInteger` with the same result and stopping conditions.

```benitoite
public function toInteger(b: UnsignedInteger8) -> Integer
```

Calls `Byte.toInteger` with the same result and stopping conditions.

```benitoite
public function toString(b: UnsignedInteger8) -> String
```

Calls `Byte.toString` with the same result and stopping conditions.

```benitoite
public function bitwiseAnd(a: UnsignedInteger8, b: UnsignedInteger8) -> UnsignedInteger8
```

Calls `Byte.bitwiseAnd` with the same result and stopping conditions.

```benitoite
public function bitwiseOr(a: UnsignedInteger8, b: UnsignedInteger8) -> UnsignedInteger8
```

Calls `Byte.bitwiseOr` with the same result and stopping conditions.

```benitoite
public function bitwiseExclusiveOr(a: UnsignedInteger8, b: UnsignedInteger8) -> UnsignedInteger8
```

Calls `Byte.bitwiseExclusiveOr` with the same result and stopping conditions.

```benitoite
public function bitwiseNot(a: UnsignedInteger8) -> UnsignedInteger8
```

Calls `Byte.bitwiseNot` with the same result and stopping conditions.

```benitoite
public function shiftLeft(a: UnsignedInteger8, n: Integer) -> UnsignedInteger8
```

Calls `Byte.shiftLeft` with the same result and stopping conditions.

```benitoite
public function shiftRight(a: UnsignedInteger8, n: Integer) -> UnsignedInteger8
```

Calls `Byte.shiftRight` with the same result and stopping conditions.
