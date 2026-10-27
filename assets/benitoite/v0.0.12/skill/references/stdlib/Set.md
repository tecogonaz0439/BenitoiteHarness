# Benitoite.Set

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Set.<name>`, for example `Set.empty`.

Functions on `Set`, a persistent set ordered by its elements.

```benitoite
public function empty[T: key]() -> Set[T]
```

Returns the empty set.

```benitoite
public function fromList[T: key](xs: List[T]) -> Set[T]
```

Returns the set of the elements of `xs`.

```benitoite
public function toList[T: key](s: Set[T]) -> List[T]
```

Returns the elements in order.

```benitoite
public function contains[T: key](s: Set[T], x: T) -> Boolean
```

Returns whether the set has `x`.

```benitoite
public function add[T: key](s: Set[T], x: T) -> Set[T]
```

Returns the set with `x` added. An existing element keeps its original form.

```benitoite
public function remove[T: key](s: Set[T], x: T) -> Set[T]
```

Returns the set without `x`.

```benitoite
public function size[T: key](s: Set[T]) -> Integer
```

Returns the number of elements.

```benitoite
public function union[T: key](a: Set[T], b: Set[T]) -> Set[T]
```

Returns the elements in `a` or `b`. An element in both sets keeps its form from `a`.

```benitoite
public function intersection[T: key](a: Set[T], b: Set[T]) -> Set[T]
```

Returns the elements in both `a` and `b`. Each element keeps its form from `a`.

```benitoite
public function difference[T: key](a: Set[T], b: Set[T]) -> Set[T]
```

Returns the elements in `a` and not in `b`. Each element keeps its form from `a`.

```benitoite
public function map[T: key, U: key, effect E](s: Set[T], f: function(value: T) -> U uses E) -> Set[U] uses E
```

Returns the set of `f` applied to each element. Equal results become one element.

```benitoite
public function filter[T: key, effect E](s: Set[T], p: function(value: T) -> Boolean uses E) -> Set[T] uses E
```

Returns the elements for which `p` returns `true`.

```benitoite
public function fold[T: key, A, effect E](s: Set[T], initial: A, f: function(acc: A, value: T) -> A uses E) -> A uses E
```

Combines the elements in order with `f`, starting from `initial`.

```benitoite
public function forEach[T: key, effect E](s: Set[T], f: function(value: T) -> Unit uses E) -> Unit uses E
```

Calls `f` with each element in order.
