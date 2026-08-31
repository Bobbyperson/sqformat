# Manual language server checks

Raw JSON-RPC checks that drive `sqformat-lsp` over stdio the way an editor does. They cover ground
the Rust unit tests cannot: capability advertisement, request and response shapes, the lifecycle,
and behavior at real project scale.

These are not run by `cargo test`. Run them by hand after changing the server, and after any change
to protocol handling, workspace indexing, or project semantics.

```sh
cargo build -p sqformat-lsp
cd sqfmt-lsp/tests/manual
python3 check_features.py ../../../target/debug/sqformat-lsp
python3 check_corpus.py ../../../target/debug/sqformat-lsp [/path/to/NorthstarMods]
```

Python 3 only, no dependencies.

## `check_features.py`

Builds a temporary workspace per check, so it runs anywhere.

| Check | What it pins |
| --- | --- |
| `check_branch_slot_join` | a slot every branch inserts becomes available after the branch; a one-sided insertion does not |
| `check_callable_signatures` | signature help for a reassigned function value and for a declared `functionref` return |
| `check_post_initializer` | a call post-initializer's slots appear beside the called type's members |
| `check_return_inference` | an undeclared return type is inferred across files |
| `check_vm_targets` | completion inside `#if CLIENT` skips `#if SERVER` globals |
| `check_manifest_targets_and_dimming` | a manifest `RunOn` narrows a whole file, and unreachable guards come back as comment tokens |
| `check_api_source_root` | API source roots provide VM-aware completion, stay diagnostic-free, and cannot be renamed |
| `check_duplicate_declarations` | a redeclared local is published as a warning with the first declaration attached, while exclusive `#if` branches are not |
| `check_cross_file_lint_refresh` | changing an open caller updates diagnostics in another open document |
| `check_unopened_workspace_lint` | workspace diagnostics are published before the affected file is opened |
| `check_invalid_members` | a name missing from a known struct is reported, while an `entity` receiver and a table literal stay silent |
| `check_call_arity` | wrong arity for a known function is reported while overridable and unknown calls stay silent |
| `check_type_mismatch` | incompatible known declared types are reported while subclass and open types stay silent |
| `check_formatting_uses_the_discovered_config` | formatting uses the nearest `.sqformat.toml` |
| `check_completion_trigger` | completion advertises `.` as its trigger character |
| `check_exit_without_dynamic_registration` | the server still exits when the client never answers `client/registerCapability` |

That last one guards a real defect: `initialized` used to await a registration request
unconditionally, and an unanswered one left the server running forever.

## `check_corpus.py`

Needs a NorthstarMods checkout, found beside the repo or its parent, or passed as an argument.
Skips cleanly when absent. Takes about four minutes: each check indexes the full corpus at startup,
and `check_warnings_over_the_corpus` then opens every script.

| Check | What it pins |
| --- | --- |
| `check_file_local_references` | `OnPlayerKilled`, defined without `global` in eight gamemodes, resolves only within its own file |
| `check_exported_references` | an exported registrar is still found across the project |
| `check_guarded_completion` | in a shared script, each `#if` guard offers only its own VM's helpers plus shared ones |
| `check_manifest_narrowing` | an unguarded global in a `RunOn: SERVER` script is hidden from a `RunOn: UI` script |
| `check_semantic_tokens` | tokens for a real file are ordered, non-overlapping, and single-line |
| `check_invalid_members` | a typo injected into a real `ServerInfo` field access is reported, and the unedited file is clean |
| `check_call_arity` | an injected extra argument to a same-file call is reported while the original is clean |
| `check_type_mismatch` | an injected incompatible declared-struct assignment is reported while the original is clean |
| `check_warnings_over_the_corpus` | every warning is either a shared lint rule or a parameter-shadowing case, and no member warning fires on code that ships |
| `report_dimmed_regions` | informational count of listed scripts with a provably unreachable region |

Corpus structure can change independently of the server, so investigate failures against the
current checkout before treating them as regressions.
