# Benitoite.Test.Property

Status: unofficial. Import it with `import Benitoite.Unofficial.Test.Property`. When the module becomes standard, the import name changes to `import Benitoite.Test.Property`.

Refer to its declarations as `Property.<name>`, for example `Property.Gen`.

Pure property trials with explicit random state and bounded shrinking.
Budgets count candidate trials and individual generation/continuation calls.
Callback time and allocation still depend on the input and callback implementation.

```benitoite test
import Benitoite.Unofficial.Test.Property
@test
function propertyExample() -> Unit uses Assert.Check
  bind config <- Property.Config(seed: 42, trials: 3, size: 4, maxShrinks: 10, maxShrinkWork: 20)
  bind result <- Property.check(config: config, gen: Property.constant(7), shrinker: Property.shrinkInteger(), property: lambda(value)
    return if value = 7 then Result.Ok(()) else Result.Error("expected seven") end if
  end lambda)
  match result with
    case Result.Ok(_) -> return ()
    case Result.Error(_) -> Assert.fail("property failed")
  end match
end function
```

```benitoite
public record Gen[T]
  run: function(generator: Random.Generator, size: Integer) -> Pair[T, Random.Generator]
end record
```

A pure generator. Direct calls interpret size according to the stored function.

```benitoite
public record Shrinker[T]
  candidates: function(value: T) -> ShrinkStep[T]
end record
```

Ordered steps; check does not require equality on T.

```benitoite
public data ShrinkStep[T]
  Done
  Yield(T, function() -> ShrinkStep[T])
  Skip(function() -> ShrinkStep[T])
end data
```

One pure shrinking step. Continuations are called only when the consumer advances.

```benitoite
public function mapSteps[A, B](steps: ShrinkStep[A], f: function(value: A) -> B) -> ShrinkStep[B]
```

Maps yielded values, preserving Skip and deferring every continuation.

```benitoite
public function appendSteps[T](left: ShrinkStep[T], right: function() -> ShrinkStep[T]) -> ShrinkStep[T]
```

Appends a deferred sequence; reaching left's Done returns a Skip before calling right.

```benitoite
public record Config
  seed: Integer
  trials: Integer
  size: Integer
  maxShrinks: Integer
  maxShrinkWork: Integer
end record
```

Inclusive budgets: trials 1..10000, size 0..10000, maxShrinks 0..100000, maxShrinkWork 0..1000000.

```benitoite
public record Counterexample[T]
  config: Config
  trial: Integer
  original: T
  reduced: T
  originalMessage: String
  message: String
  shrinks: Integer
  shrinkWork: Integer
  limitReached: Boolean
  libraryVersion: String
end record
```

A failing input and its replay information. reduced is not necessarily minimal.

```benitoite
public data CheckFailure[T]
  InvalidConfig(String)
  Falsified(Counterexample[T])
end data
```

Configuration errors never call the generator, shrinker, or property.

```benitoite
public function constant[T](value: T) -> Gen[T]
```

Returns a value without advancing the generator.

```benitoite
public function map[A, B](gen: Gen[A], f: function(value: A) -> B) -> Gen[B]
```

Transforms generated values without inventing an inverse shrinker.

```benitoite
public function both[A, B](left: Gen[A], right: Gen[B]) -> Gen[Pair[A, B]]
```

Generates left then right, passing the next random state and the same size.

```benitoite
public function integer(low: Integer, high: Integer) -> Result[Gen[Integer], String]
```

Inclusive bounds, with deliberate bias to zero and endpoints; not uniform.

```benitoite
public function boolean() -> Gen[Boolean]
```

Generates false or true from a single 0/1 draw.

```benitoite
public function float() -> Gen[Float]
```

Chooses 0, 1, -1, or a finite nextFloat-derived value in [-1, 1).

```benitoite
public function string() -> Gen[String]
```

Generates valid Unicode scalars, including non-ASCII, with length 0..size. Direct run requires size in 0..10000.

```benitoite
public function list[T](element: Gen[T]) -> Gen[List[T]]
```

Generates 0..size elements, passing the same size to each element generator. Direct run requires size in 0..10000.

```benitoite
public function option[T](element: Gen[T]) -> Gen[Option[T]]
```

Chooses None with probability 1/4, otherwise generating Some.

```benitoite
public function stringMap[T](value: Gen[T]) -> Gen[Map[String, T]]
```

Makes 0..size key/value draws; duplicate keys keep the first value, without retry. Direct run requires size in 0..10000.

```benitoite
public function shrinkInteger() -> Shrinker[Integer]
```

Tries zero, then successive halves toward zero, without negating the input.

```benitoite
public function shrinkFloat() -> Shrinker[Float]
```

Tries zero and a finite half; zero has no candidates. Non-finite inputs try zero.

```benitoite
public function shrinkBoolean() -> Shrinker[Boolean]
```

Shrinks true to false; false has no candidates.

```benitoite
public function shrinkOption[T: equality](element: Shrinker[T]) -> Shrinker[Option[T]]
```

Shrinks to None first, then shrinks the contained value; removes duplicates.

```benitoite
public function shrinkList[T: equality](element: Shrinker[T]) -> Shrinker[List[T]]
```

Tries empty, contiguous deletions of halving widths, then one element at a time.

```benitoite
public function shrinkString() -> Shrinker[String]
```

Shrinks by Unicode scalar deletions and scalar values, never UTF-8 byte fragments.

```benitoite
public function shrinkStringMap[T: equality](value: Shrinker[T]) -> Shrinker[Map[String, T]]
```

Deletes keys in key order, then shrinks one value at a time in key order.

```benitoite
public function check[T](config: Config, gen: Gen[T], shrinker: Shrinker[T], property: function(value: T) -> Result[Unit, String]) -> Result[Unit, CheckFailure[T]]
```

Runs pure trials and shrinks only Result.Error; runtime stops propagate normally.

```benitoite
public function formatFailure[T](failure: CheckFailure[T], show: function(value: T) -> String) -> String
```

Formats replay budgets, original and reduced inputs and messages using caller-supplied show.
