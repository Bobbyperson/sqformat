use sqparse::ast::{
    ClassDefinition, Expression, FunctionDefinition, GlobalDefinition, IfStatementType, Slot,
    Statement, StatementType, TableSlotType, VarDefinitionStatement,
};
use sqparse::token::{Comment, LiteralToken, Token};
use std::collections::HashSet;
use std::ops::Range;

pub mod conditional;
mod manifest;
mod rules;
pub mod semantic;
mod semantic_rules;

pub use conditional::{
    ConditionalSpan, VmTargets, condition_targets, conditional_spans, targets_at,
};
pub use manifest::{LoadOrder, ScriptEntry, read_manifest};
pub use semantic::{
    DeclarationKind, OwnedArgument, OwnedCall, OwnedDeclaration, OwnedDuplicate,
    OwnedMemberReference, OwnedParameter, OwnedReference, OwnedSignature, SemanticDocument,
    TypeIdentity, ValueSource,
};
pub use semantic_rules::{
    ARGUMENT_TYPE_RULE, CALL_ARITY_RULE, DUPLICATE_DECLARATION_RULE, INITIALIZER_TYPE_RULE,
    INVALID_MEMBER_RULE, RETURN_TYPE_RULE, ResolvedType, SemanticFile, SemanticMember,
    SemanticWorkspace,
};

pub const THREADED_LOOP_RULE: &str = "threaded-loop-without-wait";
pub const INVALID_ENTITY_RULE: &str = "invalid-entity-use";
pub const WAIT_ZERO_RULE: &str = "wait-zero";
pub const UNREGISTERED_SIGNAL_RULE: &str = "unregistered-signal";
pub const UNCHECKED_ENCODED_EHANDLE_RULE: &str = "unchecked-encoded-ehandle";
pub const ENTITY_USE_AFTER_YIELD_RULE: &str = "entity-use-after-yield";
pub const UNSAFE_ARRAY_INDEX_RULE: &str = "unsafe-array-index";
pub const UNRESOLVED_MANIFEST_CALLBACK_RULE: &str = "unresolved-manifest-callback";
pub const REMOTE_FUNCTION_CONTRACT_RULE: &str = "remote-function-contract-mismatch";
pub const THREAD_IN_POLLING_LOOP_RULE: &str = "thread-spawned-inside-polling-loop";
pub const FIND_USED_AS_BOOLEAN_RULE: &str = "find-used-as-boolean";

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    threaded_functions: HashSet<String>,
    candidates: Vec<Candidate>,
    local_diagnostics: Vec<Diagnostic>,
    registered_signals: HashSet<String>,
    signal_uses: Vec<SignalUse>,
    function_signatures: Vec<FunctionSignature>,
    registered_remote_functions: HashSet<String>,
    remote_calls: Vec<RemoteCall>,
    suppressions: Vec<Suppression>,
}

#[derive(Clone, Debug)]
struct Suppression {
    range: Range<usize>,
    rules: HashSet<String>,
}

#[derive(Clone, Debug)]
struct Candidate {
    function: String,
    range: Range<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SignalUseKind {
    Emit,
    Consume,
}

#[derive(Clone, Debug)]
struct SignalUse {
    name: String,
    range: Range<usize>,
    kind: SignalUseKind,
}

#[derive(Clone, Debug)]
struct FunctionSignature {
    name: String,
    required: usize,
    maximum: Option<usize>,
}

impl FunctionSignature {
    fn accepts(&self, arguments: usize) -> bool {
        arguments >= self.required && self.maximum.is_none_or(|maximum| arguments <= maximum)
    }
}

#[derive(Clone, Debug)]
struct RemoteCall {
    name: String,
    arguments: usize,
    range: Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub range: Range<usize>,
    pub rule: &'static str,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LintOptions {
    pub advisory: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ManifestCallback {
    name: String,
    range: Range<usize>,
}

#[derive(Debug, Default)]
pub struct Workspace {
    threaded_functions: HashSet<String>,
    registered_signals: HashSet<String>,
    emitted_signals: HashSet<String>,
    consumed_signals: HashSet<String>,
    registered_remote_functions: HashSet<String>,
    function_signatures: Vec<FunctionSignature>,
}

impl Workspace {
    pub fn new<'a>(analyses: impl IntoIterator<Item = &'a Analysis>) -> Self {
        let analyses = analyses.into_iter().collect::<Vec<_>>();
        Self {
            threaded_functions: analyses
                .iter()
                .flat_map(|analysis| analysis.threaded_functions.iter().cloned())
                .collect(),
            registered_signals: analyses
                .iter()
                .flat_map(|analysis| analysis.registered_signals.iter().cloned())
                .collect(),
            emitted_signals: analyses
                .iter()
                .flat_map(|analysis| {
                    analysis
                        .signal_uses
                        .iter()
                        .filter(|use_| use_.kind == SignalUseKind::Emit)
                        .map(|use_| use_.name.clone())
                })
                .collect(),
            consumed_signals: analyses
                .iter()
                .flat_map(|analysis| {
                    analysis
                        .signal_uses
                        .iter()
                        .filter(|use_| use_.kind == SignalUseKind::Consume)
                        .map(|use_| use_.name.clone())
                })
                .collect(),
            registered_remote_functions: analyses
                .iter()
                .flat_map(|analysis| analysis.registered_remote_functions.iter().cloned())
                .collect(),
            function_signatures: analyses
                .iter()
                .flat_map(|analysis| analysis.function_signatures.iter().cloned())
                .collect(),
        }
    }

    pub fn diagnostics(&self, analysis: &Analysis) -> Vec<Diagnostic> {
        self.diagnostics_with_options(analysis, LintOptions::default())
    }

    pub fn diagnostics_with_options(
        &self,
        analysis: &Analysis,
        options: LintOptions,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = analysis.local_diagnostics.clone();
        diagnostics.extend(
            analysis
                .candidates
                .iter()
                .filter(|candidate| self.threaded_functions.contains(&candidate.function))
                .map(|candidate| Diagnostic {
                    range: candidate.range.clone(),
                    rule: THREADED_LOOP_RULE,
                    message: format!(
                        "loop in threaded function `{}` has no wait [{}]",
                        candidate.function, THREADED_LOOP_RULE
                    ),
                })
                .collect::<Vec<_>>(),
        );
        let mut reported_signals = HashSet::new();
        diagnostics.extend(analysis.signal_uses.iter().filter_map(|use_| {
            if is_engine_signal(&use_.name)
                || !self.emitted_signals.contains(&use_.name)
                || !self.consumed_signals.contains(&use_.name)
                || self.registered_signals.contains(&use_.name)
                || !reported_signals.insert(use_.name.as_str())
            {
                return None;
            }
            Some(Diagnostic {
                range: use_.range.clone(),
                rule: UNREGISTERED_SIGNAL_RULE,
                message: format!(
                    "custom signal `{}` is used without RegisterSignal [{}]",
                    use_.name, UNREGISTERED_SIGNAL_RULE
                ),
            })
        }));
        diagnostics.extend(analysis.remote_calls.iter().filter_map(|call| {
            if !self.registered_remote_functions.contains(&call.name) {
                return None;
            }
            let signatures = self
                .function_signatures
                .iter()
                .filter(|signature| signature.name == call.name)
                .collect::<Vec<_>>();
            if signatures.is_empty()
                || signatures
                    .iter()
                    .any(|signature| signature.accepts(call.arguments))
            {
                return None;
            }
            Some(Diagnostic {
                range: call.range.clone(),
                rule: REMOTE_FUNCTION_CONTRACT_RULE,
                message: format!(
                    "remote call to `{}` passes {} arguments, but no implementation accepts them [{}]",
                    call.name, call.arguments, REMOTE_FUNCTION_CONTRACT_RULE
                ),
            })
        }));
        diagnostics.sort_by(|left, right| {
            left.range
                .start
                .cmp(&right.range.start)
                .then_with(|| left.range.end.cmp(&right.range.end))
                .then_with(|| left.rule.cmp(right.rule))
        });
        diagnostics.dedup();
        if !options.advisory {
            diagnostics.retain(|diagnostic| !is_advisory_rule(diagnostic.rule));
        }
        analysis.retain_unsuppressed(&mut diagnostics);
        diagnostics
    }

    /// Validates Northstar `Before` and `After` manifest callbacks against functions in the
    /// indexed Squirrel workspace. Invalid JSON is left to the manifest loader to report.
    pub fn manifest_diagnostics(&self, source: &str) -> Vec<Diagnostic> {
        manifest_callbacks(source)
            .into_iter()
            .filter(|callback| {
                !self
                    .function_signatures
                    .iter()
                    .any(|signature| signature.name == callback.name)
            })
            .map(|callback| Diagnostic {
                range: callback.range,
                rule: UNRESOLVED_MANIFEST_CALLBACK_RULE,
                message: format!(
                    "manifest callback `{}` does not resolve to a function [{}]",
                    callback.name, UNRESOLVED_MANIFEST_CALLBACK_RULE
                ),
            })
            .collect()
    }
}

impl Analysis {
    pub fn retain_unsuppressed(&self, diagnostics: &mut Vec<Diagnostic>) {
        diagnostics.retain(|diagnostic| {
            !self.suppressions.iter().any(|suppression| {
                suppression.range.contains(&diagnostic.range.start)
                    && suppression.rules.contains(diagnostic.rule)
            })
        });
    }
}

fn is_advisory_rule(rule: &str) -> bool {
    matches!(
        rule,
        ENTITY_USE_AFTER_YIELD_RULE | THREAD_IN_POLLING_LOOP_RULE
    )
}

fn is_engine_signal(signal: &str) -> bool {
    signal == "OnDestroy"
}

fn manifest_callbacks(source: &str) -> Vec<ManifestCallback> {
    if serde_json::from_str::<serde_json::Value>(source).is_err() {
        return Vec::new();
    }

    let strings = json_strings(source);
    strings
        .windows(2)
        .filter_map(|pair| {
            let (key, key_range) = &pair[0];
            let (value, value_range) = &pair[1];
            if !matches!(key.as_str(), "Before" | "After")
                || source[key_range.end..value_range.start].trim() != ":"
            {
                return None;
            }
            Some(ManifestCallback {
                name: value.clone(),
                range: value_range.clone(),
            })
        })
        .collect()
}

fn json_strings(source: &str) -> Vec<(String, Range<usize>)> {
    let bytes = source.as_bytes();
    let mut strings = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'"' {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' => index = (index + 2).min(bytes.len()),
                b'"' => {
                    index += 1;
                    if let Ok(value) = serde_json::from_str::<String>(&source[start..index]) {
                        strings.push((value, start..index));
                    }
                    break;
                }
                _ => index += 1,
            }
        }
    }
    strings
}

pub fn analyze(source: &str) -> Result<Analysis, String> {
    let tokens = sqparse::tokenize(source, sqparse::Flavor::SquirrelRespawn)
        .map_err(|error| error.display(source, Some("Lexer error")).to_string())?;
    let ast = sqparse::parse(&tokens, sqparse::Flavor::SquirrelRespawn).map_err(|error| {
        error
            .display(source, &tokens, Some("Parse error"))
            .to_string()
    })?;
    let statements = ast.statements.iter().collect::<Vec<_>>();
    let tokens = tokens.iter().map(|item| &item.token).collect::<Vec<_>>();
    Ok(analyze_statements_with_tokens(source, &statements, &tokens))
}

pub fn is_valid_identifier(value: &str) -> bool {
    let Ok(tokens) = sqparse::tokenize(value, sqparse::Flavor::SquirrelRespawn) else {
        return false;
    };
    matches!(
        tokens.as_slice(),
        [item]
            if item.token.range == (0..value.len())
                && matches!(item.token.ty, sqparse::token::TokenType::Identifier(_))
    )
}

/// Collects the lint facts from statements that have already been parsed.
pub fn analyze_statements(statements: &[&Statement<'_>]) -> Analysis {
    let mut analysis = Analysis::default();
    for statement in statements {
        visit_statement(statement, &mut analysis);
    }
    rules::analyze(statements, &mut analysis);
    analysis
}

/// Analyzes parsed statements and applies `// nolint: rule-id` directives found in `tokens`.
pub fn analyze_statements_with_tokens(
    source: &str,
    statements: &[&Statement<'_>],
    tokens: &[&Token<'_>],
) -> Analysis {
    let mut analysis = analyze_statements(statements);
    analysis.suppressions = lint_suppressions(source, tokens);
    analysis
}

fn lint_suppressions(source: &str, tokens: &[&Token<'_>]) -> Vec<Suppression> {
    let mut suppressions = Vec::new();
    for token in tokens {
        let comments = token
            .before_lines
            .iter()
            .flat_map(|line| line.comments.iter())
            .chain(token.comments.iter())
            .chain(token.new_line.iter().flat_map(|line| line.comments.iter()));
        for comment in comments {
            let Comment::SingleLine(value) = comment else {
                continue;
            };
            let Some(rule_list) = value.trim().strip_prefix("nolint:") else {
                continue;
            };
            let rules = rule_list
                .split(|character: char| character == ',' || character.is_whitespace())
                .filter(|rule| !rule.is_empty())
                .map(str::to_owned)
                .collect::<HashSet<_>>();
            if rules.is_empty() {
                continue;
            }

            let value_offset = value.as_ptr() as usize - source.as_ptr() as usize;
            let comment_offset = value_offset.saturating_sub(2);
            let line_start = source[..comment_offset]
                .rfind('\n')
                .map_or(0, |offset| offset + 1);
            let line_end = source[comment_offset..]
                .find('\n')
                .map_or(source.len(), |offset| comment_offset + offset);
            let range = if source[line_start..comment_offset].trim().is_empty() {
                next_code_line(source, line_end.saturating_add(1)).unwrap_or(line_start..line_end)
            } else {
                line_start..line_end
            };
            suppressions.push(Suppression { range, rules });
        }
    }
    suppressions
}

fn next_code_line(source: &str, mut start: usize) -> Option<Range<usize>> {
    let mut in_block_comment = false;
    while start < source.len() {
        let end = source[start..]
            .find('\n')
            .map_or(source.len(), |offset| start + offset);
        if line_has_code(&source[start..end], &mut in_block_comment) {
            return Some(start..end);
        }
        start = end.saturating_add(1);
    }
    None
}

fn line_has_code(line: &str, in_block_comment: &mut bool) -> bool {
    let mut remaining = line.trim_start();
    loop {
        if *in_block_comment {
            let Some(end) = remaining.find("*/") else {
                return false;
            };
            remaining = remaining[end + 2..].trim_start();
            *in_block_comment = false;
            continue;
        }
        if remaining.starts_with("//") || remaining.is_empty() {
            return false;
        }
        if let Some(after_start) = remaining.strip_prefix("/*") {
            remaining = after_start;
            *in_block_comment = true;
            continue;
        }
        return true;
    }
}

pub fn diagnostics(analyses: &[Analysis]) -> Vec<Vec<Diagnostic>> {
    diagnostics_with_options(analyses, LintOptions::default())
}

pub fn diagnostics_with_options(
    analyses: &[Analysis],
    options: LintOptions,
) -> Vec<Vec<Diagnostic>> {
    let workspace = Workspace::new(analyses);
    analyses
        .iter()
        .map(|analysis| workspace.diagnostics_with_options(analysis, options))
        .collect()
}

fn visit_statement(statement: &Statement<'_>, analysis: &mut Analysis) {
    visit_statement_type(&statement.ty, analysis);
}

fn visit_statement_type(statement: &StatementType<'_>, analysis: &mut Analysis) {
    match statement {
        StatementType::Block(block) => {
            for statement in &block.statements {
                visit_statement(statement, analysis);
            }
        }
        StatementType::If(statement) => match &statement.ty {
            IfStatementType::NoElse { body } => visit_statement_type(body, analysis),
            IfStatementType::Else {
                body, else_body, ..
            } => {
                visit_statement(body, analysis);
                visit_statement_type(else_body, analysis);
            }
        },
        StatementType::While(statement) => visit_statement_type(&statement.body, analysis),
        StatementType::DoWhile(statement) => visit_statement(&statement.body, analysis),
        StatementType::Switch(statement) => {
            for case in &statement.cases {
                for statement in &case.body {
                    visit_statement(statement, analysis);
                }
            }
        }
        StatementType::For(statement) => visit_statement_type(&statement.body, analysis),
        StatementType::Foreach(statement) => visit_statement_type(&statement.body, analysis),
        StatementType::ConstructorDefinition(statement) => {
            visit_function_definition(None, &statement.definition, analysis)
        }
        StatementType::FunctionDefinition(statement) => visit_function_definition(
            Some(statement.name.last_item.value),
            &statement.definition,
            analysis,
        ),
        StatementType::ClassDefinition(statement) => {
            visit_class_definition(&statement.definition, analysis)
        }
        StatementType::TryCatch(statement) => {
            visit_statement(&statement.body, analysis);
            visit_statement_type(&statement.catch_body, analysis);
        }
        StatementType::Thread(statement) => {
            record_thread_call(&statement.value, analysis);
        }
        StatementType::DelayThread(statement) => {
            record_thread_call(&statement.value, analysis);
        }
        StatementType::Global(statement) => {
            if let GlobalDefinition::Class(definition) = &statement.definition {
                visit_class_definition(&definition.definition, analysis);
            }
        }
        _ => {}
    }
}

fn visit_function_definition(
    name: Option<&str>,
    definition: &FunctionDefinition<'_>,
    analysis: &mut Analysis,
) {
    if let Some(name) = name {
        find_bad_loops(&definition.body, name, &mut analysis.candidates);
    }
    visit_statement_type(&definition.body, analysis);
}

fn visit_class_definition(definition: &ClassDefinition<'_>, analysis: &mut Analysis) {
    for member in &definition.members {
        match &member.slot {
            Slot::Constructor { definition, .. } => {
                visit_function_definition(None, definition, analysis)
            }
            Slot::Function {
                name, definition, ..
            } => visit_function_definition(Some(name.value), definition, analysis),
            _ => {}
        }
    }
}

fn record_thread_call(expression: &Expression<'_>, analysis: &mut Analysis) {
    if let Some(name) = called_function_name(expression) {
        analysis.threaded_functions.insert(name.to_string());
    }
}

fn find_bad_loops(statement: &StatementType<'_>, function: &str, candidates: &mut Vec<Candidate>) {
    match statement {
        StatementType::Block(block) => {
            for statement in &block.statements {
                find_bad_loops(&statement.ty, function, candidates);
            }
        }
        StatementType::If(statement) => match &statement.ty {
            IfStatementType::NoElse { body } => find_bad_loops(body, function, candidates),
            IfStatementType::Else {
                body, else_body, ..
            } => {
                find_bad_loops(&body.ty, function, candidates);
                find_bad_loops(else_body, function, candidates);
            }
        },
        StatementType::While(statement) => {
            if expression_truth(&statement.condition) == Some(true) {
                record_loop(statement.while_, &statement.body, function, candidates);
            }
            find_bad_loops(&statement.body, function, candidates);
        }
        StatementType::DoWhile(statement) => {
            if expression_truth(&statement.condition) == Some(true) {
                record_loop(statement.do_, &statement.body.ty, function, candidates);
            }
            find_bad_loops(&statement.body.ty, function, candidates);
        }
        StatementType::For(statement) => {
            if statement
                .condition
                .as_ref()
                .is_none_or(|condition| expression_truth(condition) == Some(true))
            {
                record_loop(statement.for_, &statement.body, function, candidates);
            }
            find_bad_loops(&statement.body, function, candidates);
        }
        StatementType::Foreach(statement) => {
            find_bad_loops(&statement.body, function, candidates);
        }
        StatementType::Switch(statement) => {
            for case in &statement.cases {
                for statement in &case.body {
                    find_bad_loops(&statement.ty, function, candidates);
                }
            }
        }
        StatementType::TryCatch(statement) => {
            find_bad_loops(&statement.body.ty, function, candidates);
            find_bad_loops(&statement.catch_body, function, candidates);
        }
        StatementType::ConstructorDefinition(_)
        | StatementType::FunctionDefinition(_)
        | StatementType::ClassDefinition(_) => {}
        _ => {}
    }
}

fn record_loop(
    token: &Token<'_>,
    body: &StatementType<'_>,
    function: &str,
    candidates: &mut Vec<Candidate>,
) {
    if !contains_reachable_wait(body) && !contains_reachable_exit(body, true) {
        candidates.push(Candidate {
            function: function.to_string(),
            range: token.range.clone(),
        });
    }
}

fn contains_reachable_exit(statement: &StatementType<'_>, break_exits: bool) -> bool {
    match statement {
        StatementType::Break(_) => break_exits,
        StatementType::Return(_) => true,
        StatementType::Block(block) => {
            for statement in &block.statements {
                if contains_reachable_exit(&statement.ty, break_exits) {
                    return true;
                }
                if always_exits(&statement.ty) {
                    break;
                }
            }
            false
        }
        StatementType::If(statement) => {
            match (&statement.ty, expression_truth(&statement.condition)) {
                (IfStatementType::NoElse { body }, Some(true)) => {
                    contains_reachable_exit(body, break_exits)
                }
                (IfStatementType::NoElse { .. }, Some(false)) => false,
                (IfStatementType::NoElse { body }, None) => {
                    contains_reachable_exit(body, break_exits)
                }
                (IfStatementType::Else { body, .. }, Some(true)) => {
                    contains_reachable_exit(&body.ty, break_exits)
                }
                (IfStatementType::Else { else_body, .. }, Some(false)) => {
                    contains_reachable_exit(else_body, break_exits)
                }
                (
                    IfStatementType::Else {
                        body, else_body, ..
                    },
                    None,
                ) => {
                    contains_reachable_exit(&body.ty, break_exits)
                        || contains_reachable_exit(else_body, break_exits)
                }
            }
        }
        StatementType::TryCatch(statement) => {
            contains_reachable_exit(&statement.body.ty, break_exits)
                || contains_reachable_exit(&statement.catch_body, break_exits)
        }
        // Breaks inside these constructs do not target the outer loop, but returns still exit it.
        StatementType::While(statement) => contains_reachable_exit(&statement.body, false),
        StatementType::DoWhile(statement) => contains_reachable_exit(&statement.body.ty, false),
        StatementType::Switch(statement) => statement.cases.iter().any(|case| {
            case.body
                .iter()
                .any(|statement| contains_reachable_exit(&statement.ty, false))
        }),
        StatementType::For(statement) => contains_reachable_exit(&statement.body, false),
        StatementType::Foreach(statement) => contains_reachable_exit(&statement.body, false),
        StatementType::ConstructorDefinition(_)
        | StatementType::FunctionDefinition(_)
        | StatementType::ClassDefinition(_) => false,
        _ => false,
    }
}

fn always_exits(statement: &StatementType<'_>) -> bool {
    match statement {
        StatementType::Break(_)
        | StatementType::Continue(_)
        | StatementType::Return(_)
        | StatementType::Throw(_) => true,
        StatementType::Block(block) => block
            .statements
            .iter()
            .any(|statement| always_exits(&statement.ty)),
        StatementType::If(statement) => {
            match (&statement.ty, expression_truth(&statement.condition)) {
                (IfStatementType::NoElse { body }, Some(true)) => always_exits(body),
                (IfStatementType::Else { body, .. }, Some(true)) => always_exits(&body.ty),
                (IfStatementType::Else { else_body, .. }, Some(false)) => always_exits(else_body),
                (
                    IfStatementType::Else {
                        body, else_body, ..
                    },
                    None,
                ) => always_exits(&body.ty) && always_exits(else_body),
                _ => false,
            }
        }
        StatementType::While(statement) => {
            expression_truth(&statement.condition) == Some(true)
                && !contains_reachable_exit(&statement.body, true)
        }
        StatementType::DoWhile(statement) => {
            expression_truth(&statement.condition) == Some(true)
                && !contains_reachable_exit(&statement.body.ty, true)
        }
        StatementType::For(statement) => {
            statement
                .condition
                .as_ref()
                .is_none_or(|condition| expression_truth(condition) == Some(true))
                && !contains_reachable_exit(&statement.body, true)
        }
        _ => false,
    }
}

fn expression_truth(expression: &Expression<'_>) -> Option<bool> {
    match expression {
        Expression::Var(variable) => match variable.name.value {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        Expression::Literal(expression) => match expression.literal {
            LiteralToken::Int(value, _) => Some(value != 0),
            LiteralToken::Float(value) => Some(value != 0.0),
            _ => None,
        },
        Expression::Parens(expression) => expression_truth(&expression.value),
        Expression::Prefix(expression) => match expression.operator {
            sqparse::ast::PrefixOperator::LogicalNot(_) => {
                expression_truth(&expression.value).map(|value| !value)
            }
            _ => None,
        },
        _ => None,
    }
}

fn contains_reachable_wait(statement: &StatementType<'_>) -> bool {
    match statement {
        StatementType::Wait(_)
        | StatementType::WaitThread(_)
        | StatementType::WaitThreadSolo(_) => true,
        StatementType::Block(block) => statements_contain_reachable_wait(&block.statements),
        StatementType::If(statement) => {
            if expression_contains_wait(&statement.condition) {
                return true;
            }
            match (&statement.ty, expression_truth(&statement.condition)) {
                (IfStatementType::NoElse { body }, Some(true)) => contains_reachable_wait(body),
                (IfStatementType::NoElse { .. }, Some(false)) => false,
                (IfStatementType::NoElse { body }, None) => contains_reachable_wait(body),
                (IfStatementType::Else { body, .. }, Some(true)) => {
                    contains_reachable_wait(&body.ty)
                }
                (IfStatementType::Else { else_body, .. }, Some(false)) => {
                    contains_reachable_wait(else_body)
                }
                (
                    IfStatementType::Else {
                        body, else_body, ..
                    },
                    None,
                ) => contains_reachable_wait(&body.ty) || contains_reachable_wait(else_body),
            }
        }
        StatementType::While(statement) => {
            expression_contains_wait(&statement.condition)
                || expression_truth(&statement.condition) != Some(false)
                    && contains_reachable_wait(&statement.body)
        }
        StatementType::DoWhile(statement) => {
            contains_reachable_wait(&statement.body.ty)
                || expression_contains_wait(&statement.condition)
        }
        StatementType::Switch(statement) => {
            expression_contains_wait(&statement.condition)
                || statement
                    .cases
                    .iter()
                    .any(|case| statements_contain_reachable_wait(&case.body))
        }
        StatementType::For(statement) => {
            statement
                .condition
                .as_ref()
                .is_some_and(|condition| expression_contains_wait(condition))
                || statement
                    .condition
                    .as_ref()
                    .is_none_or(|condition| expression_truth(condition) != Some(false))
                    && contains_reachable_wait(&statement.body)
        }
        StatementType::Foreach(statement) => {
            expression_contains_wait(&statement.array) || contains_reachable_wait(&statement.body)
        }
        StatementType::Return(statement) => statement
            .value
            .as_ref()
            .is_some_and(|value| expression_contains_wait(value)),
        StatementType::Yield(statement) => statement
            .value
            .as_ref()
            .is_some_and(|value| expression_contains_wait(value)),
        StatementType::VarDefinition(statement) => var_definition_contains_wait(statement),
        StatementType::TryCatch(statement) => {
            contains_reachable_wait(&statement.body.ty)
                || contains_reachable_wait(&statement.catch_body)
        }
        StatementType::Throw(statement) => expression_contains_wait(&statement.value),
        StatementType::Const(statement) => expression_contains_wait(&statement.initializer.value),
        StatementType::Expression(statement) => expression_contains_wait(&statement.value),
        StatementType::Thread(statement) => expression_contains_wait(&statement.value),
        StatementType::DelayThread(statement) => {
            expression_contains_wait(&statement.duration)
                || expression_contains_wait(&statement.value)
        }
        _ => false,
    }
}

fn statements_contain_reachable_wait(statements: &[Statement<'_>]) -> bool {
    for statement in statements {
        if contains_reachable_wait(&statement.ty) {
            return true;
        }
        if always_exits(&statement.ty) {
            break;
        }
    }
    false
}

fn var_definition_contains_wait(definition: &VarDefinitionStatement<'_>) -> bool {
    definition.definitions.items.iter().any(|(definition, _)| {
        definition
            .initializer
            .as_ref()
            .is_some_and(|initializer| expression_contains_wait(&initializer.value))
    }) || definition
        .definitions
        .last_item
        .initializer
        .as_ref()
        .is_some_and(|initializer| expression_contains_wait(&initializer.value))
}

fn expression_contains_wait(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::Call(expression) => {
            called_expression_name(&expression.function)
                .is_some_and(|name| name.starts_with("Wait"))
                || expression_contains_wait(&expression.function)
                || expression
                    .arguments
                    .iter()
                    .any(|argument| expression_contains_wait(&argument.value))
        }
        Expression::Parens(expression) => expression_contains_wait(&expression.value),
        Expression::Index(expression) => {
            expression_contains_wait(&expression.base)
                || expression_contains_wait(&expression.index)
        }
        Expression::Property(expression) => expression_contains_wait(&expression.base),
        Expression::Ternary(expression) => {
            expression_contains_wait(&expression.condition)
                || expression_contains_wait(&expression.true_value)
                || expression_contains_wait(&expression.false_value)
        }
        Expression::Binary(expression) => {
            expression_contains_wait(&expression.left)
                || expression_contains_wait(&expression.right)
        }
        Expression::Prefix(expression) => expression_contains_wait(&expression.value),
        Expression::Postfix(expression) => expression_contains_wait(&expression.value),
        Expression::Comma(expression) => {
            expression
                .values
                .items
                .iter()
                .any(|(value, _)| expression_contains_wait(value))
                || expression_contains_wait(&expression.values.last_item)
        }
        Expression::Table(expression) => expression.slots.iter().any(|slot| match &slot.ty {
            TableSlotType::Slot(Slot::Property { initializer, .. }) => {
                expression_contains_wait(&initializer.value)
            }
            TableSlotType::Slot(Slot::ComputedProperty {
                name, initializer, ..
            }) => expression_contains_wait(name) || expression_contains_wait(&initializer.value),
            TableSlotType::JsonProperty { value, .. } => expression_contains_wait(value),
            TableSlotType::Slot(Slot::Constructor { .. } | Slot::Function { .. }) => false,
        }),
        Expression::Array(expression) => expression
            .values
            .iter()
            .any(|value| expression_contains_wait(&value.value)),
        Expression::Lambda(_) => false,
        Expression::Delegate(expression) => {
            expression_contains_wait(&expression.parent)
                || expression_contains_wait(&expression.value)
        }
        Expression::Vector(expression) => {
            expression_contains_wait(&expression.x)
                || expression_contains_wait(&expression.y)
                || expression_contains_wait(&expression.z)
        }
        Expression::Expect(expression) => expression_contains_wait(&expression.value),
        Expression::Literal(_)
        | Expression::Var(_)
        | Expression::RootVar(_)
        | Expression::Class(_)
        | Expression::Function(_) => false,
    }
}

fn called_function_name<'s>(expression: &Expression<'s>) -> Option<&'s str> {
    match expression {
        Expression::Call(call) => called_expression_name(&call.function),
        _ => None,
    }
}

fn called_expression_name<'s>(expression: &Expression<'s>) -> Option<&'s str> {
    match expression {
        Expression::Var(variable) => Some(variable.name.value),
        Expression::RootVar(variable) => Some(variable.name.value),
        Expression::Property(property) => match &property.property {
            sqparse::ast::MethodIdentifier::Identifier(identifier) => Some(identifier.value),
            sqparse::ast::MethodIdentifier::Constructor(_) => None,
        },
        Expression::Parens(expression) => called_expression_name(&expression.value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, LintOptions, analyze, diagnostics, diagnostics_with_options};

    fn lint(sources: &[&str]) -> Vec<String> {
        let analyses: Vec<_> = sources
            .iter()
            .map(|source| analyze(source).unwrap())
            .collect();
        diagnostics_with_options(&analyses, LintOptions { advisory: true })
            .into_iter()
            .flatten()
            .map(|diagnostic| diagnostic.message)
            .collect()
    }

    #[test]
    fn advisory_rules_are_opt_in() {
        let source = r#"
void function Poll( entity ent ) {
	while ( true ) {
		thread Update()
		WaitFrame()
		ent.Show()
	}
}
"#;
        let analyses = [analyze(source).unwrap()];

        assert!(diagnostics(&analyses)[0].is_empty());
        let diagnostics = diagnostics_with_options(&analyses, LintOptions { advisory: true })[0]
            .iter()
            .map(|diagnostic| (diagnostic.rule, &source[diagnostic.range.clone()]))
            .collect::<Vec<_>>();
        assert_eq!(
            diagnostics,
            vec![
                ("thread-spawned-inside-polling-loop", "thread"),
                ("entity-use-after-yield", "ent"),
            ]
        );
    }

    fn lint_rules(sources: &[&str]) -> Vec<&'static str> {
        let analyses: Vec<_> = sources
            .iter()
            .map(|source| analyze(source).unwrap())
            .collect();
        diagnostics_with_options(&analyses, LintOptions { advisory: true })
            .into_iter()
            .flatten()
            .map(|diagnostic| diagnostic.rule)
            .collect()
    }

    fn assert_diagnostics(
        sources: &[&str],
        options: LintOptions,
        expected: &[(&str, usize, &str)],
    ) {
        let analyses: Vec<_> = sources
            .iter()
            .map(|source| analyze(source).unwrap())
            .collect();
        let actual = diagnostics_with_options(&analyses, options)
            .into_iter()
            .enumerate()
            .flat_map(|(file, diagnostics)| {
                diagnostics.into_iter().map(move |diagnostic: Diagnostic| {
                    (diagnostic.rule, file, &sources[file][diagnostic.range])
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn trailing_nolint_suppresses_named_rules_on_its_line() {
        let rules = lint_rules(&[r#"
void function Poll() {
	wait 0 // nolint: unsafe-array-index, wait-zero
	wait 0 // nolint: unsafe-array-index
}
"#]);

        assert_eq!(rules, vec!["wait-zero"]);
    }

    #[test]
    fn standalone_nolint_suppresses_the_next_code_line() {
        let rules = lint_rules(&[r#"
void function Poll() {
	// nolint: wait-zero
	// This explanation is not the suppressed line.

	wait 0
	wait 0
}
"#]);

        assert_eq!(rules, vec!["wait-zero"]);
    }

    #[test]
    fn standalone_nolint_skips_block_comment_lines() {
        let rules = lint_rules(&[r#"
void function Poll() {
	// nolint: wait-zero
	/* The caller intentionally wants no frame delay.
	 * Keep this explanation beside the exception. */
	wait 0
	wait 0
}
"#]);

        assert_eq!(rules, vec!["wait-zero"]);
    }

    #[test]
    fn reports_infinite_loop_without_wait_in_threaded_function() {
        let source = r#"
thread Poll()

void function Poll() {
	while ( true ) {
		DoWork()
	}
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("threaded-loop-without-wait", 0, "while")],
        );
    }

    #[test]
    fn accepts_threaded_loop_with_end_condition() {
        let messages = lint(&[r#"
thread Poll()

void function Poll() {
	while ( !IsFinished() ) {
		DoWork()
	}
}
"#]);

        assert!(messages.is_empty());
    }

    #[test]
    fn accepts_infinite_threaded_loop_with_reachable_break() {
        let messages = lint(&[r#"
thread Poll()

void function Poll() {
	for ( ;; ) {
		if ( IsFinished() )
			break
		DoWork()
	}
}
"#]);

        assert!(messages.is_empty());
    }

    #[test]
    fn accepts_infinite_threaded_loop_with_reachable_return() {
        let messages = lint(&[r#"
thread Poll()

void function Poll() {
	while ( true ) {
		if ( IsFinished() )
			return
		DoWork()
	}
}
"#]);

        assert!(messages.is_empty());
    }

    #[test]
    fn rejects_infinite_threaded_loop_with_unreachable_break() {
        let source = r#"
thread Poll()

void function Poll() {
	while ( 1 ) {
		continue
		break
	}
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("threaded-loop-without-wait", 0, "while")],
        );
    }

    #[test]
    fn rejects_break_in_constant_false_branch() {
        let source = r#"
thread Poll()

void function Poll() {
	do {
		if ( false )
			break
		DoWork()
	} while ( true )
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("threaded-loop-without-wait", 0, "do")],
        );
    }

    #[test]
    fn rejects_unreachable_wait() {
        let source = r#"
thread Poll()

void function Poll() {
	while ( true ) {
		continue
		WaitFrame()
	}
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("threaded-loop-without-wait", 0, "while")],
        );
    }

    #[test]
    fn rejects_break_after_nested_infinite_loop() {
        let source = r#"
thread Poll()

void function Poll() {
	while ( true ) {
		while ( true ) {
			DoWork()
		}
		break
	}
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[
                ("threaded-loop-without-wait", 0, "while"),
                ("threaded-loop-without-wait", 0, "while"),
            ],
        );
    }

    #[test]
    fn accepts_threaded_loop_with_wait_call() {
        let messages = lint(&[r#"
thread Poll()

void function Poll() {
	while ( true ) {
		DoWork()
		WaitFrame()
	}
}
"#]);

        assert!(messages.is_empty());
    }

    #[test]
    fn accepts_threaded_loop_with_wait_statement() {
        let messages = lint(&[r#"
thread Poll()

void function Poll() {
	while ( true ) {
		wait 0.1
	}
}
"#]);

        assert!(messages.is_empty());
    }

    #[test]
    fn ignores_non_threaded_function() {
        let messages = lint(&[r#"
void function Poll() {
	while ( true ) {
		DoWork()
	}
}
"#]);

        assert!(messages.is_empty());
    }

    #[test]
    fn matches_thread_call_across_files() {
        let sources = [
            "thread Poll()\n",
            r#"
void function Poll() {
	for ( ;; ) {
		DoWork()
	}
}
"#,
        ];
        assert_diagnostics(
            &sources,
            LintOptions { advisory: true },
            &[("threaded-loop-without-wait", 1, "for")],
        );
    }

    #[test]
    fn wait_in_nested_function_does_not_count() {
        let source = r#"
thread Poll()

void function Poll() {
	while ( true ) {
		void function Later() {
			WaitFrame()
		}
	}
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("threaded-loop-without-wait", 0, "while")],
        );
    }

    #[test]
    fn reports_wait_zero() {
        let source = "void function Poll() { wait 0 }";
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("wait-zero", 0, "0")],
        );
    }

    #[test]
    fn reports_unchecked_encoded_ehandle() {
        let source = r#"
void function Show( int handle ) {
	entity ent = GetEntityFromEncodedEHandle( handle )
	ent.Show()
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("unchecked-encoded-ehandle", 0, "ent")],
        );
    }

    #[test]
    fn accepts_encoded_ehandle_after_validity_guard() {
        let rules = lint_rules(&[r#"
void function Show( int handle ) {
	entity ent = GetEntityFromEncodedEHandle( handle )
	if ( !IsValid( ent ) )
		return
	ent.Show()
}
"#]);

        assert!(!rules.contains(&"unchecked-encoded-ehandle"));
    }

    #[test]
    fn ignores_entity_that_is_only_possibly_invalid() {
        let rules = lint_rules(&[r#"
void function Show( entity ornull ent ) {
	if ( ent != null )
		ent.Show()
}
"#]);

        assert!(!rules.contains(&"invalid-entity-use"));
    }

    #[test]
    fn reports_entity_known_to_be_invalid() {
        let source = r#"
void function Show() {
	entity ornull ent = null
	ent.Show()
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("invalid-entity-use", 0, "ent")],
        );
    }

    #[test]
    fn reports_entity_in_failed_validity_branch() {
        let source = r#"
void function Show( entity ornull ent ) {
	if ( !IsValid( ent ) )
		ent.Show()
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("invalid-entity-use", 0, "ent")],
        );
    }

    #[test]
    fn dead_entity_is_not_assumed_invalid() {
        let rules = lint_rules(&[r#"
void function Respawn( entity player ) {
	if ( !IsAlive( player ) )
		player.RespawnPlayer( null )
}
"#]);

        assert!(!rules.contains(&"invalid-entity-use"));
    }

    #[test]
    fn short_circuit_truthiness_does_not_report_unreachable_null_use() {
        let rules = lint_rules(&[r#"
entity function Select( array<entity> players ) {
	entity selected
	foreach ( player in players ) {
		if ( !selected || player.GetTeam() > selected.GetTeam() )
			selected = player
	}
	return selected
}
"#]);

        assert!(!rules.contains(&"invalid-entity-use"));
    }

    #[test]
    fn reports_entity_use_after_destroy() {
        let source = r#"
void function Show( entity ent ) {
	ent.Destroy()
	ent.Show()
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("invalid-entity-use", 0, "ent")],
        );
    }

    #[test]
    fn reports_entity_use_after_yield() {
        let source = r#"
void function Show( entity ent ) {
	WaitFrame()
	ent.Show()
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("entity-use-after-yield", 0, "ent")],
        );
    }

    #[test]
    fn accepts_entity_use_after_yield_with_destroy_signal() {
        let rules = lint_rules(&[r#"
void function Show( entity ent ) {
	ent.EndSignal( "OnDestroy" )
	WaitFrame()
	ent.Show()
}
"#]);

        assert!(!rules.contains(&"entity-use-after-yield"));
    }

    #[test]
    fn reports_find_result_used_as_array_index() {
        let source = r#"
string function Lookup( array<string> values, string wanted ) {
	int index = values.find( wanted )
	return values[index]
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("unsafe-array-index", 0, "[index]")],
        );
    }

    #[test]
    fn accepts_checked_find_result_as_array_index() {
        let rules = lint_rules(&[r#"
string function Lookup( array<string> values, string wanted ) {
	int index = values.find( wanted )
	if ( index == -1 )
		return ""
	return values[index]
}
"#]);

        assert!(!rules.contains(&"unsafe-array-index"));
    }

    #[test]
    fn reports_find_used_as_boolean() {
        let source = r#"
bool function Contains( array<string> values, string wanted ) {
	if ( values.find( wanted ) )
		return true
	return false
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("find-used-as-boolean", 0, "values.find( wanted )")],
        );
    }

    #[test]
    fn reports_thread_spawned_inside_polling_loop() {
        let source = r#"
void function Poll() {
	while ( true ) {
		thread Update()
		WaitFrame()
	}
}
"#;
        assert_diagnostics(
            &[source],
            LintOptions { advisory: true },
            &[("thread-spawned-inside-polling-loop", 0, "thread")],
        );
    }

    #[test]
    fn reports_custom_signal_used_without_registration() {
        let sources = [
            r#"void function Send( entity ent ) { Signal( ent, "CustomDone" ) }"#,
            r#"void function Receive( entity ent ) { WaitSignal( ent, "CustomDone" ) }"#,
        ];
        assert_diagnostics(
            &sources,
            LintOptions { advisory: true },
            &[
                ("unregistered-signal", 0, "\"CustomDone\""),
                ("unregistered-signal", 1, "\"CustomDone\""),
            ],
        );
    }

    #[test]
    fn accepts_registered_custom_signal() {
        let rules = lint_rules(&[
            r#"void function Init() { RegisterSignal( "CustomDone" ) }"#,
            r#"void function Send( entity ent ) { Signal( ent, "CustomDone" ) }"#,
            r#"void function Receive( entity ent ) { WaitSignal( ent, "CustomDone" ) }"#,
        ]);

        assert!(!rules.contains(&"unregistered-signal"));
    }

    #[test]
    fn reports_remote_function_argument_mismatch() {
        let sources = [
            r#"
void function Init() {
	Remote_RegisterFunction( "RemoteMessage" )
}

void function RemoteMessage( int value ) {}
"#,
            r#"
void function Send( entity player ) {
	Remote_CallFunction_NonReplay( player, "RemoteMessage", 1, 2 )
}
"#,
        ];
        assert_diagnostics(
            &sources,
            LintOptions { advisory: true },
            &[("remote-function-contract-mismatch", 1, "\"RemoteMessage\"")],
        );
    }

    #[test]
    fn reports_unresolved_manifest_callback() {
        let analysis = analyze("void function Present() {}").unwrap();
        let workspace = super::Workspace::new([&analysis]);
        let manifest = r#"{
  "Scripts": [{
    "Path": "client/example.gnut",
    "ClientCallback": { "Before": "Present", "After": "Missing" }
  }]
}"#;
        let diagnostics = workspace.manifest_diagnostics(manifest);

        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.rule, &manifest[diagnostic.range.clone()]))
                .collect::<Vec<_>>(),
            vec![("unresolved-manifest-callback", "\"Missing\"")]
        );
    }

    #[test]
    fn ignores_engine_provided_signal() {
        let rules = lint_rules(&[
            r#"void function Send( entity ent ) { Signal( ent, "OnDestroy" ) }"#,
            r#"void function Receive( entity ent ) { EndSignal( ent, "OnDestroy" ) }"#,
        ]);

        assert!(!rules.contains(&"unregistered-signal"));
    }
}
