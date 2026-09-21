use std::ops::Range as ByteRange;

use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, Range};

use crate::position_at;

pub(crate) const DIAGNOSTIC_SOURCE: &str = "sqformat";

pub(crate) fn build(
    source: &str,
    byte_range: ByteRange<usize>,
    message: String,
    severity: DiagnosticSeverity,
) -> Diagnostic {
    let mut diagnostic = Diagnostic::new_simple(lsp_range(source, byte_range), message);
    diagnostic.severity = Some(severity);
    diagnostic.source = Some(DIAGNOSTIC_SOURCE.to_string());
    diagnostic
}

fn lsp_range(source: &str, byte_range: ByteRange<usize>) -> Range {
    Range::new(
        position_at(source, byte_range.start),
        position_at(source, byte_range.end),
    )
}
