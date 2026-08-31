use crate::combinators::{
    alt, cond_or, empty_line, iter, new_line, pair, single_line, space, tuple,
};
use crate::comment::{comment, comment_no_wrap};
use crate::writer::Writer;
use sqparse::token::{
    Comment, LiteralBase, LiteralToken, StringToken, TerminalToken, Token, TokenLine, TokenType,
};

pub fn token<'s>(token: &'s Token<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = token_without_trailing(token)(i)?;
        token_trailing(token)(i)
    }
}

/// Format a token's before_lines, inline comments, and type, but NOT the trailing (new_line)
/// comment. Use with `token_trailing` to emit the trailing comment separately, this allows
/// callers to put the token inside `single_line` while emitting trailing `//` comments outside.
pub fn token_without_trailing<'s>(
    token: &'s Token<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |mut i| {
        // before_lines are structural (comments on lines above this token) and should
        // be emitted even in single_line mode, they don't affect expression width.
        // However, blank-line separators should be suppressed in single_line mode to
        // prevent them from breaking the single-line layout.
        //
        // If before_lines contain actual comments, reject in single_line mode:
        // emitting them would produce newlines via with_allow_newlines, making the
        // output multi-line even though single_line mode doesn't formally fail.
        // This caused arrays with commented-out elements to produce broken output.
        if i.is_single_line()
            && token
                .before_lines
                .iter()
                .any(|line| !line.comments.is_empty())
        {
            return None;
        }

        let suppress_blank_separators = i.is_single_line();
        i = i.with_allow_newlines(token_before_lines(
            &token.before_lines,
            suppress_blank_separators,
        ))?;

        i = cond_or(
            token.comments.is_empty(),
            token_type(token.ty),
            alt(
                single_line(pair(
                    single_line_comment_list(&token.comments),
                    token_type(token.ty),
                )),
                tuple((
                    multi_line_comment_list(&token.comments),
                    empty_line,
                    token_type(token.ty),
                )),
            ),
        )(i)?;

        Some(i)
    }
}

/// Like `token_without_trailing`, but ignores blank-line separators from before_lines.
/// Useful for tokens like closing braces where blank lines before them are not meaningful.
pub fn token_ignoring_blank_lines<'s>(
    token: &'s Token<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |mut i| {
        // Emit only before_lines that have comments, skip blank separators
        for before_line in &token.before_lines {
            if !before_line.comments.is_empty() {
                i = i.with_allow_newlines(pair(
                    inline_comment_list(&before_line.comments),
                    empty_line,
                ))?;
            }
        }

        i = cond_or(
            token.comments.is_empty(),
            token_type(token.ty),
            alt(
                single_line(pair(
                    single_line_comment_list(&token.comments),
                    token_type(token.ty),
                )),
                tuple((
                    multi_line_comment_list(&token.comments),
                    empty_line,
                    token_type(token.ty),
                )),
            ),
        )(i)?;

        token_trailing(token)(i)
    }
}

/// Process only the before_lines comments of a token (ignoring blank separators).
/// Used when before_lines must be emitted at a different indent depth than the token
/// itself (e.g., #endif before a closing brace should be indented inside the block).
pub fn token_before_lines_only<'s>(
    tok: &'s Token<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |mut i| {
        for before_line in &tok.before_lines {
            if !before_line.comments.is_empty() {
                i = i.with_allow_newlines(tuple((
                    empty_line,
                    inline_comment_list(&before_line.comments),
                    empty_line,
                )))?;
            }
        }
        Some(i)
    }
}

/// Format a token's inline comments, type, and trailing comment, but NOT its before_lines.
/// Companion to token_before_lines_only for when before_lines were already processed
/// separately.
pub fn token_without_before_lines<'s>(
    tok: &'s Token<'s>,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        let i = cond_or(
            tok.comments.is_empty(),
            token_type(tok.ty),
            alt(
                single_line(pair(
                    single_line_comment_list(&tok.comments),
                    token_type(tok.ty),
                )),
                tuple((
                    multi_line_comment_list(&tok.comments),
                    empty_line,
                    token_type(tok.ty),
                )),
            ),
        )(i)?;
        token_trailing(tok)(i)
    }
}

/// Emit just the trailing (new_line) comment of a token, if any.
pub fn token_trailing<'s>(token: &'s Token<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |mut i| {
        if let Some(line) = &token.new_line
            && !line.comments.is_empty()
        {
            // Trailing comments require newlines (`// comment\n`), which are
            // incompatible with single_line mode. Callers that need single_line should
            // use token_without_trailing() inside single_line, then emit trailing
            // separately with with_allow_newlines(token_trailing(...)).
            if i.is_single_line() {
                if i.allows_trailing_in_single_line() {
                    // The caller indicated trailing comments are allowed here
                    // (e.g., last expression in a binary op single-line branch).
                    i = i.with_allow_newlines(pair(
                        space,
                        inline_comment_list_no_wrap(&line.comments),
                    ))?;
                    return Some(i);
                }
                return None;
            }
            i = i.with_allow_newlines(pair(space, inline_comment_list_no_wrap(&line.comments)))?;
        }
        Some(i)
    }
}

// Prints comments around a token, but ignores the token itself
pub fn discard_token<'s>(token: &'s Token<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |mut i| {
        // Only reject in single_line mode if there are comments to emit on the new_line.
        // An empty new_line (just a line break, no comments) doesn't need to be emitted.
        if token
            .new_line
            .as_ref()
            .is_some_and(|line| !line.comments.is_empty())
            && i.is_single_line()
        {
            return None;
        }

        i = pair(
            token_before_lines(&token.before_lines, false),
            inline_comment_list(&token.comments),
        )(i)?;

        if let Some(line) = &token.new_line
            && !line.comments.is_empty()
        {
            i = tuple((
                space,
                inline_comment_list_no_wrap(&line.comments),
                empty_line,
            ))(i)?;
        }

        Some(i)
    }
}

fn token_before_lines<'s>(
    lines: &'s [TokenLine<'s>],
    suppress_blank_separators: bool,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |mut i| {
        // Count leading empty lines (blank lines in the source).
        let leading_empties = lines
            .iter()
            .take_while(|line| line.comments.is_empty())
            .count();

        // If there were blank lines in the source, preserve one blank line separator.
        // Only emit if the writer already has content (avoids leading blank at start of output).
        // Suppress in single-line mode to avoid breaking single-line layout.
        // Suppress at the start of a block (right after `{`) to avoid blank lines
        // between the opening brace and the first statement/member.
        if leading_empties > 0 && i.has_content() && !i.is_at_block_start() {
            if suppress_blank_separators {
                return None;
            }
            i = new_line(i)?;
        }

        let before_lines_iter = lines.iter().skip(leading_empties);

        let mut was_last_empty = false;
        for before_line in before_lines_iter {
            // Skip consecutive empty lines
            if was_last_empty && before_line.comments.is_empty() {
                continue;
            }
            was_last_empty = before_line.comments.is_empty();
            if was_last_empty {
                i = new_line(i)?;
            } else {
                i = pair(inline_comment_list(&before_line.comments), empty_line)(i)?;
            }
        }

        Some(i)
    }
}

fn single_line_comment_list<'s>(
    comments: &'s [Comment<'s>],
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    iter(comments.iter().map(|c| tuple((space, comment(c), space))))
}

fn multi_line_comment_list<'s>(
    comments: &'s [Comment<'s>],
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    iter(comments.iter().map(|c| tuple((comment(c), empty_line))))
}

fn inline_comment_list<'s>(
    comments: &'s [Comment<'s>],
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    alt(
        single_line(single_line_comment_list(comments)),
        multi_line_comment_list(comments),
    )
}

fn multi_line_comment_list_no_wrap<'s>(
    comments: &'s [Comment<'s>],
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    iter(
        comments
            .iter()
            .map(|c| tuple((comment_no_wrap(c), empty_line))),
    )
}

fn inline_comment_list_no_wrap<'s>(
    comments: &'s [Comment<'s>],
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    alt(
        single_line(single_line_comment_list(comments)),
        multi_line_comment_list_no_wrap(comments),
    )
}

fn token_type<'s>(token_ty: TokenType<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match token_ty {
        TokenType::Empty => Some(i),
        TokenType::Terminal(terminal) => terminal_token(terminal)(i),
        TokenType::Literal(literal) => literal_token(literal)(i),
        TokenType::Identifier(identifier) => i.write(identifier),
    }
}

fn terminal_token(terminal: TerminalToken) -> impl FnOnce(Writer) -> Option<Writer> {
    move |i| i.write(terminal.as_str())
}

fn int_to_base_string(val: i64, base: LiteralBase) -> String {
    match base {
        LiteralBase::Decimal => val.to_string(),
        LiteralBase::Octal => format!("0{:o}", val),
        LiteralBase::Hexadecimal => format!("{:#x}", val),
    }
}

fn literal_token<'s>(literal: LiteralToken<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match literal {
        LiteralToken::Int(val, base) => i.write(&int_to_base_string(val, base)),
        LiteralToken::Char(val) => i.write(&format!("'{}'", val)),
        LiteralToken::Float(val) => i.write(&lexical::to_string(val)),
        LiteralToken::String(StringToken::Literal(val)) => i.write(&format!("\"{}\"", val)),
        LiteralToken::String(StringToken::Verbatim(val)) => i.write(&format!("@\"{}\"", val)),
        LiteralToken::String(StringToken::Asset(val)) => i.write(&format!("$\"{}\"", val)),
    }
}
