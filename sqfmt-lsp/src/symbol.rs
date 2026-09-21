use std::ops::Range as ByteRange;

use sqparse::ast::{
    ClassDefinition, Expression, GlobalDefinition, IfStatementType, Slot, Statement, StatementType,
    VarDefinitionStatement,
};
use sqparse::token::Token;
use tower_lsp_server::ls_types::{DocumentSymbol, SymbolKind};

use crate::position_at;

#[derive(Clone, Debug)]
pub struct OwnedDocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: ByteRange<usize>,
    pub selection_range: ByteRange<usize>,
    pub children: Vec<OwnedDocumentSymbol>,
}

#[derive(Clone, Debug)]
pub struct OwnedWorkspaceSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub selection_range: ByteRange<usize>,
    pub container_name: Option<String>,
    pub score: usize,
}

impl OwnedDocumentSymbol {
    #[allow(deprecated)]
    pub fn into_lsp(self, source: &str) -> DocumentSymbol {
        DocumentSymbol {
            name: self.name,
            detail: None,
            kind: self.kind,
            tags: None,
            deprecated: None,
            range: tower_lsp_server::ls_types::Range::new(
                position_at(source, self.range.start),
                position_at(source, self.range.end),
            ),
            selection_range: tower_lsp_server::ls_types::Range::new(
                position_at(source, self.selection_range.start),
                position_at(source, self.selection_range.end),
            ),
            children: (!self.children.is_empty()).then(|| {
                self.children
                    .into_iter()
                    .map(|child| child.into_lsp(source))
                    .collect()
            }),
        }
    }
}

pub(crate) fn extract(source: &str) -> Vec<OwnedDocumentSymbol> {
    let Ok(tokenization) =
        sqparse::tokenize_partial_with_error_limit(source, sqparse::Flavor::SquirrelRespawn, 0)
    else {
        return Vec::new();
    };
    let mut symbols = Vec::new();
    for region in &tokenization.regions {
        let partial = sqparse::parse_partial(&region.tokens, sqparse::Flavor::SquirrelRespawn);
        for parsed in partial
            .statements
            .iter()
            .filter(|parsed| region.is_trusted_statement_end(parsed.token_range.end))
        {
            symbols.extend(symbols_for_statement(&parsed.statement));
        }
    }
    symbols
}

/// Symbols for statements a caller already recovered, so one tokenization and parse can serve
/// diagnostics, symbols, semantics, and tokens together.
pub(crate) fn extract_statements(statements: &[&Statement<'_>]) -> Vec<OwnedDocumentSymbol> {
    statements
        .iter()
        .flat_map(|statement| symbols_for_statement(statement))
        .collect()
}

pub(crate) fn workspace_symbols(
    symbols: &[OwnedDocumentSymbol],
    query: &str,
) -> Vec<OwnedWorkspaceSymbol> {
    let query = query.to_ascii_lowercase();
    let mut matches = Vec::new();
    flatten_symbols(symbols, None, &query, &mut matches);
    matches.sort_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
            .then_with(|| left.container_name.cmp(&right.container_name))
    });
    matches
}

fn flatten_symbols(
    symbols: &[OwnedDocumentSymbol],
    container_name: Option<&str>,
    query: &str,
    matches: &mut Vec<OwnedWorkspaceSymbol>,
) {
    for symbol in symbols {
        if let Some(score) = match_score(&symbol.name, query) {
            matches.push(OwnedWorkspaceSymbol {
                name: symbol.name.clone(),
                kind: symbol.kind,
                selection_range: symbol.selection_range.clone(),
                container_name: container_name.map(str::to_string),
                score,
            });
        }

        let child_container = match container_name {
            Some(container) => format!("{container}::{}", symbol.name),
            None => symbol.name.clone(),
        };
        flatten_symbols(&symbol.children, Some(&child_container), query, matches);
    }
}

fn match_score(name: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(4);
    }

    let name = name.to_ascii_lowercase();
    if name == query {
        return Some(0);
    }
    if name.starts_with(query) {
        return Some(1);
    }
    if name.contains(query) {
        return Some(2);
    }

    let mut name_chars = name.chars();
    if query
        .chars()
        .all(|query_char| name_chars.by_ref().any(|name_char| name_char == query_char))
    {
        Some(3)
    } else {
        None
    }
}

fn symbols_for_statement(statement: &Statement<'_>) -> Vec<OwnedDocumentSymbol> {
    symbols_for_statement_type(&statement.ty)
}

fn symbols_for_statement_type(statement: &StatementType<'_>) -> Vec<OwnedDocumentSymbol> {
    match statement {
        StatementType::Block(block) => block
            .statements
            .iter()
            .flat_map(symbols_for_statement)
            .collect(),
        StatementType::If(statement) => match &statement.ty {
            IfStatementType::NoElse { body } => symbols_for_statement_type(body),
            IfStatementType::Else {
                body, else_body, ..
            } => symbols_for_statement(body)
                .into_iter()
                .chain(symbols_for_statement_type(else_body))
                .collect(),
        },
        StatementType::While(statement) => symbols_for_statement_type(&statement.body),
        StatementType::DoWhile(statement) => symbols_for_statement(&statement.body),
        StatementType::Switch(statement) => statement
            .cases
            .iter()
            .flat_map(|case| case.body.iter().flat_map(symbols_for_statement))
            .collect(),
        StatementType::For(statement) => symbols_for_statement_type(&statement.body),
        StatementType::Foreach(statement) => symbols_for_statement_type(&statement.body),
        StatementType::TryCatch(statement) => symbols_for_statement(&statement.body)
            .into_iter()
            .chain(symbols_for_statement_type(&statement.catch_body))
            .collect(),
        StatementType::VarDefinition(statement) => var_symbols(statement, SymbolKind::VARIABLE),
        StatementType::ConstructorDefinition(statement) => {
            let name = statement.last_name.value.to_string();
            vec![function_symbol(
                name,
                SymbolKind::CONSTRUCTOR,
                statement.last_name.token,
                statement.function,
                &statement.definition.body,
            )]
        }
        StatementType::FunctionDefinition(statement) => {
            let name = qualified_name(&statement.name);
            vec![function_symbol(
                name,
                SymbolKind::FUNCTION,
                statement.name.last_item.token,
                statement.function,
                &statement.definition.body,
            )]
        }
        StatementType::ClassDefinition(statement) => vec![class_symbol(
            expression_name(&statement.name),
            expression_selection(&statement.name),
            statement.class,
            &statement.definition,
        )],
        StatementType::Const(statement) => vec![leaf_symbol(
            statement.name.value,
            SymbolKind::CONSTANT,
            statement.name.token,
        )],
        StatementType::EnumDefinition(statement) => {
            let children = statement
                .entries
                .iter()
                .map(|entry| {
                    leaf_symbol(entry.name.value, SymbolKind::ENUM_MEMBER, entry.name.token)
                })
                .collect();
            vec![parent_symbol(
                statement.name.value,
                SymbolKind::ENUM,
                statement.name.token,
                statement.enum_,
                statement.close,
                children,
            )]
        }
        StatementType::StructDefinition(statement) => vec![struct_symbol(
            statement.name.value,
            statement.name.token,
            statement.struct_,
            &statement.definition,
        )],
        StatementType::TypeDefinition(statement) => vec![leaf_symbol(
            statement.name.value,
            SymbolKind::TYPE_PARAMETER,
            statement.name.token,
        )],
        StatementType::Global(statement) => global_symbols(&statement.definition),
        _ => Vec::new(),
    }
}

fn global_symbols(definition: &GlobalDefinition<'_>) -> Vec<OwnedDocumentSymbol> {
    match definition {
        GlobalDefinition::Function { name, .. } => {
            vec![leaf_symbol(name.value, SymbolKind::FUNCTION, name.token)]
        }
        GlobalDefinition::UntypedVar { name, .. } => {
            vec![leaf_symbol(name.value, SymbolKind::VARIABLE, name.token)]
        }
        GlobalDefinition::TypedVar(statement) => var_symbols(statement, SymbolKind::VARIABLE),
        GlobalDefinition::Const(statement) => vec![leaf_symbol(
            statement.name.value,
            SymbolKind::CONSTANT,
            statement.name.token,
        )],
        GlobalDefinition::Enum(statement) => {
            let children = statement
                .entries
                .iter()
                .map(|entry| {
                    leaf_symbol(entry.name.value, SymbolKind::ENUM_MEMBER, entry.name.token)
                })
                .collect();
            vec![parent_symbol(
                statement.name.value,
                SymbolKind::ENUM,
                statement.name.token,
                statement.enum_,
                statement.close,
                children,
            )]
        }
        GlobalDefinition::Class(statement) => vec![class_symbol(
            expression_name(&statement.name),
            expression_selection(&statement.name),
            statement.class,
            &statement.definition,
        )],
        GlobalDefinition::Struct(statement) => vec![struct_symbol(
            statement.name.value,
            statement.name.token,
            statement.struct_,
            &statement.definition,
        )],
        GlobalDefinition::Type(statement) => vec![leaf_symbol(
            statement.name.value,
            SymbolKind::TYPE_PARAMETER,
            statement.name.token,
        )],
    }
}

fn class_symbol(
    name: String,
    selection: &Token<'_>,
    start: &Token<'_>,
    definition: &ClassDefinition<'_>,
) -> OwnedDocumentSymbol {
    let children = definition
        .members
        .iter()
        .map(|member| match &member.slot {
            Slot::Property { name, .. } => leaf_symbol(name.value, SymbolKind::FIELD, name.token),
            Slot::ComputedProperty { open, close, .. } => parent_symbol(
                "[computed]",
                SymbolKind::FIELD,
                open,
                open,
                close,
                Vec::new(),
            ),
            Slot::Constructor {
                constructor,
                definition,
                ..
            } => function_symbol(
                "constructor".to_string(),
                SymbolKind::CONSTRUCTOR,
                constructor,
                constructor,
                &definition.body,
            ),
            Slot::Function {
                function,
                name,
                definition,
                ..
            } => function_symbol(
                name.value.to_string(),
                SymbolKind::METHOD,
                name.token,
                function,
                &definition.body,
            ),
        })
        .collect();
    OwnedDocumentSymbol {
        name,
        kind: SymbolKind::CLASS,
        range: start.range.start..definition.close.range.end,
        selection_range: selection.range.clone(),
        children,
    }
}

fn struct_symbol(
    name: &str,
    name_token: &Token<'_>,
    start: &Token<'_>,
    definition: &sqparse::ast::StructDefinition<'_>,
) -> OwnedDocumentSymbol {
    let children = definition
        .properties
        .iter()
        .map(|property| leaf_symbol(property.name.value, SymbolKind::FIELD, property.name.token))
        .collect();
    parent_symbol(
        name,
        SymbolKind::STRUCT,
        name_token,
        start,
        definition.close,
        children,
    )
}

fn function_symbol(
    name: String,
    kind: SymbolKind,
    selection: &Token<'_>,
    start: &Token<'_>,
    body: &StatementType<'_>,
) -> OwnedDocumentSymbol {
    let (end, children) = match body {
        StatementType::Block(block) => (
            block.close.range.end,
            block
                .statements
                .iter()
                .flat_map(symbols_for_statement)
                .collect(),
        ),
        _ => (selection.range.end, Vec::new()),
    };
    OwnedDocumentSymbol {
        name,
        kind,
        range: start.range.start..end,
        selection_range: selection.range.clone(),
        children,
    }
}

fn var_symbols(
    statement: &VarDefinitionStatement<'_>,
    kind: SymbolKind,
) -> Vec<OwnedDocumentSymbol> {
    statement
        .definitions
        .items
        .iter()
        .map(|(definition, _)| leaf_symbol(definition.name.value, kind, definition.name.token))
        .chain(std::iter::once(leaf_symbol(
            statement.definitions.last_item.name.value,
            kind,
            statement.definitions.last_item.name.token,
        )))
        .collect()
}

fn parent_symbol(
    name: &str,
    kind: SymbolKind,
    selection: &Token<'_>,
    start: &Token<'_>,
    end: &Token<'_>,
    children: Vec<OwnedDocumentSymbol>,
) -> OwnedDocumentSymbol {
    OwnedDocumentSymbol {
        name: name.to_string(),
        kind,
        range: start.range.start..end.range.end,
        selection_range: selection.range.clone(),
        children,
    }
}

fn leaf_symbol(name: &str, kind: SymbolKind, token: &Token<'_>) -> OwnedDocumentSymbol {
    OwnedDocumentSymbol {
        name: name.to_string(),
        kind,
        range: token.range.clone(),
        selection_range: token.range.clone(),
        children: Vec::new(),
    }
}

fn qualified_name(
    names: &sqparse::ast::SeparatedList1<'_, sqparse::ast::Identifier<'_>>,
) -> String {
    names
        .items
        .iter()
        .map(|(name, _)| name.value)
        .chain(std::iter::once(names.last_item.value))
        .collect::<Vec<_>>()
        .join("::")
}

fn expression_name(expression: &Expression<'_>) -> String {
    match expression {
        Expression::Var(expression) => expression.name.value.to_string(),
        Expression::RootVar(expression) => expression.name.value.to_string(),
        Expression::Property(expression) => match &expression.property {
            sqparse::ast::MethodIdentifier::Identifier(name) => name.value.to_string(),
            sqparse::ast::MethodIdentifier::Constructor(_) => "constructor".to_string(),
        },
        _ => "<class>".to_string(),
    }
}

fn expression_selection<'s>(expression: &'s Expression<'s>) -> &'s Token<'s> {
    match expression {
        Expression::Var(expression) => expression.name.token,
        Expression::RootVar(expression) => expression.name.token,
        Expression::Property(expression) => match &expression.property {
            sqparse::ast::MethodIdentifier::Identifier(name) => name.token,
            sqparse::ast::MethodIdentifier::Constructor(token) => token,
        },
        Expression::Parens(expression) => expression_selection(&expression.value),
        _ => expression_start_token(expression),
    }
}

fn expression_start_token<'s>(expression: &'s Expression<'s>) -> &'s Token<'s> {
    match expression {
        Expression::Parens(expression) => expression.open,
        Expression::Literal(expression) => expression.token,
        Expression::Var(expression) => expression.name.token,
        Expression::RootVar(expression) => expression.root,
        Expression::Index(expression) => expression_start_token(&expression.base),
        Expression::Property(expression) => expression_start_token(&expression.base),
        Expression::Ternary(expression) => expression_start_token(&expression.condition),
        Expression::Binary(expression) => expression_start_token(&expression.left),
        Expression::Prefix(expression) => expression_start_token(&expression.value),
        Expression::Postfix(expression) => expression_start_token(&expression.value),
        Expression::Comma(expression) => expression_start_token(&expression.values.items[0].0),
        Expression::Table(expression) => expression.open,
        Expression::Class(expression) => expression.class,
        Expression::Array(expression) => expression.open,
        Expression::Function(expression) => expression.function,
        Expression::Lambda(expression) => expression.at,
        Expression::Call(expression) => expression_start_token(&expression.function),
        Expression::Delegate(expression) => expression.delegate,
        Expression::Vector(expression) => expression.open,
        Expression::Expect(expression) => expression.expect,
    }
}
