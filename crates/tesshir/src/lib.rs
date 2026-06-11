pub mod ast;
pub mod diagnostic;
pub mod lexer;
pub mod package;
pub mod parser;
pub mod samples;
pub mod span;
pub mod typeck;

pub use ast::*;
pub use diagnostic::{Diagnostic, DiagnosticSeverity};
pub use package::{load_package, Package, PackageLoadError, PackageOptions, PackageSourceMap};
pub use parser::{parse_program, ParseError};
pub use span::{Node, Span};
pub use typeck::{check_program, TypeCheckReport};
