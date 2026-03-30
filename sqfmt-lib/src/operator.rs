use crate::combinators::{pair, tuple};
use crate::token::token;
use crate::writer::Writer;
use sqparse::ast::{BinaryOperator, PostfixOperator, PrefixOperator};

pub fn binary_operator<'s>(
    op: &'s BinaryOperator<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match op {
        BinaryOperator::Assign(t) => token(t)(i),
        BinaryOperator::AssignNewSlot(t1, t2) => pair(token(t1), token(t2))(i),
        BinaryOperator::AssignAdd(t) => token(t)(i),
        BinaryOperator::AssignSubtract(t) => token(t)(i),
        BinaryOperator::AssignMultiply(t) => token(t)(i),
        BinaryOperator::AssignDivide(t) => token(t)(i),
        BinaryOperator::AssignModulo(t) => token(t)(i),
        BinaryOperator::Add(t) => token(t)(i),
        BinaryOperator::Subtract(t) => token(t)(i),
        BinaryOperator::Multiply(t) => token(t)(i),
        BinaryOperator::Divide(t) => token(t)(i),
        BinaryOperator::Modulo(t) => token(t)(i),
        BinaryOperator::Equal(t) => token(t)(i),
        BinaryOperator::NotEqual(t) => token(t)(i),
        BinaryOperator::Less(t) => token(t)(i),
        BinaryOperator::LessEqual(t) => token(t)(i),
        BinaryOperator::Greater(t) => token(t)(i),
        BinaryOperator::GreaterEqual(t) => token(t)(i),
        BinaryOperator::ThreeWay(t) => token(t)(i),
        BinaryOperator::LogicalAnd(t) => token(t)(i),
        BinaryOperator::LogicalOr(t) => token(t)(i),
        BinaryOperator::BitwiseAnd(t) => token(t)(i),
        BinaryOperator::BitwiseOr(t) => token(t)(i),
        BinaryOperator::BitwiseXor(t) => token(t)(i),
        BinaryOperator::ShiftLeft(t1, t2) => pair(token(t1), token(t2))(i),
        BinaryOperator::ShiftRight(t1, t2) => pair(token(t1), token(t2))(i),
        BinaryOperator::UnsignedShiftRight(t1, t2, t3) => {
            tuple((token(t1), token(t2), token(t3)))(i)
        }
        BinaryOperator::In(t) => token(t)(i),
        BinaryOperator::Instanceof(t) => token(t)(i),
    }
}

pub fn prefix_operator<'s>(
    op: &'s PrefixOperator<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match op {
        PrefixOperator::Negate(t) => token(t)(i),
        PrefixOperator::LogicalNot(t) => token(t)(i),
        PrefixOperator::BitwiseNot(t) => token(t)(i),
        PrefixOperator::Typeof(t) => token(t)(i),
        PrefixOperator::Clone(t) => token(t)(i),
        PrefixOperator::Delete(t) => token(t)(i),
        PrefixOperator::Increment(t) => token(t)(i),
        PrefixOperator::Decrement(t) => token(t)(i),
    }
}

/// Whether a prefix operator needs a space after it (keyword operators like typeof, clone, delete).
pub fn prefix_needs_space(op: &PrefixOperator) -> bool {
    matches!(
        op,
        PrefixOperator::Typeof(_) | PrefixOperator::Clone(_) | PrefixOperator::Delete(_)
    )
}

pub fn postfix_operator<'s>(
    op: &'s PostfixOperator<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match op {
        PostfixOperator::Increment(t) => token(t)(i),
        PostfixOperator::Decrement(t) => token(t)(i),
    }
}
