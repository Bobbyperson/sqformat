use crate::combinators::{iter, opt, pair, space, tuple};
use crate::shared::identifier;
use crate::token::token;
use crate::writer::Writer;
use sqparse::ast::{
    FunctionRefParam, GenericType, NullableType, ReferenceType, SeparatedListTrailing1, Type,
};

pub fn type_format<'s>(ty: &'s Type<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match ty {
        Type::Local(t) => token(t.local)(i),
        Type::Var(t) => token(t.var)(i),
        Type::Plain(t) => identifier(&t.name)(i),
        Type::Array(t) => tuple((
            type_format(&t.base),
            token(t.open),
            crate::expression::expression(&t.len),
            token(t.close),
        ))(i),
        Type::Generic(t) => generic_type(t)(i),
        Type::FunctionRef(t) => tuple((
            opt(t.return_type.as_deref(), |rt| pair(type_format(rt), space)),
            token(t.functionref),
            token(t.open),
            opt(t.params.as_ref(), |params| function_ref_params(params)),
            token(t.close),
        ))(i),
        Type::Struct(t) => tuple((
            token(t.struct_),
            space,
            crate::statement::struct_definition(&t.definition),
        ))(i),
        Type::Reference(t) => reference_type(t)(i),
        Type::Nullable(t) => nullable_type(t)(i),
    }
}

fn generic_type<'s>(t: &'s GenericType<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    tuple((
        type_format(&t.base),
        token(t.open),
        separated_list_trailing_types(&t.params),
        token(t.close),
    ))
}

fn reference_type<'s>(t: &'s ReferenceType<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    pair(type_format(&t.base), token(t.reference))
}

fn nullable_type<'s>(t: &'s NullableType<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    tuple((type_format(&t.base), space, token(t.ornull)))
}

fn separated_list_trailing_types<'s>(
    list: &'s SeparatedListTrailing1<'s, Type<'s>>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = iter(
            list.items
                .iter()
                .map(|(item, sep)| tuple((type_format(item), token(sep), space))),
        )(i)?;
        let i = type_format(&list.last_item)(i)?;
        opt(list.trailing, token)(i)
    }
}

fn function_ref_params<'s>(
    list: &'s SeparatedListTrailing1<'s, FunctionRefParam<'s>>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = iter(
            list.items
                .iter()
                .map(|(param, sep)| tuple((function_ref_param(param), token(sep), space))),
        )(i)?;
        let i = function_ref_param(&list.last_item)(i)?;
        opt(list.trailing, token)(i)
    }
}

fn function_ref_param<'s>(
    param: &'s FunctionRefParam<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = type_format(&param.type_)(i)?;
        let i = opt(param.name.as_ref(), |name| pair(space, identifier(name)))(i)?;
        opt(param.initializer.as_ref(), |init| {
            tuple((
                space,
                token(init.assign),
                space,
                crate::expression::expression(&init.value),
            ))
        })(i)
    }
}
