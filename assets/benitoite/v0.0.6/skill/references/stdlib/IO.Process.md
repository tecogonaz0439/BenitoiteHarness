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
  function run(command: Command) -> Result[Output, IOError]
  /// Starts the command with the standard streams of the script, waits for it, and returns its exit code.
  function runAttached(command: Command) -> Result[Integer, IOError]
  /// Runs `commandLine` with `/bin/sh -c` and returns the result as `run` does. Write only POSIX sh.
  function shell(commandLine: String) -> Result[Output, IOError]
end effect
```

Starting other programs.

```benitoite
public record Command
  program: String
  arguments: List[String]
  workingDirectory: Option[String]
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
