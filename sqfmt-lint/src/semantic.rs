use std::collections::{HashMap, HashSet};
use std::ops::Range;

use sqparse::ast::*;
use sqparse::token::{LiteralToken, StringToken, Token};

use crate::conditional::{
    ConditionalSpan, VmTargets, branch_spans, branches_nested, conditional_spans, targets_at,
};

#[derive(Clone, Debug)]
pub struct OwnedDeclaration {
    pub name: String,
    pub range: Range<usize>,
    /// Exported to the global namespace with `global` or `globalize_all_functions`.
    pub is_global: bool,
    /// Exported by `globalize_all_functions` rather than named in a `global` statement. Such a file
    /// writes every function it declares into the root table, which is how a mod replaces a function
    /// the game already provides, so the name may well be declared outside the workspace too.
    pub implicitly_global: bool,
    /// Declared as a slot on another table, such as `function Class::Method()`. The name is bound
    /// lexically as well, but a bare call never reaches the slot.
    pub namespaced: bool,
    /// Declared at file scope, whether or not it is exported.
    pub file_scope: bool,
    /// The VMs this declaration's conditional compilation region can run in.
    pub targets: VmTargets,
    pub kind: DeclarationKind,
    pub detail: String,
    pub declared_type: Option<String>,
    pub type_identity: Option<TypeIdentity>,
    pub return_type: Option<String>,
    pub type_object: Option<String>,
    pub owner: Option<TypeIdentity>,
    pub base_type: Option<String>,
    pub initializer_source: Option<ValueSource>,
    pub signature: Option<OwnedSignature>,
    pub return_signature: Option<OwnedSignature>,
    /// Values returned by this function, used when it declares no return type.
    pub returns: Vec<OwnedReturn>,
    pub available_from: Option<usize>,
    visibility: Range<usize>,
    scope_depth: usize,
    in_scope: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TypeIdentity {
    Nominal(String),
    Structural(Range<usize>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedSignature {
    pub label: String,
    pub parameters: Vec<OwnedParameter>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedParameter {
    pub label: String,
    pub variadic: bool,
    /// Declared with a default value, so a call may leave it out.
    pub optional: bool,
    pub type_identity: Option<TypeIdentity>,
}

/// A value a `return` statement produces, and the range of the expression that produced it.
#[derive(Clone, Debug)]
pub struct OwnedReturn {
    pub range: Range<usize>,
    pub source: ValueSource,
}

#[derive(Clone, Debug)]
pub struct OwnedCall {
    pub open: Range<usize>,
    pub close: Range<usize>,
    pub callable: ValueSource,
    pub commas: Vec<Range<usize>>,
    pub arguments: Vec<OwnedArgument>,
}

#[derive(Clone, Debug)]
pub struct OwnedArgument {
    pub range: Range<usize>,
    pub source: ValueSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueSource {
    Unknown,
    Declaration(Range<usize>),
    Workspace(String),
    Type(TypeIdentity),
    /// A function or lambda expression, identified by its own source range.
    Callable(Range<usize>),
    Call(Box<ValueSource>),
    /// A value carrying extra per-instance slots, such as a call with a post-initializer. `extra`
    /// is the structural identity holding those slots.
    Augmented {
        base: Box<ValueSource>,
        extra: Range<usize>,
    },
    Member {
        receiver: Box<ValueSource>,
        name: String,
    },
}

#[derive(Clone, Debug)]
pub struct OwnedMemberReference {
    pub name: String,
    pub range: Range<usize>,
    pub receiver: ValueSource,
    pub available: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclarationKind {
    Function,
    Constructor,
    Class,
    Constant,
    Enum,
    Struct,
    Type,
    Variable,
    Parameter,
    Field,
    Method,
}

#[derive(Clone, Debug)]
pub struct OwnedReference {
    pub name: String,
    pub range: Range<usize>,
    pub target: Option<Range<usize>>,
}

/// A declaration that reuses a name an enclosing lexical scope already bound at the same depth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedDuplicate {
    pub name: String,
    pub range: Range<usize>,
    pub kind: DeclarationKind,
    /// The declaration this one shadows in the same scope.
    pub previous: Range<usize>,
    pub previous_kind: DeclarationKind,
}

#[derive(Clone, Debug, Default)]
pub struct SemanticDocument {
    pub declarations: Vec<OwnedDeclaration>,
    pub references: Vec<OwnedReference>,
    pub member_references: Vec<OwnedMemberReference>,
    pub calls: Vec<OwnedCall>,
    pub callables: HashMap<Range<usize>, OwnedSignature>,
    pub conditions: Vec<ConditionalSpan>,
    pub duplicates: Vec<OwnedDuplicate>,
    /// Indices into `declarations`, built once when analysis finishes. A workspace-wide lookup
    /// visits every indexed file, so scanning each file's declarations there is quadratic in the
    /// project; these turn the inner scan into a hash lookup.
    by_name: HashMap<String, Vec<usize>>,
    by_owner: HashMap<TypeIdentity, Vec<usize>>,
}

impl SemanticDocument {
    /// The declarations binding this name, in source order.
    pub fn declarations_named<'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = &'a OwnedDeclaration> {
        self.by_name
            .get(name)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|index| &self.declarations[*index])
    }

    /// The declarations this type owns directly, in source order.
    pub fn declarations_owned_by<'a>(
        &'a self,
        owner: &TypeIdentity,
    ) -> impl Iterator<Item = &'a OwnedDeclaration> {
        self.by_owner
            .get(owner)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|index| &self.declarations[*index])
    }

    fn build_indices(&mut self) {
        for (index, declaration) in self.declarations.iter().enumerate() {
            self.by_name
                .entry(declaration.name.clone())
                .or_default()
                .push(index);
            if let Some(owner) = &declaration.owner {
                self.by_owner.entry(owner.clone()).or_default().push(index);
            }
        }
    }
}

impl SemanticDocument {
    /// The VMs code at this offset can run in.
    pub fn targets_at(&self, offset: usize) -> VmTargets {
        targets_at(&self.conditions, offset)
    }
}

impl SemanticDocument {
    pub fn reference_at(&self, offset: usize) -> Option<&OwnedReference> {
        self.references
            .iter()
            .find(|reference| contains_offset(&reference.range, offset))
    }

    pub fn declaration_at(&self, offset: usize) -> Option<&OwnedDeclaration> {
        self.declarations
            .iter()
            .find(|declaration| contains_offset(&declaration.range, offset))
    }

    pub fn declaration_for_range(&self, range: &Range<usize>) -> Option<&OwnedDeclaration> {
        self.declarations
            .iter()
            .find(|declaration| declaration.range == *range)
    }

    pub fn member_reference_at(&self, offset: usize) -> Option<&OwnedMemberReference> {
        self.member_references
            .iter()
            .find(|reference| reference.available && contains_offset(&reference.range, offset))
    }

    pub fn call_at(&self, offset: usize) -> Option<&OwnedCall> {
        self.calls
            .iter()
            .filter(|call| call.open.end <= offset && offset <= call.close.start)
            .max_by_key(|call| call.open.start)
    }

    pub fn local_references(&self, declaration: &Range<usize>) -> Vec<Range<usize>> {
        self.references
            .iter()
            .filter(|reference| reference.target.as_ref() == Some(declaration))
            .map(|reference| reference.range.clone())
            .collect()
    }

    pub fn global_references(&self, name: &str) -> Vec<Range<usize>> {
        self.references
            .iter()
            .filter(|reference| {
                if reference.name != name {
                    return false;
                }
                match &reference.target {
                    None => true,
                    Some(target) => self
                        .declaration_for_range(target)
                        .is_some_and(|declaration| declaration.is_global),
                }
            })
            .map(|reference| reference.range.clone())
            .collect()
    }

    pub fn visible_declarations(&self, offset: usize) -> Vec<&OwnedDeclaration> {
        let mut declarations = self
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.in_scope
                    && declaration.visibility.start <= offset
                    && offset <= declaration.visibility.end
                    && (declaration.kind != DeclarationKind::Variable
                        || declaration.range.start <= offset)
            })
            .collect::<Vec<_>>();
        declarations.sort_by(|left, right| {
            right
                .scope_depth
                .cmp(&left.scope_depth)
                .then_with(|| (right.range.start <= offset).cmp(&(left.range.start <= offset)))
                .then_with(|| right.range.start.cmp(&left.range.start))
                .then_with(|| left.name.cmp(&right.name))
        });
        declarations
    }
}

pub fn analyze(source: &str) -> SemanticDocument {
    let Ok(tokenization) =
        sqparse::tokenize_partial_with_error_limit(source, sqparse::Flavor::SquirrelRespawn, 0)
    else {
        return SemanticDocument::default();
    };
    let parses = tokenization
        .regions
        .iter()
        .map(|region| sqparse::parse_partial(&region.tokens, sqparse::Flavor::SquirrelRespawn))
        .collect::<Vec<_>>();
    let statements = tokenization
        .regions
        .iter()
        .zip(&parses)
        .flat_map(|(region, partial)| {
            partial
                .statements
                .iter()
                .filter(|parsed| region.is_trusted_statement_end(parsed.token_range.end))
                .map(|parsed| &parsed.statement)
        })
        .collect::<Vec<_>>();
    analyze_statements(source, &statements)
}

/// Analyzes statements a caller already recovered, so one tokenization and parse can serve
/// diagnostics, symbols, semantics, and tokens together.
pub fn analyze_statements(source: &str, statements: &[&Statement<'_>]) -> SemanticDocument {
    let statements = statements.to_vec();
    let mut analyzer = Analyzer {
        document: SemanticDocument::default(),
        scopes: vec![Scope {
            declarations: HashMap::new(),
            range: 0..source.len(),
        }],
        exports: collect_exports(&statements),
        owners: Vec::new(),
        return_targets: Vec::new(),
        flow: FlowState::default(),
        definite_flow: true,
    };
    for statement in &statements {
        analyzer.predeclare_statement_type(&statement.ty, true);
    }
    for statement in statements {
        analyzer.statement(statement, true);
    }

    let mut document = analyzer.document;
    document.conditions = conditional_spans(source);
    if !document.conditions.is_empty() {
        for declaration in &mut document.declarations {
            declaration.targets = targets_at(&document.conditions, declaration.range.start);
        }
        // Declarations in branches that cannot run in the same VM never coexist.
        let conditions = std::mem::take(&mut document.conditions);
        document.duplicates.retain(|duplicate| {
            targets_at(&conditions, duplicate.previous.start)
                .compatible_with(targets_at(&conditions, duplicate.range.start))
        });
        document.conditions = conditions;
    }
    if !document.duplicates.is_empty() {
        // Conditions such as `#if SP` say nothing about the VM, so target intersection cannot tell
        // whether their branches compile together. Only compare declarations one of whose guards
        // contains the other's.
        let branches = branch_spans(source);
        document.duplicates.retain(|duplicate| {
            branches_nested(&branches, duplicate.previous.start, duplicate.range.start)
        });
    }
    document.build_indices();
    document
}

/// What a file publishes to the global namespace.
#[derive(Default)]
struct Exports {
    names: HashSet<String>,
    all_functions: bool,
}

impl Exports {
    fn exports(&self, name: &str, kind: DeclarationKind) -> bool {
        self.names.contains(name) || self.exports_implicitly(name, kind)
    }

    /// Exported by `globalize_all_functions` without being named in a `global` statement.
    fn exports_implicitly(&self, name: &str, kind: DeclarationKind) -> bool {
        !self.names.contains(name)
            && self.all_functions
            && matches!(kind, DeclarationKind::Function)
    }
}

struct Analyzer {
    document: SemanticDocument,
    exports: Exports,
    scopes: Vec<Scope>,
    owners: Vec<TypeIdentity>,
    /// The declaration each enclosing function body returns into, innermost last.
    return_targets: Vec<Option<Range<usize>>>,
    flow: FlowState,
    definite_flow: bool,
}

#[derive(Clone, Debug, Default)]
struct FlowState {
    locals: HashMap<Range<usize>, ValueSource>,
    members: HashMap<(Range<usize>, String), ValueSource>,
    insertions: HashMap<(Range<usize>, String), PendingInsertion>,
}

/// A structural slot inserted on a conditional path. It only becomes a declaration once every
/// joined path inserts it.
#[derive(Clone, Debug)]
struct PendingInsertion {
    sites: Vec<(Range<usize>, ValueSource)>,
    source: ValueSource,
}

impl FlowState {
    fn join(left: &Self, right: &Self) -> Self {
        let mut locals = HashMap::new();
        for range in left.locals.keys().chain(right.locals.keys()) {
            let value = match (left.locals.get(range), right.locals.get(range)) {
                (Some(left), Some(right)) if left == right => left.clone(),
                _ => ValueSource::Unknown,
            };
            locals.insert(range.clone(), value);
        }
        let members = left
            .members
            .iter()
            .filter_map(|(key, left)| {
                right.members.get(key).map(|right| {
                    (
                        key.clone(),
                        if left == right {
                            left.clone()
                        } else {
                            ValueSource::Unknown
                        },
                    )
                })
            })
            .collect();
        let insertions = left
            .insertions
            .iter()
            .filter_map(|(key, left)| {
                right.insertions.get(key).map(|right| {
                    let mut sites = left.sites.clone();
                    for site in &right.sites {
                        if !sites.iter().any(|(range, _)| *range == site.0) {
                            sites.push(site.clone());
                        }
                    }
                    let source = if left.source == right.source {
                        left.source.clone()
                    } else {
                        ValueSource::Unknown
                    };
                    (key.clone(), PendingInsertion { sites, source })
                })
            })
            .collect();
        Self {
            locals,
            members,
            insertions,
        }
    }

    fn unknowned(&self) -> Self {
        Self {
            locals: self
                .locals
                .keys()
                .cloned()
                .map(|range| (range, ValueSource::Unknown))
                .collect(),
            members: self
                .members
                .keys()
                .cloned()
                .map(|key| (key, ValueSource::Unknown))
                .collect(),
            insertions: self
                .insertions
                .iter()
                .map(|(key, insertion)| {
                    (
                        key.clone(),
                        PendingInsertion {
                            sites: insertion.sites.clone(),
                            source: ValueSource::Unknown,
                        },
                    )
                })
                .collect(),
        }
    }
}

enum AssignmentTarget {
    Local(Range<usize>),
    StructuralMember {
        owner: Range<usize>,
        name: String,
        range: Range<usize>,
        receiver: ValueSource,
        exists: bool,
    },
    Other,
}

struct Scope {
    declarations: HashMap<String, Range<usize>>,
    range: Range<usize>,
}

impl Analyzer {
    fn predeclare_statements(&mut self, statements: &[Statement<'_>], global: bool) {
        for statement in statements {
            self.predeclare_statement_type(&statement.ty, global);
        }
    }

    fn predeclare_statement_type(&mut self, statement: &StatementType<'_>, global: bool) {
        match statement {
            StatementType::FunctionDefinition(statement) => {
                self.declare_binding(
                    statement.name.last_item.value,
                    statement.name.last_item.token,
                    global,
                    DeclarationKind::Function,
                    function_detail(
                        statement.name.last_item.value,
                        statement.return_type.as_ref(),
                        &statement.definition,
                    ),
                    statement.name.items.is_empty(),
                );
                self.declaration_mut(&statement.name.last_item.token.range)
                    .return_type = statement.return_type.as_ref().map(display_nominal_type);
                self.declaration_mut(&statement.name.last_item.token.range)
                    .return_signature = statement
                    .return_type
                    .as_ref()
                    .and_then(|type_| functionref_signature(None, type_));
                self.declaration_mut(&statement.name.last_item.token.range)
                    .signature = Some(function_signature(
                    statement.name.last_item.value,
                    statement.return_type.as_ref(),
                    &statement.definition,
                ));
            }
            StatementType::ConstructorDefinition(statement) => {
                // `function Class::constructor()` names the class, not a new binding.
                self.declare_binding(
                    statement.last_name.value,
                    statement.last_name.token,
                    global,
                    DeclarationKind::Constructor,
                    function_detail("constructor", None, &statement.definition),
                    false,
                );
                self.declaration_mut(&statement.last_name.token.range)
                    .signature = Some(function_signature(
                    "constructor",
                    None,
                    &statement.definition,
                ));
            }
            StatementType::ClassDefinition(statement) => {
                if let Some(name) = expression_identifier(&statement.name) {
                    self.declare(
                        name.value,
                        name.token,
                        global,
                        DeclarationKind::Class,
                        format!("class {}", name.value),
                    );
                    self.declaration_mut(&name.token.range).type_object =
                        Some(name.value.to_string());
                    self.declaration_mut(&name.token.range).base_type = statement
                        .definition
                        .extends
                        .as_ref()
                        .and_then(|extends| expression_identifier(&extends.name))
                        .map(|base| base.value.to_string());
                }
            }
            StatementType::Const(statement) => {
                self.declare(
                    statement.name.value,
                    statement.name.token,
                    global,
                    DeclarationKind::Constant,
                    const_detail(statement),
                );
            }
            StatementType::EnumDefinition(statement) => {
                self.declare(
                    statement.name.value,
                    statement.name.token,
                    global,
                    DeclarationKind::Enum,
                    format!("enum {}", statement.name.value),
                );
            }
            StatementType::StructDefinition(statement) => {
                self.declare(
                    statement.name.value,
                    statement.name.token,
                    global,
                    DeclarationKind::Struct,
                    format!("struct {}", statement.name.value),
                );
                self.declaration_mut(&statement.name.token.range)
                    .type_object = Some(statement.name.value.to_string());
            }
            StatementType::TypeDefinition(statement) => {
                self.declare(
                    statement.name.value,
                    statement.name.token,
                    global,
                    DeclarationKind::Type,
                    format!(
                        "typedef {} {}",
                        statement.name.value,
                        display_type(&statement.type_)
                    ),
                );
                self.declaration_mut(&statement.name.token.range)
                    .declared_type = Some(display_nominal_type(&statement.type_));
                self.declaration_mut(&statement.name.token.range)
                    .type_identity = type_identity(&statement.type_);
            }
            StatementType::Global(statement) => self.predeclare_global(&statement.definition),
            _ => {}
        }
    }

    fn predeclare_global(&mut self, definition: &GlobalDefinition<'_>) {
        match definition {
            GlobalDefinition::Function { name, .. } => {
                self.declare(
                    name.value,
                    name.token,
                    true,
                    DeclarationKind::Function,
                    format!("global function {}", name.value),
                );
            }
            GlobalDefinition::UntypedVar { name, .. } => {
                self.declare(
                    name.value,
                    name.token,
                    true,
                    DeclarationKind::Variable,
                    format!("global {}", name.value),
                );
            }
            GlobalDefinition::Const(statement) => {
                self.declare(
                    statement.name.value,
                    statement.name.token,
                    true,
                    DeclarationKind::Constant,
                    const_detail(statement),
                );
            }
            GlobalDefinition::Enum(statement) => {
                self.declare(
                    statement.name.value,
                    statement.name.token,
                    true,
                    DeclarationKind::Enum,
                    format!("global enum {}", statement.name.value),
                );
            }
            GlobalDefinition::Class(statement) => {
                if let Some(name) = expression_identifier(&statement.name) {
                    self.declare(
                        name.value,
                        name.token,
                        true,
                        DeclarationKind::Class,
                        format!("global class {}", name.value),
                    );
                    self.declaration_mut(&name.token.range).type_object =
                        Some(name.value.to_string());
                    self.declaration_mut(&name.token.range).base_type = statement
                        .definition
                        .extends
                        .as_ref()
                        .and_then(|extends| expression_identifier(&extends.name))
                        .map(|base| base.value.to_string());
                }
            }
            GlobalDefinition::Struct(statement) => {
                self.declare(
                    statement.name.value,
                    statement.name.token,
                    true,
                    DeclarationKind::Struct,
                    format!("global struct {}", statement.name.value),
                );
                self.declaration_mut(&statement.name.token.range)
                    .type_object = Some(statement.name.value.to_string());
            }
            GlobalDefinition::Type(statement) => {
                self.declare(
                    statement.name.value,
                    statement.name.token,
                    true,
                    DeclarationKind::Type,
                    format!(
                        "global typedef {} {}",
                        statement.name.value,
                        display_type(&statement.type_)
                    ),
                );
                self.declaration_mut(&statement.name.token.range)
                    .declared_type = Some(display_nominal_type(&statement.type_));
                self.declaration_mut(&statement.name.token.range)
                    .type_identity = type_identity(&statement.type_);
            }
            GlobalDefinition::TypedVar(_) => {}
        }
    }

    fn statement(&mut self, statement: &Statement<'_>, global: bool) {
        self.statement_type(&statement.ty, global);
    }

    fn statement_type(&mut self, statement: &StatementType<'_>, global: bool) {
        match statement {
            StatementType::Empty(_)
            | StatementType::Break(_)
            | StatementType::Continue(_)
            | StatementType::GlobalizeAllFunctions(_)
            | StatementType::Untyped(_) => {}
            StatementType::Block(block) => self.block(
                &block.statements,
                block.open.range.start..block.close.range.end,
            ),
            StatementType::If(statement) => {
                self.expression(&statement.condition);
                let incoming = self.flow.clone();
                let was_definite = self.definite_flow;
                self.definite_flow = false;
                let end = match &statement.ty {
                    IfStatementType::NoElse { body } => {
                        self.scoped_statement_type(body);
                        self.flow = FlowState::join(&incoming, &self.flow);
                        statement_type_end(body)
                    }
                    IfStatementType::Else {
                        body, else_body, ..
                    } => {
                        self.scoped_statement(body);
                        let then_flow = self.flow.clone();
                        self.flow = incoming;
                        self.scoped_statement_type(else_body);
                        self.flow = FlowState::join(&then_flow, &self.flow);
                        statement_type_end(else_body)
                    }
                };
                self.definite_flow = was_definite;
                self.commit_insertions(end);
            }
            StatementType::While(statement) => {
                let incoming = self.flow.unknowned();
                let was_definite = self.definite_flow;
                self.flow = incoming.clone();
                self.definite_flow = false;
                self.expression(&statement.condition);
                self.scoped_statement_type(&statement.body);
                self.flow = incoming;
                self.definite_flow = was_definite;
            }
            StatementType::DoWhile(statement) => {
                let incoming = self.flow.unknowned();
                let was_definite = self.definite_flow;
                self.flow = incoming.clone();
                self.definite_flow = false;
                self.scoped_statement(&statement.body);
                self.expression(&statement.condition);
                self.flow = incoming;
                self.definite_flow = was_definite;
            }
            StatementType::Switch(statement) => {
                self.expression(&statement.condition);
                let incoming = self.flow.unknowned();
                let was_definite = self.definite_flow;
                self.flow = incoming.clone();
                self.definite_flow = false;
                for (index, case) in statement.cases.iter().enumerate() {
                    self.flow = incoming.clone();
                    if let SwitchCaseCondition::Case { value, .. } = &case.condition {
                        self.expression(value);
                    }
                    let end = statement
                        .cases
                        .get(index + 1)
                        .map_or(statement.close_cases.range.start, switch_case_start);
                    self.block(&case.body, case.colon.range.end..end);
                }
                self.flow = incoming;
                self.definite_flow = was_definite;
            }
            StatementType::For(statement) => {
                self.push_scope(statement.for_.range.start..statement_type_end(&statement.body));
                if let Some(initializer) = &statement.initializer {
                    match initializer {
                        ForDefinition::Expression(expression) => {
                            self.expression(expression);
                        }
                        ForDefinition::Definition(definition) => {
                            self.var_definition(definition, false)
                        }
                    }
                }
                let incoming = self.flow.unknowned();
                let was_definite = self.definite_flow;
                self.flow = incoming.clone();
                self.definite_flow = false;
                if let Some(condition) = &statement.condition {
                    self.expression(condition);
                }
                self.statement_type(&statement.body, false);
                if let Some(increment) = &statement.increment {
                    self.expression(increment);
                }
                self.flow = incoming;
                self.definite_flow = was_definite;
                self.pop_scope();
            }
            StatementType::Foreach(statement) => {
                self.expression(&statement.array);
                let incoming = self.flow.unknowned();
                let was_definite = self.definite_flow;
                self.flow = incoming.clone();
                self.definite_flow = false;
                self.push_scope(statement.foreach.range.start..statement_type_end(&statement.body));
                if let Some(index) = &statement.index {
                    if let Some(type_) = &index.type_ {
                        self.type_(type_);
                    }
                    self.declare(
                        index.name.value,
                        index.name.token,
                        false,
                        DeclarationKind::Variable,
                        variable_detail(index.name.value, index.type_.as_ref()),
                    );
                    self.record_declared_type(&index.name.token.range, index.type_.as_ref());
                }
                if let Some(type_) = &statement.value_type {
                    self.type_(type_);
                }
                self.declare(
                    statement.value_name.value,
                    statement.value_name.token,
                    false,
                    DeclarationKind::Variable,
                    variable_detail(statement.value_name.value, statement.value_type.as_ref()),
                );
                self.record_declared_type(
                    &statement.value_name.token.range,
                    statement.value_type.as_ref(),
                );
                self.statement_type(&statement.body, false);
                self.pop_scope();
                self.flow = incoming;
                self.definite_flow = was_definite;
            }
            StatementType::Return(statement) => {
                if let Some(value) = &statement.value {
                    let source = self.expression(value);
                    self.record_return_source(
                        expression_start(value)..expression_end(value),
                        source,
                    );
                }
            }
            StatementType::Yield(statement) => {
                if let Some(value) = &statement.value {
                    self.expression(value);
                }
            }
            StatementType::VarDefinition(statement) => self.var_definition(statement, global),
            StatementType::ConstructorDefinition(statement) => {
                self.function_definition(&statement.definition, None);
            }
            StatementType::FunctionDefinition(statement) => {
                if let Some(type_) = &statement.return_type {
                    self.type_(type_);
                }
                self.function_definition(
                    &statement.definition,
                    Some(statement.name.last_item.token.range.clone()),
                );
            }
            StatementType::ClassDefinition(statement) => {
                if let Some(name) = expression_identifier(&statement.name) {
                    self.class_definition(
                        &statement.definition,
                        Some(TypeIdentity::Nominal(name.value.to_string())),
                    );
                } else {
                    self.class_definition(&statement.definition, None);
                }
            }
            StatementType::TryCatch(statement) => {
                let incoming = self.flow.clone();
                let was_definite = self.definite_flow;
                self.definite_flow = false;
                self.scoped_statement(&statement.body);
                let try_flow = self.flow.clone();
                self.flow = incoming;
                self.push_scope(
                    statement.catch.range.start..statement_type_end(&statement.catch_body),
                );
                self.declare(
                    statement.catch_name.value,
                    statement.catch_name.token,
                    false,
                    DeclarationKind::Variable,
                    format!("catch {}", statement.catch_name.value),
                );
                self.statement_type(&statement.catch_body, false);
                self.pop_scope();
                self.flow = FlowState::join(&try_flow, &self.flow);
                self.definite_flow = was_definite;
                self.commit_insertions(statement_type_end(&statement.catch_body));
            }
            StatementType::Throw(statement) => {
                self.expression(&statement.value);
            }
            StatementType::Const(statement) => {
                if let Some(type_) = &statement.const_type {
                    self.type_(type_);
                }
                self.expression(&statement.initializer.value);
            }
            StatementType::EnumDefinition(statement) => {
                for entry in &statement.entries {
                    if let Some(initializer) = &entry.initializer {
                        self.expression(&initializer.value);
                    }
                }
            }
            StatementType::Expression(statement) => {
                self.expression(&statement.value);
            }
            StatementType::Thread(statement) => {
                self.expression(&statement.value);
            }
            StatementType::DelayThread(statement) => {
                self.expression(&statement.duration);
                self.expression(&statement.value);
            }
            StatementType::WaitThread(statement) => {
                self.expression(&statement.value);
            }
            StatementType::WaitThreadSolo(statement) => {
                self.expression(&statement.value);
            }
            StatementType::Wait(statement) => {
                self.expression(&statement.value);
            }
            StatementType::StructDefinition(statement) => {
                self.struct_definition(
                    &statement.definition,
                    Some(TypeIdentity::Nominal(statement.name.value.to_string())),
                );
            }
            StatementType::TypeDefinition(statement) => self.type_(&statement.type_),
            StatementType::Global(statement) => self.global_definition(&statement.definition),
        }
    }

    fn global_definition(&mut self, definition: &GlobalDefinition<'_>) {
        match definition {
            GlobalDefinition::Function { .. } => {}
            GlobalDefinition::UntypedVar { initializer, .. } => {
                self.expression(&initializer.value);
            }
            GlobalDefinition::TypedVar(definition) => self.var_definition(definition, true),
            GlobalDefinition::Const(statement) => {
                if let Some(type_) = &statement.const_type {
                    self.type_(type_);
                }
                self.expression(&statement.initializer.value);
            }
            GlobalDefinition::Enum(statement) => {
                for entry in &statement.entries {
                    if let Some(initializer) = &entry.initializer {
                        self.expression(&initializer.value);
                    }
                }
            }
            GlobalDefinition::Class(statement) => {
                if let Some(name) = expression_identifier(&statement.name) {
                    self.class_definition(
                        &statement.definition,
                        Some(TypeIdentity::Nominal(name.value.to_string())),
                    );
                } else {
                    self.class_definition(&statement.definition, None);
                }
            }
            GlobalDefinition::Struct(statement) => {
                self.struct_definition(
                    &statement.definition,
                    Some(TypeIdentity::Nominal(statement.name.value.to_string())),
                );
            }
            GlobalDefinition::Type(statement) => self.type_(&statement.type_),
        }
    }

    fn block(&mut self, statements: &[Statement<'_>], range: Range<usize>) {
        self.push_scope(range);
        self.predeclare_statements(statements, false);
        for statement in statements {
            self.statement(statement, false);
        }
        self.pop_scope();
    }

    fn scoped_statement(&mut self, statement: &Statement<'_>) {
        self.push_scope(statement_type_start(&statement.ty)..statement_end(statement));
        self.predeclare_statement_type(&statement.ty, false);
        self.statement(statement, false);
        self.pop_scope();
    }

    fn scoped_statement_type(&mut self, statement: &StatementType<'_>) {
        self.push_scope(statement_type_start(statement)..statement_type_end(statement));
        self.predeclare_statement_type(statement, false);
        self.statement_type(statement, false);
        self.pop_scope();
    }

    fn var_definition(&mut self, definition: &VarDefinitionStatement<'_>, global: bool) {
        self.type_(&definition.type_);
        for (variable, _) in &definition.definitions.items {
            self.variable(variable, &definition.type_, global);
        }
        self.variable(&definition.definitions.last_item, &definition.type_, global);
    }

    fn variable(&mut self, variable: &VarDefinition<'_>, type_: &Type<'_>, global: bool) {
        let signature = variable
            .initializer
            .as_ref()
            .and_then(|initializer| initializer_signature(variable.name.value, &initializer.value));
        let callable = signature.is_some();
        let signature =
            signature.or_else(|| functionref_signature(Some(variable.name.value), type_));
        let initializer_source = variable
            .initializer
            .as_ref()
            .map(|initializer| self.expression(&initializer.value));
        self.declare(
            variable.name.value,
            variable.name.token,
            global,
            DeclarationKind::Variable,
            variable_detail(variable.name.value, Some(type_)),
        );
        self.declaration_mut(&variable.name.token.range)
            .declared_type = Some(display_nominal_type(type_));
        self.declaration_mut(&variable.name.token.range)
            .type_identity = type_identity(type_);
        self.declaration_mut(&variable.name.token.range).signature = signature;
        // A declared type answers resolution on its own, but the value the initializer produced is
        // what a mismatch check has to compare against it, so it is recorded either way.
        if let Some(source) = &initializer_source
            && *source != ValueSource::Unknown
            && !callable
        {
            self.declaration_mut(&variable.name.token.range)
                .initializer_source = Some(source.clone());
        }
        if !global && matches!(type_, Type::Local(_)) {
            let mut source = initializer_source.unwrap_or(ValueSource::Unknown);
            if callable {
                // The declaration carries the named signature for a directly assigned function or
                // lambda, so keep pointing at it rather than at the anonymous callable value.
                source = ValueSource::Declaration(variable.name.token.range.clone());
            }
            self.flow
                .locals
                .insert(variable.name.token.range.clone(), source);
        }
    }

    fn function_definition(
        &mut self,
        definition: &FunctionDefinition<'_>,
        declaration: Option<Range<usize>>,
    ) {
        if let Some(environment) = &definition.environment {
            self.expression(&environment.value);
        }
        if let Some(captures) = &definition.captures
            && let Some(names) = &captures.names
        {
            for (name, _) in &names.items {
                self.reference(name.value, name.token);
            }
            self.reference(names.last_item.value, names.last_item.token);
        }

        let outer_flow = self.flow.clone();
        let was_definite = self.definite_flow;
        self.flow = outer_flow.unknowned();
        self.flow.insertions.clear();
        self.definite_flow = true;
        self.return_targets.push(declaration);
        self.push_scope(definition.open.range.start..statement_type_end(definition.body.as_ref()));
        match &definition.params {
            FunctionParams::NonVariable { params } => {
                if let Some(params) = params {
                    for (param, _) in &params.items {
                        self.function_param(param);
                    }
                    self.function_param(&params.last_item);
                }
            }
            FunctionParams::NonEmptyVariable { params, .. } => {
                for (param, _) in &params.items {
                    self.function_param(param);
                }
                self.function_param(&params.last_item);
            }
            FunctionParams::EmptyVariable { .. } => {}
        }
        match definition.body.as_ref() {
            StatementType::Block(block) => {
                self.predeclare_statements(&block.statements, false);
                for statement in &block.statements {
                    self.statement(statement, false);
                }
            }
            body => self.statement_type(body, false),
        }
        self.pop_scope();
        self.return_targets.pop();
        self.flow = outer_flow;
        self.definite_flow = was_definite;
    }

    fn function_param(&mut self, param: &FunctionParam<'_>) {
        if let Some(type_) = &param.type_ {
            self.type_(type_);
        }
        if let Some(initializer) = &param.initializer {
            self.expression(&initializer.value);
        }
        self.declare(
            param.name.value,
            param.name.token,
            false,
            DeclarationKind::Parameter,
            parameter_detail(param),
        );
        self.declaration_mut(&param.name.token.range).declared_type =
            param.type_.as_ref().map(display_nominal_type);
        self.declaration_mut(&param.name.token.range).type_identity =
            param.type_.as_ref().and_then(type_identity);
        self.declaration_mut(&param.name.token.range).signature = param
            .type_
            .as_ref()
            .and_then(|type_| functionref_signature(Some(param.name.value), type_));
    }

    fn class_definition(&mut self, definition: &ClassDefinition<'_>, owner: Option<TypeIdentity>) {
        if let Some(extends) = &definition.extends {
            self.expression(&extends.name);
        }
        let suspended_owner = owner.is_none().then(|| self.owners.pop()).flatten();
        if let Some(owner) = &owner {
            self.owners.push(owner.clone());
        }
        self.push_scope(definition.open.range.start..definition.close.range.end);
        for member in &definition.members {
            match &member.slot {
                Slot::Property { name, .. } => {
                    self.declare(
                        name.value,
                        name.token,
                        false,
                        DeclarationKind::Field,
                        format!("field {}", name.value),
                    );
                    self.declaration_mut(&name.token.range).owner = self.owners.last().cloned();
                }
                Slot::Function {
                    return_type,
                    name,
                    definition,
                    ..
                } => {
                    self.declare(
                        name.value,
                        name.token,
                        false,
                        DeclarationKind::Method,
                        function_detail(name.value, return_type.as_ref(), definition),
                    );
                    let owner = self.owners.last().cloned();
                    let declaration = self.declaration_mut(&name.token.range);
                    declaration.owner = owner;
                    declaration.return_type = return_type.as_ref().map(display_nominal_type);
                    declaration.return_signature = return_type
                        .as_ref()
                        .and_then(|type_| functionref_signature(None, type_));
                    declaration.signature = Some(function_signature(
                        name.value,
                        return_type.as_ref(),
                        definition,
                    ));
                }
                Slot::Constructor {
                    constructor,
                    definition,
                    ..
                } => {
                    self.declare(
                        "constructor",
                        constructor,
                        false,
                        DeclarationKind::Constructor,
                        function_detail("constructor", None, definition),
                    );
                    self.declaration_mut(&constructor.range).owner = self.owners.last().cloned();
                    self.declaration_mut(&constructor.range).signature =
                        Some(function_signature("constructor", None, definition));
                }
                Slot::ComputedProperty { .. } => {}
            }
        }
        for member in &definition.members {
            self.slot(&member.slot);
        }
        self.pop_scope();
        if owner.is_some() {
            self.owners.pop();
        }
        if let Some(owner) = suspended_owner {
            self.owners.push(owner);
        }
    }

    fn struct_definition(
        &mut self,
        definition: &StructDefinition<'_>,
        owner: Option<TypeIdentity>,
    ) {
        let suspended_owner = owner.is_none().then(|| self.owners.pop()).flatten();
        if let Some(owner) = &owner {
            self.owners.push(owner.clone());
        }
        for property in &definition.properties {
            self.type_(&property.type_);
            self.record_declaration(
                property.name.value,
                property.name.token,
                false,
                DeclarationKind::Field,
                format!("{} {}", display_type(&property.type_), property.name.value),
                false,
            );
            let owner = self.owners.last().cloned();
            let declaration = self.declaration_mut(&property.name.token.range);
            declaration.owner = owner;
            declaration.declared_type = Some(display_nominal_type(&property.type_));
            declaration.type_identity = type_identity(&property.type_);
            declaration.signature =
                functionref_signature(Some(property.name.value), &property.type_);
            if let Some(initializer) = &property.initializer {
                self.expression(&initializer.value);
            }
        }
        if owner.is_some() {
            self.owners.pop();
        }
        if let Some(owner) = suspended_owner {
            self.owners.push(owner);
        }
    }

    fn slot(&mut self, slot: &Slot<'_>) {
        match slot {
            Slot::Property { initializer, .. } => {
                self.expression(&initializer.value);
            }
            Slot::ComputedProperty {
                name, initializer, ..
            } => {
                self.expression(name);
                self.expression(&initializer.value);
            }
            Slot::Constructor { definition, .. } => self.function_definition(definition, None),
            Slot::Function {
                return_type,
                name,
                definition,
                ..
            } => {
                if let Some(type_) = return_type {
                    self.type_(type_);
                }
                self.function_definition(definition, Some(name.token.range.clone()));
            }
        }
    }

    fn table_expression(&mut self, table: &TableExpression<'_>) -> ValueSource {
        let owner = TypeIdentity::Structural(table.open.range.start..table.close.range.end);
        self.owners.push(owner.clone());
        for slot in &table.slots {
            match &slot.ty {
                TableSlotType::Slot(Slot::Property { name, initializer }) => {
                    let source = self.expression(&initializer.value);
                    self.record_declaration(
                        name.value,
                        name.token,
                        false,
                        DeclarationKind::Field,
                        format!("field {}", name.value),
                        false,
                    );
                    let inferred = is_inferable_initializer(&initializer.value).then_some(source);
                    let declaration = self.declaration_mut(&name.token.range);
                    declaration.owner = Some(owner.clone());
                    if let Some(source) = &inferred {
                        declaration.initializer_source = Some(source.clone());
                    }
                    self.flow.members.insert(
                        (structural_range(&owner).clone(), name.value.to_string()),
                        inferred.unwrap_or(ValueSource::Unknown),
                    );
                }
                TableSlotType::Slot(Slot::ComputedProperty {
                    name, initializer, ..
                }) => {
                    self.expression(name);
                    self.expression(&initializer.value);
                }
                TableSlotType::Slot(Slot::Constructor {
                    constructor,
                    definition,
                    ..
                }) => {
                    self.record_declaration(
                        "constructor",
                        constructor,
                        false,
                        DeclarationKind::Constructor,
                        function_detail("constructor", None, definition),
                        false,
                    );
                    let declaration = self.declaration_mut(&constructor.range);
                    declaration.owner = Some(owner.clone());
                    declaration.signature =
                        Some(function_signature("constructor", None, definition));
                    self.flow.members.insert(
                        (structural_range(&owner).clone(), "constructor".to_string()),
                        ValueSource::Member {
                            receiver: Box::new(ValueSource::Type(owner.clone())),
                            name: "constructor".to_string(),
                        },
                    );
                    self.function_definition(definition, None);
                }
                TableSlotType::Slot(Slot::Function {
                    return_type,
                    name,
                    definition,
                    ..
                }) => {
                    if let Some(type_) = return_type {
                        self.type_(type_);
                    }
                    self.record_declaration(
                        name.value,
                        name.token,
                        false,
                        DeclarationKind::Method,
                        function_detail(name.value, return_type.as_ref(), definition),
                        false,
                    );
                    let declaration = self.declaration_mut(&name.token.range);
                    declaration.owner = Some(owner.clone());
                    declaration.return_type = return_type.as_ref().map(display_nominal_type);
                    declaration.return_signature = return_type
                        .as_ref()
                        .and_then(|type_| functionref_signature(None, type_));
                    declaration.signature = Some(function_signature(
                        name.value,
                        return_type.as_ref(),
                        definition,
                    ));
                    self.flow.members.insert(
                        (structural_range(&owner).clone(), name.value.to_string()),
                        ValueSource::Member {
                            receiver: Box::new(ValueSource::Type(owner.clone())),
                            name: name.value.to_string(),
                        },
                    );
                    self.function_definition(definition, Some(name.token.range.clone()));
                }
                TableSlotType::JsonProperty {
                    name,
                    name_token,
                    value,
                    ..
                } => {
                    let source = self.expression(value);
                    if crate::is_valid_identifier(name) {
                        let range = name_token.range.start + 1..name_token.range.end - 1;
                        self.record_declaration_range(
                            name,
                            range.clone(),
                            false,
                            DeclarationKind::Field,
                            format!("field {name}"),
                            false,
                        );
                        let inferred = is_inferable_initializer(value).then_some(source);
                        let declaration = self.declaration_mut(&range);
                        declaration.owner = Some(owner.clone());
                        if let Some(source) = &inferred {
                            declaration.initializer_source = Some(source.clone());
                        }
                        self.flow.members.insert(
                            (structural_range(&owner).clone(), name.to_string()),
                            inferred.unwrap_or(ValueSource::Unknown),
                        );
                    }
                }
            }
        }
        self.owners.pop();
        ValueSource::Type(owner)
    }

    fn expression(&mut self, expression: &Expression<'_>) -> ValueSource {
        match expression {
            Expression::Parens(expression) => self.expression(&expression.value),
            Expression::Literal(_) => ValueSource::Unknown,
            Expression::Var(expression) => {
                let target = self.reference(expression.name.value, expression.name.token);
                target.map_or_else(
                    || {
                        if self
                            .owners
                            .last()
                            .is_some_and(|_| expression.name.value == "this")
                        {
                            ValueSource::Type(self.owners.last().unwrap().clone())
                        } else {
                            ValueSource::Workspace(expression.name.value.to_string())
                        }
                    },
                    |target| {
                        self.flow
                            .locals
                            .get(&target)
                            .cloned()
                            .unwrap_or(ValueSource::Declaration(target))
                    },
                )
            }
            Expression::RootVar(expression) => {
                self.workspace_reference(expression.name.value, expression.name.token);
                ValueSource::Workspace(expression.name.value.to_string())
            }
            Expression::Index(expression) => {
                self.expression(&expression.base);
                self.expression(&expression.index);
                ValueSource::Unknown
            }
            Expression::Property(expression) => {
                let receiver = self.expression(&expression.base);
                let (name, range) = match &expression.property {
                    MethodIdentifier::Identifier(name) => {
                        (name.value.to_string(), name.token.range.clone())
                    }
                    MethodIdentifier::Constructor(token) => {
                        ("constructor".to_string(), token.range.clone())
                    }
                };
                let structural = structural_source_range(&receiver).cloned();
                let flow_value = structural.as_ref().and_then(|owner| {
                    self.flow
                        .members
                        .get(&(owner.clone(), name.clone()))
                        .cloned()
                });
                let exists = structural.as_ref().is_none_or(|owner| {
                    flow_value.is_some()
                        || self.document.declarations.iter().any(|declaration| {
                            declaration.name == name
                                && declaration.owner.as_ref()
                                    == Some(&TypeIdentity::Structural(owner.clone()))
                        })
                });
                self.document.member_references.push(OwnedMemberReference {
                    name: name.clone(),
                    range,
                    receiver: receiver.clone(),
                    available: exists,
                });
                if let Some(value) = flow_value {
                    return value;
                }
                if !exists {
                    return ValueSource::Unknown;
                }
                ValueSource::Member {
                    receiver: Box::new(receiver),
                    name,
                }
            }
            Expression::Ternary(expression) => {
                self.expression(&expression.condition);
                let incoming = self.flow.clone();
                let was_definite = self.definite_flow;
                self.definite_flow = false;
                self.expression(&expression.true_value);
                let true_flow = self.flow.clone();
                self.flow = incoming;
                self.expression(&expression.false_value);
                self.flow = FlowState::join(&true_flow, &self.flow);
                self.definite_flow = was_definite;
                self.commit_insertions(expression_end(&expression.false_value));
                ValueSource::Unknown
            }
            Expression::Binary(expression) => self.binary_expression(expression),
            Expression::Prefix(expression) => {
                if matches!(
                    expression.operator,
                    PrefixOperator::Delete(_)
                        | PrefixOperator::Increment(_)
                        | PrefixOperator::Decrement(_)
                ) {
                    let target = self.assignment_target(&expression.value, false);
                    self.apply_assignment(target, ValueSource::Unknown, false);
                } else {
                    self.expression(&expression.value);
                }
                ValueSource::Unknown
            }
            Expression::Postfix(expression) => {
                let target = self.assignment_target(&expression.value, false);
                self.apply_assignment(target, ValueSource::Unknown, false);
                ValueSource::Unknown
            }
            Expression::Comma(expression) => {
                for (value, _) in &expression.values.items {
                    self.expression(value);
                }
                self.expression(&expression.values.last_item)
            }
            Expression::Table(expression) => self.table_expression(expression),
            Expression::Class(expression) => {
                self.class_definition(&expression.definition, None);
                ValueSource::Unknown
            }
            Expression::Array(expression) => {
                for value in &expression.values {
                    self.expression(&value.value);
                }
                ValueSource::Unknown
            }
            Expression::Function(expression) => {
                if let Some(type_) = &expression.return_type {
                    self.type_(type_);
                }
                self.function_definition(&expression.definition, None);
                let range = expression.function.range.clone();
                self.document.callables.insert(
                    range.clone(),
                    anonymous_signature(
                        expression.return_type.as_ref(),
                        &expression.definition.params,
                    ),
                );
                ValueSource::Callable(range)
            }
            Expression::Lambda(expression) => {
                let outer_flow = self.flow.clone();
                let was_definite = self.definite_flow;
                self.flow = outer_flow.unknowned();
                self.flow.insertions.clear();
                self.definite_flow = true;
                self.push_scope(expression.open.range.start..expression_end(&expression.value));
                match &expression.params {
                    FunctionParams::NonVariable { params } => {
                        if let Some(params) = params {
                            for (param, _) in &params.items {
                                self.function_param(param);
                            }
                            self.function_param(&params.last_item);
                        }
                    }
                    FunctionParams::NonEmptyVariable { params, .. } => {
                        for (param, _) in &params.items {
                            self.function_param(param);
                        }
                        self.function_param(&params.last_item);
                    }
                    FunctionParams::EmptyVariable { .. } => {}
                }
                self.expression(&expression.value);
                self.pop_scope();
                self.flow = outer_flow;
                self.definite_flow = was_definite;
                let range = expression.at.range.clone();
                self.document
                    .callables
                    .insert(range.clone(), anonymous_signature(None, &expression.params));
                ValueSource::Callable(range)
            }
            Expression::Call(expression) => {
                let function = self.expression(&expression.function);
                let arguments = expression
                    .arguments
                    .iter()
                    .map(|argument| OwnedArgument {
                        range: expression_start(&argument.value)..expression_end(&argument.value),
                        source: self.expression(&argument.value),
                    })
                    .collect();
                // A post-initializer adds per-instance slots to the called value, so it is analyzed
                // as its own structural shape and kept beside the call's own type.
                let extra = expression
                    .post_initializer
                    .as_ref()
                    .map(|initializer| self.table_expression(initializer))
                    .and_then(|value| structural_source_range(&value).cloned());
                self.document.calls.push(OwnedCall {
                    open: expression.open.range.clone(),
                    close: expression.close.range.clone(),
                    callable: function.clone(),
                    commas: expression
                        .arguments
                        .iter()
                        .filter_map(|argument| argument.comma.map(|comma| comma.range.clone()))
                        .collect(),
                    arguments,
                });
                let called = ValueSource::Call(Box::new(function));
                match extra {
                    Some(extra) => ValueSource::Augmented {
                        base: Box::new(called),
                        extra,
                    },
                    None => called,
                }
            }
            Expression::Delegate(expression) => {
                self.expression(&expression.parent);
                self.expression(&expression.value);
                ValueSource::Unknown
            }
            Expression::Vector(expression) => {
                self.expression(&expression.x);
                self.expression(&expression.y);
                self.expression(&expression.z);
                ValueSource::Unknown
            }
            Expression::Expect(expression) => {
                self.type_(&expression.ty);
                self.expression(&expression.value);
                type_identity(&expression.ty).map_or(ValueSource::Unknown, ValueSource::Type)
            }
        }
    }

    fn binary_expression(&mut self, expression: &BinaryExpression<'_>) -> ValueSource {
        match expression.operator {
            BinaryOperator::Assign(_) | BinaryOperator::AssignNewSlot(_, _) => {
                let new_slot = matches!(expression.operator, BinaryOperator::AssignNewSlot(_, _));
                let target = self.assignment_target(&expression.left, new_slot);
                let source = self.expression(&expression.right);
                self.apply_assignment(target, source.clone(), new_slot);
                source
            }
            BinaryOperator::AssignAdd(_)
            | BinaryOperator::AssignSubtract(_)
            | BinaryOperator::AssignMultiply(_)
            | BinaryOperator::AssignDivide(_)
            | BinaryOperator::AssignModulo(_) => {
                let target = self.assignment_target(&expression.left, false);
                self.expression(&expression.right);
                self.apply_assignment(target, ValueSource::Unknown, false);
                ValueSource::Unknown
            }
            BinaryOperator::LogicalAnd(_) | BinaryOperator::LogicalOr(_) => {
                self.expression(&expression.left);
                let skipped = self.flow.clone();
                let was_definite = self.definite_flow;
                self.definite_flow = false;
                self.expression(&expression.right);
                self.flow = FlowState::join(&skipped, &self.flow);
                self.definite_flow = was_definite;
                ValueSource::Unknown
            }
            _ => {
                self.expression(&expression.left);
                self.expression(&expression.right);
                ValueSource::Unknown
            }
        }
    }

    fn assignment_target(
        &mut self,
        expression: &Expression<'_>,
        new_slot: bool,
    ) -> AssignmentTarget {
        match expression {
            Expression::Parens(expression) => self.assignment_target(&expression.value, new_slot),
            Expression::Var(expression) => self
                .reference(expression.name.value, expression.name.token)
                .map_or(AssignmentTarget::Other, AssignmentTarget::Local),
            Expression::Property(expression) => {
                let receiver = self.expression(&expression.base);
                let (name, range) = match &expression.property {
                    MethodIdentifier::Identifier(name) => {
                        (name.value.to_string(), name.token.range.clone())
                    }
                    MethodIdentifier::Constructor(token) => {
                        ("constructor".to_string(), token.range.clone())
                    }
                };
                let Some(owner) = structural_source_range(&receiver).cloned() else {
                    self.document.member_references.push(OwnedMemberReference {
                        name,
                        range,
                        receiver,
                        available: true,
                    });
                    return AssignmentTarget::Other;
                };
                let exists = self.structural_member_exists(&owner, &name);
                if !new_slot {
                    self.document.member_references.push(OwnedMemberReference {
                        name: name.clone(),
                        range: range.clone(),
                        receiver: receiver.clone(),
                        available: exists,
                    });
                }
                AssignmentTarget::StructuralMember {
                    owner,
                    name,
                    range,
                    receiver,
                    exists,
                }
            }
            Expression::Index(expression) => {
                let receiver = self.expression(&expression.base);
                self.expression(&expression.index);
                let Some(owner) = structural_source_range(&receiver).cloned() else {
                    return AssignmentTarget::Other;
                };
                let Some((name, range)) = static_string_key(&expression.index) else {
                    return AssignmentTarget::Other;
                };
                let exists = self.structural_member_exists(&owner, &name);
                AssignmentTarget::StructuralMember {
                    owner,
                    name,
                    range,
                    receiver,
                    exists,
                }
            }
            _ => {
                self.expression(expression);
                AssignmentTarget::Other
            }
        }
    }

    fn apply_assignment(&mut self, target: AssignmentTarget, source: ValueSource, new_slot: bool) {
        match target {
            AssignmentTarget::Local(range) => {
                let is_local =
                    self.document
                        .declaration_for_range(&range)
                        .is_some_and(|declaration| {
                            !declaration.is_global
                                && declaration
                                    .declared_type
                                    .as_deref()
                                    .is_none_or(|type_| matches!(type_, "local" | "var"))
                        });
                if is_local {
                    self.flow.locals.insert(range, source);
                }
            }
            AssignmentTarget::StructuralMember {
                owner,
                name,
                range,
                receiver,
                exists,
            } => {
                if new_slot && !exists && !self.definite_flow {
                    let entry = self
                        .flow
                        .insertions
                        .entry((owner, name))
                        .or_insert_with(|| PendingInsertion {
                            sites: Vec::new(),
                            source: source.clone(),
                        });
                    if !entry.sites.iter().any(|(site, _)| *site == range) {
                        entry.sites.push((range, receiver));
                    }
                    entry.source = source;
                    return;
                }
                if new_slot && !exists {
                    self.declare_structural_member(&owner, &name, range, &source);
                }
                if exists || new_slot {
                    self.flow.members.insert((owner, name), source);
                }
            }
            AssignmentTarget::Other => {}
        }
    }

    fn declare_structural_member(
        &mut self,
        owner: &Range<usize>,
        name: &str,
        range: Range<usize>,
        source: &ValueSource,
    ) {
        self.record_declaration_range(
            name,
            range.clone(),
            false,
            DeclarationKind::Field,
            format!("field {name}"),
            false,
        );
        let declaration = self.declaration_mut(&range);
        declaration.owner = Some(TypeIdentity::Structural(owner.clone()));
        if *source != ValueSource::Unknown {
            declaration.initializer_source = Some(source.clone());
        }
    }

    /// Materializes slots that every joined path inserted. `available_from` is the end of the
    /// construct whose paths were joined, because the slot only exists once all of them rejoin.
    fn commit_insertions(&mut self, available_from: usize) {
        if !self.definite_flow || self.flow.insertions.is_empty() {
            return;
        }
        let mut pending = std::mem::take(&mut self.flow.insertions)
            .into_iter()
            .collect::<Vec<_>>();
        pending.sort_by(
            |((left_owner, left_name), _), ((right_owner, right_name), _)| {
                left_owner
                    .start
                    .cmp(&right_owner.start)
                    .then_with(|| left_name.cmp(right_name))
            },
        );
        for ((owner, name), insertion) in pending {
            let mut sites = insertion.sites;
            sites.sort_by_key(|(range, _)| range.start);
            let mut sites = sites.into_iter();
            let Some((declaration_range, _)) = sites.next() else {
                continue;
            };
            self.declare_structural_member(
                &owner,
                &name,
                declaration_range.clone(),
                &insertion.source,
            );
            self.declaration_mut(&declaration_range).available_from = Some(available_from);
            for (range, receiver) in sites {
                self.document.member_references.push(OwnedMemberReference {
                    name: name.clone(),
                    range,
                    receiver,
                    available: true,
                });
            }
            self.flow.members.insert((owner, name), insertion.source);
        }
    }

    fn record_return_source(&mut self, range: Range<usize>, source: ValueSource) {
        let Some(Some(target)) = self.return_targets.last().cloned() else {
            return;
        };
        if let Some(declaration) = self
            .document
            .declarations
            .iter_mut()
            .find(|declaration| declaration.range == target)
        {
            declaration.returns.push(OwnedReturn { range, source });
        }
    }

    fn structural_member_exists(&self, owner: &Range<usize>, name: &str) -> bool {
        self.flow
            .members
            .contains_key(&(owner.clone(), name.to_string()))
            || self.document.declarations.iter().any(|declaration| {
                declaration.name == name
                    && declaration.owner.as_ref() == Some(&TypeIdentity::Structural(owner.clone()))
            })
    }

    fn type_(&mut self, type_: &Type<'_>) {
        match type_ {
            Type::Local(_) | Type::Var(_) => {}
            Type::Plain(type_) => self.workspace_reference(type_.name.value, type_.name.token),
            Type::Array(type_) => {
                self.type_(&type_.base);
                self.expression(&type_.len);
            }
            Type::Generic(type_) => {
                self.type_(&type_.base);
                for (param, _) in &type_.params.items {
                    self.type_(param);
                }
                self.type_(&type_.params.last_item);
            }
            Type::FunctionRef(type_) => {
                if let Some(return_type) = &type_.return_type {
                    self.type_(return_type);
                }
                if let Some(params) = &type_.params {
                    for (param, _) in &params.items {
                        self.function_ref_param(param);
                    }
                    self.function_ref_param(&params.last_item);
                }
            }
            Type::Struct(type_) => self.struct_definition(
                &type_.definition,
                Some(TypeIdentity::Structural(
                    type_.struct_.range.start..type_.definition.close.range.end,
                )),
            ),
            Type::Reference(type_) => self.type_(&type_.base),
            Type::Nullable(type_) => self.type_(&type_.base),
        }
    }

    fn function_ref_param(&mut self, param: &FunctionRefParam<'_>) {
        self.type_(&param.type_);
        if let Some(initializer) = &param.initializer {
            self.expression(&initializer.value);
        }
    }

    fn declare(
        &mut self,
        name: &str,
        token: &Token<'_>,
        is_global: bool,
        kind: DeclarationKind,
        detail: String,
    ) {
        self.declare_binding(name, token, is_global, kind, detail, true);
    }

    /// `lexical` is false for a name that becomes a slot on another table, such as
    /// `function Class::Method()`, where reusing the name is not a redeclaration.
    fn declare_binding(
        &mut self,
        name: &str,
        token: &Token<'_>,
        is_global: bool,
        kind: DeclarationKind,
        detail: String,
        lexical: bool,
    ) {
        let range = token.range.clone();
        self.record_declaration(name, token, is_global, kind, detail, true);
        self.declaration_mut(&range).namespaced = !lexical;
        // The root scope holds file-scope declarations, which are root-table slots rather than
        // lexical bindings, so redefining one there is not a duplicate declaration.
        let lexical = lexical && self.scopes.len() > 1;
        let previous = self
            .scopes
            .last_mut()
            .expect("semantic analyzer always has a scope")
            .declarations
            .insert(name.to_string(), range.clone());
        if let Some(previous) = previous
            && lexical
            && previous != range
        {
            let previous_kind = self
                .document
                .declaration_for_range(&previous)
                .map_or(kind, |declaration| declaration.kind);
            self.document.duplicates.push(OwnedDuplicate {
                name: name.to_string(),
                range,
                kind,
                previous,
                previous_kind,
            });
        }
    }

    fn record_declaration(
        &mut self,
        name: &str,
        token: &Token<'_>,
        is_global: bool,
        kind: DeclarationKind,
        detail: String,
        in_scope: bool,
    ) {
        self.record_declaration_range(name, token.range.clone(), is_global, kind, detail, in_scope);
    }

    fn record_declaration_range(
        &mut self,
        name: &str,
        range: Range<usize>,
        file_scope: bool,
        kind: DeclarationKind,
        detail: String,
        in_scope: bool,
    ) {
        // Only file-scope declarations the file exports are visible to other files.
        let is_global = file_scope && self.exports.exports(name, kind);
        self.document.declarations.push(OwnedDeclaration {
            name: name.to_string(),
            range,
            is_global,
            implicitly_global: file_scope && self.exports.exports_implicitly(name, kind),
            namespaced: false,
            file_scope,
            targets: VmTargets::ALL,
            kind,
            detail,
            declared_type: None,
            type_identity: None,
            return_type: None,
            type_object: None,
            owner: None,
            base_type: None,
            initializer_source: None,
            signature: None,
            return_signature: None,
            returns: Vec::new(),
            available_from: None,
            visibility: self
                .scopes
                .last()
                .expect("semantic analyzer always has a scope")
                .range
                .clone(),
            scope_depth: self.scopes.len() - 1,
            in_scope,
        });
    }

    /// Attaches an explicit type to a declaration that has one, so member resolution can use it.
    fn record_declared_type(&mut self, range: &Range<usize>, type_: Option<&Type<'_>>) {
        let Some(type_) = type_ else {
            return;
        };
        let declaration = self.declaration_mut(range);
        declaration.declared_type = Some(display_nominal_type(type_));
        declaration.type_identity = type_identity(type_);
    }

    fn declaration_mut(&mut self, range: &Range<usize>) -> &mut OwnedDeclaration {
        self.document
            .declarations
            .iter_mut()
            .find(|declaration| declaration.range == *range)
            .expect("declaration was just recorded")
    }

    fn reference(&mut self, name: &str, token: &Token<'_>) -> Option<Range<usize>> {
        let target = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.declarations.get(name).cloned());
        self.document.references.push(OwnedReference {
            name: name.to_string(),
            range: token.range.clone(),
            target,
        });
        self.document.references.last().unwrap().target.clone()
    }

    fn workspace_reference(&mut self, name: &str, token: &Token<'_>) {
        self.document.references.push(OwnedReference {
            name: name.to_string(),
            range: token.range.clone(),
            target: None,
        });
    }

    fn push_scope(&mut self, range: Range<usize>) {
        self.scopes.push(Scope {
            declarations: HashMap::new(),
            range,
        });
    }

    fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for declaration in scope.declarations.values() {
                self.flow.locals.remove(declaration);
            }
        }
    }
}

fn statement_end(statement: &Statement<'_>) -> usize {
    statement.semicolon.map_or_else(
        || statement_type_end(&statement.ty),
        |token| token.range.end,
    )
}

fn switch_case_start(case: &SwitchCase<'_>) -> usize {
    match &case.condition {
        SwitchCaseCondition::Default { default } => default.range.start,
        SwitchCaseCondition::Case { case, .. } => case.range.start,
    }
}

fn statement_type_start(statement: &StatementType<'_>) -> usize {
    match statement {
        StatementType::Empty(statement) => statement.empty.map_or(0, |token| token.range.start),
        StatementType::Block(statement) => statement.open.range.start,
        StatementType::If(statement) => statement.if_.range.start,
        StatementType::While(statement) => statement.while_.range.start,
        StatementType::DoWhile(statement) => statement.do_.range.start,
        StatementType::Switch(statement) => statement.switch.range.start,
        StatementType::For(statement) => statement.for_.range.start,
        StatementType::Foreach(statement) => statement.foreach.range.start,
        StatementType::Break(statement) => statement.break_.range.start,
        StatementType::Continue(statement) => statement.continue_.range.start,
        StatementType::Return(statement) => statement.return_.range.start,
        StatementType::Yield(statement) => statement.yield_.range.start,
        StatementType::VarDefinition(statement) => type_start(&statement.type_),
        StatementType::ConstructorDefinition(statement) => statement.function.range.start,
        StatementType::FunctionDefinition(statement) => statement
            .return_type
            .as_ref()
            .map_or(statement.function.range.start, type_start),
        StatementType::ClassDefinition(statement) => statement.class.range.start,
        StatementType::TryCatch(statement) => statement.try_.range.start,
        StatementType::Throw(statement) => statement.throw.range.start,
        StatementType::Const(statement) => statement.const_.range.start,
        StatementType::EnumDefinition(statement) => statement.enum_.range.start,
        StatementType::Expression(statement) => expression_start(&statement.value),
        StatementType::Thread(statement) => statement.thread.range.start,
        StatementType::DelayThread(statement) => statement.delay_thread.range.start,
        StatementType::WaitThread(statement) => statement.wait_thread.range.start,
        StatementType::WaitThreadSolo(statement) => statement.wait_thread_solo.range.start,
        StatementType::Wait(statement) => statement.wait.range.start,
        StatementType::StructDefinition(statement) => statement.struct_.range.start,
        StatementType::TypeDefinition(statement) => statement.typedef.range.start,
        StatementType::Global(statement) => statement.global.range.start,
        StatementType::GlobalizeAllFunctions(statement) => {
            statement.globalize_all_functions.range.start
        }
        StatementType::Untyped(statement) => statement.untyped.range.start,
    }
}

fn statement_type_end(statement: &StatementType<'_>) -> usize {
    match statement {
        StatementType::Empty(statement) => statement.empty.map_or(0, |token| token.range.end),
        StatementType::Block(statement) => statement.close.range.end,
        StatementType::If(statement) => match &statement.ty {
            IfStatementType::NoElse { body } => statement_type_end(body),
            IfStatementType::Else { else_body, .. } => statement_type_end(else_body),
        },
        StatementType::While(statement) => statement_type_end(&statement.body),
        StatementType::DoWhile(statement) => statement.close.range.end,
        StatementType::Switch(statement) => statement.close_cases.range.end,
        StatementType::For(statement) => statement_type_end(&statement.body),
        StatementType::Foreach(statement) => statement_type_end(&statement.body),
        StatementType::Break(statement) => statement.break_.range.end,
        StatementType::Continue(statement) => statement.continue_.range.end,
        StatementType::Return(statement) => statement
            .value
            .as_ref()
            .map_or(statement.return_.range.end, |value| expression_end(value)),
        StatementType::Yield(statement) => statement
            .value
            .as_ref()
            .map_or(statement.yield_.range.end, |value| expression_end(value)),
        StatementType::VarDefinition(statement) => variable_end(&statement.definitions.last_item),
        StatementType::ConstructorDefinition(statement) => {
            statement_type_end(&statement.definition.body)
        }
        StatementType::FunctionDefinition(statement) => {
            statement_type_end(&statement.definition.body)
        }
        StatementType::ClassDefinition(statement) => statement.definition.close.range.end,
        StatementType::TryCatch(statement) => statement_type_end(&statement.catch_body),
        StatementType::Throw(statement) => expression_end(&statement.value),
        StatementType::Const(statement) => expression_end(&statement.initializer.value),
        StatementType::EnumDefinition(statement) => statement.close.range.end,
        StatementType::Expression(statement) => expression_end(&statement.value),
        StatementType::Thread(statement) => expression_end(&statement.value),
        StatementType::DelayThread(statement) => expression_end(&statement.value),
        StatementType::WaitThread(statement) => expression_end(&statement.value),
        StatementType::WaitThreadSolo(statement) => expression_end(&statement.value),
        StatementType::Wait(statement) => expression_end(&statement.value),
        StatementType::StructDefinition(statement) => statement.definition.close.range.end,
        StatementType::TypeDefinition(statement) => type_end(&statement.type_),
        StatementType::Global(statement) => global_definition_end(&statement.definition),
        StatementType::GlobalizeAllFunctions(statement) => {
            statement.globalize_all_functions.range.end
        }
        StatementType::Untyped(statement) => statement.untyped.range.end,
    }
}

fn expression_start(expression: &Expression<'_>) -> usize {
    match expression {
        Expression::Parens(expression) => expression.open.range.start,
        Expression::Literal(expression) => expression.token.range.start,
        Expression::Var(expression) => expression.name.token.range.start,
        Expression::RootVar(expression) => expression.root.range.start,
        Expression::Index(expression) => expression_start(&expression.base),
        Expression::Property(expression) => expression_start(&expression.base),
        Expression::Ternary(expression) => expression_start(&expression.condition),
        Expression::Binary(expression) => expression_start(&expression.left),
        Expression::Prefix(expression) => expression_start(&expression.value),
        Expression::Postfix(expression) => expression_start(&expression.value),
        Expression::Comma(expression) => expression_start(&expression.values.items[0].0),
        Expression::Table(expression) => expression.open.range.start,
        Expression::Class(expression) => expression.class.range.start,
        Expression::Array(expression) => expression.open.range.start,
        Expression::Function(expression) => expression
            .return_type
            .as_ref()
            .map_or(expression.function.range.start, type_start),
        Expression::Lambda(expression) => expression.at.range.start,
        Expression::Call(expression) => expression_start(&expression.function),
        Expression::Delegate(expression) => expression.delegate.range.start,
        Expression::Vector(expression) => expression.open.range.start,
        Expression::Expect(expression) => expression.expect.range.start,
    }
}

fn expression_end(expression: &Expression<'_>) -> usize {
    match expression {
        Expression::Parens(expression) => expression.close.range.end,
        Expression::Literal(expression) => expression.token.range.end,
        Expression::Var(expression) => expression.name.token.range.end,
        Expression::RootVar(expression) => expression.name.token.range.end,
        Expression::Index(expression) => expression.close.range.end,
        Expression::Property(expression) => match &expression.property {
            MethodIdentifier::Identifier(name) => name.token.range.end,
            MethodIdentifier::Constructor(token) => token.range.end,
        },
        Expression::Ternary(expression) => expression_end(&expression.false_value),
        Expression::Binary(expression) => expression_end(&expression.right),
        Expression::Prefix(expression) => expression_end(&expression.value),
        Expression::Postfix(expression) => match expression.operator {
            PostfixOperator::Increment(token) | PostfixOperator::Decrement(token) => {
                token.range.end
            }
        },
        Expression::Comma(expression) => expression_end(&expression.values.last_item),
        Expression::Table(expression) => expression.close.range.end,
        Expression::Class(expression) => expression.definition.close.range.end,
        Expression::Array(expression) => expression.close.range.end,
        Expression::Function(expression) => statement_type_end(&expression.definition.body),
        Expression::Lambda(expression) => expression_end(&expression.value),
        Expression::Call(expression) => expression
            .post_initializer
            .as_ref()
            .map_or(expression.close.range.end, |table| table.close.range.end),
        Expression::Delegate(expression) => expression_end(&expression.value),
        Expression::Vector(expression) => expression.close.range.end,
        Expression::Expect(expression) => expression.close.range.end,
    }
}

fn type_start(type_: &Type<'_>) -> usize {
    match type_ {
        Type::Local(type_) => type_.local.range.start,
        Type::Var(type_) => type_.var.range.start,
        Type::Plain(type_) => type_.name.token.range.start,
        Type::Array(type_) => type_start(&type_.base),
        Type::Generic(type_) => type_start(&type_.base),
        Type::FunctionRef(type_) => type_
            .return_type
            .as_ref()
            .map_or(type_.functionref.range.start, |base| type_start(base)),
        Type::Struct(type_) => type_.struct_.range.start,
        Type::Reference(type_) => type_start(&type_.base),
        Type::Nullable(type_) => type_start(&type_.base),
    }
}

fn type_end(type_: &Type<'_>) -> usize {
    match type_ {
        Type::Local(type_) => type_.local.range.end,
        Type::Var(type_) => type_.var.range.end,
        Type::Plain(type_) => type_.name.token.range.end,
        Type::Array(type_) => type_.close.range.end,
        Type::Generic(type_) => type_.close.range.end,
        Type::FunctionRef(type_) => type_.close.range.end,
        Type::Struct(type_) => type_.definition.close.range.end,
        Type::Reference(type_) => type_.reference.range.end,
        Type::Nullable(type_) => type_.ornull.range.end,
    }
}

fn variable_end(variable: &VarDefinition<'_>) -> usize {
    variable
        .initializer
        .as_ref()
        .map_or(variable.name.token.range.end, |initializer| {
            expression_end(&initializer.value)
        })
}

fn global_definition_end(definition: &GlobalDefinition<'_>) -> usize {
    match definition {
        GlobalDefinition::Function { name, .. } => name.token.range.end,
        GlobalDefinition::UntypedVar { initializer, .. } => expression_end(&initializer.value),
        GlobalDefinition::TypedVar(definition) => variable_end(&definition.definitions.last_item),
        GlobalDefinition::Const(definition) => expression_end(&definition.initializer.value),
        GlobalDefinition::Enum(definition) => definition.close.range.end,
        GlobalDefinition::Class(definition) => definition.definition.close.range.end,
        GlobalDefinition::Struct(definition) => definition.definition.close.range.end,
        GlobalDefinition::Type(definition) => type_end(&definition.type_),
    }
}

fn expression_identifier<'s>(expression: &'s Expression<'s>) -> Option<&'s Identifier<'s>> {
    match expression {
        Expression::Var(expression) => Some(&expression.name),
        Expression::RootVar(expression) => Some(&expression.name),
        Expression::Parens(expression) => expression_identifier(&expression.value),
        _ => None,
    }
}

/// Collects what a file exports. `global` names an exported declaration, which for functions is
/// usually a forward declaration whose implementation appears later as a plain definition, and
/// `globalize_all_functions` exports every function the file defines.
fn collect_exports(statements: &[&Statement<'_>]) -> Exports {
    let mut exports = Exports::default();
    for statement in statements {
        match &statement.ty {
            StatementType::GlobalizeAllFunctions(_) => exports.all_functions = true,
            StatementType::Global(statement) => {
                for name in global_definition_names(&statement.definition) {
                    exports.names.insert(name);
                }
            }
            _ => {}
        }
    }
    exports
}

fn global_definition_names(definition: &GlobalDefinition<'_>) -> Vec<String> {
    match definition {
        GlobalDefinition::Function { name, .. } | GlobalDefinition::UntypedVar { name, .. } => {
            vec![name.value.to_string()]
        }
        GlobalDefinition::Const(statement) => vec![statement.name.value.to_string()],
        GlobalDefinition::Enum(statement) => vec![statement.name.value.to_string()],
        GlobalDefinition::Struct(statement) => vec![statement.name.value.to_string()],
        GlobalDefinition::Type(statement) => vec![statement.name.value.to_string()],
        GlobalDefinition::Class(statement) => expression_identifier(&statement.name)
            .map(|name| vec![name.value.to_string()])
            .unwrap_or_default(),
        GlobalDefinition::TypedVar(statement) => statement
            .definitions
            .items
            .iter()
            .map(|(definition, _)| definition)
            .chain(std::iter::once(statement.definitions.last_item.as_ref()))
            .map(|definition| definition.name.value.to_string())
            .collect(),
    }
}

fn is_inferable_initializer(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::Parens(expression) => is_inferable_initializer(&expression.value),
        Expression::Expect(_) => true,
        Expression::Table(_) => true,
        Expression::Function(_) | Expression::Lambda(_) => true,
        Expression::Call(expression) => {
            matches!(
                expression.function.as_ref(),
                Expression::Var(_) | Expression::RootVar(_)
            )
        }
        _ => false,
    }
}

fn initializer_signature(name: &str, expression: &Expression<'_>) -> Option<OwnedSignature> {
    match expression {
        Expression::Parens(expression) => initializer_signature(name, &expression.value),
        Expression::Function(expression) => Some(function_signature(
            name,
            expression.return_type.as_ref(),
            &expression.definition,
        )),
        Expression::Lambda(expression) => Some(OwnedSignature {
            label: format!(
                "function {name}({})",
                display_function_params(&expression.params)
            ),
            parameters: function_parameters(&expression.params),
        }),
        _ => None,
    }
}

fn function_detail(
    name: &str,
    return_type: Option<&Type<'_>>,
    definition: &FunctionDefinition<'_>,
) -> String {
    let prefix = return_type
        .map(|type_| format!("{} function", display_type(type_)))
        .unwrap_or_else(|| "function".to_string());
    format!(
        "{prefix} {name}({})",
        display_function_params(&definition.params)
    )
}

fn function_signature(
    name: &str,
    return_type: Option<&Type<'_>>,
    definition: &FunctionDefinition<'_>,
) -> OwnedSignature {
    OwnedSignature {
        label: function_detail(name, return_type, definition),
        parameters: function_parameters(&definition.params),
    }
}

fn anonymous_signature(
    return_type: Option<&Type<'_>>,
    params: &FunctionParams<'_>,
) -> OwnedSignature {
    let prefix = return_type
        .map(|type_| format!("{} ", display_type(type_)))
        .unwrap_or_default();
    OwnedSignature {
        label: format!("{prefix}function({})", display_function_params(params)),
        parameters: function_parameters(params),
    }
}

/// Builds the signature a `functionref` type describes, so declarations typed with one are callable
/// even though they have no function body of their own.
fn functionref_signature(name: Option<&str>, type_: &Type<'_>) -> Option<OwnedSignature> {
    let type_ = match type_ {
        Type::Reference(type_) => return functionref_signature(name, &type_.base),
        Type::Nullable(type_) => return functionref_signature(name, &type_.base),
        Type::FunctionRef(type_) => type_,
        _ => return None,
    };
    let prefix = type_
        .return_type
        .as_ref()
        .map(|type_| format!("{} ", display_type(type_)))
        .unwrap_or_default();
    let name = name.map(|name| format!(" {name}")).unwrap_or_default();
    let params: Vec<OwnedParameter> = type_
        .params
        .as_ref()
        .map(|params| {
            params
                .items
                .iter()
                .map(|(param, _)| param)
                .chain(std::iter::once(params.last_item.as_ref()))
                .map(|param| OwnedParameter {
                    label: display_function_ref_param(param),
                    variadic: false,
                    optional: param.initializer.is_some(),
                    type_identity: type_identity(&param.type_),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(OwnedSignature {
        label: format!(
            "{prefix}function{name}({})",
            params
                .iter()
                .map(|parameter: &OwnedParameter| parameter.label.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        parameters: params,
    })
}

fn function_parameters(params: &FunctionParams<'_>) -> Vec<OwnedParameter> {
    let parameters = match params {
        FunctionParams::NonVariable { params } => params.as_ref().map(|params| {
            params
                .items
                .iter()
                .map(|(parameter, _)| parameter)
                .chain(std::iter::once(params.last_item.as_ref()))
                .map(owned_parameter)
                .collect()
        }),
        FunctionParams::NonEmptyVariable { params, .. } => Some(
            params
                .items
                .iter()
                .map(|(parameter, _)| parameter)
                .chain(std::iter::once(params.last_item.as_ref()))
                .map(owned_parameter)
                .chain(std::iter::once(OwnedParameter {
                    label: "...".to_string(),
                    variadic: true,
                    optional: false,
                    type_identity: None,
                }))
                .collect(),
        ),
        FunctionParams::EmptyVariable { .. } => Some(vec![OwnedParameter {
            label: "...".to_string(),
            variadic: true,
            optional: false,
            type_identity: None,
        }]),
    };
    parameters.unwrap_or_default()
}

fn owned_parameter(parameter: &FunctionParam<'_>) -> OwnedParameter {
    OwnedParameter {
        label: parameter_detail(parameter),
        variadic: false,
        optional: parameter.initializer.is_some(),
        type_identity: parameter.type_.as_ref().and_then(type_identity),
    }
}

fn display_function_params(params: &FunctionParams<'_>) -> String {
    match params {
        FunctionParams::NonVariable { params } => {
            params.as_ref().map(display_param_list).unwrap_or_default()
        }
        FunctionParams::EmptyVariable { .. } => "...".to_string(),
        FunctionParams::NonEmptyVariable { params, .. } => {
            let mut values = params
                .items
                .iter()
                .map(|(param, _)| parameter_detail(param))
                .chain(std::iter::once(parameter_detail(&params.last_item)))
                .collect::<Vec<_>>();
            values.push("...".to_string());
            values.join(", ")
        }
    }
}

fn display_param_list(params: &SeparatedListTrailing1<'_, FunctionParam<'_>>) -> String {
    params
        .items
        .iter()
        .map(|(param, _)| parameter_detail(param))
        .chain(std::iter::once(parameter_detail(&params.last_item)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn parameter_detail(param: &FunctionParam<'_>) -> String {
    let mut detail = variable_detail(param.name.value, param.type_.as_ref());
    if param.initializer.is_some() {
        detail.push_str(" = ...");
    }
    detail
}

fn variable_detail(name: &str, type_: Option<&Type<'_>>) -> String {
    match type_ {
        Some(type_) => format!("{} {name}", display_type(type_)),
        None => format!("var {name}"),
    }
}

fn const_detail(statement: &ConstDefinitionStatement<'_>) -> String {
    match &statement.const_type {
        Some(type_) => format!("const {} {}", display_type(type_), statement.name.value),
        None => format!("const {}", statement.name.value),
    }
}

fn display_type(type_: &Type<'_>) -> String {
    match type_ {
        Type::Local(_) => "local".to_string(),
        Type::Var(_) => "var".to_string(),
        Type::Plain(type_) => type_.name.value.to_string(),
        Type::Array(type_) => format!("{}[]", display_type(&type_.base)),
        Type::Generic(type_) => {
            let params = type_
                .params
                .items
                .iter()
                .map(|(param, _)| display_type(param))
                .chain(std::iter::once(display_type(&type_.params.last_item)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}<{params}>", display_type(&type_.base))
        }
        Type::FunctionRef(type_) => {
            let return_type = type_
                .return_type
                .as_ref()
                .map(|type_| format!("{} ", display_type(type_)))
                .unwrap_or_default();
            let params = type_
                .params
                .as_ref()
                .map(|params| {
                    params
                        .items
                        .iter()
                        .map(|(param, _)| display_function_ref_param(param))
                        .chain(std::iter::once(display_function_ref_param(
                            &params.last_item,
                        )))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            format!("{return_type}functionref({params})")
        }
        Type::Struct(_) => "struct {...}".to_string(),
        Type::Reference(type_) => format!("{}&", display_type(&type_.base)),
        Type::Nullable(type_) => format!("{} ornull", display_type(&type_.base)),
    }
}

fn display_nominal_type(type_: &Type<'_>) -> String {
    match type_ {
        Type::Reference(type_) => display_nominal_type(&type_.base),
        Type::Nullable(type_) => display_nominal_type(&type_.base),
        Type::Generic(type_) => display_nominal_type(&type_.base),
        _ => display_type(type_),
    }
}

fn type_identity(type_: &Type<'_>) -> Option<TypeIdentity> {
    match type_ {
        Type::Plain(type_) => Some(TypeIdentity::Nominal(type_.name.value.to_string())),
        Type::Struct(type_) => Some(TypeIdentity::Structural(
            type_.struct_.range.start..type_.definition.close.range.end,
        )),
        Type::Generic(type_) => type_identity(&type_.base),
        Type::Reference(type_) => type_identity(&type_.base),
        Type::Nullable(type_) => type_identity(&type_.base),
        Type::Local(_) | Type::Var(_) | Type::Array(_) | Type::FunctionRef(_) => None,
    }
}

fn structural_range(identity: &TypeIdentity) -> &Range<usize> {
    match identity {
        TypeIdentity::Structural(range) => range,
        TypeIdentity::Nominal(_) => unreachable!("table identities are structural"),
    }
}

fn structural_source_range(source: &ValueSource) -> Option<&Range<usize>> {
    match source {
        ValueSource::Type(TypeIdentity::Structural(range)) => Some(range),
        _ => None,
    }
}

fn static_string_key(expression: &Expression<'_>) -> Option<(String, Range<usize>)> {
    match expression {
        Expression::Parens(expression) => static_string_key(&expression.value),
        Expression::Literal(expression) => match expression.literal {
            LiteralToken::String(StringToken::Literal(name))
                if crate::is_valid_identifier(name) =>
            {
                Some((
                    name.to_string(),
                    expression.token.range.start + 1..expression.token.range.end - 1,
                ))
            }
            _ => None,
        },
        _ => None,
    }
}

fn display_function_ref_param(param: &FunctionRefParam<'_>) -> String {
    let mut detail = display_type(&param.type_);
    if let Some(name) = &param.name {
        detail.push(' ');
        detail.push_str(name.value);
    }
    if param.initializer.is_some() {
        detail.push_str(" = ...");
    }
    detail
}

fn contains_offset(range: &Range<usize>, offset: usize) -> bool {
    range.start <= offset && offset < range.end
}
