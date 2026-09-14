//! Semantic tokens.
//!
//! Lexical tokens supply keywords, literals, and operators. Identifiers are classified from the
//! document's own declarations and references, falling back to a workspace lookup for names this
//! file does not declare. Code the file's VM cannot reach is reported as comment tokens so themes
//! dim it.

use std::collections::HashMap;
use std::ops::Range as ByteRange;

use tower_lsp_server::ls_types::{SemanticToken, SemanticTokenModifier, SemanticTokenType};

use sqfmt_lint::{DeclarationKind, SemanticDocument, VmTargets};

/// Token types this server produces, in legend order.
pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::KEYWORD,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
    SemanticTokenType::OPERATOR,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::CLASS,
    SemanticTokenType::STRUCT,
    SemanticTokenType::ENUM,
    SemanticTokenType::TYPE,
    SemanticTokenType::COMMENT,
];

/// Token modifiers this server produces, in legend order.
pub const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::READONLY,
];

const KEYWORD: u32 = 0;
const STRING: u32 = 1;
const NUMBER: u32 = 2;
const OPERATOR: u32 = 3;
const FUNCTION: u32 = 4;
const METHOD: u32 = 5;
const VARIABLE: u32 = 6;
const PARAMETER: u32 = 7;
const PROPERTY: u32 = 8;
const CLASS: u32 = 9;
const STRUCT: u32 = 10;
const ENUM: u32 = 11;
const TYPE: u32 = 12;
const COMMENT: u32 = 13;

const DECLARATION: u32 = 1;
const READONLY: u32 = 2;

/// What a token's own lexical kind says about it. An identifier says nothing on its own: it is
/// classified from the document's declarations and references, or from the workspace index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexicalKind {
    Classified(u32),
    Identifier,
}

/// A token kept from the document's one tokenization, so semantic-token requests do not tokenize
/// the document again.
#[derive(Clone, Debug)]
pub struct LexicalToken {
    pub range: ByteRange<usize>,
    pub kind: LexicalKind,
}

/// What a token contributes on its own, or `None` for punctuation and empty tokens, which are left
/// to the client's grammar so bracket colorization survives.
pub(crate) fn lexical_token(token: &sqparse::token::Token<'_>) -> Option<LexicalToken> {
    if token.range.is_empty() {
        return None;
    }
    let kind = match &token.ty {
        sqparse::token::TokenType::Terminal(terminal) => {
            LexicalKind::Classified(terminal_type(terminal.as_str())?)
        }
        sqparse::token::TokenType::Literal(literal) => {
            LexicalKind::Classified(literal_type(*literal))
        }
        sqparse::token::TokenType::Identifier(_) => LexicalKind::Identifier,
        sqparse::token::TokenType::Empty => return None,
    };
    Some(LexicalToken {
        range: token.range.clone(),
        kind,
    })
}

/// Builds the delta-encoded tokens for a document from the tokens its analysis kept.
/// `resolve_global` classifies names the document does not declare itself, and `file_targets` are
/// the VMs the file runs in.
pub fn semantic_tokens(
    source: &str,
    lexical: &[LexicalToken],
    semantic: &SemanticDocument,
    file_targets: VmTargets,
    resolve_global: &dyn Fn(&str) -> Option<DeclarationKind>,
) -> Vec<SemanticToken> {
    let mut raw = Vec::new();
    let inactive = inactive_ranges(semantic, file_targets);
    let declarations = index_by_start(semantic.declarations.iter().map(|declaration| {
        (
            declaration.range.clone(),
            (
                token_type(declaration.kind),
                modifiers(declaration.kind) | DECLARATION,
            ),
        )
    }));
    let references = index_by_start(semantic.references.iter().filter_map(|reference| {
        let value = match &reference.target {
            Some(target) => {
                let kind = semantic.declaration_for_range(target)?.kind;
                (token_type(kind), modifiers(kind))
            }
            None => match resolve_global(&reference.name) {
                Some(kind) => (token_type(kind), modifiers(kind)),
                // Builtin type names are not declared anywhere in the project.
                None if is_builtin_type(&reference.name) => (TYPE, 0),
                None => return None,
            },
        };
        Some((reference.range.clone(), value))
    }));
    let members = index_by_start(
        semantic
            .member_references
            .iter()
            .map(|reference| (reference.range.clone(), (PROPERTY, 0))),
    );

    for token in lexical {
        if in_any(&inactive, &token.range) {
            continue;
        }
        let classified = match token.kind {
            LexicalKind::Classified(token_type) => Some((token_type, 0)),
            LexicalKind::Identifier => declarations
                .get(&token.range.start)
                .or_else(|| references.get(&token.range.start))
                .or_else(|| members.get(&token.range.start))
                .copied(),
        };
        if let Some((token_type, modifiers)) = classified {
            raw.push((token.range.clone(), token_type, modifiers));
        }
    }

    for range in &inactive {
        for line in line_ranges(source, range) {
            raw.push((line, COMMENT, 0));
        }
    }

    raw.sort_by_key(|(range, _, _)| range.start);
    encode(source, raw)
}

fn index_by_start(
    entries: impl Iterator<Item = (ByteRange<usize>, (u32, u32))>,
) -> HashMap<usize, (u32, u32)> {
    let mut map = HashMap::new();
    for (range, value) in entries {
        map.entry(range.start).or_insert(value);
    }
    map
}

/// The regions of the document that the file's VMs cannot reach.
fn inactive_ranges(semantic: &SemanticDocument, file_targets: VmTargets) -> Vec<ByteRange<usize>> {
    if file_targets.is_all() {
        return Vec::new();
    }
    semantic
        .conditions
        .iter()
        .filter(|span| !span.targets.compatible_with(file_targets))
        .map(|span| span.range.clone())
        .collect()
}

fn in_any(ranges: &[ByteRange<usize>], range: &ByteRange<usize>) -> bool {
    ranges.iter().any(|inactive| {
        inactive.start <= range.start && range.start < inactive.end.max(inactive.start + 1)
    })
}

/// Splits a range into per-line pieces, because a semantic token may not span lines.
fn line_ranges(source: &str, range: &ByteRange<usize>) -> Vec<ByteRange<usize>> {
    let end = range.end.min(source.len());
    let mut ranges = Vec::new();
    let mut start = range.start;
    while start < end {
        let line_end = source[start..end]
            .find('\n')
            .map_or(end, |index| start + index);
        let trimmed = source[start..line_end].trim_end();
        if !trimmed.is_empty() {
            let leading =
                source[start..line_end].len() - source[start..line_end].trim_start().len();
            ranges.push(start + leading..start + leading + trimmed.trim_start().len());
        }
        start = line_end + 1;
    }
    ranges
}

/// Type names the language provides, which no file declares.
fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "void"
            | "var"
            | "bool"
            | "int"
            | "float"
            | "string"
            | "asset"
            | "vector"
            | "table"
            | "array"
            | "entity"
            | "functionref"
    )
}

/// Keywords and operators are classified; punctuation is left alone so the client keeps its own
/// bracket and delimiter colors.
fn terminal_type(text: &str) -> Option<u32> {
    if text
        .chars()
        .next()
        .is_some_and(|character| character.is_alphabetic() || character == '_')
    {
        return Some(KEYWORD);
    }
    text.chars()
        .any(|character| {
            !matches!(
                character,
                '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '.'
            )
        })
        .then_some(OPERATOR)
}

fn literal_type(literal: sqparse::token::LiteralToken<'_>) -> u32 {
    match literal {
        sqparse::token::LiteralToken::Int(_, _) | sqparse::token::LiteralToken::Float(_) => NUMBER,
        sqparse::token::LiteralToken::Char(_) | sqparse::token::LiteralToken::String(_) => STRING,
    }
}

fn token_type(kind: DeclarationKind) -> u32 {
    match kind {
        DeclarationKind::Function => FUNCTION,
        DeclarationKind::Constructor | DeclarationKind::Method => METHOD,
        DeclarationKind::Class => CLASS,
        DeclarationKind::Constant => VARIABLE,
        DeclarationKind::Enum => ENUM,
        DeclarationKind::Struct => STRUCT,
        DeclarationKind::Type => TYPE,
        DeclarationKind::Variable => VARIABLE,
        DeclarationKind::Parameter => PARAMETER,
        DeclarationKind::Field => PROPERTY,
    }
}

fn modifiers(kind: DeclarationKind) -> u32 {
    match kind {
        DeclarationKind::Constant => READONLY,
        _ => 0,
    }
}

fn encode(source: &str, raw: Vec<(ByteRange<usize>, u32, u32)>) -> Vec<SemanticToken> {
    let mut tokens = Vec::with_capacity(raw.len());
    let mut previous_line = 0;
    let mut previous_start = 0;
    let mut previous_end = 0;
    for (range, token_type, token_modifiers_bitset) in raw {
        // Overlapping or multi-line tokens are not representable.
        if range.start < previous_end {
            continue;
        }
        let start = crate::position_at(source, range.start);
        let end = crate::position_at(source, range.end);
        if start.line != end.line || end.character <= start.character {
            continue;
        }
        let delta_line = start.line - previous_line;
        let delta_start = if delta_line == 0 {
            start.character - previous_start
        } else {
            start.character
        };
        tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length: end.character - start.character,
            token_type,
            token_modifiers_bitset,
        });
        previous_line = start.line;
        previous_start = start.character;
        previous_end = range.end;
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze_document;

    fn tokens(source: &str, targets: VmTargets) -> Vec<SemanticToken> {
        let analysis = analyze_document(source);
        semantic_tokens(
            source,
            &analysis.lexical,
            &analysis.semantic,
            targets,
            &|_| None,
        )
    }

    #[test]
    fn classifies_declarations_and_references() {
        let source = "void function Example(int count) {\n\tlocal total = count\n}\n";
        let analysis = analyze_document(source);
        let tokens = semantic_tokens(
            source,
            &analysis.lexical,
            &analysis.semantic,
            VmTargets::ALL,
            &|_| None,
        );
        let types = tokens
            .iter()
            .map(|token| (token.token_type, token.token_modifiers_bitset))
            .collect::<Vec<_>>();

        // void function Example ( int count ) { local total = count }
        assert_eq!(types[0], (TYPE, 0), "the builtin return type");
        assert_eq!(types[1], (KEYWORD, 0), "the function keyword");
        assert_eq!(types[2], (FUNCTION, DECLARATION));
        assert!(types.contains(&(TYPE, 0)), "the builtin parameter type");
        assert!(types.contains(&(PARAMETER, DECLARATION)));
        assert!(types.contains(&(VARIABLE, DECLARATION)));
        // The use of the parameter in the body keeps its kind without the declaration modifier.
        assert!(types.contains(&(PARAMETER, 0)));
    }

    #[test]
    fn resolves_unknown_names_through_the_workspace() {
        let source = "void function Example() {\n\tGlobalThing()\n}\n";
        let analysis = analyze_document(source);
        let resolved = semantic_tokens(
            source,
            &analysis.lexical,
            &analysis.semantic,
            VmTargets::ALL,
            &|name| (name == "GlobalThing").then_some(DeclarationKind::Function),
        );
        assert!(resolved.iter().any(|token| {
            token.delta_line == 1
                && token.delta_start == 1
                && token.token_type == FUNCTION
                && token.token_modifiers_bitset == 0
        }));
        let unresolved = semantic_tokens(
            source,
            &analysis.lexical,
            &analysis.semantic,
            VmTargets::ALL,
            &|_| None,
        );
        assert!(
            !unresolved
                .iter()
                .any(|token| token.delta_line == 1 && token.delta_start == 1),
            "an unresolved GlobalThing gets no token"
        );
    }

    #[test]
    fn dims_regions_the_file_cannot_reach() {
        let source =
            "#if SERVER\nvoid function ServerOnly() {}\n#endif\nvoid function Always() {}\n";
        let dimmed = tokens(source, VmTargets::UI);
        let comments = dimmed
            .iter()
            .filter(|token| token.token_type == COMMENT)
            .count();
        assert_eq!(comments, 1, "the guarded line is reported as a comment");
        assert!(
            dimmed
                .iter()
                .any(|token| token.token_type == FUNCTION
                    && token.token_modifiers_bitset == DECLARATION),
            "the unguarded declaration is still classified"
        );
        // A file that runs everywhere has nothing to dim.
        assert!(
            tokens(source, VmTargets::ALL)
                .iter()
                .all(|token| token.token_type != COMMENT)
        );
    }

    #[test]
    fn encodes_positions_as_deltas() {
        let source = "local a = 1\nlocal b = 2\n";
        let tokens = tokens(source, VmTargets::ALL);
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 0);
        let second_line = tokens
            .iter()
            .position(|token| token.delta_line == 1)
            .expect("the second line starts a new delta line");
        assert_eq!(tokens[second_line].delta_start, 0);
    }
}
