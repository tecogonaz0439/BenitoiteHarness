# Benitoite.Option

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Option.<name>`, for example `Option.Option`.

Optional values.

```benitoite test
@test
function optionExample() -> Unit uses Assert.Check
  Assert.equal(actual: Option.map(o: Option.Some(3), f: lambda(value) return value + 1), expected: Option.Some(4))
  Assert.equal(actual: Option.unwrapOr(o: Option.None, fallback: 9), expected: 9)
end function
```

```benitoite
public data Option[T]
  Some(T)
  None
end data
```

A value that may be absent: `Option.Some(x)` holds `x`, and `Option.None` holds nothing.

```benitoite
public function map[T, U, effect E](o: Option[T], f: function(value: T) -> U uses E) -> Option[U] uses E
```

Applies `f` to the value inside `Option.Some`.

```benitoite
public function andThen[T, U, effect E](o: Option[T], f: function(value: T) -> Option[U] uses E) -> Option[U] uses E
```

Applies `f` to the value inside `Option.Some` and returns its result.

```benitoite
public function unwrapOr[T](o: Option[T], fallback: T) -> T
```

Returns the value inside `Option.Some`, or `fallback` for `Option.None`.

```benitoite
public function isSome[T](o: Option[T]) -> Boolean
```

Returns whether `o` is `Option.Some`.

```benitoite
public function isNone[T](o: Option[T]) -> Boolean
```

Returns whether `o` is `Option.None`.

```benitoite
public function okOr[T, X](o: Option[T], error: X) -> Result[T, X]
```

Turns `Option.Some(x)` into `Result.Ok(x)` and `Option.None` into `Result.Error(error)`.
