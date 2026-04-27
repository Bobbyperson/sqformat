use clap::Parser;
use rayon::prelude::*;
use similar::{ChangeTag, TextDiff};
use std::io::{self, Read};
use std::path::Path;

use sqfmt_lib::config::Format;

#[derive(serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct TomlConfig {
    column_limit: Option<usize>,
    indent_style: Option<String>,
    indent_width: Option<usize>,
    spaces_in_expr_brackets: Option<bool>,
    array_spaces: Option<bool>,
    array_multiline_commas: Option<bool>,
    array_multiline_trailing_commas: Option<bool>,
    array_singleline_trailing_commas: Option<bool>,
}

/// Squirrel code formatter.
///
/// If no files are given, reads from stdin and writes formatted code to stdout.
/// If files are given, writes formatted output to stdout (or edits in-place with -i).
#[derive(Parser, Debug)]
#[clap(author, version)]
struct Args {
    /// Edit files in-place (only valid with file arguments).
    #[clap(short, long, conflicts_with = "check")]
    inplace: bool,

    /// Check if files are formatted without writing changes. Exits with 1 if any file would change.
    #[clap(short, long)]
    check: bool,

    /// Show a unified diff of changes. Exits with 1 if any file would change.
    #[clap(short, long)]
    diff: bool,

    /// Recursively format directories.
    #[clap(short, long)]
    recursive: bool,

    /// Suppress progress and summary output.
    #[clap(short, long, conflicts_with = "verbose")]
    quiet: bool,

    /// Show per-file progress for all modes, including single files.
    #[clap(short, long)]
    verbose: bool,

    /// Column limit (overrides config file).
    #[clap(long, value_name = "N")]
    column_limit: Option<usize>,

    /// Indent style: tab or space (overrides config file).
    #[clap(long, value_name = "STYLE")]
    indent_style: Option<String>,

    /// Columns per indent level (overrides config file).
    #[clap(long, value_name = "N")]
    indent_width: Option<usize>,

    /// Add spaces inside expression brackets (overrides config file).
    #[clap(long, num_args = 0..=1, default_missing_value = "true")]
    spaces_in_expr_brackets: Option<bool>,

    /// Add spaces inside array literals (overrides config file).
    #[clap(long, num_args = 0..=1, default_missing_value = "true")]
    array_spaces: Option<bool>,

    /// Add leading commas on multiline arrays (overrides config file).
    #[clap(long, num_args = 0..=1, default_missing_value = "true")]
    array_multiline_commas: Option<bool>,

    /// Add trailing commas on multiline arrays (overrides config file).
    #[clap(long, num_args = 0..=1, default_missing_value = "true")]
    array_multiline_trailing_commas: Option<bool>,

    /// Add trailing commas on single-line arrays (overrides config file).
    #[clap(long, num_args = 0..=1, default_missing_value = "true")]
    array_singleline_trailing_commas: Option<bool>,

    /// Path to config file (default: .sqformat.toml, searched from current directory upward).
    #[clap(long, value_name = "PATH")]
    config: Option<String>,

    /// Filename to use when reading from stdin (for error messages and diffs).
    #[clap(long, value_name = "NAME")]
    stdin_filename: Option<String>,

    /// Files or directories to format.
    files: Vec<String>,
}

const SQUIRREL_EXTENSIONS: &[&str] = &["nut", "gnut"];

fn is_squirrel_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| SQUIRREL_EXTENSIONS.contains(&ext))
}

fn collect_squirrel_files(dir: &Path, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{}: {}", dir.display(), e);
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_squirrel_files(&path, out);
        } else if is_squirrel_file(&path) {
            out.push(path.to_string_lossy().into_owned());
        }
    }
}

fn find_config() -> Option<std::path::PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(".sqformat.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn load_config(config_arg: Option<&str>) -> TomlConfig {
    let path = if let Some(arg) = config_arg {
        Some(std::path::PathBuf::from(arg))
    } else {
        find_config()
    };

    let path = match path {
        Some(p) => p,
        None => return TomlConfig::default(),
    };

    match std::fs::read_to_string(&path) {
        Ok(content) => match toml::from_str(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("{}: invalid config: {}", path.display(), e);
                std::process::exit(1);
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if config_arg.is_some() {
                eprintln!("{}: {}", path.display(), e);
                std::process::exit(1);
            }
            TomlConfig::default()
        }
        Err(e) => {
            eprintln!("{}: {}", path.display(), e);
            std::process::exit(1);
        }
    }
}

fn build_format(args: &Args, cfg: &TomlConfig) -> Format {
    let defaults = Format::default();
    let indent_style = args
        .indent_style
        .as_deref()
        .or(cfg.indent_style.as_deref())
        .unwrap_or("tab");
    let indent_width = args
        .indent_width
        .or(cfg.indent_width)
        .unwrap_or(defaults.indent_columns);
    let indent = match indent_style {
        s if s.starts_with("space") => " ".repeat(indent_width),
        "tab" => "\t".to_string(),
        other => {
            eprintln!("error: unknown indent_style {:?} (expected \"tab\" or \"space\")", other);
            std::process::exit(1);
        }
    };
    Format {
        column_limit: args
            .column_limit
            .or(cfg.column_limit)
            .unwrap_or(defaults.column_limit),
        indent,
        indent_columns: indent_width,
        spaces_in_expr_brackets: args
            .spaces_in_expr_brackets
            .or(cfg.spaces_in_expr_brackets)
            .unwrap_or(defaults.spaces_in_expr_brackets),
        array_spaces: args
            .array_spaces
            .or(cfg.array_spaces)
            .unwrap_or(defaults.array_spaces),
        array_multiline_commas: args
            .array_multiline_commas
            .or(cfg.array_multiline_commas)
            .unwrap_or(defaults.array_multiline_commas),
        array_multiline_trailing_commas: args
            .array_multiline_trailing_commas
            .or(cfg.array_multiline_trailing_commas)
            .unwrap_or(defaults.array_multiline_trailing_commas),
        array_singleline_trailing_commas: args
            .array_singleline_trailing_commas
            .or(cfg.array_singleline_trailing_commas)
            .unwrap_or(defaults.array_singleline_trailing_commas),
    }
}

fn print_diff(filename: &str, original: &str, formatted: &str) {
    use std::io::IsTerminal;

    let diff = TextDiff::from_lines(original, formatted);
    let color = std::io::stdout().is_terminal();

    print!("--- a/{filename}\n+++ b/{filename}\n");
    for hunk in diff.unified_diff().iter_hunks() {
        if color {
            println!("\x1b[36m{}\x1b[0m", hunk.header());
        } else {
            println!("{}", hunk.header());
        }
        for change in hunk.iter_changes() {
            match change.tag() {
                ChangeTag::Delete if color => print!("\x1b[31m-{}\x1b[0m", change.value()),
                ChangeTag::Insert if color => print!("\x1b[32m+{}\x1b[0m", change.value()),
                ChangeTag::Delete => print!("-{}", change.value()),
                ChangeTag::Insert => print!("+{}", change.value()),
                ChangeTag::Equal => print!(" {}", change.value()),
            }
            if change.missing_newline() {
                println!("\\ No newline at end of file");
            }
        }
    }
}

fn summary_line(reformatted: usize, unchanged: usize, check_mode: bool) {
    let verb = if check_mode {
        "would be reformatted"
    } else {
        "reformatted"
    };
    let files = |n: usize| if n == 1 { "file" } else { "files" };
    match (reformatted, unchanged) {
        (0, 0) => {}
        (r, 0) => eprintln!("{} {} {}.", r, files(r), verb),
        (0, u) => eprintln!("{} {} left unchanged.", u, files(u)),
        (r, u) => eprintln!(
            "{} {} {}, {} {} left unchanged.",
            r,
            files(r),
            verb,
            u,
            files(u)
        ),
    }
}

enum FileOutcome {
    Processed { original: String, formatted: String },
    Skipped,
    IoError(String),
    FormatError(String),
}

fn process_file(file: &str, format: &Format) -> FileOutcome {
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::InvalidData => return FileOutcome::Skipped,
        Err(e) => return FileOutcome::IoError(e.to_string()),
    };
    match sqfmt_lib::format_source(&source, format.clone()) {
        Ok(formatted) => FileOutcome::Processed {
            original: source,
            formatted,
        },
        Err(e) => FileOutcome::FormatError(e),
    }
}

fn main() {
    let args = Args::parse();

    let cfg = load_config(args.config.as_deref());
    let format = build_format(&args, &cfg);

    if args.files.is_empty() {
        let display_name = args.stdin_filename.as_deref().unwrap_or("<stdin>");
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .expect("Failed to read stdin");

        match sqfmt_lib::format_source(&source, format) {
            Ok(formatted) => {
                if args.diff {
                    if formatted != source {
                        print_diff(display_name, &source, &formatted);
                        std::process::exit(1);
                    }
                } else if args.check {
                    if formatted != source {
                        eprintln!("{}: would reformat", display_name);
                        std::process::exit(1);
                    }
                } else {
                    print!("{}", formatted);
                }
            }
            Err(e) => {
                eprintln!("{}: {}", display_name, e);
                std::process::exit(1);
            }
        }
    } else {
        let mut had_error = false;
        let mut files: Vec<String> = Vec::new();

        for path in &args.files {
            if Path::new(path).is_dir() {
                if !args.recursive {
                    eprintln!("{}: is a directory (use -r to format recursively)", path);
                    had_error = true;
                    continue;
                }
                collect_squirrel_files(Path::new(path), &mut files);
            } else {
                files.push(path.clone());
            }
        }

        let total = files.len();
        let tracking = args.inplace || args.check || args.diff;
        let show_progress = !args.quiet && (args.verbose || (total > 1 && tracking));
        let mut reformatted = 0usize;
        let mut unchanged = 0usize;

        let outcomes: Vec<(String, FileOutcome)> = files
            .par_iter()
            .map(|file| (file.clone(), process_file(file, &format)))
            .collect();

        for (i, (file, outcome)) in outcomes.into_iter().enumerate() {
            if show_progress {
                eprintln!("[{}/{}] {}", i + 1, total, file);
            }
            match outcome {
                FileOutcome::Skipped => {
                    eprintln!("{}: skipping (not valid UTF-8)", file);
                }
                FileOutcome::IoError(e) => {
                    eprintln!("{}: {}", file, e);
                    had_error = true;
                }
                FileOutcome::FormatError(e) => {
                    eprintln!("{}: {}", file, e);
                    had_error = true;
                }
                FileOutcome::Processed { original, formatted } => {
                    let changed = original != formatted;
                    if args.diff && changed {
                        print_diff(&file, &original, &formatted);
                    }
                    if args.check {
                        if changed {
                            eprintln!("{}: would reformat", file);
                            reformatted += 1;
                        } else {
                            unchanged += 1;
                        }
                    } else if args.inplace {
                        if changed {
                            if let Err(e) = std::fs::write(&file, &formatted) {
                                eprintln!("{}: {}", file, e);
                                had_error = true;
                                continue;
                            }
                            reformatted += 1;
                        } else {
                            unchanged += 1;
                        }
                    } else if args.diff {
                        if changed { reformatted += 1; } else { unchanged += 1; }
                    } else {
                        print!("{}", formatted);
                    }
                }
            }
        }

        let dry_run = args.check || (args.diff && !args.inplace);
        if tracking && !args.quiet {
            summary_line(reformatted, unchanged, dry_run);
        }

        if dry_run && reformatted > 0 {
            had_error = true;
        }

        if had_error {
            std::process::exit(1);
        }
    }
}
