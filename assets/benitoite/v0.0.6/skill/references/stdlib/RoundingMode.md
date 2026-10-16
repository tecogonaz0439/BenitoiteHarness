# Benitoite.RoundingMode

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `RoundingMode.<name>`, for example `RoundingMode.RoundingMode`.

Rounding directions for `Decimal.round`.

```benitoite
public data RoundingMode
  /// The nearest value; halves go to the value whose last digit is even.
  HalfToEven
  /// The nearest value; halves go away from zero.
  HalfAwayFromZero
  /// Toward zero.
  TowardZero
  /// Toward negative infinity.
  TowardNegativeInfinity
  /// Toward positive infinity.
  TowardPositiveInfinity
end data
```

How `Decimal.round` chooses the result.
