use crate::diagnostic::Diagnostic;
use crate::span::{Node, Span};
use num_bigint::BigInt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Ident(String),
    Int(BigInt),
    Keyword(Keyword),
    Symbol(Symbol),
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Keyword {
    Const,
    Struct,
    Enum,
    Interface,
    Impl,
    For,
    Fn,
    SelfLower,
    SelfUpper,
    Mut,
    Where,
    Match,
    If,
    Else,
    Return,
    True,
    False,
    As,
    Let,
    Todo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Symbol {
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    EqEq,
    Ne,
    Bang,
    Colon,
    ColonColon,
    Semi,
    Comma,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    AmpAmp,
    Pipe,
    PipePipe,
    Caret,
    Shl,
    Shr,
    Arrow,
    FatArrow,
}

pub type Token = Node<TokenKind>;

pub fn lex(input: &str) -> Result<Vec<Token>, Vec<Diagnostic>> {
    let mut lexer = Lexer {
        input,
        offset: 0,
        diagnostics: vec![],
        tokens: vec![],
    };
    lexer.run();
    if lexer.diagnostics.is_empty() {
        Ok(lexer.tokens)
    } else {
        Err(lexer.diagnostics)
    }
}

struct Lexer<'a> {
    input: &'a str,
    offset: usize,
    diagnostics: Vec<Diagnostic>,
    tokens: Vec<Token>,
}

impl Lexer<'_> {
    fn run(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.bump_char();
                continue;
            }

            if ch == '/' && self.peek_next_char() == Some('/') {
                self.bump_char();
                self.bump_char();
                while let Some(ch) = self.peek_char() {
                    self.bump_char();
                    if ch == '\n' {
                        break;
                    }
                }
                continue;
            }

            if ch.is_ascii_alphabetic() || ch == '_' {
                self.lex_ident_or_keyword();
                continue;
            }

            if ch.is_ascii_digit() {
                self.lex_int();
                continue;
            }

            self.lex_symbol();
        }
        let eof = Span::new(self.offset, self.offset);
        self.tokens.push(Node::new(eof, TokenKind::Eof));
    }

    fn lex_ident_or_keyword(&mut self) {
        let start = self.offset;
        self.bump_char();
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                self.bump_char();
            } else {
                break;
            }
        }
        let text = &self.input[start..self.offset];
        let kind = match text {
            "const" => TokenKind::Keyword(Keyword::Const),
            "struct" => TokenKind::Keyword(Keyword::Struct),
            "enum" => TokenKind::Keyword(Keyword::Enum),
            "interface" => TokenKind::Keyword(Keyword::Interface),
            "impl" => TokenKind::Keyword(Keyword::Impl),
            "for" => TokenKind::Keyword(Keyword::For),
            "fn" => TokenKind::Keyword(Keyword::Fn),
            "self" => TokenKind::Keyword(Keyword::SelfLower),
            "Self" => TokenKind::Keyword(Keyword::SelfUpper),
            "mut" => TokenKind::Keyword(Keyword::Mut),
            "where" => TokenKind::Keyword(Keyword::Where),
            "match" => TokenKind::Keyword(Keyword::Match),
            "if" => TokenKind::Keyword(Keyword::If),
            "else" => TokenKind::Keyword(Keyword::Else),
            "return" => TokenKind::Keyword(Keyword::Return),
            "true" => TokenKind::Keyword(Keyword::True),
            "false" => TokenKind::Keyword(Keyword::False),
            "as" => TokenKind::Keyword(Keyword::As),
            "let" => TokenKind::Keyword(Keyword::Let),
            "todo" => TokenKind::Keyword(Keyword::Todo),
            _ => TokenKind::Ident(text.to_owned()),
        };
        self.tokens
            .push(Node::new(Span::new(start, self.offset), kind));
    }

    fn lex_int(&mut self) {
        let start = self.offset;
        self.bump_char();
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                self.bump_char();
            } else {
                break;
            }
        }
        let text = &self.input[start..self.offset];
        let value = text.parse::<BigInt>().expect("decimal literal");
        self.tokens.push(Node::new(
            Span::new(start, self.offset),
            TokenKind::Int(value),
        ));
    }

    fn lex_symbol(&mut self) {
        let start = self.offset;
        let Some(ch) = self.bump_char() else {
            return;
        };
        let kind = match ch {
            '(' => Some(Symbol::LParen),
            ')' => Some(Symbol::RParen),
            '{' => Some(Symbol::LBrace),
            '}' => Some(Symbol::RBrace),
            '[' => Some(Symbol::LBracket),
            ']' => Some(Symbol::RBracket),
            ':' if self.consume_char(':') => Some(Symbol::ColonColon),
            ':' => Some(Symbol::Colon),
            ';' => Some(Symbol::Semi),
            ',' => Some(Symbol::Comma),
            '.' => Some(Symbol::Dot),
            '+' => Some(Symbol::Plus),
            '-' if self.consume_char('>') => Some(Symbol::Arrow),
            '-' => Some(Symbol::Minus),
            '*' => Some(Symbol::Star),
            '/' => Some(Symbol::Slash),
            '%' => Some(Symbol::Percent),
            '&' if self.consume_char('&') => Some(Symbol::AmpAmp),
            '&' => Some(Symbol::Amp),
            '|' if self.consume_char('|') => Some(Symbol::PipePipe),
            '|' => Some(Symbol::Pipe),
            '^' => Some(Symbol::Caret),
            '=' if self.consume_char('=') => Some(Symbol::EqEq),
            '=' if self.consume_char('>') => Some(Symbol::FatArrow),
            '=' => Some(Symbol::Eq),
            '!' if self.consume_char('=') => Some(Symbol::Ne),
            '!' => Some(Symbol::Bang),
            '<' if self.consume_char('<') => Some(Symbol::Shl),
            '<' if self.consume_char('=') => Some(Symbol::Le),
            '<' => Some(Symbol::Lt),
            '>' if self.consume_char('>') => Some(Symbol::Shr),
            '>' if self.consume_char('=') => Some(Symbol::Ge),
            '>' => Some(Symbol::Gt),
            _ => None,
        };
        if let Some(symbol) = kind {
            self.tokens.push(Node::new(
                Span::new(start, self.offset),
                TokenKind::Symbol(symbol),
            ));
        } else {
            self.diagnostics.push(Diagnostic::error(
                Span::new(start, self.offset),
                format!("unexpected character `{ch}`"),
            ));
        }
    }

    fn consume_char(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.bump_char();
            true
        } else {
            false
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.offset..].chars().next()
    }

    fn peek_next_char(&self) -> Option<char> {
        let mut chars = self.input[self.offset..].chars();
        chars.next()?;
        chars.next()
    }

    fn bump_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.offset += ch.len_utf8();
        Some(ch)
    }
}
