# sqformat

A code formatter and linter for [Squirrel](http://squirrel-lang.org/), with first-class support for [Respawn's dialect](https://noskill.gitbook.io/titanfall2/documentation/file-format/nut-and-gnut-squirrel) used in Titanfall 2 and Apex Legends.

## Features

- Formats all Squirrel language constructs: functions, classes, enums, tables, arrays, control flow, and more
- Supports Respawn-specific syntax, including `thread`, `delaythread`, `waitthread`, `global`, `struct`, `typedef`, `untyped`, and `globalize_all_functions`
- Preserves single-line, multi-line, doc, and script-style comments with automatic word-wrapping
- Breaks long constructs cleanly while keeping short constructs on one line
- Supports configurable indentation, column limits, expression spacing, and array formatting
- Provides project-wide linting for unsafe Squirrel patterns
- Includes a language server with diagnostics, formatting, completion, signature help, symbols, navigation, hover, references, rename, semantic highlighting, and nominal member resolution

## Documentation

- [Formatting style and configuration](docs/formatting.md)
- [Linter reference](docs/linter.md)
- [Language server](docs/language-server.md)
- [Contributor guide](CONTRIBUTING.md)
- [Documentation index](docs/README.md)

## Installation

The [VS Code extension](https://marketplace.visualstudio.com/items?itemName=Bobbyperson.sqformat-vscode) installs editor integration for the formatter and language server.

Pre-built formatter and language-server binaries are available from the [latest release](../../releases/latest) for Linux (`x86_64` and `aarch64`), macOS (`x86_64` and Apple Silicon), and Windows (`x86_64`). Release assets are named `sqformat-<platform>-<architecture>` and `sqformat-lsp-<platform>-<architecture>` (with `.exe` on Windows).

To install both binaries from source, first install [Rust](https://rustup.rs/), then run:

```sh
cargo install --git https://github.com/Bobbyperson/sqformat sqformat
cargo install --git https://github.com/Bobbyperson/sqformat sqformat-lsp
```

## Usage

```sh
# Format from stdin
echo 'void function Foo(){print("hi")}' | sqformat

# Format a file (prints to stdout)
sqformat path/to/file.gnut

# Format multiple files in place
sqformat -i src/*.gnut src/*.nut

# Recursively format a directory in place
sqformat -ri src/

# Check formatting, or show the changes as a unified diff
sqformat --check -r src/
sqformat --diff path/to/file.nut

# Lint all Squirrel files and Northstar manifests under the current directory
sqformat --lint

# Lint selected files or directories
sqformat --lint scripts/ mods/example/file.nut

# Also run advisory lifetime and scheduling checks
sqformat --lint --advisory-lints

# Show every option
sqformat --help
```

Lint rules cover threaded loops without suspension, zero-duration waits, entity validity across decoded handles, unchecked array indexes, `find()` in boolean contexts, unregistered signals, remote-function argument contracts, and unresolved Northstar manifest callbacks. `--advisory-lints` additionally checks entity use after a suspension point and threads spawned repeatedly from polling loops; these are opt-in because programmer-known lifetime or scheduling guarantees can make them intentional.

Lint diagnostics are written to stderr. The command exits with status 1 for lint findings, unreadable inputs, or parse failures.

## Configuration

The formatter searches the current directory and its parents for `.sqformat.toml`. Use `--config <path>` to select a file explicitly. Command-line formatting options override file settings.

```toml
column_limit = 160
indent_style = "tab" # "tab" or "space"
indent_width = 4

spaces_in_expr_brackets = true
array_spaces = true
array_multiline_commas = true
array_multiline_trailing_commas = false
array_singleline_trailing_commas = false
```

See [Formatting style and configuration](docs/formatting.md) for the resulting layout rules and `sqformat --help` for all command-line overrides.

## Language Server

`sqformat-lsp` communicates over standard input/output using the Language Server Protocol. It uses the same formatter and configuration discovery as the CLI and recursively indexes `.nut`, `.gnut`, and `mod.json` files in each opened workspace.

The server provides live syntax and lint diagnostics, formatting, symbols, completion, signature help, go-to-definition, hover, references, rename, semantic highlighting, VM-aware cross-file analysis, and type-aware member support. Formatting remains strict and requires syntactically valid input, while semantic analysis recovers independently valid regions around syntax errors.

See the [language-server reference](docs/language-server.md) for installation, client initialization, the complete capability list, workspace behavior, and current limitations.

## GitHub Actions

Add this workflow to your project to enforce formatting on every push and pull request:

```yaml
name: Check Formatting

on: [push, pull_request]

jobs:
  format:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Download sqformat
        run: |
          curl -sSL https://github.com/Bobbyperson/sqformat/releases/latest/download/sqformat-linux-x86_64 -o sqformat
          chmod +x sqformat
      - name: Check formatting
        run: ./sqformat --check -r .
```

This downloads the latest release and checks all `.nut` and `.gnut` files, failing if any would be reformatted.

## Example

Input:

```squirrel
void function example(entity player) {
if (IsValid(player)) {
if (IsAlive(player)) {
if (player.isMechanical()) {
player.SetMaxHealth(100)
}
}
}
}
```

Output:

```squirrel
void function example( entity player )
{
	if ( IsValid( player ) )
	{
		if ( IsAlive( player ) )
		{
			if ( player.isMechanical() )
			{
				player.SetMaxHealth( 100 )
			}
		}
	}
}
```

## Library Usage

The formatting engine is available as the `sqfmt-lib` crate:

```rust
use sqfmt_lib::config::Format;
use sqfmt_lib::{format_source, format_source_default};

// Format with defaults (160-column limit and tab indentation).
let output = format_source_default(source)?;

// Format with custom settings.
let format = Format {
    column_limit: 80,
    indent: "    ".to_string(),
    indent_columns: 4,
    ..Format::default()
};
let output = format_source(source, format)?;
```

## How It Works

sqformat parses Squirrel source into an AST using [sqparse](https://github.com/Bobbyperson/sqparse), then reconstructs the output using a combinator-based formatter. For each construct, it first tries to fit everything on one line. If that exceeds the column limit, it falls back to a multi-line layout with the configured indentation.
