# Benitoite.Trait

Status: standard. Import it with `import Benitoite.Trait`.

Refer to its declarations as `Trait.<name>`, for example `Trait.Ordering`.

Standard type classes. Import `Benitoite.Trait` and write `Trait.Show.show(x)`.

```benitoite
public data Ordering
  Less
  Equal
  Greater
end data
```

The result of comparing two values.

```benitoite
public trait Show[T]
  /// Returns the text of `x`.
  function show(x: T) -> String
end trait
```

Values that can be shown as text close to how they are written in code.

```benitoite
public trait Order[T]
  /// Compares `x` with `y`.
  function compare(x: T, y: T) -> Ordering
end trait
```

Values with a total order.

```benitoite
public trait Semigroup[T]
  /// Combines `x` and `y`.
  function combine(x: T, y: T) -> T
end trait
```

Values that can be combined.

```benitoite
public trait Monoid[T: Semigroup]
  /// Returns the empty value.
  function empty() -> T
end trait
```

Values that can be combined and have an empty value.

```benitoite
public trait Functor[F[_]]
  /// Applies `f` to each value inside `x`.
  function map[A, B, effect E](x: F[A], f: function(value: A) -> B uses E) -> F[B] uses E
end trait
```

Containers whose values can be transformed.

```benitoite
public trait Applicative[F[_]: Functor]
  /// Wraps `x`.
  function pure[A](x: A) -> F[A]
  /// Applies the functions in `fs` to the values in `x`.
  function apply[A, B, effect E](fs: F[function(value: A) -> B uses E], x: F[A]) -> F[B] uses E
end trait
```

Functors that can wrap a value and apply wrapped functions.

```benitoite
public trait Monad[F[_]: Applicative]
  /// Applies `f` to each value inside `x` and flattens the results.
  function flatMap[A, B, effect E](x: F[A], f: function(value: A) -> F[B] uses E) -> F[B] uses E
end trait
```

Applicatives whose computations can depend on earlier values.

```benitoite
public trait Functor2[F[_, _]]
  /// Applies `f` to the successful value inside `x`, keeping its second type.
  function map[A, B, X, effect E](x: F[A, X], f: function(value: A) -> B uses E) -> F[B, X] uses E
end trait
```

Two-parameter containers whose first value can be transformed while keeping the second type.

```benitoite
public trait Applicative2[F[_, _]: Functor2]
  /// Wraps `x`. Annotate the result when its second type cannot be inferred.
  function pure[A, X](x: A) -> F[A, X]
  /// Applies the wrapped function to `x`; the Result implementation prefers an error in `fs`.
  function apply[A, B, X, effect E](fs: F[(function(value: A) -> B uses E), X], x: F[A, X]) -> F[B, X] uses E
end trait
```

Two-parameter functors that can wrap a value and apply a wrapped function.

```benitoite
public trait Monad2[F[_, _]: Applicative2]
  /// Applies `f` to the successful value inside `x`, keeping its second type.
  function flatMap[A, B, X, effect E](x: F[A, X], f: function(value: A) -> F[B, X] uses E) -> F[B, X] uses E
end trait
```

Two-parameter applicatives whose computations can depend on earlier values.

```benitoite
public trait Foldable[F[_]]
  /// Combines the values from the first with `f`, starting from `initial`.
  function fold[A, B, effect E](x: F[A], initial: B, f: function(acc: B, value: A) -> B uses E) -> B uses E
end trait
```

Containers whose values can be combined in order.

```benitoite
public trait Traversable[F[_]: Functor & Foldable]
  /// Applies `f` to each value in order and collects the results inside `G`.
  function traverse[G[_]: Applicative, A, B, effect E](x: F[A], f: function(value: A) -> G[B] uses E) -> G[F[B]] uses E
end trait
```

Containers that can be walked through with an applicative effect.
