# Benitoite.NetworkError

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `NetworkError.<name>`, for example `NetworkError.kind`.

Failures of network operations. The type `NetworkError` is built in.

```benitoite
public function kind(e: NetworkError) -> NetworkErrorKind
```

Returns the kind of the failure.

```benitoite
public function message(e: NetworkError) -> String
```

Returns a human-readable description of the failure. The wording may change between versions.
