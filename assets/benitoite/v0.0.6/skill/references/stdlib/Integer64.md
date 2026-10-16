# Benitoite.Integer64

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Integer64.<name>`, for example `Integer64.toString`.

Functions on `Integer64`, the same type as `Integer`, exposed under the canonical module name.

## Type

Type: `Integer64`. Canonical name: `Integer64`. Displayed as `Integer`.

Accepted aliases for this type: `Integer`. These names denote the same type; they do not require a conversion.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function toString(n: Integer64) -> String
```

Calls `Integer.toString` with the same result and stopping conditions.

```benitoite
public function parse(s: String) -> Option[Integer64]
```

Calls `Integer.parse` with the same result and stopping conditions.

```benitoite
public function toFloat(n: Integer64) -> Float
```

Calls `Integer.toFloat` with the same result and stopping conditions.

```benitoite
public function floorDivide(a: Integer64, b: Integer64) -> Integer64
```

Calls `Integer.floorDivide` with the same result and stopping conditions.

```benitoite
public function floorModulo(a: Integer64, b: Integer64) -> Integer64
```

Calls `Integer.floorModulo` with the same result and stopping conditions.

```benitoite
public function absolute(n: Integer64) -> Integer64
```

Calls `Integer.absolute` with the same result and stopping conditions.

```benitoite
public function minimum(a: Integer64, b: Integer64) -> Integer64
```

Calls `Integer.minimum` with the same result and stopping conditions.

```benitoite
public function maximum(a: Integer64, b: Integer64) -> Integer64
```

Calls `Integer.maximum` with the same result and stopping conditions.

```benitoite
public function bitwiseAnd(a: Integer64, b: Integer64) -> Integer64
```

Calls `Integer.bitwiseAnd` with the same result and stopping conditions.

```benitoite
public function bitwiseOr(a: Integer64, b: Integer64) -> Integer64
```

Calls `Integer.bitwiseOr` with the same result and stopping conditions.

```benitoite
public function bitwiseExclusiveOr(a: Integer64, b: Integer64) -> Integer64
```

Calls `Integer.bitwiseExclusiveOr` with the same result and stopping conditions.

```benitoite
public function bitwiseNot(a: Integer64) -> Integer64
```

Calls `Integer.bitwiseNot` with the same result and stopping conditions.

```benitoite
public function shiftLeft(a: Integer64, n: Integer) -> Integer64
```

Calls `Integer.shiftLeft` with the same result and stopping conditions.

```benitoite
public function shiftRight(a: Integer64, n: Integer) -> Integer64
```

Calls `Integer.shiftRight` with the same result and stopping conditions.

```benitoite
public function shiftRightUnsigned(a: Integer64, n: Integer) -> Integer64
```

Calls `Integer.shiftRightUnsigned` with the same result and stopping conditions.
