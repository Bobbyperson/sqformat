use crate::combinators::{definitely_multi_line, empty_line, pair};
use crate::writer::Writer;
use sqparse::token::Comment;

const SINGLE_LINE_START: &str = "// ";
const PREPROCESSOR_START: &str = "#";
const SINGLE_MULTI_START: &str = "/* ";
const SINGLE_MULTI_END: &str = " */";

pub fn comment<'s>(comment: &'s Comment<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match comment {
        Comment::MultiLine(val) => multi_line_comment(val)(i),
        Comment::SingleLine(val) => single_line_comment(SINGLE_LINE_START, val)(i),
        Comment::ScriptStyle(val) => preprocessor_comment(val)(i),
    }
}

/// Like `comment`, but does not wrap single-line (`//`) comments.
/// Used for trailing comments where wrapping would cause continuation lines to be
/// re-attributed to the next token on re-parse, breaking idempotency.
pub fn comment_no_wrap<'s>(comment: &'s Comment<'s>) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| match comment {
        Comment::MultiLine(val) => multi_line_comment(val)(i),
        Comment::SingleLine(val) => single_line_comment_no_wrap(SINGLE_LINE_START, val)(i),
        Comment::ScriptStyle(val) => preprocessor_comment(val)(i),
    }
}

fn multi_line_comment<'s>(val: &'s str) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    move |i| {
        if val.contains('\n') {
            // Multi-line: preserve original formatting verbatim.
            // Just trim trailing whitespace on each line.
            definitely_multi_line(pair(empty_line, move |mut i: Writer| {
                i = i.write("/*")?;
                for (idx, line) in val.split('\n').enumerate() {
                    if idx > 0 {
                        i = i.write_raw_new_line()?;
                    }
                    i = i.write(line.trim_end())?;
                }
                i.write("*/")?.empty_line()
            }))(i)
        } else {
            // Single-line: trim and wrap in /* */
            let trimmed = val.trim();
            if trimmed.is_empty() {
                i.write("/* */")
            } else {
                i.write(SINGLE_MULTI_START)?
                    .write(trimmed)?
                    .write(SINGLE_MULTI_END)
            }
        }
    }
}

fn single_line_comment<'s>(
    line_start: &'s str,
    val: &'s str,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    definitely_multi_line(move |mut i| {
        let available = i.remaining_columns().saturating_sub(line_start.len());
        let target = balanced_wrap_columns(val, available);
        let mut line_iter = TextWrapIter::new(val);
        while let Some(line_text) = line_iter.next(target) {
            i = i.write(line_start)?.write(line_text)?.write_new_line()?;
        }

        Some(i)
    })
}

fn single_line_comment_no_wrap<'s>(
    line_start: &'s str,
    val: &'s str,
) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    definitely_multi_line(move |i| {
        let trimmed = TextWrapIter::new(val).next(usize::MAX / 2).unwrap_or("");
        i.write(line_start)?.write(trimmed)?.write_new_line()
    })
}

/// Emits a preprocessor directive verbatim: `#` followed by val with no trimming.
/// Adjusts preproc_depth for block-forming directives:
///   - `#if`/`#ifdef`/`#ifndef`: emit then open (+1)
///   - `#endif`: emit then close (-1)
///   - `#else`/`#elseif`: close (-1) then emit then open (+1)
fn preprocessor_comment<'s>(val: &'s str) -> impl FnOnce(Writer) -> Option<Writer> + 's {
    definitely_multi_line(move |i| {
        let keyword = val.split_whitespace().next().unwrap_or("");
        let is_open = matches!(keyword, "if" | "ifdef" | "ifndef");
        let is_close = keyword == "endif";
        let is_else = matches!(keyword, "else" | "elseif");

        // For #else/#elseif and #endif: close the current block first, then reset the
        // current line's indent to the new (shallower) depth before writing the directive.
        let i = if is_else || is_close {
            i.adjust_preproc_depth(-1).empty_line()?
        } else {
            i
        };
        let i = i.write(PREPROCESSOR_START)?.write(val)?.write_new_line()?;
        let i = if is_open || is_else {
            i.adjust_preproc_depth(1)
        } else {
            i
        };
        Some(i)
    })
}

/// Returns a target column width ≤ `max_columns` that distributes words as evenly as possible
/// across lines. Uses binary search to find the minimum width that still requires the same number
/// of lines as greedy wrapping at `max_columns`.
fn balanced_wrap_columns(text: &str, max_columns: usize) -> usize {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return max_columns;
    }

    let max_word_len = words.iter().map(|w| w.chars().count()).max().unwrap_or(0);
    if max_word_len >= max_columns {
        return max_columns;
    }

    let count_lines = |target: usize| -> usize {
        let mut lines = 1usize;
        let mut current = 0usize;
        for word in &words {
            let wlen = word.chars().count();
            if current == 0 {
                current = wlen;
            } else if current + 1 + wlen <= target {
                current += 1 + wlen;
            } else {
                lines += 1;
                current = wlen;
            }
        }
        lines
    };

    let num_lines = count_lines(max_columns);
    if num_lines <= 1 {
        return max_columns;
    }

    // Binary search for the minimum width that still fits in `num_lines` lines.
    let mut lo = max_word_len;
    let mut hi = max_columns;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if count_lines(mid) <= num_lines {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }

    lo
}

// todo: this needs to handle tabs in the text correctly
#[derive(Clone, Copy)]
struct TextWrapIter<'s> {
    text: Option<&'s str>,
}

impl<'s> TextWrapIter<'s> {
    fn new(text: &'s str) -> Self {
        let text = trim_line_start(text);
        let text = text.trim_end();

        TextWrapIter { text: Some(text) }
    }

    fn next(&mut self, columns: usize) -> Option<&'s str> {
        let text = self.text?;

        // Find the byte offset of the (columns+1)th character, or end of string.
        let max_end = text
            .char_indices()
            .nth(columns + 1)
            .map(|(i, _)| i)
            .unwrap_or(text.len());
        let next_line_max = &text[..max_end];
        let char_count = text.chars().count();
        let break_pos = match next_line_max.find('\n') {
            Some(pos) => pos,
            None if char_count <= columns => text.len(),
            None => {
                // Break at the first word boundary before the end of the line.
                // Or if there isn't one, the first word boundary after the end of the line.
                next_line_max.rfind(char::is_whitespace).unwrap_or_else(|| {
                    text[max_end..]
                        .find(char::is_whitespace)
                        .map(|offset| max_end + offset)
                        .unwrap_or(text.len())
                })
            }
        };

        let (this_line, remaining) = text.split_at(break_pos);
        let new_text = trim_line_start(remaining);
        self.text = if new_text.is_empty() {
            None
        } else {
            Some(new_text)
        };
        Some(this_line.trim_end())
    }
}

fn trim_line_start(line: &str) -> &str {
    let after_newline = line.strip_prefix('\n').unwrap_or(line);
    after_newline.strip_prefix(' ').unwrap_or(after_newline)
}

#[cfg(test)]
mod test {
    use crate::comment::comment;
    use crate::test_utils::{test_write, test_write_columns};
    use sqparse::token::Comment;

    #[test]
    fn single_line_no_wrapping() {
        let c = Comment::SingleLine("    Hello world!  ");
        let val = test_write(comment(&c));

        assert_eq!(val, "//    Hello world!\n");
    }

    #[test]
    fn single_line_wrapping() {
        let c = Comment::SingleLine("0 1 2 3 4 5 6 7 8 9 This comment is over 20 columns wide");
        let val = test_write(comment(&c));

        assert_eq!(
            val,
            "// 0 1 2 3 4 5 6 7\n// 8 9 This\n// comment is over\n// 20 columns wide\n"
        );
    }

    #[test]
    fn single_line_long_words() {
        let c = Comment::SingleLine(
            "Thiswordisover10chars Thiswordisalsoover10chars ok? Andsoisthisone",
        );
        let val = test_write(comment(&c));

        assert_eq!(
            val,
            "// Thiswordisover10chars\n// Thiswordisalsoover10chars\n// ok?\n// Andsoisthisone\n"
        );
    }

    #[test]
    fn single_line_no_column() {
        let c = Comment::SingleLine("Hello world, this is some text");
        let val = test_write_columns(0, comment(&c));

        assert_eq!(
            val,
            "// Hello\n// world,\n// this\n// is\n// some\n// text\n"
        );
    }

    #[test]
    fn preprocessor_directives() {
        let c = Comment::ScriptStyle("if SERVER");
        let val = test_write(comment(&c));
        assert_eq!(val, "#if SERVER\n");

        let c = Comment::ScriptStyle("else");
        let val = test_write(comment(&c));
        assert_eq!(val, "#else\n");

        let c = Comment::ScriptStyle("endif");
        let val = test_write(comment(&c));
        assert_eq!(val, "#endif\n");

        let c = Comment::ScriptStyle(" define FOO bar");
        let val = test_write(comment(&c));
        assert_eq!(val, "# define FOO bar\n");
    }

    #[test]
    fn multiline_single_word() {
        let c = Comment::MultiLine("Hello");
        let val = test_write(comment(&c));

        assert_eq!(val, "/* Hello */");
    }

    #[test]
    fn multiline_single_line() {
        let c = Comment::MultiLine("Hello world!");
        let val = test_write(comment(&c));

        assert_eq!(val, "/* Hello world! */");
    }

    #[test]
    fn multiline_preserves_formatting() {
        let c = Comment::MultiLine(" Hello  \n      world! ");
        let val = test_write(comment(&c));

        assert_eq!(val, "/* Hello\n      world!*/\n");
    }

    #[test]
    fn multiline_preserves_doc_comments() {
        let c = Comment::MultiLine("*\n * Hello\n * world! ");
        let val = test_write(comment(&c));

        assert_eq!(val, "/**\n * Hello\n * world!*/\n");
    }
}
