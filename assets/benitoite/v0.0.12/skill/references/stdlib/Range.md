# Benitoite.Range

Status: unofficial. Import it with `import Benitoite.Unofficial.Range`. When the module becomes standard, the import name changes to `import Benitoite.Range`.

Refer to its declarations as `Range.<name>`, for example `Range.CheckedInteger`.

Pure inclusive Integer range checks. Success retains both bounds and the value; bounds may differ between values of the same CheckedInteger type.

Local interval analysis recognizes direct calls to the standard checkInteger and value functions, including import aliases. With fixed Integer64 bounds, it can retain the bounds through a direct match or one local binding of the Result. In a single Ok(checked) pattern, a direct Range.value(checked) can supply that interval to later arithmetic.
The analysis does not carry this information through aliases, repackaging, value joins, function boundaries, or dynamic bounds. A join of condition environments also forgets it, including information from outside the condition. It adds no warning for Range errors and keeps all runtime checks. This analysis does not extend to CheckedLength or CheckedIndex.

```benitoite test
import Benitoite.Unofficial.Range
@test
function rangeExample() -> Unit uses Assert.Check
  match Range.checkInteger(minimum: 1, maximum: 10, value: 10) with
    case Result.Ok(value) ->
      Assert.equal(actual: Range.minimum(value), expected: 1)
      Assert.equal(actual: Range.maximum(value), expected: 10)
      Assert.equal(actual: Range.value(value), expected: 10)
    case Result.Error(_) -> Assert.fail("inclusive bound did not match")
  end match
  match Range.checkInteger(minimum: 10, maximum: 1, value: 0) with
    case Result.Error(Range.RangeError.InvalidBounds(low, high)) ->
      Assert.equal(actual: low, expected: 10)
      Assert.equal(actual: high, expected: 1)
    case _ -> Assert.fail("reversed bounds must take priority")
  end match
end function
```

Invalid constant inputs are still Result.Error values, so this checks successfully.

```benitoite check
import Benitoite.Unofficial.Range
function main() -> Unit
  bind result <- Range.checkInteger(minimum: 1, maximum: 10, value: 11)
  return ()
end function
```

```benitoite error
import Benitoite.Unofficial.Range
function main() -> Unit
  bind value <- Range.CheckedInteger(minimum: 1, maximum: 10, value: 5)
  return ()
end function
```
```diagnostic
E0340
```

Checked lengths and indices retain their original immutable list. In particular, get takes only a checked index; a different list must be checked again. Reference elements remain shared.
When the element type supports equality, = compares every stored field structurally, including the original list: minimum/maximum/length/values for CheckedLength and values/index/element for CheckedIndex. Assert failure output shows these internal field names and values; it does not make their automatic accessors public. These wrappers do not implement Trait.Show.
When the element type is a key type, both wrappers are key types and can be Map keys or Set elements. Keys compare lexicographically in stored field declaration order: minimum, maximum, length, values for CheckedLength; values, index, element for CheckedIndex. Map.keys and Set.toList return ascending key order.

```benitoite test
import Benitoite.Unofficial.Range
@test
function checkedListExample() -> Unit uses Assert.Check
  match Range.checkLength(minimum: 2, maximum: 2, values: [10, 20]) with
    case Result.Ok(value) ->
      Assert.equal(actual: Range.checkedLength(value), expected: 2)
      Assert.equal(actual: Range.lengthValues(value), expected: [10, 20])
    case Result.Error(_) -> Assert.fail("exact length was rejected")
  end match
  match Range.checkIndex(index: 1, values: [10, 20]) with
    case Result.Ok(value) -> Assert.equal(actual: Range.get(value), expected: 20)
    case Result.Error(_) -> Assert.fail("valid index was rejected")
  end match
end function
```

```benitoite
@opaque
public record CheckedInteger
  minimum: Integer
  maximum: Integer
  value: Integer
end record
```

An Integer together with the inclusive bounds it satisfies. Use checkInteger and the public projections.

```benitoite
public data RangeError
  InvalidBounds(Integer, Integer)
  OutOfRange(Integer, Integer, Integer)
end data
```

InvalidBounds takes priority over OutOfRange. Failures retain the original inputs.

```benitoite
public function checkInteger(minimum: Integer, maximum: Integer, value: Integer) -> Result[CheckedInteger, RangeError]
```

Checks minimum <= value <= maximum, rejecting reversed bounds first. Constant invalid inputs still return Result.Error. No upper-bound addition is used.

```benitoite
public function minimum(value: CheckedInteger) -> Integer
```

Returns the inclusive lower bound retained by the check.

```benitoite
public function maximum(value: CheckedInteger) -> Integer
```

Returns the inclusive upper bound retained by the check.

```benitoite
public function value(value: CheckedInteger) -> Integer
```

Returns the checked Integer. Call checkInteger again to check it against different bounds.

```benitoite
@opaque
public record CheckedLength[T]
  minimum: Integer
  maximum: Integer
  length: Integer
  values: List[T]
end record
```

A list and its checked inclusive length bounds. This wrapper retains the original list, including references in its elements.

```benitoite
public data LengthError
  InvalidBounds(Integer, Integer)
  OutOfRange(Integer, Integer, Integer)
end data
```

InvalidBounds takes priority over OutOfRange. Failures retain only bounds and the actual length, never the list.

```benitoite
public function checkLength[T](minimum: Integer, maximum: Integer, values: List[T]) -> Result[CheckedLength[T], LengthError]
```

Checks the element count. A negative minimum or reversed bounds is rejected before comparing the length. Constant invalid inputs still return Result.Error.

```benitoite
public function lengthMinimum[T](value: CheckedLength[T]) -> Integer
```

Returns the inclusive minimum length retained by the check.

```benitoite
public function lengthMaximum[T](value: CheckedLength[T]) -> Integer
```

Returns the inclusive maximum length retained by the check.

```benitoite
public function checkedLength[T](value: CheckedLength[T]) -> Integer
```

Returns the element count at the successful check.

```benitoite
public function lengthValues[T](value: CheckedLength[T]) -> List[T]
```

Returns the original immutable list. Its elements, including Reference values, remain shared.

```benitoite
@opaque
public record CheckedIndex[T]
  values: List[T]
  index: Integer
  element: T
end record
```

A list, its checked zero-based index and the element found there. This wrapper retains the original list.

```benitoite
public data IndexError
  OutOfBounds(Integer, Integer)
end data
```

The rejected index and the element count. The original list is not retained.

```benitoite
public function checkIndex[T](index: Integer, values: List[T]) -> Result[CheckedIndex[T], IndexError]
```

Checks the zero-based index and retains its successful element. Both negative and too-large indices return OutOfBounds, including constant inputs.

```benitoite
public function indexValue[T](value: CheckedIndex[T]) -> Integer
```

Returns the zero-based index retained by the check.

```benitoite
public function indexValues[T](value: CheckedIndex[T]) -> List[T]
```

Returns the original immutable list retained by the check.

```benitoite
public function get[T](value: CheckedIndex[T]) -> T
```

Returns the element found during construction. A different list requires a new checkIndex call.
