use tessera::samples::*;
use tessera::{check_program, parse_program, DiagnosticSeverity};

fn parse_and_check(src: &str) -> Vec<String> {
    let program = parse_program(src).expect("sample should parse");
    check_program(&program)
        .diagnostics
        .into_iter()
        .map(|diag| {
            assert_eq!(diag.severity, DiagnosticSeverity::Error);
            diag.message
        })
        .collect()
}

#[test]
fn reader_interface_method_call_typechecks() {
    let diagnostics = parse_and_check(READER_PROGRAM);
    assert_eq!(diagnostics, Vec::<String>::new());
}

#[test]
fn enum_match_typechecks() {
    let diagnostics = parse_and_check(OPTION_PROGRAM);
    assert_eq!(diagnostics, Vec::<String>::new());
}

#[test]
fn associated_const_signature_typechecks() {
    let diagnostics = parse_and_check(BUFFER_ASSOC_CONST_PROGRAM);
    assert_eq!(diagnostics, Vec::<String>::new());
}

#[test]
fn rejects_non_exhaustive_enum_match_with_nested_binding() {
    let diagnostics = parse_and_check(
        r#"
enum Option<T> {
    None,
    Some(T),
}

fn only_some(x: Option<i32>) -> i32 {
    match x {
        Option::Some(n) => n,
    }
}
"#,
    );
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg.contains("match is not exhaustive")),
        "{diagnostics:#?}"
    );
}

#[test]
fn rejects_interface_as_value_type() {
    let diagnostics = parse_and_check(INTERFACE_AS_TYPE_PROGRAM);
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg.contains("interface `Reader` is not a value type")),
        "{diagnostics:#?}"
    );
}

#[test]
fn rejects_const_overflow() {
    let diagnostics = parse_and_check(CONST_OVERFLOW_PROGRAM);
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg.contains("does not fit in u8")),
        "{diagnostics:#?}"
    );
}

#[test]
fn rejects_missing_impl_method() {
    let diagnostics = parse_and_check(MISSING_IMPL_METHOD_PROGRAM);
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg.contains("missing method `read`")),
        "{diagnostics:#?}"
    );
}

#[test]
fn parser_errors_have_source_spans() {
    let err = parse_program("fn broken(").expect_err("invalid program");
    let first = err.diagnostics.first().expect("diagnostic");
    assert!(first.span.start <= first.span.end);
    assert!(!first.message.is_empty());
}
