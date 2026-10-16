# Benitoite idioms

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

Working programs for common tasks. Copy one and change it. Every example is checked against the current `benitoite`.

All examples import the IO modules under `Benitoite.Unofficial`, because those modules are unofficial in this release.

## Contents

- [Program shape and error handling](#program-shape-and-error-handling)
- [Calls, callback contracts, holes, and pipes](#calls-callback-contracts-holes-and-pipes)
- [Reading and writing files](#reading-and-writing-files)
- [JSON](#json)
- [CSV](#csv)
- [Running external commands](#running-external-commands)
- [Command-line arguments and environment variables](#command-line-arguments-and-environment-variables)
- [HTTP](#http)
- [Collections](#collections)
- [Records and data types](#records-and-data-types)
- [Writing tests](#writing-tests)
- [Numeric widths and conversions](#numeric-widths-and-conversions)
- [WASM external functions and WIT types](#wasm-external-functions-and-wit-types)

## Program shape and error handling

Use `or return` in a reusable function, library, or callback, and `or die` in an entry point that should report a failure and end the run. This example preserves the error in `load` and uses it as the cause of an exit in `main`.

```files
name: input.txt
ready
```

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.File
import Benitoite.Unofficial.IO.Process

function load(path: String) -> Result[String, IOError] uses File.Read
  return Result.Ok(try File.readText(path) or return)
end function

function main() -> Unit uses File.Read, Console.Write, Process.Exit
  bind text <- try load("input.txt") or die("could not read input.txt")
  Console.write(text)
end function
```

```output
ready
```

`load` preserves the original IOError for its caller. `main` uses the standard Show implementation if loading fails, so it needs no conversion solely for display. Adding Process.Exit changes the function's contract; review the action, return type, and effects together.

`try e or die` and `try e or die(message)` accept a standard `Result[T, E]` or `Option[T]` and produce `T` on success, without restricting the enclosing function's return type. They add `Process.Exit`; import `Benitoite.Unofficial.IO.Process` to name it in `uses`. The module cannot be renamed with `as` (E0339), so write `uses Process.Exit`. The compiler connects the standard Show and exit references internally, but does not import names into your scope. No `Console.Write` effect is required for the host's failure report.

| Action | Enclosing return type | Display requirement | Additional effect |
|---|---|---|---|
| Bare try or `or return` | `Result[U, E]` or `Option[U]`, matching the input | None | None |
| `or die` or `or die(message)` | Any | `Trait.Show` for Result's error type; none for Option | `Process.Exit` |

The input is evaluated once. Success evaluates neither the message nor Show. Failure evaluates the optional pure `String` message once, then the error's Show once, then calls the standard exit operation with status 1. A message does not implicitly bind the error to a name. Direct `return`, `try`, and outer `resume` are forbidden in the message; a lambda written there has its own return boundary. `IOError`, `NetworkError`, `Json.ParseError`, and `Csv.ParseError` already implement Show by returning their `message` string unchanged. Implement Show for another error type, or use `Result.mapError` to convert it to a type with Show. The returning forms need no Show.

A String cause retains Show's quotes and escapes. Option.None has the fixed cause `expected a value, got Option.None`. The message is displayed as plain text, and an empty message is distinct from no message; neither removes the cause. If the message or Show stops with a runtime error or resource failure, that stop is used instead of a die report. Purity and static checking do not guarantee termination, the intended result, or success of external operations.

The default exit ends the whole run, including when called by a child task: remaining tasks are cancelled, resources are released, and buffered output is transferred. The CLI then reports `error: explicit failure exit`, the source location, an optional `message: ...`, and a `cause: ...` once to standard error. Release and final output failures follow as supplementary reports and keep status 1. With `--diagnostics=json`, the die report has `kind: "die"`, `code: null`, and the same location and notes. Cancellation does not undo external work already started. Competing exits, interruption, and internal errors follow the ordinary stop rules; only the adopted die carries this report.

A language handler can replace the existing `Process.Exit` operation; it receives only the usual exit code, and the handled call's report is discarded rather than attached to a later exit. In `benitoite test`, die fails that test and the runner continues with the next one. Text and JSON preserve its cause, message, and location without adding a second copy to captured output. Use `Process.exit(code)` for an arbitrary exit status.

The input includes pipes up to `or return` or `or die` at the same depth; ordinary Boolean `or` stays an operator. Parenthesize an explicit try before adding an operator or call, as in `(try parsePort(text) or die) + 1`. `or return` takes no value. A physical newline immediately after its `return` ends the statement. Start a message's `(` on the same physical line as `die`; outside parentheses, a newline after `die` or its message ends the statement. A short lambda must include its whole action and message on one physical line. The formatter preserves bare and explicit forms.

The following example instead makes returning `Result[Unit, String]` the contract of `main`. When it returns `Result.Error(message)`, the message is reported and the exit code is 1. `or return` (with the same meaning as bare try) returns early with the error.

```benitoite
import Benitoite.Unofficial.IO.Console

function parsePort(text: String) -> Result[Integer, String]
  return match Integer.parse(text) with
    case Option.Some(n) if n > 0 and n < 65536 -> Result.Ok(n)
    case otherPort -> Result.Error("not a port: ${text}")
  end match
end function

function main() -> Result[Unit, String] uses Console.Write
  bind port <- try parsePort("8080") or return
  Console.writeLine("port ${port}")
  bind other <- try parsePort("http") or return
  Console.writeLine("never printed ${other}")
  return Result.Ok(())
end function
```

```output
port 8080
exit 1
```

```stderr
not a port: http
```

Use `Process.exit(code)` (effect `Process.Exit`) only when a specific exit code is needed.

## Calls, callback contracts, holes, and pipes

For an ordinary call with two or more parameters, label every argument with its declared name. A one-parameter call may omit its label; a zero-parameter call uses `f()`. Labels can appear in any order. The callee is evaluated first, then each argument once in source order.

Parameter names belong to function types. A callback must have exactly the expected names in the expected order, including unused parameters. `List.map` expects `value`, while `Integer.toString` declares `n`; use an explicit wrapper to adapt it. `List.fold` expects `acc, value`; Map callbacks commonly expect `key, value`. Read the full declaration in `stdlib/<Module>.md`. A descriptive alias can be bound inside a multiline lambda without changing its contract.

Holes retain their target parameter names in the order written. A pipe without direct holes supplies the first declared parameter and requires labels for the remaining arguments. With direct holes, or a parenthesized right side, it calls the resulting function with one argument. Constructor values use positional names `arg1`, `arg2`, etc.; record constructor values use field names and record accessors take `value`.

```benitoite
import Benitoite.Unofficial.IO.Console

function subtract(a: Integer, b: Integer) -> Integer
  return a - b
end function

function main() -> Unit uses Console.Write
  bind xs <- [1, 2, 3]
  bind doubled <- List.map(xs: xs, f: lambda(value) return value * 2)
  bind labels <- doubled |> List.map(f: lambda(value) return Integer.toString(value))
  Console.writeLine(labels |> String.join(separator: ","))
  bind swapped: function(b: Integer, a: Integer) -> Integer <- subtract(b: _, a: _)
  Console.writeLine("${swapped(b: 2, a: 9)}")
  bind make <- Pair
  bind pair <- make(arg1: 4, arg2: "four")
  Console.writeLine("${Pair.first(pair)} ${Pair.second(pair)}")
end function
```

```output
2,4,6
7
4 four
```

For E0454, compare callback names position by position. For E0455, annotate the function parameter with a named function type; call-label order cannot determine its contract. Direct constructors, patterns, attributes, handler bindings, and `resume` keep their own syntax.

## Reading and writing files

`File.readText` reads a whole file; `File.readLines` reads it as lines. Relative paths are resolved against the directory where `benitoite` was started.

```files
name: input.txt
apple 3
banana 5
cherry 7
```

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.File
import Benitoite.Unofficial.IO.Process

function main() -> Unit uses File.Read, File.Write, Console.Write, Process.Exit
  bind lines <- try File.readLines("input.txt") or die
  bind total <- List.fold(xs: lines, initial: 0, f: lambda(acc, value)
    return match String.split(s: value, separator: " ") with
      case [_, count] -> acc + Option.unwrapOr(o: Integer.parse(count), fallback: 0)
      case otherFields -> acc
    end match
  end lambda)
  bind report <- "lines: ${List.length(lines)}\ntotal: ${total}\n"
  bind _ <- try File.writeText(path: "report.txt", text: report) or die
  bind _ <- try File.appendText(path: "report.txt", text: "done\n") or die
  Console.write(try File.readText("report.txt") or die)
  return ()
end function
```

```output
lines: 3
total: 15
done
```

A missing file is an `IOError` value, not a crash. Match on the `Result` to handle it.

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.File

function main() -> Unit uses File.Read, Console.Write
  return match File.readText("missing.txt") with
    case Result.Ok(text) -> Console.write(text)
    case Result.Error(error) ->
      match IOError.kind(error) with
        case IOErrorKind.NotFound -> Console.writeLine("missing.txt does not exist")
        case otherKind -> Console.writeLine("could not read: ${IOError.message(error)}")
      end match
  end match
end function
```

```output
missing.txt does not exist
```

Listing a directory and checking for a file:

```files
name: notes.md
# notes
```

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.File
import Benitoite.Unofficial.IO.Process

function main() -> Unit uses File.Read, File.Write, Console.Write, Process.Exit
  bind _ <- try File.createDirectory("out") or die
  bind names <- try File.listDirectory(".") or die
  List.forEach(xs: List.sort(names), f: lambda(value) Console.writeLine(value))
  bind found <- try File.exists("notes.md") or die
  Console.writeLine(match found with
    case true -> "notes.md exists"
    case false -> "no notes.md"
  end match)
  return ()
end function
```

```output
main.bnt
notes.md
out
notes.md exists
```

## JSON

`Json.parse` returns a `Json.Value`. Read fields with `Json.get` and the `Json.as...` functions, which return `Option`.

```files
name: orders.json
[
  {"category": "book", "amount": 1200},
  {"category": "food", "amount": 300},
  {"category": "book", "amount": 800}
]
```

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.File
import Benitoite.Unofficial.IO.Process
import Benitoite.Unofficial.Json

function amountOf(order: Json.Value) -> Result[Pair[String, Integer], String]
  bind category <- Option.andThen(o: Json.get(value: order, k: "category"), f: Json.asString)
  bind amount <- Option.andThen(o: Json.get(value: order, k: "amount"), f: Json.asInteger)
  return match Pair(category, amount) with
    case Pair(Option.Some(c), Option.Some(a)) -> Result.Ok(Pair(c, a))
    case otherFields -> Result.Error("an order needs a category and an amount")
  end match
end function

function main() -> Unit uses File.Read, Console.Write, Process.Exit
  bind text <- try File.readText("orders.json") or die
  bind parsed <- try Json.parse(text) or die
  bind orders <- try Option.okOr(o: Json.asArray(parsed), error: "orders.json is not an array") or die
  bind totals <- List.fold(xs: orders, initial: Result.Ok(Map.empty()), f: lambda(acc, value)
    bind totalsMap <- try acc or return
    bind entry <- try amountOf(value) or return
    bind current <- Map.get(m: totalsMap, k: Pair.first(entry)) |> Option.unwrapOr(fallback: 0)
    return Result.Ok(Map.set(m: totalsMap, k: Pair.first(entry), value: current + Pair.second(entry)))
  end lambda)
  Map.forEach(m: try totals or die, f: lambda(key, value)
    Console.writeLine("${key}: ${value}")
  end lambda)
  return ()
end function
```

```output
book: 2000
food: 300
```

Build JSON with the constructors of `Json.Value` and write it with `Json.stringify` or `Json.stringifyPretty`. Object members are written in the order of their keys.

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.Json

function main() -> Unit uses Console.Write
  bind value <- Json.Value.Object(Map.fromList([
    Pair("name", Json.Value.String("benitoite")),
    Pair("tags", Json.Value.Array([Json.Value.String("script"), Json.Value.String("typed")])),
    Pair("stars", Json.Value.Integer(42)),
    Pair("archived", Json.Value.Boolean(false)),
    Pair("license", Json.Value.Null)
  ]))
  return Console.writeLine(Json.stringify(value))
end function
```

```output
{"archived":false,"license":null,"name":"benitoite","stars":42,"tags":["script","typed"]}
```

## CSV

`Csv.parseWithHeader` reads the first row as the header and returns a map per row. Every field is a `String`; convert numbers with `Integer.parse` or `Decimal.parse`.

```files
name: scores.csv
name,score
Alice,90
"Bob, Jr.",75
Carol,82
```

```benitoite
import Benitoite.Unofficial.Csv
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.File
import Benitoite.Unofficial.IO.Process

function scoreOf(row: Map[String, String]) -> Integer
  return Option.andThen(o: Map.get(m: row, k: "score"), f: lambda(value) return Integer.parse(value)) |> Option.unwrapOr(fallback: 0)
end function

function main() -> Unit uses File.Read, File.Write, Console.Write, Process.Exit
  bind text <- try File.readText("scores.csv") or die
  bind rows <- try Csv.parseWithHeader(text) or die
  bind passed <- List.filter(xs: rows, p: lambda(value) return scoreOf(value) >= 80)
  bind output <- List.map(xs: passed, f: lambda(value)
    return [Option.unwrapOr(o: Map.get(m: value, k: "name"), fallback: ""), Integer.toString(scoreOf(value))]
  end lambda)
  bind csv <- Csv.format(List.prepend(xs: output, x: ["name", "score"]))
  bind _ <- try File.writeText(path: "passed.csv", text: csv) or die
  Console.write(csv)
  Console.write(Csv.format([["Bob, Jr.", "75"]]))
  return ()
end function
```

```output
name,score
Alice,90
Carol,82
"Bob, Jr.",75
```

## Running external commands

`Process.run` starts a program with a list of arguments, without a shell. Arguments are passed as they are: `$HOME` below is not expanded. A non-zero exit code is not an error; read it with `Process.Output.exitCode`.

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.Process

function main() -> Unit uses Process.Run, Console.Write, Process.Exit
  bind printed <- try Process.run(Process.command(program: "echo", arguments: ["hello", "$HOME"])) or die
  Console.write(Process.Output.standardOutput(printed))
  bind sort <- Process.Command(..Process.command(program: "sort", arguments: []), environment: Map.fromList([Pair("LC_ALL", "C")]), input: Option.Some("pear\napple\n"))
  bind sorted <- try Process.run(sort) or die
  Console.write(Process.Output.standardOutput(sorted))
  bind failed <- try Process.run(Process.command(program: "sh", arguments: ["-c", "exit 3"])) or die
  Console.writeLine("exit code: ${Process.Output.exitCode(failed)}")
  return ()
end function
```

```output
hello $HOME
apple
pear
exit code: 3
```

`Process.shell` runs one command line with `/bin/sh -c`. Write only POSIX sh: no bash arrays, no `[[ ]]`, no `{a,b}` brace expansion, no `$'...'`. Prefer `Process.run` when no pipe or redirection is needed.

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.Process

function main() -> Unit uses Process.Run, Console.Write, Process.Exit
  bind piped <- try Process.shell("printf 'b\\na\\nc\\n' | LC_ALL=C sort | tr 'a-z' 'A-Z'") or die
  Console.write(Process.Output.standardOutput(piped))
  bind checked <- try Process.shell("if [ -d / ]; then echo root; fi; echo oops >&2; exit 2") or die
  Console.write(Process.Output.standardOutput(checked))
  Console.write("stderr: ${Process.Output.standardError(checked)}")
  Console.writeLine("exit code: ${Process.Output.exitCode(checked)}")
  return ()
end function
```

```output
A
B
C
root
stderr: oops
exit code: 2
```

## Command-line arguments and environment variables

`benitoite run script.bnt a b` passes `a` and `b`; `Process.arguments()` returns them (without the script name). Both need the effect `Process.Environment`.

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.Process

function main() -> Unit uses Process.Environment, Console.Write, Process.Exit
  bind arguments <- Process.arguments()
  Console.writeLine("arguments: ${List.length(arguments)}")
  bind name <- match arguments with
    case [first, ..] -> first
    case [] -> "world"
  end match
  Console.writeLine("Hello, ${name}!")
  bind unset <- try Process.environmentVariable("BENITOITE_SKILL_EXAMPLE_UNSET") or die
  Console.writeLine(Option.unwrapOr(o: unset, fallback: "(not set)"))
  return ()
end function
```

```output
arguments: 0
Hello, world!
(not set)
```

## HTTP

A client and a server in one program. The server listens on port 0 (the system picks a free port), serves a fixed number of requests in a task, and the client calls it. `with` releases its resources in reverse order when the block ends, and releasing a `TaskGroup` waits for all its tasks; if the server expects more requests than the client sends, the program waits forever at `end with`. Keep the two counts equal. Network effects are not part of `IO.All`; list them separately.

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.Process
import Benitoite.Unofficial.Json
import Benitoite.Unofficial.Network.Http

function route(request: Http.Request) -> Http.Response
  return match Pair(Http.Request.method(request), Http.pathSegments(Http.Request.path(request))) with
    case Pair("GET", ["hello", name]) -> Http.text(status: 200, s: "Hello, ${name}!")
    case Pair("GET", ["items"]) -> Http.json(status: 200, value: Json.Value.Array([Json.Value.Integer(1), Json.Value.Integer(2)]))
    case otherPath -> Http.text(status: 404, s: "not found")
  end match
end function

function serveLoop(listener: Http.Listener, remaining: Integer) -> Result[Unit, String] uses Http.Listen, State
  if remaining = 0 then return Result.Ok(()) end if
  bind exchange <- try Http.accept(listener) |> Result.mapError(r: _, f: lambda(error) return NetworkError.message(error)) or return
  bind _ <- try Http.respond(exchange: exchange, response: route(Http.requestOf(exchange))) |> Result.mapError(r: _, f: lambda(error) return NetworkError.message(error)) or return
  bind _ <- try Http.closeExchange(exchange) |> Result.mapError(r: _, f: lambda(error) return NetworkError.message(error)) or return
  return serveLoop(listener: listener, remaining: remaining - 1)
end function

function show(response: Http.Response) -> Unit uses Console.Write
  bind body <- Option.unwrapOr(o: String.fromUTF8(Http.Response.body(response)), fallback: "(not UTF-8)")
  return Console.writeLine("${Http.Response.status(response)} ${body}")
end function

function main() -> Unit uses Http.Listen, Http.Connect, Console.Write, State, Process.Exit
  with listener = try Http.listen(host: "127.0.0.1", port: 0) or die,
    group = TaskGroup.open() do
    bind base <- "http://127.0.0.1:${Http.listenerPort(listener)}"
    bind server <- TaskGroup.spawn(group: group, action: lambda() return serveLoop(listener: listener, remaining: 3))
    bind hello <- try Http.get(base + "/hello/Ada") or die
    bind items <- try Http.get(base + "/items") or die
    bind request <- Http.ClientRequest(..Http.clientRequest(method: "POST", url: base + "/upload"), body: String.toUTF8("data"))
    bind missing <- try Http.send(request) or die
    bind _ <- try Task.await(server) or die
    List.forEach(xs: [hello, items, missing], f: lambda(value) show(value))
    return ()
  end with
end function
```

```output
200 Hello, Ada!
200 [1,2]
404 not found
```

A server that runs until it is stopped is simplest with `Http.serve`, which calls the handler for every request. (This example is only checked, not run, because it never ends.)

```benitoite check
import Benitoite.Unofficial.Network.Http

function respondTo(request: Http.Request) -> Http.Response
  return match Http.pathSegments(Http.Request.path(request)) with
    case [] -> Http.html(status: 200, s: "<h1>Home</h1>")
    case ["health"] -> Http.text(status: 200, s: "ok")
    case otherPath -> Http.text(status: 404, s: "not found")
  end match
end function

function main() -> Result[Unit, String] uses Http.Listen, State
  return Http.serve(host: "127.0.0.1", port: 8080, handler: respondTo) |> Result.mapError(r: _, f: lambda(error) return NetworkError.message(error))
end function
```

## Collections

Lists, maps, and sets are persistent: functions return new collections and never change the old one. Maps and sets are ordered by their keys, not by insertion order, so `Map.forEach` and `Set.toList` visit keys in sorted order.

```benitoite
import Benitoite.Unofficial.IO.Console

function main() -> Unit uses Console.Write
  bind numbers <- List.range(start: 1, stop: 6)
  bind squares <- List.map(xs: numbers, f: lambda(value) return value * value)
  bind even <- List.filter(xs: squares, p: lambda(value) return value mod 2 = 0)
  Console.writeLine("${List.length(numbers)} numbers, sum ${List.fold(xs: numbers, initial: 0, f: lambda(acc, value) return acc + value)}")
  Console.writeLine(String.join(xs: List.map(xs: even, f: lambda(value) return Integer.toString(value)), separator: ", "))
  bind words <- String.split(s: "the cat saw the dog", separator: " ")
  bind counts <- List.fold(xs: words, initial: Map.empty(), f: lambda(acc, value)
    return Map.set(m: acc, k: value, value: Option.unwrapOr(o: Map.get(m: acc, k: value), fallback: 0) + 1)
  end lambda)
  Map.forEach(m: counts, f: lambda(key, value) Console.writeLine("${key}=${value}"))
  Console.writeLine(String.join(xs: Set.toList(Set.fromList(words)), separator: " "))
  return ()
end function
```

```output
5 numbers, sum 15
4, 16
cat=1
dog=1
saw=1
the=2
cat dog saw the
```

`List.range(start: start, stop: stop)` stops before `stop`. Lists have no index operator; use `List.get(xs: xs, i: i)`, which returns an `Option`.

Loops are written with recursion or with `List.forEach`, `List.map`, `List.fold`. A function that calls itself last (`return f(...)`) does not grow the stack.

```benitoite
import Benitoite.Unofficial.IO.Console

function countDown(n: Integer, acc: List[String]) -> List[String]
  if n = 0 then return List.reverse(acc) end if
  return countDown(n: n - 1, acc: List.prepend(xs: acc, x: Integer.toString(n)))
end function

function main() -> Unit uses Console.Write
  return Console.writeLine(String.join(xs: countDown(n: 5, acc: []), separator: " "))
end function
```

```output
5 4 3 2 1
```

A mutable cell is a `Reference`; its functions need the effect `State`.

```benitoite
import Benitoite.Unofficial.IO.Console

function main() -> Unit uses Console.Write, State
  bind counter <- Reference.new(0)
  List.forEach(xs: [3, 4, 5], f: lambda(value) Reference.set(reference: counter, value: Reference.get(counter) + value))
  return Console.writeLine("total ${Reference.get(counter)}")
end function
```

```output
total 12
```

## Records and data types

A record has named fields. Read a field with `Type.field(value)`; make a changed copy with `Type(..value, field: new)`. A data type lists its constructors, which are always written with the type name.

Use the argument-free `@extensible` attribute on a data or record declaration when external patterns must also cover future shapes. Put it before `public` or `public(package)` if present. Inside its defining package, listing the current constructors can be exhaustive. Outside it, include an unguarded `case other ->` or `case _ ->`; a known constructor named `Other` does not cover future shapes. Guards do not count toward exhaustiveness. The rule applies through aliases and inside `Result`, `Option`, lists, and record fields. The package boundary is the same as for `public(package)`; a different module in the same package is still internal.

The four standard failure types `IOErrorKind`, `NetworkErrorKind`, `Json.ParseError`, and `Csv.ParseError` have this attribute. `Result` and `Option` remain closed; `Result.Error(error)` binds the whole error without decomposing it. For the two ParseError records, prefer `Json.ParseError.message(error)` or `Csv.ParseError.message(error)` to destructuring. A record pattern with `..` covers only the current shape, so an external `match` needs another variable or `_` branch (`E0601`), and a destructuring `bind` or `shadow` is rejected (`E0607`). Construction, updates, and getters still use the declared fields. The attribute does not promise compatibility for adding record fields.

Adding constructors to extensible data preserves external pattern exhaustiveness, but can change results, including by reclassifying a value formerly represented by `Other`. Removing or renaming constructors, or changing their fields, is outside this guarantee. Adding the attribute to a closed type can reject existing external patterns; removing it commits to a closed set and can make a branch unreachable. It does not let callers add constructors or change runtime tags, ABI, equality, or key constraints. WIT matching remains exact; the attribute does not allow unknown external tags.

```benitoite
import Benitoite.Unofficial.IO.Console

record Item
  name: String
  price: Integer
  quantity: Integer
end record

data Shape
  Circle(Float)
  Rectangle(Float, Float)
end data

function area(shape: Shape) -> Float
  return match shape with
    case Shape.Circle(r) -> 3.0 * r * r
    case Shape.Rectangle(w, h) -> w * h
  end match
end function

function main() -> Unit uses Console.Write
  bind item <- Item(name: "pen", price: 120, quantity: 3)
  bind more <- Item(..item, quantity: 10)
  Console.writeLine("${Item.name(more)}: ${Item.price(more) * Item.quantity(more)}")
  bind shapes <- [Shape.Circle(1.0), Shape.Rectangle(2.0, 3.5)]
  List.forEach(xs: shapes, f: lambda(value) Console.writeLine(Float.toString(area(value))))
  return ()
end function
```

```output
pen: 1200
3.0
7.0
```

## Writing tests

A test is a function with `@test("description")` that takes no arguments and returns `Unit` or `Result[Unit, String]`. A test that uses `try e or return` returns `Result[Unit, String]`; returning `Result.Error(message)` fails the test. Check results with `Assert.equal(actual: actual, expected: expected)` and `Assert.isTrue(condition: condition, message: message)` (effect `Assert.Check`). Run the file with `benitoite test file.bnt`; do not run `benitoite check` on a test file without `main`.

```benitoite test
function slugify(title: String) -> String
  return String.join(xs: String.split(s: String.toLowercase(String.trim(title)), separator: " "), separator: "-")
end function

@test("slugify lowercases and joins words")
function slugifyJoins() -> Unit uses Assert.Check
  Assert.equal(actual: slugify(" Hello World "), expected: "hello-world")
  Assert.isTrue(condition: String.contains(s: slugify("A B"), part: "-"), message: "the words are joined with -")
end function
```

Replace a built-in operation in a test with `handle ... with case Op(args) -> ...`. The handler runs instead of the real operation, so the test does not touch files or start commands. `resume(value)` returns `value` to the code that called the operation. Handling some operations of an effect does not remove the effect from the type, so the test still lists `File.Read` and `Process.Run` in `uses`. Functions whose type has `State` (`Reference`, `TaskGroup`, `Task.await`, the `close...` functions) cannot be handled in a `case`.

```benitoite test
import Benitoite.Unofficial.IO.File
import Benitoite.Unofficial.IO.Process

function countWords(path: String) -> Result[Integer, String] uses File.Read
  bind text <- try File.readText(path) |> Result.mapError(r: _, f: lambda(error) return IOError.message(error)) or return
  return Result.Ok(List.length(List.filter(xs: String.split(s: text, separator: " "), p: lambda(value) return value <> "")))
end function

function gitBranch() -> Result[String, String] uses Process.Run
  bind output <- try Process.run(Process.command(program: "git", arguments: ["branch", "--show-current"])) |> Result.mapError(r: _, f: lambda(error) return IOError.message(error)) or return
  return Result.Ok(String.trim(Process.Output.standardOutput(output)))
end function

@test("countWords reads the file through File.readText")
function countWordsReadsFile() -> Unit uses Assert.Check, File.Read
  bind result <- handle
    countWords("notes.txt")
  with
    case File.readText(path) ->
      Assert.equal(actual: path, expected: "notes.txt")
      resume(Result.Ok("one two  three"))
  end handle
  Assert.equal(actual: result, expected: Result.Ok(3))
end function

@test("gitBranch runs git without starting it")
function gitBranchRunsGit() -> Unit uses Assert.Check, Process.Run
  bind result <- handle
    gitBranch()
  with
    case Process.run(command) ->
      Assert.equal(actual: Process.Command.program(command), expected: "git")
      resume(Result.Ok(Process.Output(exitCode: 0, standardOutput: "main\n", standardError: "")))
  end handle
  Assert.equal(actual: result, expected: Result.Ok("main"))
end function
```

## Numeric widths and conversions

Use the aliases `Integer`, `Float`, and `Byte` as the usual names. `Integer64`, `Float64`, and `UnsignedInteger8` are their canonical names, and `UnsignedInteger` is an alias of `UnsignedInteger64`. Aliases are the same type. The seven types (`Integer8`, `Integer16`, `Integer32`, `UnsignedInteger16`, `UnsignedInteger32`, `UnsignedInteger64`, and `Float32`) are distinct. All numeric types and modules are in the prelude; no import is needed.

Suffixes explicitly choose a width: `42s16`, `0xffu8`, `65535u16`, `18446744073709551615u64`, `1.5f32`. Unsuffixed values retain `Integer` or `Float`; the expected type never changes them. The suffix spelling is provisional and may change when the syntax is revised. Floating-point suffixes require a decimal point or exponent (`1.0f32`, not `1f32`). Only a directly applied minus sign includes the sign in signed-literal range checking: `-128s8` works, `-(128s8)` does not. Unsigned unary minus is forbidden even on zero. `Byte` has no arithmetic. `Integer8`, `Integer16`, `Integer32`, the unsigned integer types, `Byte`, and `Float32` cannot be constant or range patterns; use a bound name and a guard.

Range-checked conversions return `Option`; integer arithmetic overflow and division by zero stop execution. Narrow integer modules convert through `Integer`. `UnsignedInteger64.toInteger` returns `Option` because its upper half does not fit; `toDecimal` is exact over its whole range. `Float32.fromFloat` rounds and returns `None` when finite input becomes infinite; `Float32.toFloat` widens finite values exactly. Arithmetic rounds to binary32 after each operation. Read the corresponding generated module reference for every function's signature.

```benitoite
import Benitoite.Unofficial.IO.Console

function smallInteger(text: String) -> Option[Integer16]
  bind value <- try Integer.parse(text) or return
  return Integer16.fromInteger(value)
end function

function main() -> Unit uses Console.Write
  bind value <- Option.unwrapOr(o: smallInteger("42"), fallback: 0s16)
  bind wide: UnsignedInteger <- 18446744073709551615u64
  Console.writeLine("${value + 1s16} ${smallInteger("32768") = Option.None}")
  Console.writeLine("${UnsignedInteger64.toDecimal(wide)} ${UnsignedInteger64.toInteger(wide) = Option.None}")
  Console.writeLine("${Float32.toFloat(1.5f32)} ${Byte.toInteger(255u8)}")
end function
```

```output
43 true
18446744073709551615 true
1.5 255
```

## WASM external functions and WIT types

Declare external functions without a body or `end function`, and check the component's actual WIT contract before calling. The component must be a binary, import-free Component Model component, with no WASI or host functions. Paths are relative to the root that owns the declaration; package components must be under `wasm/`. The export is a world function or `interface#function`, matching the full interface name including its version. The declaration is monomorphic and uses concrete types. Functions containing external resource types in their actual parameter/result fields need `uses State`; others are pure.

| Benitoite type | WIT correspondence |
|---|---|
| `Integer8`/`Integer16`/`Integer32`/`Integer` | `s8`/`s16`/`s32`/`s64` |
| `Byte`/`UnsignedInteger16`/`UnsignedInteger32`/`UnsignedInteger64` | `u8`/`u16`/`u32`/`u64` |
| `Float32`/`Float` | `f32`/`f64` |
| `Bytes`, `List[Byte]`; other lists, options, results | `list<u8>`; recursive correspondence to WIT list/option/result |
| Concrete records and `data` | Substitute type arguments, then match fields/cases in declaration order and normalized names |
| `Set[D]`, D with 1–32 argument-free constructors | `flags`; an empty selection is allowed |
| `Map[K, V]` | `map`; K is Boolean, an integer type, Character, or String; V is any supported type |
| `Pair`, `Triple`, or another single-constructor `data` with one or more arguments | `tuple` by argument position |
| A variant constructor with multiple arguments | One flat tuple payload, distributed back into those arguments |

Record/case names match after removing hyphens and lowercasing ASCII. Finite generic nesting is allowed; recursive declarations and unresolved type/effect variables are rejected. Functions, Decimal, ordinary Set types, tasks, and resources used with `with` cannot be passed. `Unit` is allowed only as an omitted function result or omitted side of a standard `Result`.

These declarations match the sample components used by the repository's `external/wit-docs-values` executable example:

```text
record Box[T]
  value: T
end record
@external("wasm", "generic.wasm", "box")
function box(v: Box[Integer]) -> Box[Integer]
@external("wasm", "generic.wasm", "nested")
function nested(v: Box[Box[Integer]]) -> Box[Box[Integer]]

data D1
  F1
end data
@external("wasm", "flags.wasm", "f1")
function f1(v: Set[D1]) -> Pair[Set[D1], Integer]

@external("wasm", "maps.wasm", "result")
function result(v: Map[String, Integer]) -> Map[String, Integer]

data Four
  Item(Integer, Integer, Integer, Integer)
end data
@external("wasm", "tuples.wasm", "tuple4")
function tuple4(v: Four) -> Four

data Two
  Value(Integer, Integer)
  Empty
end data
@external("wasm", "variants.wasm", "fields")
function two(v: Two) -> Two
```

The WIT shapes are a record with `value: s64`, a nested record, `flags { f1 }` with a tuple result, `map<string, s64>`, a four-element `tuple<s64, s64, s64, s64>`, and `variant { value(tuple<s64, s64>), empty }`. The sample guests return 42 from `box(Box(value: 35))`, inner value 0 from `nested(Box(value: Box(value: -7)))`, `Pair(Set.fromList([D1.F1]), 0)` from `f1(Set.empty())`, `Four.Item(12, 14, 16, 18)` from `tuple4(Four.Item(1, 2, 3, 4))`, and `Two.Value(14, 29)` from `two(Two.Value(3, 7))`. The sample `Map.toList(result(Map.empty()))` is `[Pair("a", 2), Pair("z", 3)]`: every returned entry is checked before normalization; duplicate keys keep the last value, and the final map is ordered by key. These numerical transformations are behavior of the sample guests.

The checker selects `typed`, `adapter`, `dynamic`, or `stateful` before execution, and does not retry after a failure. The `typed` route counts bytes and elements. Other supported values use a generated adapter; external resource types use the stateful dynamic route. Adapter generation or layout limits can select the dynamic route before calling. Generated and compiled code is shared within one execution, while pure calls each get a fresh Store.

Default limits are 10^10 fuel, 64 MiB total guest/adapter linear memory, 16 each of core instances/memories/tables, 10,000 elements per table, and 256 MiB transfer per direction. The adapter counts the entire packed argument/result sequences, including a 32-byte header and padding. Separately, the same `--wasm-transfer` value limits logical construction per direction: 16 bytes per language value occurrence plus string/Bytes contents. Bytes elements do not count as separate values; List[Byte] elements do. Map entries discarded by duplicate-key normalization still count. ABI-only grouping tuples do not add a language value. These limits do not bound total process memory. Adapter instructions consume fuel and respond to SIGINT/SIGTERM; `Task.withTimeout` cannot cancel an external call. String/Bytes/list size limits also apply.

Use `--wasm-stats` on `run` or `test` to inspect routes, fallback reasons, transfer, `construction_bytes` (the sum of both directions), and adapter failures. `--wasm-memory`, `--wasm-fuel`, and `--wasm-transfer` change limits without changing the function contract. Fuel exhaustion reports R0904, transfer/construction/memory exhaustion R0905, object limits R0906, and returned-value-size limits R0902. Traps and guest ABI violations report R0601; a declared `Result.Error` is an ordinary value. These runtime failures cannot be caught with `match` or `try`.
