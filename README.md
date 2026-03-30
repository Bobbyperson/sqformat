# sqformat

A code formatter for [Squirrel](http://squirrel-lang.org/), with first-class support for [Respawn's dialect](https://noskill.gitbook.io/titanfall2/documentation/file-format/nut-and-gnut-squirrel) used in Titanfall 2 and Apex Legends.

> **Warning:** This project is pre-1.0. Formatting output may change between versions and is not guaranteed to preserve the semantics of your code. Always review formatted output and use version control.

## Features

- Formats all Squirrel language constructs: functions, classes, enums, tables, arrays, control flow, and more
- Full support for Respawn-specific syntax: `thread`, `delaythread`, `waitthread`, `global`, `struct`, `typedef`, `untyped`, `globalize_all_functions`
- Preserves all comments (single-line, multi-line, doc, and script-style) with automatic word-wrapping
- Intelligent line breaking: fits code on one line when possible, wraps cleanly when it doesn't
- Configurable indent style, column limit, and array formatting options

## Installation

Requires [Rust](https://rustup.rs/).

```sh
cargo build --release
```

The binary will be at `target/release/sqformat`.

## Usage

```sh
# Format from stdin
echo 'void function Foo(){print("hi")}' | sqformat

# Format a file (prints to stdout)
sqformat path/to/file.gnut

# Format multiple files in-place
sqformat -i src/*.gnut src/*.nut
```

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

// Format with defaults (120 column limit, tab indent)
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
