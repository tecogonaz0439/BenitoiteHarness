# Benitoite standard library

The modules of the standard library. Each module has its own file in this directory. Unofficial modules may change; their import name changes when they become standard.

IOError, NetworkError, Json.ParseError, and Csv.ParseError implement Trait.Show by returning their message string unchanged, without added quotes or locations. The first two implementations live in Trait; the ParseError implementations live in Json and Csv. Json.Codec.ConversionError is an extensible conversion failure; its Show displays the relative path and expected/found kinds, without automatically including the input payload or reason. Codec conversions are pure Results. A Result error used with `or die` requires Show; `or return` and bare try do not. Option's die cause is fixed and needs no Show. Explicit Trait.Show.show calls require `import Benitoite.Trait`; die connects its standard references internally without adding names to the user's scope.

Parameter names in these declarations are part of the function contract. Label every argument of an ordinary call with two or more parameters, including calls through function values. Callback names and their order must match the nested function type, even for one parameter. For example, List.map expects `value`, List.fold expects `acc, value`, and Result.mapError expects `error`. An existing function with different names needs an explicit lambda wrapper. See [Idioms](../idioms.md#calls-callback-contracts-holes-and-pipes) for examples.

| Module | Status | Import | Description |
|---|---|---|---|
| [Integer](Integer.md) | standard (prelude) | not needed | Functions on `Integer`, the 64-bit signed integer type. |
| [Float](Float.md) | standard (prelude) | not needed | Functions on `Float`, the IEEE 754 double-precision type. |
| [Decimal](Decimal.md) | standard (prelude) | not needed | Functions on `Decimal`, the 128-bit decimal fraction type. |
| [RoundingMode](RoundingMode.md) | standard (prelude) | not needed | Rounding directions for `Decimal.round`. |
| [Byte](Byte.md) | standard (prelude) | not needed | Functions on `Byte`, the integers from 0 to 255. |
| [Character](Character.md) | standard (prelude) | not needed | Functions on `Character`, a Unicode scalar value. |
| [String](String.md) | standard (prelude) | not needed | Functions on `String`, a sequence of Unicode scalar values. |
| [Boolean](Boolean.md) | standard (prelude) | not needed | The type `Boolean` with the values `true` and `false`. |
| [List](List.md) | standard (prelude) | not needed | Functions on `List`. |
| [Map](Map.md) | standard (prelude) | not needed | Functions on `Map`, a persistent map ordered by its keys. |
| [Set](Set.md) | standard (prelude) | not needed | Functions on `Set`, a persistent set ordered by its elements. |
| [Option](Option.md) | standard (prelude) | not needed | Optional values. |
| [Result](Result.md) | standard (prelude) | not needed | Results of operations that may fail. |
| [Pair](Pair.md) | standard (prelude) | not needed | Pairs of two values. |
| [Triple](Triple.md) | standard (prelude) | not needed | Triples of three values. |
| [IOError](IOError.md) | standard (prelude) | not needed | Failures of IO operations. |
| [IOErrorKind](IOErrorKind.md) | standard (prelude) | not needed | Kinds of IO failures. |
| [Reference](Reference.md) | standard (prelude) | not needed | Mutable cells. |
| [Lazy](Lazy.md) | standard (prelude) | not needed | Explicitly delayed values, made with `lazy ... end lazy`. |
| [Task](Task.md) | standard (prelude) | not needed | Running tasks concurrently and waiting for their results. |
| [TaskGroup](TaskGroup.md) | standard (prelude) | not needed | Groups of tasks started one by one. |
| [IO](IO.md) | standard (prelude) | not needed | The effect `IO.All`, which stands for all the effects of the modules under `Benitoite.IO`. |
| [IO.Clock](IO.Clock.md) | unofficial | `import Benitoite.Unofficial.IO.Clock` | Reading the clock and waiting for time to pass. |
| [IO.Console](IO.Console.md) | unofficial | `import Benitoite.Unofficial.IO.Console` | Standard input, standard output, and standard error. |
| [IO.File](IO.File.md) | unofficial | `import Benitoite.Unofficial.IO.File` | Reading and writing files. |
| [IO.Process](IO.Process.md) | unofficial | `import Benitoite.Unofficial.IO.Process` | The running process: its arguments and its exit. |
| [Trait](Trait.md) | standard | `import Benitoite.Trait` | Standard type classes. |
| [Assert](Assert.md) | standard (prelude) | not needed | Checks of expected values in tests. |
| [Bytes](Bytes.md) | standard (prelude) | not needed | Functions on `Bytes`, an immutable sequence of `Byte` values. |
| [ByteOrder](ByteOrder.md) | standard (prelude) | not needed | Byte orders for reading and writing integers in `Bytes`. |
| [NetworkError](NetworkError.md) | standard (prelude) | not needed | Failures of network operations. |
| [NetworkErrorKind](NetworkErrorKind.md) | standard (prelude) | not needed | Kinds of network failures. |
| [Time](Time.md) | unofficial | `import Benitoite.Unofficial.Time` | Points in time and calendar dates with a fixed offset from UTC. |
| [IO.Random](IO.Random.md) | unofficial | `import Benitoite.Unofficial.IO.Random` | Random numbers. |
| [Path](Path.md) | unofficial | `import Benitoite.Unofficial.Path` | Joining and splitting paths as strings. |
| [Json](Json.md) | unofficial | `import Benitoite.Unofficial.Json` | JSON values, parsing, and writing. |
| [Regex](Regex.md) | unofficial | `import Benitoite.Unofficial.Regex` | Regular expressions. |
| [Csv](Csv.md) | unofficial | `import Benitoite.Unofficial.Csv` | Reading and writing CSV as in RFC 4180. |
| [Encoding](Encoding.md) | unofficial | `import Benitoite.Unofficial.Encoding` | Base64 encoding of bytes. |
| [Hash](Hash.md) | unofficial | `import Benitoite.Unofficial.Hash` | Hash values of bytes. |
| [Network.Http](Network.Http.md) | unofficial | `import Benitoite.Unofficial.Network.Http` | HTTP/1.1 servers without TLS and clients for http and https URLs. |
| [Integer8](Integer8.md) | standard (prelude) | not needed | Functions on `Integer8`, the integers from -128 to 127. |
| [Integer16](Integer16.md) | standard (prelude) | not needed | Functions on `Integer16`, the integers from -32768 to 32767. |
| [Integer32](Integer32.md) | standard (prelude) | not needed | Functions on `Integer32`, the integers from -2147483648 to 2147483647. |
| [Integer64](Integer64.md) | standard (prelude) | not needed | Functions on `Integer64`, the same type as `Integer`, exposed under the canonical module name. |
| [UnsignedInteger8](UnsignedInteger8.md) | standard (prelude) | not needed | Functions on `UnsignedInteger8`, the same type as `Byte`, exposed under the canonical module name. |
| [UnsignedInteger16](UnsignedInteger16.md) | standard (prelude) | not needed | Functions on `UnsignedInteger16`, the integers from 0 to 65535. |
| [UnsignedInteger32](UnsignedInteger32.md) | standard (prelude) | not needed | Functions on `UnsignedInteger32`, the integers from 0 to 4294967295. |
| [UnsignedInteger64](UnsignedInteger64.md) | standard (prelude) | not needed | Conversions and functions over the full range of `UnsignedInteger64`. |
| [UnsignedInteger](UnsignedInteger.md) | standard (prelude) | not needed | Functions on `UnsignedInteger`, an alias of `UnsignedInteger64`. |
| [Float32](Float32.md) | standard (prelude) | not needed | Conversions and functions on `Float32`, the IEEE 754 single-precision type. |
| [Float64](Float64.md) | standard (prelude) | not needed | Functions on `Float64`, the same type as `Float`, exposed under the canonical module name. |
| [Failure](Failure.md) | unofficial | `import Benitoite.Unofficial.Failure` | Typed, non-resumable failures. |
| [Json.Codec](Json.Codec.md) | unofficial | `import Benitoite.Unofficial.Json.Codec` | Pure, explicit JSON conversions. |
| [Test.Property](Test.Property.md) | unofficial | `import Benitoite.Unofficial.Test.Property` | Pure property trials with explicit random state and bounded shrinking. |
| [Scope](Scope.md) | unofficial | `import Benitoite.Unofficial.Scope` | Checks host and absolute-path spelling against an explicit scope. |
| [Range](Range.md) | unofficial | `import Benitoite.Unofficial.Range` | Pure inclusive Integer range checks. |
| [NonEmptyList](NonEmptyList.md) | unofficial | `import Benitoite.Unofficial.NonEmptyList` | An opaque nonempty list. |
| [StringBuilder](StringBuilder.md) | standard | `import Benitoite.StringBuilder` | Function-local string construction. |
| [ListBuilder](ListBuilder.md) | standard | `import Benitoite.ListBuilder` | Function-local list construction. |
| [BytesBuilder](BytesBuilder.md) | standard | `import Benitoite.BytesBuilder` | Function-local bytes construction. |
| [SourceLocation](SourceLocation.md) | unofficial | `import Benitoite.Unofficial.SourceLocation` | Explicit source locations. |
| [Log](Log.md) | unofficial | `import Benitoite.Unofficial.Log` | Explicit structured logging. |
