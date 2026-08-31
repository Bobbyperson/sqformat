# Language Server

`sqformat-lsp` is a Language Server Protocol (LSP) server for Respawn Squirrel
(`.nut` and `.gnut`). It provides diagnostics, formatting, semantic analysis,
navigation, and editor assistance. The server communicates over stdio, it does
not listen on a TCP port.

## Installation and launch

Install both binaries from the repository:

```sh
cargo install --git https://github.com/Bobbyperson/sqformat sqformat
cargo install --git https://github.com/Bobbyperson/sqformat sqformat-lsp
```

Or build locally:

```sh
cargo build -p sqformat-lsp
```

Launch the server with no arguments and connect its stdin and stdout to an LSP
client:

```sh
sqformat-lsp
```

The binary uses LSP `Content-Length` framed JSON-RPC on stdin/stdout. Do not
send shell prompts or logging to stdout. The server's protocol implementation
uses the `tower-lsp-server` 0.23 release line; its parser and formatter
dependencies are declared in `sqfmt-lsp/Cargo.toml` and resolved by `Cargo.lock`.

## Client setup

An editor/client should:

1. Start `sqformat-lsp` as a stdio process.
2. Send `initialize` with a workspace folder (or the legacy `rootUri`).
3. Send `initialized`, then normal document lifecycle notifications.
4. Synchronize Squirrel documents with full text, not incremental ranges.

Open the project directory rather than only one source file. The server scans
workspace folders recursively for `.nut`, `.gnut`, and `mod.json`; unopened
Squirrel files are indexed too.

### Initialization options

`initializationOptions` is an object with these camelCase fields:

```json
{
  "provideFormatting": true,
  "advisoryLints": false,
  "configFile": "/absolute/path/to/.sqformat.toml",
  "apiSourceRoots": ["/absolute/path/to/native/scripts"]
}
```

All fields are optional. Defaults are `provideFormatting: true`,
`advisoryLints: false`, no named config file, and no API source roots. A
malformed or incompatible options object falls back to these defaults
(`main.rs`, `InitializationOptions`).
`configFile` overrides config discovery for formatting; it is not a workspace
root and does not affect indexing.

`apiSourceRoots` adds absolute directories containing external or native
Squirrel declarations. Their `.nut` and `.gnut` files participate in
completion, signatures, navigation, hover, semantic classification, and type
analysis, but the server does not publish diagnostics for them. Their `mod.json`
files contribute VM targets and load order, so a NorthstarMods checkout retains
its normal `RunOn` behavior. Declarations still follow normal Squirrel
visibility rules, so APIs intended for other files need `global` declarations.
Rename never edits API-root files. API roots are scanned at startup; reload the
server after their files change.

The [VS Code extension](https://marketplace.visualstudio.com/items?itemName=Bobbyperson.sqformat-vscode)
is distributed separately from this repository. It starts the configured
`sqformat-lsp` command for language `squirrel`, sends these options, and maps
`.nut` and `.gnut` to that language. Its settings are:

| Setting | Default | Meaning |
|---|---|---|
| `sqformat.languageServer.enabled` | `true` | Start the language server; otherwise use the process formatter fallback. |
| `sqformat.languageServer.executablePath` | `sqformat-lsp` | Server command or path. |
| `sqformat.languageServer.apiSourceRoots` | `[]` | Absolute external/native Squirrel source directories to index. Reload after changing. |
| `sqformat.configFile` | `""` | Optional config path passed as `configFile`. |
| `sqformat.advisoryLints` | `false` | Include advisory lint rules. Reload the window after changing it. |
| `sqformat.executablePath` | `sqformat` | Fallback formatter command when the server is disabled or fails to start. |

The VS Code client deliberately leaves formatting to the server while it is
running. It registers the CLI formatter only if server startup fails.

## Advertised capabilities and protocol details

The `initialize` response advertises:

- UTF-16 positions and full text document synchronization.
- Document formatting, when `provideFormatting` is enabled.
- Completion, triggered automatically by `.` for members.
- Signature help, triggered by `(` and `,`, retriggered by `)`.
- Document symbols and workspace symbols.
- Definition and references.
- Hover.
- Rename with prepare-rename.
- Full semantic tokens (no range request and no result IDs/deltas).
- Workspace folders and workspace-folder change notifications.

The server advertises workspace file watching only dynamically, and only when
the client says `workspace.didChangeWatchedFiles.dynamicRegistration` is
supported. It then watches `**/*.{nut,gnut}` and `**/mod.json`. Clients that do
not advertise dynamic registration still work, but external file changes need
a restart (the server logs this fact). There is no file-operation capability.

The server accepts `shutdown` and then exits on `exit`. It does not require a
response to `client/registerCapability` from clients that did not advertise
dynamic registration.

## Workspace indexing and synchronization

After `initialized`, each workspace folder and configured API source root is
recursively scanned. Symlinked files and directories are skipped. Only files
with the exact `nut` or `gnut` extensions are indexed. Each indexed file retains
source text, document symbols, semantic analysis, and lint facts. Dependency
manifests contribute VM and load-order metadata, but diagnostics are published
only for workspace files and manifests.

- `textDocument/didOpen` and `didChange` replace the indexed disk entry with
  the complete open buffer and reanalyze it.
- `didChange` uses the last content change in the notification; clients should
  send one complete document.
- Open buffers take precedence over disk files for workspace queries.
- `didClose` removes the open override and reloads the file from disk. It clears
  open-buffer diagnostics, then republishes any workspace lint diagnostics for
  the disk version.
- Watched create/change/delete events update unopened Squirrel files.
- A changed `mod.json` rebuilds manifest mappings and VM targets.
- Workspace-folder additions and removals are supported and trigger a rescan.

The index is not a build system and does not execute Squirrel. Files that are
unreadable or fail to produce an index entry are silently unavailable to
workspace queries.

## Formatting and configuration

Formatting is a single full-document `textDocument/formatting` edit. It uses
the same `sqfmt-lib` formatter as the CLI. If the text is already formatted,
the response is an empty edit list. If formatting is disabled, the document is
not open, or strict formatting fails, the server returns no edits. Formatting
remains strict: recovery used for diagnostics and language features does not
make malformed source format-able.

With no `configFile` initialization option, the server searches upward from the
document's directory for the nearest `.sqformat.toml`. With a named file, it
reads that file instead. A read, TOML parse, or indent-style error is logged as
an error and formatting falls back to defaults.

Supported `.sqformat.toml` keys are:

```toml
column_limit = 160
indent_style = "tab"       # or "space"
indent_width = 4
spaces_in_expr_brackets = true
array_spaces = true
array_multiline_commas = true
array_multiline_trailing_commas = false
array_singleline_trailing_commas = false
```

Unset keys retain formatter defaults. Unknown keys are rejected by the config
parser. The nearest-file behavior is implemented in
`sqfmt-lib/src/config.rs` and is intentionally the same discovery used by the
CLI once a starting directory is selected.

## Diagnostics

Diagnostics are pushed after initialization, open, change, watched-file, and
workspace-folder events. Their source is `sqformat`. Each family is bounded to
avoid flooding a client; syntax, per-document semantic, member, arity, type,
and lint findings each have an implementation limit of 100 where applicable.

### Syntax diagnostics

Lexer and parser errors are errors. Partial tokenization and parsing can retain
independently valid top-level regions, so later declarations and features may
remain available after an invalid character, unterminated string, delimiter
error, or parser error. The malformed statement itself is omitted rather than
represented by a synthetic AST node. Formatting still requires valid input.

### Semantic diagnostics

Semantic checks are warnings because they are conservative analysis, not proof
of what the game runtime accepts:

- A declaration reuses a name already bound in the same scope. A local that
  shadows a function parameter is called out, and the first declaration is
  attached as related information.
- A member is absent from a fully known workspace `struct` or `class` owner.
- No known signature accepts a call's argument count, respecting defaults and
  variadic parameters.
- A call passes a fully known nominal value to a fully known incompatible
  nominal parameter. Typedef aliases and subclass-to-base compatibility are
  considered; unresolved, native, primitive, structural, and variadic values
  remain unchecked.
- An initializer or return value contradicts a fully known nominal declared
  type. Typedef aliases, inheritance, and subclass-to-base compatibility are
  considered.

These checks intentionally stay silent for native/game-provided types, most
tables, inline structural values, unresolved values, and incomplete type
chains. Member and arity checks do not use VM targets; this conservative choice
avoids claiming that a member is absent merely because another branch defines
it.

### Linter diagnostics

The server runs the workspace-aware `sqfmt-lint` analysis over indexed files and
publishes warnings for open and unopened files. Lint codes include:

- `threaded-loop-without-wait`
- `invalid-entity-use`
- `wait-zero`
- `unregistered-signal`
- `unchecked-encoded-ehandle`
- `unsafe-array-index`
- `unresolved-manifest-callback`
- `remote-function-contract-mismatch`
- `find-used-as-boolean`

With `advisoryLints: true`, it also publishes:
`entity-use-after-yield` and `thread-spawned-inside-polling-loop`. Lint codes
are returned as diagnostic string codes. `mod.json` files are checked for
unresolved `Before`/`After` manifest callbacks. A linter diagnostic is a
warning, and diagnostic refresh removes stale findings.

## Completion and signature help

Completion without a member receiver offers visible declarations in the
current lexical scope, filtered by declaration order and shadowing, followed by
deduplicated exported workspace globals. Results include declaration kind,
canonical Squirrel detail, and Markdown documentation.

Completion after `.` (including an empty or partial member name) resolves the
receiver and offers members from the resolved owner and base classes. Results
are filtered to members available at the cursor and to VM targets compatible
with the current position. Unresolved or dynamic receivers return no member
list rather than guessing.

Signature help resolves named functions, methods, table methods, constructors,
function/lambda values, `functionref` values and returns, and inferred return
types. It tracks the active parameter from commas and probes unfinished calls
by adding temporary closing parentheses; the probe is not stored or published.
Class constructors use explicit or inherited signatures, or a synthesized
zero-argument `ClassName()` when a fully known class has no constructor.

## Navigation, symbols, and rename

Document symbols are hierarchical and include functions, constructors,
classes, methods, structs, fields, enums, globals, constants, typedefs, and
variables, including nested declarations. Workspace symbols flatten that tree,
use `::` container names, perform fuzzy matching, and return at most 100
results.

Definition, hover, references, and rename share the semantic resolver:

- locals and parameters honor lexical shadowing;
- exported globals resolve across indexed files;
- nominal members resolve through inheritance and declared return chains;
- document-local inline structs and static table-literal slots retain their
  structural identity;
- typedef aliases, class construction, `expect Type(...)`, typed fields,
  callable values, and conservative local value flow are supported.

Hover returns Markdown Squirrel code blocks with canonical declaration details.
References honor `includeDeclaration`. Rename provides a placeholder, checks
that the new name is a valid non-keyword Squirrel identifier, and returns edits
ordered safely for each file. Ambiguous members and unresolved globals cannot be
renamed. Nonliteral computed properties are not navigable.

## VM and `mod.json` awareness

The server understands `#if`, `#ifdef`, `#elseif`/`#elif`, `#else`, and
`#endif` lines. It models the VM set `SERVER`, `CLIENT`, and `UI` using
three-valued condition evaluation. Unknown names such as `MP` and `DEV` do
not narrow the VM set; unknown conditions therefore remain open to every VM.

For every discovered `mod.json`, `Scripts[].Path` is resolved relative to
`mod/scripts/vscripts` beside that manifest. `RunOn` uses the same condition
model, and `LoadPriority` plus script list position records load order. A listed
script's effective VM set is the intersection of its manifest target and its
local `#if` region. A file no manifest lists remains available to every VM.

VM filtering affects completion, cross-file definition, and semantic-token
classification. It does not filter hover, references, rename, workspace
symbols, member diagnostics, or arity diagnostics. This distinction is
intentional: broad navigation and edits should not silently omit a branch.

## Semantic tokens

The server advertises full-document semantic tokens with this legend:

Types: `keyword`, `string`, `number`, `operator`, `function`, `method`,
`variable`, `parameter`, `property`, `class`, `struct`, `enum`, `type`,
`comment`.

Modifiers: `declaration`, `readonly`.

Keywords, literals, and operators come from lexical tokens. Identifiers are
classified from local declarations/references or workspace globals; builtin
types such as `int`, `entity`, and `functionref` are recognized without a
workspace declaration. Constants receive `readonly`; declarations receive
`declaration`.

Comments and punctuation are not emitted, leaving ordinary client grammar
coloring and bracket coloring intact. A region proven unreachable for the
file's manifest VM is emitted as `comment` tokens so themes dim it. The server
does not provide range tokens or token result IDs, and open-document lexical
tokens are reused rather than retokenized per request.

## Logging and troubleshooting

Protocol traffic belongs on stdio; inspect client logs/output channels for
server messages. The server logs:

- configuration read/parse failures as errors;
- failed dynamic watcher registration as warnings;
- lack of dynamic file-watch support as informational;
- successful initialization and indexed-file count as informational;
- formatting failures as errors.

If there are no diagnostics or cross-file results, check that the editor opened
the project folder, not just a file, and that the file extension is `.nut` or
`.gnut`. If external edits are stale, check dynamic watcher support or restart
the server. If formatting uses defaults, verify the nearest `.sqformat.toml`,
the named `configFile`, and the client's process environment. If the server
does not start, run `sqformat-lsp` directly and inspect stderr/client logs; in
VS Code verify both executable-path settings and use the `sqformat` output
channel. A server startup failure does not disable the VS Code CLI formatting
fallback.

## Current limitations

These are implementation boundaries, not promised features:

- Synchronization is full-text only; incremental sync is not advertised.
- There are no code actions, code lenses, document links, folding ranges,
  selection ranges, inlay hints, or execute-command features.
- Semantic tokens are full-document only; comments and punctuation depend on
  the client grammar.
- Recovery is conservative. A malformed delimiter context can suppress
  declarations inside it, and heavily malformed source can have fewer features
  until repaired.
- Native/game-provided APIs and types are not indexed, so their members,
  signatures, argument types, and nominal compatibility are generally
  unknown.
- Inference does not model nonliteral computed keys, global/captured mutation,
  many per-instance insertions, or arbitrary runtime dispatch. Recursive and
  conflicting type/value chains resolve conservatively to nothing.
- Primitive and literal type compatibility is not checked. Argument type checks
  cover only fully known nominal types.
- Duplicate member declarations can make rename ambiguous. VM targets are not
  applied to member, arity, or type-mismatch diagnostics.
- A manifest parser accepts only the fields and path layout implemented in
  `sqfmt-lint/src/manifest.rs`; invalid or unreadable manifests contribute no
  mapping, while the linter may still report diagnostics for valid manifest
  JSON.

## Contributor and manual validation

The Rust unit tests cover parser recovery, semantic analysis, workspace
resolution, diagnostics, tokens, and protocol helpers. Run the workspace tests
and checks from the repository root:

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

The stdio integration checks are not part of `cargo test`. They require Python
3 and no third-party packages:

```sh
cargo build -p sqformat-lsp
cd sqfmt-lsp/tests/manual
python3 check_features.py ../../../target/debug/sqformat-lsp
python3 check_corpus.py ../../../target/debug/sqformat-lsp [/path/to/NorthstarMods]
```

`check_features.py` validates capability/lifecycle behavior, recovery,
completion, callable and constructor signatures, VM and manifest narrowing,
semantic tokens, all diagnostic families, formatting discovery, workspace lint
refresh, and clean exit without dynamic registration. `check_corpus.py`
exercises cross-file resolution, manifest narrowing, token ordering, member
diagnostics, warning safety, and dimmed-region reporting against a NorthstarMods
checkout. The expected corpus counts are snapshots; inspect a mismatch before
treating it as a regression (`sqfmt-lsp/tests/manual/README.md`).

Primary implementation references:

- Protocol lifecycle and capabilities: `sqfmt-lsp/src/main.rs`.
- Indexing and cross-file resolution: `sqfmt-lsp/src/workspace.rs`.
- Semantic model and scope/value inference: `sqfmt-lint/src/semantic.rs`.
- Diagnostics construction: `sqfmt-lsp/src/diagnostics.rs`.
- Linter rules and options: `sqfmt-lint/src/lib.rs` and `sqfmt-lint/src/rules.rs`.
- Workspace semantic diagnostics: `sqfmt-lint/src/semantic_rules.rs`.
- VM conditions and manifests: `sqfmt-lint/src/conditional.rs` and
  `sqfmt-lint/src/manifest.rs`.
- Semantic-token legend and encoding: `sqfmt-lsp/src/tokens.rs`.
