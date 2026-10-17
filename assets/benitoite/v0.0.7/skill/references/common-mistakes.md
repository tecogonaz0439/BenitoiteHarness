# Common mistakes in Benitoite

<!--
How the code examples in this file are checked (tests/skill_examples.rs):
- A `benitoite error` block must fail `benitoite check`, and every line of the `diagnostic` block after it must appear in the report.
- A `benitoite` block is run, and its standard output is compared with the `output` block after it.
  A last line `exit N` in the `output` block is the expected exit code (0 when absent).
  A `stderr` block after the `output` block is the expected standard error (empty when absent).
- A `benitoite check` block is only checked; it must have no errors and no warnings.
- A `benitoite test` block is run with `benitoite test`; all its tests must pass.
- A `files` block before an example creates a file next to the script: the first line is `name: <file name>`, the rest is the content.
- Blocks with other info strings (such as `text`) are not checked.
-->

Habits from other languages that `benitoite check` rejects, with the first line of the report and the correct way to write it. When a report has a code you do not know, look it up in `diagnostics.md`.

## Confusing inspection, diagnostics, and execution

- `benitoite check` requires `main`. For a test file, use `benitoite test --check` to inspect without running, or add `--report=json` for its contracts. `test --report=json` alone is a usage error; it does not run tests with a different result format.
- `--diagnostics=json` alone selects diagnostic JSON Lines. `check --report=json` and `test --check --report=json` instead write one object on standard output with diagnostics inside it. Explicit `--diagnostics=text` conflicts with reports. Require a complete object with a known version and kind, `status: "ok"`, `complete: true`, and exit status 0 before accepting an aggregate. A null or missing contract is unknown, not pure.
- E0512 means a freshly checked declared bound exceeds `--allow-effects`. Inspect and review the added effects before changing the allowed set; do not automatically copy the rejected set into an approval. Use canonical atomic names without whitespace or duplicates. `IO.All`, aliases, and `*` are not allowed. Tests need `Assert.Check` in that set even when no external operation is declared; `IO.All` excludes it and HTTP effects. A pure set is `--allow-effects=`.
- With an effect limit, a preparation failure in any test entry prevents all tests from starting. Zero passed and zero failed is not enough to establish success: check exit status, summary presence, `filesNotRun`, and `interrupted`. Non-executing inspection has no test-summary line.
- A source hash does not freeze the next execution, and an accepted effect set does not fix files, commands, or their contents. A shell reference is not proof of invocation or reachability; an absent reference does not mean external commands cannot start a shell. Keep isolation in the harness or OS.

See [Inspecting before execution](inspection.md) for the complete workflow and partial-result rules.

## Setting a forbidden command environment variable

`Process.run` and `Process.runAttached` return `Result.Error` with `IOErrorKind.InvalidInput` before starting a child when `Command.environment` explicitly sets a name beginning with `DYLD_` or `LD_` (ASCII case-insensitive). This includes `LD_RUN_PATH` used by build tools, empty values, and resetting the same inherited value. Static checking may still succeed: the failure is a Result value, not a new E diagnostic. Using `or die` turns it into the existing die report and status 1.

Review and remove the explicit setting or choose a supported operation. Do not recommend moving it to the parent's environment or using a shell to bypass the rule. Inherited variables are unchanged, and allowing `Process.Run` also permits a command such as `env` or a shell to set variables itself. This rule does not provide process isolation. See [Running external commands](idioms.md#running-external-commands).

## Contents

- [`let` instead of `bind`](#let-instead-of-bind)
- [Binding the same name twice](#binding-the-same-name-twice)
- [Braces for blocks](#braces-for-blocks)
- [C-style operators](#c-style-operators)
- [The return type after `:`](#the-return-type-after-)
- [Constructors without their type name](#constructors-without-their-type-name)
- [Forgetting to import an IO module](#forgetting-to-import-an-io-module)
- [Importing an unofficial module as `Benitoite.X`](#importing-an-unofficial-module-as-benitoitex)
- [Forgetting `uses`](#forgetting-uses)
- [Ignoring a `Result`](#ignoring-a-result)
- [A last expression without `return`](#a-last-expression-without-return)
- [`for` and `while` loops](#for-and-while-loops)
- [Keywords as names](#keywords-as-names)
- [Callback parameter names that differ from the contract](#callback-parameter-names-that-differ-from-the-contract)
- [`_` as a lambda parameter](#_-as-a-lambda-parameter)

## `let` instead of `bind`

```benitoite error
import Benitoite.Unofficial.IO.Console

function main() -> Unit uses Console.Write
  let total = 1 + 2
  return Console.writeLine(Integer.toString(total))
end function
```

```diagnostic
error[E0230]: `let` is not used to bind names
```

Write `bind name <- expression`:

```benitoite
import Benitoite.Unofficial.IO.Console

function main() -> Unit uses Console.Write
  bind total <- 1 + 2
  return Console.writeLine(Integer.toString(total))
end function
```

```output
3
```

## Binding the same name twice

`bind` cannot reuse a name that is already a local name. Prefer a new name that describes the new value. `shadow` can hide the old name explicitly, but is discouraged, even for intentional hiding; it remains valid and produces no warning.

```benitoite error
function next(n: Integer) -> Integer
  bind x <- n
  bind x <- x + 1
  return x
end function

function main() -> Unit
  return ()
end function
```

```diagnostic
error[E0334]: `x` is already a local name here
```

```benitoite
import Benitoite.Unofficial.IO.Console

function next(n: Integer) -> Integer
  bind x <- n
  bind increased <- x + 1
  return increased
end function

function main() -> Unit uses Console.Write
  return Console.writeLine(Integer.toString(next(41)))
end function
```

```output
42
```

## Braces for blocks

Except for short lambdas (see `grammar.md`), blocks end with `end` and the keyword that opened them: `end function`, `end if`, `end match`, `end lambda`, `end handle`, `end with`, `end data`, `end record`.

```benitoite error
import Benitoite.Unofficial.IO.Console

function main() -> Unit uses Console.Write {
  return Console.writeLine("hi")
}
```

```diagnostic
error[E0212]: blocks are not written with braces
```

```benitoite
import Benitoite.Unofficial.IO.Console

function main() -> Unit uses Console.Write
  match 1 < 2 with
    case true -> Console.writeLine("hi")
    case false -> Console.writeLine("bye")
  end match
  return ()
end function
```

```output
hi
```

## C-style operators

`==`, `!=`, `&&`, `||`, and `!` are not operators. Equality is `=`, inequality is `<>`, and the logical operators are the words `and`, `or`, `not`.

```benitoite error
function inRange(n: Integer) -> Boolean
  return n >= 0 && n != 10
end function

function main() -> Unit
  return ()
end function
```

```diagnostic
error[E0211]: `&&` is not an operator in Benitoite
```

```benitoite error
function isZero(n: Integer) -> Boolean
  return n == 0
end function

function main() -> Unit
  return ()
end function
```

```diagnostic
error[E0211]: `==` is not an operator in Benitoite
```

```benitoite
import Benitoite.Unofficial.IO.Console

function inRange(n: Integer) -> Boolean
  return n >= 0 and n <> 10 and not (n = 7)
end function

function main() -> Unit uses Console.Write
  return Console.writeLine(match inRange(3) with
    case true -> "yes"
    case false -> "no"
  end match)
end function
```

```output
yes
```

## The return type after `:`

```benitoite error
function double(n: Integer): Integer
  return n * 2
end function

function main() -> Unit
  return ()
end function
```

```diagnostic
error[E0234]: the return type is written after `->`
```

```benitoite
import Benitoite.Unofficial.IO.Console

function double(n: Integer) -> Integer
  return n * 2
end function

function main() -> Unit uses Console.Write
  return Console.writeLine(Integer.toString(double(21)))
end function
```

```output
42
```

## Constructors without their type name

`Some`, `None`, `Ok`, `Error`, and the constructors of your own `data` types are written with the type name: `Option.Some(x)`, `Option.None`, `Result.Ok(x)`, `Result.Error(e)`, `Shape.Circle(r)`. This holds in patterns too.

```benitoite error
function first(xs: List[Integer]) -> Option[Integer]
  return match xs with
    case [] -> None
    case [x, ..] -> Some(x)
  end match
end function

function main() -> Unit
  return ()
end function
```

```diagnostic
error[E0331]: the constructor `None` must be written with its type name
```

```benitoite
import Benitoite.Unofficial.IO.Console

function first(xs: List[Integer]) -> Option[Integer]
  return match xs with
    case [] -> Option.None
    case [x, ..] -> Option.Some(x)
  end match
end function

function main() -> Unit uses Console.Write
  return match first([7, 8]) with
    case Option.Some(x) -> Console.writeLine(Integer.toString(x))
    case Option.None -> Console.writeLine("empty")
  end match
end function
```

```output
7
```

## Forgetting to import an IO module

`Console`, `File`, `Process`, `Clock`, `Random`, `Json`, `Csv`, `Http`, and other modules outside the prelude must be imported, also when they only appear in `uses`.

```benitoite error
function main() -> Unit uses Console.Write
  return Console.writeLine("hi")
end function
```

```diagnostic
error[E0332]: the module `Console` is not imported
```

```benitoite
import Benitoite.Unofficial.IO.Console

function main() -> Unit uses Console.Write
  return Console.writeLine("hi")
end function
```

```output
hi
```

## Importing an unofficial module as `Benitoite.X`

Unofficial modules are imported under `Benitoite.Unofficial`. The table in `stdlib/index.md` gives the import name of every module.

```benitoite error
import Benitoite.IO.Console

function main() -> Unit uses Console.Write
  return Console.writeLine("hi")
end function
```

```diagnostic
error[E0321]: `Benitoite.IO.Console` is not a standard library module
```

The correct import is `import Benitoite.Unofficial.IO.Console`, as in the previous section. The module is still referred to as `Console`.

## Forgetting `uses`

Every function that performs an effect, directly or through a function it calls, lists the effect after `uses`.

```benitoite error
import Benitoite.Unofficial.IO.Console

function greet(name: String) -> Unit
  return Console.writeLine("Hello, ${name}!")
end function

function main() -> Unit uses Console.Write
  return greet("Ada")
end function
```

```diagnostic
error[E0501]: this function performs `Console.Write` but does not declare it
```

```benitoite
import Benitoite.Unofficial.IO.Console

function greet(name: String) -> Unit uses Console.Write
  return Console.writeLine("Hello, ${name}!")
end function

function main() -> Unit uses Console.Write
  return greet("Ada")
end function
```

```output
Hello, Ada!
```

## Ignoring a `Result`

A statement whose value is not `Unit` is an error. IO functions that can fail return `Result`; handle it with `try`, `match`, or bind it to `_` only when you really want to ignore the error.

For reusable code, write `try operation or return` and keep a matching Result/Option return type. For an entry point that should exit, write `try operation or die` with `uses Process.Exit` and `import Benitoite.Unofficial.IO.Process`. Do not add Console.Write just for the host's die report. Result errors need Show; IOError, NetworkError, Json.ParseError, and Csv.ParseError already have it. Do not convert these errors to String solely for display. Bare try always returns a failure, including in main; it never infers die. See [Program shape and error handling](idioms.md#program-shape-and-error-handling) for a complete explicit example.

An explicit action ends the try expression: use `(try r or return) + 1`, not `try r or return + 1`. `or return` takes no value; `die()` and `die(a, b)` are invalid. A message is a pure String evaluated only on failure, with no direct return/try/outer resume. Start its opening parenthesis on the same physical line as die. A short lambda must keep the entire action and message on one line.

```benitoite error
import Benitoite.Unofficial.IO.File

function main() -> Unit uses File.Write
  File.writeText(path: "out.txt", text: "data")
  return ()
end function
```

```diagnostic
error[E0416]: the value of this expression is not used
```

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.File
import Benitoite.Unofficial.IO.Process

function main() -> Unit uses File.Write, Console.Write, Process.Exit
  bind _ <- try File.writeText(path: "out.txt", text: "data") or die
  Console.writeLine("written")
  return ()
end function
```

```output
written
```

## A last expression without `return`

A function body returns its value with `return`. A lambda body also needs `return` unless its value is `Unit`. Start the value on the same physical line as `return`. A line break immediately after `return` is E0201, even inside parentheses or before an operator on the next line; comments and blank lines do not allow the value to start later. Once the value has started, ordinary continuation rules allow multiple lines: `return x +` followed by the next operand, `return (` followed by a parenthesized value, `return f(` followed by arguments, or `return if condition then` followed by the branches. A short lambda must still fit entirely on one physical line. `benitoite fmt` reports the error and leaves the file unchanged; it does not join the lines.

```benitoite error
function next(n: Integer) -> Integer
  n + 1
end function

function main() -> Unit
  return ()
end function
```

```diagnostic
error[E0416]: the value of this expression is not used
```

```benitoite
import Benitoite.Unofficial.IO.Console

function next(n: Integer) -> Integer
  return n + 1
end function

function main() -> Unit uses Console.Write
  bind doubled <- List.map(xs: [1, 2], f: lambda(value) return value * 2)
  return Console.writeLine("${next(1)} ${String.join(xs: List.map(xs: doubled, f: lambda(value) return Integer.toString(value)), separator: ",")}")
end function
```

```output
2 2,4
```

## `for` and `while` loops

There are no loop statements. Use `List.forEach`, `List.map`, `List.fold`, or a recursive function.

```benitoite error
import Benitoite.Unofficial.IO.Console

function main() -> Unit uses Console.Write
  for x in [1, 2, 3] do
    Console.writeLine(Integer.toString(x))
  end for
  return ()
end function
```

```diagnostic
error[E0201]: expected a newline or the end of the block, found `x`
```

```benitoite
import Benitoite.Unofficial.IO.Console

function main() -> Unit uses Console.Write
  List.forEach(xs: [1, 2, 3], f: lambda(value)
    Console.writeLine(Integer.toString(value))
  end lambda)
  return ()
end function
```

```output
1
2
3
```

## Keywords as names

These 35 words are keywords and cannot be names: `and`, `bind`, `case`, `const`, `data`, `die`, `div`, `do`, `effect`, `else`, `end`, `false`, `function`, `handle`, `if`, `implement`, `import`, `lambda`, `lazy`, `match`, `mod`, `not`, `or`, `public`, `record`, `resume`, `return`, `shadow`, `then`, `trait`, `true`, `try`, `type`, `uses`, `with`. Choose another name. This restriction also applies to argument labels, fields, import aliases, and names after a dot. `die` is accepted only as a failure action: `try value or die` or `try value or die(message)`. A standalone `die` expression is rejected. `Die`, `died`, and `dieLater` are ordinary identifiers.

`fn`, `pub`, `impl`, and `mut` are ordinary names, not abbreviations for syntax. Write declarations with `function`, `public`, and `implement`, and anonymous functions with `lambda`. `for`, `while`, `loop`, `break`, `continue`, `switch`, `where`, `async`, `await`, `spawn`, and `select` are not reserved either. Use collection functions such as `List.map`, `List.fold`, and `List.forEach` for iteration, and APIs such as `Task.await`, `TaskGroup.spawn`, and `Task.race` for concurrency.

`as` is special only in an import declaration. `language`, `dependency`, `package`, and header field names are read specially in file headers. `equality` and `key` name built-in constraints; `ordered` is a constraint available only in standard-library source. Attribute names (`external`, `test`, `deprecated`, `extensible`, and the standard-library-only `builtin`) are ordinary identifiers outside attributes. Write `@extensible` without arguments on data or record declarations. Outside the defining package, cover future shapes with an unguarded variable or `_`; listing every known constructor, including `Other`, is insufficient. See [Records and data types](idioms.md#records-and-data-types) for the four standard failure types and the limits on record destructuring. A record pattern's `..` does not cover future shapes: use a getter or rewrite a rejected binding as a `match` with an unguarded `case _ ->` branch.

```benitoite error
function handle(request: String) -> String
  return request
end function

function main() -> Unit
  return ()
end function
```

```diagnostic
error[E0216]: `handle` is a keyword and cannot be used as a name
```

## Callback parameter names that differ from the contract

`List.map` expects `function(value: T) -> U`. Naming a lambda parameter `x` changes its contract; it is not silently renamed to `value`. Names must match even for a one-parameter callback, and their order must also match for callbacks with several parameters.

```benitoite error
function main() -> Unit
  bind _ <- List.map(xs: [1, 2], f: lambda(x) return x * 2)
  return ()
end function
```

```diagnostic
error[E0454]: function parameter names do not match
```

Rename the parameter and its bound references together: `List.map(xs: [1, 2], f: lambda(value) return value * 2)`. To pass an existing function such as `Integer.toString`, whose parameter is `n`, wrap it as `lambda(value) return Integer.toString(value)`. If a descriptive local name is needed, bind an alias inside a multiline lambda. Keep required names even for unused parameters. See `idioms.md` for a complete example.

## `_` as a lambda parameter

`_` alone cannot be a lambda parameter. Keep the parameter names required by a callback contract even when a parameter is unused: `Map.filter` requires `key, value`, so an unused key is still named `key`. In a standalone lambda without an expected contract, an identifier such as `_key` is valid, but that identifier then becomes part of its function type.

```benitoite error
function evens(m: Map[String, Integer]) -> Map[String, Integer]
  return Map.filter(m: m, p: lambda(_, value) return value mod 2 = 0 end lambda)
end function

function main() -> Unit
  return ()
end function
```

```diagnostic
error[E0210]: `_` cannot be a lambda parameter
```

```benitoite
import Benitoite.Unofficial.IO.Console

function evens(m: Map[String, Integer]) -> Map[String, Integer]
  return Map.filter(m: m, p: lambda(key, value) return value mod 2 = 0 end lambda)
end function

function main() -> Unit uses Console.Write
  return Console.writeLine(String.join(xs: Map.keys(evens(Map.fromList([Pair("a", 1), Pair("b", 2)]))), separator: ","))
end function
```

```output
b
```
