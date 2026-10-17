# Benitoite.Task

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `Task.<name>`, for example `Task.all`.

Running tasks concurrently and waiting for their results. The type `Task` is built in.

```benitoite
public function all[T, effect E](actions: List[function() -> T uses E]) -> List[T] uses E
```

Runs each action as a task and returns their results in the order of `actions`.

```benitoite
public function allOk[T, X, effect E](actions: List[function() -> Result[T, X] uses E]) -> Result[List[T], X] uses E
```

Runs each action as a task. Returns all values when every task returns `Result.Ok`;
otherwise returns the `Result.Error` of the earliest failing action in `actions`.

```benitoite
public function race[T, effect E](actions: List[function() -> T uses E]) -> Option[T] uses Clock.Time, State, E
```

Runs each action as a task and returns the result of the first to finish, cancelling the others.

```benitoite
public function withTimeout[T, effect E](milliseconds: Integer, action: function() -> T uses E) -> Option[T] uses Clock.Time, State, E
```

Runs `action` as a task and returns its result if it finishes within `milliseconds`; otherwise cancels it.

```benitoite
public function await[T](task: Task[T]) -> T uses State
```

Waits for `task` to finish and returns its result.
