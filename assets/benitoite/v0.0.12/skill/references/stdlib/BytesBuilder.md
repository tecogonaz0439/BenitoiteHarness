# Benitoite.BytesBuilder

Status: standard. Import it with `import Benitoite.BytesBuilder`.

Function-local bytes construction. Import Benitoite.BytesBuilder explicitly.
Declare with bind b: BytesBuilder <- BytesBuilder.new(). Local operations add no effects; effects from ordinary arguments are preserved.
A builder is a second-class value: it cannot be stored in an ordinary variable,
returned from a function, or captured by a lambda or lazy expression.
Its kind name is not an ordinary type; builders cannot be ordinary function arguments.
Use copy to obtain an immutable Bytes before capturing or storing a value.
Builders cannot cross lambda, lazy, partial-application, or handler-clause boundaries.
appendAll accepts previously prepared fragments; callbacks cannot capture the builder.

```text
BytesBuilder.new() -> BytesBuilder uses State
```

```text
BytesBuilder.append(builder: BytesBuilder, value: Byte) -> Unit uses State
```

```text
BytesBuilder.appendAll(builder: BytesBuilder, bytes: Bytes) -> Unit uses State
```

```text
BytesBuilder.copy(builder: BytesBuilder) -> Bytes uses State
```

```text
BytesBuilder.length(builder: BytesBuilder) -> Integer uses State
```
