use crate::combinators::{
    alt, empty_line, format, indented, iter, opt, pair, single_line, space, tuple,
};
use crate::expression::{expression, function_definition, table_expression};
use crate::shared::identifier;
use crate::token::{
    discard_token, token, token_before_lines_only, token_ignoring_blank_lines,
    token_without_before_lines,
};
use crate::type_format::type_format;
use crate::writer::Writer;
use sqparse::ast::*;

pub fn program<'s>(prog: &'s Program<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = iter(
            prog.statements
                .iter()
                .map(|s| pair(empty_line, statement(s))),
        )(i)?;
        Some(i)
    }
}

fn statement<'s>(stmt: &'s Statement<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = statement_type(&stmt.ty)(i)?;
        let i = opt(stmt.semicolon, |t| discard_token(t))(i)?;
        Some(i)
    }
}

fn statement_type<'s>(ty: &'s StatementType<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match ty {
        StatementType::Empty(s) => opt(s.empty, token)(i),
        StatementType::Block(s) => block_statement(s)(i),
        StatementType::If(s) => if_statement(s)(i),
        StatementType::While(s) => while_statement(s)(i),
        StatementType::DoWhile(s) => do_while_statement(s)(i),
        StatementType::Switch(s) => switch_statement(s)(i),
        StatementType::For(s) => for_statement(s)(i),
        StatementType::Foreach(s) => foreach_statement(s)(i),
        StatementType::Break(s) => token(s.break_)(i),
        StatementType::Continue(s) => token(s.continue_)(i),
        StatementType::Return(s) => return_statement(s)(i),
        StatementType::Yield(s) => yield_statement(s)(i),
        StatementType::VarDefinition(s) => var_definition_statement(s)(i),
        StatementType::ConstructorDefinition(s) => constructor_definition_statement(s)(i),
        StatementType::FunctionDefinition(s) => function_definition_statement(s)(i),
        StatementType::ClassDefinition(s) => class_definition_statement(s)(i),
        StatementType::TryCatch(s) => try_catch_statement(s)(i),
        StatementType::Throw(s) => tuple((token(s.throw), space, expression(&s.value)))(i),
        StatementType::Const(s) => const_definition_statement(s)(i),
        StatementType::EnumDefinition(s) => enum_definition_statement(s)(i),
        StatementType::Expression(s) => expression(&s.value)(i),
        StatementType::Thread(s) => tuple((token(s.thread), space, expression(&s.value)))(i),
        StatementType::DelayThread(s) => delay_thread_statement(s)(i),
        StatementType::WaitThread(s) => {
            tuple((token(s.wait_thread), space, expression(&s.value)))(i)
        }
        StatementType::WaitThreadSolo(s) => {
            tuple((token(s.wait_thread_solo), space, expression(&s.value)))(i)
        }
        StatementType::Wait(s) => tuple((token(s.wait), space, expression(&s.value)))(i),
        StatementType::StructDefinition(s) => {
            let i = tuple((token(s.struct_), space, identifier(&s.name)))(i)?;
            pair(empty_line, struct_definition(&s.definition))(i)
        }
        StatementType::TypeDefinition(s) => tuple((
            token(s.typedef),
            space,
            identifier(&s.name),
            space,
            type_format(&s.type_),
        ))(i),
        StatementType::Global(s) => global_statement(s)(i),
        StatementType::GlobalizeAllFunctions(s) => token(s.globalize_all_functions)(i),
        StatementType::Untyped(s) => token(s.untyped)(i),
    }
}

/// Formats a statement body (the part after `if (...)`, `while (...)`, etc.)
/// This wraps it in a block if it's not already one.
pub fn statement_type_body<'s>(
    body: &'s StatementType<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match body {
        StatementType::Block(b) => {
            // Allman style: brace on new line
            pair(empty_line, block_statement(b))(i)
        }
        StatementType::Empty(s) => opt(s.empty, token)(i),
        _ => {
            // Single-statement body: indent on next line
            indented(pair(empty_line, statement_type(body)))(i)
        }
    }
}

fn block_statement<'s>(stmt: &'s BlockStatement<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        if stmt.statements.is_empty() {
            let close_has_comments = stmt
                .close
                .before_lines
                .iter()
                .any(|l| !l.comments.is_empty());
            if close_has_comments {
                // Empty block with comments: use token() which preserves before_lines comments,
                // but go through the non-empty path to get proper indented layout
                let i = token(stmt.open)(i)?;
                let i = empty_line(i)?;
                return token_ignoring_blank_lines(stmt.close)(i);
            }
            let i = token(stmt.open)(i)?;
            let i = empty_line(i)?;
            return token_ignoring_blank_lines(stmt.close)(i);
        }
        let i = token(stmt.open)(i)?;
        let i = indented(|i| {
            let i = iter(
                stmt.statements
                    .iter()
                    .map(|s| pair(empty_line, statement(s))),
            )(i)?;
            // Process close brace's before_lines inside the indented context so
            // preprocessor directives like #endif emit at the correct depth.
            token_before_lines_only(stmt.close)(i)
        })(i)?;
        let i = empty_line(i)?;
        token_without_before_lines(stmt.close)(i)
    }
}

fn if_statement<'s>(stmt: &'s IfStatement<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = tuple((token(stmt.if_), space, token(stmt.open)))(i)?;
        let i = alt(
            single_line(tuple((
                format(|f| f.spaces_in_expr_brackets, space),
                expression(&stmt.condition),
                format(|f| f.spaces_in_expr_brackets, space),
            ))),
            tuple((
                indented(pair(empty_line, expression(&stmt.condition))),
                empty_line,
            )),
        )(i)?;
        let i = token(stmt.close)(i)?;
        match &stmt.ty {
            IfStatementType::NoElse { body } => statement_type_body(body)(i),
            IfStatementType::Else {
                body,
                else_,
                else_body,
            } => {
                let i = statement_type_body_for_else(body)(i)?;
                let i = empty_line(i)?;
                let i = token_ignoring_blank_lines(else_)(i)?;
                match else_body.as_ref() {
                    StatementType::If(if_stmt) => {
                        let i = space(i)?;
                        if_statement(if_stmt)(i)
                    }
                    _ => statement_type_body(else_body)(i),
                }
            }
        }
    }
}

/// Format body before an `else` clause. In Allman style, `else` goes on its own line.
fn statement_type_body_for_else<'s>(
    body: &'s Statement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match &body.ty {
        StatementType::Block(b) => {
            let i = empty_line(i)?;
            let i = block_statement(b)(i)?;
            opt(body.semicolon, |t| discard_token(t))(i)
        }
        _ => {
            let i = indented(pair(empty_line, statement(body)))(i)?;
            empty_line(i)
        }
    }
}

fn while_statement<'s>(stmt: &'s WhileStatement<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = tuple((token(stmt.while_), space, token(stmt.open)))(i)?;
        let i = alt(
            single_line(tuple((
                format(|f| f.spaces_in_expr_brackets, space),
                expression(&stmt.condition),
                format(|f| f.spaces_in_expr_brackets, space),
            ))),
            tuple((
                indented(pair(empty_line, expression(&stmt.condition))),
                empty_line,
            )),
        )(i)?;
        let i = token(stmt.close)(i)?;
        statement_type_body(&stmt.body)(i)
    }
}

fn do_while_statement<'s>(
    stmt: &'s DoWhileStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = token(stmt.do_)(i)?;
        let i = match &stmt.body.ty {
            StatementType::Block(b) => {
                let i = empty_line(i)?;
                let i = block_statement(b)(i)?;
                opt(stmt.body.semicolon, |t| discard_token(t))(i)?
            }
            _ => {
                let i = indented(pair(empty_line, statement(&stmt.body)))(i)?;
                empty_line(i)?
            }
        };
        let i = empty_line(i)?;
        let i = token(stmt.while_)(i)?;
        let i = space(i)?;
        let i = token(stmt.open)(i)?;
        let i = format(|f| f.spaces_in_expr_brackets, space)(i)?;
        let i = expression(&stmt.condition)(i)?;
        let i = format(|f| f.spaces_in_expr_brackets, space)(i)?;
        token(stmt.close)(i)
    }
}

fn switch_statement<'s>(
    stmt: &'s SwitchStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = tuple((
            token(stmt.switch),
            space,
            token(stmt.open_condition),
            format(|f| f.spaces_in_expr_brackets, space),
            expression(&stmt.condition),
            format(|f| f.spaces_in_expr_brackets, space),
            token(stmt.close_condition),
        ))(i)?;
        let i = empty_line(i)?;
        let i = token(stmt.open_cases)(i)?;
        let i = indented(|i| {
            iter(stmt.cases.iter().map(|case| {
                move |i: Writer| {
                    let i = empty_line(i)?;
                    let i = match &case.condition {
                        SwitchCaseCondition::Default { default } => token(default)(i)?,
                        SwitchCaseCondition::Case { case, value } => {
                            tuple((token(case), space, expression(value)))(i)?
                        }
                    };
                    let i = token(case.colon)(i)?;
                    indented(|i| iter(case.body.iter().map(|s| pair(empty_line, statement(s))))(i))(
                        i,
                    )
                }
            }))(i)
        })(i)?;
        let i = empty_line(i)?;
        token(stmt.close_cases)(i)
    }
}

fn for_statement<'s>(stmt: &'s ForStatement<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = tuple((
            token(stmt.for_),
            space,
            token(stmt.open),
            format(|f| f.spaces_in_expr_brackets, space),
        ))(i)?;
        let i = opt(stmt.initializer.as_ref(), |init| {
            move |i: Writer| match init {
                ForDefinition::Expression(expr) => expression(expr)(i),
                ForDefinition::Definition(def) => var_definition_statement(def)(i),
            }
        })(i)?;
        let i = token(stmt.semicolon_1)(i)?;
        let i = space(i)?;
        let i = opt(stmt.condition.as_deref(), expression)(i)?;
        let i = token(stmt.semicolon_2)(i)?;
        let i = space(i)?;
        let i = opt(stmt.increment.as_deref(), expression)(i)?;
        let i = format(|f| f.spaces_in_expr_brackets, space)(i)?;
        let i = token(stmt.close)(i)?;
        statement_type_body(&stmt.body)(i)
    }
}

fn foreach_statement<'s>(
    stmt: &'s ForeachStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = tuple((
            token(stmt.foreach),
            space,
            token(stmt.open),
            format(|f| f.spaces_in_expr_brackets, space),
        ))(i)?;
        let i = opt(stmt.index.as_ref(), |idx| {
            tuple((
                opt(idx.type_.as_ref(), |ty| pair(type_format(ty), space)),
                identifier(&idx.name),
                token(idx.comma),
                space,
            ))
        })(i)?;
        let i = opt(stmt.value_type.as_ref(), |ty| pair(type_format(ty), space))(i)?;
        let i = identifier(&stmt.value_name)(i)?;
        let i = tuple((space, token(stmt.in_), space, expression(&stmt.array)))(i)?;
        let i = format(|f| f.spaces_in_expr_brackets, space)(i)?;
        let i = token(stmt.close)(i)?;
        statement_type_body(&stmt.body)(i)
    }
}

fn return_statement<'s>(
    stmt: &'s ReturnStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = token(stmt.return_)(i)?;
        opt(stmt.value.as_deref(), |val| pair(space, expression(val)))(i)
    }
}

fn yield_statement<'s>(stmt: &'s YieldStatement<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = token(stmt.yield_)(i)?;
        opt(stmt.value.as_deref(), |val| pair(space, expression(val)))(i)
    }
}

fn var_definition_statement<'s>(
    stmt: &'s VarDefinitionStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = type_format(&stmt.type_)(i)?;
        let i = space(i)?;
        let i = var_definition_list(&stmt.definitions)(i)?;
        Some(i)
    }
}

fn var_definition_list<'s>(
    list: &'s SeparatedListTrailing1<'s, VarDefinition<'s>>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = iter(
            list.items
                .iter()
                .map(|(def, sep)| tuple((var_definition(def), token(sep), space))),
        )(i)?;
        let i = var_definition(&list.last_item)(i)?;
        opt(list.trailing, token)(i)
    }
}

fn var_definition<'s>(def: &'s VarDefinition<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = identifier(&def.name)(i)?;
        opt(def.initializer.as_ref(), |init| {
            tuple((space, token(init.assign), space, expression(&init.value)))
        })(i)
    }
}

fn constructor_definition_statement<'s>(
    stmt: &'s ConstructorDefinitionStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = token(stmt.function)(i)?;
        let i = space(i)?;
        let i = iter(
            stmt.namespaces
                .iter()
                .map(|(name, sep)| pair(identifier(name), token(sep))),
        )(i)?;
        let i = identifier(&stmt.last_name)(i)?;
        let i = token(stmt.last_namespace)(i)?;
        let i = token(stmt.constructor)(i)?;
        function_definition(&stmt.definition)(i)
    }
}

fn function_definition_statement<'s>(
    stmt: &'s FunctionDefinitionStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = opt(stmt.return_type.as_ref(), |rt| pair(type_format(rt), space))(i)?;
        let i = token(stmt.function)(i)?;
        let i = space(i)?;
        // name is SeparatedList1<Identifier> separated by ::
        let i = iter(
            stmt.name
                .items
                .iter()
                .map(|(name, sep)| pair(identifier(name), token(sep))),
        )(i)?;
        let i = identifier(&stmt.name.last_item)(i)?;
        function_definition(&stmt.definition)(i)
    }
}

fn class_definition_statement<'s>(
    stmt: &'s ClassDefinitionStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    tuple((
        token(stmt.class),
        space,
        expression(&stmt.name),
        space,
        class_definition(&stmt.definition),
    ))
}

pub fn class_definition<'s>(
    def: &'s ClassDefinition<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = opt(def.extends.as_ref(), |ext| {
            tuple((token(ext.extends), space, expression(&ext.name)))
        })(i)?;
        let i = empty_line(i)?;
        let i = token(def.open)(i)?;
        if def.members.is_empty() {
            return token(def.close)(i);
        }
        let i = indented(|i| {
            iter(def.members.iter().map(|member| {
                move |i: Writer| {
                    let i = empty_line(i)?;
                    let i = opt(member.attributes.as_ref(), |attrs| {
                        pair(table_expression(attrs), empty_line)
                    })(i)?;
                    let i = opt(member.static_, |t| pair(token(t), space))(i)?;
                    let i = slot(&member.slot)(i)?;
                    opt(member.semicolon, |t| discard_token(t))(i)
                }
            }))(i)
        })(i)?;
        let i = empty_line(i)?;
        token(def.close)(i)
    }
}

pub fn slot<'s>(s: &'s Slot<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match s {
        Slot::Property { name, initializer } => tuple((
            identifier(name),
            space,
            token(initializer.assign),
            space,
            expression(&initializer.value),
        ))(i),
        Slot::ComputedProperty {
            open,
            name,
            close,
            initializer,
        } => tuple((
            token(open),
            expression(name),
            token(close),
            space,
            token(initializer.assign),
            space,
            expression(&initializer.value),
        ))(i),
        Slot::Constructor {
            function,
            constructor,
            definition,
        } => {
            let i = opt(*function, |t| pair(token(t), space))(i)?;
            let i = token(constructor)(i)?;
            function_definition(definition)(i)
        }
        Slot::Function {
            return_type,
            function,
            name,
            definition,
        } => {
            let i = opt(return_type.as_ref(), |rt| pair(type_format(rt), space))(i)?;
            let i = token(function)(i)?;
            let i = space(i)?;
            let i = identifier(name)(i)?;
            function_definition(definition)(i)
        }
    }
}

fn try_catch_statement<'s>(
    stmt: &'s TryCatchStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = token(stmt.try_)(i)?;
        let i = match &stmt.body.ty {
            StatementType::Block(b) => {
                let i = empty_line(i)?;
                let i = block_statement(b)(i)?;
                opt(stmt.body.semicolon, |t| discard_token(t))(i)?
            }
            _ => {
                let i = indented(pair(empty_line, statement(&stmt.body)))(i)?;
                empty_line(i)?
            }
        };
        let i = empty_line(i)?;
        let i = tuple((
            token(stmt.catch),
            space,
            token(stmt.open),
            format(|f| f.spaces_in_expr_brackets, space),
            identifier(&stmt.catch_name),
            format(|f| f.spaces_in_expr_brackets, space),
            token(stmt.close),
        ))(i)?;
        statement_type_body(&stmt.catch_body)(i)
    }
}

fn const_definition_statement<'s>(
    stmt: &'s ConstDefinitionStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = token(stmt.const_)(i)?;
        let i = space(i)?;
        let i = opt(stmt.const_type.as_ref(), |ty| pair(type_format(ty), space))(i)?;
        let i = identifier(&stmt.name)(i)?;
        tuple((
            space,
            token(stmt.initializer.assign),
            space,
            expression(&stmt.initializer.value),
        ))(i)
    }
}

fn enum_definition_statement<'s>(
    stmt: &'s EnumDefinitionStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = tuple((token(stmt.enum_), space, identifier(&stmt.name)))(i)?;
        let i = empty_line(i)?;
        let i = token(stmt.open)(i)?;
        if stmt.entries.is_empty() {
            return token(stmt.close)(i);
        }
        let i = indented(|i| {
            iter(stmt.entries.iter().map(|entry| {
                move |i: Writer| {
                    let i = empty_line(i)?;
                    let i = identifier(&entry.name)(i)?;
                    let i = opt(entry.initializer.as_ref(), |init| {
                        tuple((space, token(init.assign), space, expression(&init.value)))
                    })(i)?;
                    opt(entry.comma, token)(i)
                }
            }))(i)
        })(i)?;
        let i = empty_line(i)?;
        token(stmt.close)(i)
    }
}

fn delay_thread_statement<'s>(
    stmt: &'s DelayThreadStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    tuple((
        token(stmt.delay_thread),
        token(stmt.open),
        format(|f| f.spaces_in_expr_brackets, space),
        expression(&stmt.duration),
        format(|f| f.spaces_in_expr_brackets, space),
        token(stmt.close),
        space,
        expression(&stmt.value),
    ))
}

pub fn struct_definition<'s>(
    def: &'s StructDefinition<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = token(def.open)(i)?;
        if def.properties.is_empty() {
            return token(def.close)(i);
        }
        let i = indented(|i| {
            iter(def.properties.iter().map(|prop| {
                move |i: Writer| {
                    let i = empty_line(i)?;
                    let i = type_format(&prop.type_)(i)?;
                    let i = space(i)?;
                    let i = identifier(&prop.name)(i)?;
                    let i = opt(prop.initializer.as_ref(), |init| {
                        tuple((space, token(init.assign), space, expression(&init.value)))
                    })(i)?;
                    opt(prop.comma, |t| discard_token(t))(i)
                }
            }))(i)
        })(i)?;
        let i = empty_line(i)?;
        token(def.close)(i)
    }
}

fn global_statement<'s>(
    stmt: &'s GlobalStatement<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = token(stmt.global)(i)?;
        let i = space(i)?;
        match &stmt.definition {
            GlobalDefinition::Function { function, name } => {
                tuple((token(function), space, identifier(name)))(i)
            }
            GlobalDefinition::UntypedVar { name, initializer } => tuple((
                identifier(name),
                space,
                token(initializer.assign),
                space,
                expression(&initializer.value),
            ))(i),
            GlobalDefinition::TypedVar(def) => var_definition_statement(def)(i),
            GlobalDefinition::Const(def) => const_definition_statement(def)(i),
            GlobalDefinition::Enum(def) => enum_definition_statement(def)(i),
            GlobalDefinition::Class(def) => class_definition_statement(def)(i),
            GlobalDefinition::Struct(def) => {
                let i = tuple((token(def.struct_), space, identifier(&def.name)))(i)?;
                pair(empty_line, struct_definition(&def.definition))(i)
            }
            GlobalDefinition::Type(def) => tuple((
                token(def.typedef),
                space,
                identifier(&def.name),
                space,
                type_format(&def.type_),
            ))(i),
        }
    }
}
