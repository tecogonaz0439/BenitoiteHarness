# Benitoite grammar

The base grammar of Benitoite in EBNF. `{ ... }` means zero or more repetitions, `[ ... ]` means optional, and `|` separates alternatives. `NL` is a line break that separates items. `CommaList(X)` is a comma-separated list of `X` that may be empty and may end with a comma; `LineList(X)` is a list of `X` separated by line breaks.

Identifiers exclude the 36 keywords listed in [Keywords as names](common-mistakes.md#keywords-as-names), except that `record` is also accepted where a lowercase name is required (including parameters, labels, bindings, and value references). `die` cannot be a name, including in labels, fields, import aliases, and names after a dot. It is accepted only as a failure action of `try`, not as a standalone expression. `fn`, `pub`, `impl`, `mut`, `async`, `await`, `spawn`, and `select` remain ordinary identifiers. `as`, file-header words, built-in constraint names, and attribute names are read specially only in their own contexts. `extensible` is an ordinary identifier. The argument-free `@extensible` attribute applies to data and record declarations. Outside their defining package, patterns must cover future shapes with an unguarded variable or `_`. The argument-free `@opaque` attribute applies to public or package-public data and record declarations. It exposes the type and ordinary values while restricting constructors, record updates, representation patterns, and field accessors to the declaring module. Other modules of the same package are outside this boundary; use public construction and inspection functions.

A physical line break immediately after `return` always ends the statement, even inside parentheses or before an operator on the next line. Comments and blank lines do not change this rule. For an ordinary `return`, start the required value on the same physical line (otherwise E0201). Once the value has started, ordinary multiline continuation is allowed after operators, inside parentheses or call arguments, and after `then`. A short lambda must still fit on one physical line.

`try e or return` explicitly returns a failure to the caller, with the same types and evaluation as bare `try e`. Bare try is permanently fixed to returning failure; it never infers an exit. Prefer explicit return in reusable functions, libraries, and callbacks, and die in entry points that should end the run. Neither returning form requires Show. The return action takes no value. `TryOperand` includes the whole expression, including pipes, but stops before `or return` or `or die` at the same depth. A nested `try` takes its own action first; parentheses, arguments, lambdas, and blocks have their own expression boundaries. Other `or` expressions stay Boolean operators. Line breaks before and after `or` continue the expression; a line break immediately after the action's `return` ends it, even inside parentheses. Put a comma or closing delimiter on that same line. Parenthesize an explicit try before continuing it with an operator, call, or type application: `(try r or return) + 1`. The formatter preserves bare and explicit forms.

`try e or die` and `try e or die(message)` keep the enclosing return type unrestricted and add `Process.Exit`. Import `Benitoite.Unofficial.IO.Process` to write `uses Process.Exit`; the internal dependency does not import that name. Result errors require `Trait.Show`; Option does not. The optional message must be a pure String. Success evaluates neither message nor Show. Failure evaluates the message, then Show (or a fixed cause for None), then `Process.exit(1)`. Start the message's opening parenthesis on the same physical line as `die`. Outside parentheses, a line break after `die` or its message ends the statement. Direct `return`, `try`, and outer `resume` are forbidden in the message; a lambda written there has its own return boundary. A short lambda must include the entire message on its physical line. The default exit cancels remaining tasks, releases resources, transfers output, then reports the source location, optional message, and cause once, with status 1. No Console.Write is required for this host report. Release and flush failures follow as supplements without changing status 1. A Process.Exit handler receives the ordinary exit code; the handled call's report is discarded and never attached to a later exit. A test using die fails and the runner continues with the next test. Purity does not guarantee termination of message or Show, and static checking does not guarantee intended results or success of external operations.

A short lambda contains exactly one statement, and the entire lambda (including its header and any nested syntax) must fit on one physical line. Its expression uses the ordinary precedence; outer commas, closing delimiters, branch/block boundaries, and the end of the line or file end it. Use `return` to return a value. For a multiline body, start the body on the line after the header and close it with `end lambda`. A body starting on the header's last line takes an explicit `end lambda` only immediately after its statement on that same physical line. It never takes a terminator on a later line; blank lines do not affect which lambda a terminator closes. When either lambda in a directly nested pair omits its terminator, parenthesize the inner lambda. Call arguments and binding statements provide independent boundaries. Prefer the short form for a one-statement body; explicit terminators are valid and the formatter preserves the chosen form. Before adding another statement, start the body after the header and add `end lambda`.

For selecting a value or action, prefer `match`. Use `if` without `else` for conditional work or an early return. `else`, `else if`, and `shadow` are valid but are discouraged; these recommendations add no warnings. Prefer a fresh `bind` name for each new value. Preserve condition evaluation count and order, evaluate only the selected branch, and keep the target of `return`; nest boolean matches for effectful conditions instead of moving them into guards. For a final catch-all branch, prefer an ordinary binding name such as `other` or `otherCode` that does not collide with outer local names or visible constants, and put no unreachable branch after it. `case _ ->` is valid. This advice does not apply to `_` inside constructors, discarded bindings, or partial applications.

Function types require a name for every parameter. Ordinary functions with two or more parameters require named arguments; single-parameter functions accept either positional or named arguments. Parameter names must match even for a single positional higher-order argument. Ordinary calls accept named values such as `f(b: readB(), a: readA())`. Labels name parameters; they do not refer to local variables. The callee is evaluated first, then argument expressions in source order; only the saved values are placed in declaration order. Unknown, repeated, missing, or mixed named/positional arguments are errors. Function-type parameter names and lambda parameter names must match the contract; names are never inferred from the order of call labels. Direct data constructors use positional fields, while record construction uses named fields. Calls through an unannotated parameter wait for a known contract from constraints in the same function body. If the names and order remain unknown, E0455 requires a function-type annotation with parameter names, even for an unused lambda or a single positional argument. Contracts are not generalized or supplied later by callers of an already checked function. Named holes preserve the original parameter names in source order: `f(b: _, a: _)` creates a `function(b: B, a: A) -> R`. The callee and fixed arguments run inside that generated function, when it is called; `return` and `try` target it, and `resume` cannot cross its boundary. A pipe first evaluates its left side once, then its right callee and explicit arguments. Without direct holes it supplies parameter 0: `xs |> List.map(f: action)`. Explicitly supplying that parameter is a duplicate; remaining parameters require labels. With direct holes, or a parenthesized right side, the pipe instead calls the resulting function with one argument. It never fills an arbitrary free position or automatically partially applies a function. Constructor values use `arg1`, `arg2`, etc.; record constructor values use field names. Direct constructor holes keep their original position names.

```text
Program     = [ NL ] [ ProgramBody [ NL ] ] .
ProgramBody = Header [ NL Items ]
            | Items .
Header      = HeaderItem { NL HeaderItem } .
HeaderItem  = "language" StringLit
            | "dependency" UpperIdent NL { HeaderField NL } "end" "dependency"
            | "package" UpperIdent NL { HeaderField NL } "end" "package" .
HeaderField = LowerIdent StringLit .
Items       = ImportDecl [ NL Items ]
            | FileUses [ NL Decls ]
            | Decls .
FileUses    = "uses" EffectRef { "," EffectRef } [ "," ] .
Decls       = AttrDecl { NL AttrDecl } .
AttrDecl    = { Attribute [ NL ] } Decl .
Attribute   = "@" LowerIdent [ "(" CommaList(StringLit) ")" ] .
Visibility  = "public" [ "(" "package" ")" ] .
Decl        = [ Visibility ] FnDecl
            | [ Visibility ] ExternalFnDecl
            | [ Visibility ] ExternalTypeDecl
            | [ Visibility ] ConstDecl
            | [ Visibility ] DataDecl
            | [ Visibility ] AliasDecl
            | [ Visibility ] RecordDecl
            | [ Visibility ] TraitDecl
            | [ Visibility ] EffectDecl
            | ImplDecl .

ImportDecl  = "import" UpperIdent { "." UpperIdent } [ "as" UpperIdent ] .

FnDecl      = "function" LowerIdent [ FnTypeParams ] "(" CommaList(Param) ")"
              "->" Type [ Uses ] Body "end" "function" .
ExternalFnDecl = "function" LowerIdent "(" CommaList(Param) ")" "->" Type [ Uses ] .
ExternalTypeDecl = "type" UpperIdent .
FnTypeParams = "[" FnTypeParam { "," FnTypeParam } [ "," ] "]" .
FnTypeParam = "effect" UpperIdent
            | TypeParamDecl [ ":" Constraint { "&" Constraint } ] .
Constraint  = QualUpper | LowerIdent .
TypeParamDecl = UpperIdent [ "[" "_" { "," "_" } "]" ] .
Param       = LowerIdent ":" Type .
Uses        = "uses" EffectRef { "," EffectRef } .
EffectRef   = QualUpper [ "[" CommaList(Type) "]" ] .

ConstDecl   = "const" LowerIdent ":" Type "=" Expr .

DataDecl    = "data" UpperIdent [ TypeParams ] LineList(Variant) "end" "data" .
AliasDecl   = "type" UpperIdent [ TypeParams ] "=" Type .
TypeParams  = "[" UpperIdent { "," UpperIdent } [ "," ] "]" .
Variant     = UpperIdent [ "(" CommaList(Type) ")" ] .
RecordDecl  = "record" [ NL ] UpperIdent [ TypeParams ] LineList(Field) "end" "record" .
Field       = LowerIdent ":" Type .
TraitDecl   = "trait" UpperIdent "[" TypeParamDecl [ ":" QualUpper { "&" QualUpper } ] "]"
              LineList(MethodSig) "end" "trait" .
MethodSig   = "function" LowerIdent [ FnTypeParams ] "(" CommaList(Param) ")" "->" Type [ Uses ] .
ImplDecl    = "implement" [ FnTypeParams ] QualUpper "[" Type "]" LineList(FnDecl) "end" "implement" .
EffectDecl  = "effect" UpperIdent LineList(OpSig) "end" "effect" .
OpSig       = "function" LowerIdent [ FnTypeParams ] "(" CommaList(Param) ")" "->" Type .

Type        = QualUpper [ "[" Type { "," Type } [ "," ] "]" ]
            | "function" "(" CommaList(NamedType) ")" "->" Type [ Uses ]
            | "(" Type ")" .
QualUpper   = UpperIdent { "." UpperIdent } .

Body        = LineList(Stmt) .
Stmt        = BindStmt | Expr .
BindStmt    = ( "bind" | "shadow" ) Pattern [ ":" Type ] "<-" Expr .

Expr        = "return" Expr
            | "try" TryOperand [ "or" TryAction ]
            | PipeExpr .
TryOperand  = Expr .
TryAction   = "return" | "die" [ "(" Expr ")" ] .
PipeExpr    = OrExpr { "|>" OrExpr } .
OrExpr      = AndExpr { "or" AndExpr } .
AndExpr     = CmpExpr { "and" CmpExpr } .
CmpExpr     = AddExpr [ CmpOp AddExpr ] .
CmpOp       = "=" | "<>" | "<" | "<=" | ">" | ">=" .
AddExpr     = MulExpr { ( "+" | "-" ) MulExpr } .
MulExpr     = UnaryExpr { ( "*" | "/" | "div" | "mod" ) UnaryExpr } .
UnaryExpr   = ( "-" | "not" ) UnaryExpr | CallExpr .
CallExpr    = Primary { "(" CommaList(Arg) ")" } .
Arg         = [ LowerIdent ":" ] ( Expr | "_" ) .
NamedType   = LowerIdent ":" Type .
Primary     = Literal
            | InterpString
            | Name
            | "(" ")"
            | "(" Expr ")"
            | "[" [ ListElem { "," ListElem } [ "," ] ] "]"
            | IfExpr
            | MatchExpr
            | Lambda
            | RecordExpr
            | "block" Body "end" "block"
            | "lazy" Body "end" "lazy"
            | WithExpr
            | HandleExpr
            | "resume" "(" Expr ")" .
Literal     = IntLit | FloatLit | DecimalLit | StringLit | CharLit | "true" | "false" .
InterpString = StrStart Expr [ ":" FormatSpec ] { StrMid Expr [ ":" FormatSpec ] } StrEnd .
FormatSpec  = [ Align ] [ Sign ] [ "#" ] [ "0" ] [ Width ] [ Precision ] [ Kind ] .
Align       = "<" | ">" | "^" .
Sign        = "+" .
Width       = NonZeroDigit { Digit } .
Precision   = "." Digit { Digit } .
Kind        = "d" | "b" | "o" | "x" | "X" | "f" | "s" | "c" .
NonZeroDigit = "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" .
Digit       = "0" | NonZeroDigit .
ListElem    = Expr | ".." Expr .
Name        = { UpperIdent "." } ( LowerIdent | UpperIdent ) .
RecordExpr  = QualUpper "(" [ ".." Expr "," ] FieldArg { "," FieldArg } [ "," ] ")" .
FieldArg    = LowerIdent ":" Expr .
IfExpr      = "if" Expr "then" Body { "else" "if" Expr "then" Body } [ "else" Body ] "end" "if" .
MatchExpr   = "match" Expr "with" [ NL ] Arm { NL Arm } [ NL ] "end" "match" .
Arm         = "case" Pattern { "," Pattern } [ "if" Expr ] "->" ArmBody .
ArmBody     = Stmt { NL Stmt } .
Lambda      = LambdaHeader ( Body "end" "lambda" | ShortBody ) .
LambdaHeader = "lambda" "(" CommaList(LambdaParam) ")" [ "->" Type [ Uses ] ] .
ShortBody   = Stmt .
LambdaParam = LowerIdent [ ":" Type ] .
WithExpr    = "with" WithBind { "," WithBind } "do" Body "end" "with" .
WithBind    = LowerIdent "=" Expr .
HandleExpr  = "handle" Body "with" [ NL ] HandleArm { NL HandleArm } [ NL ] "end" "handle" .
HandleArm   = "case" OpName "(" CommaList(OpParam) ")" "->" ArmBody .
OpName      = { UpperIdent "." } LowerIdent .
OpParam     = LowerIdent | "_" .

Pattern     = "_"
            | LowerIdent
            | [ "-" ] IntLit
            | StringLit
            | CharLit
            | "true" | "false"
            | "(" ")"
            | QualUpper [ "(" CommaList(Pattern) ")" ]
            | QualUpper "(" FieldPat { "," FieldPat } [ "," ".." ] [ "," ] ")"
            | RangeEnd ".." RangeEnd
            | "[" [ ListPatElem { "," ListPatElem } [ "," ] ] "]" .
FieldPat    = LowerIdent ":" Pattern .
RangeEnd    = [ "-" ] IntLit | CharLit .
ListPatElem = Pattern | ".." [ LowerIdent ] .
```
