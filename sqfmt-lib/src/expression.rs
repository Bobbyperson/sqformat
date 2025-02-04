use crate::combinators::{
    alt, cond, empty_line, format, indented, iter, opt, pair, single_line, space,
    tag, tuple,
};
use crate::shared::{identifier, optional_separator, token_or_tag};
use crate::token::{discard_token, token};
use crate::writer::Writer;
use sqparse::ast::{
    ArrayExpression, ClassExpression, Expression, IndexExpression, LiteralExpression,
    ParensExpression, PropertyExpression, RootVarExpression, TableExpression, TernaryExpression,
};

/// Formats an expression by dispatching on its AST structure.
///
/// In this version we no longer pass in a “parent_precedence” value. Instead the AST’s
/// structure (which has already been disambiguated by the parser) drives the formatting.
pub fn expression<'s>(
    expr: &'s Expression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |mut i| {
        // Special-case for parenthesized expressions: collapse nested parens and print them.
        if let Expression::Parens(parens_expr) = expr {
            let mut parens_expressions = Vec::new();
            let mut inner_parens_expression = parens_expr;
            // Peel off nested Parens nodes.
            loop {
                match inner_parens_expression.value.as_ref() {
                    Expression::Parens(inner) => {
                        parens_expressions.push(inner_parens_expression);
                        inner_parens_expression = inner;
                    }
                    _ => break,
                }
            }
            // Discard the extra open-paren tokens.
            for discard_expr in &parens_expressions {
                i = discard_token(&discard_expr.open)(i)?;
            }
            // Format the inner expression (with a helper that formats it as a parenthesized group).
            i = parens_expression(inner_parens_expression)(i)?;
            // Discard the extra close-paren tokens.
            for discard_expr in parens_expressions.iter().rev() {
                i = discard_token(&discard_expr.close)(i)?;
            }
            return Some(i);
        }
        // For all other expressions, format them without extra wrapping.
        expression_without_parens(expr)(i)
    }
}

/// Formats an expression that is not a Parens node.
fn expression_without_parens<'s>(
    expr: &'s Expression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        match expr {
            Expression::Parens(_) => unreachable!("Parens should be handled in `expression`"),
            Expression::Literal(l)   => literal_expression(l)(i),
            Expression::Var(v) => identifier(&v.name)(i),
            Expression::RootVar(r)   => root_var_expression(r)(i),
            Expression::Table(t)     => table_expression(t)(i),
            Expression::Class(c)     => class_expression(c)(i),
            Expression::Array(a)     => array_expression(a)(i),
            Expression::Index(idx)   => index_expression(idx)(i),
            Expression::Property(p)  => property_expression(p)(i),
            Expression::Ternary(t)   => ternary_operator_expression(t)(i),
            // Add additional expression variants as needed.
            _ => todo!("Handle additional expression variants"),
        }
    }
}

/// Formats a parenthesized expression.
fn parens_expression<'s>(
    expr: &'s ParensExpression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    alt(
        // Single–line style:
        single_line(tuple((
            token(&expr.open),
            format(|f| f.spaces_in_expr_brackets, space),
            expression(&expr.value),
            format(|f| f.spaces_in_expr_brackets, space),
            token(&expr.close),
        ))),
        // Multi–line style:
        tuple((
            token(&expr.open),
            indented(pair(empty_line, expression(&expr.value))),
            empty_line,
            token(&expr.close),
        )),
    )
}

/// Formats a literal expression.
fn literal_expression<'s>(
    expr: &'s LiteralExpression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    token(expr.token)
}

/// Formats a root–variable expression (which is represented as a namespace operator followed by an identifier).
fn root_var_expression<'s>(
    expr: &'s RootVarExpression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    pair(token(expr.root), identifier(&expr.name))
}

/// Formats a table expression.
///
/// (Implementation left as TODO.)
fn table_expression<'s>(
    _expr: &'s TableExpression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| todo!("Implement table_expression formatting")
}

/// Formats a class expression.
///
/// (Implementation left as TODO.)
fn class_expression<'s>(
    _expr: &'s ClassExpression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| todo!("Implement class_expression formatting")
}

/// Formats an array expression by choosing between single–line and multi–line styles.
fn array_expression<'s>(
    expr: &'s ArrayExpression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    alt(
        single_line(array_expression_single_line(expr)),
        array_expression_multi_line(expr),
    )
}

/// Formats an array expression in a single line.
fn array_expression_single_line<'s>(
    expr: &'s ArrayExpression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let last_needs_trailing =
            expr.spread.is_some() || i.format().array_singleline_trailing_commas;
        tuple((
            token(&expr.open),
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
            token(&expr.close),
        ))(i)
    }
}

/// Formats an array expression in multiple lines.
fn array_expression_multi_line<'s>(
    expr: &'s ArrayExpression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let has_commas =
            i.format().array_multiline_commas || i.format().array_multiline_trailing_commas;
        let last_has_comma = expr.spread.is_some() || i.format().array_multiline_trailing_commas;
        tuple((
            token(&expr.open),
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
            token(&expr.close),
        ))(i)
    }
}

/// Formats an index expression (e.g. array indexing).
fn index_expression<'s>(
    expr: &'s IndexExpression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    pair(
        expression(&expr.base),
        alt(
            single_line(tuple((
                token(expr.open),
                expression(&expr.index),
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

/// Formats a property expression (e.g. object member access).
fn property_expression<'s>(
    expr: &'s PropertyExpression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    // The property is stored as a MethodIdentifier. Match on it.
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
        pair(
            expression(&expr.base),
            indented(tuple((
                empty_line,
                token(expr.dot),
                property_writer,
            ))),
        ),
    )
}

/// Formats a ternary operator expression.
fn ternary_operator_expression<'s>(
    expr: &'s TernaryExpression<'s>
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    alt(
        single_line(tuple((
            expression(&expr.condition),
            space,
            token(&expr.question),
            space,
            expression(&expr.true_value),
            space,
            token(&expr.separator),
            space,
            expression(&expr.false_value),
        ))),
        pair(
            expression(&expr.condition),
            indented(tuple((
                empty_line,
                token(&expr.question),
                space,
                expression(&expr.true_value),
                empty_line,
                token(&expr.separator),
                space,
                expression(&expr.false_value),
            ))),
        ),
    )
}
