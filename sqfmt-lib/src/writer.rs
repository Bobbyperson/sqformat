use crate::config::Format;
use std::sync::Arc;

#[derive(Clone)]
struct WriterStore {
    lines: im::Vector<String>,
    current_line: String,
    remaining_columns: isize,
}

#[derive(Clone, Copy)]
struct WriteConfig {
    is_single_line: bool,
    indent_depth: usize,
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
            }),
            config: WriteConfig {
                is_single_line: false,
                indent_depth: 0,
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

    pub fn with_indent<F: FnOnce(Self) -> Option<Self>>(self, f: F) -> Option<Self> {
        let config = WriteConfig {
            indent_depth: self.config.indent_depth + 1,
            ..self.config
        };
        self.with_config(config, f)
    }

    pub fn empty_line(mut self) -> Option<Self> {
        // todo(perf): store a flag on if the line has had non-whitespace added, instead of scanning
        // here?
        if self.store.current_line.trim().is_empty() {
            // Ensure the indent on the current line matches the current config depth.
            // This can get out of sync when trailing comments emit newlines at a different
            // indent depth (e.g. inside an indented block) and then we leave that block.
            let correct_indent = self.format.indent.repeat(self.config.indent_depth);
            let store = Arc::make_mut(&mut self.store);
            store.remaining_columns =
                self.format.column_limit as isize - correct_indent.len() as isize;
            store.current_line = correct_indent;
            Some(self)
        } else {
            self.write_new_line()
        }
    }

    pub fn write_new_line(mut self) -> Option<Self> {
        if self.config.is_single_line {
            return None;
        }

        let new_line = self.format.indent.repeat(self.config.indent_depth);
        let store = Arc::make_mut(&mut self.store);
        store.remaining_columns = self.format.column_limit as isize - new_line.len() as isize;
        store
            .lines
            .push_back(std::mem::replace(&mut store.current_line, new_line));

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
        let text_columns = text.len() as isize;
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
}

impl std::fmt::Display for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for line in &self.store.lines {
            f.write_str(line.trim_end())?;
            f.write_str("\n")?;
        }
        f.write_str(self.store.current_line.trim_end())
    }
}
