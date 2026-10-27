# Benitoite.Pair

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Pair.<name>`, for example `Pair.Pair`.

Pairs of two values.

```benitoite
public data Pair[A, B]
  Pair(A, B)
end data
```

A pair of two values, written `Pair(a, b)`.

```benitoite
public function first[A, B](p: Pair[A, B]) -> A
```

Returns the first value.

```benitoite
public function second[A, B](p: Pair[A, B]) -> B
```

Returns the second value.
