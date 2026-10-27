# Benitoite.Reference

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Reference.<name>`, for example `Reference.new`.

Mutable cells. The type `Reference` is built in.

```benitoite
public function new[T](value: T) -> Reference[T] uses State
```

Returns a new cell holding `value`.

```benitoite
public function get[T](reference: Reference[T]) -> T uses State
```

Returns the value in the cell.

```benitoite
public function set[T](reference: Reference[T], value: T) -> Unit uses State
```

Replaces the value in the cell with `value`.

```benitoite
public function update[T](reference: Reference[T], f: function(value: T) -> T) -> Unit uses State
```

Replaces the value in the cell with `f` applied to it. No other task touches the cell in between.
