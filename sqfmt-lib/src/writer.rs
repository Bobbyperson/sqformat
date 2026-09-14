use crate::config::Format;
use std::sync::Arc;

#[derive(Clone)]
struct WriterStore {
    lines: im::Vector<WrittenLine>,
    current_line: String,
    remaining_columns: isize,
    /// Extra indent levels from preprocessor blocks (#if / #endif).
    /// Lives in store (not config) so it persists across with_config restoration.
    preproc_depth: isize,
}

#[derive(Clone)]
struct WrittenLine {
    text: String,
    preserve_trailing: bool,
}

#[derive(Clone, Copy)]
struct WriteConfig {
    is_single_line: bool,
    /// Number of completed lines when the current single-line region started.
    /// If a permitted trailing comment adds a line, any later write rejects the layout.
    single_line_start: usize,
    /// When true, trailing comments (// comment) are allowed even in single_line
    /// mode by temporarily suspending single_line. Used for the last expression
    /// in a single_line region (e.g., RHS of a binary expression) where the
    /// trailing comment doesn't affect whether the code fits on one line.
    allow_trailing_in_single_line: bool,
    indent_depth: usize,
    /// When true, the next `with_indent` call will not increase the indent depth.
    /// Used for if-condition multi-line formatting so that binary expression
    /// continuation lines stay at the same indent level as the first condition line,
    /// rather than being double-indented. Consumed (set to false) on first use.
    flat_indent: bool,
}

#[derive(Clone)]
pub struct Writer {
    format: Arc<Format>,
    store: Arc<WriterStore>,
    config: WriteConfig,
}

impl Writer {
    pub fn new(format: Arc<Format>) -> Self {
        let remaining_columns = format.column_limit as isize;
        Writer {
            format,
            store: Arc::new(WriterStore {
                lines: im::Vector::new(),
                current_line: String::new(),
                remaining_columns,
                preproc_depth: 0,
            }),
            config: WriteConfig {
                is_single_line: false,
                single_line_start: 0,
                allow_trailing_in_single_line: false,
                indent_depth: 0,
                flat_indent: false,
            },
        }
    }

    pub fn format(&self) -> &Format {
        self.format.as_ref()
    }

    pub fn remaining_columns(&self) -> usize {
        self.store.remaining_columns.max(0) as usize
    }

    pub fn is_single_line(&self) -> bool {
        self.config.is_single_line
    }

    pub fn allows_trailing_in_single_line(&self) -> bool {
        self.config.allow_trailing_in_single_line
    }

    /// Mark that the next trailing comment encountered in single_line mode should
    /// be allowed (emitted via with_allow_newlines instead of causing failure).
    /// Used for the last expression in a single_line group where the trailing
    /// comment doesn't affect whether code fits on one line.
    pub fn with_allow_trailing_in_single_line<F: FnOnce(Self) -> Option<Self>>(
        self,
        f: F,
    ) -> Option<Self> {
        let config = WriteConfig {
            allow_trailing_in_single_line: true,
            ..self.config
        };
        self.with_config(config, f)
    }

    /// Returns true if we're at the very start of a block body (current line is
    /// whitespace-only and the previous completed line ends with `{`). Used to
    /// suppress blank-line separators between an opening brace and the first statement.
    pub fn is_at_block_start(&self) -> bool {
        self.store.current_line.trim().is_empty()
            && self
                .store
                .lines
                .last()
                .is_some_and(|line| line.text.trim_end().ends_with('{'))
    }

    pub fn has_content(&self) -> bool {
        !self.store.lines.is_empty() || !self.store.current_line.is_empty()
    }

    fn with_config<F: FnOnce(Self) -> Option<Self>>(
        mut self,
        config: WriteConfig,
        f: F,
    ) -> Option<Self> {
        let original_config = std::mem::replace(&mut self.config, config);
        f(self).map(|mut new_self| {
            new_self.config = original_config;
            new_self
        })
    }

    pub fn with_single_line<F: FnOnce(Self) -> Option<Self>>(self, f: F) -> Option<Self> {
        let config = WriteConfig {
            is_single_line: true,
            single_line_start: if self.config.is_single_line {
                self.config.single_line_start
            } else {
                self.store.lines.len()
            },
            ..self.config
        };
        self.with_config(config, f)
    }

    /// Temporarily suspend single_line mode so structural elements (before_lines, trailing
    /// comments) can emit newlines without causing single_line to fail.
    pub fn with_allow_newlines<F: FnOnce(Self) -> Option<Self>>(self, f: F) -> Option<Self> {
        let config = WriteConfig {
            is_single_line: false,
            ..self.config
        };
        self.with_config(config, f)
    }

    /// Adjust the preprocessor indent depth (survives with_config restoration).
    /// Positive = open a block (#if), negative = close a block (#endif).
    pub fn adjust_preproc_depth(mut self, delta: isize) -> Self {
        let store = Arc::make_mut(&mut self.store);
        store.preproc_depth = (store.preproc_depth + delta).max(0);
        self
    }

    pub fn with_indent<F: FnOnce(Self) -> Option<Self>>(self, f: F) -> Option<Self> {
        if self.config.flat_indent {
            // Suppress this indent level and turn off flat_indent for nested calls,
            // so further indentation (function call args) works normally.
            let config = WriteConfig {
                flat_indent: false,
                ..self.config
            };
            return self.with_config(config, f);
        }
        let config = WriteConfig {
            indent_depth: self.config.indent_depth + 1,
            ..self.config
        };
        self.with_config(config, f)
    }

    /// Run `f` with flat-indent mode enabled: the next `with_indent` call inside
    /// will not increase the indent depth (consumed after first use).
    pub fn with_flat_indent<F: FnOnce(Self) -> Option<Self>>(self, f: F) -> Option<Self> {
        let config = WriteConfig {
            flat_indent: true,
            ..self.config
        };
        self.with_config(config, f)
    }

    fn effective_depth(&self) -> usize {
        (self.config.indent_depth as isize + self.store.preproc_depth).max(0) as usize
    }

    pub fn empty_line(mut self) -> Option<Self> {
        // todo(perf): store a flag on if the line has had non-whitespace added, instead of scanning
        // here?
        if self.store.current_line.trim().is_empty() {
            // Ensure the indent on the current line matches the current config depth.
            // This can get out of sync when trailing comments emit newlines at a different
            // indent depth (inside an indented block) and then we leave that block.
            let effective_depth = self.effective_depth();
            let correct_indent = self.format.indent.repeat(effective_depth);
            let remaining_columns = self.format.column_limit as isize
                - self.format.indent_columns as isize * effective_depth as isize;
            let store = Arc::make_mut(&mut self.store);
            store.remaining_columns = remaining_columns;
            store.current_line = correct_indent;
            Some(self)
        } else {
            self.write_new_line()
        }
    }

    /// Write a newline without adding any indentation prefix.
    /// Used for verbatim content like multiline comment bodies.
    pub fn write_raw_new_line(mut self) -> Option<Self> {
        if self.config.is_single_line {
            return None;
        }
        let store = Arc::make_mut(&mut self.store);
        store.remaining_columns = self.format.column_limit as isize;
        store.lines.push_back(WrittenLine {
            text: std::mem::take(&mut store.current_line),
            preserve_trailing: false,
        });
        Some(self)
    }

    pub fn write_new_line(mut self) -> Option<Self> {
        if self.config.is_single_line {
            return None;
        }

        let effective_depth = self.effective_depth();
        let new_line = self.format.indent.repeat(effective_depth);
        let remaining_columns = self.format.column_limit as isize
            - self.format.indent_columns as isize * effective_depth as isize;
        let store = Arc::make_mut(&mut self.store);
        store.remaining_columns = remaining_columns;
        store.lines.push_back(WrittenLine {
            text: std::mem::replace(&mut store.current_line, new_line),
            preserve_trailing: false,
        });

        Some(self)
    }

    pub fn write_space(self) -> Self {
        if self.store.current_line.is_empty()
            || self.store.current_line.ends_with(char::is_whitespace)
        {
            self
        } else {
            self.write_without_breaking(" ", 1)
        }
    }

    pub fn write(self, text: &str) -> Option<Self> {
        if self.config.is_single_line && self.store.lines.len() > self.config.single_line_start {
            return None;
        }

        if text.contains('\n') {
            if self.config.is_single_line {
                return None;
            }

            let mut written = self;
            for part in text.split_inclusive('\n') {
                if let Some(line) = part.strip_suffix('\n') {
                    let columns = written.text_columns(line);
                    written = written.write_without_breaking(line, columns);
                    let store = Arc::make_mut(&mut written.store);
                    store.lines.push_back(WrittenLine {
                        text: std::mem::take(&mut store.current_line),
                        preserve_trailing: true,
                    });
                    store.remaining_columns = written.format.column_limit as isize;
                } else {
                    let columns = written.text_columns(part);
                    written = written.write_without_breaking(part, columns);
                }
            }
            return Some(written);
        }

        let text_columns = self.text_columns(text);
        let written = self.write_without_breaking(text, text_columns);

        if written.config.is_single_line && written.store.remaining_columns < 0 {
            None
        } else {
            Some(written)
        }
    }

    fn write_without_breaking(mut self, text: &str, text_columns: isize) -> Self {
        debug_assert!(!text.contains('\n'));

        let store = Arc::make_mut(&mut self.store);
        store.current_line.push_str(text);
        store.remaining_columns -= text_columns;

        self
    }

    fn text_columns(&self, text: &str) -> isize {
        text.chars()
            .map(|character| {
                if character == '\t' {
                    self.format.indent_columns
                } else {
                    1
                }
            })
            .sum::<usize>() as isize
    }
}

impl std::fmt::Display for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for line in &self.store.lines {
            if line.preserve_trailing {
                f.write_str(&line.text)?;
            } else {
                f.write_str(line.text.trim_end())?;
            }
            f.write_str("\n")?;
        }
        f.write_str(self.store.current_line.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer(column_limit: usize, indent_columns: usize) -> Writer {
        Writer::new(Arc::new(Format {
            column_limit,
            indent: "\t".to_string(),
            indent_columns,
            ..Format::default()
        }))
    }

    #[test]
    fn columns_count_unicode_tabs_and_configured_indentation() {
        let unicode = writer(4, 4)
            .with_single_line(|writer| writer.write("éééé"))
            .expect("four Unicode characters occupy four columns");
        assert_eq!(unicode.remaining_columns(), 0);

        let tab = writer(4, 4)
            .with_single_line(|writer| writer.write("\t"))
            .expect("a tab occupies the configured width");
        assert_eq!(tab.remaining_columns(), 0);

        let indented = writer(5, 4)
            .with_indent(|writer| writer.empty_line())
            .expect("indentation fits");
        assert_eq!(indented.remaining_columns(), 1);
    }

    #[test]
    fn multiline_text_preserves_literal_line_contents() {
        let text = "@\"first  \n\tsecond\n\"";
        let written = writer(20, 4).write(text).expect("multiline write");

        assert_eq!(written.to_string(), text);
        assert_eq!(written.remaining_columns(), 19);
    }
}
