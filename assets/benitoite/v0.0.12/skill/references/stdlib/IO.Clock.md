# Benitoite.IO.Clock

Status: unofficial. Import it with `import Benitoite.Unofficial.IO.Clock`. When the module becomes standard, the import name changes to `import Benitoite.IO.Clock`.

Refer to its declarations as `Clock.<name>`, for example `Clock.Time`.

Reading the clock and waiting for time to pass.

```benitoite
public effect Time
  /// Stops the calling task for at least `milliseconds` while other tasks run. Returns at once for 0 or less.
  function sleep(milliseconds: Integer) -> Unit
  /// Returns milliseconds from an unspecified start. The value never decreases within a run.
  function monotonicMilliseconds() -> Integer
  /// Returns the current time.
  function now() -> TimeValue.Instant
  /// Returns the difference of local time from UTC in minutes, east positive. Returns 0 when it is unknown.
  function localOffsetMinutes() -> Integer
end effect
```

Operations that depend on time.
