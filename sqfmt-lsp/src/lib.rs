use std::ops::Range as ByteRange;

use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticSeverity, DocumentSymbol, Position, Range, Uri,
};

mod diagnostics;
mod symbol;
mod tokens;

pub use sqfmt_lint::conditional::{
    ConditionalSpan, VmTargets, condition_targets, conditional_spans, targets_at,
};
pub use sqfmt_lint::semantic::{
    DeclarationKind, OwnedArgument, OwnedCall, OwnedDeclaration, OwnedDuplicate,
    OwnedMemberReference, OwnedParameter, OwnedReference, OwnedSignature, SemanticDocument,
    TypeIdentity, ValueSource,
};
pub use sqfmt_lint::{LoadOrder, ScriptEntry, read_manifest};
pub use symbol::{OwnedDocumentSymbol, OwnedWorkspaceSymbol};
pub use tokens::{LexicalToken, TOKEN_MODIFIERS, TOKEN_TYPES, semantic_tokens};

const MAX_SYNTAX_DIAGNOSTICS: usize = 100;

/// Everything one tokenization and parse of a document produces. Syntax errors are kept as owned
/// ranges and messages rather than diagnostics, so indexing a file it will never publish for costs
/// nothing extra.
#[derive(Debug, Default)]
pub struct DocumentAnalysis {
    pub symbols: Vec<OwnedDocumentSymbol>,
    pub semantic: SemanticDocument,
    pub lint: sqfmt_lint::Analysis,
    /// Tokens kept for semantic-token requests, which would otherwise tokenize again.
    pub lexical: Vec<LexicalToken>,
    errors: Vec<(ByteRange<usize>, String)>,
}

/// Tokenizes and parses a document once, then builds every owned view of it the server keeps.
pub fn analyze_document(source: &str) -> DocumentAnalysis {
    let tokenization = match sqparse::tokenize_partial_with_error_limit(
        source,
        sqparse::Flavor::SquirrelRespawn,
        MAX_SYNTAX_DIAGNOSTICS,
    ) {
        Ok(tokenization) => tokenization,
        Err(error) => {
            // Nothing was recovered, so the file has no symbols, semantics, or tokens at all.
            return DocumentAnalysis {
                errors: vec![(error.range, error.ty.to_string())],
                ..DocumentAnalysis::default()
            };
        }
    };
    let mut errors = tokenization
        .errors
        .iter()
        .map(|error| (error.range.clone(), error.ty.to_string()))
        .collect::<Vec<_>>();
    let mut lexical = Vec::new();
    let parses = tokenization
        .regions
        .iter()
        .map(|region| {
            lexical.extend(
                region
                    .tokens
                    .iter()
                    .filter_map(|item| tokens::lexical_token(&item.token)),
            );
            sqparse::parse_partial(&region.tokens, sqparse::Flavor::SquirrelRespawn)
        })
        .collect::<Vec<_>>();
    let mut statements = Vec::new();
    for (region, partial) in tokenization.regions.iter().zip(&parses) {
        errors.extend(
            partial
                .errors
                .iter()
                .filter(|recovery| {
                    !region.ends_at_error || recovery.error.token_index < region.tokens.len()
                })
                .map(|recovery| {
                    let range = region
                        .tokens
                        .get(recovery.error.token_index)
                        .map_or(source.len()..source.len(), |item| item.token.range.clone());
                    (range, recovery.error.ty.to_string())
                }),
        );
        statements.extend(
            partial
                .statements
                .iter()
                .filter(|parsed| region.is_trusted_statement_end(parsed.token_range.end))
                .map(|parsed| &parsed.statement),
        );
    }
    errors.sort_by(|left, right| {
        left.0
            .start
            .cmp(&right.0.start)
            .then_with(|| left.0.end.cmp(&right.0.end))
            .then_with(|| left.1.cmp(&right.1))
    });
    errors.dedup();
    errors.truncate(MAX_SYNTAX_DIAGNOSTICS);
    let lint_tokens = tokenization
        .regions
        .iter()
        .flat_map(|region| region.tokens.iter().map(|item| &item.token))
        .collect::<Vec<_>>();
    DocumentAnalysis {
        symbols: symbol::extract_statements(&statements),
        semantic: sqfmt_lint::semantic::analyze_statements(source, &statements),
        lint: sqfmt_lint::analyze_statements_with_tokens(source, &statements, &lint_tokens),
        lexical,
        errors,
    }
}

impl DocumentAnalysis {
    pub fn syntax_diagnostics(&self, source: &str) -> Vec<Diagnostic> {
        self.errors
            .iter()
            .map(|(range, message)| diagnostic(source, range.clone(), message.clone()))
            .collect()
    }

    /// Syntax diagnostics for an open document. Semantic rules are evaluated by `sqfmt-lint`
    /// against the workspace so the CLI and language server use the same rule implementation.
    pub fn diagnostics(&self, _uri: &Uri, source: &str) -> Vec<Diagnostic> {
        self.syntax_diagnostics(source)
    }
}

pub fn syntax_diagnostics(source: &str) -> Vec<Diagnostic> {
    analyze_document(source).syntax_diagnostics(source)
}

/// A warning at a byte range, shaped like every other diagnostic this server publishes. Checks
/// that need the workspace index build their diagnostics with this.
pub fn warning(source: &str, byte_range: ByteRange<usize>, message: String) -> Diagnostic {
    diagnostics::build(source, byte_range, message, DiagnosticSeverity::WARNING)
}

pub fn full_document_range(source: &str) -> Range {
    Range::new(Position::new(0, 0), position_at(source, source.len()))
}

pub fn document_symbols(source: &str) -> Vec<DocumentSymbol> {
    owned_document_symbols(source)
        .into_iter()
        .map(|symbol| symbol.into_lsp(source))
        .collect()
}

pub fn owned_document_symbols(source: &str) -> Vec<OwnedDocumentSymbol> {
    symbol::extract(source)
}

pub fn workspace_symbols(
    symbols: &[OwnedDocumentSymbol],
    query: &str,
) -> Vec<OwnedWorkspaceSymbol> {
    symbol::workspace_symbols(symbols, query)
}

pub fn position_at(source: &str, byte_offset: usize) -> Position {
    let byte_offset = byte_offset.min(source.len());
    let byte_offset = floor_char_boundary(source, byte_offset);
    let before = &source[..byte_offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let character = source[line_start..byte_offset].encode_utf16().count() as u32;
    Position::new(line, character)
}

pub fn offset_at(source: &str, position: Position) -> Option<usize> {
    let mut line_start = 0;
    for _ in 0..position.line {
        line_start += source[line_start..].find('\n')? + 1;
    }

    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |offset| line_start + offset);
    let mut utf16_offset = 0;
    for (byte_offset, character) in source[line_start..line_end].char_indices() {
        if utf16_offset == position.character {
            return Some(line_start + byte_offset);
        }
        utf16_offset += character.len_utf16() as u32;
        if utf16_offset > position.character {
            return None;
        }
    }
    (utf16_offset == position.character).then_some(line_end)
}

pub fn semantic_document(source: &str) -> SemanticDocument {
    sqfmt_lint::semantic::analyze(source)
}

pub fn is_valid_identifier(value: &str) -> bool {
    sqfmt_lint::is_valid_identifier(value)
}

fn diagnostic(source: &str, byte_range: ByteRange<usize>, message: String) -> Diagnostic {
    diagnostics::build(source, byte_range, message, DiagnosticSeverity::ERROR)
}

fn floor_char_boundary(source: &str, mut offset: usize) -> usize {
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_SYNTAX_DIAGNOSTICS, analyze_document, document_symbols, full_document_range,
        is_valid_identifier, offset_at, owned_document_symbols, position_at, syntax_diagnostics,
        workspace_symbols,
    };
    use crate::diagnostics;
    use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, Position, SymbolKind, Uri};

    fn semantic_diagnostics(source: &str) -> Vec<Diagnostic> {
        let uri = Uri::from_file_path("/project/test.nut").expect("absolute path");
        let analysis = analyze_document(source);
        sqfmt_lint::SemanticWorkspace::new([sqfmt_lint::SemanticFile {
            id: uri,
            document: &analysis.semantic,
            targets: sqfmt_lint::VmTargets::ALL,
        }])
        .diagnostics(&Uri::from_file_path("/project/test.nut").expect("absolute path"))
        .into_iter()
        .map(|diagnostic| {
            diagnostics::build(
                source,
                diagnostic.range,
                diagnostic.message,
                DiagnosticSeverity::WARNING,
            )
        })
        .collect()
    }

    #[test]
    fn converts_byte_offsets_to_utf16_positions() {
        let source = "a😀b\nç";

        assert_eq!(position_at(source, 1), Position::new(0, 1));
        assert_eq!(position_at(source, 5), Position::new(0, 3));
        assert_eq!(position_at(source, source.len()), Position::new(1, 1));
    }

    #[test]
    fn converts_utf16_positions_to_byte_offsets() {
        let source = "a😀b\nç";

        assert_eq!(offset_at(source, Position::new(0, 1)), Some(1));
        assert_eq!(offset_at(source, Position::new(0, 3)), Some(5));
        assert_eq!(offset_at(source, Position::new(1, 1)), Some(source.len()));
        assert_eq!(offset_at(source, Position::new(0, 2)), None);
    }

    #[test]
    fn validates_squirrel_identifiers() {
        assert!(is_valid_identifier("valid_name2"));
        assert!(!is_valid_identifier("2invalid"));
        assert!(!is_valid_identifier("two names"));
        assert!(!is_valid_identifier("function"));
        assert!(!is_valid_identifier(""));
    }

    #[test]
    fn computes_the_full_document_range() {
        let range = full_document_range("first\n😀");

        assert_eq!(range.start, Position::new(0, 0));
        assert_eq!(range.end, Position::new(1, 2));
    }

    #[test]
    fn valid_source_has_no_diagnostics() {
        let diagnostics = syntax_diagnostics("void function Foo() {}\n");

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn lexer_errors_become_diagnostics() {
        let diagnostics = syntax_diagnostics("void function Foo() {");

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("unclosed delimiter"));
        assert_eq!(diagnostics[0].source.as_deref(), Some("sqformat"));
    }

    #[test]
    fn parser_errors_become_diagnostics() {
        let diagnostics = syntax_diagnostics("void function Foo() { local x = }\n");

        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert!(diagnostics[0].message.contains("expected"));
    }

    #[test]
    fn collects_multiple_lexer_errors() {
        let diagnostics = syntax_diagnostics("€\n£\n");

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].range.start, Position::new(0, 0));
        assert_eq!(diagnostics[0].range.end, Position::new(0, 1));
        assert_eq!(diagnostics[1].range.start, Position::new(1, 0));
        assert_eq!(diagnostics[1].range.end, Position::new(1, 1));
    }

    #[test]
    fn caps_raw_lexer_diagnostics() {
        let source = "€\n".repeat(MAX_SYNTAX_DIAGNOSTICS + 1);

        assert_eq!(syntax_diagnostics(&source).len(), MAX_SYNTAX_DIAGNOSTICS);
    }

    #[test]
    fn reports_declarations_that_reuse_a_name_in_one_scope() {
        let source = concat!(
            "void function Duplicates( int first, int first )\n",
            "{\n",
            "\tlocal value = 1\n",
            "\tlocal value = 2\n",
            "}\n",
        );

        let diagnostics = semantic_diagnostics(source);

        assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].range.start, Position::new(0, 41));
        assert!(
            diagnostics[0]
                .message
                .contains("`first` is already declared in this scope"),
            "{diagnostics:#?}"
        );
        assert_eq!(
            diagnostics[0].severity,
            Some(DiagnosticSeverity::WARNING),
            "redeclaration compiles, so it is not an error"
        );
        assert_eq!(diagnostics[1].range.start, Position::new(3, 7));
    }

    #[test]
    fn reports_a_local_that_shadows_its_parameter() {
        let source = concat!(
            "void function Shadow( entity player )\n",
            "{\n",
            "\tentity player = GetLocalClientPlayer()\n",
            "}\n",
        );

        let diagnostics = semantic_diagnostics(source);

        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert!(
            diagnostics[0]
                .message
                .contains("`player` shadows the parameter"),
            "{diagnostics:#?}"
        );
    }

    #[test]
    fn keeps_separate_scopes_and_slots_out_of_duplicates() {
        let source = concat!(
            "global function Exported\n",
            "\n",
            "void function Exported()\n",
            "{\n",
            "\tlocal value = 1\n",
            "\t{\n",
            "\t\tlocal value = 2\n",
            "\t}\n",
            "\tforeach ( value in [ 1 ] )\n",
            "\t\tprintt( value )\n",
            "}\n",
            "\n",
            "void function Sibling()\n",
            "{\n",
            "\tlocal value = 3\n",
            "\t{\n",
            "\t\tfunction Table::Slot() {}\n",
            "\t\tfunction Other::Slot() {}\n",
            "\t}\n",
            "}\n",
        );

        let diagnostics = semantic_diagnostics(source);

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn keeps_exclusive_conditional_branches_out_of_duplicates() {
        let source = concat!(
            "void function Guarded()\n",
            "{\n",
            "\t#if SERVER\n",
            "\t\tlocal vm = 1\n",
            "\t#else\n",
            "\t\tlocal vm = 2\n",
            "\t#endif\n",
            "\t#if SP\n",
            "\t\tlocal build = 1\n",
            "\t#else\n",
            "\t\tlocal build = 2\n",
            "\t#endif\n",
            "\t#if DEV\n",
            "\t\tlocal chain = 1\n",
            "\t#endif\n",
            "\t#if MP\n",
            "\t\tlocal chain = 2\n",
            "\t#endif\n",
            "}\n",
        );

        let diagnostics = semantic_diagnostics(source);

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn reports_a_duplicate_inside_a_conditional_branch() {
        let source = concat!(
            "void function Guarded()\n",
            "{\n",
            "\t#if SERVER\n",
            "\t\tlocal value = 1\n",
            "\t\tlocal value = 2\n",
            "\t#endif\n",
            "}\n",
        );

        let diagnostics = semantic_diagnostics(source);

        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].range.start.line, 4);
    }

    #[test]
    fn collects_parser_errors_from_separate_statements() {
        let diagnostics = syntax_diagnostics("local first =\nlocal second =\nlocal valid = 1\n");

        assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].range.start.line, 1);
        assert_eq!(diagnostics[1].range.start.line, 2);
    }

    #[test]
    fn recovers_at_the_next_top_level_statement() {
        let diagnostics = syntax_diagnostics(
            "void function Broken() {\n\tlocal value =\n}\n\nvoid function Good() {}\n€\n",
        );

        assert!(diagnostics.len() >= 2);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.range.start.line < 3)
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.range.start.line == 5)
        );
    }

    #[test]
    fn recovers_symbols_around_parser_errors() {
        let source = r#"void function Before() {}
void function Broken() { local nested = }
class After {
	void function Respawn() {}
}
"#;

        let symbols = owned_document_symbols(source);
        assert_eq!(
            symbols
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            ["Before", "After"]
        );
    }

    #[test]
    fn recovers_symbols_around_delimiter_errors() {
        let source = r#"void function Before() {}
void function Broken() { local nested = Call(] }
class After {
	void function Respawn() {}
}
"#;

        let diagnostics = syntax_diagnostics(source);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert!(diagnostics[0].message.contains("mismatched delimiter"));

        let symbols = owned_document_symbols(source);
        assert_eq!(
            symbols
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            ["Before", "After"]
        );
    }

    #[test]
    fn recovers_symbols_around_raw_lexer_errors() {
        let source = r#"void function Before() {}
€ invalid
local broken = "text
class After {
	void function Respawn() {}
}
"#;

        let diagnostics = syntax_diagnostics(source);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("unrecognized token"))
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("cannot span multiple lines"))
        );

        let symbols = owned_document_symbols(source);
        assert_eq!(
            symbols
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            ["Before", "After"]
        );
    }

    #[test]
    fn preserves_symbols_before_an_unterminated_verbatim_string() {
        let source = "void function Before() {}\nlocal broken = @\"first\nsecond\n";

        let diagnostics = syntax_diagnostics(source);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert!(
            diagnostics[0]
                .message
                .contains("input ended in the middle of a string")
        );
        assert_eq!(owned_document_symbols(source)[0].name, "Before");
    }

    #[test]
    fn extracts_hierarchical_document_symbols() {
        let symbols = document_symbols(
            r#"class Pilot {
	health = 100
	void function Respawn() {
		local location = GetLocation()
	}
}

struct Loadout {
	string weapon
}

enum Team {
	Militia,
	Imc
}

global function Shared
typedef Callback void functionref()
"#,
        );

        assert_eq!(
            symbols
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            ["Pilot", "Loadout", "Team", "Shared", "Callback"]
        );
        assert_eq!(symbols[0].kind, SymbolKind::CLASS);
        let class_children = symbols[0].children.as_ref().unwrap();
        assert_eq!(class_children[0].name, "health");
        assert_eq!(class_children[0].kind, SymbolKind::FIELD);
        assert_eq!(class_children[1].name, "Respawn");
        assert_eq!(class_children[1].kind, SymbolKind::METHOD);
        assert_eq!(
            class_children[1].children.as_ref().unwrap()[0].name,
            "location"
        );
        assert_eq!(symbols[1].children.as_ref().unwrap()[0].name, "weapon");
        assert_eq!(symbols[2].children.as_ref().unwrap().len(), 2);
        assert_eq!(symbols[3].kind, SymbolKind::FUNCTION);
        assert_eq!(symbols[4].kind, SymbolKind::TYPE_PARAMETER);
    }

    #[test]
    fn invalid_source_has_no_document_symbols() {
        assert!(document_symbols("void function Broken() {").is_empty());
    }

    #[test]
    fn filters_and_flattens_workspace_symbols() {
        let symbols = owned_document_symbols(
            "class Pilot {\n void function Respawn() {\n  local location = GetLocation()\n }\n}\n",
        );

        let matches = workspace_symbols(&symbols, "rsp");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "Respawn");
        assert_eq!(matches[0].container_name.as_deref(), Some("Pilot"));

        let matches = workspace_symbols(&symbols, "loc");
        assert_eq!(matches[0].name, "location");
        assert_eq!(matches[0].container_name.as_deref(), Some("Pilot::Respawn"));
    }
}
