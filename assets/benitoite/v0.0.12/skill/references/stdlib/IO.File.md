# Benitoite.IO.File

Status: unofficial. Import it with `import Benitoite.Unofficial.IO.File`. When the module becomes standard, the import name changes to `import Benitoite.IO.File`.

Refer to its declarations as `File.<name>`, for example `File.Read`.

Reading and writing files. The resource types `Reader` and `Writer` are built in.

```benitoite
public effect Read
  /// Reads the whole file as text. Fails with `IOErrorKind.InvalidUTF8` when the content is not UTF-8.
  function readText(path: String) -> Result[String, IOError]
  /// Opens the file for reading line by line.
  function openReader(path: String) -> Result[Reader, IOError]
  /// Reads one line. Returns `Option.None` at the end of the file.
  function readLine(reader: Reader) -> Result[Option[String], IOError]
  /// Reads the whole file as bytes.
  function readBytes(path: String) -> Result[Bytes, IOError]
  /// Reads the whole file as text and splits it into lines.
  function readLines(path: String) -> Result[List[String], IOError]
  /// Returns whether something exists at the path. Fails when it cannot be checked.
  function exists(path: String) -> Result[Boolean, IOError]
  /// Returns information about the file. A symbolic link itself is described, not its target.
  function info(path: String) -> Result[Info, IOError]
  /// Returns the names in the directory, ordered by their UTF-8 bytes.
  function listDirectory(path: String) -> Result[List[String], IOError]
  /// Returns every file and directory under the directory as relative paths, ordered by their UTF-8 bytes.
  function walk(path: String) -> Result[List[String], IOError]
  /// Returns the absolute path with symbolic links resolved.
  function canonicalize(path: String) -> Result[String, IOError]
  /// Reads at most `maximumBytes` bytes. Returns `Option.None` at the end of the file.
  function readChunk(reader: Reader, maximumBytes: Integer) -> Result[Option[Bytes], IOError]
end effect
```

Reading files.

```benitoite
public effect Write
  /// Creates or replaces the file with `text`.
  function writeText(path: String, text: String) -> Result[Unit, IOError]
  /// Adds `text` to the end of the file, creating it when missing.
  function appendText(path: String, text: String) -> Result[Unit, IOError]
  /// Creates or replaces the file with `content`.
  function writeBytes(path: String, content: Bytes) -> Result[Unit, IOError]
  /// Adds `content` to the end of the file, creating it when missing.
  function appendBytes(path: String, content: Bytes) -> Result[Unit, IOError]
  /// Creates the directory and any missing parents. Does nothing when the directory exists.
  function createDirectory(path: String) -> Result[Unit, IOError]
  /// Removes a file, a symbolic link, or an empty directory.
  function remove(path: String) -> Result[Unit, IOError]
  /// Removes the directory and everything under it. Symbolic links are removed, not followed.
  function removeTree(path: String) -> Result[Unit, IOError]
  /// Moves `from` to `to`.
  function rename(from: String, to: String) -> Result[Unit, IOError]
  /// Opens the file for writing. `WriteMode.Replace` creates or empties it; `WriteMode.Append` adds to its end.
  function openWriter(path: String, mode: WriteMode) -> Result[Writer, IOError]
  /// Writes `text`.
  /// After a write fails, returns the same error without writing anything.
  function write(writer: Writer, text: String) -> Result[Unit, IOError]
  /// Writes `text` and a line feed.
  /// After a write fails, returns the same error without writing anything.
  function writeLine(writer: Writer, text: String) -> Result[Unit, IOError]
  /// Writes `content`.
  /// After a write fails, returns the same error without writing anything.
  function writeChunk(writer: Writer, content: Bytes) -> Result[Unit, IOError]
end effect
```

Writing files.

```benitoite
public function closeReader(reader: Reader) -> Result[Unit, IOError] uses State
```

Closes the reader. A `with` binding closes it in the same way.

```benitoite
public function copy(from: String, to: String) -> Result[Unit, IOError] uses Read, Write
```

Copies the content of the regular file `from` to `to`, replacing `to` when it exists.

```benitoite
public function closeWriter(writer: Writer) -> Result[Unit, IOError] uses State
```

Writes out what was written and closes the writer. A `with` binding closes it in the same way.
After a write fails, closes without writing and returns the same message with `IOErrorKind.Other`.

```benitoite
public record Info
  kind: EntryKind
  size: Integer
  modified: Time.Instant
end record
```

Information about a file, returned by `File.info`.

```benitoite
public data EntryKind
  RegularFile
  Directory
  SymbolicLink
  Other
end data
```

What a path points to.

```benitoite
public data WriteMode
  /// Create the file or empty it.
  Replace
  /// Add to the end of the file, creating it when missing.
  Append
end data
```

How `File.openWriter` opens a file.
