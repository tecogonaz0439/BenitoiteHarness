# Benitoite.IOError

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `IOError.<name>`, for example `IOError.message`.

Failures of IO operations. The type `IOError` is built in.

```benitoite
public function message(e: IOError) -> String
```

Returns a human-readable description of the failure. The wording may change between versions.

```benitoite
public function kind(e: IOError) -> IOErrorKind
```

Returns the kind of the failure.
