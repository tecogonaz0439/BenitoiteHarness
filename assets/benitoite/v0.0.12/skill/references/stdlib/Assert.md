# Benitoite.Assert

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Assert.<name>`, for example `Assert.Check`.

Checks of expected values in tests. The runner of `benitoite test` handles them.

```benitoite
public effect Check
  /// Fails the test when `actual = expected` is false.
  function equal[T: equality](actual: T, expected: T) -> Unit
  /// Fails the test when `actual = expected` is true.
  function notEqual[T: equality](actual: T, expected: T) -> Unit
  /// Fails the test with `message` when `condition` is false.
  function isTrue(condition: Boolean, message: String) -> Unit
  /// Fails the test with `message`. It does not return.
  function fail[T](message: String) -> T
end effect
```

Checks in test functions. `main` cannot use this effect.
