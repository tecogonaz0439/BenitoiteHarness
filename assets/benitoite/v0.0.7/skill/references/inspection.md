# Inspecting before execution

While editing, use ordinary `benitoite check main.bnt` or non-executing `benitoite test --check tests/` and fix the diagnostics. Once the checks pass, obtain one JSON report immediately before confirmation. Inspect that report, explain the declared effects to the user using the Skill's confirmation rules, then pass the accepted set to execution:

```sh
benitoite --version=json
benitoite check --report=json main.bnt
benitoite run --allow-effects=Console.Write,Process.Exit main.bnt
benitoite test --check --report=json tests/
benitoite test --allow-effects=Assert.Check,File.Read tests/
```

The allowed sets above are examples; use the set reviewed for the actual program. For pure execution, write `benitoite run --allow-effects= pure.bnt`. An omitted option imposes no effect limit. `run` options go before the script path; following arguments belong to the script. `test` options can also follow paths.

The version query accepts no following arguments and returns one JSON object with `schemaVersion: 1`, `kind: "version"`, `name`, `version`, `unicodeVersion`, `build`, `formats`, and `loaderEnvironment`. Require the needed format keys and integer versions that you understand, rather than inferring capabilities from the release number. Current format keys are `version`, `diagnostics`, `testResults`, `checkReport`, `testCheckReport`, `effectLimit`, `loaderEnvironment`, `skillList`, and `skillDocument`, each with `[1]`. The existing diagnostic and test-result JSON Lines have no added schema field or initial version line; their format includes die diagnostics and test `failure.die`. `effectLimit` versions an option and its diagnostic contract, not a new output object. Unknown formats, statuses, or effects are unsupported, not success or an empty set.

Build fields are `commit`, `dirty`, `target`, and `provenance: {commit, dirty}`. A missing commit is `null`; dirty is a boolean or `null`, where `null` means unknown. Dirty state is not collected automatically: an omitted build input has provenance `notCollected`. Explicit build-time `BENITOITE_BUILD_COMMIT` (a full 40- or 64-digit hexadecimal ID or `unknown`) and `BENITOITE_BUILD_DIRTY` (`true`, `false`, or `unknown`) take precedence independently and have provenance `environment`. Otherwise commit lookup uses Git when available, with provenance `git` or `unknown`; the target comes from Cargo. Runtime environment changes do not change embedded information.

Automatic commit lookup monitors existing HEAD, branch ref, packed-refs, and HEAD reflog files, not the index or working files. Changes outside those monitored files can go unnoticed. Change an explicit build input or clean-build the package to recollect information when needed. A build's commit and dirty values neither prove its complete source identity nor attest to executable trust.

Both inspection commands check without executing user functions, generating Benitoite bytecode, or starting a VM. Compile-time constant evaluation and static checks of external declarations still occur; a WASM component can be compiled for checking, without instantiation or calls. Inspection does not prove later code generation will fit the compiler's limits. `check` requires `main`; `test --check` does not. Test selection matches executing `test`: only tests declared in each selected entry count; imported tests do not. Direct files use their parent as root; directory discoveries share that directory's root, skip `package.bnt` and nested packages, and use lexical file order. Explicit paths keep argument order and duplicates.

`--report=json` requires `=`, accepts no other value, and writes one UTF-8 JSON object followed by LF to standard output. Diagnostics, including warnings and recognized usage errors, are inside it, without duplicates on standard error. `--diagnostics=json` can be added but does not add JSON Lines; explicit `--diagnostics=text` conflicts with report mode. Plain `test --check` writes only diagnostics on standard error, with empty standard output. `test --check` rejects `--allow-effects`, `--max-call-stack`, and all `--wasm-*` options, even with default values. `test --report=json` without `--check`, and reports for `run` or `fmt`, are usage errors.

Before using an inspection as a confirmed set, require a complete single JSON object, a known `schemaVersion` and `kind`, `status: "ok"`, `complete: true`, and exit status 0. These fields guide interpretation:

| Field | Meaning |
|---|---|
| `kind` | `checkReport` for main, `testCheckReport` for tests; both use `schemaVersion: 1`. |
| `producer` | `{name, version, build}`, matching the version query. |
| `executed` | Always `false`; no user function ran. |
| `targetsComplete` | Entry selection or discovery finished; this alone does not mean checks succeeded. |
| `requestedTargets`, `targets` | Supplied paths with their indices, and selected entries with `targetId`, `requestIndex`, `entry`, `root`, `sourceId`, `kind`, `status`, and `complete`. |
| `main`, `tests` on a target | Main's symbol, location, effects, and shell reference; or a declaration-ordered list of test function names, descriptions, locations, and effects. Failed or uninspected contracts are `null`. |
| `aggregateEffects` | Main's declared effects or the union of all selected tests' declared effects; `null` unless the whole inspection is complete. |
| `sources`, `sourceSets` | Source IDs, display paths, kinds, and hashes, with source IDs grouped by target. |
| `diagnostics`, `diagnosticTargets` | Diagnostic objects and their target associations; discovery-wide and usage errors have no target. |

An Effects object has `basis: "declaredUpperBound"` and `expanded`, a lexical, unique array of canonical atomic names. It includes unused declarations, not just effects reached by executing the body. `IO.All` expands to `Clock.Time`, `Console.Read`, `Console.Write`, `File.Read`, `File.Write`, `Process.Environment`, `Process.Exit`, `Process.Run`, `Random.Generate`, and `State`. HTTP effects and `Assert.Check` are not in `IO.All`. Keep `Assert.Check` in test contracts, their union, and the allowed set; it is forbidden on `main` (E0415). Unhandled `or die` needs `Process.Exit`; a fully handled exit removed from the declared bound is absent. Bare `try` and `or return` add no exit effect.

A pure success has `expanded: []`; unknown effects are `null`, not `[]`. A checked file without tests has `tests: []`; an unknown test list is `null`. An empty directory is a complete success with no targets and an empty aggregate. A failed file leaves other successful contracts available, but the overall `complete` is false and the aggregate is null. Discovery failure stops before any entry is checked; already discovered entries remain incomplete reference information. Usage errors have no inspected targets or sources. Their embedded diagnostic alone uses `kind: "check"` with `code: null`; this combination does not extend ordinary diagnostic JSON Lines. Missing test descriptions are `null`, while explicitly empty descriptions are `""`.

`processShellReference` is `present`, `absent`, or `unknown`, with `shellReferenceScope: "checkedProgram"`. It lives inside main's contract or on the test target, not on individual test functions. It covers expression references to the standard shell function across the checked program, including unused functions, function values, and dependencies. Imports, comments, strings, a handler operation name alone, and unrelated user functions named shell do not count. This is not reachability analysis: `present` does not prove invocation, and `absent` does not prevent external commands from starting a shell.

Source hashes use `{algorithm: "sha256", value: "<64 lowercase hexadecimal digits>"}` over the full original bytes used by the check, including BOM, original line endings, and invalid UTF-8 without normalization. Sources include settings, dependencies, and embedded prelude sources, but exclude WASM binaries, files read only to verify dependency hashes, missing settings, directory observations, and sources whose full bytes were not retained. IDs are local to each report; repeated paths remain separate. Display paths cannot always be reversed to OS paths. Neither these hashes nor an inspection fixes the bytes used by a later run.

The checks extend through external-function matching. Inspection exits with 0 for `ok`, 2 for `checkError` or `usageError`, 3 for `internalError`, and 130 for an observed test-inspection interruption. `--deny-warnings` promotes warnings to check errors. Test inspection checks interruption before each file; a request during the last file may go unreported. Main inspection retains OS-default signal handling. Test-inspection status precedence is interruption, internal error, usage error, then check error. Write or flush failure exits with 1 and can leave incomplete output; initialization failures, aborts, forced termination, or failures outside the report boundary also need not return a report. Never accept an incomplete response.

`--allow-effects` accepts one comma-separated set of canonical atomic names, with `State` and `Assert.Check` included. Both `--allow-effects=<names>` and `--allow-effects <names>` work. Duplicate options or names, empty elements, whitespace, unknown or user-defined names, import aliases, `IO.All`, and `*` are usage errors. The newly checked declared bound must be a subset of the accepted set; extra allowed names are fine. Earlier check errors take precedence over comparison. A missing effect produces one E0512 per offending function with the missing effects, allowed set, and location, then exits with 2 before execution. Ordinary diagnostics go to standard error, including JSON Lines with `--diagnostics=json`.

`run` executes the same checked program whose bound was compared. With a limit, `test` prepares, compares, and compiles all entries before starting any test. If any entry fails preparation, none run. When discovery finished, executing test JSON Lines contain only a `testSummary` with zero passed and failed, `filesNotRun` equal to all unstarted entries (including duplicates and files without tests), and `interrupted: false` for a check rejection. An observed interruption uses `interrupted: true` and status 130; an internal error uses status 3. Discovery and usage errors or outer failures with unknown target counts write no summary. Check status, summary presence, `filesNotRun`, and `interrupted`, not just passed and failed. Executing test status precedence is 130, 3, 2, 1, then 0; output failure exits with 1. Inspection never writes test-result or summary lines.

An effect limit compares static contracts; it does not restrict runtime operations or ensure intended results, termination, or successful IO. The same `File.Write` can write another path or contents, and the same `Process.Run` can start another command. Different test entries can observe different dependency contents during preparation; all-input snapshot consistency is not guaranteed. Keep actual operations and resource isolation in the harness or OS.

To retrieve documentation without installing it:

```sh
benitoite skill list --format=json
benitoite skill show --format=json references/stdlib/IO.Process.md
```

`list` returns `schemaVersion: 1`, `kind: "skillList"`, `producer`, and a lexical `documents` array of `{name, byteLength, contentHash}`. `show` returns `schemaVersion: 1`, `kind: "skillDocument"`, `producer`, `name`, `byteLength`, `contentHash`, and `text`. Lengths count rendered UTF-8 bytes and SHA-256 hashes cover those same bytes, after version replacement in `SKILL.md`; `install` writes identical text. These queries read embedded documents without inspecting `HOME`, the working directory, or installed copies. Names, including licenses, must match the list exactly, with case-sensitive `/` components. Empty names, leading `/`, empty components, `.` or `..`, backslashes, and colons are invalid; names are never opened as OS paths.

Text is the default format: `list` prints one name per line and `show` only the document, without adding a newline. JSON queries write one object followed by LF. `--format=text|json` requires `=`, may occur once, and may precede or follow the show name. `list` takes no name; `show` requires one. These queries reject installation options, and install/uninstall reject `--format`. Success exits with 0; invalid syntax or an unknown document exits with 2, empty standard output, and a standard-error explanation even for JSON queries. Internal errors exit with 3 and output failures with 1; do not interpret missing output as an empty document. Every name returned by list or show satisfies these same rules; invalid or duplicate embedded names cause an internal error instead of a partial list.

