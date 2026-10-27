# Benitoite.IOErrorKind

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `IOErrorKind.<name>`, for example `IOErrorKind.IOErrorKind`.

Kinds of IO failures.

```benitoite
@extensible
public data IOErrorKind
  /// The path or the command does not exist.
  NotFound
  /// The operating system refused the operation.
  PermissionDenied
  /// The thing to create already exists.
  AlreadyExists
  /// A directory was given where a file is needed.
  IsDirectory
  /// Something other than a directory was given where a directory is needed.
  NotDirectory
  /// A non-empty directory was given where an empty one is needed.
  DirectoryNotEmpty
  /// The content read is not valid UTF-8.
  InvalidUTF8
  /// An argument cannot be passed to the operating system, such as a path containing NUL.
  InvalidInput
  /// None of the above.
  Other
end data
```

The kind of an IO failure, returned by `IOError.kind`.
