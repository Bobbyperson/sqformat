use crate::combinators::{
    allow_trailing, alt, cond, empty_line, format, indented, iter, opt, pair, single_line, space,
    tuple,
};
use crate::operator::{binary_operator, postfix_operator, prefix_needs_space, prefix_operator};
use crate::shared::{identifier, optional_separator, token_or_tag};
use crate::token::{
    discard_token, token, token_ignoring_blank_lines, token_trailing, token_without_trailing,
};
use crate::type_format::type_format;
use crate::writer::Writer;
use sqparse::ast::{
    ArrayExpression, CallExpression, CommaExpression, DelegateExpression, ExpectExpression,
    Expression, FunctionDefinition, FunctionExpression, FunctionParams, IndexExpression,
    LambdaExpression, LiteralExpression, ParensExpression, PropertyExpression, RootVarExpression,
    TableExpression, TableSlot, TableSlotType, TernaryExpression, VectorExpression,
};

pub fn expression<'s>(expr: &'s Expression<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |mut i| {
        if let Expression::Parens(parens_expr) = expr {
            let mut parens_expressions = Vec::new();
            let mut inner_parens_expression = parens_expr;
            while let Expression::Parens(inner) = inner_parens_expression.value.as_ref() {
                parens_expressions.push(inner_parens_expression);
                inner_parens_expression = inner;
            }
            for discard_expr in &parens_expressions {
                i = discard_token(discard_expr.open)(i)?;
            }
            i = parens_expression(inner_parens_expression)(i)?;
            for discard_expr in parens_expressions.iter().rev() {
                i = discard_token(discard_expr.close)(i)?;
            }
            return Some(i);
        }
        expression_without_parens(expr)(i)
    }
}

fn expression_without_parens<'s>(
    expr: &'s Expression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match expr {
        Expression::Parens(_) => unreachable!("Parens should be handled in `expression`"),
        Expression::Literal(l) => literal_expression(l)(i),
        Expression::Var(v) => identifier(&v.name)(i),
        Expression::RootVar(r) => root_var_expression(r)(i),
        Expression::Table(t) => table_expression(t)(i),
        Expression::Class(c) => tuple((
            token(c.class),
            space,
            crate::statement::class_definition(&c.definition),
        ))(i),
        Expression::Array(a) => array_expression(a)(i),
        Expression::Index(idx) => index_expression(idx)(i),
        Expression::Property(p) => property_expression(p)(i),
        Expression::Ternary(t) => ternary_expression(t)(i),
        Expression::Binary(b) => alt(
            // Branch 1: Try everything on one line. allow_trailing on the RHS lets
            // trailing comments (// comment) pass through single_line mode so they
            // don't force an unnecessary multi-line split.
            single_line(tuple((
                expression(&b.left),
                space,
                binary_operator(&b.operator),
                space,
                allow_trailing(expression(&b.right)),
            ))),
            // Branches 2+: Evaluate LHS once, then try right-side strategies.
            // This avoids O(K^N) blowup on left-associative binary chains.
            move |i: Writer| {
                let left_result = expression(&b.left)(i)?;

                // Branch 2: op+right on same line as LHS (also handles before_line
                // comments on LHS since LHS is emitted normally above).
                let branch2 = single_line(tuple((
                    space,
                    binary_operator(&b.operator),
                    space,
                    allow_trailing(expression(&b.right)),
                )))(left_result.clone());
                if let Some(result) = branch2 {
                    return Some(result);
                }

                // Branch 3: LHS+op on current line, right side indented on next line
                let left_op =
                    single_line(tuple((space, binary_operator(&b.operator))))(left_result.clone());
                if let Some(left_op) = left_op {
                    let branch3 = indented(pair(empty_line, expression(&b.right)))(left_op);
                    if let Some(result) = branch3 {
                        return Some(result);
                    }
                }

                // Branch 4 (last resort): break before operator
                indented(tuple((
                    empty_line,
                    binary_operator(&b.operator),
                    space,
                    expression(&b.right),
                )))(left_result)
            },
        )(i),
        Expression::Prefix(p) => {
            if prefix_needs_space(&p.operator) {
                tuple((prefix_operator(&p.operator), space, expression(&p.value)))(i)
            } else {
                pair(prefix_operator(&p.operator), expression(&p.value))(i)
            }
        }
        Expression::Postfix(p) => pair(expression(&p.value), postfix_operator(&p.operator))(i),
        Expression::Comma(c) => comma_expression(c)(i),
        Expression::Call(c) => call_expression(c)(i),
        Expression::Function(f) => function_expression(f)(i),
        Expression::Lambda(l) => lambda_expression(l)(i),
        Expression::Delegate(d) => delegate_expression(d)(i),
        Expression::Vector(v) => vector_expression(v)(i),
        Expression::Expect(e) => expect_expression(e)(i),
    }
}

fn parens_expression<'s>(
    expr: &'s ParensExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    alt(
        move |i| {
            let i = single_line(tuple((
                token(expr.open),
                format(|f| f.spaces_in_expr_brackets, space),
                expression(&expr.value),
                format(|f| f.spaces_in_expr_brackets, space),
                token_without_trailing(expr.close),
            )))(i)?;
            i.with_allow_newlines(token_trailing(expr.close))
        },
        tuple((
            token(expr.open),
            indented(pair(empty_line, expression(&expr.value))),
            empty_line,
            token(expr.close),
        )),
    )
}

fn literal_expression<'s>(
    expr: &'s LiteralExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    token(expr.token)
}

fn root_var_expression<'s>(
    expr: &'s RootVarExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    pair(token(expr.root), identifier(&expr.name))
}

pub fn table_expression<'s>(
    expr: &'s TableExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        if expr.slots.is_empty() && expr.spread.is_none() {
            return tuple((token(expr.open), token_ignoring_blank_lines(expr.close)))(i);
        }
        alt(
            single_line(table_expression_single_line(expr)),
            table_expression_multi_line(expr),
        )(i)
    }
}

fn table_expression_single_line<'s>(
    expr: &'s TableExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = tuple((token(expr.open), space))(i)?;
        let i = iter(expr.slots.iter().map(|slot| pair(table_slot(slot), space)))(i)?;
        let i = opt(expr.spread, |t| pair(token(t), space))(i)?;
        token(expr.close)(i)
    }
}

fn table_expression_multi_line<'s>(
    expr: &'s TableExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = token(expr.open)(i)?;
        let i = indented(move |i| {
            let i = iter(
                expr.slots
                    .iter()
                    .map(|slot| pair(empty_line, table_slot(slot))),
            )(i)?;
            opt(expr.spread, |t| pair(empty_line, token(t)))(i)
        })(i)?;
        let i = empty_line(i)?;
        token(expr.close)(i)
    }
}

fn table_slot<'s>(slot: &'s TableSlot<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = match &slot.ty {
            TableSlotType::Slot(s) => crate::statement::slot(s)(i)?,
            TableSlotType::JsonProperty {
                name_token,
                colon,
                value,
                ..
            } => tuple((token(name_token), token(colon), space, expression(value)))(i)?,
        };
        opt(slot.comma, token)(i)
    }
}

fn array_expression<'s>(
    expr: &'s ArrayExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        if expr.values.is_empty() && expr.spread.is_none() {
            return tuple((token(expr.open), token(expr.close)))(i);
        }
        alt(
            // Single-line: format content inside single_line, but emit the close
            // bracket's trailing comment (`// comment`) outside single_line
            // so it doesn't cause single-line to reject.
            move |i| {
                let i = single_line(array_expression_single_line(expr))(i)?;
                i.with_allow_newlines(token_trailing(expr.close))
            },
            array_expression_multi_line(expr),
        )(i)
    }
}

fn array_expression_single_line<'s>(
    expr: &'s ArrayExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let last_needs_trailing =
            expr.spread.is_some() || i.format().array_singleline_trailing_commas;
        tuple((
            token(expr.open),
            format(|f| f.array_spaces, space),
            opt(expr.values.split_last(), |(last_value, first_values)| {
                tuple((
                    iter(first_values.iter().map(|value| {
                        tuple((
                            expression(&value.value),
                            token_or_tag(value.separator, ","),
                            space,
                        ))
                    })),
                    expression(&last_value.value),
                    optional_separator(last_needs_trailing, last_value.separator, ","),
                    cond(expr.spread.is_some(), space),
                ))
            }),
            opt(expr.spread, token),
            format(|f| f.array_spaces, space),
            token_without_trailing(expr.close),
        ))(i)
    }
}

fn array_expression_multi_line<'s>(
    expr: &'s ArrayExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let has_commas =
            i.format().array_multiline_commas || i.format().array_multiline_trailing_commas;
        let last_has_comma = expr.spread.is_some() || i.format().array_multiline_trailing_commas;
        tuple((
            token(expr.open),
            indented(tuple((
                opt(expr.values.split_last(), |(last_value, first_values)| {
                    tuple((
                        iter(first_values.iter().map(|value| {
                            tuple((
                                empty_line,
                                expression(&value.value),
                                optional_separator(has_commas, value.separator, ","),
                            ))
                        })),
                        empty_line,
                        expression(&last_value.value),
                        optional_separator(last_has_comma, last_value.separator, ","),
                    ))
                }),
                opt(expr.spread, |t| pair(empty_line, token(t))),
            ))),
            empty_line,
            token(expr.close),
        ))(i)
    }
}

fn index_expression<'s>(
    expr: &'s IndexExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    pair(
        expression(&expr.base),
        alt(
            single_line(tuple((
                token(expr.open),
                space,
                expression(&expr.index),
                space,
                token(expr.close),
            ))),
            tuple((
                token(expr.open),
                indented(pair(empty_line, expression(&expr.index))),
                empty_line,
                token(expr.close),
            )),
        ),
    )
}

fn property_expression<'s>(
    expr: &'s PropertyExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let property_writer = |w: Writer| -> Option<Writer> {
            match &expr.property {
                sqparse::ast::MethodIdentifier::Identifier(id) => identifier(id)(w),
                sqparse::ast::MethodIdentifier::Constructor(tok) => token(tok)(w),
            }
        };

        alt(
            single_line(tuple((
                expression(&expr.base),
                token(expr.dot),
                property_writer,
            ))),
            // Emit the base (handling any before-line comments it carries), then try
            // to fit `.prop` on the same line. Only indent if it genuinely doesn't fit.
            pair(
                expression(&expr.base),
                alt(
                    single_line(tuple((token(expr.dot), property_writer))),
                    indented(tuple((empty_line, token(expr.dot), property_writer))),
                ),
            ),
        )(i)
    }
}

fn ternary_expression<'s>(
    expr: &'s TernaryExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    alt(
        single_line(tuple((
            expression(&expr.condition),
            space,
            token(expr.question),
            space,
            expression(&expr.true_value),
            space,
            token(expr.separator),
            space,
            expression(&expr.false_value),
        ))),
        pair(
            expression(&expr.condition),
            indented(tuple((
                empty_line,
                token(expr.question),
                space,
                expression(&expr.true_value),
                empty_line,
                token(expr.separator),
                space,
                expression(&expr.false_value),
            ))),
        ),
    )
}

fn comma_expression<'s>(
    expr: &'s CommaExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = iter(
            expr.values
                .items
                .iter()
                .map(|(item, sep)| tuple((expression(item), token(sep), space))),
        )(i)?;
        expression(&expr.values.last_item)(i)
    }
}

fn call_expression<'s>(expr: &'s CallExpression<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = expression(&expr.function)(i)?;
        let i = if expr.arguments.is_empty() {
            tuple((token(expr.open), token(expr.close)))(i)?
        } else {
            alt(
                // Single-line: put args on one line. Use token_without_trailing for the
                // close paren so trailing `//` comments don't break single_line mode.
                move |i| {
                    let i = single_line(move |i| {
                        let i = token(expr.open)(i)?;
                        let i = format(|f| f.spaces_in_expr_brackets, space)(i)?;
                        let i = call_args_inline(&expr.arguments)(i)?;
                        let i = format(|f| f.spaces_in_expr_brackets, space)(i)?;
                        token_without_trailing(expr.close)(i)
                    })(i)?;
                    i.with_allow_newlines(token_trailing(expr.close))
                },
                move |i| {
                    let i = token_without_trailing(expr.open)(i)?;
                    let i = indented(move |i| call_args_multi(&expr.arguments)(i))(i)?;
                    let i = empty_line(i)?;
                    token(expr.close)(i)
                },
            )(i)?
        };
        opt(expr.post_initializer.as_ref(), |t| {
            pair(space, table_expression(t))
        })(i)
    }
}

fn call_args_inline<'s>(
    args: &'s [sqparse::ast::CallArgument<'s>],
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |mut i| {
        for (idx, arg) in args.iter().enumerate() {
            i = expression(&arg.value)(i)?;
            let is_last = idx == args.len() - 1;
            if !is_last {
                if arg.comma.is_some() {
                    i = token_or_tag(arg.comma, ",")(i)?;
                    i = space(i)?;
                } else {
                    // No comma between args (void function)
                    i = space(i)?;
                }
            }
        }
        Some(i)
    }
}

fn call_args_multi<'s>(
    args: &'s [sqparse::ast::CallArgument<'s>],
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |mut i| {
        let mut inline_next = false;
        for (idx, arg) in args.iter().enumerate() {
            let is_last = idx == args.len() - 1;
            if inline_next {
                i = space(i)?;
            } else {
                i = empty_line(i)?;
            }
            i = expression(&arg.value)(i)?;
            if !is_last {
                if arg.comma.is_some() {
                    i = token_or_tag(arg.comma, ",")(i)?;
                    inline_next = false;
                } else {
                    // No comma between args (void function)
                    inline_next = true;
                }
            }
        }
        Some(i)
    }
}

fn function_expression<'s>(
    expr: &'s FunctionExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = opt(expr.return_type.as_ref(), |rt| pair(type_format(rt), space))(i)?;
        let i = token(expr.function)(i)?;
        function_definition(&expr.definition)(i)
    }
}

pub fn function_definition<'s>(
    def: &'s FunctionDefinition<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = opt(def.environment.as_ref(), |env| {
            tuple((token(env.open), expression(&env.value), token(env.close)))
        })(i)?;
        let i = token(def.open)(i)?;
        let has_params =
            !matches!(&def.params, FunctionParams::NonVariable { params } if params.is_none());
        let i = if has_params {
            tuple((
                format(|f| f.spaces_in_expr_brackets, space),
                function_params(&def.params),
                format(|f| f.spaces_in_expr_brackets, space),
            ))(i)?
        } else {
            function_params(&def.params)(i)?
        };
        let i = token(def.close)(i)?;
        let i = opt(def.captures.as_ref(), |caps| {
            tuple((
                space,
                token(caps.colon),
                space,
                token(caps.open),
                opt(caps.names.as_ref(), |names| {
                    tuple((
                        space,
                        move |i| {
                            let i =
                                iter(names.items.iter().map(|(name, sep)| {
                                    tuple((identifier(name), token(sep), space))
                                }))(i)?;
                            let i = identifier(&names.last_item)(i)?;
                            opt(names.trailing, token)(i)
                        },
                        space,
                    ))
                }),
                token(caps.close),
            ))
        })(i)?;
        crate::statement::statement_type_body(&def.body)(i)
    }
}

pub fn function_params<'s>(
    params: &'s FunctionParams<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match params {
        FunctionParams::NonVariable { params } => {
            opt(params.as_ref(), |params| {
                move |i: Writer| {
                    let i =
                        iter(params.items.iter().map(|(param, sep)| {
                            tuple((function_param(param), token(sep), space))
                        }))(i)?;
                    let i = function_param(&params.last_item)(i)?;
                    opt(params.trailing, token)(i)
                }
            })(i)
        }
        FunctionParams::EmptyVariable { vararg } => token(vararg)(i),
        FunctionParams::NonEmptyVariable {
            params,
            comma,
            vararg,
        } => {
            let i = iter(
                params
                    .items
                    .iter()
                    .map(|(param, sep)| tuple((function_param(param), token(sep), space))),
            )(i)?;
            let i = function_param(&params.last_item)(i)?;
            tuple((token(comma), space, token(vararg)))(i)
        }
    }
}

fn function_param<'s>(
    param: &'s sqparse::ast::FunctionParam<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = opt(param.type_.as_ref(), |ty| pair(type_format(ty), space))(i)?;
        let i = identifier(&param.name)(i)?;
        opt(param.initializer.as_ref(), |init| {
            tuple((space, token(init.assign), space, expression(&init.value)))
        })(i)
    }
}

fn lambda_expression<'s>(
    expr: &'s LambdaExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = token(expr.at)(i)?;
        let i = token(expr.open)(i)?;
        let has_params =
            !matches!(&expr.params, FunctionParams::NonVariable { params } if params.is_none());
        let i = if has_params {
            tuple((
                format(|f| f.spaces_in_expr_brackets, space),
                function_params(&expr.params),
                format(|f| f.spaces_in_expr_brackets, space),
            ))(i)?
        } else {
            function_params(&expr.params)(i)?
        };
        let i = token(expr.close)(i)?;
        pair(space, expression(&expr.value))(i)
    }
}

fn delegate_expression<'s>(
    expr: &'s DelegateExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    tuple((
        token(expr.delegate),
        space,
        expression(&expr.parent),
        space,
        token(expr.colon),
        space,
        expression(&expr.value),
    ))
}

fn vector_expression<'s>(
    expr: &'s VectorExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    alt(
        move |i| {
            let i = single_line(tuple((
                token(expr.open),
                space,
                expression(&expr.x),
                token(expr.comma_1),
                space,
                expression(&expr.y),
                token(expr.comma_2),
                space,
                expression(&expr.z),
                space,
                token_without_trailing(expr.close),
            )))(i)?;
            i.with_allow_newlines(token_trailing(expr.close))
        },
        tuple((
            token(expr.open),
            indented(tuple((
                empty_line,
                expression(&expr.x),
                token(expr.comma_1),
                empty_line,
                expression(&expr.y),
                token(expr.comma_2),
                empty_line,
                expression(&expr.z),
            ))),
            empty_line,
            token(expr.close),
        )),
    )
}

fn expect_expression<'s>(
    expr: &'s ExpectExpression<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    tuple((
        token(expr.expect),
        space,
        type_format(&expr.ty),
        token(expr.open),
        format(|f| f.spaces_in_expr_brackets, space),
        expression(&expr.value),
        format(|f| f.spaces_in_expr_brackets, space),
        token(expr.close),
    ))
}
