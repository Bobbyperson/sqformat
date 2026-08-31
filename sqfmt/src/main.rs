use clap::Parser;
use rayon::prelude::*;
use similar::{ChangeTag, TextDiff};
use std::io::{self, Read};
use std::path::Path;

use sqfmt_lib::config::{self, FileConfig, Format};

/// Squirrel code formatter and linter.
///
/// If no files are given, reads from stdin and writes formatted code to stdout.
/// If files are given, writes formatted output to stdout (or edits in-place with -i).
#[derive(Parser, Debug)]
#[clap(author, version)]
struct Args {
    /// Scan Squirrel code for bad patterns. Exits with 1 if any are found.
    #[clap(short = 'l', long, conflicts_with_all = ["inplace", "check", "diff"])]
    lint: bool,

    /// Include advisory checks that may rely on programmer-known lifetime or scheduling guarantees.
    #[clap(long, requires = "lint")]
    advisory_lints: bool,

    /// Emit GitHub Actions warning and error annotations.
    #[clap(long, requires = "lint")]
    github_actions: bool,

    /// Edit files in-place (only valid with file arguments).
    #[clap(short, long, conflicts_with = "check")]
    inplace: bool,

    /// Check if files are formatted without writing changes. Exits with 1 if any file would change.
    #[clap(short, long)]
    check: bool,

    /// Show a unified diff of changes. Exits with 1 if any file would change.
    #[clap(short, long)]
    diff: bool,

    /// Recursively format directories (lint always recurses).
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

    /// Files or directories to process.
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

fn collect_lint_files(
    dir: &Path,
    squirrel: &mut Vec<String>,
    manifests: &mut Vec<String>,
    errors: &mut Vec<(String, String)>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            errors.push((dir.to_string_lossy().into_owned(), error.to_string()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push((dir.to_string_lossy().into_owned(), error.to_string()));
                continue;
            }
        };
        let path = entry.path();
        if path.is_dir() {
            collect_lint_files(&path, squirrel, manifests, errors);
        } else if is_squirrel_file(&path) {
            squirrel.push(path.to_string_lossy().into_owned());
        } else if path.file_name().is_some_and(|name| name == "mod.json") {
            manifests.push(path.to_string_lossy().into_owned());
        }
    }
}

/// The format to use: the discovered or named `.sqformat.toml`, with command-line flags on top.
///
/// Discovery lives in `sqfmt_lib::config` so the language server finds the same file the same way.
fn build_format(args: &Args) -> Format {
    let file = match &args.config {
        // A named config must exist; a discovered one is optional.
        Some(path) => Some(exit_on_error(FileConfig::read(Path::new(path)))),
        None => config::find(&std::env::current_dir().unwrap_or_default())
            .map(|path| exit_on_error(FileConfig::read(&path))),
    };
    let mut format = exit_on_error(file.unwrap_or_default().apply(Format::default()));
    if let Some(column_limit) = args.column_limit {
        format.column_limit = column_limit;
    }
    if args.indent_style.is_some() || args.indent_width.is_some() {
        let style = args
            .indent_style
            .as_deref()
            .unwrap_or_else(|| format.indent_style());
        let width = args.indent_width.unwrap_or(format.indent_columns);
        exit_on_error(format.set_indent(style, width));
    }
    if let Some(value) = args.spaces_in_expr_brackets {
        format.spaces_in_expr_brackets = value;
    }
    if let Some(value) = args.array_spaces {
        format.array_spaces = value;
    }
    if let Some(value) = args.array_multiline_commas {
        format.array_multiline_commas = value;
    }
    if let Some(value) = args.array_multiline_trailing_commas {
        format.array_multiline_trailing_commas = value;
    }
    if let Some(value) = args.array_singleline_trailing_commas {
        format.array_singleline_trailing_commas = value;
    }
    format
}

fn exit_on_error<T>(result: Result<T, config::ConfigError>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
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

fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = before
        .rsplit_once('\n')
        .map_or(before, |(_, line)| line)
        .chars()
        .count()
        + 1;
    (line, column)
}

fn github_escape_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn github_escape_property(value: &str) -> String {
    github_escape_data(value)
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn github_annotation(
    level: &str,
    file: &str,
    position: Option<(usize, usize)>,
    title: &str,
    message: &str,
) -> String {
    let mut properties = format!("file={}", github_escape_property(file));
    if let Some((line, column)) = position {
        properties.push_str(&format!(",line={line},col={column}"));
    }
    properties.push_str(&format!(",title={}", github_escape_property(title)));
    format!("::{level} {properties}::{}", github_escape_data(message))
}

fn lint_message(diagnostic: &sqfmt_lint::Diagnostic) -> String {
    let suffix = format!("[{}]", diagnostic.rule);
    if diagnostic.message.ends_with(&suffix) {
        diagnostic.message.clone()
    } else {
        format!("{} {suffix}", diagnostic.message)
    }
}

fn print_lint_diagnostic(
    github_actions: bool,
    file: &str,
    source: &str,
    diagnostic: &sqfmt_lint::Diagnostic,
) {
    let (line, column) = line_column(source, diagnostic.range.start);
    let message = lint_message(diagnostic);
    if github_actions {
        println!(
            "{}",
            github_annotation(
                "warning",
                file,
                Some((line, column)),
                diagnostic.rule,
                &message,
            )
        );
    } else {
        eprintln!("{}:{}:{}: {}", file, line, column, message);
    }
}

fn print_lint_error(github_actions: bool, file: &str, error: &str) {
    if github_actions {
        println!(
            "{}",
            github_annotation("error", file, None, "sqformat", error)
        );
    } else {
        eprintln!("{}: {}", file, error);
    }
}

fn run_lint(args: &Args) -> bool {
    let mut files = Vec::new();
    let mut manifests = Vec::new();
    let mut discovery_errors = Vec::new();
    let paths: Vec<&str> = if args.files.is_empty() {
        vec!["."]
    } else {
        args.files.iter().map(String::as_str).collect()
    };

    for path in paths {
        if Path::new(path).is_dir() {
            collect_lint_files(
                Path::new(path),
                &mut files,
                &mut manifests,
                &mut discovery_errors,
            );
        } else if Path::new(path)
            .file_name()
            .is_some_and(|name| name == "mod.json")
        {
            manifests.push(path.to_string());
        } else {
            files.push(path.to_string());
        }
    }
    files.sort();
    files.dedup();
    manifests.sort();
    manifests.dedup();
    let script_targets = manifests
        .iter()
        .flat_map(|manifest| sqfmt_lint::read_manifest(Path::new(manifest)))
        .map(|(path, entry)| (path, entry.targets))
        .collect::<std::collections::HashMap<_, _>>();

    let results: Vec<_> = files
        .par_iter()
        .map(|file| {
            let analysis = std::fs::read_to_string(file)
                .map_err(|error| error.to_string())
                .and_then(|source| {
                    sqfmt_lint::analyze(&source).map(|analysis| {
                        let semantic = sqfmt_lint::semantic::analyze(&source);
                        (source, analysis, semantic)
                    })
                });
            (file.clone(), analysis)
        })
        .collect();

    discovery_errors.sort();
    discovery_errors.dedup();
    let mut had_error = !discovery_errors.is_empty();
    for (path, error) in discovery_errors {
        print_lint_error(args.github_actions, &path, &error);
    }
    let mut successful_files = Vec::new();
    let mut analyses = Vec::new();
    let mut semantics = Vec::new();
    for (file, result) in results {
        match result {
            Ok((source, analysis, semantic)) => {
                successful_files.push((file, source));
                analyses.push(analysis);
                semantics.push(semantic);
            }
            Err(error) => {
                print_lint_error(args.github_actions, &file, &error);
                had_error = true;
            }
        }
    }

    let workspace = sqfmt_lint::Workspace::new(&analyses);
    let semantic_workspace = sqfmt_lint::SemanticWorkspace::new(
        successful_files
            .iter()
            .zip(&semantics)
            .map(|((file, _), semantic)| sqfmt_lint::SemanticFile {
                id: file.clone(),
                document: semantic,
                targets: script_targets
                    .get(Path::new(file))
                    .copied()
                    .unwrap_or(sqfmt_lint::VmTargets::ALL),
            }),
    );
    let mut finding_count = 0;
    for ((file, source), analysis) in successful_files.iter().zip(&analyses) {
        let mut diagnostics = workspace.diagnostics_with_options(
            analysis,
            sqfmt_lint::LintOptions {
                advisory: args.advisory_lints,
            },
        );
        diagnostics.extend(semantic_workspace.diagnostics(file));
        analysis.retain_unsuppressed(&mut diagnostics);
        diagnostics.sort_by(|left, right| {
            left.range
                .start
                .cmp(&right.range.start)
                .then_with(|| left.range.end.cmp(&right.range.end))
                .then_with(|| left.rule.cmp(right.rule))
        });
        diagnostics.dedup();
        for diagnostic in diagnostics {
            print_lint_diagnostic(args.github_actions, file, source, &diagnostic);
            finding_count += 1;
        }
    }

    let mut manifest_count = 0;
    for manifest in manifests {
        match std::fs::read_to_string(&manifest) {
            Ok(source) => {
                manifest_count += 1;
                for diagnostic in workspace.manifest_diagnostics(&source) {
                    print_lint_diagnostic(args.github_actions, &manifest, &source, &diagnostic);
                    finding_count += 1;
                }
            }
            Err(error) => {
                print_lint_error(args.github_actions, &manifest, &error.to_string());
                had_error = true;
            }
        }
    }

    if !args.quiet {
        let files = successful_files.len() + manifest_count;
        eprintln!(
            "Scanned {} {}, found {} {}.",
            files,
            if files == 1 { "file" } else { "files" },
            finding_count,
            if finding_count == 1 {
                "problem"
            } else {
                "problems"
            }
        );
    }

    had_error || finding_count > 0
}

fn main() {
    let args = Args::parse();

    if args.lint {
        if run_lint(&args) {
            std::process::exit(1);
        }
        return;
    }

    let format = build_format(&args);

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
                FileOutcome::Processed {
                    original,
                    formatted,
                } => {
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
                        if changed {
                            reformatted += 1;
                        } else {
                            unchanged += 1;
                        }
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

#[cfg(test)]
mod tests {
    use super::github_annotation;

    #[test]
    fn github_annotations_escape_untrusted_command_data_and_titles() {
        assert_eq!(
            github_annotation(
                "warning",
                "script.gnut",
                None,
                "rule,group:100%",
                "first%\r\nsecond",
            ),
            "::warning file=script.gnut,title=rule%2Cgroup%3A100%25::first%25%0D%0Asecond"
        );
    }
}
