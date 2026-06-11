use tessera::samples::*;
use tessera::span::SourceFile;
use tessera::{check_program, parse_program, DiagnosticSeverity};

#[test]
fn tess_hir_public_api_is_exposed() {
    let _: Option<tessera::tess_hir::Program> = None;
    let _: tessera::tess_hir::IntTy = tessera::tess_hir::IntTy::i32();
}

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
fn struct_pattern_typechecks() {
    let diagnostics = parse_and_check(STRUCT_PATTERN_PROGRAM);
    assert_eq!(diagnostics, Vec::<String>::new());
}

#[test]
fn superinterface_proves_parent_associated_const() {
    let diagnostics = parse_and_check(SUPERINTERFACE_ASSOC_CONST_PROGRAM);
    assert_eq!(diagnostics, Vec::<String>::new());
}

#[test]
fn symbolic_const_generic_type_argument_typechecks() {
    let diagnostics = parse_and_check(CONST_GENERIC_TYPE_PROGRAM);
    assert_eq!(diagnostics, Vec::<String>::new());
}

#[test]
fn nested_generic_type_argument_typechecks() {
    let diagnostics = parse_and_check(NESTED_GENERIC_TYPE_PROGRAM);
    assert_eq!(diagnostics, Vec::<String>::new());
}

#[test]
fn const_equality_where_predicate_typechecks_when_normalized() {
    let diagnostics = parse_and_check(
        r#"
const PAGE: usize = 4 * 1024;

fn page() -> i32 where PAGE == 4096 {
    0
}
"#,
    );
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
fn rejects_interface_inheritance_cycle() {
    let diagnostics = parse_and_check(
        r#"
interface A: B {
    fn a(self: &Self) -> i32;
}

interface B: A {
    fn b(self: &Self) -> i32;
}
"#,
    );
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg.contains("interface inheritance cycle")),
        "{diagnostics:#?}"
    );
}

#[test]
fn rejects_conflicting_inherited_method() {
    let diagnostics = parse_and_check(
        r#"
interface ReadI32 {
    fn read(self: &Self) -> i32;
}

interface ReadBool {
    fn read(self: &Self) -> bool;
}

interface Both: ReadI32 + ReadBool {}
"#,
    );
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg.contains("conflicting inherited method `read`")),
        "{diagnostics:#?}"
    );
}

#[test]
fn rejects_duplicate_concrete_impl() {
    let diagnostics = parse_and_check(
        r#"
interface Reader {
    fn read(self: &Self) -> i32;
}

struct File {}

impl Reader for File {
    fn read(self: &Self) -> i32 { 0 }
}

impl Reader for File {
    fn read(self: &Self) -> i32 { 0 }
}
"#,
    );
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg.contains("conflicting impl of `Reader` for `File`")),
        "{diagnostics:#?}"
    );
}

#[test]
fn rejects_associated_const_default_overflow() {
    let diagnostics = parse_and_check(
        r#"
interface Buffer {
    const SIZE: u8 = 256;
}
"#,
    );
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg.contains("associated const default `256` does not fit in u8")),
        "{diagnostics:#?}"
    );
}

#[test]
fn rejects_associated_const_impl_type_mismatch() {
    let diagnostics = parse_and_check(
        r#"
interface Buffer {
    const SIZE: usize;
}

struct Page {}

impl Buffer for Page {
    const SIZE: u8 = 1;
}
"#,
    );
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg.contains("associated const `SIZE` has type `u8`, expected `usize`")),
        "{diagnostics:#?}"
    );
}

#[test]
fn rejects_const_generic_argument_overflow() {
    let diagnostics = parse_and_check(
        r#"
struct Small<const N: u8> {
    data: [u8; N],
}

struct Bad {
    small: Small<256>,
}
"#,
    );
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg
                .contains("const argument for `N` has value `256` which does not fit in u8")),
        "{diagnostics:#?}"
    );
}

#[test]
fn rejects_negative_array_length() {
    let diagnostics = parse_and_check(
        r#"
struct Bad {
    data: [u8; -1],
}
"#,
    );
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg.contains("array length must be nonnegative")),
        "{diagnostics:#?}"
    );
}

#[test]
fn rejects_failed_const_equality_where_predicate() {
    let diagnostics = parse_and_check(
        r#"
const PAGE: usize = 4 * 1024;

fn page() -> i32 where PAGE == 4097 {
    0
}
"#,
    );
    assert!(
        diagnostics
            .iter()
            .any(|msg| msg.contains("const equality predicate failed: `4096` != `4097`")),
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

#[test]
fn typechecker_errors_have_source_spans_and_rendered_locations() {
    let src = "const BAD: u8 = 256;";
    let program = parse_program(src).expect("program parses");
    let report = check_program(&program);
    let first = report.diagnostics.first().expect("diagnostic");
    assert!(first.span.start < first.span.end);
    assert!(!first.message.is_empty());

    let source = SourceFile::new("bad.tess", src);
    let rendered = first.render(&source);
    assert!(rendered.contains("bad.tess:1:17"), "{rendered}");
    assert!(rendered.contains("does not fit in u8"), "{rendered}");
}
