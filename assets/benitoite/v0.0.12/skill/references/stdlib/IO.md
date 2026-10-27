# Benitoite.IO

Status: standard, in the prelude. It can be used without `import`.

The effect `IO.All`, which stands for all the effects of the modules under `Benitoite.IO`.
It is built in. Import the modules under it, such as `Console`, to use their functions.

Handle IO explicitly when a test supplies its own input.

```benitoite test
import Benitoite.Unofficial.IO.Console
@test
function ioExample() -> Unit uses Assert.Check
  bind value <- handle Console.readLine() with
    case Console.readLine() -> resume(Result.Ok(Option.Some("supplied")))
    case Console.readAll() -> resume(Result.Ok("supplied"))
    case Console.readAllLines() -> resume(Result.Ok(["supplied"]))
    case Console.readAllBytes() -> resume(Result.Ok(Bytes.empty()))
  end handle
  match value with
    case Result.Ok(line) -> Assert.equal(actual: line, expected: Option.Some("supplied"))
    case Result.Error(_) -> Assert.fail("mock input failed")
  end match
end function
```

This run example declares the output effect for the pre-execution limit check.

```benitoite
import Benitoite.Unofficial.IO.Console
function main() -> Unit uses Console.Write
  Console.writeLine("doc output")
end function
```
```output
doc output
```
