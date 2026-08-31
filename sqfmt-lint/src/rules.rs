use std::collections::{HashMap, HashSet};

use sqparse::ast::{
    BinaryOperator, ClassDefinition, Expression, ForDefinition, FunctionDefinition, FunctionParam,
    FunctionParams, GlobalDefinition, IfStatementType, MethodIdentifier, PrefixOperator, Slot,
    Statement, StatementType, SwitchCaseCondition, TableSlotType, Type, VarDefinitionStatement,
};
use sqparse::token::{LiteralToken, StringToken};

use super::{
    Analysis, Diagnostic, ENTITY_USE_AFTER_YIELD_RULE, FIND_USED_AS_BOOLEAN_RULE,
    FunctionSignature, INVALID_ENTITY_RULE, RemoteCall, SignalUse, SignalUseKind,
    THREAD_IN_POLLING_LOOP_RULE, UNCHECKED_ENCODED_EHANDLE_RULE, UNSAFE_ARRAY_INDEX_RULE,
    WAIT_ZERO_RULE, called_expression_name, contains_reachable_wait,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntityState {
    Valid,
    PossiblyInvalid,
    Null,
    Invalid,
    DecodedHandle,
    AfterYield,
}

#[derive(Clone, Debug, Default)]
struct FlowState {
    entities: HashMap<String, EntityState>,
    destroy_protected: HashSet<String>,
    unchecked_find_indexes: HashSet<String>,
}

impl FlowState {
    fn join(left: &Self, right: &Self) -> Self {
        let mut entities = HashMap::new();
        for name in left.entities.keys().chain(right.entities.keys()) {
            let state = match (left.entities.get(name), right.entities.get(name)) {
                (Some(left), Some(right)) if left == right => *left,
                (Some(EntityState::DecodedHandle), _) | (_, Some(EntityState::DecodedHandle)) => {
                    EntityState::DecodedHandle
                }
                (Some(EntityState::AfterYield), _) | (_, Some(EntityState::AfterYield)) => {
                    EntityState::AfterYield
                }
                _ => EntityState::PossiblyInvalid,
            };
            entities.insert(name.clone(), state);
        }
        Self {
            entities,
            destroy_protected: left
                .destroy_protected
                .intersection(&right.destroy_protected)
                .cloned()
                .collect(),
            unchecked_find_indexes: left
                .unchecked_find_indexes
                .union(&right.unchecked_find_indexes)
                .cloned()
                .collect(),
        }
    }

    fn mark_yield(&mut self) {
        for (name, state) in &mut self.entities {
            if *state == EntityState::Valid && !self.destroy_protected.contains(name) {
                *state = EntityState::AfterYield;
            }
        }
    }

    fn widen_invalid_after_loop(&mut self) {
        for state in self.entities.values_mut() {
            if matches!(*state, EntityState::Null | EntityState::Invalid) {
                *state = EntityState::PossiblyInvalid;
            }
        }
    }
}

pub(super) fn analyze(statements: &[&Statement<'_>], analysis: &mut Analysis) {
    let mut analyzer = Analyzer { analysis };
    let mut flow = FlowState::default();
    for statement in statements {
        analyzer.statement_type(&statement.ty, &mut flow);
    }
}

struct Analyzer<'a> {
    analysis: &'a mut Analysis,
}

impl Analyzer<'_> {
    fn diagnostic(&mut self, range: std::ops::Range<usize>, rule: &'static str, message: String) {
        self.analysis.local_diagnostics.push(Diagnostic {
            range,
            rule,
            message: format!("{message} [{rule}]"),
        });
    }

    fn statement_type(&mut self, statement: &StatementType<'_>, flow: &mut FlowState) -> bool {
        match statement {
            StatementType::Block(block) => {
                for statement in &block.statements {
                    if !self.statement_type(&statement.ty, flow) {
                        return false;
                    }
                }
                true
            }
            StatementType::If(statement) => {
                self.boolean_context(&statement.condition);
                self.expression(&statement.condition, flow);
                let (true_flow, false_flow) = refined_condition(&statement.condition, flow);
                match &statement.ty {
                    IfStatementType::NoElse { body } => {
                        let mut body_flow = true_flow;
                        let body_falls_through = self.statement_type(body, &mut body_flow);
                        *flow = if body_falls_through {
                            FlowState::join(&body_flow, &false_flow)
                        } else {
                            false_flow
                        };
                        true
                    }
                    IfStatementType::Else {
                        body, else_body, ..
                    } => {
                        let mut body_flow = true_flow;
                        let mut else_flow = false_flow;
                        let body_falls_through = self.statement_type(&body.ty, &mut body_flow);
                        let else_falls_through = self.statement_type(else_body, &mut else_flow);
                        match (body_falls_through, else_falls_through) {
                            (true, true) => {
                                *flow = FlowState::join(&body_flow, &else_flow);
                                true
                            }
                            (true, false) => {
                                *flow = body_flow;
                                true
                            }
                            (false, true) => {
                                *flow = else_flow;
                                true
                            }
                            (false, false) => false,
                        }
                    }
                }
            }
            StatementType::While(statement) => {
                self.polling_loop(&statement.condition, &statement.body);
                self.boolean_context(&statement.condition);
                self.expression(&statement.condition, flow);
                let (mut body_flow, false_flow) = refined_condition(&statement.condition, flow);
                self.statement_type(&statement.body, &mut body_flow);
                *flow = FlowState::join(&false_flow, &body_flow);
                flow.widen_invalid_after_loop();
                true
            }
            StatementType::DoWhile(statement) => {
                self.polling_loop(&statement.condition, &statement.body.ty);
                let mut body_flow = flow.clone();
                self.statement_type(&statement.body.ty, &mut body_flow);
                self.boolean_context(&statement.condition);
                self.expression(&statement.condition, &mut body_flow);
                let (_, false_flow) = refined_condition(&statement.condition, &body_flow);
                *flow = FlowState::join(&false_flow, &body_flow);
                flow.widen_invalid_after_loop();
                true
            }
            StatementType::For(statement) => {
                if let Some(initializer) = &statement.initializer {
                    match initializer {
                        ForDefinition::Definition(definition) => {
                            self.var_definition(definition, flow)
                        }
                        ForDefinition::Expression(expression) => self.expression(expression, flow),
                    }
                }
                if let Some(condition) = &statement.condition {
                    self.polling_loop(condition, &statement.body);
                    self.boolean_context(condition);
                    self.expression(condition, flow);
                } else {
                    self.polling_loop_without_condition(&statement.body);
                }
                let mut body_flow = statement.condition.as_ref().map_or_else(
                    || flow.clone(),
                    |condition| refined_condition(condition, flow).0,
                );
                self.statement_type(&statement.body, &mut body_flow);
                if let Some(increment) = &statement.increment {
                    self.expression(increment, &mut body_flow);
                }
                *flow = FlowState::join(flow, &body_flow);
                flow.widen_invalid_after_loop();
                true
            }
            StatementType::Foreach(statement) => {
                self.expression(&statement.array, flow);
                let mut body_flow = flow.clone();
                self.statement_type(&statement.body, &mut body_flow);
                *flow = FlowState::join(flow, &body_flow);
                flow.widen_invalid_after_loop();
                true
            }
            StatementType::Switch(statement) => {
                self.expression(&statement.condition, flow);
                let mut outcomes = Vec::new();
                for case in &statement.cases {
                    let mut case_flow = flow.clone();
                    if let SwitchCaseCondition::Case { value, .. } = &case.condition {
                        self.expression(value, &mut case_flow);
                    }
                    let mut falls_through = true;
                    for statement in &case.body {
                        if !self.statement_type(&statement.ty, &mut case_flow) {
                            falls_through = false;
                            break;
                        }
                    }
                    if falls_through {
                        outcomes.push(case_flow);
                    }
                }
                if let Some(joined) = outcomes.into_iter().reduce(|a, b| FlowState::join(&a, &b)) {
                    *flow = FlowState::join(flow, &joined);
                }
                true
            }
            StatementType::Break(_) | StatementType::Continue(_) => false,
            StatementType::Return(statement) => {
                if let Some(value) = &statement.value {
                    self.expression(value, flow);
                }
                false
            }
            StatementType::Yield(statement) => {
                if let Some(value) = &statement.value {
                    self.expression(value, flow);
                }
                flow.mark_yield();
                true
            }
            StatementType::VarDefinition(statement) => {
                self.var_definition(statement, flow);
                true
            }
            StatementType::ConstructorDefinition(statement) => {
                self.function(None, &statement.definition);
                true
            }
            StatementType::FunctionDefinition(statement) => {
                self.function(Some(statement.name.last_item.value), &statement.definition);
                true
            }
            StatementType::ClassDefinition(statement) => {
                self.class(&statement.definition);
                true
            }
            StatementType::TryCatch(statement) => {
                let mut body_flow = flow.clone();
                let mut catch_flow = flow.clone();
                let body_falls_through = self.statement_type(&statement.body.ty, &mut body_flow);
                let catch_falls_through =
                    self.statement_type(&statement.catch_body, &mut catch_flow);
                match (body_falls_through, catch_falls_through) {
                    (true, true) => *flow = FlowState::join(&body_flow, &catch_flow),
                    (true, false) => *flow = body_flow,
                    (false, true) => *flow = catch_flow,
                    (false, false) => return false,
                }
                true
            }
            StatementType::Throw(statement) => {
                self.expression(&statement.value, flow);
                false
            }
            StatementType::Const(statement) => {
                self.expression(&statement.initializer.value, flow);
                true
            }
            StatementType::Expression(statement) => {
                self.expression(&statement.value, flow);
                true
            }
            StatementType::Thread(statement) => {
                self.expression(&statement.value, flow);
                true
            }
            StatementType::DelayThread(statement) => {
                self.expression(&statement.duration, flow);
                self.expression(&statement.value, flow);
                true
            }
            StatementType::WaitThread(statement) => {
                self.expression(&statement.value, flow);
                flow.mark_yield();
                true
            }
            StatementType::WaitThreadSolo(statement) => {
                self.expression(&statement.value, flow);
                flow.mark_yield();
                true
            }
            StatementType::Wait(statement) => {
                self.expression(&statement.value, flow);
                if numeric_zero(&statement.value) {
                    self.diagnostic(
                        expression_range(&statement.value),
                        WAIT_ZERO_RULE,
                        "`wait 0` does not advance a game frame; use WaitFrame()".to_string(),
                    );
                }
                flow.mark_yield();
                true
            }
            StatementType::Global(statement) => {
                if let GlobalDefinition::Class(class) = &statement.definition {
                    self.class(&class.definition);
                }
                true
            }
            StatementType::Empty(_)
            | StatementType::EnumDefinition(_)
            | StatementType::StructDefinition(_)
            | StatementType::TypeDefinition(_)
            | StatementType::GlobalizeAllFunctions(_)
            | StatementType::Untyped(_) => true,
        }
    }

    fn function(&mut self, name: Option<&str>, definition: &FunctionDefinition<'_>) {
        if let Some(name) = name {
            let (required, maximum) = parameter_arity(&definition.params);
            self.analysis.function_signatures.push(FunctionSignature {
                name: name.to_string(),
                required,
                maximum,
            });
        }
        let mut flow = FlowState::default();
        for_each_parameter(&definition.params, |parameter| {
            if is_entity_type(parameter.type_.as_ref()) {
                let state = if parameter
                    .initializer
                    .as_ref()
                    .is_some_and(|initializer| is_null(&initializer.value))
                    || parameter
                        .type_
                        .as_ref()
                        .is_some_and(is_nullable_entity_type)
                {
                    EntityState::PossiblyInvalid
                } else {
                    EntityState::Valid
                };
                flow.entities
                    .insert(parameter.name.value.to_string(), state);
            }
        });
        self.statement_type(&definition.body, &mut flow);
    }

    fn class(&mut self, definition: &ClassDefinition<'_>) {
        for member in &definition.members {
            match &member.slot {
                Slot::Constructor { definition, .. } => self.function(None, definition),
                Slot::Function {
                    name, definition, ..
                } => self.function(Some(name.value), definition),
                Slot::Property { initializer, .. } | Slot::ComputedProperty { initializer, .. } => {
                    let mut flow = FlowState::default();
                    self.expression(&initializer.value, &mut flow);
                }
            }
        }
    }

    fn var_definition(&mut self, definition: &VarDefinitionStatement<'_>, flow: &mut FlowState) {
        for (variable, _) in &definition.definitions.items {
            self.variable(
                variable.name.value,
                variable.initializer.as_ref().map(|v| &*v.value),
                is_entity_type(Some(&definition.type_)),
                flow,
            );
        }
        let variable = &definition.definitions.last_item;
        self.variable(
            variable.name.value,
            variable.initializer.as_ref().map(|v| &*v.value),
            is_entity_type(Some(&definition.type_)),
            flow,
        );
    }

    fn variable(
        &mut self,
        name: &str,
        initializer: Option<&Expression<'_>>,
        declared_entity: bool,
        flow: &mut FlowState,
    ) {
        if let Some(initializer) = initializer {
            self.expression(initializer, flow);
        }
        if declared_entity
            || initializer.is_some_and(|value| entity_value_state(value, flow).is_some())
        {
            let state = initializer.map_or(EntityState::Null, |value| {
                entity_value_state(value, flow).unwrap_or(EntityState::Valid)
            });
            flow.entities.insert(name.to_string(), state);
        }
        if initializer.is_some_and(is_find_call) {
            flow.unchecked_find_indexes.insert(name.to_string());
        }
    }

    fn expression(&mut self, expression: &Expression<'_>, flow: &mut FlowState) {
        match expression {
            Expression::Parens(expression) => self.expression(&expression.value, flow),
            Expression::Index(expression) => {
                self.check_entity_use(&expression.base, flow);
                self.expression(&expression.base, flow);
                self.expression(&expression.index, flow);
                let unchecked_find = is_find_call(&expression.index)
                    || direct_var(&expression.index)
                        .is_some_and(|name| flow.unchecked_find_indexes.contains(name));
                let out_of_literal_bounds =
                    match (&*expression.base, integer_literal(&expression.index)) {
                        (Expression::Array(array), Some(index)) => {
                            index < 0 || index as usize >= array.values.len()
                        }
                        _ => false,
                    };
                if unchecked_find || out_of_literal_bounds {
                    self.diagnostic(
                        expression.open.range.start..expression.close.range.end,
                        UNSAFE_ARRAY_INDEX_RULE,
                        if unchecked_find {
                            "result of `find()` is used as an index without checking for not-found"
                                .to_string()
                        } else {
                            "array literal is indexed outside its bounds".to_string()
                        },
                    );
                }
            }
            Expression::Property(expression) => {
                self.check_entity_use(&expression.base, flow);
                self.expression(&expression.base, flow);
            }
            Expression::Ternary(expression) => {
                self.boolean_context(&expression.condition);
                self.expression(&expression.condition, flow);
                let (mut true_flow, mut false_flow) =
                    refined_condition(&expression.condition, flow);
                self.expression(&expression.true_value, &mut true_flow);
                self.expression(&expression.false_value, &mut false_flow);
                *flow = FlowState::join(&true_flow, &false_flow);
            }
            Expression::Binary(expression) => {
                if matches!(expression.operator, BinaryOperator::Assign(_))
                    && let Some(name) = direct_var(&expression.left)
                {
                    self.expression(&expression.right, flow);
                    let tracked_entity = flow.entities.contains_key(name);
                    if let Some(state) = entity_value_state(&expression.right, flow) {
                        flow.entities.insert(name.to_string(), state);
                    } else if tracked_entity {
                        flow.entities.insert(name.to_string(), EntityState::Valid);
                    }
                    flow.destroy_protected.remove(name);
                    if is_find_call(&expression.right) {
                        flow.unchecked_find_indexes.insert(name.to_string());
                    } else {
                        flow.unchecked_find_indexes.remove(name);
                    }
                } else if matches!(expression.operator, BinaryOperator::LogicalAnd(_)) {
                    self.expression(&expression.left, flow);
                    let (mut right_flow, _) = refined_condition(&expression.left, flow);
                    self.expression(&expression.right, &mut right_flow);
                    *flow = FlowState::join(flow, &right_flow);
                } else if matches!(expression.operator, BinaryOperator::LogicalOr(_)) {
                    self.expression(&expression.left, flow);
                    let (_, mut right_flow) = refined_condition(&expression.left, flow);
                    self.expression(&expression.right, &mut right_flow);
                    *flow = FlowState::join(flow, &right_flow);
                } else {
                    self.expression(&expression.left, flow);
                    self.expression(&expression.right, flow);
                }
            }
            Expression::Prefix(expression) => self.expression(&expression.value, flow),
            Expression::Postfix(expression) => self.expression(&expression.value, flow),
            Expression::Comma(expression) => {
                for (value, _) in &expression.values.items {
                    self.expression(value, flow);
                }
                self.expression(&expression.values.last_item, flow);
            }
            Expression::Table(expression) => {
                for slot in &expression.slots {
                    match &slot.ty {
                        TableSlotType::Slot(Slot::Property { initializer, .. }) => {
                            self.expression(&initializer.value, flow)
                        }
                        TableSlotType::Slot(Slot::ComputedProperty {
                            name, initializer, ..
                        }) => {
                            self.expression(name, flow);
                            self.expression(&initializer.value, flow);
                        }
                        TableSlotType::JsonProperty { value, .. } => self.expression(value, flow),
                        TableSlotType::Slot(Slot::Constructor { definition, .. }) => {
                            self.function(None, definition)
                        }
                        TableSlotType::Slot(Slot::Function {
                            name, definition, ..
                        }) => self.function(Some(name.value), definition),
                    }
                }
            }
            Expression::Class(expression) => self.class(&expression.definition),
            Expression::Array(expression) => {
                for value in &expression.values {
                    self.expression(&value.value, flow);
                }
            }
            Expression::Function(expression) => self.function(None, &expression.definition),
            Expression::Lambda(expression) => {
                let mut lambda_flow = FlowState::default();
                self.expression(&expression.value, &mut lambda_flow);
            }
            Expression::Call(call) => {
                self.call_facts(call);
                self.expression(&call.function, flow);
                for argument in &call.arguments {
                    self.expression(&argument.value, flow);
                }
                if let Some(name) = called_expression_name(&call.function) {
                    if name == "Assert"
                        && let Some(condition) = call.arguments.first()
                    {
                        *flow = refined_condition(&condition.value, flow).0;
                    }
                    if name == "EndSignal"
                        && call
                            .arguments
                            .iter()
                            .skip(1)
                            .any(|argument| string_literal(&argument.value) == Some("OnDestroy"))
                        && let Some(entity) = call
                            .arguments
                            .first()
                            .and_then(|argument| direct_var(&argument.value))
                    {
                        flow.destroy_protected.insert(entity.to_string());
                    }
                    if is_yielding_call(name) {
                        flow.mark_yield();
                    }
                }
                if let Expression::Property(property) = &*call.function
                    && method_name(&property.property) == Some("EndSignal")
                    && call
                        .arguments
                        .iter()
                        .any(|argument| string_literal(&argument.value) == Some("OnDestroy"))
                    && let Some(entity) = direct_var(&property.base)
                {
                    flow.destroy_protected.insert(entity.to_string());
                }
                if let Expression::Property(property) = &*call.function
                    && method_name(&property.property) == Some("Destroy")
                    && let Some(entity) = direct_var(&property.base)
                {
                    flow.entities
                        .insert(entity.to_string(), EntityState::Invalid);
                    flow.destroy_protected.remove(entity);
                }
                if let Some(post_initializer) = &call.post_initializer {
                    for slot in &post_initializer.slots {
                        if let TableSlotType::Slot(Slot::Property { initializer, .. }) = &slot.ty {
                            self.expression(&initializer.value, flow);
                        }
                    }
                }
            }
            Expression::Delegate(expression) => {
                self.expression(&expression.parent, flow);
                self.expression(&expression.value, flow);
            }
            Expression::Vector(expression) => {
                self.expression(&expression.x, flow);
                self.expression(&expression.y, flow);
                self.expression(&expression.z, flow);
            }
            Expression::Expect(expression) => self.expression(&expression.value, flow),
            Expression::Literal(_) | Expression::Var(_) | Expression::RootVar(_) => {}
        }
    }

    fn check_entity_use(&mut self, receiver: &Expression<'_>, flow: &mut FlowState) {
        let Some((state, range)) = entity_use_state(receiver, flow) else {
            return;
        };
        let (rule, message) = match state {
            EntityState::Valid | EntityState::PossiblyInvalid => return,
            EntityState::Null | EntityState::Invalid => (
                INVALID_ENTITY_RULE,
                "entity known to be invalid is dereferenced",
            ),
            EntityState::DecodedHandle => (
                UNCHECKED_ENCODED_EHANDLE_RULE,
                "entity decoded from an eHandle is dereferenced without an IsValid check",
            ),
            EntityState::AfterYield => (
                ENTITY_USE_AFTER_YIELD_RULE,
                "entity is dereferenced after suspension without revalidation or an OnDestroy end signal",
            ),
        };
        self.diagnostic(range, rule, message.to_string());
        if let Some(name) = direct_var(receiver) {
            flow.entities.insert(name.to_string(), EntityState::Valid);
        }
    }

    fn boolean_context(&mut self, expression: &Expression<'_>) {
        let expression = strip_parens(expression);
        if is_find_call(expression) {
            self.diagnostic(
                expression_range(expression),
                FIND_USED_AS_BOOLEAN_RULE,
                "`find()` returns an index or null/-1 and should be compared explicitly"
                    .to_string(),
            );
            return;
        }
        match expression {
            Expression::Prefix(expression)
                if matches!(expression.operator, PrefixOperator::LogicalNot(_)) =>
            {
                self.boolean_context(&expression.value)
            }
            Expression::Binary(expression)
                if matches!(
                    expression.operator,
                    BinaryOperator::LogicalAnd(_) | BinaryOperator::LogicalOr(_)
                ) =>
            {
                self.boolean_context(&expression.left);
                self.boolean_context(&expression.right);
            }
            _ => {}
        }
    }

    fn call_facts(&mut self, call: &sqparse::ast::CallExpression<'_>) {
        let Some(name) = called_expression_name(&call.function) else {
            return;
        };
        if name == "RegisterSignal"
            && let Some(signal) = call
                .arguments
                .first()
                .and_then(|argument| string_literal(&argument.value))
        {
            self.analysis.registered_signals.insert(signal.to_string());
        }
        if matches!(name, "Signal" | "EndSignal" | "WaitSignal") {
            let first_signal = if matches!(&*call.function, Expression::Property(_)) {
                0
            } else {
                1
            };
            for argument in call.arguments.iter().skip(first_signal) {
                let Some(signal) = string_literal(&argument.value) else {
                    continue;
                };
                self.analysis.signal_uses.push(SignalUse {
                    name: signal.to_string(),
                    range: expression_range(&argument.value),
                    kind: if name == "Signal" {
                        SignalUseKind::Emit
                    } else {
                        SignalUseKind::Consume
                    },
                });
            }
        }
        if name == "Remote_RegisterFunction"
            && let Some(remote) = call
                .arguments
                .first()
                .and_then(|argument| string_literal(&argument.value))
        {
            self.analysis
                .registered_remote_functions
                .insert(remote.to_string());
        }
        if matches!(
            name,
            "Remote_CallFunction_NonReplay"
                | "Remote_CallFunction_Replay"
                | "Remote_CallFunction_UI"
        ) && let Some(remote) = call
            .arguments
            .get(1)
            .and_then(|argument| string_literal(&argument.value))
        {
            self.analysis.remote_calls.push(RemoteCall {
                name: remote.to_string(),
                arguments: call.arguments.len().saturating_sub(2),
                range: expression_range(&call.arguments[1].value),
            });
        }
    }

    fn polling_loop(&mut self, condition: &Expression<'_>, body: &StatementType<'_>) {
        if super::expression_truth(condition) == Some(false) {
            return;
        }
        self.polling_loop_without_condition(body);
    }

    fn polling_loop_without_condition(&mut self, body: &StatementType<'_>) {
        if !contains_reachable_wait(body) {
            return;
        }
        let mut spawns = Vec::new();
        collect_thread_spawns(body, &mut spawns);
        for range in spawns {
            self.diagnostic(
                range,
                THREAD_IN_POLLING_LOOP_RULE,
                "polling loop may start overlapping threads on successive iterations".to_string(),
            );
        }
    }
}

fn refined_condition(expression: &Expression<'_>, flow: &FlowState) -> (FlowState, FlowState) {
    let expression = strip_parens(expression);
    if let Some((name, invalid_when_false)) = entity_check_call(expression) {
        let mut true_flow = flow.clone();
        let mut false_flow = flow.clone();
        true_flow
            .entities
            .insert(name.to_string(), EntityState::Valid);
        if invalid_when_false {
            false_flow
                .entities
                .insert(name.to_string(), EntityState::Invalid);
        }
        return (true_flow, false_flow);
    }
    if let Some(name) = direct_var(expression)
        && flow.entities.get(name) == Some(&EntityState::Null)
    {
        let mut true_flow = flow.clone();
        true_flow
            .entities
            .insert(name.to_string(), EntityState::PossiblyInvalid);
        return (true_flow, flow.clone());
    }
    match expression {
        Expression::Prefix(expression)
            if matches!(expression.operator, PrefixOperator::LogicalNot(_)) =>
        {
            let (true_flow, false_flow) = refined_condition(&expression.value, flow);
            (false_flow, true_flow)
        }
        Expression::Binary(expression) => match expression.operator {
            BinaryOperator::LogicalAnd(_) => {
                let (left_true, _) = refined_condition(&expression.left, flow);
                let (right_true, _) = refined_condition(&expression.right, &left_true);
                (right_true, flow.clone())
            }
            BinaryOperator::LogicalOr(_) => {
                let (_, left_false) = refined_condition(&expression.left, flow);
                let (_, right_false) = refined_condition(&expression.right, &left_false);
                (flow.clone(), right_false)
            }
            BinaryOperator::NotEqual(_) | BinaryOperator::Equal(_) => {
                refine_find_index_check(expression, flow)
                    .unwrap_or_else(|| (flow.clone(), flow.clone()))
            }
            BinaryOperator::Less(_)
            | BinaryOperator::LessEqual(_)
            | BinaryOperator::Greater(_)
            | BinaryOperator::GreaterEqual(_) => refine_find_index_check(expression, flow)
                .unwrap_or_else(|| (flow.clone(), flow.clone())),
            _ => (flow.clone(), flow.clone()),
        },
        _ => (flow.clone(), flow.clone()),
    }
}

fn collect_thread_spawns(statement: &StatementType<'_>, out: &mut Vec<std::ops::Range<usize>>) {
    match statement {
        StatementType::Thread(statement) => out.push(statement.thread.range.clone()),
        StatementType::DelayThread(statement) => out.push(statement.delay_thread.range.clone()),
        StatementType::Block(block) => {
            for statement in &block.statements {
                collect_thread_spawns(&statement.ty, out);
            }
        }
        StatementType::If(statement) => match &statement.ty {
            IfStatementType::NoElse { body } => collect_thread_spawns(body, out),
            IfStatementType::Else {
                body, else_body, ..
            } => {
                collect_thread_spawns(&body.ty, out);
                collect_thread_spawns(else_body, out);
            }
        },
        StatementType::Switch(statement) => {
            for case in &statement.cases {
                for statement in &case.body {
                    collect_thread_spawns(&statement.ty, out);
                }
            }
        }
        StatementType::TryCatch(statement) => {
            collect_thread_spawns(&statement.body.ty, out);
            collect_thread_spawns(&statement.catch_body, out);
        }
        StatementType::While(_)
        | StatementType::DoWhile(_)
        | StatementType::For(_)
        | StatementType::Foreach(_)
        | StatementType::ConstructorDefinition(_)
        | StatementType::FunctionDefinition(_)
        | StatementType::ClassDefinition(_) => {}
        _ => {}
    }
}

fn entity_use_state(
    expression: &Expression<'_>,
    flow: &FlowState,
) -> Option<(EntityState, std::ops::Range<usize>)> {
    if let Some(name) = direct_var(expression)
        && let Some(state) = flow.entities.get(name)
    {
        return Some((*state, expression_range(expression)));
    }
    entity_value_state(expression, flow).map(|state| (state, expression_range(expression)))
}

fn entity_value_state(expression: &Expression<'_>, flow: &FlowState) -> Option<EntityState> {
    let expression = strip_parens(expression);
    if is_null(expression) {
        return Some(EntityState::Null);
    }
    if let Some(name) = direct_var(expression) {
        return flow.entities.get(name).copied();
    }
    let Expression::Call(call) = expression else {
        return None;
    };
    match called_expression_name(&call.function) {
        Some("GetEntityFromEncodedEHandle" | "GetHeavyWeightEntityFromEncodedEHandle") => {
            Some(EntityState::DecodedHandle)
        }
        Some(
            "GetOffhandWeapon"
            | "GetActiveWeapon"
            | "GetLatestPrimaryWeapon"
            | "GetPetTitan"
            | "GetEntByIndex",
        ) => Some(EntityState::PossiblyInvalid),
        Some(name) if name.starts_with("Create") || name.starts_with("Spawn") => {
            Some(EntityState::Valid)
        }
        _ => None,
    }
}

fn entity_check_call<'s>(expression: &Expression<'s>) -> Option<(&'s str, bool)> {
    let Expression::Call(call) = strip_parens(expression) else {
        return None;
    };
    let check = called_expression_name(&call.function)?;
    let entity = call
        .arguments
        .first()
        .and_then(|argument| direct_var(&argument.value))?;
    match check {
        "IsValid" => Some((entity, true)),
        "IsAlive" => Some((entity, false)),
        _ => None,
    }
}

fn refine_find_index_check(
    expression: &sqparse::ast::BinaryExpression<'_>,
    flow: &FlowState,
) -> Option<(FlowState, FlowState)> {
    let (variable, variable_on_left) = if direct_var(&expression.left)
        .is_some_and(|name| flow.unchecked_find_indexes.contains(name))
    {
        (direct_var(&expression.left)?, true)
    } else if direct_var(&expression.right)
        .is_some_and(|name| flow.unchecked_find_indexes.contains(name))
    {
        (direct_var(&expression.right)?, false)
    } else {
        return None;
    };
    let other = if variable_on_left {
        &expression.right
    } else {
        &expression.left
    };
    let is_not_found = is_null(other) || integer_literal(other) == Some(-1);
    let is_zero = integer_literal(other) == Some(0);
    let safe_on_true = match expression.operator {
        BinaryOperator::NotEqual(_) if is_not_found => true,
        BinaryOperator::Equal(_) if is_not_found => false,
        BinaryOperator::GreaterEqual(_) if is_zero && variable_on_left => true,
        BinaryOperator::Less(_) if is_zero && variable_on_left => false,
        BinaryOperator::LessEqual(_) if is_zero && !variable_on_left => true,
        BinaryOperator::Greater(_) if is_zero && !variable_on_left => false,
        _ => return None,
    };
    let mut safe_flow = flow.clone();
    safe_flow.unchecked_find_indexes.remove(variable);
    Some(if safe_on_true {
        (safe_flow, flow.clone())
    } else {
        (flow.clone(), safe_flow)
    })
}

fn is_find_call(expression: &Expression<'_>) -> bool {
    let Expression::Call(call) = strip_parens(expression) else {
        return false;
    };
    matches!(
        &*call.function,
        Expression::Property(property) if method_name(&property.property) == Some("find")
    )
}

fn direct_var<'s>(expression: &Expression<'s>) -> Option<&'s str> {
    match strip_parens(expression) {
        Expression::Var(variable) => Some(variable.name.value),
        _ => None,
    }
}

fn strip_parens<'a, 's>(mut expression: &'a Expression<'s>) -> &'a Expression<'s> {
    while let Expression::Parens(parens) = expression {
        expression = &parens.value;
    }
    expression
}

fn string_literal<'s>(expression: &Expression<'s>) -> Option<&'s str> {
    match strip_parens(expression) {
        Expression::Literal(literal) => match literal.literal {
            LiteralToken::String(StringToken::Literal(value) | StringToken::Verbatim(value)) => {
                Some(value)
            }
            _ => None,
        },
        _ => None,
    }
}

fn numeric_zero(expression: &Expression<'_>) -> bool {
    match strip_parens(expression) {
        Expression::Literal(literal) => match literal.literal {
            LiteralToken::Int(0, _) => true,
            LiteralToken::Float(value) => value == 0.0,
            _ => false,
        },
        _ => false,
    }
}

fn integer_literal(expression: &Expression<'_>) -> Option<i64> {
    match strip_parens(expression) {
        Expression::Literal(literal) => match literal.literal {
            LiteralToken::Int(value, _) => Some(value),
            _ => None,
        },
        Expression::Prefix(prefix) if matches!(prefix.operator, PrefixOperator::Negate(_)) => {
            integer_literal(&prefix.value).map(|value| -value)
        }
        _ => None,
    }
}

fn is_null(expression: &Expression<'_>) -> bool {
    matches!(strip_parens(expression), Expression::Var(variable) if variable.name.value == "null")
}

fn is_yielding_call(name: &str) -> bool {
    name == "WaitFrame"
        || name == "WaitEndFrame"
        || name == "WaitSignal"
        || name.starts_with("FlagWait")
}

fn is_entity_type(type_: Option<&Type<'_>>) -> bool {
    match type_ {
        Some(Type::Plain(type_)) => type_.name.value == "entity",
        Some(Type::Reference(type_)) => is_entity_type(Some(&type_.base)),
        Some(Type::Nullable(type_)) => is_entity_type(Some(&type_.base)),
        _ => false,
    }
}

fn is_nullable_entity_type(type_: &Type<'_>) -> bool {
    matches!(type_, Type::Nullable(nullable) if is_entity_type(Some(&nullable.base)))
}

fn for_each_parameter<'s>(
    params: &'s FunctionParams<'s>,
    mut visit: impl FnMut(&'s FunctionParam<'s>),
) {
    match params {
        FunctionParams::NonVariable {
            params: Some(params),
        } => {
            for (param, _) in &params.items {
                visit(param);
            }
            visit(&params.last_item);
        }
        FunctionParams::NonEmptyVariable { params, .. } => {
            for (param, _) in &params.items {
                visit(param);
            }
            visit(&params.last_item);
        }
        FunctionParams::NonVariable { params: None } | FunctionParams::EmptyVariable { .. } => {}
    }
}

fn parameter_arity(params: &FunctionParams<'_>) -> (usize, Option<usize>) {
    let mut required = 0;
    let mut total = 0;
    for_each_parameter(params, |parameter| {
        total += 1;
        if parameter.initializer.is_none() {
            required += 1;
        }
    });
    let variadic = matches!(
        params,
        FunctionParams::EmptyVariable { .. } | FunctionParams::NonEmptyVariable { .. }
    );
    (required, (!variadic).then_some(total))
}

fn method_name<'s>(identifier: &MethodIdentifier<'s>) -> Option<&'s str> {
    match identifier {
        MethodIdentifier::Identifier(identifier) => Some(identifier.value),
        MethodIdentifier::Constructor(_) => None,
    }
}

fn expression_range(expression: &Expression<'_>) -> std::ops::Range<usize> {
    match expression {
        Expression::Parens(expression) => expression.open.range.start..expression.close.range.end,
        Expression::Literal(expression) => expression.token.range.clone(),
        Expression::Var(expression) => expression.name.token.range.clone(),
        Expression::RootVar(expression) => {
            expression.root.range.start..expression.name.token.range.end
        }
        Expression::Index(expression) => {
            expression_range(&expression.base).start..expression.close.range.end
        }
        Expression::Property(expression) => {
            let end = match &expression.property {
                MethodIdentifier::Identifier(identifier) => identifier.token.range.end,
                MethodIdentifier::Constructor(token) => token.range.end,
            };
            expression_range(&expression.base).start..end
        }
        Expression::Ternary(expression) => {
            expression_range(&expression.condition).start
                ..expression_range(&expression.false_value).end
        }
        Expression::Binary(expression) => {
            expression_range(&expression.left).start..expression_range(&expression.right).end
        }
        Expression::Prefix(expression) => {
            let start = match expression.operator {
                PrefixOperator::Negate(token)
                | PrefixOperator::LogicalNot(token)
                | PrefixOperator::BitwiseNot(token)
                | PrefixOperator::Typeof(token)
                | PrefixOperator::Clone(token)
                | PrefixOperator::Delete(token)
                | PrefixOperator::Increment(token)
                | PrefixOperator::Decrement(token) => token.range.start,
            };
            start..expression_range(&expression.value).end
        }
        Expression::Postfix(expression) => expression_range(&expression.value),
        Expression::Comma(expression) => {
            expression_range(&expression.values.items[0].0).start
                ..expression_range(&expression.values.last_item).end
        }
        Expression::Table(expression) => expression.open.range.start..expression.close.range.end,
        Expression::Class(expression) => {
            expression.class.range.start..expression.definition.close.range.end
        }
        Expression::Array(expression) => expression.open.range.start..expression.close.range.end,
        Expression::Function(expression) => {
            expression.function.range.start..statement_type_end(&expression.definition.body)
        }
        Expression::Lambda(expression) => {
            expression.at.range.start..expression_range(&expression.value).end
        }
        Expression::Call(expression) => {
            expression_range(&expression.function).start
                ..expression
                    .post_initializer
                    .as_ref()
                    .map_or(expression.close.range.end, |table| table.close.range.end)
        }
        Expression::Delegate(expression) => {
            expression.delegate.range.start..expression_range(&expression.value).end
        }
        Expression::Vector(expression) => expression.open.range.start..expression.close.range.end,
        Expression::Expect(expression) => expression.expect.range.start..expression.close.range.end,
    }
}

fn statement_type_end(statement: &StatementType<'_>) -> usize {
    match statement {
        StatementType::Block(block) => block.close.range.end,
        _ => 0,
    }
}
