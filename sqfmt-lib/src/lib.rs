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

use std::ops::Range;
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
    let disabled_regions = format_disabled_regions(source)
        .into_iter()
        .map(|range| &source[range])
        .collect::<Vec<_>>();
    let tokens = sqparse::tokenize(source, sqparse::Flavor::SquirrelRespawn)
        .map_err(|e| e.display(source, Some("Lexer error")).to_string())?;

    let ast = sqparse::parse(&tokens, sqparse::Flavor::SquirrelRespawn)
        .map_err(|e| e.display(source, &tokens, Some("Parse error")).to_string())?;

    let writer = Writer::new(Arc::new(format));
    match program(&ast)(writer) {
        Some(w) => {
            let mut s = w.to_string();
            if s.ends_with('\n') {
                restore_disabled_regions(&mut s, &disabled_regions);
            } else {
                s.push('\n');
                restore_disabled_regions(&mut s, &disabled_regions);
            }
            Ok(s)
        }
        None => Err("Formatting failed: could not fit output within column limit".to_string()),
    }
}

fn format_disabled_regions(source: &str) -> Vec<Range<usize>> {
    let mut regions = Vec::new();
    let mut disabled_at = None;
    let mut line_start = 0;

    for line in source.split_inclusive('\n') {
        let line_end = line_start + line.len();
        match line.trim() {
            "// fmt: off" if disabled_at.is_none() => disabled_at = Some(line_end),
            "// fmt: on" => {
                if let Some(start) = disabled_at.take() {
                    regions.push(start..line_start);
                }
            }
            _ => {}
        }
        line_start = line_end;
    }
    if let Some(start) = disabled_at {
        regions.push(start..source.len());
    }
    regions
}

fn restore_disabled_regions(formatted: &mut String, original_regions: &[&str]) {
    let formatted_regions = format_disabled_regions(formatted);
    if formatted_regions.len() != original_regions.len() {
        return;
    }

    for (range, original) in formatted_regions.into_iter().zip(original_regions).rev() {
        formatted.replace_range(range, original);
    }
}

/// Format a Squirrel source string using default format configuration.
pub fn format_source_default(source: &str) -> Result<String, String> {
    format_source(source, Format::default())
}
