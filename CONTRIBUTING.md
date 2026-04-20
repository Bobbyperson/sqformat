# Contributing to sqformat

Thanks for your interest in contributing!

## Getting Started

You'll need [Rust](https://rustup.rs/) installed.

```sh
git clone https://github.com/Bobbyperson/sqformat.git
cd sqformat
cargo build
cargo test --all-features
```

## Development Workflow

Before submitting a PR, make sure all checks pass:

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

## Project Structure

This is a Cargo workspace with two crates:

- **`sqfmt`** -- CLI binary
- **`sqfmt-lib`** -- Core formatting library

Most contributions will be in `sqfmt-lib`.

### Formatter Design

The formatter uses a **combinator-based architecture** where formatting operations are composable functions of type `FnOnce(Writer) -> Option<Writer>`. Returning `None` signals that a formatting choice failed (e.g., exceeded column limit in single-line mode), triggering fallback to an alternative layout.

Key modules in `sqfmt-lib/src/`:

- **`combinators/`**: Core combinator primitives (`alt`, `pair`, `tuple`, `iter`, `cond`, `opt`) and Writer-specific combinators (`single_line`, `indented`, `empty_line`, `new_line`, `space`, `tag`, `definitely_multi_line`).
- **`writer.rs`**: The `Writer` type tracks output lines, current column position, remaining columns, indent depth, and single-line mode. Uses `im::Vector` for persistent data structures and `Arc` for cheap cloning (needed for backtracking in `alt`).
- **`token.rs`**: Formats individual tokens including their attached comments (before-lines, inline, and trailing).
- **`comment.rs`**: Comment formatting with word-wrapping, handling `//`, `#`, `/* */`, and `/** */` comments.
- **`expression.rs`**: Expression formatting with single-line/multi-line fallback via `alt`.
- **`statement.rs`**: Statement formatting for all statement types including Respawn-specific ones (`thread`, `delaythread`, `waitthread`, `global`, `struct`, `typedef`, `untyped`).
- **`operator.rs`**: Binary, prefix, and postfix operator formatting.
- **`type_format.rs`**: Type annotation formatting (plain, generic, array, functionref, struct, reference, nullable).
- **`config.rs`**: `Format` struct with formatting parameters (column limit, indent style, array spacing, etc.).
- **`shared.rs`**: Helpers for identifiers and optional separators/tokens.

### The `alt` Pattern

The core formatting strategy is `alt(single_line(...), multi_line_fallback)`. The `single_line` combinator constrains the Writer to reject newlines and overflow. If it returns `None`, `alt` tries the multi-line alternative. This is how the formatter chooses between compact and expanded layouts.

## Testing

Tests live alongside the code in `#[cfg(test)] mod test` blocks. When adding or changing formatting behavior:

1. Add a test case that covers the new behavior.
2. Run `cargo test -p sqfmt-lib` to verify.
3. If you changed a formatting rule, update `STYLE.md` to match.
4. Check idempotency on real files, formatting already-formatted output should produce identical results:
   ```sh
   cargo run -- file.nut > /tmp/pass1.nut
   cargo run -- /tmp/pass1.nut > /tmp/pass2.nut
   diff /tmp/pass1.nut /tmp/pass2.nut
   ```
   Or run the full idempotency CI job locally with [act](https://github.com/nektos/act):
   ```sh
   act --workflows ".github/workflows/rust.yml" --job "idempotency"
   ```

## Reporting Bugs

If sqformat produces incorrect output, please include:

- The input source code (or a minimal reproduction)
- The expected output
- The actual output

## Pull Requests

- Keep PRs focused on a single change.
- Include tests for new formatting behavior.
- Make sure the formatter remains idempotent (formatting twice produces the same result).

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
