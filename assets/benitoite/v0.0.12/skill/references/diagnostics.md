# Benitoite diagnostic codes

Each diagnostic code with its message template and explanation. Words in braces such as `{name}` are filled in when the diagnostic is reported.

For argument-label errors, compare the available parameter names in declaration order, but keep argument expressions in source order when adding labels. For E0454, match callback names at every position or use an explicit lambda wrapper. For E0455, add a function-type annotation with names; the order of call labels does not define a contract. See [Common mistakes](common-mistakes.md) and [Idioms](idioms.md#calls-callback-contracts-holes-and-pipes) for examples.

## E0101

- Kind: check (error)
- Message: cannot read file `{path}`
- `reason`: {reason}
- `imported`: the module is imported here

The source file could not be read. Check the path and the file permissions. A file larger than 256 MiB, or a file that makes all sources of the program larger than 512 MiB, is also reported with this code.

## E0102

- Kind: check (error)
- Message: source file is not valid UTF-8
- Label: invalid UTF-8 byte sequence

Source files must be encoded in UTF-8.

## E0103

- Kind: check (error)
- Message: character {char} is not allowed here
- Label: not allowed outside literals and comments
- `allowed`: only printable ASCII characters, spaces, tabs, and newlines may appear outside string literals, character literals, and comments

Outside literals and comments, only printable ASCII, spaces, tabs, and newlines are allowed, so that invisible or look-alike characters cannot change the meaning of code.

## E0104

- Kind: check (error)
- Message: invisible control character {char} is not allowed
- `escape`: to include it in a value, write the escape `{escape}` in a string or character literal

Bidirectional control characters and a byte order mark after the start of the file are not allowed anywhere, even in literals, comments, and the shebang line.

## E0105

- Kind: check (error)
- Message: unexpected symbol `{symbol}`
- Label: not a valid token

This symbol is not part of the language.

## E0106

- Kind: check (error)
- Message: semicolons are not used
- Label: remove this `;`
- `newline`: separate statements with a newline

Statements are separated by newlines, not semicolons.

## E0107

- Kind: check (error)
- Message: carriage return without a following line feed
- Label: expected LF after CR

Line breaks must be LF or CR LF.

## E0108

- Kind: check (error)
- Message: unterminated string literal
- Label: this string is not closed on this line
- `one_line`: a string literal must end on the line where it starts; write `\n` for a line break
- `multi_line`: for text with line breaks, use a multi-line string that starts and ends with `"""`

String literals must be closed with `"` on the same line. Multi-line strings are written with `"""`.

## E0109

- Kind: check (error)
- Message: unterminated character literal
- Label: this character literal is not closed

Character literals must be closed with `'`.

## E0110

- Kind: check (error)
- Message: invalid escape sequence `{escape}`
- Label: invalid escape
- `valid_string`: valid escapes are `\n`, `\r`, `\t`, `\\`, `\"`, `\$`, and `\u{...}`
- `valid_char`: valid escapes are `\n`, `\r`, `\t`, `\\`, `\"`, `\'`, `\$`, and `\u{...}`
- `unicode`: `\u{...}` must contain 1 to 6 hexadecimal digits naming a Unicode scalar value

Only the listed escape sequences may follow `\`.

## E0112

- Kind: check (error)
- Message: empty character literal

A character literal must contain exactly one character.

## E0113

- Kind: check (error)
- Message: character literal contains more than one character
- `use_string`: use a string literal ("...") for text made of more than one character

A character literal holds exactly one Unicode scalar value. Characters built from several scalar values must be written as strings.

## E0114

- Kind: check (error)
- Message: control character {char} must be escaped
- `escape`: write it as `{escape}`

Control characters other than tab must be written with an escape sequence inside literals.

## E0115

- Kind: check (error)
- Message: malformed integer literal `{text}`
- `leading_zero`: decimal literals other than `0` must not start with `0`
- `underscore`: `_` must be placed between two digits
- `digit`: invalid digit for this base
- `no_digits`: the base prefix must be followed by digits
- `prefix`: base prefixes must be lowercase: `0x`, `0o`, `0b`
- `suffix`: a number must not be directly followed by a letter, a digit of another base, or `_`; separate words with a space, such as `2 mod 3`

Integer literals are decimal, or hexadecimal, octal, or binary with a lowercase prefix.

## E0116

- Kind: check (error)
- Message: malformed floating-point literal `{text}`
- `missing_digits`: digits are required on both sides of `.`
- `leading_zero`: the integer part must not start with `0` unless it is `0`
- `underscore`: `_` must be placed between two digits
- `exponent`: the exponent needs at least one digit
- `suffix`: a number must not be directly followed by a letter or `_`; separate words with a space, such as `2 mod 3`
- `second_dot`: a number cannot contain a second `.` followed by digits

Floating-point literals have a decimal point or an exponent. Each part must contain digits.

## E0118

- Kind: check (error)
- Message: empty string interpolation
- Label: `${}` contains no expression
- `escape`: write `\$` to include a literal `$` followed by `{`

An interpolation `${...}` must contain an expression.

## E0119

- Kind: check (error)
- Message: string interpolation is not closed on this line
- Label: this `${` is not closed
- `one_line`: an interpolation must end with `}` on the line where it starts, also inside multi-line strings

The expression of an interpolation is written on one line and closed with `}`.

## E0120

- Kind: check (error)
- Message: comments are not allowed inside string interpolation
- `move`: move the comment outside the string literal

`//` inside `${...}` would hide the rest of the line, so comments cannot be written there.

## E0121

- Kind: check (error)
- Message: text after the opening `"""` of a multi-line string
- `newline`: start the text on the line after `"""`

The first line of a multi-line string starts on the line after the opening `"""`.

## E0122

- Kind: check (error)
- Message: this line does not start with the indentation of the closing `"""`
- `closing`: the closing `"""` is here
- `same_chars`: indent the line with the same spaces or tabs as the closing line

Every non-blank line of a multi-line string must start with the indentation of the closing line. The indentation is removed from every line.

## E0123

- Kind: check (error)
- Message: unterminated multi-line string
- Label: this string is not closed
- `close`: close the string with `"""` at the start of a line after indentation

A multi-line string ends with `"""` after indentation at the start of a line. The expression may continue after the closing quotes.

## E0124

- Kind: check (error)
- Message: malformed Decimal literal `{text}`
- `exponent`: a Decimal literal cannot have an exponent
- `suffix_case`: the Decimal suffix is a lowercase `m`
- `base`: a Decimal literal must be written in decimal
- `missing_digits`: digits are required on both sides of `.`
- `leading_zero`: the integer part must not start with `0` unless it is `0`
- `underscore`: `_` must be placed between two digits
- `suffix`: `m` must not be directly followed by a letter, a digit, or `_`; separate words with a space

Decimal literals are decimal numbers followed by a lowercase `m`, such as `12.50m`.

## E0125

- Kind: check (error)
- Message: cannot write formatted file `{path}`
- `reason`: {reason}
- `temp_left`: the temporary file `{temp}` could not be removed

`benitoite fmt` writes the formatted source to a temporary file in the same directory and renames it over the original file. It refuses to replace a read-only file, a file with more than one hard link, or a file owned by another user. Writing or renaming failed or was refused, so the original file was left unchanged. Check the permissions, links, and owner of the file and its directory.

## E0126

- Kind: check (error)
- Message: cannot write `{path}`
- `reason`: {reason}
- `temp_left`: temporary file left at `{temp}`
- `create_rollback`: could not remove newly created `{config}`: {rollback_reason}; the entry file was not changed
- `rollback`: could not restore `{config}`: {rollback_reason}; declarations may remain in both files and cause E0911

The package command could not safely replace or create a file. Check file permissions and the reported reason.

## E0127

- Kind: check (error)
- Message: invalid numeric suffix in `{text}`
- `integer_form`: `s8/s16/s32/s64/u8/u16/u32/u64` require an integer literal
- `float_form`: `f32/f64` require a decimal point or an exponent; write a form such as `1.0f32`

Integer suffixes can follow decimal, hexadecimal, octal, or binary integers. Floating-point suffixes require a decimal point or an exponent. Suffixes cannot be combined with each other or with `m`.

## E0201

- Kind: check (error)
- Message: expected {expected}, found {found}
- Label: unexpected {found}
- `lambda_line`: put `end lambda` on the same physical line as the body, or start the body on the line after the lambda header
- `return_start`: return starts here
- `return_value_line`: start the return value on the same physical line as `return`
- `die_action`: use `try value or die` or `try value or die(message)` to exit on failure
- `type_arguments`: call functions without type arguments, such as `Map.empty()`; write the type on the binding, such as `bind m: Map[String, Integer] <- Map.empty()`, and use `List.get` to read list elements

The parser found a token that cannot appear here.

## E0202

- Kind: check (error)
- Message: comparison operators cannot be chained
- Label: second comparison
- `and`: write the comparisons separately, such as `a < b and b < c`

Comparison operators are non-associative.

## E0203

- Kind: check (error)
- Message: `.` cannot follow a value
- Label: method-call syntax is not supported
- `pipe`: pass the value with a pipe, such as `xs |> List.map(f: f)`
- `field`: read a record field with its field function, such as `Person.name(p)` or `p |> Person.name`

A dot is only used after a module, type, record, trait, or effect name, such as `List.map` or `Shape.Circle`.

## E0204

- Kind: check (error)
- Message: `_` can only be a direct argument of a call
- Label: not a call argument
- `record`: a record construction is not a call; write a lambda instead

The placeholder `_` builds a function from a call, such as `clamp(lo: 0, value: _, hi: 100)`.

## E0206

- Kind: check (error)
- Message: constructor `{name}` without fields must not have parentheses
- `remove`: remove `()`

Constructors without fields are declared and written without parentheses.

## E0207

- Kind: check (error)
- Message: function names cannot be qualified with a module name
- `remove`: remove `{module}.` from the name

Top-level functions are declared with a plain name. The module of a function is the file it is written in.

## E0208

- Kind: check (error)
- Message: the program is nested too deeply
- Label: nesting limit of {limit} reached here

The parser limits the depth of nested syntax.

## E0209

- Kind: check (error)
- Message: a function type with `uses` must be parenthesized here
- Label: read as part of the `uses` list
- `paren`: write the function type in parentheses, such as `function((function() -> Unit uses Console.Write), List[Integer]) -> Unit`

`uses` reads as many effect names as possible, so a function type with `uses` inside a list of types must be parenthesized.

## E0210

- Kind: check (error)
- Message: `_` cannot be a lambda parameter
- `name`: use a name that starts with `_`, such as `_unused`

Unused lambda parameters are named with a leading `_`.

## E0211

- Kind: check (error)
- Message: `{symbol}` is not an operator in Benitoite
- Label: not a Benitoite operator
- `equal`: write `=` to compare values
- `not_equal`: write `<>` for inequality
- `and`: write `and`
- `or`: write `or`
- `not`: write `not`
- `remainder`: write `mod` for the remainder
- `question`: write `try` before the expression, such as `try parse(text) or return`
- `fat_arrow`: write the branch as `case pattern -> expression`

Benitoite writes these operators as words or with other symbols.

## E0212

- Kind: check (error)
- Message: blocks are not written with braces
- Label: braces are not used
- `end`: close the block with `end {construct}` instead of `}`
- `open`: start the block with the keyword of the construct; it ends with `end` and the construct name, such as `end function` or `end if`

Blocks start with a keyword and end with `end` followed by the name of the construct.

## E0213

- Kind: check (error)
- Message: `end {found}` does not close `{expected}`
- Label: expected `end {expected}`
- `lambda_line`: put `end lambda` on the same physical line as the body, or start the body on the line after the lambda header
- `opened`: the `{expected}` block starts here
- `replace`: write `end {expected}`

A block is closed by `end` followed by the name of the construct that opened it.

## E0214

- Kind: check (error)
- Message: `{construct}` is not closed
- Label: expected `end {construct}` before this
- `opened`: the `{construct}` block starts here

Every block must be closed with `end` and the name of its construct.

## E0215

- Kind: check (error)
- Message: `with` must be on the same line as `match`
- Label: `with` starts a new line here
- `same_line`: put `with` at the end of the previous line, such as `match value with`

A line break after the value of `match` ends the expression, so `with` must follow on the same line.

## E0216

- Kind: check (error)
- Message: `{word}` is a keyword and cannot be used as a name
- Label: keyword used as a name
- `rename`: choose another name
- `renamed`: `{old}` was renamed to `{new}` in 0.0.1
- `alias`: use `{new}`

Keywords cannot be used as names of variables, functions, or types.

## E0217

- Kind: check (error)
- Message: parenthesized lists of values are not tuples
- `pair`: use `Pair`, such as `Pair(a, b)` or the type `Pair[A, B]`
- `triple`: use `Triple`, such as `Triple(a, b, c)` or the type `Triple[A, B, C]`
- `record`: declare a `record` for four or more values

Benitoite has no tuple syntax. `Pair` and `Triple` hold two or three values, and records hold more.

## E0218

- Kind: check (error)
- Message: `{keyword}` cannot be used inside {context}
- `lazy_body`: a `lazy` body produces one value; compute the value with an expression
- `guard`: a guard is a condition; check the value in the branch body instead
- `die_message`: use a pure String expression for the message of `or die`
- `lambda`: `{keyword}` is allowed inside a `lambda ... end lambda` written there

`return` and `try` cannot leave a `lazy` body, a `match` guard, or the message of `or die`, except inside a `lambda ... end lambda` written there. The function and the arguments of a call with `_` placeholders are not exempt.

## E0219

- Kind: check (error)
- Message: `import` must come before other declarations
- Label: import after a declaration
- `first`: the first declaration is here

All `import` declarations are written at the start of the file.

## E0220

- Kind: check (error)
- Message: an attribute argument must be a string literal
- `plain`: write a plain string literal without interpolation, such as `@deprecated("use format instead")`

Attribute arguments are string literals, and interpolation is not allowed in them.

## E0221

- Kind: check (error)
- Message: `public` is not allowed on {what}
- `remove`: remove `public`
- `implement`: an implementation is visible wherever its trait and type are visible

Implementations and the functions inside them are not marked `public`.

## E0222

- Kind: check (error)
- Message: `{name}` is not a built-in constraint
- `builtin`: the lowercase constraints are `equality` and `key`; traits are written with an uppercase name
- `ordered`: `ordered` can only be used in the standard library

A lowercase name in a constraint must be one of the built-in constraints.

## E0223

- Kind: check (error)
- Message: the built-in constraint `{name}` cannot be a supertrait
- `remove`: remove `{name}` from the supertraits

Only traits can be supertraits of a trait.

## E0224

- Kind: check (error)
- Message: a record update must change at least one field
- `use_value`: use `{value}` directly

`Record(..value)` without fields would copy the value unchanged.

## E0225

- Kind: check (error)
- Message: a record pattern must name at least one field
- `wildcard`: use the pattern `_` to match any value

`Record(..)` without fields matches every value; `_` says this directly.

## E0226

- Kind: check (error)
- Message: a list literal can contain at most one spread
- Label: second spread
- `first`: first spread here
- `concat`: join the lists with `List.concatenate`

A list literal may contain one `..e`. Join more lists with `List.concatenate`.

## E0227

- Kind: check (error)
- Message: `{written}` is not the spread syntax
- `spread`: write `..{name}` to spread a list

A list is spread into a list literal with `..e`.

## E0228

- Kind: check (error)
- Message: this documentation comment is not attached to a declaration
- `plain`: write a normal comment with `//`

A `///` comment must be directly followed by the declaration it documents.

## E0229

- Kind: check (error)
- Message: a module documentation comment must be at the start of the file
- `plain`: write a normal comment with `//`

`//!` comments come before `import` declarations and other declarations.

## E0230

- Kind: check (error)
- Message: `let` is not used to bind names
- `bind`: write `bind {name} <- {value}`
- `shadow`: if `{name}` is already a local name here, write `shadow {name} <- {value}`

A binding statement is written `bind x <- e`, or `shadow x <- e` to hide a visible local name.

## E0231

- Kind: check (error)
- Message: `case ... of` is not the syntax of `match`
- `match_with`: write `match {value} with`

Pattern matching is written `match value with`, followed by branches `case pattern ->`.

## E0232

- Kind: check (error)
- Message: `when` does not start a branch
- `case`: write the branch as `case {pattern} ->`

Branches of `match` and clauses of `handle` start with `case` and use `->`.

## E0233

- Kind: check (error)
- Message: an algebraic data type is declared with `data`
- `data`: write `data {name}` and close it with `end data`

`type` declares a type alias. Types with constructors are declared with `data ... end data`.

## E0234

- Kind: check (error)
- Message: the return type is written after `->`
- Label: expected `->`
- `arrow`: replace `:` with `->`

The return type of a function, lambda, method, or operation follows `->`.

## E0235

- Kind: check (error)
- Message: `try` does not start a block
- `result`: there are no exceptions; return a `Result` and write `try` before a call that returns `Result` or `Option`

`try` returns an `Error` or `None` to the caller early. Errors are values, and there is no exception-catching syntax.

## E0236

- Kind: check (error)
- Message: alternatives in a pattern are separated by commas
- `comma`: write `case {first}, {second} ->`

A branch that accepts several patterns lists them separated by `,`.

## E0237

- Kind: check (error)
- Message: a type alias cannot list constructors
- `data`: declare a `data` type with one constructor per line, closed by `end data`

`type` gives another name to an existing type. Constructors are declared in `data`.

## E0238

- Kind: check (error)
- Message: a branch must start with `case`
- `case`: write the branch as `case {pattern} -> ...`

Every branch of `match` starts with `case` and uses `->`.

## E0239

- Kind: check (error)
- Message: `{word}` is not Benitoite syntax
- `switch`: write `match {value} with`
- `default`: write `case _ ->`

Pattern matching is written with `match ... with` and branches `case pattern ->`.

## E0240

- Kind: check (error)
- Message: `break` is not needed in a branch
- `remove`: remove `break`; a branch never continues into the next branch

Branches of `match` do not fall through.

## E0241

- Kind: check (error)
- Message: a guard is written with `if`
- `if`: write `case {pattern} if {condition} ->`

A branch condition follows the pattern after `if`.

## E0242

- Kind: check (error)
- Message: a range is written `low..high`
- `range`: write `{low}..{high}`; both ends are included

Range patterns include both ends and are written with `..` only.

## E0243

- Kind: check (error)
- Message: a lambda is written as `lambda(parameters)` followed by a body
- `lambda`: write `lambda({params}) return {body}`; keep the entire lambda on one physical line, or start the body on the next line and add `end lambda`

For one statement, prefer a short lambda: `lambda(x) return x + 1`.

## E0244

- Kind: check (error)
- Message: modules are imported by name
- `by_name`: write `import {module}`, a module name relative to the directory of the starting file

`import` names a module, such as `import Lib.Text` for the file `Lib/Text.bnt`.

## E0245

- Kind: check (error)
- Message: `implement` must name a trait
- `functions`: write top-level functions instead, such as `function describe(p: {name}) -> String`

`implement` implements a trait for a type, such as `implement Show[Person]`.

## E0246

- Kind: check (error)
- Message: `with` does not install a handler
- `handle`: write `handle ... with case operation(args) -> ... end handle`

Handlers are written with `handle ... with` and clauses `case`. `with` also binds resources.

## E0247

- Kind: check (error)
- Message: attributes are written with `@` and a lowercase name
- `test`: write `@test`
- `other`: the attributes are `@test`, `@test("...")`, `@deprecated("...")`, and `@external("wasm", "...", "...")`

Attributes are written on the line before a declaration.

## E0248

- Kind: check (error)
- Message: a type alias is written `type Name = Type`
- `alias`: write `type {name} = ...`

`type` declares a type alias; there is no `alias` keyword.

## E0249

- Kind: check (error)
- Message: documentation comments are written with `///`
- `doc`: write each line of the documentation with `///` before the declaration

Documentation comments start with `///`, and module documentation starts with `//!`.

## E0250

- Kind: check (error)
- Message: a list pattern can contain at most one `..`
- Label: second `..`
- `first`: first `..` here

A list pattern has one rest part, such as `[first, ..rest]`.

## E0251

- Kind: check (error)
- Message: `fn` is not used for function declarations or lambdas
- `function`: write `function {signature} ... end function`
- `lambda`: write `lambda{signature} ... end lambda`, using `return` for the result

Declare functions with `function` and write anonymous functions with `lambda`.

## E0252

- Kind: check (error)
- Message: `{item}` must come at the start of the file, before `import` and declarations
- Label: written after an import or a declaration
- `first`: the first import or declaration is here

Write `language`, `dependency`, and `package` before the first `import` or declaration.

## E0253

- Kind: check (error)
- Message: `public(...)` accepts only `package`
- Label: expected `package`
- `package`: write `package` inside the parentheses

Use `public(package)` to share a declaration only within its package.

## E0254

- Kind: check (error)
- Message: a lambda without `end lambda` must fit on one physical line
- `crossed`: the lambda continues on another physical line
- `end`: start the body on the next line and add `end lambda` to use a multiline body

A lambda without its terminator contains one statement and fits on one physical line.

## E0256

- Kind: check (error)
- Message: parenthesize the inner lambda when either lambda omits `end lambda`
- `paren`: enclose the inner lambda in parentheses

A directly nested lambda needs parentheses when either lambda omits its terminator.

## E0257

- Kind: check (error)
- Message: each function-type parameter must have a name
- `named`: write `function(name: Type) -> Result`

Function-type parameter names are part of the contract. Use the names required by the surrounding contract.

## E0258

- Kind: check (error)
- Message: invalid failure action after `try`
- `return_value`: the `or return` action takes no value
- `die_message`: write exactly one message expression inside the parentheses
- `die_action`: use `or die` or `or die(message)` to exit on failure

Write `or return` without a value to return the failure, or `or die` with an optional parenthesized message to exit.

## E0259

- Kind: check (error)
- Message: parenthesize an explicit `try` expression before continuing it
- `paren`: write `(try ... or return)` or `(try ... or die)` before applying another operator or call

Enclose an explicit `try` expression in parentheses before continuing it.

## E0260

- Kind: check (error)
- Message: invalid documentation example: {reason}
- `syntax`: use `benitoite test`, `benitoite check`, `benitoite error` followed by `diagnostic`, or `benitoite` followed by `output`

Documentation examples use fenced blocks. Error examples require a real E-code on its own diagnostic line; run examples require output. Files and nonzero exit expectations are not supported in comments.

## E0261

- Kind: check (error)
- Message: invalid file-level `uses` declaration
- Label: file-level effects declared here
- `position`: write one file-level `uses` after all imports and before ordinary declarations
- `empty`: a file-level `uses` requires at least one effect; omit the declaration for an empty set
- `function_line`: write a function's `uses` on the same physical line as its return type

A file-level `uses` is a single nonempty declaration after imports. Its original location is retained separately from each function's contract.

## E0262

- Kind: check (error)
- Message: invalid interpolation format
- Label: invalid format here
- `empty`: omit the colon for an interpolation without a format
- `whitespace`: remove whitespace from the format
- `minus`: omit `-`; only `+` can be written as a sign flag
- `kind`: use one of `d`, `b`, `o`, `x`, `X`, `f`, `s`, or `c` in this order: align, sign, #, zero, width, precision, kind
- `width`: write an ASCII width from 1 to 65535
- `precision`: write an ASCII precision from 0 to 28 after `.`
- `fixed`: precision requires the kind `f`; append `f` if that is intended
- `align_width`: alignment requires a width
- `zero_width`: zero padding requires a width
- `zero_align`: zero padding cannot be combined with explicit alignment
- `alternate`: `#` requires `b`, `o`, `x`, or `X`

After a complete expression, the colon starts a static format. Its syntax, bounds, and flag combinations are checked separately from compatibility with the expression's type.

## E0301

- Kind: check (error)
- Message: cannot find `{name}` in this scope
- Label: not found
- `similar`: a similar name exists: {candidates}
- `renamed`: `{old}` was renamed to `{new}` in 0.0.1

The name is not bound by a local binding, a function, or a constant visible here.

## E0302

- Kind: check (error)
- Message: cannot find `{name}`
- Label: not found
- `similar`: a similar name exists: {candidates}
- `renamed`: `{old}` was renamed to `{new}` in 0.0.1

The name before `.` must be a module imported in this file, a prelude module, a type, a record, a trait, or an effect.

## E0303

- Kind: check (error)
- Message: `{module}` has no member `{name}`
- Label: not found in `{module}`
- `similar`: a similar name exists: {candidates}
- `length`: strings have no unit-less length; use `{module}.byteLength` (bytes) or `{module}.characterCount` (characters)
- `slice`: use `{module}.byteSlice` (byte positions) or `{module}.characterSlice` (character positions)
- `index_of`: use `{module}.byteIndexOf`, which returns a byte position
- `unwrap`: there is no `{name}`; use `{module}.unwrapOr` or `match`
- `renamed`: `{old}` was renamed to `{module}.{new}` in 0.0.1
- `alias`: use `{module}.{new}`
- `import_member`: add `import {full}` and use `{local}.`
- `use_imported`: `{full}` is imported as `{local}`; use `{local}.`
- `import_member_as`: add `import {full} as <name>` and use that name, since `{local}` is already used in this file
- `construct`: build a record with its field names, such as `{record}({fields})`
- `members`: `{module}` has: {names}
- `members_more`: `{module}` has: {names}, and {remaining} more
- `shadowed`: `{first}` here is the {shadow_kind} declared in this file, which hides the prelude {prelude_kind} `{first}`
- `shadowing`: `{first}` declared here
- `shadowed_import`: `{first}` here is the module imported in this file, which hides the prelude {prelude_kind} `{first}`
- `imported`: `{first}` imported here
- `prelude`: write `Benitoite.{first}` to use the prelude {prelude_kind}
- `not_reexported`: names imported by `{module}` are not visible through it; import that module directly

The module, type, record, trait, or effect does not define this name.

## E0304

- Kind: check (error)
- Message: `{name}` is {found_kind}, not {expected_kind}
- `construct`: build a record with its field names, such as `{record}({fields})`

Each position accepts only one kind of name.

## E0305

- Kind: check (error)
- Message: the name `{name}` is defined more than once
- Label: redefined here
- `first`: first defined here
- `import`: give the import another name with `as`, such as `{suggestion}`
- `rename`: rename the declaration `{name}`; the module `{module}` declares built-in effects and cannot be renamed with `as`

Types, records, type aliases, traits, effects, and import names share one namespace in a module.

## E0306

- Kind: check (error)
- Message: `{name}` is defined more than once
- Label: redefined here
- `first`: first defined here

Functions, constants, and effect operations share one namespace in a module.

## E0309

- Kind: check (error)
- Message: the constructor `{name}` is defined more than once in `{ty}`
- Label: redefined here
- `first`: first defined here

Constructor names must be unique within a type.

## E0310

- Kind: check (error)
- Message: the parameter `{name}` is declared more than once
- Label: redeclared here
- `first`: first declared here

Parameter names must be unique within a function, lambda, or function type.

## E0311

- Kind: check (error)
- Message: `{name}` is bound more than once in the same pattern
- Label: bound again here
- `first`: first bound here

A pattern cannot bind the same variable twice.

## E0312

- Kind: check (error)
- Message: the type parameter `{name}` is declared more than once
- Label: redeclared here
- `first`: first declared here

Type parameter names must be unique within a list.

## E0313

- Kind: check (error)
- Message: the type parameter `{name}` has the same name as a top-level declaration, an imported name, or a prelude name
- `declared`: the other declaration is here

Type parameters and effect variables cannot reuse the module's top-level uppercase names, imported names, prelude names, or `Benitoite`.

## E0314

- Kind: check (error)
- Message: `{name}` is an effect and cannot be used as a type
- `uses_only`: effects and effect variables can only appear after `uses`

Effect names and effect variables are not types.

## E0315

- Kind: check (error)
- Message: `{name}` is not an effect
- Label: expected an effect name or an effect variable
- `qualify`: effects of other modules are written with the module name, such as `Console.Write`

After `uses`, write effects declared in this module, `State`, effects qualified with a module name, or effect variables.

## E0316

- Kind: check (error)
- Message: `{name}` is not an effect
- Label: read as part of the `uses` list
- `paren`: if `{name}` belongs to the surrounding list, parenthesize the function type that has `uses`, such as `function((function() -> Unit uses Console.Write), Integer) -> Unit`

`uses` reads as many comma-separated names as possible, so a following list element may be taken as an effect.

## E0318

- Kind: check (error)
- Message: cannot find module `{module}`
- Label: no file `{path}` under {where}
- `similar`: a similar module exists: {candidates}
- `case_only`: the directory has `{actual}`, which differs only in upper and lower case
- `package`: modules of the package `{package}` are imported by their path from the package root `{root}`

A module name refers to a file under the starting root or a declared package root, such as `Lib/Text.bnt` for `Lib.Text`.

## E0319

- Kind: check (error)
- Message: the module `{module}` is outside {where}
- `resolved`: `{path}` resolves to `{target}`, outside `{root}`

An imported module must stay within its root directory or package root after resolving symbolic links.

## E0320

- Kind: check (error)
- Message: the module where execution starts cannot be imported
- `entry`: `{module}` is the starting file of this program

The starting module is not a library. Move the shared declarations to another module.

## E0321

- Kind: check (error)
- Message: `{module}` is not a standard library module
- `similar`: a similar module exists: {candidates}
- `last_segment`: use {candidates}
- `root_file`: names that start with `Benitoite` always refer to the standard library, so `{path}` cannot be imported
- `unofficial`: this module is unofficial in this version and is imported as `{suggestion}`
- `standard`: this module is a standard module in this version and is imported as `{suggestion}`

Modules whose names start with `Benitoite` are the standard library modules. Unofficial modules are imported with `Unofficial` after `Benitoite`, such as `Benitoite.Unofficial.IO.Console`.

## E0322

- Kind: check (error)
- Message: modules import each other in a cycle
- Label: this import closes the cycle
- `member`: part of the cycle
- `chain`: the cycle is {chain}

Imports must not form a cycle. Move the declarations the modules share to another module.

## E0323

- Kind: check (error)
- Message: two imports have the name `{name}`
- Label: second import named `{name}`
- `first`: first import named `{name}`
- `alias`: give one of them another name with `as`, such as `import {module} as {suggestion}`

Each import gives the module one name in the file, the last part of its name or the name after `as`.

## E0324

- Kind: check (error)
- Message: the module `{module}` is imported more than once
- Label: imported again here
- `first`: first imported here
- `remove`: remove this import

A module is imported once in a file.

## E0325

- Kind: check (error)
- Message: `Benitoite` cannot be used as a name here
- `reserved`: `Benitoite` always refers to the standard library namespace
- `dependency`: a dependency cannot be named `Benitoite`, which always refers to the standard library

`Benitoite` cannot be declared or used as an import name or a dependency alias.

## E0326

- Kind: check (error)
- Message: the effect `{name}` has the same name as its module
- `rename`: effects are not modules; choose a name different from the module name

An effect cannot have the name of the module that declares it, because a qualified name such as `Log.write` would be ambiguous.

## E0327

- Kind: check (error)
- Message: the field `{name}` is declared more than once
- Label: redeclared here
- `first`: first declared here

Field names must be unique within a record.

## E0328

- Kind: check (error)
- Message: the method `{name}` is declared more than once in `{trait_name}`
- Label: redeclared here
- `first`: first declared here

Method names must be unique within a trait.

## E0329

- Kind: check (error)
- Message: the {level} {what} `{name}` uses the {used_level} {used_kind} `{used}`
- Label: `{used}` is not public
- `declared`: `{used}` is declared here
- `make_public`: mark `{used}` as `{target}`
- `handle`: handle the effect inside the function before returning
- `file_contract`: the file-level uses adds this effect to every public top-level function in the file
- `file`: remove the effect from the file-level uses or move the public functions to a file without that declaration

Everything a declaration exposes, including types, traits, and effects, must be at least as visible as the declaration.

## E0330

- Kind: check (error)
- Message: `{name}` is not public in `{module}`
- Label: not public
- `declared`: declared here
- `shadowed`: `{first}` here is the {shadow_kind} declared in this file, which hides the prelude {prelude_kind} `{first}`
- `shadowing`: `{first}` declared here
- `shadowed_import`: `{first}` here is the module imported in this file, which hides the prelude {prelude_kind} `{first}`
- `imported`: `{first}` imported here
- `prelude`: write `Benitoite.{first}` to use the prelude {prelude_kind}
- `make_public`: mark `{name}` as `public` in `{declared_in}`
- `package`: `{declared_in}` belongs to a dependency package; use its public names
- `package_only`: `{name}` is marked `public(package)`, so only modules of the same package can use it

Use `public` declarations of another module, or `public(package)` declarations within the same package.

## E0331

- Kind: check (error)
- Message: the constructor `{name}` must be written with its type name
- `qualify`: write `{suggestion}`

Constructors are written with their type name, such as `Option.Some(x)`, `Result.Error(e)`, and `Shape.Circle(r)`.

## E0332

- Kind: check (error)
- Message: the module `{module}` is not imported
- `import`: add `import {full}` at the start of the file

Standard library modules outside the prelude must be imported before use.

## E0333

- Kind: check (error)
- Message: `IO` is a module, not an effect
- Label: expected an effect
- `all`: use `IO.All` to allow every effect in `Benitoite.IO`

The built-in effects are `Console.Write`, `File.Read`, and so on. `IO.All` stands for all of them.

## E0334

- Kind: check (error)
- Message: `{name}` is already a local name here
- Label: `bind` cannot hide a local name
- `visible`: `{name}` is bound here
- `shadow`: write `shadow` to bind a new value to `{name}`

`bind` introduces names that are not visible yet. `shadow` hides a visible local name.

## E0335

- Kind: check (error)
- Message: `{name}` is not a local name here
- Label: nothing to shadow
- `bind`: write `bind` to introduce `{name}`

`shadow` hides a visible local name. `bind` introduces a new one.

## E0336

- Kind: check (error)
- Message: this binding mixes visible and new names
- `visible`: `{name}` is bound here
- `split`: split the statement into a `bind` and a `shadow`

All variables on the left of `bind` must be new, and all variables on the left of `shadow` must be visible local names.

## E0337

- Kind: check (error)
- Message: `shadow` does not bind any variable here
- `bind`: write `bind _ <- ...` to evaluate and discard the value

`shadow` needs at least one variable on its left.

## E0338

- Kind: check (error)
- Message: {binder} `{name}` hides a local name
- `hidden`: `{name}` is bound here
- `rename`: choose another name

Lambda parameters, pattern variables of branches, handler clause parameters, and `with` bindings cannot hide visible local names.

## E0339

- Kind: check (error)
- Message: the module `{module}` declares built-in effects and cannot be renamed with `as`
- Label: renamed here
- `remove`: remove `as {name}` and replace references to `{name}.` with `{original}.`

Modules that declare built-in effects cannot be renamed, so that `uses` always shows the original effect names, such as `Console.Write` and `File.Write`.

## E0340

- Kind: check (error)
- Message: the representation of opaque type `{type}` is private to `{module}`
- Label: representation is private
- `declared`: opaque type declared here
- `public_api`: use the module's public construction and inspection functions

Outside the declaring module, an opaque type can be passed as an ordinary value, but its constructors, fields, record updates, and representation patterns cannot be used.

## E0401

- Kind: check (error)
- Message: mismatched types
- Label: expected `{expected}`, found `{found}`
- `declared`: expected because of this
- `escaped`: `{param}` is a type parameter of the operation `{operation}`; a value of this type cannot leave the clause
- `clause`: the clause of `{operation}`
- `expanded`: the expanded expected type is `{expanded}`
- `because_call_arg`: argument {index} must match the parameter type of the called function
- `because_if_branches`: the branches of `if` must have compatible types
- `because_if_no_else`: `if` without `else` must have type `Unit`
- `because_condition`: a condition must have type `Boolean`
- `because_match_arms`: the branches of `match` must have compatible types
- `because_pattern`: a pattern must match the type of the value being matched
- `because_alternatives`: a variable bound in several alternatives must have one type
- `because_list`: the elements of a list must have compatible types
- `because_spread`: a spread `..e` must be a list of the element type
- `because_bind_annotation`: the value must match the type annotation
- `because_const_annotation`: the value of a constant must match its type
- `because_return`: the returned value must match the declared return type
- `because_lambda_return`: the lambda body must match the annotated return type
- `because_field`: the value must match the type of the field `{field}`
- `because_update`: a record update keeps the type arguments of the record
- `because_resume`: the value passed to `resume` must match the result type of the operation
- `because_handle`: the body and the clauses of `handle` must have compatible types
- `because_operands`: both operands of `{op}` must have the same type
- `because_logic`: the operands of `{op}` must have type `Boolean`
- `because_int_operands`: the operands of `{op}` must have type `Integer`
- `because_call`: the called value must be a function
- `float_literal`: write a floating-point literal such as `{literal}.0`
- `numeric_width`: use the `{suffix}` literal suffix for `{expected}`, or explicitly convert a value to that type; numeric types are not converted implicitly
- `to_string`: convert the value with `{function}`, or use string interpolation such as `"${x}"`
- `to_string_any`: convert the other value to a `String` before joining it with `+`, or use string interpolation
- `shadowed`: `{shadowed}` here is a type declared in a user module, which hides the prelude name `{shadowed}`
- `prelude_type`: write `Benitoite.{shadowed}` for the prelude type

Two types that must be equal are different.

## E0402

- Kind: check (error)
- Message: this function takes {expected} argument(s) but {found} were supplied
- `declared`: function declared here

Functions are not curried; supply every argument, or use `_` placeholders to build a new function.

## E0403

- Kind: check (error)
- Message: a value of type `{found}` is not a function
- Label: cannot be called
- `constant`: `{name}` is a constant; remove `()`

Only functions can be called.

## E0404

- Kind: check (error)
- Message: a type would contain itself
- Label: this makes the type infinite

A type variable cannot be equal to a type that contains it.

## E0405

- Kind: check (error)
- Message: `{op}` cannot be applied to `{ty}`
- `allowed`: `{op}` works on {allowed}
- `div`: use `div` for integer division, or convert with `Integer.toFloat` before `/`
- `type_param`: operators cannot be applied to values of a type parameter
- `to_string`: convert the value with `{function}`, or use string interpolation such as `"${x}"`
- `to_string_any`: convert the other value to a `String` before joining it with `+`, or use string interpolation

Arithmetic and ordering operators work only on the listed basic types.

## E0406

- Kind: check (error)
- Message: values of type `{ty}` cannot be compared with `{op}`
- `why`: types that contain functions or built-in opaque types without equality such as `IOError` cannot be compared with `=` or `<>`
- `constraint`: add the constraint `equality` to the type parameter, such as `[{param}: equality]`
- `operation`: add the constraint `{constraint}` to the type parameter `{declared}` in the declaration of the operation `{operation}`

Equality types contain no function types, no built-in opaque types without equality such as `IOError`, and no external types. An `@opaque` data type or record follows the equality rules for its contents.

## E0407

- Kind: check (error)
- Message: the type of this expression cannot be determined
- `annotate`: add a type annotation

An operator, an interpolation, a comparison, a key, a trait method, a `try`, or a `with` needs its type to be known by the end of the function body.

## E0408

- Kind: check (error)
- Message: integer literal is out of range for `Integer`
- `range`: `Integer` values range from -9223372036854775808 to 9223372036854775807

Integer literals must fit in a 64-bit signed integer.

## E0409

- Kind: check (error)
- Message: floating-point literal is too large for `Float`

The literal rounds to infinity.

## E0410

- Kind: check (error)
- Message: `{name}` expects {expected} type argument(s) but {found} were given
- `shadowed`: `{shadowed}` here is a type declared in a user module, which hides the prelude name `{shadowed}`
- `prelude_type`: write `Benitoite.{shadowed}` for the prelude type

Every type argument of a generic type or a type alias must be written.

## E0411

- Kind: check (error)
- Message: the type `{name}` has no constructors
- `add`: add at least one constructor

A `data` declaration needs at least one constructor.

## E0412

- Kind: check (error)
- Message: the constructor `{name}` has {expected} field(s) but the pattern has {found}

A constructor pattern needs one sub-pattern per field.

## E0413

- Kind: check (error)
- Message: the constructor `{name}` has no fields and is written without parentheses
- `remove`: remove `()`

Constructors without fields are values, not functions.

## E0414

- Kind: check (error)
- Message: no `main` function
- `define`: define `function main() -> Unit` in the file where execution starts

A program starts by calling `main` of the starting module.

## E0415

- Kind: check (error)
- Message: `main` has an invalid signature
- `params`: `main` takes no parameters
- `type_params`: `main` cannot have type parameters or effect variables
- `ret`: `main` must return `Unit` or `Result[Unit, String]`
- `effects`: `main` can only use the built-in effects in `IO.All`, the network effects, and `Log`
- `assert`: `main` cannot use `Assert.Check`

`main` has no parameters, returns `Unit` or `Result[Unit, String]`, and uses only built-in effects.

## E0416

- Kind: check (error)
- Message: the value of this expression is not used
- Label: this has type `{found}`, not `Unit`
- `return_value`: write `return` to return this value from the {what}
- `discard`: write `bind _ <- ...` to discard the value
- `reassign`: `=` compares values and variables cannot be reassigned; to bind a new value to the name, write `shadow {name} <- ...`

An expression statement must have type `Unit` unless it is the last in a block other than a function or lambda body. Return a value from a function or lambda with `return`.

## E0417

- Kind: check (error)
- Message: a `uses` list can contain at most one effect variable

Use the same effect variable for several parameters to combine their effects.

## E0418

- Kind: check (error)
- Message: the effect variable `{name}` does not appear in any parameter type

An effect variable must be determined by a parameter type.

## E0419

- Kind: check (error)
- Message: `{name}` appears more than once in `uses`
- `remove`: remove the second `{name}`

Each effect is listed once.

## E0420

- Kind: check (error)
- Message: Decimal literal has more than 28 digits after the decimal point
- `limit`: a `Decimal` has at most 28 digits after the decimal point

Decimal literals are not rounded, so their digits must fit the `Decimal` type.

## E0421

- Kind: check (error)
- Message: Decimal literal is out of range for `Decimal`
- `limit`: the digits of a `Decimal`, without the decimal point, must be less than 2^96

Decimal literals are not rounded, so their value must fit the `Decimal` type.

## E0422

- Kind: check (error)
- Message: a value of type `{ty}` cannot be interpolated into a string
- `show`: convert the value to a `String` first with `{function}`
- `import_show`: if this type implements `Show`, write `import Benitoite.Trait` and convert the value with `Trait.Show.show`
- `convert`: convert the value to a `String` before interpolating it
- `type_param`: add the constraint `{constraint}` to the type parameter, such as `[{param}: {constraint}]`, and convert the value with `{function}`
- `type_param_import`: write `import Benitoite.Trait`, add the constraint `Trait.Show` to the type parameter, such as `[{param}: Trait.Show]`, and convert the value with `Trait.Show.show`
- `rigid`: values of the operation's type parameter `{declared}` cannot be converted to `String`; interpolate a value of a known type instead

`${...}` accepts `String`, `Integer`, `Float`, `Decimal`, `Byte`, `Character`, and `Boolean`.

## E0423

- Kind: check (error)
- Message: `{ty}` cannot be used as a key
- `float`: `Float` has values that are not equal to themselves; use `Integer`, `Decimal`, or `String` instead

Map keys and set elements must have types that satisfy `key`. Types that contain `Float` do not.

## E0424

- Kind: check (error)
- Message: the type parameter `{param}` needs the constraint `{constraint}`
- `required`: required by this use
- `add`: add the constraint, such as `[{param}: {constraint}]`
- `operation`: add the constraint `{constraint}` to the type parameter `{declared}` in the declaration of the operation `{operation}`

Comparing values or using them as keys requires `equality` or `key` on a type parameter.

## E0425

- Kind: check (error)
- Message: the constraint `{constraint}` cannot be written here

Type declarations cannot have constraints. Built-in constraints can be written on value type parameters of functions, methods, and implementations, but not on type constructor parameters or supertraits.

## E0426

- Kind: check (error)
- Message: `{name}` needs type arguments here

A type constructor such as `List` or a parameter `F[_]` is a type only with its type arguments.

## E0427

- Kind: check (error)
- Message: `{name}` takes {expected} type argument(s) where {found} are needed

A trait over type constructors, such as `Functor`, needs a type constructor that takes the same number of type arguments.

## E0428

- Kind: check (error)
- Message: the record `{name}` is missing fields: {fields}
- Label: missing fields
- `declared`: the record is declared here

A record construction gives a value to every field. Use `Record(..value, field: x)` to copy the other fields.

## E0429

- Kind: check (error)
- Message: the record `{name}` has no field `{field}`
- Label: unknown field
- `similar`: a similar field exists: {candidates}

Only the fields declared in the record can be written.

## E0430

- Kind: check (error)
- Message: the field `{field}` is given more than once
- Label: given again here
- `first`: first given here

Each field is written once in a construction, an update, or a pattern.

## E0431

- Kind: check (error)
- Message: the type alias `{name}` refers to itself
- `member`: part of the cycle
- `chain`: the cycle is {chain}
- `data`: declare a `data` type for a recursive type

A type alias is replaced by its definition, so it cannot refer to itself directly or through other aliases.

## E0432

- Kind: check (error)
- Message: the type parameter `{param}` is not used in the type alias `{name}`

Every type parameter of a type alias must appear on its right-hand side.

## E0433

- Kind: check (error)
- Message: this expression cannot be used in a constant
- Label: not a constant expression
- `function`: write a function without parameters to compute the value when it is called

A constant is computed before the program runs, from literals, other constants, operators, constructors, records, lists, and `Map.fromList` or `Set.fromList`.

## E0434

- Kind: check (error)
- Message: the type of the constant `{name}` cannot contain type parameters

A constant has one value, so its type must be a concrete type.

## E0435

- Kind: check (error)
- Message: computing the constant `{name}` fails: {condition}
- Label: fails here
- `budget`: compute this value in a function without parameters, which runs when the script runs

A constant is computed when the script is checked. Its computation must not stop with a runtime error, and the values that the constants create must fit in the budget for constants; compute a large value in a function without parameters instead.

## E0436

- Kind: check (error)
- Message: the key `{key}` appears more than once in a constant
- Label: repeated key
- `first`: first given here

`Map.fromList` and `Set.fromList` in a constant must not repeat a key.

## E0437

- Kind: check (error)
- Message: constants refer to each other in a cycle
- Label: this constant is part of a cycle
- `member`: part of the cycle
- `chain`: the cycle is {chain}

A constant can use other constants, but not itself through a chain of constants.

## E0438

- Kind: check (error)
- Message: the function can reach its end without `return`
- Label: the end of the body is reachable
- `path`: the body can reach its end through this path
- `add`: add `return` with a value of type `{expected}`

A function or lambda whose return type is not `Unit` must leave through `return` on every path.

## E0439

- Kind: check (error)
- Message: this statement is never executed
- Label: unreachable statement
- `exit`: this always leaves the block

A statement after `return`, or after an `if` or `match` that always leaves, is never executed.

## E0440

- Kind: check (error)
- Message: `try` cannot be applied to `{ty}`
- `kinds`: `try` works on `Result` and `Option`

`try` takes a `Result` or an `Option`.

## E0441

- Kind: check (error)
- Message: `try` on `{ty}` needs the function to return {needed}
- `declared`: the function returns `{declared}`
- `declared_lambda`: the lambda returns `{declared}`
- `declared_placeholder`: the function made by `_` in this call returns `{declared}`
- `placeholder_call`: this call with `_` makes a function
- `convert`: convert the value with `Option.okOr` or `Result.ok` before `try`

`try` on a `Result` needs the enclosing function or lambda to return a `Result`, and `try` on an `Option` needs it to return an `Option`.

## E0442

- Kind: check (error)
- Message: `try` cannot return an error of type `{found}` from a function that returns errors of type `{expected}`
- `declared`: the function returns `{declared}`
- `declared_lambda`: the lambda returns `{declared}`
- `declared_placeholder`: the function made by `_` in this call returns `{declared}`
- `placeholder_call`: this call with `_` makes a function
- `map_error`: convert the error with `Result.mapError` before `try`

The error type of the value after `try` must be the error type of the function's result.

## E0443

- Kind: check (error)
- Message: `{ty}` is not a resource type
- `resources`: `with` binds values opened by functions such as `File.openReader` and `TaskGroup.open`

A `with` binding needs a resource, which is released when the block ends.

## E0446

- Kind: check (error)
- Message: integer literal is out of range for `Byte`
- `range`: a `u8` literal must be between 0 and 255

The suffix `u8` denotes `Byte`. Byte literals are checked before execution and cannot be negative.

## E0447

- Kind: check (error)
- Message: integer literal is out of range for `{ty}`
- `range`: `{ty}` values range from {minimum} to {maximum}

A suffixed integer literal must fit in its declared width, including a directly applied minus sign.

## E0449

- Kind: check (error)
- Message: this function requires named arguments
- `available`: available parameters, in declaration order: {names}
- `named`: add the parameter names in declaration order

Use parameter names when calling a function with two or more parameters.

## E0450

- Kind: check (error)
- Message: named and positional arguments cannot be mixed
- `first`: the first argument uses the other form

Use named arguments consistently in a call.

## E0451

- Kind: check (error)
- Message: this function has no parameter named `{name}`
- `available`: available parameters, in declaration order: {names}

An argument label must name a parameter of the function contract.

## E0452

- Kind: check (error)
- Message: the parameter `{name}` is supplied more than once
- `first`: first supplied here

Each parameter must be supplied exactly once.

## E0453

- Kind: check (error)
- Message: missing arguments for parameters: {names}

Supply each parameter of the function contract.

## E0456

- Kind: check (error)
- Message: direct constructor calls use positional fields
- `bind`: use positional fields, or bind the constructor to a function value before using named arguments

Direct data construction uses positional fields; constructor function values have parameter contracts.

## E0457

- Kind: check (error)
- Message: invalid constant regular expression: {reason}
- Label: invalid pattern
- `constant`: constant declared here
- `origins_omitted`: {omitted} other constant declarations omitted

A direct Regex.compile call checks a known constant source. Pass dynamic input to observe Result.Error at runtime.

## E0458

- Kind: check (error)
- Message: constant regular expression exceeds the compilation limit: {reason}
- Label: compilation limit
- `constant`: constant declared here
- `origins_omitted`: {omitted} other constant declarations omitted

The runtime RegexBuilder also rejects this source. The constant evaluation budget is a separate limit.

## E0459

- Kind: check (error)
- Message: invalid constant scope pattern: {reason}
- Label: invalid pattern
- `constant`: constant declared here
- `origins_omitted`: {omitted} other constant declarations omitted

Use an exact host/path pattern, *.host, **.host, or an absolute path ending in /**.

## E0460

- Kind: check (error)
- Message: invalid constant scope target: {reason}
- Label: invalid target
- `constant`: constant declared here
- `origins_omitted`: {omitted} other constant declarations omitted

Supply a host or absolute path with the spelling required by Scope.

## E0461

- Kind: check (error)
- Message: invalid constant scope port bounds or port
- Label: invalid port
- `constant`: constant declared here
- `origins_omitted`: {omitted} other constant declarations omitted
- `condition`: lower bound supplied here

Ports must be between 0 and 65535, and minimumPort must not exceed maximumPort.

## E0463

- Kind: check (error)
- Message: a builder requires an annotated local bind and a direct new call
- `bind`: use `bind b: StringBuilder <- StringBuilder.new()`

Builder declarations require a dedicated annotation and matching direct new call.

## E0464

- Kind: check (error)
- Message: a local builder cannot be used as an ordinary type or value
- `copy`: use copy to obtain an ordinary immutable value

Builder classifications are allowed only in annotated local binds; builder names are allowed only as direct operation targets.

## E0465

- Kind: check (error)
- Message: a local builder cannot be captured across {boundary}
- `declared`: builder declared here
- `boundary`: capture boundary starts here
- `copy`: bind a copy or length before the boundary and capture that ordinary value

A lambda, lazy expression, partial application, or handler clause starts a fresh builder context.

## E0466

- Kind: check (error)
- Message: builder operations require a direct call
- `direct`: write a direct call without holes or a pipe

Builder operations cannot be used as function values, partially applied, or piped.

## E0467

- Kind: check (error)
- Message: SourceLocation.here requires a direct call
- `direct`: write `SourceLocation.here()` and pass the resulting location explicitly

The standard here expression cannot be used as a function value, partially applied, or piped.

## E0468

- Kind: check (error)
- Message: a value of type `{ty}` is incompatible with this interpolation format
- Label: incompatible format
- `value`: value has type `{ty}`
- `allowed`: this format accepts {allowed}
- `change`: choose a format compatible with the value's type; numeric formatting requires an explicitly numeric value
- `integer_fixed`: use `d` for integer output, or explicitly convert the value to Float, Float32, or Decimal if fractional output is intended

Static interpolation formats restrict the value to a closed set of basic types. No implicit conversion or user-defined Show implementation changes this set.

## E0462

- Kind: check (error)
- Message: constant target is outside the supplied scope
- Label: outside scope
- `constant`: constant declared here
- `origins_omitted`: {omitted} other constant declarations omitted
- `condition`: scope condition supplied here

The known target does not satisfy the supplied host/path pattern or inclusive port range.

## E0454

- Kind: check (error)
- Message: function parameter names do not match
- `declared`: method declared here
- `contracts`: expected `{expected}`, found `{found}`
- `first_name`: parameter {index}: expected `{expected_name}`, found `{found_name}`

Function types must have the same parameter names in the same order.

## E0455

- Kind: check (error)
- Message: the parameter names and order of this function cannot be determined
- `annotation`: add a function-type annotation with parameter names
- `bind`: bind the callee to a value with an explicit named function type before using a pipe or placeholders

The function contract must be determined before type checking finishes.

## E0448

- Kind: check (error)
- Message: floating-point literal is too large for `Float32`

The literal rounds to infinity in binary32.

## E0501

- Kind: check (error)
- Message: this function performs `{effect}` but does not declare it
- Label: `{effect}` happens here
- `declared`: declared here
- `add_uses`: add `uses {effect}` to the signature, or move this call to a function that uses `{effect}`
- `die_uses`: add `uses Process.Exit` to the signature for `or die`
- `die_import`: add `import Benitoite.Unofficial.IO.Process` and `uses Process.Exit`; the internal dependency of `or die` does not import the name `Process`

A function body can only perform the effects listed after `uses`.

## E0502

- Kind: check (error)
- Message: this lambda performs `{effect}` but its `uses` does not include it
- Label: `{effect}` happens here
- `declared`: declared here

A lambda with `uses` can only perform the listed effects.

## E0503

- Kind: check (error)
- Message: a `lazy` body cannot perform `{effect}`
- Label: `{effect}` happens here
- `lambda`: use a lambda without parameters to delay an effectful computation

A `lazy` body is computed at most once at an unknown time, so it cannot have effects.

## E0504

- Kind: check (error)
- Message: a guard cannot perform `{effect}`
- Label: `{effect}` happens here
- `body`: compute the condition in the branch body instead

Guards are conditions and cannot have effects.

## E0505

- Kind: check (error)
- Message: `TaskGroup.open` can only be called in a `with` binding
- `with`: write `with group = TaskGroup.open() do ... end with`

A task group must be released when its block ends, so it is opened only by `with`.

## E0506

- Kind: check (error)
- Message: `{function}` does not handle the effect `{effect}`
- Label: `{effect}` is not handled
- `handle`: handle `{effect}` with `handle ... with` inside `{function}`
- `to_result`: or convert the failure with Failure.toResult inside `{function}`
- `file`: remove the effect from the file-level uses or move `{function}` to a file without that declaration; handling it in the body does not remove the declared effect

Effects declared by the program must be handled inside `main` and inside test functions. For the standard Failure.Fail, import Benitoite.Unofficial.Failure, catch it with Failure.toResult, and remove the handled Fail from the entry function's uses. Result.andThen/flatMap chains Result values; it does not handle Fail.

## E0507

- Kind: check (error)
- Message: `{name}` is not an operation of an effect
- `state`: `State` has no operations and cannot be handled

A clause of `handle` names an operation declared in an `effect`.

## E0508

- Kind: check (error)
- Message: the operation `{name}` is handled twice in one `handle`
- Label: second clause
- `first`: first clause here

Each operation has at most one clause in a `handle`.

## E0509

- Kind: check (error)
- Message: `resume` can only be used directly in a clause of `handle`
- `lambda`: `resume` cannot be used in a lambda or a `lazy` body inside a clause
- `die_message`: `resume` cannot be used in the message of `or die`; use a pure String expression
- `guard`: `resume` cannot be used in a `match` guard; call it in the body of the branch
- `placeholder`: `resume` cannot be used in an argument of a call with `_` placeholders; the arguments are evaluated inside the function that the call creates

`resume` continues the computation that performed the operation, so it is written in the clause itself, not inside a lambda, a `lazy` body, a `match` guard, the message of `or die`, or an argument of a call with `_` placeholders.

## E0510

- Kind: check (error)
- Message: invalid effect operation declaration
- `uses`: an operation declaration cannot have `uses`; the handler decides the effects
- `effect_var`: an operation cannot have effect variables
- `type_ctor`: an operation cannot have type constructor parameters such as `F[_]`

Operations of an effect are declared with a name, parameters, and a result type.

## E0511

- Kind: check (error)
- Message: the message of `or die` cannot perform `{effect}`
- `pure`: use a pure String expression for the message
- `message`: the message is written here

The optional message of `or die` must be a pure String expression.

## E0512

- Kind: check (error)
- Message: effects of `{function}` exceed `--allow-effects`
- Label: effect limit checked for this function
- `missing`: effects not allowed: {missing}
- `allowed`: allowed effects: {allowed}
- `file`: added to this function by the file-level uses declaration
- `review`: review the required effects before changing --allow-effects

The declared atomic effects of the checked function (main or a test) exceed the set passed to --allow-effects. Nothing is started. Review the required effects before changing the allowed set. This is a static check, not runtime permission control or an OS sandbox.

## E0513

- Kind: check (error)
- Message: invalid type arguments for effect `{name}` (expected {count})

Only the standard Failure.Fail takes exactly one value type argument.

## E0514

- Kind: check (error)
- Message: incompatible failure types `{expected}` and `{found}` in one effect set
- `convert`: convert errors explicitly in a failure clause before joining effects
- `individual`: the function's uses is declared here

A source computation can have at most one invariant failure type.

## E0515

- Kind: check (error)
- Message: cannot establish that the remaining effects exclude Failure.Fail
- `signature`: declare Failure.Fail in the callback signature, handle failures inside the callback, or move the handle boundary

NoFail conditions come only from declared signatures. A row next to an explicit Failure.Fail excludes failures.

## E0516

- Kind: check (error)
- Message: `resume` is not allowed in a failure clause
- `result`: return the result of the whole handle instead

Failure.raise aborts the captured continuation.

## E0517

- Kind: check (error)
- Message: cannot determine the error type caught by this handle
- `annotation`: annotate the callback or result type to fix the failure type

The failure type must be fixed before its handler is erased.

## E0518

- Kind: check (error)
- Message: cannot mix failure and ordinary operation clauses in one handle
- `nest`: nest two handles instead

A failure handle contains exactly one Failure.raise clause.

## E0601

- Kind: check (error)
- Message: `match` does not cover every value
- Label: `{witness}` is not matched
- `add_arm`: add a branch `case {witness} ->`, or `case _ ->`
- `future`: add an unguarded variable or `_` pattern to cover future values of `{future_ty}`
- `extensible_decl`: this type permits future shapes outside its package

Every possible value must be matched by some branch. Character ranges can cover all Unicode scalars without a catch-all. For a match containing only 'a', the missing-character witness is '\u{0}'. Integer ranges still need a catch-all.

## E0602

- Kind: check (error)
- Message: this branch can never be selected
- Label: unreachable branch
- `covering`: this earlier branch already matches the values

Earlier branches match every value this branch matches.

## E0603

- Kind: check (error)
- Message: the pattern variable `{name}` has the name of a constant
- Label: binds a new variable
- `constant`: the constant is declared here
- `guard`: compare with the constant in a guard, such as `case n if n = {name} ->`

A lowercase name in a pattern always binds a new variable. Compare with a constant in a guard.

## E0604

- Kind: check (error)
- Message: the alternatives bind different variables
- `missing`: `{name}` is not bound in this alternative
- `first`: `{name}` is bound here
- `extra`: `{name}` is bound only in this alternative
- `sets`: missing variables: {missing}; extra variables: {extra}
- `missing_set`: missing variables: {missing}
- `extra_set`: extra variables: {extra}

Every alternative in one branch must bind the same variables.

## E0605

- Kind: check (error)
- Message: the range `{low}..{high}` is empty
- `swap`: write `{high}..{low}`

The lower end of a range pattern must not be greater than the upper end.

## E0606

- Kind: check (error)
- Message: a range pattern needs two `Integer` or two `Character` literals

Range patterns compare integers or characters.

## E0607

- Kind: check (error)
- Message: this pattern does not match every value of `{ty}`
- Label: `{witness}` is not matched
- `match`: use `match ... with` to handle the other values
- `future`: use `match ... with` and add an unguarded `case _ ->` branch to cover future values of `{future_ty}`
- `extensible_decl`: this type permits future shapes outside its package

The pattern on the left of `bind` or `shadow` must match every value.

## E0608

- Kind: check (error)
- Message: this alternative can never be selected
- Label: unreachable alternative
- `covering`: already matched here

Earlier patterns match every value this alternative matches.

## E0609

- Kind: check (error)
- Message: could not confirm that {subject}
- Label: checking exceeded a coverage budget: {limit}
- `split`: split the `match` into smaller ones, or check fewer fields in one branch
- `split_pattern`: check fewer fields in this pattern, or bind the value first and check the fields separately

The checker limits the work it spends on deciding whether a `match` covers every value and whether a pattern on the left of `bind` or `shadow` matches every value. A `match` or a pattern that needs more work than the limit is an error.

## E0701

- Kind: check (error)
- Message: this implementation must be in the module of `{trait_name}` or of `{ty}`
- `trait_decl`: the trait is declared here
- `type_decl`: the type is declared here

An implementation is written in the module that declares the trait or the type, so that it is found without importing other modules.

## E0702

- Kind: check (error)
- Message: `{trait_name}` is implemented more than once for `{ty}`
- Label: overlapping implementation
- `other`: the other implementation is here

Each type constructor has at most one implementation of a trait.

## E0703

- Kind: check (error)
- Message: traits are supertraits of each other in a cycle
- `member`: part of the cycle
- `chain`: the cycle is {chain}

Supertraits must not form a cycle.

## E0704

- Kind: check (error)
- Message: the implementation of `{trait_name}` for `{ty}` needs `{supertrait}` for `{ty}`
- `constraint`: add the constraint `{constraint}` to the type parameter of the implementation

A type implements a trait only if it implements the supertraits under the same constraints.

## E0705

- Kind: check (error)
- Message: `{ty}` does not implement `{trait_name}`
- Label: `{trait_name}` is required here
- `required`: required by this use
- `die_show`: implement Trait.Show or convert the error with Result.mapError

A value used with a trait method, or passed where a constraint is required, must have an implementation.

## E0706

- Kind: check (error)
- Message: the type parameter `{param}` needs the constraint `{trait_name}`
- `add`: add the constraint, such as `[{param}: {trait_name}]`
- `die_show`: implement Trait.Show or convert the error with Result.mapError

Using a trait method on a value of a type parameter requires that constraint on the parameter.

## E0707

- Kind: check (error)
- Message: the implementation of `{trait_name}` is missing methods: {methods}
- `trait_decl`: the trait is declared here

An implementation defines every method of its trait.

## E0708

- Kind: check (error)
- Message: `{name}` is not a method of `{trait_name}`
- `similar`: a similar method exists: {candidates}

An implementation defines only the methods of its trait.

## E0709

- Kind: check (error)
- Message: the method `{name}` does not mention the type parameter of `{trait_name}`

A method is chosen by the type of its parameters or its result, so it must mention the trait's type parameter.

## E0710

- Kind: check (error)
- Message: the trait `{name}` declares no methods

A trait declares at least one method.

## E0711

- Kind: check (error)
- Message: the method `{name}` does not match its declaration in `{trait_name}`
- Label: expected `{expected}`, found `{found}`
- `declared`: declared here

An implemented method has the parameter and result types of the trait's method, and at most its effects.

## E0712

- Kind: check (error)
- Message: cannot implement a trait for `{ty}`
- `form`: implement it for a type name with distinct type parameters, such as `Option[T]`

Implementations are written for one type constructor applied to distinct type parameters.

## E0713

- Kind: check (error)
- Message: the type parameter `{param}` is not used in the implemented type

Every type parameter of an implementation must appear in the implemented type.

## E0714

- Kind: check (error)
- Message: `{name}` does not fit the trait `{trait_name}`
- `value`: `{trait_name}` is for types of values, such as `Integer`
- `constructor`: `{trait_name}` is for type constructors, such as `List`

A trait is either for types of values or for type constructors, and its implementations and constraints must match.

## E0715

- Kind: check (error)
- Message: the dictionary for `{trait_name}` on `{ty}` is too large
- Label: more than {limit} dictionary and type nodes are needed here
- `reduce`: nest the types less deeply, or require fewer traits of the type parameters in the implementations

Nesting implementations that require two or more traits of the same type parameter doubles the size of the dictionary at each level. The checker limits the total number of dictionary nodes and type nodes copied into them.

## E0801

- Kind: check (error)
- Message: unknown attribute `@{name}`
- `allowed`: the attributes are `@test`, `@test("...")`, `@deprecated("...")`, `@extensible`, `@opaque`, and `@external("wasm", "...", "...")`

Only the listed attributes can be written.

## E0802

- Kind: check (error)
- Message: `@{name}` cannot be written on {what}

`@test` is written on functions. `@external` is written on top-level functions and on `type` declarations without a right-hand side. `@deprecated` is written on functions, constants, `data` types, type aliases, records, traits, and effects. `@extensible` is written on `data` types and records. `@opaque` is written on public or package-public `data` types and records.

## E0803

- Kind: check (error)
- Message: `@{name}` is written more than once
- Label: written again here
- `first`: first written here
- `remove`: remove this attribute

Each attribute is written at most once on a declaration.

## E0804

- Kind: check (error)
- Message: `@deprecated` needs one non-empty message
- `message`: write the reason and the replacement, such as `@deprecated("use formatDate instead")`

The message of `@deprecated` is shown with every use of the declaration.

## E0805

- Kind: check (error)
- Message: `@test` takes at most one description

`@test` is written alone or with one description string.

## E0806

- Kind: check (error)
- Message: the test function `{name}` has an invalid signature
- `params`: a test function takes no parameters
- `type_params`: a test function cannot have type parameters or effect variables
- `ret`: a test function must return `Unit` or `Result[Unit, String]`

Test functions are called without arguments by `benitoite test`.

## E0807

- Kind: check (error)
- Message: `@builtin` can only be used in the standard library
- `remove`: remove `@builtin` and write the body of the function

Built-in functions are provided by the standard library.

## E0808

- Kind: check (error)
- Message: the function `{name}` has no body
- `body`: write the body and close it with `end function`
- `external`: to call a function of a WebAssembly component, write `@external("wasm", "path", "name")` before the function

A function needs a body, unless it is an external function declared with `@external` or a built-in function of the standard library.

## E0809

- Kind: check (error)
- Message: `@external` takes three strings: the kind, the component path, and the function name
- `form`: write `@external("wasm", "wasm/fuzzy.wasm", "distance")`

External declarations identify a component and one exported function.

## E0810

- Kind: check (error)
- Message: unknown kind of external function `{kind}`
- Label: unknown kind
- `wasm`: this version supports only `"wasm"`

Only WebAssembly components can provide external functions.

## E0811

- Kind: check (error)
- Message: an external {what} cannot have type parameters
- Label: type parameters
- `fixed`: write concrete types for the parameters and the result; the types are checked against the component
- `resource`: an external type stands for one resource of the component and takes no type parameters

External functions have concrete parameter and result types.

## E0812

- Kind: check (error)
- Message: an external function can declare only `uses State`, and only when its type contains an external type
- Label: not allowed here
- `pure`: external functions that pass external types declare `uses State`; all other external functions are pure
- `remove`: remove `uses`
- `state`: replace `uses` with `uses State`
- `file`: remove the incompatible effect from the file-level uses or move the external function to a file without that declaration

External functions declare State exactly when passing external types.

## E0813

- Kind: check (error)
- Message: `@external` cannot be combined with `@{other}`

An external function cannot also be a test or a built-in function.

## E0814

- Kind: check (error)
- Message: an external function has no body
- Label: remove this body
- `remove`: the component provides the body; end the declaration after the result type

An external function obtains its body from the component.

## E0815

- Kind: check (error)
- Message: the interface and function names in `@external` must both be nonempty
- `form`: write `interface#function`, with a name on each side of the last `#`

The third argument of `@external` selects a function directly or as `interface#function`. Neither side of the last `#` can be empty.

## E0816

- Kind: check (error)
- Message: the external function `{name}` passes an external type, so it must declare `uses State`
- Label: external type in this signature
- `state`: add `uses State` after the result type

Functions that pass external types use component state.

## E0817

- Kind: check (error)
- Message: `@extensible` takes no arguments
- `remove`: remove the arguments from `@extensible`

`@extensible` is written without arguments on a `data` type or record.

## E0818

- Kind: check (error)
- Message: `@opaque` takes no arguments
- `remove`: remove the arguments from `@opaque`

Write `@opaque` without arguments on a public or package-public data type or record.

## E0819

- Kind: check (error)
- Message: `@opaque` cannot be combined with `@{other}`
- Label: incompatible attributes

`@opaque` cannot be combined with `@extensible` or `@external`; an external type is already opaque.

## E0820

- Kind: check (error)
- Message: `@opaque` requires a public or package-public data or record declaration

The opaque attribute separates visibility of a type from access to its representation.

## E0901

- Kind: check (error)
- Message: unknown field `{field}` in `{block}`
- `fields`: the fields of `{block}` are {fields}

Dependency fields are `path` and `hash`; `source`, `version`, and `commit` are recognized but unavailable. `package` blocks accept only `license`.

## E0902

- Kind: check (error)
- Message: {what} is written more than once
- Label: written again here
- `first`: first written here

Each field, `language` line, `package` block, and dependency alias may appear only once in the header.

## E0903

- Kind: check (error)
- Message: the field `{field}` cannot be used in this version
- `local`: this version reads dependencies only from a local directory written with `path`

Only local `path` dependencies are supported. Do not write `source`, `version`, or `commit`.

## E0904

- Kind: check (error)
- Message: the dependency `{alias}` has no `path`
- `path`: write the directory of the package, such as `path "../textkit"`

Each dependency needs a `path` field.

## E0905

- Kind: check (error)
- Message: malformed language version `{version}`
- `form`: write three numbers separated by `.`, such as `0.0.3`

Language versions have three ASCII decimal numbers that fit in 64 bits, without leading zeroes except for zero itself.

## E0906

- Kind: check (error)
- Message: malformed hash `{hash}`
- `form`: write `sha256:` followed by 64 lowercase hexadecimal digits

A dependency hash consists of `sha256:` and exactly 64 lowercase hexadecimal digits.

## E0907

- Kind: check (error)
- Message: this file needs Benitoite {version} or later, but this is Benitoite {current}

The declared language version is newer than the interpreter. Use an interpreter at least as new as the declaration.

## E0908

- Kind: check (error)
- Message: this file was written for Benitoite {version}, and the language changed incompatibly in {changed}

The main message identifies the first incompatible language change after the declared version and no later than the interpreter's version. Notes list every applicable version and its summary in version order, and helps list the corresponding migration guidance in the same order, without source edits.

## E0909

- Kind: check (error)
- Message: `{item}` can be written only at the start of the starting file or in `package.bnt`

Imported modules cannot have `language` or `dependency` headers. A `package` block can be written only in `package.bnt`.

## E0910

- Kind: check (error)
- Message: a `package` block can be written only in `package.bnt`

Move the `package` block into `package.bnt` at the root.

## E0911

- Kind: check (error)
- Message: {what} is written both at the start of `{file}` and in `package.bnt`
- `other`: also written here

Write dependencies in one place, either the starting file or `package.bnt`. The same rule applies to `language` declarations.

## E0912

- Kind: check (error)
- Message: `package.bnt` can contain only `language`, `dependency`, and `package`
- Label: not allowed in `package.bnt`

A settings file cannot contain imports or declarations.

## E0913

- Kind: check (error)
- Message: `{path}` is a settings file and cannot be checked, run, or tested

Do not specify `package.bnt` as a starting file. Check, run, or test the scripts that use it; `fmt` can format it.

## E0914

- Kind: check (error)
- Message: the package `{package}` needs a `language` line

A `package.bnt` with a `package` block must declare the language version, even when checked as the root package.

## E0915

- Kind: check (error)
- Message: a package cannot have dependencies in this version
- `package`: this `package.bnt` declares the package `{package}`

A `package.bnt` with a `package` block cannot contain dependencies in this version.

## E0916

- Kind: check (error)
- Message: cannot read the dependency `{alias}` at `{path}`
- `reason`: {reason}

The dependency directory and its `package.bnt` must be readable.

## E0917

- Kind: check (error)
- Message: the dependency `{alias}` at `{path}` is not a Benitoite package
- `no_config`: the directory has no `package.bnt`
- `no_block`: its `package.bnt` has no `package` block

A dependency must have `package.bnt` with a `package` block and a `language` line.

## E0918

- Kind: check (error)
- Message: `{module}` names both a module under the root directory and a module of the dependency `{alias}`
- `root_file`: under the root directory: `{root_path}`
- `package_file`: in the package `{package}`: `{package_path}`
- `rename`: rename the directory under the root directory, or the dependency

An import cannot refer to two different files. A root path and package path resolving to the same file are allowed.

## E0919

- Kind: check (error)
- Message: the contents of the dependency `{alias}` do not match its `hash`
- `expected`: expected: {expected}
- `computed`: computed: {computed}
- `package`: package: `{package}`
- `update`: if the change is intended, write the computed hash

A dependency with `hash` must match the computed hash of its package contents. Update the declaration only if the change is intended.

## E0920

- Kind: check (error)
- Message: cannot compute the hash of the dependency `{alias}`
- `symlink`: `{path}` is a symbolic link
- `case`: `{path}` and `{other}` have the same path after conversion to Unicode lowercase
- `name`: `{path}` contains a newline
- `too_many`: the number of included files exceeds the limit of {limit}
- `too_many_entries`: the number of visited directory entries exceeds the limit of {limit}
- `too_large`: the total size of included files exceeds the limit of {limit} bytes
- `read`: cannot read `{path}`: {reason}
- `package`: package: `{package}`

The package hash rejects symbolic links, colliding lowercase paths, newline-containing paths, unreadable files, and packages exceeding the file count, directory entry count, or total size limit.

## E0921

- Kind: check (error)
- Message: `{path}` belongs to the dependency `{alias}` and cannot be imported as a root module
- Label: use the dependency alias
- `package`: package: `{package}`
- `import`: write `import {suggestion}`

A file under a declared dependency root must be imported through its dependency alias.

## E0922

- Kind: check (error)
- Message: the dependency `{alias}` at `{path}` overlaps another root
- Label: invalid dependency root
- `root`: a dependency root cannot be the root directory itself or an ancestor of it
- `dependency`: dependency roots cannot be equal or contain one another
- `other`: the overlapping dependency `{other}` is declared here

Choose a dependency directory that does not contain the starting root and does not overlap another dependency root.

## E0923

- Kind: check (error)
- Message: the dependency `{alias}` is already declared
- Label: existing dependency

Use a different alias or update the existing dependency.

## E0924

- Kind: check (error)
- Message: the dependency `{alias}` is not declared

Specify an alias declared in the entry or package.bnt.

## E0925

- Kind: check (error)
- Message: there are no header items to move

The entry must contain language or dependency declarations.

## E0926

- Kind: check (error)
- Message: `{path}` already contains language or dependency declarations
- Label: existing declaration

Do not overwrite existing declarations when moving the entry header.

## E0927

- Kind: check (error)
- Message: cannot choose a file for the dependency declaration
- `entry`: specify an entry file, or create package.bnt

A root without an entry and without package.bnt has no destination.

## E0928

- Kind: check (error)
- Message: cannot move the header of `package.bnt`

Specify an entry file rather than the configuration file.

## E0929

- Kind: check (error)
- Message: malformed recorded effects

The effects field must be empty or a comma-separated list of uppercase names such as Console.Write, File.Read. It records the last package command report and is not checked against package contents during loading.

## E1001

- Kind: check (error)
- Message: cannot read the component `{path}`
- `reason`: {reason}

The component must be a readable file within the allowed directory.

## E1002

- Kind: check (error)
- Message: malformed component path `{path}`
- `form`: write a relative path separated by `/` that ends with `.wasm`, without `.` or `..`
- `hidden`: a name that starts with `.` is not part of a package

Component paths use visible relative names and the `.wasm` extension.

## E1003

- Kind: check (error)
- Message: the component `{path}` is outside the {base}
- `root`: a component of a module under the root directory must be under the root directory
- `package`: a component of the package `{package}` must be under its `wasm` directory

Canonical component paths must remain within the allowed directory.

## E1004

- Kind: check (error)
- Message: `{path}` is not a WebAssembly component
- `not_wasm`: the file does not start with the WebAssembly magic bytes
- `core`: it is a core WebAssembly module; wrap it with `wasm-tools component new`
- `invalid`: the component is invalid: {reason}

External functions require binary WebAssembly components.

## E1005

- Kind: check (error)
- Message: the component `{path}` imports {count}, so it cannot be called as a pure function
- `imports`: imports: {names}
- `target`: build it for `wasm32-unknown-unknown` and wrap it with `wasm-tools component new`; `wasm32-wasip2` adds WASI imports

Only components without imports can provide pure external functions, including calls through generated adapters.

## E1006

- Kind: check (error)
- Message: the component `{path}` has no function `{name}`
- `exports`: the functions it exports are {names}
- `version`: the component exports the interface as `{found}`; the version must match exactly
- `interface`: `{name}` is an interface; write `{name}#function`

External declarations name a function exported by the component, directly or as `interface#function`.

## E1007

- Kind: check (error)
- Message: the function `{name}` of the component takes {found}, but `{function}` declares {expected}
- `declaration`: the component function corresponds to `{declaration}`

The declared parameter count must match the component function.

## E1008

- Kind: check (error)
- Message: argument {index} of `{function}` does not match the component
- Label: declared as `{declared}`
- `component`: the component expects `{wit}`
- `maps`: `{declared}` is passed as `{expected}`
- `reason`: {reason}
- `declaration`: the component function corresponds to `{declaration}`
- `user_type`: `{name}` here is the type declared in `{file}`, not `Benitoite.{name}.{name}`

Parameter types are checked by position against their WIT representations.

## E1009

- Kind: check (error)
- Message: the result of `{function}` does not match the component
- Label: declared as `{declared}`
- `component`: the component returns `{wit}`
- `maps`: `{declared}` is passed as `{expected}`
- `reason`: {reason}
- `declaration`: the component function corresponds to `{declaration}`
- `user_type`: `{name}` here is the type declared in `{file}`, not `Benitoite.{name}.{name}`

The result type must match its WIT representation.

## E1010

- Kind: check (error)
- Message: `{ty}` cannot be passed to or from an external function in this version
- `types`: the types that can be passed are Integer, Integer8, Integer16, Integer32, Float, Float32, Boolean, Character, String, Byte, UnsignedInteger16, UnsignedInteger32, UnsignedInteger64, Bytes, List, Map, Set[D] as flags when D is a data type with 1 to 32 constructors that take no arguments, Option, Result, Pair, Triple, records and `data` types with concrete type arguments that are not recursive (including a single-constructor data type passed as a tuple), and Unit as a result
- `recursive`: recursive types cannot be passed, including recursion through List or other types
- `recursive_declaration`: the dependency cycle includes the declaration of `{recursive_name}`
- `flags_count`: flags require a data type with 1 to 32 constructors; empty selections are allowed
- `flags_data`: flags require Set[D], where D is a concrete data type and every constructor takes no arguments
- `map_key`: map keys must be Boolean, an integer of any supported width, Character, or String
- `budget`: types deeper than 100 cannot match any WIT type; larger types are not checked (100,000 expanded nodes per type, 1,000,000 per check); declaration dependency visits also count toward the per-check limit
- `adapter_preparation`: external function `{name}` cannot prepare its generated adapter within the type-walk budget
- `adapter_preparation_budget`: adapter preparation allows 4,000,000 type-node visits per declaration, separately from WIT matching
- `declaration_budget`: external function `{name}` is not checked because the total type expansion budget is exhausted
- `declaration`: external declaration `{name}`
- `user_type`: `{name}` here is the type declared in `{file}`, not `Benitoite.{name}.{name}`

External functions accept only the supported concrete types.

## E1011

- Kind: check (error)
- Message: the function `{name}` of the component `{path}` is asynchronous

External functions must be synchronous in this version.

## E1012

- Kind: check (error)
- Message: the component `{path}` has no resource `{name}`
- `exports`: exported resources: {names}

External types name an exported resource of the component.

## W0301

- Kind: check (warning)
- Message: `{name}` is deprecated
- Label: deprecated
- `declared`: declared deprecated here
- `message`: {message}

The declaration is marked `@deprecated` and may be removed later. The note shows its message.

## W0401

- Kind: check (warning)
- Message: this divides by zero
- Label: the divisor is always 0
- `inferred_range`: the inferred divisor interval is {interval}
- `local_analysis`: this local analysis applies only if execution reaches this operation and all operands are evaluated successfully
- `origin`: this binding or comparison contributes to the inferred interval

Constant evaluation or local interval analysis shows that the divisor is always zero. If execution reaches this operation and all operands are evaluated successfully, the division stops the program at runtime.

## W0402

- Kind: check (warning)
- Message: this calculation overflows `{ty}`
- Label: always overflows
- `inferred_range`: the inferred result interval before the range check is {interval}
- `allowed_range`: the range of `{ty}` is {range}
- `local_analysis`: this local analysis applies only if execution reaches this operation and all operands are evaluated successfully
- `origin`: this binding or comparison contributes to the inferred interval

Constant evaluation or local interval analysis shows that the result does not fit the type. If execution reaches this operation and all operands are evaluated successfully, the calculation stops the program at runtime.

## W0403

- Kind: check (warning)
- Message: the key `{key}` appears more than once
- Label: repeated key
- `first`: first given here

`Map.fromList` keeps the last value for a repeated key and `Set.fromList` keeps one element, so a repeated key in a list literal is likely a mistake.

## W0404

- Kind: check (warning)
- Message: argument `{parameter}` to `{function}` is always out of range
- Label: always out of range
- `inferred_range`: the inferred interval is {interval}
- `allowed_range`: the allowed range is {range}
- `local_analysis`: this local analysis applies only if execution reaches this call and all arguments are evaluated successfully
- `origin`: this binding or comparison contributes to the inferred interval

Local interval analysis shows that the argument is always out of range. If execution reaches this call and all arguments are evaluated successfully, the call stops the program at runtime.

## W0501

- Kind: check (warning)
- Message: `uses IO.All` allows more effects than this function performs
- `narrow`: write `uses {effects}`
- `remove`: remove `uses IO.All`; this function performs no effects
- `covered`: remove this `IO.All`; the remaining declarations cover this function's effects
- `shared`: review the file-level IO.All declaration for all functions; no automatic edit is offered
- `file`: IO.All added by this file-level uses declaration

Listing only the effects a function performs shows which operations it needs.

## R0101

- Kind: runtime (error)
- Message: division by zero

Integer and Decimal division, `mod`, `Integer.floorDivide`, and `Integer.floorModulo` by zero stop the program.

## R0102

- Kind: runtime (error)
- Message: integer overflow

The result does not fit in a 64-bit signed integer.

## R0103

- Kind: runtime (error)
- Message: Decimal overflow

The result does not fit in a `Decimal` even after rounding.

## R0201

- Kind: runtime (error)
- Message: failed to write to standard output
- `reason`: {reason}

Writing to standard output failed, for example because the reading side of a pipe was closed.

## R0202

- Kind: runtime (error)
- Message: failed to write to standard error
- `reason`: {reason}

Writing to standard error failed.

## R0301

- Kind: args (error)
- Message: command-line argument {index} is not valid UTF-8

Arguments passed to a script must be valid UTF-8.

## R0401

- Kind: runtime (error)
- Message: failed to release `{resource}` opened at {location}
- Label: released here
- `failure`: failed to release `{resource}` opened at {location}: {reason}
- `reason`: {reason}

Releasing a resource failed when its `with` block ended, when its task was cancelled, or when a handler discarded the computation.

## R0402

- Kind: runtime (error)
- Message: `{resource}` is used after it was released

A resource cannot be used after its `with` block has ended.

## R0501

- Kind: runtime (error)
- Message: `resume` was called twice in one clause

A continuation can be resumed at most once.

## R0502

- Kind: runtime (error)
- Message: the handler for `{operation}` cannot be used in a task
- `clause`: the clause is here
- `tail`: a handler inherited by a task must end its clause with `resume`

Tasks inherit only handlers whose clauses end by calling `resume`.

## R0601

- Kind: runtime (error)
- Message: the external function `{function}` trapped: {trap}
- `component`: component `{component}`, function `{export}`

A WebAssembly trap or a Component Model ABI violation stops execution. Guest panics and Rust allocation failures can also become an unreachable trap.

## R0701

- Kind: runtime (error)
- Message: argument {index} of `{function}` is out of range

The function accepts only the values described in its documentation for this argument.

## R0801

- Kind: runtime (error)
- Message: a response was already sent for this request

`Http.respond` can be called once for each exchange.

## R0901

- Kind: resource (error)
- Message: the call stack is too deep
- `frames`: {frames} calls were active
- `raise`: use tail calls, or raise the limit with `--max-call-stack`

Nested non-tail calls exceeded the call stack limit.

## R0902

- Kind: resource (error)
- Message: `{function}` would create a value that is too large
- `size`: the result would have {size} {unit}; the limit is {limit}

A single operation cannot build a string or `Bytes` larger than 2^30 bytes, or a list longer than 2^24 elements.

## R0903

- Kind: resource (error)
- Message: `{function}` read more than the limit of {limit} {unit}

A single read cannot produce a value larger than the size limit. Read the input in parts.

## R0904

- Kind: resource (error)
- Message: the external function `{function}` used up its fuel limit of {limit}
- `component`: component `{component}`

Each external call has a default fuel limit of 10^10, independent of elapsed time. The user running the script can change it with `--wasm-fuel`.

## R0905

- Kind: resource (error)
- Message: the external function `{function}` exceeded its {budget} limit of {limit} bytes
- `component`: component `{component}`
- `linear_memory`: total guest linear memory is limited to {limit} bytes per call, or over the lifetime of a stateful instance
- `adapter_allocation`: the external function `{function}` could not allocate generated adapter memory for {requested} bytes
- `adapter_memory`: the generated adapter allocator's memory.grow failed while reserving {requested} bytes, with {allocated} bytes already allocated; adapter memory also counts toward the Store's total linear-memory limit
- `adapter_store_memory`: this call uses generated adapter memory as well as guest memory; both count toward the Store's total linear-memory limit
- `adapter_memory32`: the generated adapter allocator requested {requested} bytes, exceeding the memory32 address limit of {limit} bytes
- `transfer`: values passed to or from an external function are limited to {limit} bytes in their host representation, in each direction

By default, external calls limit total guest linear memory to 64 MiB and each direction of value transfer to a 256 MiB budget. Generated adapters count the complete transfer buffer and separately limit logical value construction to the same budget. The user running the script can change these limits with `--wasm-memory` and `--wasm-transfer`.

## R0906

- Kind: resource (error)
- Message: the external function `{function}` needed {size} {resource}; the limit is {limit}
- `component`: component `{component}`
- `component_handles`: the external function `{function}` exceeded the limit of {limit} component handles

Each Store limits core instances, memories, and tables to 16 each, each guest table to 10,000 elements, and simultaneously live component handles to 100,000. Stateful calls retain the live handle count across calls; dropping a handle returns capacity. Reduce the component's resource requirements.

## R0907

- Kind: resource (error)
- Message: `{function}` could not reserve buffer memory
- `capacity`: the requested buffer capacity was {requested_bytes} bytes

The logical length was within the value size limit, but the host could not reserve the requested buffer capacity. This is not a process-wide memory usage measurement.

## R1001

- Kind: runtime (error)
- Message: no task can proceed because tasks are waiting for each other

Every remaining task waits for another task, so the program cannot continue.

## R1002

- Kind: runtime (error)
- Message: the awaited task was cancelled, so it has no result

`Task.await` cannot return a value for a task that was cancelled. A `Task` value used outside its `with` block may refer to such a task.

## L0101

- Kind: limit (error)
- Message: the function `{name}` needs too many registers
- `limit`: a function can use at most 65535 registers; split it into smaller functions

The bytecode addresses registers with 16 bits.

## L0102

- Kind: limit (error)
- Message: the function `{name}` has too many constants
- `limit`: a function can have at most 65536 constants; split it into smaller functions

The bytecode addresses constants with 16 bits.

## L0103

- Kind: limit (error)
- Message: the function `{name}` has too many `match` expressions
- `limit`: a function can have at most 65536 switch tables; split it into smaller functions

The bytecode addresses the switch tables of a function with 16 bits.

## L0104

- Kind: limit (error)
- Message: the function `{name}` has too many `handle` expressions
- `limit`: a function can have at most 65536 handlers; split it into smaller functions

The bytecode addresses the handler descriptions of a function with 16 bits.

## L0105

- Kind: limit (error)
- Message: the program has too many constructors
- `limit`: a program can have at most 65535 constructors

The bytecode addresses constructors with 16 bits.

## L0106

- Kind: limit (error)
- Message: the program uses too many built-in functions
- `limit`: a program can use at most 65535 built-in functions

The bytecode addresses built-in functions with 16 bits.

## L0107

- Kind: limit (error)
- Message: the program has too many implementations
- `limit`: a program can have at most 65535 implementations

The bytecode addresses implementations with 16 bits.

## L0108

- Kind: limit (error)
- Message: the program has too many effect operations
- `limit`: a program can have at most 65535 effect operations

The bytecode addresses effect operations with 16 bits.

## L0109

- Kind: limit (error)
- Message: the trait `{name}` has too many methods
- `limit`: a trait can have at most 65535 methods

The bytecode addresses the methods of a trait with 16 bits.

## L0110

- Kind: limit (error)
- Message: the trait `{name}` has too many supertraits
- `limit`: a trait can have at most 65535 supertraits

The bytecode addresses the supertraits of a trait with 16 bits.

## L0111

- Kind: limit (error)
- Message: an implementation of `{name}` has too many constraints
- `limit`: an implementation can have at most 65535 constraint dictionaries

The bytecode addresses the dictionaries of an implementation with 16 bits.

## L0113

- Kind: limit (error)
- Message: the program uses too many external functions
- `limit`: a program can use at most 65535 external functions

The bytecode addresses external functions with 16 bits.

## L0112

- Kind: limit (error)
- Message: the definition `{name}` is too large to compile
- `definition_final`: the final match decisions in this definition exceed their node or edge budget; split it into smaller functions
- `nodes`: the final match decision exceeds its node budget; split it into smaller matches
- `edges`: the final match decision exceeds its edge budget; split it into smaller matches
- `table`: the match decision requires more than 64 MiB of working tables; simplify the match
- `instructions`: the definition requires more than 4,194,304 generated instructions; split it into smaller functions

The compiler shares match decisions and falls back to matching rows in source order; final decision nodes, edges, working tables, and generated instructions remain bounded.
