# Benitoite.List

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `List.<name>`, for example `List.length`.

Functions on `List`. Lists are made with list literals and these functions.

```benitoite test
@test
function listExample() -> Unit uses Assert.Check
  Assert.equal(actual: List.map(xs: [1, 2, 3], f: lambda(value) return value * 2), expected: [2, 4, 6])
end function
```

```benitoite
public function length[T](xs: List[T]) -> Integer
```

Returns the number of elements.

```benitoite
public function isEmpty[T](xs: List[T]) -> Boolean
```

Returns whether the list has no elements.

```benitoite
public function head[T](xs: List[T]) -> Option[T]
```

Returns the first element, or `Option.None` when the list is empty.

```benitoite
public function tail[T](xs: List[T]) -> Option[List[T]]
```

Returns the list without its first element, or `Option.None` when the list is empty.

```benitoite
public function get[T](xs: List[T], i: Integer) -> Option[T]
```

Returns the element at position `i`, counting from 0, or `Option.None` when `i` is out of range.

```benitoite
public function prepend[T](xs: List[T], x: T) -> List[T]
```

Returns the list with `x` added at the front.

```benitoite
public function append[T](xs: List[T], x: T) -> List[T]
```

Returns the list with `x` added at the end.

```benitoite
public function concatenate[T](xs: List[T], ys: List[T]) -> List[T]
```

Returns the elements of `xs` followed by the elements of `ys`.

```benitoite
public function reverse[T](xs: List[T]) -> List[T]
```

Returns the elements in reverse order.

```benitoite
public function take[T](xs: List[T], n: Integer) -> List[T]
```

Returns the first `n` elements.

```benitoite
public function drop[T](xs: List[T], n: Integer) -> List[T]
```

Returns the list without its first `n` elements.

```benitoite
public function range(start: Integer, stop: Integer) -> List[Integer]
```

Returns the integers from `start` up to, but not including, `stop`.

```benitoite
public function contains[T: equality](xs: List[T], x: T) -> Boolean
```

Returns whether some element is equal to `x` by `=`.

```benitoite
public function sort[T: ordered](xs: List[T]) -> List[T]
```

Returns the elements in ascending order by `<`. Equal elements keep their order. NaN goes last.

```benitoite
public function map[T, U, effect E](xs: List[T], f: function(value: T) -> U uses E) -> List[U] uses E
```

Returns the list of `f` applied to each element.

```benitoite
public function filter[T, effect E](xs: List[T], p: function(value: T) -> Boolean uses E) -> List[T] uses E
```

Returns the elements for which `p` returns `true`, in order.

```benitoite
public function fold[T, A, effect E](xs: List[T], initial: A, f: function(acc: A, value: T) -> A uses E) -> A uses E
```

Combines the elements from the first with `f`, starting from `initial`.

```benitoite
public function forEach[T, effect E](xs: List[T], f: function(value: T) -> Unit uses E) -> Unit uses E
```

Calls `f` with each element in order.

```benitoite
public function any[T, effect E](xs: List[T], p: function(value: T) -> Boolean uses E) -> Boolean uses E
```

Returns whether `p` returns `true` for some element. Stops calling `p` at the first `true`.

```benitoite
public function all[T, effect E](xs: List[T], p: function(value: T) -> Boolean uses E) -> Boolean uses E
```

Returns whether `p` returns `true` for every element. Stops calling `p` at the first `false`.

```benitoite
public function find[T, effect E](xs: List[T], p: function(value: T) -> Boolean uses E) -> Option[T] uses E
```

Returns the first element for which `p` returns `true`, or `Option.None` when there is none.
