# Benitoite.SourceLocation

Status: unofficial. Import it with `import Benitoite.Unofficial.SourceLocation`. When the module becomes standard, the import name changes to `import Benitoite.SourceLocation`.

Refer to its declarations as `SourceLocation.<name>`, for example `SourceLocation.Location`.

Explicit source locations. `here()` is expanded before type checking.

```benitoite
public record Location
  file: String
  line: Integer
  column: Integer
end record
```

A display path and one-based Unicode line and column. User-created locations are allowed.

```benitoite
public function here() -> Location
```

The location of this zero-argument direct call. Cannot be stored, partially applied, or piped.
