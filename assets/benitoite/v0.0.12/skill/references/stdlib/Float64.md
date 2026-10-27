# Benitoite.Float64

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Float64.<name>`, for example `Float64.toString`.

Functions on `Float64`, the same type as `Float`, exposed under the canonical module name.

## Type

Type: `Float64`. Canonical name: `Float64`. Displayed as `Float`.

Accepted aliases for this type: `Float`. These names denote the same type; they do not require a conversion.

This is a distinct numeric type. There are no implicit conversions between numeric types.

```benitoite
public function toString(x: Float64) -> String
```

Calls `Float.toString` with the same result and stopping conditions.

```benitoite
public function parse(s: String) -> Option[Float64]
```

Calls `Float.parse` with the same result and stopping conditions.

```benitoite
public function truncate(x: Float64) -> Option[Integer]
```

Calls `Float.truncate` with the same result and stopping conditions.

```benitoite
public function isNaN(x: Float64) -> Boolean
```

Calls `Float.isNaN` with the same result and stopping conditions.

```benitoite
public function absolute(x: Float64) -> Float64
```

Calls `Float.absolute` with the same result and stopping conditions.

```benitoite
public function floor(x: Float64) -> Float64
```

Calls `Float.floor` with the same result and stopping conditions.

```benitoite
public function ceiling(x: Float64) -> Float64
```

Calls `Float.ceiling` with the same result and stopping conditions.

```benitoite
public function round(x: Float64) -> Float64
```

Calls `Float.round` with the same result and stopping conditions.

```benitoite
public function squareRoot(x: Float64) -> Float64
```

Calls `Float.squareRoot` with the same result and stopping conditions.
