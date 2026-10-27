# Benitoite.Map

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Map.<name>`, for example `Map.empty`.

Functions on `Map`, a persistent map ordered by its keys.

```benitoite
public function empty[K: key, V]() -> Map[K, V]
```

Returns the empty map.

```benitoite
public function fromList[K: key, V](pairs: List[Pair[K, V]]) -> Map[K, V]
```

Returns the map of the pairs in `pairs`. A later pair wins when two pairs have the same key.

```benitoite
public function toList[K: key, V](m: Map[K, V]) -> List[Pair[K, V]]
```

Returns the pairs in the order of their keys.

```benitoite
public function get[K: key, V](m: Map[K, V], k: K) -> Option[V]
```

Returns the value for `k`, or `Option.None` when the map does not have it.

```benitoite
public function set[K: key, V](m: Map[K, V], k: K, value: V) -> Map[K, V]
```

Returns the map with the value for `k` set to `value`. An existing key keeps its original form.

```benitoite
public function remove[K: key, V](m: Map[K, V], k: K) -> Map[K, V]
```

Returns the map without `k`.

```benitoite
public function contains[K: key, V](m: Map[K, V], k: K) -> Boolean
```

Returns whether the map has `k`.

```benitoite
public function size[K: key, V](m: Map[K, V]) -> Integer
```

Returns the number of pairs.

```benitoite
public function keys[K: key, V](m: Map[K, V]) -> List[K]
```

Returns the keys in order.

```benitoite
public function values[K: key, V](m: Map[K, V]) -> List[V]
```

Returns the values in the order of their keys.

```benitoite
public function map[K: key, V, U, effect E](m: Map[K, V], f: function(key: K, value: V) -> U uses E) -> Map[K, U] uses E
```

Returns the map whose values are `f` applied to each key and value. The keys stay the same.

```benitoite
public function filter[K: key, V, effect E](m: Map[K, V], p: function(key: K, value: V) -> Boolean uses E) -> Map[K, V] uses E
```

Returns the pairs for which `p` returns `true`.

```benitoite
public function fold[K: key, V, A, effect E](m: Map[K, V], initial: A, f: function(acc: A, key: K, value: V) -> A uses E) -> A uses E
```

Combines the pairs in the order of their keys with `f`, starting from `initial`.

```benitoite
public function forEach[K: key, V, effect E](m: Map[K, V], f: function(key: K, value: V) -> Unit uses E) -> Unit uses E
```

Calls `f` with each key and value in the order of the keys.
