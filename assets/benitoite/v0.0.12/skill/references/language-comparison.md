# Benitoite compared with other languages

<!--
How the code examples in this file are checked (tests/skill_examples.rs):
- A `benitoite` block is run, and its standard output is compared with the `output` block after it.
  A last line `exit N` in the `output` block is the expected exit code (0 when absent).
  A `stderr` block after the `output` block is the expected standard error (empty when absent).
- A `benitoite check` block is only checked; it must have no errors and no warnings.
- A `benitoite test` block is run with `benitoite test`; all its tests must pass.
- A `benitoite error` block must fail `benitoite check`, and every line of the `diagnostic` block after it must appear in the report.
- A `files` block before an example creates a file next to the script: the first line is `name: <file name>`, the rest is the content.
- Code in tables and blocks with other info strings are not checked; the examples after each table use the same forms.
-->

How common constructs of Python, JavaScript, and Rust are written in Benitoite. Modules outside the prelude (`Console`, `File`, `Process`, `Json`, `Csv`, `Http`, ...) must be imported; see `stdlib/index.md` for their import names.

## Syntax

For failure propagation, Rust's `?` corresponds to `try result or return`: bare `try result` keeps this same returning meaning. To end a run with a cause, use `try result or die` with Process.Exit; it permits any enclosing return type and requires Show for the Result error. It is not an exception handler. See [Program shape and error handling](idioms.md#program-shape-and-error-handling).

| Python | JavaScript | Rust | Benitoite |
|---|---|---|---|
| `int` | `number` | `i64` | `Integer` |
| `float` | `number` | `f64` | `Float` |
| `str` | `string` | `String` | `String` |
| `bool` | `boolean` | `bool` | `Boolean` |
| `list[int]` | `number[]` | `Vec<i64>` | `List[Integer]` (type arguments use `[]`, never `<>`) |
| `dict[str, int]` | `Map<string, number>` | `BTreeMap<String, i64>` | `Map[String, Integer]` |
| `Optional[int]` | `number \| null` | `Option<i64>` | `Option[Integer]` |

| Python | JavaScript | Rust | Benitoite |
|---|---|---|---|
| `x = 1` | `const x = 1` | `let x = 1;` | `bind x <- 1` |
| `x = x + 1` (rebinding) | `let x; x = x + 1` | `let x = x + 1;` | `bind increased <- x + 1` (prefer a new name; `shadow` is valid but discouraged) |
| `def f(a: int) -> int:` | `function f(a) {` | `fn f(a: i64) -> i64 {` | `function f(a: Integer) -> Integer` ... `end function` |
| `lambda a: a + 1` | `(a) => a + 1` | `\|a\| a + 1` | `lambda(a) return a + 1` |
| `if c: ... elif d: ... else: ...` | `if (c) {} else if (d) {} else {}` | `if c {} else if d {} else {}` | `if c then ... else if d then ... else ... end if` |
| `a if c else b` | `c ? a : b` | `if c { a } else { b }` | `match c with` / `case true -> a` / `case false -> b` / `end match` |
| `match v:` / `case ...:` | `switch (v)` | `match v { p => e }` | `match v with` / `case p -> e` / `end match` |
| `==`, `!=` | `===`, `!==` | `==`, `!=` | `=`, `<>` |
| `and`, `or`, `not` | `&&`, `\|\|`, `!` | `&&`, `\|\|`, `!` | `and`, `or`, `not` |
| `7 // 2`, `7 % 2` (round down) | `Math.floor(7 / 2)` | `7_i64.div_euclid(2)` | `Integer.floorDivide(a: 7, b: 2)`, `Integer.floorModulo(a: 7, b: 2)` |
| `int(7 / 2)`, `math.fmod(7, 2)` (toward zero) | `Math.trunc(7 / 2)`, `7 % 2` | `7 / 2`, `7 % 2` | `7 div 2`, `7 mod 2` |
| `7 / 2` | `7 / 2` | `7.0 / 2.0` | `7.0 / 2.0` (both `Float`; `/` is not allowed on `Integer`) |
| `f"{name}: {n}"` | `` `${name}: ${n}` `` | `format!("{name}: {n}")` | `"${name}: ${n}"` |
| `"a" + "b"` | `"a" + "b"` | `format!("{a}{b}")` | `"a" + "b"` |
| `# comment` | `// comment` | `// comment` | `// comment` |
| `None` | `null`, `undefined` | `None` | `Option.None` |
| `Some`-like value | value | `Some(x)` | `Option.Some(x)` |
| `raise E(...)` / `try:` | `throw` / `try {}` | `Err(e)` / `?` | `return Result.Error(e)` / `try expr or return` |
| `class P:` with fields | `{ name, age }` | `struct P { name: String }` | `record P` ... `end record` |
| `P(name="a")` | `{ name: "a" }` | `P { name: "a".into() }` | `P(name: "a")` |
| `p.name` | `p.name` | `p.name` | `P.name(p)` |
| `dataclasses.replace(p, age=3)` | `{ ...p, age: 3 }` | `P { age: 3, ..p }` | `P(..p, age: 3)` |
| `Enum` / tagged union | tagged object | `enum Shape { Circle(f64) }` | `data Shape` / `Circle(Float)` / `end data` |
| `for x in xs:` | `for (const x of xs)` | `for x in xs {}` | `List.forEach(xs: xs, f: lambda(value) ...)` |
| `while` loop | `while` loop | `loop` / `while` | a recursive function |
| `import json` | `import fs from "fs"` | `use std::fs;` | `import Benitoite.Unofficial.Json` |
| `def main():` | top-level code | `fn main()` | `function main() -> Result[Unit, String] uses ...` |

For selecting values or actions, prefer `match`; use an `if` without `else` for conditional work or an early return. The table keeps `else if` to explain valid syntax, but it and `else` are discouraged. Preserve condition evaluation order and count, branch evaluation, and the target of `return`; nest boolean matches for effectful conditions. Prefer a fresh ordinary name for a final catch-all pattern; `_` inside constructors, in discarded bindings, and in partial applications is unaffected. Short lambdas must fit entirely on one physical line; multiline bodies start after the header and require `end lambda`. See `grammar.md` for terminator matching and nested lambdas.

Notes on numbers and ranges:

- `Integer` is 64-bit. Overflow stops the program with a runtime error (Python integers have no limit).
- `div` rounds toward zero and the sign of `mod` follows the dividend: `-7 div 2` is `-3` and `-7 mod 2` is `-1`. Python's `//` and `%` round down (`-7 // 2` is `-4`, `-7 % 2` is `1`); use `Integer.floorDivide` and `Integer.floorModulo` for that behavior.
- `div` or `mod` by 0 stops the program with a runtime error. `Float` division by 0 gives infinity or NaN.
- `Integer` has no `/`; convert with `Integer.toFloat` first when you need a fractional result.
- A range pattern `case 0..19` includes both ends (Rust `0..=19`). `List.range(start: 0, stop: 20)` excludes the upper end (Rust `0..20`, Python `range(0, 20)`).

There is no `null`, no exception, no mutable variable, and no loop statement. A function that performs IO lists its effects after `uses`.

```benitoite
import Benitoite.Unofficial.IO.Console

record Person
  name: String
  age: Integer
end record

function describe(p: Person) -> String
  bind group <- match Person.age(p) < 20 with
    case true -> "young"
    case false -> "adult"
  end match
  return "${Person.name(p)} (${group})"
end function

function main() -> Unit uses Console.Write
  bind ada <- Person(name: "Ada", age: 36)
  bind older <- Person(..ada, age: Person.age(ada) + 1)
  Console.writeLine(describe(older))
  Console.writeLine("${7 div 2} ${7 mod 2} ${Float.toString(7.0 / 2.0)}")
  Console.writeLine("${-7 div 2} ${-7 mod 2} ${Integer.floorDivide(a: -7, b: 2)} ${Integer.floorModulo(a: -7, b: 2)}")
  bind label <- match Person.age(older) with
    case 0..19 -> "teen or child"
    case n if n >= 65 -> "senior"
    case otherAge -> "working age"
  end match
  Console.writeLine(label)
  return ()
end function
```

```output
Ada (adult)
3 1 3.5
-3 -1 -4 1
working age
```

## Functions

For ordinary calls with two or more parameters, label every argument with its declared name; labels may be written in any order, but argument expressions run in source order after the callee. One-parameter calls can omit the label. Callback names and their order are part of the function type: `List.map` requires `function(value: T) -> U`, so an existing function with another parameter name needs a wrapper such as `lambda(value) return Integer.toString(value)`. `List.fold` requires `acc, value`; unused parameters keep their expected names. See `idioms.md` for holes, pipes, and generated names.

| Python | JavaScript | Rust | Benitoite |
|---|---|---|---|
| `print(s)` | `console.log(s)` | `println!("{s}")` | `Console.writeLine(s)` (effect `Console.Write`) |
| `print(s, end="")` | `process.stdout.write(s)` | `print!("{s}")` | `Console.write(s)` |
| `sys.stderr.write(s)` | `console.error(s)` | `eprintln!("{s}")` | `Console.writeErrorLine(s)` |
| `input()` | `readline` | `stdin().read_line` | `Console.readLine()` (effect `Console.Read`) |
| `str(n)` | `String(n)` | `n.to_string()` | `Integer.toString(n)`, `Float.toString(x)` |
| `int(s)` | `parseInt(s)` | `s.parse::<i64>()` | `Integer.parse(s)` (returns `Option[Integer]`) |
| `float(n)` | `Number(n)` | `n as f64` | `Integer.toFloat(n)` |
| `len(xs)` | `xs.length` | `xs.len()` | `List.length(xs)` |
| `len(s)` | `s.length` | `s.chars().count()` | `String.characterCount(s)` |
| `xs[i]` | `xs[i]` | `xs.get(i)` | `List.get(xs: xs, i: i)` (returns `Option`) |
| `xs + [x]` | `[...xs, x]` | `xs.push(x)` | `List.append(xs: xs, x: x)` (returns a new list) |
| `[f(x) for x in xs]` | `xs.map(f)` | `xs.iter().map(f)` | `List.map(xs: xs, f: f)` |
| `[x for x in xs if p(x)]` | `xs.filter(p)` | `xs.iter().filter(p)` | `List.filter(xs: xs, p: p)` |
| `functools.reduce(f, xs, init)` | `xs.reduce(f, init)` | `xs.iter().fold(init, f)` | `List.fold(xs: xs, initial: init, f: f)` |
| `sorted(xs)` | `xs.toSorted((a, b) => a - b)` | `xs.sort()` (in place) | `List.sort(xs)` (returns a new list) |
| `range(a, b)` | | `a..b` | `List.range(start: a, stop: b)` |
| `s.split(",")` | `s.split(",")` | `s.split(',')` | `String.split(s: s, separator: ",")` |
| `",".join(xs)` | `xs.join(",")` | `xs.join(",")` | `String.join(xs: xs, separator: ",")` |
| `s.strip()` | `s.trim()` | `s.trim()` | `String.trim(s)` |
| `s.splitlines()` | `s.split("\n")` | `s.lines()` | `String.lines(s)` |
| `x in s` (strings) | `s.includes(x)` | `s.contains(x)` | `String.contains(s: s, part: x)` |
| `{"a": 1}` | `new Map([["a", 1]])` | `BTreeMap::from([("a", 1)])` | `Map.fromList([Pair("a", 1)])` (ordered by key, not by insertion as in Python) |
| `d.get(k)` | `m.get(k)` | `m.get(&k)` | `Map.get(m: m, k: k)` (returns `Option`) |
| `d[k] = v` | `m.set(k, v)` | `m.insert(k, v)` | `Map.set(m: m, k: k, value: v)` (returns a new map) |
| `set(xs)` | `new Set(xs)` | `HashSet::from_iter` | `Set.fromList(xs)` |
| `open(p).read()` | `fs.readFileSync(p, "utf8")` | `fs::read_to_string(p)` | `File.readText(p)` (effect `File.Read`) |
| `open(p, "w").write(s)` | `fs.writeFileSync(p, s)` | `fs::write(p, s)` | `File.writeText(path: p, text: s)` (effect `File.Write`) |
| `os.path.exists(p)` | `fs.existsSync(p)` | `Path::new(p).exists()` | `File.exists(p)` |
| `json.loads(s)` | `JSON.parse(s)` | `serde_json::from_str(s)` | `Json.parse(s)` |
| `json.dumps(v)` | `JSON.stringify(v)` | `serde_json::to_string(&v)` | `Json.stringify(v)` |
| `csv.reader(f)` | | `csv::Reader` | `Csv.parse(text)` |
| `subprocess.run(["ls", "-l"])` | `execFileSync("ls", ["-l"])` | `Command::new("ls").arg("-l")` | `Process.run(Process.command(program: "ls", arguments: ["-l"]))` (effect `Process.Run`) |
| `subprocess.run(s, shell=True)` | `execSync(s)` | `Command::new("sh").arg("-c")` | `Process.shell(s)` |
| `sys.argv[1:]` | `process.argv.slice(2)` | `env::args().skip(1)` | `Process.arguments()` (effect `Process.Environment`) |
| `os.environ.get(k)` | `process.env[k]` | `env::var(k)` | `Process.environmentVariable(k)` |
| `sys.exit(2)` | `process.exit(2)` | `std::process::exit(2)` | `Process.exit(2)` (effect `Process.Exit`) |
| `time.sleep(1)` | `setTimeout` | `thread::sleep` | `Clock.sleep(1000)` (effect `Clock.Time`) |
| `random.randint(1, 6)` | | `rng.gen_range(1..7)` | `Random.integer(low: 1, high: 7)` (effect `Random.Generate`; `high` is exclusive) |
| `random.random()` | `Math.random()` | `rng.gen::<f64>()` | `Random.float()` |
| `requests.get(url)` | `fetch(url)` | `reqwest::get(url)` | `Http.get(url)` (effect `Http.Connect`) |
| `assert a == b` | `assert.equal(a, b)` | `assert_eq!(a, b)` | `Assert.equal(actual: a, expected: b)` (in `@test` functions) |

Unlike passing an arbitrary environment map to a subprocess API, explicit `Process.Command.environment` settings cannot have names starting with `DYLD_` or `LD_` (ASCII case-insensitive), including `LD_RUN_PATH`, empty values, or same-value overwrites. Both `Process.run` and `Process.runAttached` return `IOErrorKind.InvalidInput` before starting a child. Inherited variables remain unchanged, including for `Process.shell`; accepting `Process.Run` still permits commands to set variables themselves. This is an input rule, not process isolation. See [Running external commands](idioms.md#running-external-commands).

Benitoite's declared effects can be inspected before execution: use `benitoite check --report=json main.bnt` or `benitoite test --check --report=json tests/`, then pass the reviewed atomic set to `run` or executing `test` with `--allow-effects=<names>`. Keep `Assert.Check` for tests. These are static upper bounds, not a sandbox or a promise of the intended result; the same set can permit different files or commands. A test-inspection report does not run tests. See [Inspecting before execution](inspection.md) for capability discovery and complete-result checks.

The same task in Benitoite, with the Python equivalent in comments:

```files
name: words.txt
the quick brown fox
jumps over the lazy dog
```

```benitoite
import Benitoite.Unofficial.IO.Console
import Benitoite.Unofficial.IO.File
import Benitoite.Unofficial.IO.Process

// counts = {}
// for word in open("words.txt").read().split():
//     counts[word] = counts.get(word, 0) + 1
// for word in sorted(counts):
//     if counts[word] > 1: print(f"{word} {counts[word]}")
function main() -> Unit uses File.Read, Console.Write, Process.Exit
  bind text <- try File.readText("words.txt") or die
  bind words <- List.filter(xs: String.split(s: String.replace(s: text, old: "\n", new: " "), separator: " "), p: lambda(value) return value <> "")
  bind counts <- List.fold(xs: words, initial: Map.empty(), f: lambda(acc, value)
    return Map.set(m: acc, k: value, value: Option.unwrapOr(o: Map.get(m: acc, k: value), fallback: 0) + 1)
  end lambda)
  Map.forEach(m: Map.filter(m: counts, p: lambda(key, value) return value > 1), f: lambda(key, value)
    Console.writeLine("${key} ${value}")
  end lambda)
  Console.writeLine("${List.length(words)} words, ${Map.size(counts)} distinct")
  return ()
end function
```

```output
the 2
9 words, 8 distinct
```
