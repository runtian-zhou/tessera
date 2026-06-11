use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tesshir::span::SourceFile;
use tesshir::{check_program, parse_program, Diagnostic};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

const CLI_CASES: &[CliCase] = &[
    CliCase {
        name: "check_ok",
        fixture_dir: "check",
        fixture: "reader_interface",
        args: &[],
    },
    CliCase {
        name: "dump_ast",
        fixture_dir: "dump_ast",
        fixture: "simple_fn",
        args: &["--dump-ast"],
    },
    CliCase {
        name: "type_error",
        fixture_dir: "check",
        fixture: "const_overflow",
        args: &[],
    },
];

struct CliCase {
    name: &'static str,
    fixture_dir: &'static str,
    fixture: &'static str,
    args: &'static [&'static str],
}

#[test]
fn check_goldens() {
    for source_path in fixture_sources("check") {
        let golden_path = source_path.with_extension("golden");
        let actual = run_check_case(&source_path);
        assert_or_update_golden(&golden_path, &actual);
    }
}

#[test]
fn dump_ast_goldens() {
    for source_path in fixture_sources("dump_ast") {
        let golden_path = source_path.with_extension("golden");
        let actual = run_dump_ast_case(&source_path);
        assert_or_update_golden(&golden_path, &actual);
    }
}

#[test]
fn cli_goldens() {
    for case in CLI_CASES {
        let source_path = fixture_source(case.fixture_dir, case.fixture);
        let golden_path = fixture_golden("cli", case.name);
        let output = Command::new(env!("CARGO_BIN_EXE_tesshir"))
            .args(case.args)
            .arg(&source_path)
            .output()
            .expect("run tesshir");

        let stdout = normalize_paths(
            &String::from_utf8(output.stdout).expect("stdout"),
            &source_path,
        );
        let stderr = normalize_paths(
            &String::from_utf8(output.stderr).expect("stderr"),
            &source_path,
        );
        let actual = format!(
            "status: {}\nstdout:\n{}stderr:\n{}",
            output.status.code().unwrap_or(-1),
            stdout,
            stderr
        );
        assert_or_update_golden(&golden_path, &actual);
    }
}

fn run_check_case(source_path: &Path) -> String {
    let source = fs::read_to_string(source_path).expect("read fixture");
    let source_name = logical_fixture_name(source_path);
    let source_file = SourceFile::new(&source_name, &source);

    match parse_program(&source) {
        Ok(program) => {
            let report = check_program(&program);
            if report.diagnostics.is_empty() {
                "ok\n".to_owned()
            } else {
                render_diagnostics(&report.diagnostics, &source_file)
            }
        }
        Err(err) => render_diagnostics(&err.diagnostics, &source_file),
    }
}

fn run_dump_ast_case(source_path: &Path) -> String {
    let source = fs::read_to_string(source_path).expect("read fixture");
    let source_name = logical_fixture_name(source_path);
    let source_file = SourceFile::new(&source_name, &source);

    match parse_program(&source) {
        Ok(program) => format!("{program:#?}\n"),
        Err(err) => render_diagnostics(&err.diagnostics, &source_file),
    }
}

fn render_diagnostics(diagnostics: &[Diagnostic], source: &SourceFile<'_>) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.render(source))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn assert_or_update_golden(path: &Path, actual: &str) {
    let actual = normalize_newlines(actual);
    if env::var_os("TESSHIR_UPDATE_GOLDEN").is_some() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create golden directory");
        }
        fs::write(path, actual).expect("write golden");
        return;
    }

    let expected = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read golden `{}`: {err}", path.display()));
    assert_eq!(
        normalize_newlines(&expected),
        actual,
        "golden mismatch for `{}`; rerun with TESSHIR_UPDATE_GOLDEN=1 to update",
        path.display()
    );
}

fn fixture_source(dir: &str, name: &str) -> PathBuf {
    Path::new(FIXTURES).join(dir).join(format!("{name}.tess"))
}

fn fixture_golden(dir: &str, name: &str) -> PathBuf {
    Path::new(FIXTURES).join(dir).join(format!("{name}.golden"))
}

fn fixture_sources(dir: &str) -> Vec<PathBuf> {
    let root = Path::new(FIXTURES).join(dir);
    assert_fixture_dir_is_clean(&root);

    let mut sources = Vec::new();
    collect_fixture_files(&root, "tess", &mut sources);
    sources.sort();
    assert!(
        !sources.is_empty(),
        "no `.tess` fixtures found under `{}`",
        root.display()
    );
    sources
}

fn assert_fixture_dir_is_clean(root: &Path) {
    let mut goldens = Vec::new();
    collect_fixture_files(root, "golden", &mut goldens);
    for golden in goldens {
        let source = golden.with_extension("tess");
        assert!(
            source.exists(),
            "stale golden `{}` has no matching source `{}`",
            golden.display(),
            source.display()
        );
    }
}

fn collect_fixture_files(root: &Path, extension: &str, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(root).unwrap_or_else(|err| {
        panic!(
            "failed to read fixture directory `{}`: {err}",
            root.display()
        )
    });
    for entry in entries {
        let path = entry.expect("read fixture directory entry").path();
        if path.is_dir() {
            collect_fixture_files(&path, extension, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
            files.push(path);
        }
    }
}

fn logical_fixture_name(path: &Path) -> String {
    path.strip_prefix(FIXTURES)
        .expect("fixture under root")
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn normalize_paths(output: &str, source_path: &Path) -> String {
    let logical = logical_fixture_name(source_path);
    let mut output = output.replace(&source_path.to_string_lossy().to_string(), &logical);
    if let Ok(canonical) = source_path.canonicalize() {
        output = output.replace(&canonical.to_string_lossy().to_string(), &logical);
    }
    normalize_newlines(&output)
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}
