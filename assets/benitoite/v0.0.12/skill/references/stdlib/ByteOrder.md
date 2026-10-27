# Benitoite.ByteOrder

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `ByteOrder.<name>`, for example `ByteOrder.ByteOrder`.

Byte orders for reading and writing integers in `Bytes`.

```benitoite
public data ByteOrder
  /// The most significant byte first.
  BigEndian
  /// The least significant byte first.
  LittleEndian
end data
```

The order of the bytes of an integer.
