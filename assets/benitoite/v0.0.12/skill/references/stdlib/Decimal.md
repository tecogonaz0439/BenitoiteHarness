# Benitoite.Decimal

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Decimal.<name>`, for example `Decimal.round`.

Functions on `Decimal`, the 128-bit decimal fraction type.

```benitoite
public function round(x: Decimal, places: Integer, mode: RoundingMode) -> Decimal
```

Rounds `x` to `places` digits after the decimal point in the direction `mode`.

```benitoite
public function absolute(x: Decimal) -> Decimal
```

Returns the absolute value, keeping the number of digits after the decimal point.

```benitoite
public function fromInteger(n: Integer) -> Decimal
```

Returns the `Decimal` equal to `n`.

```benitoite
public function truncate(x: Decimal) -> Option[Integer]
```

Rounds toward zero. Returns `Option.None` when the result is out of the `Integer` range.

```benitoite
public function toFloat(x: Decimal) -> Float
```

Returns the `Float` nearest to `x`.

```benitoite
public function fromFloat(x: Float) -> Option[Decimal]
```

Reads the text of `Float.toString(x)` as a `Decimal`. Returns `Option.None` for NaN, infinities, and values out of range.

```benitoite
public function toString(x: Decimal) -> String
```

Returns the text representation of `x`.

```benitoite
public function parse(s: String) -> Option[Decimal]
```

Reads a decimal number. Returns `Option.None` when `s` is not a valid `Decimal`.
