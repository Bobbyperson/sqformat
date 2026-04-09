mod combinators;
mod comment;
pub mod config;
mod expression;
mod operator;
mod shared;
mod statement;
mod token;
mod type_format;
mod writer;

#[cfg(test)]
mod test_utils;

use config::Format;
use statement::program;
use writer::Writer;

use std::sync::Arc;

/// Format a Squirrel source string using the given format configuration.
pub fn format_source(source: &str, format: Format) -> Result<String, String> {
    // Ensure source ends with a newline so the lexer attaches trailing `// comments`
    // to the preceding token's new_line rather than emitting a synthetic Empty token.
    let owned;
    let source: &str = if source.ends_with('\n') {
        source
    } else {
        owned = format!("{source}\n");
        &owned
    };
    let tokens = sqparse::tokenize(source, sqparse::Flavor::SquirrelRespawn)
        .map_err(|e| e.display(source, Some("Lexer error")).to_string())?;

    let ast = sqparse::parse(&tokens, sqparse::Flavor::SquirrelRespawn)
        .map_err(|e| e.display(source, &tokens, Some("Parse error")).to_string())?;

    let writer = Writer::new(Arc::new(format));
    match program(&ast)(writer) {
        Some(w) => {
            let s = w.to_string();
            if s.ends_with('\n') {
                Ok(s)
            } else {
                Ok(s + "\n")
            }
        }
        None => Err("Formatting failed: could not fit output within column limit".to_string()),
    }
}

/// Format a Squirrel source string using default format configuration.
pub fn format_source_default(source: &str) -> Result<String, String> {
    format_source(source, Format::default())
}
