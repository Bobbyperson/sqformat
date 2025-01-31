use crate::combinators::{
    alt, definitely_multi_line, empty_line, indented, pair, single_line, space, tag, tuple,
};
use crate::token::token;
use crate::writer::Writer;
use sqparse::ast::IfStatement;

fn if_statement<'s>(stmt: &'s IfStatement<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    definitely_multi_line(|f| {
        tuple((
            token(stmt.if_),
            space,
            token(stmt.open),

            alt(
                single_line(tuple((
                    space,
                    tag("expr"),
                    space,
                    token(stmt.close),
                ))),
                tuple((
                    indented(pair(empty_line, tag("expr"))),
                    empty_line,
                    token(stmt.close),
                ))
            ),

            alt(
                indented(pair(empty_line, single_line(tag("my very long body is very long and will cause this to wrap or it should! but how long does it need to be? more than 80 chars at least!")))),
                tuple((
                    empty_line,
                    tag("{"),
                    indented(pair(empty_line, tag("my very long body is very long and will cause this to wrap or it should! but how long does it need to be? more than 80 chars at least!"))),
                    empty_line,
                    tag("}"),
                ))
            )
        ))(f)
    })
}
