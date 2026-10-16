# Benitoite.Time

Status: unofficial. Import it with `import Benitoite.Unofficial.Time`. When the module becomes standard, the import name changes to `import Benitoite.Time`.

Refer to its declarations as `Time.<name>`, for example `Time.Instant`.

Points in time and calendar dates with a fixed offset from UTC. Time zone names are not used.

```benitoite
public record Instant
  unixNanoseconds: Integer
end record
```

A point in time, as nanoseconds since 1970-01-01T00:00:00Z.

```benitoite
public record DateTime
  year: Integer
  month: Integer
  day: Integer
  hour: Integer
  minute: Integer
  second: Integer
  nanosecond: Integer
  offsetMinutes: Integer
end record
```

A calendar date and time seen with an offset of `offsetMinutes` from UTC.

```benitoite
public function fromUnixSeconds(seconds: Integer) -> Instant
```

Returns the time `seconds` seconds after 1970-01-01T00:00:00Z.

```benitoite
public function fromUnixMilliseconds(milliseconds: Integer) -> Instant
```

Returns the time `milliseconds` milliseconds after 1970-01-01T00:00:00Z.

```benitoite
public function toUnixSeconds(t: Instant) -> Integer
```

Returns the seconds since 1970-01-01T00:00:00Z, rounded toward negative infinity.

```benitoite
public function toUnixMilliseconds(t: Instant) -> Integer
```

Returns the milliseconds since 1970-01-01T00:00:00Z, rounded toward negative infinity.

```benitoite
public function addMilliseconds(t: Instant, milliseconds: Integer) -> Instant
```

Returns the time `milliseconds` milliseconds after `t`.

```benitoite
public function differenceMilliseconds(a: Instant, b: Instant) -> Integer
```

Returns `a` minus `b` in milliseconds, rounded toward negative infinity.

```benitoite
public function toDateTime(t: Instant, offsetMinutes: Integer) -> DateTime
```

Returns the date and time of `t` seen with an offset of `offsetMinutes` from UTC.

```benitoite
public function fromDateTime(dt: DateTime) -> Result[Instant, String]
```

Returns the time that `dt` stands for, or the reason when the date does not exist or is out of range.

```benitoite
public function formatISO8601(t: Instant, offsetMinutes: Integer) -> String
```

Writes `t` in ISO 8601 form, such as 2026-09-28T12:34:56.789+09:00.

```benitoite
public function parseISO8601(text: String) -> Result[Instant, String]
```

Reads ISO 8601 text that has an offset or `Z`.

```benitoite
public function format(t: Instant, offsetMinutes: Integer, pattern: String) -> Result[String, String]
```

Writes `t` by `pattern` with strftime specifiers such as %Y and %m. Unknown specifiers give `Result.Error`.
