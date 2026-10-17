# Benitoite.IO.Random

Status: unofficial. Import it with `import Benitoite.Unofficial.IO.Random`. When the module becomes standard, the import name changes to `import Benitoite.IO.Random`.

Refer to its declarations as `Random.<name>`, for example `Random.Generate`.

Random numbers. The type `Generator` is built in.

```benitoite
public effect Generate
  /// Returns an integer from `low` up to, but not including, `high`. Stops the program when `high` is not greater than `low`.
  function integer(low: Integer, high: Integer) -> Integer
  /// Returns a number from 0.0 up to, but not including, 1.0.
  function float() -> Float
  /// Returns `true` or `false`.
  function boolean() -> Boolean
  /// Returns the elements in a random order.
  function shuffle[T](xs: List[T]) -> List[T]
  /// Returns a random element, or `Option.None` when the list is empty.
  function choose[T](xs: List[T]) -> Option[T]
end effect
```

Operations that use the hidden generator, seeded from the operating system at the start of a run.

```benitoite
public function fromSeed(seed: Integer) -> Generator
```

Returns a generator made from `seed`. The same seed gives the same values in every version.

```benitoite
public function nextInteger(generator: Generator, low: Integer, high: Integer) -> Pair[Integer, Generator]
```

Returns an integer from `low` up to, but not including, `high`, and the next generator.

```benitoite
public function nextFloat(generator: Generator) -> Pair[Float, Generator]
```

Returns a number from 0.0 up to, but not including, 1.0, and the next generator.

```benitoite
public function shuffleWith[T](generator: Generator, xs: List[T]) -> Pair[List[T], Generator]
```

Returns the elements in a random order and the next generator.
