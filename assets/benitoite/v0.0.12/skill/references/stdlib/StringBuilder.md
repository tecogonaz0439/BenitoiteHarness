# Benitoite.StringBuilder

Status: standard. Import it with `import Benitoite.StringBuilder`.

Function-local string construction. Import Benitoite.StringBuilder explicitly.
Declare with bind b: StringBuilder <- StringBuilder.new(). Local operations add no effects; effects from ordinary arguments are preserved.
A builder is a second-class value: it cannot be stored in an ordinary variable,
returned from a function, or captured by a lambda or lazy expression.
Its kind name is not an ordinary type; builders cannot be ordinary function arguments.
Use copy to obtain an immutable String before capturing or storing a value.
Builders cannot cross lambda, lazy, partial-application, or handler-clause boundaries.
appendAll accepts previously prepared fragments; callbacks cannot capture the builder.

```text
StringBuilder.new() -> StringBuilder uses State
```

```text
StringBuilder.append(builder: StringBuilder, text: String) -> Unit uses State
```

```text
StringBuilder.appendAll(builder: StringBuilder, texts: List[String]) -> Unit uses State
```

```text
StringBuilder.copy(builder: StringBuilder) -> String uses State
```

```text
StringBuilder.byteLength(builder: StringBuilder) -> Integer uses State
```
