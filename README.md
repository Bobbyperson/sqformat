# sqformat

A code formatter for [Squirrel](http://squirrel-lang.org/), with first-class support for [Respawn's dialect](https://noskill.gitbook.io/titanfall2/documentation/file-format/nut-and-gnut-squirrel) used in Titanfall 2 and Apex Legends.

## Features

- Formats all Squirrel language constructs: functions, classes, enums, tables, arrays, control flow, and more
- Full support for Respawn-specific syntax: `thread`, `delaythread`, `waitthread`, `global`, `struct`, `typedef`, `untyped`, `globalize_all_functions`
- Preserves all comments (single-line, multi-line, doc, and script-style) with automatic word-wrapping
- Intelligent line breaking: fits code on one line when possible, wraps cleanly when it doesn't
- Configurable indent style, column limit, and array formatting options

## Installation

**Download a pre-built binary** from the [latest release](../../releases/latest).

**Or install from source** with [Rust](https://rustup.rs/):

```sh
cargo install --git https://github.com/Bobbyperson/sqformat
```

## Usage

```sh
# Format from stdin
echo 'void function Foo(){print("hi")}' | sqformat

# Format a file (prints to stdout)
sqformat path/to/file.gnut

# Format multiple files in-place
sqformat -i src/*.gnut src/*.nut

# Recursively format a directory in-place
sqformat -i -r src/
```

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
void function example(entity player) {
	if ( IsValid(player) ) {
		if ( IsAlive(player) ) {
			if ( player.isMechanical() ) {
				player.SetMaxHealth(100)
			}
		}
	}
}
```

## Library Usage

The formatting engine is available as a library crate (`sqfmt-lib`):

```rust
use sqfmt_lib::{format_source_default, format_source};
use sqfmt_lib::config::Format;

// Format with defaults (160 column limit, tab indent)
let output = format_source_default(source)?;

// Format with custom settings
let format = Format {
    column_limit: 80,
    indent: "    ".to_string(),
    indent_columns: 4,
    ..Format::default()
};
let output = format_source(source, format)?;
```

## How It Works

sqformat parses Squirrel source into an AST using [sqparse](https://github.com/cpdt/sqparse), then reconstructs the output using a combinator-based formatter. For each construct, it first tries to fit everything on a single line. If that exceeds the column limit, it falls back to a multi-line layout with proper indentation. This backtracking approach produces compact output without sacrificing readability.
