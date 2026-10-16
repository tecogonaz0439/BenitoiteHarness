# Benitoite.Triple

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Triple.<name>`, for example `Triple.Triple`.

Triples of three values.

```benitoite
public data Triple[A, B, C]
  Triple(A, B, C)
end data
```

A triple of three values, written `Triple(a, b, c)`.

```benitoite
public function first[A, B, C](t: Triple[A, B, C]) -> A
```

Returns the first value.

```benitoite
public function second[A, B, C](t: Triple[A, B, C]) -> B
```

Returns the second value.

```benitoite
public function third[A, B, C](t: Triple[A, B, C]) -> C
```

Returns the third value.
