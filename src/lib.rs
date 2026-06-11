pub mod ast;
pub mod diagnostic;
pub mod lexer;
pub mod parser;
pub mod samples;
pub mod span;
pub mod typeck;

pub mod tess_hir {
    pub use crate::ast::*;
}

pub use diagnostic::{Diagnostic, DiagnosticSeverity};
pub use parser::{parse_program, ParseError};
pub use span::{Node, Span};
pub use typeck::{check_program, TypeCheckReport};
