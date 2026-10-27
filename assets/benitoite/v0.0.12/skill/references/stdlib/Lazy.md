# Benitoite.Lazy

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Lazy.<name>`, for example `Lazy.force`.

Explicitly delayed values, made with `lazy ... end lazy`. The type `Lazy` is built in.

```benitoite
public function force[T](value: Lazy[T]) -> T
```

Evaluates the delayed body on the first call and returns the remembered value afterwards.
