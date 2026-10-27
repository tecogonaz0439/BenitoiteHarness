# Benitoite.IO.Process

Status: unofficial. Import it with `import Benitoite.Unofficial.IO.Process`. When the module becomes standard, the import name changes to `import Benitoite.IO.Process`.

Refer to its declarations as `Process.<name>`, for example `Process.Environment`.

The running process: its arguments and its exit.

```benitoite
public effect Environment
  /// Returns the command-line arguments given to the script.
  function arguments() -> List[String]
  /// Returns the absolute path of the directory of the script that started the run.
  function scriptDirectory() -> String
  /// Returns the value of the environment variable, or `Option.None` when it is not defined.
  function environmentVariable(name: String) -> Result[Option[String], IOError]
  /// Returns the absolute path of the working directory.
  function workingDirectory() -> Result[String, IOError]
end effect
```

Reading the environment of the process.

```benitoite
public effect Exit
  /// Ends the run with the exit status `code`. It does not return.
  function exit[T](code: Integer) -> T
end effect
```

Ends the run; `or die` also reports its cause and optional message to standard error without Console.Write. A language handler may replace the exit operation.

```benitoite
public effect Run
  /// Starts the command without a shell, waits for it, and returns its exit code and output.
  /// Returns Result.Error with IOErrorKind.InvalidInput before starting a child or arranging its pipes if Command.environment explicitly sets a name starting with DYLD_ or LD_ (ASCII case-insensitive).
  /// This includes LD_RUN_PATH used by build tools, lowercase names, empty values, and resetting the same inherited value, on every OS.
  /// Existing invalid-input checks (NUL, empty names, and `=` in names) take precedence. The error names the first forbidden variable in lexical order of the original names, without its value.
  /// Inherited variables are unchanged. Allowing Process.Run still permits env or a shell to set variables itself; this rule does not isolate child processes or control PATH, grandchildren, or every Windows DLL search.
  function run(command: Command) -> Result[Output, IOError]
  /// Starts the command with the standard streams of the script, waits for it, and returns its exit code.
  /// Rejects explicit DYLD_ and LD_ settings with IOErrorKind.InvalidInput before starting a child, with the same ASCII case-insensitive rules and error ordering as run, including LD_RUN_PATH, empty values, and same-value overwrites; inherited variables are unchanged.
  function runAttached(command: Command) -> Result[Integer, IOError]
  /// Runs `commandLine` with `/bin/sh -c` and returns the result as `run` does. Write only POSIX sh.
  /// Inherits the parent's environment unchanged. The Command.environment input rule does not prevent a shell command from setting loader variables; enforce isolation in the harness or OS.
  function shell(commandLine: String) -> Result[Output, IOError]
end effect
```

Starting other programs.

```benitoite
public record Command
  program: String
  arguments: List[String]
  workingDirectory: Option[String]
  /// Adds or overwrites variables in the inherited environment. run and runAttached reject names starting with DYLD_ or LD_, ignoring ASCII case, including LD_RUN_PATH, empty values, and same-value overwrites. This does not isolate child processes or remove inherited variables.
  environment: Map[String, String]
  input: Option[String]
end record
```

A command to start, made with `Process.command` and changed with record update.

```benitoite
public record Output
  exitCode: Integer
  standardOutput: String
  standardError: String
end record
```

The result of a finished command.

```benitoite
public function command(program: String, arguments: List[String]) -> Command
```

Returns a command that uses the base directory, adds no environment variables, and gives no input.
