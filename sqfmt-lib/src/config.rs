use std::fmt;
use std::path::{Path, PathBuf};

/// The file every tool discovers formatter settings from.
pub const CONFIG_FILE_NAME: &str = ".sqformat.toml";

#[derive(Clone, Debug)]
pub struct Format {
    pub column_limit: usize,

    pub indent: String,
    pub indent_columns: usize,

    pub spaces_in_expr_brackets: bool,

    pub array_spaces: bool,
    pub array_multiline_commas: bool,
    pub array_multiline_trailing_commas: bool,
    pub array_singleline_trailing_commas: bool,
}

impl Default for Format {
    fn default() -> Self {
        Format {
            column_limit: 160,
            indent: "\t".to_string(),
            indent_columns: 4,
            spaces_in_expr_brackets: true,
            array_spaces: true,
            array_multiline_commas: true,
            array_multiline_trailing_commas: false,
            array_singleline_trailing_commas: false,
        }
    }
}

impl Format {
    /// The indent style this format writes, as a `.sqformat.toml` would name it.
    pub fn indent_style(&self) -> &'static str {
        if self.indent.starts_with('\t') {
            "tab"
        } else {
            "space"
        }
    }

    /// Sets the indent from a style name and a width, the two settings that describe it together.
    pub fn set_indent(&mut self, style: &str, width: usize) -> Result<(), ConfigError> {
        self.indent = match style {
            "tab" => "\t".to_string(),
            style if style.starts_with("space") => " ".repeat(width),
            other => return Err(ConfigError::IndentStyle(other.to_string())),
        };
        self.indent_columns = width;
        Ok(())
    }
}

/// A `.sqformat.toml` file. Every setting is optional, so an absent one keeps its default.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub column_limit: Option<usize>,
    pub indent_style: Option<String>,
    pub indent_width: Option<usize>,
    pub spaces_in_expr_brackets: Option<bool>,
    pub array_spaces: Option<bool>,
    pub array_multiline_commas: Option<bool>,
    pub array_multiline_trailing_commas: Option<bool>,
    pub array_singleline_trailing_commas: Option<bool>,
}

impl FileConfig {
    pub fn read(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|error| ConfigError::Read(path.to_path_buf(), error.to_string()))?;
        toml::from_str(&content)
            .map_err(|error| ConfigError::Parse(path.to_path_buf(), error.to_string()))
    }

    /// This file's settings over a base format, leaving unset ones alone.
    pub fn apply(&self, mut base: Format) -> Result<Format, ConfigError> {
        if let Some(column_limit) = self.column_limit {
            base.column_limit = column_limit;
        }
        if self.indent_style.is_some() || self.indent_width.is_some() {
            let style = self
                .indent_style
                .as_deref()
                .unwrap_or_else(|| base.indent_style());
            let width = self.indent_width.unwrap_or(base.indent_columns);
            base.set_indent(style, width)?;
        }
        if let Some(value) = self.spaces_in_expr_brackets {
            base.spaces_in_expr_brackets = value;
        }
        if let Some(value) = self.array_spaces {
            base.array_spaces = value;
        }
        if let Some(value) = self.array_multiline_commas {
            base.array_multiline_commas = value;
        }
        if let Some(value) = self.array_multiline_trailing_commas {
            base.array_multiline_trailing_commas = value;
        }
        if let Some(value) = self.array_singleline_trailing_commas {
            base.array_singleline_trailing_commas = value;
        }
        Ok(base)
    }
}

/// The nearest `.sqformat.toml` at or above `start`.
pub fn find(start: &Path) -> Option<PathBuf> {
    let mut directory = start;
    loop {
        let candidate = directory.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        directory = directory.parent()?;
    }
}

/// The format the nearest `.sqformat.toml` at or above `start` describes, or the default format
/// when there is none. Every tool that formats a file on disk discovers its settings this way.
pub fn discover(start: &Path) -> Result<Format, ConfigError> {
    match find(start) {
        Some(path) => FileConfig::read(&path)?.apply(Format::default()),
        None => Ok(Format::default()),
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Read(PathBuf, String),
    Parse(PathBuf, String),
    IndentStyle(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read(path, error) => write!(formatter, "{}: {error}", path.display()),
            ConfigError::Parse(path, error) => {
                write!(formatter, "{}: invalid config: {error}", path.display())
            }
            ConfigError::IndentStyle(style) => write!(
                formatter,
                "unknown indent_style {style:?} (expected \"tab\" or \"space\")"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_only_the_settings_a_file_names() {
        let config: FileConfig =
            toml::from_str("column_limit = 80\nindent_style = \"space\"\n").expect("valid config");
        let format = config.apply(Format::default()).expect("valid indent style");

        assert_eq!(format.column_limit, 80);
        // The width keeps its default, because the file named only the style.
        assert_eq!(format.indent, "    ");
        assert_eq!(format.indent_columns, 4);
        assert!(format.array_spaces, "an unset setting keeps its default");
    }

    #[test]
    fn keeps_the_indent_style_when_only_the_width_changes() {
        let config: FileConfig = toml::from_str("indent_width = 2\n").expect("valid config");
        let format = config.apply(Format::default()).expect("valid indent style");

        assert_eq!(format.indent, "\t", "the default style is tabs");
        assert_eq!(format.indent_columns, 2);
    }

    #[test]
    fn rejects_an_unknown_indent_style() {
        let config: FileConfig = toml::from_str("indent_style = \"tabs\"\n").expect("valid config");

        let error = config
            .apply(Format::default())
            .expect_err("an unknown style is an error");

        assert_eq!(
            error.to_string(),
            "unknown indent_style \"tabs\" (expected \"tab\" or \"space\")"
        );
    }

    #[test]
    fn finds_the_nearest_config_above_a_directory() {
        let root = std::env::temp_dir().join(format!("sqfmt-config-{}", std::process::id()));
        let nested = root.join("mod/scripts");
        std::fs::create_dir_all(&nested).expect("temporary directories");
        std::fs::write(root.join(CONFIG_FILE_NAME), "column_limit = 100\n").expect("config file");

        let format = discover(&nested).expect("a valid config");

        assert_eq!(format.column_limit, 100);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
