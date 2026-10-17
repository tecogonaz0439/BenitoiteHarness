# Benitoite.Result

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Result.<name>`, for example `Result.Result`.

Results of operations that may fail.

```benitoite
public data Result[T, E]
  Ok(T)
  Error(E)
end data
```

The result of an operation: `Result.Ok(x)` on success, `Result.Error(e)` on failure.

```benitoite
public function map[T, U, X, effect E](r: Result[T, X], f: function(value: T) -> U uses E) -> Result[U, X] uses E
```

Applies `f` to the value inside `Result.Ok`.

```benitoite
public function mapError[T, X, Y, effect E](r: Result[T, X], f: function(error: X) -> Y uses E) -> Result[T, Y] uses E
```

Applies `f` to the value inside `Result.Error`.

```benitoite
public function andThen[T, U, X, effect E](r: Result[T, X], f: function(value: T) -> Result[U, X] uses E) -> Result[U, X] uses E
```

Applies `f` to the value inside `Result.Ok` and returns its result.

```benitoite
public function unwrapOr[T, X](r: Result[T, X], fallback: T) -> T
```

Returns the value inside `Result.Ok`, or `fallback` for `Result.Error`.

```benitoite
public function isOk[T, X](r: Result[T, X]) -> Boolean
```

Returns whether `r` is `Result.Ok`.

```benitoite
public function isError[T, X](r: Result[T, X]) -> Boolean
```

Returns whether `r` is `Result.Error`.

```benitoite
public function ok[T, X](r: Result[T, X]) -> Option[T]
```

Turns `Result.Ok(x)` into `Option.Some(x)` and `Result.Error` into `Option.None`.
