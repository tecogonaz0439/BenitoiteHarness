# Benitoite.Regex

Status: unofficial. Import it with `import Benitoite.Unofficial.Regex`. When the module becomes standard, the import name changes to `import Benitoite.Regex`.

Refer to its declarations as `Regex.<name>`, for example `Regex.compile`.

Regular expressions. A single match (`isMatch`, `find`, `replaceFirst`) takes time proportional to the product of the input length and pattern size in the worst case.
Repeated matching (`findAll`, `replaceAll`, `replaceAllWith`, `split`) can take time proportional to the square of the input length times the pattern size in the worst case. The types `Pattern` and `Match` are built in.

```benitoite
public function compile(source: String) -> Result[Pattern, String]
```

Builds a regular expression. Returns the reason as `Result.Error` when the syntax is wrong.

```benitoite
public function isMatch(pattern: Pattern, text: String) -> Boolean
```

Returns whether some part of `text` matches.

```benitoite
public function find(pattern: Pattern, text: String) -> Option[Match]
```

Returns the leftmost match.

```benitoite
public function findAll(pattern: Pattern, text: String) -> List[Match]
```

Returns the non-overlapping matches from the left.

```benitoite
public function matchText(m: Match) -> String
```

Returns the matched text.

```benitoite
public function matchByteStart(m: Match) -> Integer
```

Returns the byte position where the match starts.

```benitoite
public function matchByteEnd(m: Match) -> Integer
```

Returns the byte position where the match ends. The match is from the start up to, but not including, the end.

```benitoite
public function group(m: Match, index: Integer) -> Option[String]
```

Returns the text of capture group `index`, where 0 is the whole match.

```benitoite
public function namedGroup(m: Match, name: String) -> Option[String]
```

Returns the text of the capture group named `name`.

```benitoite
public function replaceFirst(pattern: Pattern, text: String, replacement: String) -> String
```

Replaces the first match with `replacement`, taken literally.

```benitoite
public function replaceAll(pattern: Pattern, text: String, replacement: String) -> String
```

Replaces every match with `replacement`, taken literally.

```benitoite
public function split(pattern: Pattern, text: String) -> List[String]
```

Splits `text` at each match.

```benitoite
public function replaceAllWith[effect E](pattern: Pattern, text: String, f: function(matched: Match) -> String uses E) -> String uses E
```

Replaces every match with `f` applied to it. `f` is called for each match from the left.
