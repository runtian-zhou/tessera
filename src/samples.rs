pub const READER_PROGRAM: &str = r#"
const PAGE_SIZE: usize = 4 * 1024;

interface Reader {
    fn read(self: &mut Self, buf: [u8; PAGE_SIZE]) -> i32;
}

struct File {}

impl Reader for File {
    fn read(self: &mut Self, buf: [u8; 4096]) -> i32 {
        0
    }
}

fn consume<T: Reader>(r: &mut T, buf: [u8; PAGE_SIZE]) -> i32 {
    r.read(buf)
}
"#;

pub const OPTION_PROGRAM: &str = r#"
enum Option<T> {
    None,
    Some(T),
}

fn unwrap_or_zero(x: Option<i32>) -> i32 {
    match x {
        Option::None => 0,
        Option::Some(n) => n,
    }
}
"#;

pub const BUFFER_ASSOC_CONST_PROGRAM: &str = r#"
interface Buffer {
    const SIZE: usize;
    fn read(self: &Self, buf: [u8; <Self as Buffer>::SIZE]) -> usize;
}

struct Page {}

impl Buffer for Page {
    const SIZE: usize = 4096;
    fn read(self: &Self, buf: [u8; 4096]) -> usize {
        0
    }
}
"#;

pub const INTERFACE_AS_TYPE_PROGRAM: &str = r#"
interface Reader {
    fn read(self: &Self) -> i32;
}

fn bad(x: Reader) -> i32 {
    0
}
"#;

pub const CONST_OVERFLOW_PROGRAM: &str = r#"
const BAD: u8 = 256;
"#;

pub const MISSING_IMPL_METHOD_PROGRAM: &str = r#"
interface Reader {
    fn read(self: &Self) -> i32;
}

struct File {}

impl Reader for File {}
"#;
