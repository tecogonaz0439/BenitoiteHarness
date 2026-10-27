# Benitoite.Hash

Status: unofficial. Import it with `import Benitoite.Unofficial.Hash`. When the module becomes standard, the import name changes to `import Benitoite.Hash`.

Refer to its declarations as `Hash.<name>`, for example `Hash.sha256`.

Hash values of bytes.

```benitoite
public function sha256(b: Bytes) -> Bytes
```

Returns the SHA-256 hash of `b` (32 bytes).
