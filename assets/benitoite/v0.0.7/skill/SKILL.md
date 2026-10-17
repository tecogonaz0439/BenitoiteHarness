---
name: benitoite
description: Write, check, and run Benitoite scripts (`.bnt`) with the `benitoite` command.
license: MIT OR Apache-2.0
metadata:
  benitoite-version: "{{benitoite-version}}"
---

<!--
How the code examples in this file are checked (tests/skill_examples.rs):
- A `benitoite` block is run, and its standard output is compared with the `output` block after it.
  A last line `exit N` in the `output` block is the expected exit code (0 when absent).
  A `stderr` block after the `output` block is the expected standard error (empty when absent).
- A `benitoite check` block is only checked; it must have no errors and no warnings.
- A `benitoite test` block is run with `benitoite test`; all its tests must pass.
- A `benitoite error` block must fail `benitoite check`, and every line of the `diagnostic` block after it must appear in the report.
- A `files` block before an example creates a file next to the script: the first line is `name: <file name>`, the rest is the content.
- Blocks with other info strings (such as `text` and `sh`) are not checked.
-->

# Benitoite

Benitoite is a statically typed functional scripting language. Scripts end in `.bnt` and are run with the `benitoite` command. The checker reports type errors and undeclared effects before anything runs; use it on every change.

## Check the version first

Run `benitoite --version` and compare the version on its first line (`benitoite X.Y.Z`) with `metadata.benitoite-version` at the top of this file. If they differ, tell the user to run `benitoite skill install` again, because this Skill describes another version of the language.

For machine-readable capabilities, run `benitoite --version=json` as the first and only argument. Require a known `schemaVersion` and the needed integer versions in `formats`; do not infer support from the release number. Unknown formats, statuses, and effect names are unsupported, not success or an empty set. Build information is a build-time self-report: `build.dirty: null` means unknown, not clean, and an omitted dirty input has provenance `notCollected`.

To read this executable's Skill without installing it, use `benitoite skill list` and `benitoite skill show references/idioms.md`. Names are case-sensitive logical names from the list, not OS paths. `--format=json` returns one `skillList` or `skillDocument` object; lengths and SHA-256 hashes describe the same rendered UTF-8 text that `install` writes, including version replacement in this file. These queries do not inspect `HOME` or installed copies. When integrating a harness or interpreting inspection JSON, read [Inspecting before execution](references/inspection.md).

## Workflow for a script

1. Before writing, read `references/idioms.md` and, for every module the script uses, its file `references/stdlib/<Module>.md` (the `references/` directory is next to this file). Do not guess function names, field names, or import names; look them up there. Then write the script, starting from an example in `references/idioms.md` when one fits.
2. Run `benitoite check script.bnt` to check without executing user functions.
3. Read the diagnostics on standard error and fix the script. A diagnostic gives a code such as `E0230`; when its meaning is unclear, look the code up in `references/diagnostics.md`.
4. Repeat 2 and 3 until the check succeeds with exit status 0. Fix warnings too; they usually point at a real mistake.
5. Immediately before the confirmation in [Before running](#before-running), run `benitoite check --report=json script.bnt` once to obtain the declared effects of `main`. Require a complete single JSON object, a known `schemaVersion` and `kind: "checkReport"`, `status: "ok"`, `complete: true`, and exit status 0. Never treat a missing, partial, or failed report as an empty effect set. If it fails, return to ordinary checks while fixing the script, then obtain a fresh report before confirmation.
6. Do the confirmation in [Before running](#before-running), then run `benitoite run --allow-effects=<accepted names> script.bnt [arguments...]`, using the accepted atomic names from `aggregateEffects.expanded`. For a pure script, write `--allow-effects=`. If `E0512` reports an added effect, inspect and review it again before changing the allowed set.

Exit codes of `run`: 0 when `main` finishes, 1 when `main` returns `Result.Error`, an unhandled `or die` ends the run, or the program stops with a runtime error, 2 when the script does not pass the checks, exceeds the allowed effects, hits a compiler limit, or the command line is wrong, 3 for an internal error of `benitoite`. `Process.exit(n)` ends with `n`. `run` and executing `test` end with 130 when interrupted (`SIGINT` or `SIGTERM`). Output failures exit with 1.

## Workflow for tests

`benitoite check` reports an error for a file without `main`, so do not use `check` on test files.

1. Write test functions (see [Tests](#tests)).
2. Run `benitoite test --check file.bnt` (or a directory). Fix diagnostics and repeat until the check succeeds with exit status 0; fix warnings too. This checks the same selected tests without running them and requires no `main`. Diagnostics go to standard error, with no output on standard output.
3. Immediately before confirmation, run `benitoite test --check --report=json file.bnt` once. Apply the same report acceptance rules as for scripts, with `kind: "testCheckReport"`. If it fails, return to ordinary `test --check` while fixing the files, then obtain a fresh report before confirmation. Do the confirmation in [Before running](#before-running), using the union of the test functions' declared effects in `aggregateEffects.expanded`. Keep `Assert.Check` in the accepted set; it is part of the test contract.
4. Run `benitoite test --allow-effects=<accepted names> file.bnt`. With this option, every entry must pass preparation and effect comparison before any test starts; a failure rejects the whole run. Fix it, inspect again, and show any added effects to the user before changing the allowed set. Without the option, a file with check errors is skipped while other files can run.

`test --check` rejects `--allow-effects`, `--max-call-stack`, and every `--wasm-*` option. `test --report=json` without `--check` is a usage error. An inspection writes no `test` or `testSummary` lines. When handling partial results, exit statuses, or executing-test summaries, read [Inspecting before execution](references/inspection.md).

## Before running

Before running a script for the first time, and whenever the effects of `main` grow beyond what you last showed the user:

1. Read the successful inspection's declared upper bound (`main.effects` or each selected test's `effects`, and `aggregateEffects`) and tell the user, in the words of the table below, what kinds of operations the program declares. Compare it with the `uses` signatures when explaining the code. Declared but unused effects still count.
2. If the list contains `File.Write`, `Process.Run`, `Http.Connect`, or `Http.Listen`, ask the user to confirm before running. For `Process.Run`, also name the commands the script starts; what those commands do is outside the checks of `benitoite`.
3. Treat `IO.All` as including `File.Write` and `Process.Run`. Write the specific effects in `uses` instead of `IO.All`.

A script that passes `check` performs no built-in operation outside the `uses` of `main`. `--allow-effects` checks that the newly checked declared bound is a subset of the accepted set and uses that same checked program for execution. It accepts canonical atomic names, not import aliases, `IO.All`, or `*`. Effects say only what kind of operation is allowed, not which file, command, or host. A change to the destination or contents of a write, or to a command within `Process.Run`, can fit the same set. Inspection hashes do not freeze a later run. Static checking does not guarantee the intended result, termination, or success of external operations. This version of `benitoite` does not restrict operations while running: run scripts inside the agent's sandbox.

`processShellReference` describes expression references to the standard shell function anywhere in the checked program, including unused functions and dependencies. It is not a reachability analysis: `present` does not prove a shell will run, and `absent` does not make external commands safe. Review the actual commands when accepting `Process.Run`.

`Process.run` and `Process.runAttached` reject explicit `Command.environment` names starting with `DYLD_` or `LD_` (ASCII case-insensitive), including `LD_RUN_PATH`, empty values, and same-value overwrites, with `IOErrorKind.InvalidInput` before starting a child. Inherited variables remain unchanged, including for `Process.shell`. Allowing `Process.Run` still permits `env` or a shell to set such variables itself; this input rule does not replace OS isolation. See [Running external commands](references/idioms.md#running-external-commands).

| Effect | Tell the user |
|---|---|
| `Console.Write` | writes to standard output and standard error |
| `Console.Read` | reads standard input |
| `File.Read` | reads files and directories |
| `File.Write` | creates, changes, moves, or deletes files and directories |
| `Process.Run` | starts external commands (name them) |
| `Process.Exit` | ends the run with a chosen exit code; `or die` reports its cause and optional message to standard error and exits with status 1 |
| `Process.Environment` | reads command-line arguments, environment variables, the working directory, and the script directory |
| `Clock.Time` | reads the clock and waits for time to pass |
| `Random.Generate` | generates random numbers |
| `Http.Listen` | accepts network connections as an HTTP server |
| `Http.Connect` | connects to HTTP servers over the network |
| `State` | uses mutable cells and concurrent tasks inside the program (no outside effect) |
| `Assert.Check` | checks expectations in tests (no outside effect) |
| `IO.All` | all of `Console`, `File`, `Process`, `Clock`, `Random`, and `State` above |

## Language rules that differ from other languages

- There are 35 keywords; see [Keywords as names](references/common-mistakes.md#keywords-as-names). `die` is reserved and cannot be a name, including a label, field, import alias, or name after a dot. It is accepted only in `try value or die` and `try value or die(message)`, with a pure String message, a Show constraint for Result errors, and the Process.Exit effect. A standalone `die` is rejected. `fn`, `pub`, `impl`, `mut`, `async`, `await`, `spawn`, and `select` remain ordinary names, not abbreviations for syntax. Write declarations with `function`, `public`, and `implement`, and anonymous functions with `lambda`. `as`, file-header words, and attribute names have special meanings only in their own contexts. `extensible` is an ordinary name. The argument-free `@extensible` attribute applies to data and record declarations. Outside their defining package, patterns must cover future shapes with an unguarded variable or `_`.
- Except for a short lambda, blocks end with `end` and the keyword: `end function`, `end if`, `end match`, `end lambda`, `end data`, `end record`, `end handle`, `end with`. There are no braces and no indentation rules.
- Bind a local name with `bind x <- e`, not `let`. `shadow x <- e` can hide a local name that is already visible, but is discouraged. Prefer a new name describing the new value, even when the hiding is intentional. Names cannot be reassigned.
- A function is `function name(a: Type, b: Type) -> ReturnType uses Effect1, Effect2` ... `end function`. The return type follows `->`, never `:`. Return values with `return`. Start the value on the same physical line as `return`; an immediate line break ends the statement even inside parentheses and is an error (E0201). After the value has started, ordinary multiline continuation is allowed (operators, parentheses, call arguments, and `then`). Short lambdas must still fit on one physical line.
- Prefer `match` for selecting a value or action. Use `if` without `else` for a conditional action or an early return. `else` and `else if` are valid but are discouraged; this advice adds no warning. Preserve condition evaluation count and order, evaluate only the selected branch, and keep `return` in the same function or lambda. For effectful conditions, nest matches on booleans instead of moving the conditions into guards.
- For a final catch-all branch, prefer a fresh name such as `case other ->` or `case otherCode ->`. It is an ordinary binding, not a keyword; avoid collisions with outer local names and put no unreachable branch after it. `case _ ->` is valid. This advice does not apply to `_` inside constructors, discarded bindings, or partial applications.
- Prefer a short lambda for one statement: `List.map(xs: xs, f: lambda(value) return value * 2)`. Its whole header and body must fit on one physical line; a non-`Unit` result still needs `return`. A body starting on the header's last line takes `end lambda` only immediately after its statement on that same physical line. A terminator on a later line closes the corresponding outer lambda; blank or comment lines do not change that correspondence. For a multiline body, start on the line after the header and add `end lambda`. Before adding a second statement, switch to the multiline form. The formatter preserves the chosen ending.
- Parenthesize the inner lambda in a directly nested pair if either omits its terminator: `lambda(x) return (lambda(y) return x + y)`. Call arguments and binding statements provide independent boundaries. A pipe in `lambda(x) return x + 1 |> increment` belongs to the body; `(lambda(x) return x + 1)(3)` calls the lambda itself.
- Match patterns with `match value with`, then one `case pattern -> body` per line, then `end match`. A handler is `handle body with` / `case Module.operation(arguments) -> clause` / `end handle`.
- Declare data types with `data Name` ... `end data` and records with `record Name` ... `end record`.
- `@extensible` marks `IOErrorKind`, `NetworkErrorKind`, `Json.ParseError`, and `Csv.ParseError`; it is also available on user data and records. Outside their defining package, cover future shapes with an unguarded variable or `_`, even after every known constructor (including `Other`). A record pattern's `..` only omits current fields: use getters or a `match` with a branch for future shapes instead of a destructuring `bind`. Construction, updates, and getters keep their contracts; record field additions have no compatibility guarantee. `Result` and `Option` remain closed. See [Records and data types](references/idioms.md#records-and-data-types).
- Constructors are written with their type name, in expressions and in patterns: `Option.Some(x)`, `Option.None`, `Result.Ok(x)`, `Result.Error(e)`, `Shape.Circle(r)`.
- Operators: `=` and `<>` compare; `and`, `or`, `not` are logical; `div` and `mod` divide integers; `+` also joins strings. `==`, `!=`, `&&`, `||`, `!` do not exist.
- Use `try e or return` in reusable functions, libraries, and callbacks. It returns the failure from the innermost function or lambda, whose return type must be a matching Result or Option. Convert different error types with `Result.mapError` first. Bare `try e` is permanently fixed to the same returning meaning and adds no Show requirement. The input includes pipes up to the action: `try File.readText(p) |> Result.mapError(r: _, f: lambda(error) return IOError.message(error)) or return`.
- Prefer `try e or die` or `try e or die(message)` in an entry point that should end on failure. It allows any enclosing return type and adds Process.Exit; import `Benitoite.Unofficial.IO.Process` for `uses Process.Exit`. Result errors require Trait.Show; Option does not. The standard Show for IOError, NetworkError, Json.ParseError, and Csv.ParseError returns their message unchanged. Success evaluates neither the pure String message nor Show; failure evaluates message, then Show, then exit. No Console.Write is needed for the host report. See [Program shape and error handling](references/idioms.md#program-shape-and-error-handling) for boundaries, handlers, and stopping rules. A main that returns Result.Error or cannot use Process.Exit can use `or return`.
- For an ordinary call with two or more parameters, label every argument: `List.map(xs: xs, f: action)`. Do not mix named and positional arguments. A one-parameter function accepts `f(value)` or `f(name: value)`; use `f()` for zero parameters. Labels can be written in any order, but evaluation is the callee first, then argument expressions once each in source order. Labels refer to parameters, not local values. These rules also apply to built-ins, function values, methods, effect operations, and external functions. Direct data constructors and patterns, attributes, handler bindings, and `resume` keep their own syntax.
- Function types include names: `function(value: T) -> U`. Names and their order must match every callback contract, even for a single parameter. `List.map` takes `value`, `List.fold` takes `acc, value`, `Map.map` takes `key, value`, and `Result.mapError` takes `error`. Keep expected names even for unused parameters; use a local `bind` for a descriptive alias. Wrap a function with another contract: `lambda(value) return Integer.toString(value)`. Do not invent a contract from call-label order; add a named function-type annotation when E0455 says the contract is unknown.
- Without direct holes, `xs |> List.map(f: action)` supplies the first declared parameter. Label the remaining arguments and do not supply the first parameter twice. Evaluation is the left side once, then the right callee, then explicit arguments in source order. With direct holes, or a parenthesized right side, the pipe calls the resulting function with one argument; two holes do not trigger automatic partial application.
- `_` makes a partial application only as a direct argument of a call: `Result.mapError(r: _, f: lambda(error) return IOError.message(error))`. Holes inherit target parameter names in source order: `f(b: _, a: _)` takes `b, a`. The callee and fixed expressions run when that generated function is called. Positional constructor values and direct constructor holes use original position names `arg1`, `arg2`, etc.; record constructor values use field names, and record accessors take `value`. See `references/idioms.md` for examples of adapting contracts.
- `return` is used in function and lambda bodies. The value of an `if`, a `match` arm, or a handler clause is its last expression; a `return` inside a `case` body returns from the whole function. A function or lambda that produces a value must end with `return`, also when its last expression is a `match` or an `if`: `lambda(acc, value) return acc + value`. A `lazy` body and a `match` guard cannot contain `return` or `try`, except inside a lambda written there (not inside a call with `_`).
- The value of type `Unit` is written `()`: `Result.Ok(())`.
- Records are built with named arguments and read with the type name: `bind item <- Item(name: "pen", price: 120)`, `Item.price(item)`, and copied with changes as `Item(..item, price: 100)`. `item.price`, `Item { ... }`, and `Item(price = 100)` are errors.
- Calls take no type arguments: write `Map.empty()`, not `Map.empty[String, Integer]()`. Add an entry to a map with `Map.set(m: m, k: key, value: value)`; there is no `Map.insert`. Parsing functions such as `Integer.parse` return `Option`.
- Type arguments use brackets: `List[Integer]`, `Map[String, Integer]`, `Result[Unit, String]`. `List<Integer>` is not valid.
- `with r = acquire() do ... end with` releases its resources in reverse order when the block ends. `TaskGroup.spawn(group: group, action: lambda() ... end lambda)` starts a task in a group opened with `TaskGroup.open()`, and `Task.await(task)` waits for its result; releasing the group waits for all its tasks.
- A handler that handles only some operations of an effect does not remove the effect from the type. Functions whose type has `State` (`Reference`, `TaskGroup`, `Task.await`, `close...`) cannot be handled.
- Maps and sets are ordered by key, not by insertion.
- Numeric widths are distinct types with no implicit conversion. Unsuffixed `1` is always `Integer`; use `1s32` for `Integer32` and `1.0f32` for `Float32`. Suffixes (`s8`–`s64`, `u8`–`u64`, `f32`/`f64`) are provisional and may change as the syntax is revised. `Integer64` = `Integer`, `Float64` = `Float`, `UnsignedInteger8` = `Byte`, and `UnsignedInteger` = `UnsignedInteger64`. Keep `Integer`, `Float`, and `Byte` as the usual names. `Byte` has no arithmetic, and unsigned types have no unary minus.
- Integer arithmetic stops on overflow (including unsigned subtraction below zero) or division by zero. Failed numeric conversions return `Option.None`; check the per-module signatures. `Float32` rounds after each operation. See the numeric conversion examples in `references/idioms.md`.
- Experimental `@external("wasm", "path.wasm", "export")` functions use a binary import-free Component Model component and have no body or `end function`. Concrete records/data, flags as `Set` of argument-free constructors, and `Map` with WIT-compatible keys can cross the boundary. External resource types require `uses State`; other external calls are pure. The generated adapter counts packed bytes and separately limits logical value construction with the same `--wasm-transfer` value. See the WIT examples and limits in `references/idioms.md`.
- Strings interpolate with `"${expression}"`. Comments start with `//`.
- There is no `null`, no exception, and no loop statement. Use `Option`, `Result` with `try`, and `List.map`, `List.fold`, `List.forEach`, or recursion.
- Import every module outside the prelude, also when it only appears in `uses`. Unofficial modules (all IO and network modules, `Json`, `Csv`, and others) are imported under `Benitoite.Unofficial`: `import Benitoite.Unofficial.IO.Console`, then referred to as `Console`. `import Benitoite.IO.Console` is an error. The unofficial modules are `IO.Console`, `IO.File`, `IO.Process`, `IO.Clock`, `IO.Random`, `Network.Http`, `Json`, `Csv`, `Regex`, `Path`, `Time`, `Encoding`, and `Hash`; for example `import Benitoite.Unofficial.Network.Http` is referred to as `Http`. Do not rename the modules that declare effects (the IO and network modules, `IO`, and `Assert`) with `as`; that is an error, so that `uses` always shows the real effect names.
- Every function that performs an effect, directly or through a call, lists it after `uses`. Network effects (`Http.Connect`, `Http.Listen`) are not part of `IO.All`.
- A statement whose value is not `Unit` is an error: handle each `Result` with `try`, `match`, or `bind _ <-`.
- `Process.shell` runs its string with `/bin/sh -c`. Write only POSIX sh: no bash arrays, no `[[ ]]`, no `{a,b}` expansion, no `$'...'`. Prefer `Process.run` with an argument list when no shell feature is needed.

A complete script:

```files
name: input.txt
one
two
three
```

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.File

function main() -> Result[Unit, String] uses File.Read, Console.Write
  bind lines <- try File.readLines("input.txt") |> Result.mapError(r: _, f: lambda(error) return IOError.message(error)) or return
  List.forEach(xs: lines, f: lambda(value) Console.writeLine("${String.characterCount(value)} ${value}"))
  return match List.head(lines) with
    case Option.Some(first) -> Result.Ok(Console.writeLine("first: ${first}"))
    case Option.None -> Result.Error("input.txt is empty")
  end match
end function
```

```output
3 one
3 two
5 three
first: one
```

## Tests

A test is a function with `@test("description")`, no parameters, and the return type `Unit` or `Result[Unit, String]` (use the latter when the test uses `try e or return`). Check with `Assert.equal(actual: actual, expected: expected)`. Replace an operation such as `File.readText` or `Process.run` with a handler so that the test does not touch the outside world; more examples are in `references/idioms.md`. Checks in generic helpers recover constructor names when the caller's types can be determined; otherwise, a type may remain unknown. If a report shows `<constructor #n of an unknown type>` (the zero-based constructor position), compare values with a concrete type in the check to show their names.

```benitoite test
import Benitoite.Unofficial.IO.File

function firstLine(path: String) -> Result[String, String] uses File.Read
  bind text <- try File.readText(path) |> Result.mapError(r: _, f: lambda(error) return IOError.message(error)) or return
  return Option.okOr(o: List.head(String.lines(text)), error: "empty file")
end function

@test("firstLine returns the first line")
function firstLineWorks() -> Unit uses Assert.Check, File.Read
  bind result <- handle
    firstLine("data.txt")
  with
    case File.readText(_) -> resume(Result.Ok("alpha\nbeta\n"))
  end handle
  Assert.equal(actual: result, expected: Result.Ok("alpha"))
end function
```

## Reference documents

Read these when you need them:

| Document | When to read it |
|---|---|
| `references/idioms.md` | Writing a common task: files, JSON, CSV, external commands, arguments, HTTP, collections, records, tests, numeric conversions, and WASM/WIT declarations. |
| `references/inspection.md` | Integrating a harness or interpreting inspection reports, build information, effect limits, test summaries, and embedded Skill queries. |
| `references/common-mistakes.md` | A diagnostic comes from a habit of another language (`let`, braces, `==`, missing imports). |
| `references/language-comparison.md` | You know how to write something in Python, JavaScript, or Rust and need the Benitoite form. |
| `references/diagnostics.md` | A diagnostic code (`E0201`, `W0501`, ...) is unclear. |
| `references/grammar.md` | You need the exact syntax of a construct. |
| `references/stdlib/index.md` | You need a standard library module: it lists every module with its import name. |

Each module has its own file named `references/stdlib/<Module>.md`, such as `references/stdlib/List.md` or `references/stdlib/IO.Console.md`; follow the links from `references/stdlib/index.md`.
