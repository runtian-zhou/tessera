use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn join(self, other: Span) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Node<T> {
    pub span: Span,
    pub kind: T,
}

impl<T> Node<T> {
    pub fn new(span: Span, kind: T) -> Self {
        Self { span, kind }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Node<U> {
        Node {
            span: self.span,
            kind: f(self.kind),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceFile<'a> {
    pub name: &'a str,
    pub text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceFile<'a> {
    pub fn new(name: &'a str, text: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (idx, ch) in text.char_indices() {
            if ch == '\n' {
                line_starts.push(idx + 1);
            }
        }
        Self {
            name,
            text,
            line_starts,
        }
    }

    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        (line_idx + 1, offset.saturating_sub(line_start) + 1)
    }

    pub fn format_span(&self, span: Span) -> String {
        let (line, col) = self.line_col(span.start);
        format!("{}:{}:{}", self.name, line, col)
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}
