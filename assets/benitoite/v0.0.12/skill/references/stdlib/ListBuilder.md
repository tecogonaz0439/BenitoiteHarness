# Benitoite.ListBuilder

Status: standard. Import it with `import Benitoite.ListBuilder`.

Function-local list construction. Import Benitoite.ListBuilder explicitly.
Declare with bind b: ListBuilder[T] <- ListBuilder.new(). Local operations add no effects; effects from ordinary arguments are preserved.
A builder is a second-class value: it cannot be stored in an ordinary variable,
returned from a function, or captured by a lambda or lazy expression.
Its kind name is not an ordinary type; builders cannot be ordinary function arguments.
Use copy to obtain an immutable List before capturing or storing a value.
Builders cannot cross lambda, lazy, partial-application, or handler-clause boundaries.
appendAll accepts previously prepared fragments; callbacks cannot capture the builder.

```text
ListBuilder.new() -> ListBuilder[T] uses State
```

```text
ListBuilder.append(builder: ListBuilder[T], value: T) -> Unit uses State
```

```text
ListBuilder.appendAll(builder: ListBuilder[T], values: List[T]) -> Unit uses State
```

```text
ListBuilder.copy(builder: ListBuilder[T]) -> List[T] uses State
```

```text
ListBuilder.length(builder: ListBuilder[T]) -> Integer uses State
```
