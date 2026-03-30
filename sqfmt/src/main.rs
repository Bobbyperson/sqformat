use clap::Parser;
use std::io::{self, Read};

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

    /// Files to format.
    files: Vec<String>,
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

        for file in &args.files {
            let source = match std::fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}: {}", file, e);
                    had_error = true;
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
