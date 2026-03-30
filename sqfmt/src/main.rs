use clap::Parser;
use std::io::{self, Read};
use std::path::Path;

/// Squirrel code formatter.
///
/// If no files are given, reads from stdin and writes formatted code to stdout.
/// If files are given, writes formatted output to stdout (or edits in-place with -i).
#[derive(Parser, Debug)]
#[clap(author, version)]
struct Args {
    /// Edit files in-place (only valid with file arguments).
    #[clap(short)]
    inplace_edit: bool,

    /// Recursively format directories.
    #[clap(short, long)]
    recursive: bool,

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

fn format_source(source: &str, filename: &str) -> Result<String, String> {
    sqfmt_lib::format_source_default(source).map_err(|e| format!("{}: {}", filename, e))
}

fn main() {
    let args = Args::parse();

    if args.files.is_empty() {
        // Read from stdin
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .expect("Failed to read stdin");

        match format_source(&source, "<stdin>") {
            Ok(formatted) => {
                print!("{}", formatted);
            }
            Err(e) => {
                eprintln!("{}", e);
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

        for file in &files {
            let source = match std::fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    if e.kind() == io::ErrorKind::InvalidData {
                        eprintln!("{}: skipping (not valid UTF-8)", file);
                    } else {
                        eprintln!("{}: {}", file, e);
                        had_error = true;
                    }
                    continue;
                }
            };

            match format_source(&source, file) {
                Ok(formatted) => {
                    if args.inplace_edit {
                        if let Err(e) = std::fs::write(file, &formatted) {
                            eprintln!("{}: {}", file, e);
                            had_error = true;
                        }
                    } else {
                        print!("{}", formatted);
                    }
                }
                Err(e) => {
                    eprintln!("{}", e);
                    had_error = true;
                }
            }
        }

        if had_error {
            std::process::exit(1);
        }
    }
}
