# Inspecting before execution

The harness uses inspection reports to explain declared effects to the user before execution. While editing, use ordinary `benitoite check main.bnt` or non-executing `benitoite test --check tests/` and fix the diagnostics. Once the checks pass, obtain one JSON report immediately before confirmation. Inspect that report, explain the declared effects to the user using the Skill's confirmation rules, then pass the accepted set to execution:

```sh
benitoite --version=json
benitoite check --report=json main.bnt
benitoite run --allow-effects=Console.Write,Process.Exit main.bnt
benitoite test --check --report=json tests/
benitoite test --allow-effects=Assert.Check,File.Read tests/
```

The allowed sets above are examples; use the set reviewed for the actual program. For pure execution, write `benitoite run --allow-effects= pure.bnt`. An omitted option imposes no effect limit. `run` options go before the script path; following arguments belong to the script. `test` options can also follow paths.

The version query accepts no following arguments and returns one JSON object with `schemaVersion: 1`, `kind: "version"`, `name`, `version`, `unicodeVersion`, `build`, `formats`, and `loaderEnvironment`. Require the needed format keys and integer versions that you understand, rather than inferring capabilities from the release number. Current format keys are `version`, `diagnostics`, `testResults`, `checkReport`, `testCheckReport`, `effectLimit`, `loaderEnvironment`, `skillList`, and `skillDocument`, with `[1,2]` for `testCheckReport` and `testResults` and `[1]` for the other keys. The existing diagnostic and test-result JSON Lines have no added schema field or initial version line; their format includes die diagnostics and test `failure.die`. `effectLimit` versions an option and its diagnostic contract, not a new output object. Unknown formats, statuses, or effects are unsupported, not success or an empty set.

Build fields are `commit`, `dirty`, `target`, and `provenance: {commit, dirty}`. A missing commit is `null`; dirty is a boolean or `null`, where `null` means unknown. Dirty state is not collected automatically: an omitted build input has provenance `notCollected`. Explicit build-time `BENITOITE_BUILD_COMMIT` (a full 40- or 64-digit hexadecimal ID or `unknown`) and `BENITOITE_BUILD_DIRTY` (`true`, `false`, or `unknown`) take precedence independently and have provenance `environment`. Otherwise commit lookup uses Git when available, with provenance `git` or `unknown`; the target comes from Cargo. Runtime environment changes do not change embedded information.

Automatic commit lookup monitors existing HEAD, branch ref, packed-refs, and HEAD reflog files, not the index or working files. Changes outside those monitored files can go unnoticed. Change an explicit build input or clean-build the package to recollect information when needed. A build's commit and dirty values neither prove its complete source identity nor attest to executable trust.

Both inspection commands check without executing user functions, generating Benitoite bytecode, or starting a VM. Compile-time constant evaluation and static checks of external declarations still occur; a WASM component can be compiled for checking, without instantiation or calls. Inspection does not prove later code generation will fit the compiler's limits. `check` requires `main`; `test --check` does not. Test selection matches executing `test`: only tests declared in each selected entry count; imported tests do not. Direct files use their parent as root; directory discoveries share that directory's root, skip `package.bnt` and nested packages, and use lexical file order. Explicit paths keep argument order and duplicates.

`--report=json` requires `=`, accepts no other value, and writes one UTF-8 JSON object followed by LF to standard output. Diagnostics, including warnings and recognized usage errors, are inside it, without duplicates on standard error. `--diagnostics=json` can be added but does not add JSON Lines; explicit `--diagnostics=text` conflicts with report mode. Plain `test --check` writes only diagnostics on standard error, with empty standard output. `test --check` rejects `--allow-effects`, `--max-call-stack`, and all `--wasm-*` options, even with default values. `test --report=json` without `--check`, and reports for `run` or `fmt`, are usage errors.

Before using an inspection as a confirmed set, require a complete single JSON object, a known `schemaVersion` and `kind`, `status: "ok"`, `complete: true`, and exit status 0. These fields guide interpretation:

| Field | Meaning |
|---|---|
| `kind` | `checkReport` for main, `testCheckReport` for tests; use `schemaVersion: 1` by default; `test --doc` uses version 2. |
| `producer` | `{name, version, build}`, matching the version query. |
| `executed` | Always `false`; no user function ran. |
| `targetsComplete` | Entry selection or discovery finished; this alone does not mean checks succeeded. |
| `requestedTargets`, `targets` | Supplied paths with their indices, and selected entries with `targetId`, `requestIndex`, `entry`, `root`, `sourceId`, `kind`, `status`, and `complete`. |
| `main`, `tests` on a target | Main's symbol, location, effects, and shell reference; or a declaration-ordered list of test function names, descriptions, locations, and effects. Failed or uninspected contracts are `null`. |
| `aggregateEffects` | Main's declared effects or the union of all selected tests' declared effects; `null` unless the whole inspection is complete. |
| `sources`, `sourceSets` | Source IDs, display paths, kinds, and hashes, with source IDs grouped by target. |
| `diagnostics`, `diagnosticTargets` | Diagnostic objects and their target associations; discovery-wide and usage errors have no target. |
| `intervalAnalysis` on the report and each target | Additional local interval analysis summary, described below; independent of `complete` and report acceptance. |

An Effects object has `basis: "declaredUpperBound"` and `expanded`, a lexical, unique array of canonical atomic names. It includes unused declarations, not just effects reached by executing the body. `IO.All` expands to `Clock.Time`, `Console.Read`, `Console.Write`, `File.Read`, `File.Write`, `Process.Environment`, `Process.Exit`, `Process.Run`, `Random.Generate`, and `State`. Log, HTTP effects and `Assert.Check` are not in `IO.All`. Log is a canonical atomic name accepted by --allow-effects. Disabled log levels do not remove a declared Log upper bound. Keep `Assert.Check` in test contracts, their union, and the allowed set; it is forbidden on `main` (E0415). Unhandled `or die` needs `Process.Exit`; a fully handled exit removed from the declared bound is absent. Bare `try` and `or return` add no exit effect.

A pure success has `expanded: []`; unknown effects are `null`, not `[]`. A checked file without tests has `tests: []`; an unknown test list is `null`. An empty directory is a complete success with no targets and an empty aggregate. A failed file leaves other successful contracts available, but the overall `complete` is false and the aggregate is null. Discovery failure stops before any entry is checked; already discovered entries remain incomplete reference information. Usage errors have no inspected targets or sources. Their embedded diagnostic alone uses `kind: "check"` with `code: null`; this combination does not extend ordinary diagnostic JSON Lines. Missing test descriptions are `null`, while explicitly empty descriptions are `""`.

File-level `uses` is merged with the individual declaration in the checked Scheme. After accepting the whole report under the rules above, show `targets[0].main.effects.expanded` as main's declared upper bound; tests use each selected test's effects and their aggregate. Even an empty body retains shared Console.Write or Log. Reading only main's source line can miss that bound. Do not turn a report into automatic permission. Ordinary `check` still writes no standard output. The processor owns the existing required fields and acceptance rules: see the [v0.0.12 English reference](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.12/docs/reference/benitoite.md#inspection-reports), with checkReport 1 and testCheckReport 1/2. The additional `intervalAnalysis` field below preserves those versions and rules.

When an unaccepted effect was added by the file declaration, E0512 includes that source location as a secondary label. W0501 for a shared IO.All labels each affected function and the shared origin, with no automatic edit to the common declaration. An edit for an individual IO.All accounts for effects already supplied by the file. Doc examples keep their own imports and contracts; the outer file's shared declaration does not apply to them.

`processShellReference` is `present`, `absent`, or `unknown`, with `shellReferenceScope: "checkedProgram"`. It lives inside main's contract or on the test target, not on individual test functions. It covers expression references to the standard shell function across the checked program, including unused functions, function values, and dependencies. Imports, comments, strings, a handler operation name alone, and unrelated user functions named shell do not count. This is not reachability analysis: `present` does not prove invocation, and `absent` does not prevent external commands from starting a shell.

Source hashes use `{algorithm: "sha256", value: "<64 lowercase hexadecimal digits>"}` over the full original bytes used by the check, including BOM, original line endings, and invalid UTF-8 without normalization. Sources include settings, dependencies, and embedded prelude sources, but exclude WASM binaries, files read only to verify dependency hashes, missing settings, directory observations, and sources whose full bytes were not retained. IDs are local to each report; repeated paths remain separate. Display paths cannot always be reversed to OS paths. Neither these hashes nor an inspection fixes the bytes used by a later run.

The checks extend through external-function matching. Inspection exits with 0 for `ok`, 2 for `checkError` or `usageError`, 3 for `internalError`, and 130 for an observed test-inspection interruption. `--deny-warnings` promotes warnings to check errors. Test inspection checks interruption before each file; a request during the last file may go unreported. Main inspection retains OS-default signal handling. Test-inspection status precedence is interruption, internal error, usage error, then check error. Write or flush failure exits with 1 and can leave incomplete output; initialization failures, aborts, forced termination, or failures outside the report boundary also need not return a report. Never accept an incomplete response.

`--allow-effects` accepts one comma-separated set of canonical atomic names, with `State`, `Log`, and `Assert.Check` included. Both `--allow-effects=<names>` and `--allow-effects <names>` work. Duplicate options or names, empty elements, whitespace, unknown or user-defined names, import aliases, `IO.All`, and `*` are usage errors. The newly checked declared bound must be a subset of the accepted set; extra allowed names are fine. Earlier check errors take precedence over comparison. A missing effect produces one E0512 per offending function with the missing effects, allowed set, and location, then exits with 2 before execution. Ordinary diagnostics go to standard error, including JSON Lines with `--diagnostics=json`.

`run` executes the same checked program whose bound was compared. With a limit, `test` prepares, compares, and compiles all entries before starting any test. If any entry fails preparation, none run. When discovery finished, executing test JSON Lines contain only a `testSummary` with zero passed and failed, `filesNotRun` equal to all unstarted entries (including duplicates and files without tests), and `interrupted: false` for a check rejection. An observed interruption uses `interrupted: true` and status 130; an internal error uses status 3. Discovery and usage errors or outer failures with unknown target counts write no summary. Check status, summary presence, `filesNotRun`, and `interrupted`, not just passed and failed. Executing test status precedence is 130, 3, 2, 1, then 0; output failure exits with 1. Inspection never writes test-result or summary lines.

An effect limit compares static contracts; it does not restrict runtime operations or ensure intended results, termination, or successful IO. The same `File.Write` can write another path or contents, and the same `Process.Run` can start another command. Different test entries can observe different dependency contents during preparation; all-input snapshot consistency is not guaranteed. Keep actual operations and resource isolation in the harness or OS.

To retrieve documentation without installing it:

```sh
benitoite skill list --format=json
benitoite skill show --format=json references/stdlib/IO.Process.md
```

`list` returns `schemaVersion: 1`, `kind: "skillList"`, `producer`, and a lexical `documents` array of `{name, byteLength, contentHash}`. `show` returns `schemaVersion: 1`, `kind: "skillDocument"`, `producer`, `name`, `byteLength`, `contentHash`, and `text`. Lengths count rendered UTF-8 bytes and SHA-256 hashes cover those same bytes, after version replacement in `SKILL.md`; `install` writes identical text. These queries read embedded documents without inspecting `HOME`, the working directory, or installed copies. Names, including licenses, must match the list exactly, with case-sensitive `/` components. Empty names, leading `/`, empty components, `.` or `..`, backslashes, and colons are invalid; names are never opened as OS paths.

Text is the default format: `list` prints one name per line and `show` only the document, without adding a newline. JSON queries write one object followed by LF. `--format=text|json` requires `=`, may occur once, and may precede or follow the show name. `list` takes no name; `show` requires one. These queries reject installation options, and install/uninstall reject `--format`. Success exits with 0; invalid syntax or an unknown document exits with 2, empty standard output, and a standard-error explanation even for JSON queries. Internal errors exit with 3 and output failures with 1; do not interpret missing output as an empty document. Every name returned by list or show satisfies these same rules; invalid or duplicate embedded names cause an internal error instead of a partial list.


For documentation examples, use `--doc` consistently on both inspection and execution:
`benitoite test --doc --check --report=json tests/`, then
`benitoite test --doc --allow-effects=<accepted-set> tests/`. Require version 2 of
testCheckReport and testResults in the version query. Omitting --doc keeps version 1;
do not ignore unknown JSON Lines kinds as though they were version 1 successes.

Version 2 keeps Target.tests and adds docExamples and ignoredDocBlocks. Examples
declare their own imports and uses, check public API boundaries, and have modes
test/check/error/run. The aggregate includes only ordinary tests and executable
test/run examples; check/error examples add no execution permissions. Expected
static rejection is a successful error example with observedDiagnostics, not a
top-level diagnostic failure. Unknown effects/tests are null, successful empty
ones are empty arrays. An unexpected rejection or diagnostic mismatch makes the
inspection incomplete. --check never executes a main or verifies output.

All executable examples are prepared and checked against the allowed set before
any execution. Execution emits docExample lines, a docSummary with discovered
mode counts and ignoredBlocks, and the final testSummary. The final summary counts
ordinary test functions and completed doc examples, one per example even when it
contains multiple @test functions. No imported or standard-library docs are
recursively collected. Each example runs with independent state and empty stdin.

## Local interval analysis

The report and each target carry an additional `intervalAnalysis` object with `basis: "localIntervals"`. It describes the range covered by local interval analysis. `complete` still describes type/effect checking and contract extraction. The harness must keep the report acceptance rules above: `coverage` does not participate in acceptance. A report can have `complete: true` with `coverage: "some"`, or `complete: false` with `coverage: "all"`. No warnings does not mean safe, even with `all`; runtime checks remain necessary.

Do not require `coverage: "all"` to accept an otherwise successful inspection, or use `all` to accept a failed one. Explain an unknown value or an unsupported or unfinished range as a limit of this analysis. These limits add no check error and do not by themselves call for code changes or expanded effect declarations. Read diagnostics separately: W0401/W0402/W0404 with a local-analysis note describe conditional failure at the reported position, not failure of the whole program. See [Explaining local interval warnings](idioms.md#explaining-local-interval-warnings) for examples, Range success values, and HTTP fields that are not tracked.

| Field in `intervalAnalysis` | Meaning |
|---|---|
| `coverage` | `all` when analysis has no unsupported or unfinished range or invalid body; `some` when analysis started but such a range remains, including a mix of valid and invalid bodies; `none` before analysis, including failures before type checking and targets with one or more bodies all invalidated by type checking. A checked target with no bodies at all is a zero-count `all`. |
| `inputDefinitions` | Type-checked original definitions supplied to analysis; separately analyzed lambda/lazy/clause bodies do not add to this count. |
| `unsupportedBodies` | Body outputs whose parent or saved independent input contains unsupported syntax, including stopped outputs; at most one per body output. |
| `bodyBudgetExceeded` | Body outputs stopped by an ordinary body-budget overrun; excludes stops classified as unidentified. |
| `wholeBudgetSkipped` | Ranges left unanalyzed due to the whole-check budget: stopped outputs, discovered pending bodies, remaining valid original definitions, and incomplete discovery or control for an original definition. Undiscovered nested bodies are not estimated. |
| `unidentifiedBodies` | Body outputs stopped by a budget overrun during numbering, whether the body or whole-check limit caused it. Other stops that leave numbers unavailable do not enter this count. |
| `phaseStops` | Stop events in entry preparation, discovery, queues, nested-body control, and whole-check control. `exceeded` and `unidentified` each contain `bodyOperations`, `bodyItems`, `wholeOperations`, and `wholeItems`. `depth` and `other` count the remaining stop kinds. These are events, not a count of unanalyzed bodies. |
| `usage` | `operations`, `savedItems`, `peakItems`, `maxBodyOperations`, and `maxBodyItems`: consumed operations, retained items, peak simultaneous items, maximum body operations, and maximum simultaneous body items. These are analysis-budget units, not allocated bytes. |

Counts and usage are nonnegative JSON integers. Entry/control stops without a body output appear in `phaseStops`, not in `bodyBudgetExceeded` or `unidentifiedBodies`. `wholeBudgetSkipped` does not count the same original definition's incomplete range again for each control stop, or add an unidentified output to its ordinary-overrun count. The four body/range counts need not partition `inputDefinitions`: nested bodies are separate outputs, and a stopped output may also contain unsupported syntax.

`some` includes body and phase stops, remaining original inputs, unprepared entries, unfinished discovery or queues, and discovered pending bodies. Zero counts alone do not establish `all`. For test reports, counts are summed across targets, including duplicate entries as separate checks. Coverage is `none` when every target is `none`, `some` when otherwise any target is not `all`, and `all` otherwise. Successful empty discovery is `all` with zero counts; usage errors and discovery failures are `none`.

For main, the report summary equals the target summary. For doc tests, each target combines its ordinary entry with all generated examples, including check/error examples; unfinished example discovery leaves an uninspected range. This does not add check/error examples to execution effects. Aggregate usage sums `operations` and `savedItems`, and takes the maximum of `peakItems`, `maxBodyOperations`, and `maxBodyItems`. Body maxima cover only bodies with saved outputs; an original definition includes entry, parent analysis, discovery, and queue work. Consumption from phase stops without an output still remains in overall usage.

`--deny-warnings` preserves the analysis summary even when promoted warnings make inspection fail. Adding unrelated definitions can exhaust the whole-check budget and remove later interval warnings, changing acceptance under `--deny-warnings`. Existing constant warnings and interval warnings already obtained remain. The harness must not describe an unsupported or unanalyzed range as safe. The added summary preserves main/ordinary-test schema version 1 and doc-test version 2, existing required fields, statuses, and effect-report acceptance rules.

For the processor's definition of these additional fields and their aggregation, use the [v0.0.12 English reference](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.12/docs/reference/benitoite.md#interval-analysis-coverage). Its [local warning rules](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.12/docs/reference/benitoite.md#local-interval-warnings) define the analysis scope; the executable's embedded Skill supplies the documentation for the installed version.
