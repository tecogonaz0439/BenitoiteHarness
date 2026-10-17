# Benitoite.Encoding

Status: unofficial. Import it with `import Benitoite.Unofficial.Encoding`. When the module becomes standard, the import name changes to `import Benitoite.Encoding`.

Refer to its declarations as `Encoding.<name>`, for example `Encoding.base64Encode`.

Base64 encoding of bytes.

```benitoite
public function base64Encode(b: Bytes) -> String
```

Encodes `b` with the standard alphabet and `=` padding.

```benitoite
public function base64Decode(text: String) -> Result[Bytes, String]
```

Decodes standard Base64 with `=` padding. Spaces and line breaks are errors.

```benitoite
public function base64UrlEncode(b: Bytes) -> String
```

Encodes `b` with the URL and file name safe alphabet, without padding.

```benitoite
public function base64UrlDecode(text: String) -> Result[Bytes, String]
```

Decodes URL and file name safe Base64 without padding.
