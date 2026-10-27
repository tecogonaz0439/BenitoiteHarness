# Benitoite.Bytes

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Bytes.<name>`, for example `Bytes.empty`.

Functions on `Bytes`, an immutable sequence of `Byte` values. The type `Bytes` is built in.

```benitoite
public function empty() -> Bytes
```

Returns the empty byte sequence.

```benitoite
public function fromList(bs: List[Byte]) -> Bytes
```

Returns the bytes of `bs` in order.

```benitoite
public function fromIntegers(ns: List[Integer]) -> Option[Bytes]
```

Returns the bytes of `ns`, or `Option.None` when a value is not between 0 and 255.

```benitoite
public function fromHex(s: String) -> Option[Bytes]
```

Reads two hexadecimal digits per byte. `_` and spaces are skipped.

```benitoite
public function fromBinary(s: String) -> Option[Bytes]
```

Reads eight binary digits per byte. `_` and spaces are skipped.

```benitoite
public function toList(b: Bytes) -> List[Byte]
```

Returns the bytes as a list.

```benitoite
public function toHex(b: Bytes) -> String
```

Returns two lowercase hexadecimal digits per byte, without separators.

```benitoite
public function toBinary(b: Bytes) -> String
```

Returns eight binary digits per byte, without separators.

```benitoite
public function length(b: Bytes) -> Integer
```

Returns the number of bytes.

```benitoite
public function get(b: Bytes, i: Integer) -> Option[Byte]
```

Returns the byte at position `i`, or `Option.None` when `i` is out of range.

```benitoite
public function slice(b: Bytes, start: Integer, stop: Integer) -> Option[Bytes]
```

Returns the bytes from position `start` up to `stop`, or `Option.None` when the positions are not valid.

```benitoite
public function concatenate(a: Bytes, b: Bytes) -> Bytes
```

Returns the bytes of `a` followed by the bytes of `b`.

```benitoite
public function readUnsigned(b: Bytes, offset: Integer, count: Integer, order: ByteOrder) -> Option[Integer]
```

Reads `count` bytes from `offset` as an unsigned integer in the order `order`.

```benitoite
public function readSigned(b: Bytes, offset: Integer, count: Integer, order: ByteOrder) -> Option[Integer]
```

Reads `count` bytes from `offset` as a two's complement signed integer in the order `order`.

```benitoite
public function fromUnsigned(n: Integer, count: Integer, order: ByteOrder) -> Option[Bytes]
```

Returns `n` as `count` unsigned bytes, or `Option.None` when it does not fit.

```benitoite
public function fromSigned(n: Integer, count: Integer, order: ByteOrder) -> Option[Bytes]
```

Returns `n` as `count` two's complement bytes, or `Option.None` when it does not fit.
