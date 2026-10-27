# Benitoite.NetworkErrorKind

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `NetworkErrorKind.<name>`, for example `NetworkErrorKind.NetworkErrorKind`.

Kinds of network failures.

```benitoite
@extensible
public data NetworkErrorKind
  /// The name cannot be resolved.
  HostNotFound
  /// The connection was refused.
  ConnectionRefused
  /// The connection was cut in the middle.
  ConnectionReset
  /// The time ran out.
  TimedOut
  /// The address and port to listen on are in use.
  AddressInUse
  /// What was received is not valid HTTP.
  InvalidHTTPData
  /// A URL or an address is not in a valid form.
  InvalidInput
  /// The operating system refused the operation.
  PermissionDenied
  /// None of the above, including a failed TLS certificate check.
  Other
end data
```

The kind of a network failure, returned by `NetworkError.kind`.
