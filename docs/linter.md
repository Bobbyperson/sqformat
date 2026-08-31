# Linter

`sqformat` includes a linter for Respawn-flavor Squirrel (`.nut` and `.gnut`)
source and Northstar `mod.json` manifests. The linter is project-aware: it
indexes facts from all successfully parsed Squirrel files before reporting
some diagnostics.

## Running the linter

```sh
# Recursively scan the current directory
sqformat --lint

# Scan selected files and directories
sqformat -l scripts/example.gnut mods/example

# Include advisory lifetime and scheduling checks
sqformat --lint --advisory-lints

# Omit progress and the final summary
sqformat --lint --quiet

# Emit annotations understood by GitHub Actions
sqformat --lint --github-actions --quiet
```

`--advisory-lints` and `--github-actions` require `--lint`. Lint mode conflicts with formatting
options `-i/--inplace`, `-c/--check`, and `-d/--diff`; `-r/--recursive` is not
needed because linting always recurses. Lint mode does not read stdin: with no
file arguments it scans `.`.

The effective lint-mode form is `sqformat --lint [--advisory-lints] [--quiet]
[--github-actions] [FILES...]` (the short lint form is `-l`). Formatter settings such
as `--config`, `--column-limit`, indent/array options, `--stdin-filename`, and
`--verbose` are parsed by the shared CLI but have no effect in lint mode.

### Discovery and manifests

When a path is a directory, the linter recursively visits it. Directory scans
include files whose extension is exactly `nut` or `gnut`, plus files named
exactly `mod.json`. A named file is accepted regardless of its extension,
except that a named `mod.json` is treated as a manifest. Discovered paths are
sorted and deduplicated.

Squirrel files are parsed independently first. Their analyses are then joined
into one workspace for project-wide checks. Every selected `mod.json` is
checked against that workspace. Manifest callback checking only recognizes
valid JSON; malformed JSON produces no callback diagnostics in the linter.
The linter does not discover or apply `.sqformat.toml`; formatter configuration
has no effect on lint results.

### Output and status

Diagnostics are written to stderr in this form:

```text
path/to/file.gnut:LINE:COLUMN: message [rule-id]
```

With `--github-actions`, findings are instead written to stdout as native
workflow-command annotations:

```text
::warning file=path/to/file.gnut,line=LINE,col=COLUMN,title=rule-id::message [rule-id]
```

Read and parse failures use `::error` annotations. Annotation values are
escaped according to the GitHub Actions workflow-command format. Use `--quiet`
when a CI step should contain annotations without the normal scan summary.

Unless `--quiet` is used, linting also writes a scan summary to stderr. Exit
status is `0` only when all selected files were read and parsed and no
diagnostic was found. It is `1` if any rule reports a problem, or if a file
cannot be read or parsed. Diagnostics use byte offsets internally; the CLI
converts their start offset to a one-based line and column.

The default run reports all rules below except the two marked **advisory**.
Advisory rules are not weaker matches; they are omitted by default because
correctness can depend on lifetime or scheduling guarantees known only to the
programmer.

### Suppressing a diagnostic

Add `// nolint: rule-id` to suppress a named rule on that line. Multiple rule
IDs can be separated by commas or whitespace.

```squirrel
wait 0 // nolint: wait-zero
```

When the directive is on a line by itself, it applies to the next nonblank,
non-comment line. This allows an explanation to sit between the directive and
the affected code.

```squirrel
// nolint: entity-use-after-yield
// The caller guarantees this entity remains valid.
ent.Show()
```

Unknown rule IDs have no effect. A directive only suppresses diagnostics whose
reported source range begins on its target line.

## Rules

### Workspace semantic rules

The following **default; project-wide** rules use the semantic model built from
all successfully parsed Squirrel files. They are conservative: they report a
finding only when the relevant nominal type, member set, or signature is fully
known. Native/game-provided types, primitive and literal compatibility,
structural or inline table values, unresolved values, and incomplete or
computed type/value chains remain unchecked. Variadic parameters are not used
for argument-type checks.

VM conditions are modeled for `SERVER`, `CLIENT`, and `UI`. Mutually exclusive
conditional branches do not produce duplicate-declaration findings. Manifest
`RunOn` targets and local `#if` regions do not filter invalid-member,
call-arity, argument-type, initializer-type, or return-type findings; unknown
conditions remain available to every VM.

### `duplicate-declaration`

Reports a name reused in the same scope. A local shadowing a function
parameter is reported specially. Declarations in mutually exclusive VM or
conditional branches are not reported as duplicates.

```squirrel
void function Show( int value ) {
    local value = 1
}
```

### `invalid-member`

Reports a member absent from a fully known workspace `struct` or `class`,
including its base classes. Unknown, native, structural, and dynamic owners
are not checked.

```squirrel
class Panel {
    function Show() {}
}

void function Use( Panel panel ) {
    panel.Hide()
}
```

### `call-arity`

Reports a call when no known signature accepts its argument count. Defaults
and variadic parameters are considered; a call is not reported when any known
overload accepts it.

```squirrel
void function Show( int value ) {}
Show( 1, 2 )
```

### `argument-type`

Reports an argument passed to a fully known nominal parameter when every
viable signature rejects its nominal type. Typedef aliases and subclass-to-base
compatibility are considered. Primitive, literal, structural, native, and
unresolved values remain unchecked.

```squirrel
class Panel {}
class Button {}
void function Show( Panel panel ) {}
Show( Button() )
```

### `initializer-type`

Reports an initializer that contradicts a fully known nominal declared type.
Typedef aliases, inheritance, and subclass-to-base compatibility are
considered.

```squirrel
class Panel {}
class Button {}
Panel panel = Button()
```

### `return-type`

Reports a returned value that contradicts a fully known nominal declared
return type. Functions without a declared return type are not checked by this
rule.

```squirrel
class Panel {}
class Button {}
Panel function MakePanel() {
    return Button()
}
```

### `threaded-loop-without-wait`

**Default.** Reports an infinite or otherwise statically always-true `while`,
`do while`, or `for` loop in a function that is started with `thread`, when the
loop body has no reachable wait and no reachable `break` or `return` that exits
that loop. A loop with an unknown run-time condition is not treated as
infinite. The check follows constant `true`/`false`, integer, float, and `!`
expressions and does not look through nested function definitions.

```squirrel
thread Poll()
void function Poll() {
    while ( true ) {
        DoWork()
    }
}
```

Safe patterns include a reachable suspension or an exit:

```squirrel
while ( true ) {
    DoWork()
    WaitFrame()
}
```

Threaded-function discovery is project-wide: a `thread Name()` declaration in
one file also marks `void function Name()` in another selected file.

### `wait-zero`

**Default.** Reports the statement `wait 0` (including numeric zero written as
a zero float). It marks a yield-looking statement that does not advance a game
frame; use `WaitFrame()` when a frame boundary is intended.

```squirrel
wait 0
// safer when a frame must elapse:
WaitFrame()
```

This rule is about the literal `wait` statement, not arbitrary calls with a
zero argument.

### `invalid-entity-use`

**Default.** Reports dereferencing an entity the flow analysis knows is
`null`/invalid, including a direct use after `Destroy()`, or a use in a branch
where `IsValid(entity)` is known to be false. Entity method/property receivers
and array-index bases are checked.

```squirrel
void function Show() {
    entity ornull ent = null
    ent.Show()
}
```

Guard before use:

```squirrel
if ( IsValid(ent) )
    ent.Show()
```

The analysis is conservative and local to a function's visible control flow.
It tracks declared entity variables, selected entity-producing calls, simple
assignments, branches, loops, and short-circuit conditions. It does not prove
arbitrary engine calls, aliases, mutations through tables, or interprocedural
effects. An entity merely marked possibly invalid is intentionally not
reported.

### `unchecked-encoded-ehandle`

**Default.** Reports dereferencing the result of
`GetEntityFromEncodedEHandle()` or
`GetHeavyWeightEntityFromEncodedEHandle()` without a recognized `IsValid`
guard.

```squirrel
entity ent = GetEntityFromEncodedEHandle(handle)
```

```squirrel
entity ent = GetEntityFromEncodedEHandle(handle)
if ( !IsValid(ent) )
    return
ent.Show()
```

This is flow-sensitive but deliberately recognizes only the direct patterns
implemented by the analyzer; it does not establish validity for arbitrary
wrappers or aliases.

### `unsafe-array-index`

**Default.** Reports either an array literal indexed outside its statically
known bounds, or a `find()` result used as an index without a recognized
not-found check. `find()` results are treated as unchecked until compared with
`-1`/`null`, or with an equivalent supported zero-bound comparison.

```squirrel
string function Lookup(array<string> values, string wanted) {
    int index = values.find(wanted)
    return values[index]
}
```

```squirrel
int index = values.find(wanted)
if ( index == -1 )
    return ""
return values[index]
```

The check does not evaluate general expressions or prove arbitrary bounds.
Checks must be in forms understood by the flow analysis; aliases and other
indirect uses may not be recognized.

### `find-used-as-boolean`

**Default.** Reports a direct `find()` call used as an `if`/logical boolean
condition. `find()` returns an index or a not-found value, so compare it
explicitly instead of relying on truthiness.

```squirrel
if ( values.find(wanted) )
    return true
```

```squirrel
if ( values.find(wanted) != -1 )
    return true
```

The check targets direct `find()` calls in boolean contexts, including `!`,
`&&`, and `||`; it does not infer the meaning of arbitrary wrapper functions.

### `unregistered-signal`

**Default; project-wide.** Reports a custom literal signal used by
`Signal`, `EndSignal`, or `WaitSignal` when the selected workspace has no
matching `RegisterSignal` call. The engine-provided `OnDestroy` signal is
excluded. A signal must be observed as both emitted and consumed before this
check reports it. At most one diagnostic is reported for each signal in each
file.

```squirrel
Signal(ent, "CustomDone")
WaitSignal(ent, "CustomDone")
```

```squirrel
RegisterSignal("CustomDone")
Signal(ent, "CustomDone")
WaitSignal(ent, "CustomDone")
```

Only literal signal names are indexed. Dynamic names, nonstandard registration
wrappers, and signals in files that were not selected are not resolved.

### `remote-function-contract-mismatch`

**Default; project-wide.** Reports a call through
`Remote_CallFunction_NonReplay`, `Remote_CallFunction_Replay`, or
`Remote_CallFunction_UI` when the named function is registered with
`Remote_RegisterFunction` but no selected declaration accepts the number of
arguments sent after the player/remote-function name. Required and optional
parameters and variadic parameters are considered.

```squirrel
Remote_RegisterFunction("RemoteMessage")
void function RemoteMessage(int value) {}
Remote_CallFunction_NonReplay(player, "RemoteMessage", 1, 2)
```

Send the declared arity (or make the declaration variadic) and keep the
registration and declaration in the selected workspace. Names and remote
function strings must be statically recognizable; the linter does not infer
dynamic dispatch or argument types.

### `unresolved-manifest-callback`

**Default; project-wide.** Reports a string value of a `Before` or `After`
property anywhere in a valid `mod.json` when no selected Squirrel function
declaration has that name. This validates Northstar callbacks against the
indexed workspace, including callbacks nested in the manifest structure.

```json
{ "ClientCallback": { "Before": "Missing", "After": "Present" } }
```

```squirrel
void function Present() {}
```

Only function names/signatures collected by the linter count; the callback
does not need to be callable with a particular argument list. Invalid JSON is
left to other tooling and is not reported by this rule.

### `entity-use-after-yield`

**Advisory.** Reports a known-valid entity dereferenced after a suspension
point such as `WaitFrame`, `WaitEndFrame`, `WaitSignal`, `FlagWait...`, `wait`,
`waitthread`, or `waitthreadsolo`, unless the entity has an `OnDestroy`
`EndSignal` protection or is revalidated by recognized flow checks.

```squirrel
void function Show(entity ent) {
    WaitFrame()
    ent.Show()
}
```

```squirrel
ent.EndSignal("OnDestroy")
WaitFrame()
```

This is a conservative intra-function flow check. It cannot know whether an
entity is stable in a particular game context, and it does not model arbitrary
lifetime guarantees or aliases.

### `thread-spawned-inside-polling-loop`

**Advisory.** Reports `thread` or `delaythread` spawned inside a loop that
contains a reachable wait and can therefore execute repeatedly. Such a loop
may create overlapping work on successive iterations.

```squirrel
while ( true ) {
    thread Update()
    WaitFrame()
}
```

Use an explicit completion/ownership strategy, or move the spawn outside the
polling loop when repeated overlapping work is not intended. The linter does
not prove whether overlap is safe; it reports the structural risk and may
report branches within the loop conservatively.

## Rust API

The `sqfmt-lint` crate is a workspace member (version `0.1.0`) and parses with
the Respawn flavor of `sqparse` `v0.5.0`. The supported public entry points are:

- `analyze(&str) -> Result<Analysis, String>` parses source and collects facts.
- `analyze_statements(&[&Statement]) -> Analysis` analyzes already parsed AST
  statements.
- `analyze_statements_with_tokens(&str, &[&Statement], &[&Token]) -> Analysis`
  also collects `nolint` directives from an existing tokenization.
- `diagnostics(&[Analysis])` runs default (non-advisory) diagnostics.
- `diagnostics_with_options(&[Analysis], LintOptions)` enables advisory rules
  when `LintOptions { advisory: true }` is supplied.
- `Workspace::new(...)` builds cross-file facts;
  `Workspace::diagnostics[_with_options](...)` reports Squirrel diagnostics;
  `Workspace::manifest_diagnostics(&str)` checks valid manifest callbacks.
- `Diagnostic` exposes `range`, `rule`, and `message`; rule IDs are also
  exported as string constants in `sqfmt_lint`.

`Analysis` stores implementation details and should be obtained from the
analysis functions rather than assembled from fields. The API returns
diagnostics and applies source directives for the `Analysis`/`Workspace` lint
path. `SemanticWorkspace::diagnostics` and
`SemanticWorkspace::diagnostics_with_document` return raw semantic findings;
callers must pass them to `Analysis::retain_unsuppressed` before publishing
them. The API does not provide per-rule configuration or automatic fixes.
