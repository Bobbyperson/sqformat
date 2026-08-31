use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct TempFixture {
    path: PathBuf,
}

impl TempFixture {
    fn new() -> Self {
        let unique = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sqformat-cli-lint-{}-{}-{unique}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is before the Unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("create fixture directory");
        Self { path }
    }

    fn write(&self, relative: &str, source: &str) {
        let path = self.path.join(relative);
        fs::create_dir_all(path.parent().expect("fixture file has a parent"))
            .expect("create fixture file parent");
        fs::write(path, source).expect("write fixture source");
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sqformat"))
            .args(args)
            .current_dir(&self.path)
            .output()
            .expect("run sqformat")
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("sqformat emitted UTF-8 stdout")
}

#[test]
fn github_annotations_escape_file_properties_from_cli_input() {
    let fixture = TempFixture::new();
    let file = "scripts/a,b:c%.gnut";
    fixture.write(file, "void function Poll() { wait 0 }");

    let output = fixture.run(&["--lint", "--github-actions", "--quiet", file]);

    assert!(!output.status.success());
    assert_eq!(
        stdout(&output),
        "::warning file=scripts/a%2Cb%3Ac%25.gnut,line=1,col=29,title=wait-zero::`wait 0` does not advance a game frame; use WaitFrame() [wait-zero]\n"
    );
}

#[test]
fn github_error_annotations_omit_positions_for_unreadable_targets() {
    let fixture = TempFixture::new();
    let missing_directory = "missing-directory";

    let result = fixture.run(&["--lint", "--github-actions", "--quiet", missing_directory]);
    let output = stdout(&result);

    assert!(!result.status.success());
    assert!(output.starts_with("::error file=missing-directory,title=sqformat::"));
    assert!(!output.contains(",line="));
    assert!(!output.contains(",col="));
}

#[test]
fn github_annotation_columns_count_unicode_characters() {
    let fixture = TempFixture::new();
    fixture.write(
        "unicode.gnut",
        "void function Poll() { local label = \"é\"; wait 0 }",
    );

    let output = fixture.run(&["--lint", "--github-actions", "--quiet", "unicode.gnut"]);

    assert!(!output.status.success());
    assert!(stdout(&output).contains("line=1,col=48"));
}

#[test]
fn lint_output_includes_each_rule_id_once() {
    let fixture = TempFixture::new();
    fixture.write("rule.gnut", "void function Poll() { wait 0 }");

    let result = fixture.run(&["--lint", "--github-actions", "--quiet", "rule.gnut"]);
    let output = stdout(&result);

    assert!(!result.status.success());
    assert_eq!(output.matches("[wait-zero]").count(), 1);
}

#[test]
fn nonexistent_directory_reports_a_read_error_and_fails() {
    let fixture = TempFixture::new();
    let missing_directory = fixture
        .path
        .join("missing-directory")
        .to_string_lossy()
        .into_owned();

    let output = fixture.run(&["--lint", "--quiet", &missing_directory]);

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("sqformat emitted UTF-8 stderr");
    assert!(stderr.starts_with(&format!("{missing_directory}: ")));
}
