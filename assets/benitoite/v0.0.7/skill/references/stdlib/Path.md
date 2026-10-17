# Benitoite.Path

Status: unofficial. Import it with `import Benitoite.Unofficial.Path`. When the module becomes standard, the import name changes to `import Benitoite.Path`.

Refer to its declarations as `Path.<name>`, for example `Path.join`.

Joining and splitting paths as strings. Nothing here reads the file system.

```benitoite
public function join(base: String, child: String) -> String
```

Returns `child` after `base` with a separator between, or `child` when it is absolute.

```benitoite
public function joinAll(parts: List[String]) -> String
```

Joins `parts` from the first with `Path.join`. Returns "" for an empty list.

```benitoite
public function parent(p: String) -> Option[String]
```

Returns the path without its last component, or `Option.None` for a root or an empty path.

```benitoite
public function fileName(p: String) -> Option[String]
```

Returns the last component, or `Option.None` when it is `..`, a root, or empty.

```benitoite
public function stem(p: String) -> Option[String]
```

Returns the file name without its last extension.

```benitoite
public function extension(p: String) -> Option[String]
```

Returns the extension of the file name without the dot.

```benitoite
public function withExtension(p: String, extension: String) -> String
```

Returns the path with its extension changed to `extension`. An empty `extension` removes it.

```benitoite
public function isAbsolute(p: String) -> Boolean
```

Returns whether the path is absolute.

```benitoite
public function components(p: String) -> List[String]
```

Returns the components, with the root first for an absolute path.

```benitoite
public function normalize(p: String) -> String
```

Removes `.` and cancels `..` with the component before it, without reading the file system.
