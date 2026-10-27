# Benitoite.TaskGroup

Status: standard, in the prelude. It can be used without `import`.

Refer to its declarations as `TaskGroup.<name>`, for example `TaskGroup.open`.

Groups of tasks started one by one. The resource type `TaskGroup` is built in.

```benitoite
public function open() -> TaskGroup uses State
```

Returns an empty group. It can be written only as the expression of a `with` binding.

```benitoite
public function spawn[T, effect E](group: TaskGroup, action: function() -> T uses E) -> Task[T] uses State, E
```

Runs `action` as a task in `group` and returns without waiting for it.
