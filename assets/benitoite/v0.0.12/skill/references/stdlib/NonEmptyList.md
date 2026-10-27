# Benitoite.NonEmptyList

Status: unofficial. Import it with `import Benitoite.Unofficial.NonEmptyList`. When the module becomes standard, the import name changes to `import Benitoite.NonEmptyList`.

Refer to its declarations as `NonEmptyList.<name>`, for example `NonEmptyList.NonEmptyList`.

An opaque nonempty list. Construction checks once; head returns an element and tail may be empty.
The wrapper retains its elements and shares any Reference values they contain. Import this module explicitly.
When the element type supports equality, = compares both stored fields, first and rest, structurally. Assert failure output shows these internal field names and values. Their automatic accessors remain private; use head and tail. The wrapper does not implement Trait.Show.
When the element type is a key type, the wrapper is a key type and can be a Map key or Set element. Keys compare lexicographically by first, then rest. Map.keys and Set.toList return ascending key order.

```benitoite test
import Benitoite.Unofficial.NonEmptyList
@test
function nonEmptyExample() -> Unit uses Assert.Check
  match NonEmptyList.fromList([3, 4]) with
    case Result.Ok(checked) ->
      Assert.equal(actual: NonEmptyList.head(checked), expected: 3)
      Assert.equal(actual: NonEmptyList.tail(checked), expected: [4])
      bind mapped <- NonEmptyList.map(value: checked, f: lambda(value) return Integer.toString(value) end lambda)
      Assert.equal(actual: NonEmptyList.toList(mapped), expected: ["3", "4"])
    case Result.Error(_) -> Assert.fail("nonempty input was rejected")
  end match
end function
```

```benitoite
@opaque
public record NonEmptyList[T]
  first: T
  rest: List[T]
end record
```

A first element and a possibly empty tail. Use fromList and the public projections.

```benitoite
public data EmptyListError
  EmptyList
end data
```

Construction failed because the input list was empty. The input list is not retained.

```benitoite
public function fromList[T](values: List[T]) -> Result[NonEmptyList[T], EmptyListError]
```

Checks whether the list is nonempty. Even a constant empty input returns Result.Error.

```benitoite
public function toList[T](value: NonEmptyList[T]) -> List[T]
```

Returns all elements in the original order, preserving shared Reference elements.

```benitoite
public function head[T](value: NonEmptyList[T]) -> T
```

Returns the first element, without a second check or an Option wrapper.

```benitoite
public function tail[T](value: NonEmptyList[T]) -> List[T]
```

Returns the remaining elements, which may be empty.

```benitoite
public function map[T, U, effect E](value: NonEmptyList[T], f: function(value: T) -> U uses E) -> NonEmptyList[U] uses E
```

Applies f once to each element, from first to last. Effects and failures propagate; a successful result remains nonempty.
