# Design Doc: AST for Const Integers, Enums, and Monomorphizable Interfaces

## 1. Overview

This document specifies a Rust-style abstract syntax tree for a statically typed language with:

```text
1. Nominal structs
2. Nominal enums
3. Compile-time integer constants
4. Const generic parameters
5. Interfaces as monomorphizable compile-time contracts
6. Interface impls as compile-time evidence
7. Pattern matching over enums
```

The important semantic decision is:

```text
Interfaces are not runtime types.
Interfaces are not existential types.
Interfaces are not dynamically dispatched trait objects.
```

Instead:

```text
interface Reader { ... }
```

defines a compile-time predicate over types:

```text
Reader(T)
```

and:

```text
impl Reader for File { ... }
```

provides compile-time evidence that:

```text
Reader(File)
```

A generic function:

```text
fn use_reader<T: Reader>(x: T) { ... }
```

is monomorphized for each concrete `T`.

There is no equivalent of:

```rust
dyn Reader
```

and this should be rejected:

```text
fn bad(x: Reader) { ... }
```

because `Reader` is not a value type.

---

# 2. Design goals

## Goals

The AST should support:

```text
const PAGE_SIZE: usize = 4 * 1024;

interface Reader {
    fn read(self: &mut Self, buf: [u8; PAGE_SIZE]) -> i32;
}

struct File {}

impl Reader for File {
    fn read(self: &mut Self, buf: [u8; 4096]) -> i32 {
        ...
    }
}

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
```

The design should also support associated consts in interfaces:

```text
interface Buffer {
    const SIZE: usize;

    fn read(self: &Self, buf: [u8; <Self as Buffer>::SIZE]) -> usize;
}
```

and generic const-dependent enums:

```text
enum Packet<const N: usize> {
    Inline([u8; N]),
    Empty,
}
```

## Non-goals

This AST does **not** support:

```text
dyn Interface
interface values
existential interface packages
runtime vtables
dynamic dispatch
```

Interfaces are used only for generic bounds and impl resolution.

---

# 3. Compiler stages assumed by this design

This document defines the **source AST**, meaning the tree produced after parsing but before full name resolution and type checking.

Later compiler stages should perform:

```text
1. Name collection
2. Name resolution
3. Const evaluation
4. Type well-formedness checking
5. Interface checking
6. Impl checking
7. Function type checking
8. Monomorphization
9. Lowering/code generation
```

The source AST uses symbolic names like:

```rust
Path { segments: vec!["Option".into()] }
```

A later resolved AST or HIR can replace these with internal IDs such as:

```rust
StructId
EnumId
InterfaceId
ConstId
FnId
ImplId
```

Spans, source locations, attributes, visibility, and documentation comments are omitted here for clarity, but they should usually be added in a real compiler.

---

# 4. Names and paths

```rust
pub type Symbol = String;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Path {
    pub segments: Vec<Symbol>,
}
```

## Meaning

A `Path` represents an unresolved name path.

Examples:

```text
PAGE_SIZE
Option
Option::Some
foo::bar::Baz
```

AST form:

```rust
Path {
    segments: vec!["Option".into()],
}
```

or:

```rust
Path {
    segments: vec!["foo".into(), "bar".into(), "Baz".into()],
}
```

At the AST level, a `Path` does not yet know whether it names:

```text
a struct
an enum
an enum variant
a function
a const
a type parameter
an interface
an associated const
```

That is determined during name resolution.

---

# 5. Program and top-level items

```rust
#[derive(Clone, Debug)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Const(ConstItem),
    Struct(StructItem),
    Enum(EnumItem),
    Interface(InterfaceItem),
    Impl(ImplItem),
    Fn(FnItem),
}
```

## Meaning

A `Program` is a list of top-level declarations.

Each `Item` introduces or defines something in the program.

```text
Item::Const       top-level compile-time integer constant
Item::Struct      nominal product type
Item::Enum        nominal sum type
Item::Interface   compile-time contract over types
Item::Impl        implementation of an interface for a concrete type pattern
Item::Fn          function item
```

---

# 6. Integer types

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Signedness {
    Signed,
    Unsigned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IntWidth {
    W8,
    W16,
    W32,
    W64,
    W128,

    /// Pointer-sized integer.
    Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntTy {
    pub signedness: Signedness,
    pub width: IntWidth,
}
```

## Meaning

`IntTy` represents an integer type.

Examples:

```text
i32
u8
usize
isize
```

AST examples:

```rust
IntTy {
    signedness: Signedness::Signed,
    width: IntWidth::W32,
}
```

means:

```text
i32
```

and:

```rust
IntTy {
    signedness: Signedness::Unsigned,
    width: IntWidth::Size,
}
```

means:

```text
usize
```

The integer range is determined by both signedness and width:

```text
u8     0 .. 255
i8     -128 .. 127
u32    0 .. 4294967295
i32    -2147483648 .. 2147483647
usize  target-dependent unsigned pointer-sized integer
isize  target-dependent signed pointer-sized integer
```

For cross-platform compilation, `usize` and `isize` should be resolved using the target architecture during type checking or lowering.

---

# 7. Types

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Unit,
    Bool,
    Never,

    Int(IntTy),

    /// Unresolved nominal type path or type parameter.
    ///
    /// This can later resolve to:
    /// - struct type
    /// - enum type
    /// - type alias
    /// - generic type parameter
    ///
    /// It must not resolve to an interface in value type position.
    Path {
        path: Path,
        args: Vec<GenericArg>,
    },

    /// The special `Self` type inside interfaces and impls.
    SelfTy,

    Array {
        elem: Box<Ty>,
        len: ConstExpr,
    },

    Ref {
        mutability: Mutability,
        ty: Box<Ty>,
    },

    Fn {
        params: Vec<Ty>,
        ret: Box<Ty>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mutability {
    Shared,
    Mutable,
}
```

## Meaning

`Ty` represents a source-level type.

### Unit

```rust
Ty::Unit
```

means:

```text
()
```

### Bool

```rust
Ty::Bool
```

means:

```text
bool
```

### Never

```rust
Ty::Never
```

means:

```text
!
```

This is the type of expressions that never return, such as infinite loops, panics, or exits.

### Integer

```rust
Ty::Int(IntTy {
    signedness: Signedness::Signed,
    width: IntWidth::W32,
})
```

means:

```text
i32
```

### Path type

```rust
Ty::Path {
    path: Path {
        segments: vec!["Option".into()],
    },
    args: vec![
        GenericArg::Ty(Ty::Int(IntTy {
            signedness: Signedness::Signed,
            width: IntWidth::W32,
        })),
    ],
}
```

means:

```text
Option<i32>
```

`Ty::Path` is used for nominal types and generic type parameters.

Important invariant:

```text
A path used as a value type must not resolve to an interface.
```

So this is invalid:

```text
fn f(x: Reader) {}
```

because `Reader` is an interface, not a concrete type.

The programmer should instead write:

```text
fn f<T: Reader>(x: T) {}
```

### Self type

```rust
Ty::SelfTy
```

means:

```text
Self
```

It is valid only inside:

```text
interface declarations
impl declarations
method signatures
```

For example:

```text
interface CloneLike {
    fn clone(self: &Self) -> Self;
}
```

Since there are no dynamic traits, returning `Self` is allowed. This remains monomorphizable.

### Array type

```rust
Ty::Array {
    elem: Box::new(Ty::Int(IntTy {
        signedness: Signedness::Unsigned,
        width: IntWidth::W8,
    })),
    len: ConstExpr::Path(Path {
        segments: vec!["PAGE_SIZE".into()],
    }),
}
```

means:

```text
[u8; PAGE_SIZE]
```

The length is a const integer expression. It may be:

```text
a top-level const
a const parameter
an associated const projection
an arithmetic expression
```

Examples:

```text
[u8; 4096]
[u8; PAGE_SIZE]
[u8; N]
[u8; <T as Buffer>::SIZE]
[u8; PAGE_SIZE * 2]
```

### Reference type

```rust
Ty::Ref {
    mutability: Mutability::Mutable,
    ty: Box::new(Ty::SelfTy),
}
```

means:

```text
&mut Self
```

### Function type

```rust
Ty::Fn {
    params: vec![Ty::Int(i32_ty())],
    ret: Box::new(Ty::Bool),
}
```

means:

```text
fn(i32) -> bool
```

---

# 8. Generic arguments

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GenericArg {
    Ty(Ty),
    Const(ConstExpr),
}
```

## Meaning

A generic argument is either a type argument or a const argument.

Examples:

```text
Option<i32>
```

uses:

```rust
GenericArg::Ty(Ty::Int(i32_ty()))
```

while:

```text
Packet<128>
```

uses:

```rust
GenericArg::Const(ConstExpr::IntLit(128.into()))
```

and:

```text
Array<T, N>
```

uses:

```rust
GenericArg::Ty(Ty::Path {
    path: Path {
        segments: vec!["T".into()],
    },
    args: vec![],
})
```

and:

```rust
GenericArg::Const(ConstExpr::Param("N".into()))
```

---

# 9. Const integer expressions

```rust
use num_bigint::BigInt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConstExpr {
    IntLit(BigInt),

    /// Top-level const reference:
    ///
    /// PAGE_SIZE
    Path(Path),

    /// Generic const parameter:
    ///
    /// N
    Param(Symbol),

    /// Associated const projection:
    ///
    /// <T as Buffer>::SIZE
    AssocConst {
        ty: Box<Ty>,
        interface: Path,
        name: Symbol,
    },

    Unary {
        op: ConstUnaryOp,
        expr: Box<ConstExpr>,
    },

    Binary {
        op: ConstBinaryOp,
        lhs: Box<ConstExpr>,
        rhs: Box<ConstExpr>,
    },

    Cast {
        expr: Box<ConstExpr>,
        ty: IntTy,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstUnaryOp {
    Plus,
    Neg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    BitAnd,
    BitOr,
    BitXor,
}
```

## Meaning

`ConstExpr` represents compile-time integer expressions.

Examples:

```text
42
PAGE_SIZE
N
<T as Buffer>::SIZE
4 * 1024
PAGE_SIZE + 1
1 << 12
```

### Integer literal

```rust
ConstExpr::IntLit(4096.into())
```

means:

```text
4096
```

Literals are represented as arbitrary-precision integers so the compiler can evaluate first and range-check later.

### Path

```rust
ConstExpr::Path(Path {
    segments: vec!["PAGE_SIZE".into()],
})
```

means:

```text
PAGE_SIZE
```

This must resolve to a top-level const.

### Param

```rust
ConstExpr::Param("N".into())
```

means a generic const parameter:

```text
N
```

For example:

```text
fn f<const N: usize>(x: [u8; N]) {}
```

### Associated const

```rust
ConstExpr::AssocConst {
    ty: Box::new(Ty::Path {
        path: Path {
            segments: vec!["T".into()],
        },
        args: vec![],
    }),
    interface: Path {
        segments: vec!["Buffer".into()],
    },
    name: "SIZE".into(),
}
```

means:

```text
<T as Buffer>::SIZE
```

This is valid when the type checker can prove:

```text
T: Buffer
```

or when `T` is a concrete type with an implementation of `Buffer`.

### Arithmetic

```rust
ConstExpr::Binary {
    op: ConstBinaryOp::Mul,
    lhs: Box::new(ConstExpr::IntLit(4.into())),
    rhs: Box::new(ConstExpr::IntLit(1024.into())),
}
```

means:

```text
4 * 1024
```

### Cast

```rust
ConstExpr::Cast {
    expr: Box::new(ConstExpr::IntLit(255.into())),
    ty: IntTy {
        signedness: Signedness::Unsigned,
        width: IntWidth::W8,
    },
}
```

means:

```text
255 as u8
```

The cast is accepted only if the value fits the destination type.

---

# 10. Const items

```rust
#[derive(Clone, Debug)]
pub struct ConstItem {
    pub name: Symbol,
    pub ty: IntTy,
    pub expr: ConstExpr,
}
```

## Meaning

A `ConstItem` defines a top-level compile-time integer constant.

Example:

```text
const PAGE_SIZE: usize = 4 * 1024;
```

AST:

```rust
ConstItem {
    name: "PAGE_SIZE".into(),
    ty: IntTy {
        signedness: Signedness::Unsigned,
        width: IntWidth::Size,
    },
    expr: ConstExpr::Binary {
        op: ConstBinaryOp::Mul,
        lhs: Box::new(ConstExpr::IntLit(4.into())),
        rhs: Box::new(ConstExpr::IntLit(1024.into())),
    },
}
```

## Const item invariants

A valid const item must satisfy:

```text
1. The name is unique in its namespace.
2. The expression is const-evaluable.
3. The expression does not depend on a const cycle.
4. The resulting value fits the declared integer type.
```

Example:

```text
const GOOD: u8 = 255;
```

is valid.

```text
const BAD: u8 = 256;
```

is invalid because `256` does not fit in `u8`.

```text
const BAD: u32 = 1 / 0;
```

is invalid because division by zero occurs at compile time.

```text
const A: u32 = B + 1;
const B: u32 = A + 1;
```

is invalid because the constants form a cycle.

---

# 11. Generic parameters

```rust
#[derive(Clone, Debug)]
pub enum GenericParam {
    Type {
        name: Symbol,
        bounds: Vec<InterfaceRef>,
    },

    Const {
        name: Symbol,
        ty: IntTy,
    },
}
```

## Meaning

A generic parameter can be either a type parameter or a const integer parameter.

### Type parameter

```rust
GenericParam::Type {
    name: "T".into(),
    bounds: vec![],
}
```

means:

```text
T
```

With bounds:

```rust
GenericParam::Type {
    name: "T".into(),
    bounds: vec![
        InterfaceRef {
            path: Path {
                segments: vec!["Reader".into()],
            },
            args: vec![],
        },
    ],
}
```

means:

```text
T: Reader
```

### Const parameter

```rust
GenericParam::Const {
    name: "N".into(),
    ty: IntTy {
        signedness: Signedness::Unsigned,
        width: IntWidth::Size,
    },
}
```

means:

```text
const N: usize
```

Example:

```text
struct Array<T, const N: usize> {
    data: [T; N],
}
```

---

# 12. Interface references and where predicates

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InterfaceRef {
    pub path: Path,
    pub args: Vec<GenericArg>,
}

#[derive(Clone, Debug)]
pub enum WherePredicate {
    Implements {
        ty: Ty,
        interface: InterfaceRef,
    },

    ConstEq {
        lhs: ConstExpr,
        rhs: ConstExpr,
    },
}
```

## InterfaceRef meaning

An `InterfaceRef` names an interface, possibly with generic arguments.

Example:

```text
Reader
```

AST:

```rust
InterfaceRef {
    path: Path {
        segments: vec!["Reader".into()],
    },
    args: vec![],
}
```

Example:

```text
Iterator<Item = T>
```

could be represented in a future extension using associated type equality predicates. Since this AST only has associated consts, not associated types, the current `InterfaceRef` only has positional generic arguments.

## WherePredicate meaning

A `WherePredicate` expresses an obligation.

### Implements

```rust
WherePredicate::Implements {
    ty: Ty::Path {
        path: Path {
            segments: vec!["T".into()],
        },
        args: vec![],
    },
    interface: InterfaceRef {
        path: Path {
            segments: vec!["Reader".into()],
        },
        args: vec![],
    },
}
```

means:

```text
T: Reader
```

### ConstEq

```rust
WherePredicate::ConstEq {
    lhs: ConstExpr::Param("N".into()),
    rhs: ConstExpr::IntLit(4096.into()),
}
```

means:

```text
N == 4096
```

This can be used to constrain symbolic consts.

For a minimal language, `ConstEq` can be omitted until needed. It becomes useful when you want generic functions with constraints such as:

```text
where N == <T as Buffer>::SIZE
```

---

# 13. Struct items

```rust
#[derive(Clone, Debug)]
pub struct StructItem {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub where_predicates: Vec<WherePredicate>,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: Symbol,
    pub ty: Ty,
}
```

## Meaning

A `StructItem` defines a nominal product type.

Example:

```text
struct Point {
    x: i32,
    y: i32,
}
```

AST:

```rust
StructItem {
    name: "Point".into(),
    generics: vec![],
    where_predicates: vec![],
    fields: vec![
        Field {
            name: "x".into(),
            ty: Ty::Int(i32_ty()),
        },
        Field {
            name: "y".into(),
            ty: Ty::Int(i32_ty()),
        },
    ],
}
```

Generic example:

```text
struct Array<T, const N: usize> {
    data: [T; N],
}
```

Meaning:

```text
Array<T, N> is a nominal type with one field:
data: [T; N]
```

## Struct invariants

A valid struct must satisfy:

```text
1. The struct name is unique in the type namespace.
2. Generic parameter names are unique.
3. Field names are unique within the struct.
4. Every field type is well-formed.
5. Where predicates are well-formed.
```

---

# 14. Enum items

```rust
#[derive(Clone, Debug)]
pub struct EnumItem {
    pub name: Symbol,

    pub generics: Vec<GenericParam>,

    pub where_predicates: Vec<WherePredicate>,

    /// Optional integer representation for fieldless/C-like enums.
    pub repr: Option<IntTy>,

    pub variants: Vec<EnumVariant>,
}

#[derive(Clone, Debug)]
pub struct EnumVariant {
    pub name: Symbol,

    pub payload: VariantPayload,

    /// Optional integer discriminant.
    ///
    /// Usually only meaningful for unit variants.
    pub discriminant: Option<ConstExpr>,
}

#[derive(Clone, Debug)]
pub enum VariantPayload {
    /// Example:
    ///
    /// None
    /// Red
    Unit,

    /// Example:
    ///
    /// Some(T)
    /// Rgb(u8, u8, u8)
    Tuple(Vec<Ty>),

    /// Example:
    ///
    /// Move { x: i32, y: i32 }
    Struct(Vec<Field>),
}
```

## Meaning

An `EnumItem` defines a nominal sum type.

Example:

```text
enum Option<T> {
    None,
    Some(T),
}
```

AST:

```rust
EnumItem {
    name: "Option".into(),
    generics: vec![
        GenericParam::Type {
            name: "T".into(),
            bounds: vec![],
        },
    ],
    where_predicates: vec![],
    repr: None,
    variants: vec![
        EnumVariant {
            name: "None".into(),
            payload: VariantPayload::Unit,
            discriminant: None,
        },
        EnumVariant {
            name: "Some".into(),
            payload: VariantPayload::Tuple(vec![
                Ty::Path {
                    path: Path {
                        segments: vec!["T".into()],
                    },
                    args: vec![],
                },
            ]),
            discriminant: None,
        },
    ],
}
```

This defines:

```text
Option<T> = None | Some(T)
```

## Unit variants

```text
enum Color {
    Red,
    Green,
    Blue,
}
```

Each variant has no payload.

Conceptual runtime representation:

```text
Color = tag
```

## Tuple variants

```text
enum Option<T> {
    None,
    Some(T),
}
```

`Some(T)` has one unnamed payload field.

Conceptual runtime representation:

```text
tag = Some
payload = T
```

## Struct variants

```text
enum Message {
    Move { x: i32, y: i32 },
    Quit,
}
```

`Move` has named payload fields.

Conceptual runtime representation:

```text
tag = Move
payload = { x: i32, y: i32 }
```

## Repr and discriminants

A fieldless enum may specify an integer representation:

```text
enum Color: u8 {
    Red = 1,
    Green = 2,
    Blue = 3,
}
```

This uses:

```rust
repr: Some(u8_ty())
```

and each discriminant is a `ConstExpr`.

For a minimal language, use this rule:

```text
Only unit variants may have explicit discriminants.
```

So this is valid:

```text
enum Color: u8 {
    Red = 1,
    Green = 2,
}
```

but this is invalid:

```text
enum Message: u8 {
    Quit = 0,
    Write(String) = 1,
}
```

unless the language later adds Rust-like representation rules for payload enums.

## Enum invariants

A valid enum must satisfy:

```text
1. The enum name is unique in the type namespace.
2. Generic parameter names are unique.
3. Variant names are unique within the enum.
4. Every payload type is well-formed.
5. Struct variant field names are unique.
6. Every discriminant is const-evaluable.
7. Discriminants fit the chosen repr type.
8. Unit variant discriminants are unique.
9. Recursive enum definitions must be representable.
```

For sized-by-default languages, this should be rejected:

```text
enum Bad {
    Loop(Bad),
}
```

because it has infinite size.

This should be accepted:

```text
enum List<T> {
    Nil,
    Cons(T, Box<List<T>>),
}
```

if the language has a pointer-like indirection type such as `Box<T>`.

---

# 15. Interface items

```rust
#[derive(Clone, Debug)]
pub struct InterfaceItem {
    pub name: Symbol,

    pub generics: Vec<GenericParam>,

    /// Superinterfaces:
    ///
    /// interface Seekable: Reader { ... }
    pub super_interfaces: Vec<InterfaceRef>,

    pub members: Vec<InterfaceMember>,
}

#[derive(Clone, Debug)]
pub enum InterfaceMember {
    Method(MethodSig),
    AssocConst(AssocConstSig),
}

#[derive(Clone, Debug)]
pub struct AssocConstSig {
    pub name: Symbol,
    pub ty: IntTy,
    pub default: Option<ConstExpr>,
}

#[derive(Clone, Debug)]
pub struct MethodSig {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub receiver: Option<Receiver>,
    pub params: Vec<Param>,
    pub ret: Ty,
}

#[derive(Clone, Debug)]
pub enum Receiver {
    ByValue,

    ByRef {
        mutability: Mutability,
    },
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: Symbol,
    pub ty: Ty,
}
```

## Meaning

An `InterfaceItem` defines a compile-time contract.

Example:

```text
interface Reader {
    fn read(self: &mut Self, buf: [u8; PAGE_SIZE]) -> i32;
}
```

Meaning:

```text
For a type T to implement Reader, T must provide:

read(self: &mut T, buf: [u8; PAGE_SIZE]) -> i32
```

After const normalization:

```text
read(self: &mut T, buf: [u8; 4096]) -> i32
```

if:

```text
PAGE_SIZE = 4096
```

## Interface is not a type

This AST intentionally has no `Ty::DynInterface`.

Therefore:

```text
Reader
```

may appear in:

```text
T: Reader
impl Reader for File
interface Seekable: Reader
```

but may not appear as:

```text
let x: Reader
fn f(x: Reader)
struct S { r: Reader }
```

Those should be rejected.

## Associated consts

Example:

```text
interface Buffer {
    const SIZE: usize;

    fn read(self: &Self, buf: [u8; <Self as Buffer>::SIZE]) -> usize;
}
```

AST includes:

```rust
InterfaceMember::AssocConst(AssocConstSig {
    name: "SIZE".into(),
    ty: usize_ty(),
    default: None,
})
```

and the method uses:

```rust
ConstExpr::AssocConst {
    ty: Box::new(Ty::SelfTy),
    interface: Path {
        segments: vec!["Buffer".into()],
    },
    name: "SIZE".into(),
}
```

Meaning:

```text
Any implementor of Buffer must provide a compile-time integer constant SIZE.
```

If a default is present:

```text
interface Buffer {
    const SIZE: usize = 4096;
}
```

then implementors may omit it, and the default is used.

## Superinterfaces

Example:

```text
interface Seekable: Reader {
    fn seek(self: &mut Self, offset: i64) -> i64;
}
```

Meaning:

```text
T: Seekable implies T: Reader
```

The full requirement set of `Seekable` is:

```text
requirements(Seekable)
=
requirements(Reader)
+
own requirements of Seekable
```

## Interface invariants

A valid interface must satisfy:

```text
1. The interface name is unique in the interface namespace.
2. Generic parameter names are unique.
3. Every superinterface exists.
4. The superinterface graph is acyclic.
5. Member names are unique, unless duplicate inherited methods have identical normalized signatures.
6. Associated const defaults are const-well-formed.
7. Method parameter types are well-formed.
8. Method return types are well-formed.
9. Self is used only in valid interface positions.
```

Because interfaces are monomorphized and never turned into runtime objects, methods may use `Self` freely:

```text
interface Eq {
    fn eq(self: &Self, other: Self) -> bool;
}
```

This is allowed.

It would be problematic for dynamic dispatch, but dynamic dispatch does not exist in this design.

---

# 16. Impl items

```rust
#[derive(Clone, Debug)]
pub struct ImplItem {
    pub generics: Vec<GenericParam>,

    pub where_predicates: Vec<WherePredicate>,

    /// `Some(...)` for interface impls.
    ///
    /// `None` can represent inherent impls.
    pub interface: Option<InterfaceRef>,

    pub self_ty: Ty,

    pub members: Vec<ImplMember>,
}

#[derive(Clone, Debug)]
pub enum ImplMember {
    Method(MethodDef),
    AssocConst(AssocConstImpl),
}

#[derive(Clone, Debug)]
pub struct AssocConstImpl {
    pub name: Symbol,
    pub ty: IntTy,
    pub expr: ConstExpr,
}

#[derive(Clone, Debug)]
pub struct MethodDef {
    pub sig: MethodSig,
    pub body: Expr,
}
```

## Meaning

An `ImplItem` provides methods and associated consts for a type.

Interface impl example:

```text
impl Reader for File {
    fn read(self: &mut Self, buf: [u8; PAGE_SIZE]) -> i32 {
        ...
    }
}
```

Meaning:

```text
File satisfies Reader.
```

Generic impl example:

```text
impl<T> Display for Option<T>
where
    T: Display
{
    fn display(self: &Self) -> String {
        ...
    }
}
```

Meaning:

```text
For every T, if T: Display, then Option<T>: Display.
```

This is monomorphizable.

At a concrete use site:

```text
Option<i32>: Display
```

the compiler must prove:

```text
i32: Display
```

then it can instantiate the impl.

## Associated const impl

Example:

```text
impl Buffer for Page {
    const SIZE: usize = 4096;

    fn read(self: &Self, buf: [u8; 4096]) -> usize {
        ...
    }
}
```

AST includes:

```rust
ImplMember::AssocConst(AssocConstImpl {
    name: "SIZE".into(),
    ty: usize_ty(),
    expr: ConstExpr::IntLit(4096.into()),
})
```

Meaning:

```text
<Page as Buffer>::SIZE = 4096
```

## Impl invariants

A valid interface impl must satisfy:

```text
1. The interface exists.
2. The self type is well-formed.
3. All impl generic parameters are unique.
4. All where predicates are well-formed.
5. Every required associated const is provided or has a default.
6. Every required method is provided.
7. Provided method signatures match required signatures after substituting Self.
8. Const expressions in method signatures normalize consistently.
9. The impl does not conflict with another impl.
```

Conflict rule example:

```text
impl Reader for File { ... }
impl Reader for File { ... }
```

should be rejected.

For generic impls, overlap checking is harder:

```text
impl<T> Reader for T where T: FileLike { ... }
impl Reader for File { ... }
```

A minimal language can reject overlapping impls conservatively.

---

# 17. Function items

```rust
#[derive(Clone, Debug)]
pub struct FnItem {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub where_predicates: Vec<WherePredicate>,
    pub params: Vec<Param>,
    pub ret: Ty,
    pub body: Expr,
}
```

## Meaning

A `FnItem` defines a top-level function.

Example:

```text
fn consume<T: Reader>(r: &mut T, buf: [u8; PAGE_SIZE]) -> i32 {
    r.read(buf)
}
```

AST:

```rust
FnItem {
    name: "consume".into(),
    generics: vec![
        GenericParam::Type {
            name: "T".into(),
            bounds: vec![
                InterfaceRef {
                    path: Path {
                        segments: vec!["Reader".into()],
                    },
                    args: vec![],
                },
            ],
        },
    ],
    where_predicates: vec![],
    params: vec![
        Param {
            name: "r".into(),
            ty: Ty::Ref {
                mutability: Mutability::Mutable,
                ty: Box::new(Ty::Path {
                    path: Path {
                        segments: vec!["T".into()],
                    },
                    args: vec![],
                }),
            },
        },
        Param {
            name: "buf".into(),
            ty: Ty::Array {
                elem: Box::new(Ty::Int(u8_ty())),
                len: ConstExpr::Path(Path {
                    segments: vec!["PAGE_SIZE".into()],
                }),
            },
        },
    ],
    ret: Ty::Int(i32_ty()),
    body: Expr::MethodCall {
        receiver: Box::new(Expr::Var("r".into())),
        method: "read".into(),
        args: vec![Expr::Var("buf".into())],
    },
}
```

Meaning:

```text
For every concrete T such that T: Reader,
consume<T> can be instantiated.
```

At monomorphization time:

```text
consume::<File>
```

becomes:

```text
consume__File
```

with the method call statically resolved to:

```text
<File as Reader>::read
```

---

# 18. Expressions

```rust
#[derive(Clone, Debug)]
pub enum Expr {
    UnitLit,
    BoolLit(bool),
    IntLit(BigInt),

    Var(Symbol),

    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },

    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },

    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },

    MethodCall {
        receiver: Box<Expr>,
        method: Symbol,
        args: Vec<Expr>,
    },

    StructLit {
        path: Path,
        fields: Vec<FieldExpr>,
    },

    EnumCtor {
        enum_path: Path,
        variant: Symbol,
        args: EnumCtorArgs,
    },

    Field {
        base: Box<Expr>,
        name: Symbol,
    },

    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },

    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },

    If {
        cond: Box<Expr>,
        then_branch: Block,
        else_branch: Option<Block>,
    },

    Block(Block),

    Return(Option<Box<Expr>>),

    Assign {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },

    Todo,
}

#[derive(Clone, Copy, Debug)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,

    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    And,
    Or,

    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let(LetStmt),
    Expr(Expr),
    Semi(Expr),
}

#[derive(Clone, Debug)]
pub struct LetStmt {
    pub name: Symbol,
    pub ty: Option<Ty>,
    pub init: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct FieldExpr {
    pub name: Symbol,
    pub expr: Expr,
}

#[derive(Clone, Debug)]
pub enum EnumCtorArgs {
    Unit,
    Tuple(Vec<Expr>),
    Struct(Vec<FieldExpr>),
}
```

## Meaning

### Literals

```rust
Expr::IntLit(123.into())
```

means:

```text
123
```

### Variables

```rust
Expr::Var("x".into())
```

means:

```text
x
```

The variable is resolved in the local environment.

### Calls

```rust
Expr::Call {
    callee: Box::new(Expr::Var("f".into())),
    args: vec![Expr::IntLit(1.into())],
}
```

means:

```text
f(1)
```

### Method calls

```rust
Expr::MethodCall {
    receiver: Box::new(Expr::Var("r".into())),
    method: "read".into(),
    args: vec![Expr::Var("buf".into())],
}
```

means:

```text
r.read(buf)
```

For a generic receiver:

```text
r: &mut T
T: Reader
```

the method call is resolved through the interface obligation.

For a concrete receiver:

```text
file: File
```

the method call is resolved through inherent methods or interface impls.

No vtable is created. After monomorphization, every method call is a direct call.

### Struct literal

```text
Point { x: 1, y: 2 }
```

AST:

```rust
Expr::StructLit {
    path: Path {
        segments: vec!["Point".into()],
    },
    fields: vec![
        FieldExpr {
            name: "x".into(),
            expr: Expr::IntLit(1.into()),
        },
        FieldExpr {
            name: "y".into(),
            expr: Expr::IntLit(2.into()),
        },
    ],
}
```

### Enum constructor

```text
Option::Some(123)
```

AST:

```rust
Expr::EnumCtor {
    enum_path: Path {
        segments: vec!["Option".into()],
    },
    variant: "Some".into(),
    args: EnumCtorArgs::Tuple(vec![
        Expr::IntLit(123.into()),
    ]),
}
```

### Match

```text
match x {
    Option::None => 0,
    Option::Some(n) => n,
}
```

AST:

```rust
Expr::Match {
    scrutinee: Box::new(Expr::Var("x".into())),
    arms: vec![
        MatchArm {
            pat: Pat::EnumVariant {
                enum_path: Path {
                    segments: vec!["Option".into()],
                },
                variant: "None".into(),
                args: EnumPatArgs::Unit,
            },
            body: Expr::IntLit(0.into()),
        },
        MatchArm {
            pat: Pat::EnumVariant {
                enum_path: Path {
                    segments: vec!["Option".into()],
                },
                variant: "Some".into(),
                args: EnumPatArgs::Tuple(vec![
                    Pat::Binding {
                        name: "n".into(),
                    },
                ]),
            },
            body: Expr::Var("n".into()),
        },
    ],
}
```

---

# 19. Patterns

```rust
#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pat: Pat,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub enum Pat {
    Wildcard,

    Binding {
        name: Symbol,
    },

    Unit,

    BoolLit(bool),

    IntLit(BigInt),

    EnumVariant {
        enum_path: Path,
        variant: Symbol,
        args: EnumPatArgs,
    },

    Struct {
        path: Path,
        fields: Vec<FieldPat>,
    },
}

#[derive(Clone, Debug)]
pub enum EnumPatArgs {
    Unit,
    Tuple(Vec<Pat>),
    Struct(Vec<FieldPat>),
}

#[derive(Clone, Debug)]
pub struct FieldPat {
    pub name: Symbol,
    pub pat: Pat,
}
```

## Meaning

Patterns destructure values.

### Wildcard

```text
_
```

AST:

```rust
Pat::Wildcard
```

Matches anything and binds nothing.

### Binding

```text
x
```

AST:

```rust
Pat::Binding {
    name: "x".into(),
}
```

Matches anything and binds the value to `x`.

### Enum variant pattern

```text
Option::Some(n)
```

AST:

```rust
Pat::EnumVariant {
    enum_path: Path {
        segments: vec!["Option".into()],
    },
    variant: "Some".into(),
    args: EnumPatArgs::Tuple(vec![
        Pat::Binding {
            name: "n".into(),
        },
    ]),
}
```

If the scrutinee has type:

```text
Option<i32>
```

then this binds:

```text
n: i32
```

### Struct variant pattern

```text
Message::Move { x, y }
```

could be represented as:

```rust
Pat::EnumVariant {
    enum_path: Path {
        segments: vec!["Message".into()],
    },
    variant: "Move".into(),
    args: EnumPatArgs::Struct(vec![
        FieldPat {
            name: "x".into(),
            pat: Pat::Binding {
                name: "x".into(),
            },
        },
        FieldPat {
            name: "y".into(),
            pat: Pat::Binding {
                name: "y".into(),
            },
        },
    ]),
}
```

---

# 20. Type-checking meaning

This AST is checked using several environments:

```text
Σ   type environment
C   top-level const environment
I   interface environment
E   impl environment
Γ   local variable environment
Ω   current generic obligations
```

Their roles:

```text
Σ   knows structs, enums, type params
C   knows evaluated top-level constants
I   knows interface requirement sets
E   knows impls
Γ   knows local variables
Ω   knows assumptions like T: Reader
```

Example:

```text
const PAGE_SIZE: usize = 4096;

interface Reader {
    fn read(self: &mut Self, buf: [u8; PAGE_SIZE]) -> i32;
}

fn consume<T: Reader>(r: &mut T, buf: [u8; PAGE_SIZE]) -> i32 {
    r.read(buf)
}
```

During type checking:

```text
C(PAGE_SIZE) = 4096

I(Reader) = {
    read: (&mut Self, [u8; 4096]) -> i32
}

Ω = {
    T: Reader
}

Γ = {
    r: &mut T,
    buf: [u8; 4096]
}
```

The method call:

```text
r.read(buf)
```

is valid because:

```text
T: Reader
```

and `Reader` requires:

```text
read: (&mut Self, [u8; 4096]) -> i32
```

Substituting:

```text
Self := T
```

gives:

```text
read: (&mut T, [u8; 4096]) -> i32
```

---

# 21. Const expression checking

Top-level consts are fully evaluated at compile time.

Judgment:

```text
C ⊢ e ⇓ n
```

means:

```text
Under const environment C, const expression e evaluates to integer n.
```

Examples:

```text
C ⊢ 4096 ⇓ 4096
C(PAGE_SIZE) = 4096
C ⊢ PAGE_SIZE ⇓ 4096
C ⊢ 4 * 1024 ⇓ 4096
```

A const declaration:

```text
const PAGE_SIZE: usize = 4 * 1024;
```

is valid if:

```text
1. 4 * 1024 evaluates to 4096.
2. 4096 fits in usize.
3. PAGE_SIZE does not create a const cycle.
```

Generic const expressions may remain symbolic during generic checking.

Example:

```text
fn f<const N: usize>(x: [u8; N]) {}
```

Here:

```text
N
```

does not evaluate to a concrete integer until monomorphization.

Associated consts may also remain symbolic:

```text
<T as Buffer>::SIZE
```

is valid if the current obligations imply:

```text
T: Buffer
```

At monomorphization, if:

```text
T = Page
```

then:

```text
<T as Buffer>::SIZE
```

becomes:

```text
<Page as Buffer>::SIZE
```

and resolves to the value provided by the impl.

---

# 22. Type well-formedness

Judgment:

```text
Σ; C; I; Ω ⊢ ty wf
```

means:

```text
The type is well-formed.
```

Examples:

```text
i32
bool
Option<i32>
[T; N]
[u8; PAGE_SIZE]
[u8; <T as Buffer>::SIZE]
&mut T
```

A path type is well-formed if it resolves to a concrete type constructor or type parameter:

```text
Σ contains Option
———————————————
Option<i32> wf
```

An interface path is **not** well-formed as a value type:

```text
Reader ∈ I
———————————————
Reader is rejected as a type
```

Array type rule:

```text
elem wf
len const-wf
———————————————
[elem; len] wf
```

If `len` is concrete, it must evaluate to a nonnegative integer.

If `len` is symbolic, it must be valid under the current generic obligations.

---

# 23. Interface checking

An interface declaration:

```text
interface I: Parent1 + Parent2 {
    const K: usize;
    fn m(self: &Self, x: T) -> U;
}
```

is checked by:

```text
1. Ensuring I is fresh.
2. Checking all generic parameters.
3. Checking all parent interfaces.
4. Rejecting inheritance cycles.
5. Checking associated const signatures.
6. Checking method signatures.
7. Computing the full inherited requirement set.
8. Rejecting conflicting inherited methods.
```

The interface elaborates to a requirement set.

Example:

```text
interface Reader {
    fn read(self: &mut Self, buf: [u8; 4096]) -> i32;
}

interface Seekable: Reader {
    fn seek(self: &mut Self, offset: i64) -> i64;
}
```

Requirement set:

```text
Reader(Self) requires:
    read(&mut Self, [u8; 4096]) -> i32

Seekable(Self) requires:
    read(&mut Self, [u8; 4096]) -> i32
    seek(&mut Self, i64) -> i64
```

---

# 24. Impl checking

An impl:

```text
impl Reader for File {
    fn read(self: &mut Self, buf: [u8; 4096]) -> i32 {
        ...
    }
}
```

is checked by:

```text
1. Resolve Reader.
2. Resolve File.
3. Load Reader requirements.
4. Substitute Self := File.
5. Check that each required method is present.
6. Check that each required associated const is present or has a default.
7. Normalize const expressions in signatures.
8. Compare signatures.
9. Type-check method bodies.
```

If the interface requires:

```text
fn read(self: &mut Self, buf: [u8; PAGE_SIZE]) -> i32;
```

and the impl provides:

```text
fn read(self: &mut Self, buf: [u8; 4096]) -> i32;
```

the impl is valid if:

```text
PAGE_SIZE ⇓ 4096
```

---

# 25. Enum checking

An enum:

```text
enum Option<T> {
    None,
    Some(T),
}
```

is checked by:

```text
1. Ensuring Option is fresh.
2. Checking generic parameters.
3. Ensuring variant names are unique.
4. Checking payload types.
5. Checking discriminants.
6. Computing layout information.
```

A constructor expression:

```text
Option::Some(123)
```

is valid when:

```text
Option<T>::Some expects T
123: i32
T := i32
```

so:

```text
Option::Some(123): Option<i32>
```

A match expression:

```text
match x {
    Option::None => 0,
    Option::Some(n) => n,
}
```

is valid when:

```text
x: Option<i32>
Option::None matches Option<i32>
Option::Some(n) binds n: i32
both arms return i32
the match is exhaustive
```

Therefore:

```text
match x { ... } : i32
```

---

# 26. Monomorphization semantics

Generic functions are compiled into concrete specialized functions.

Source:

```text
fn consume<T: Reader>(r: &mut T, buf: [u8; PAGE_SIZE]) -> i32 {
    r.read(buf)
}
```

After const normalization:

```text
fn consume<T: Reader>(r: &mut T, buf: [u8; 4096]) -> i32 {
    r.read(buf)
}
```

At call site:

```text
consume::<File>(&mut file, buf)
```

the compiler generates:

```text
fn consume__File(r: &mut File, buf: [u8; 4096]) -> i32 {
    <File as Reader>::read(r, buf)
}
```

The method call:

```text
r.read(buf)
```

becomes a direct call:

```text
<File as Reader>::read(r, buf)
```

There is:

```text
no dyn object
no vtable
no existential package
no runtime interface dispatch
```

---

# 27. Associated const monomorphization

Source:

```text
interface Buffer {
    const SIZE: usize;

    fn read(self: &Self, buf: [u8; <Self as Buffer>::SIZE]) -> usize;
}

struct Page {}

impl Buffer for Page {
    const SIZE: usize = 4096;

    fn read(self: &Self, buf: [u8; 4096]) -> usize {
        ...
    }
}

fn use_buffer<T: Buffer>(
    x: &T,
    buf: [u8; <T as Buffer>::SIZE]
) -> usize {
    x.read(buf)
}
```

During generic checking, this remains symbolic:

```text
<T as Buffer>::SIZE
```

At monomorphization:

```text
use_buffer::<Page>
```

becomes:

```text
fn use_buffer__Page(
    x: &Page,
    buf: [u8; 4096]
) -> usize {
    <Page as Buffer>::read(x, buf)
}
```

because:

```text
<Page as Buffer>::SIZE = 4096
```

---

# 28. Complete AST listing

For convenience, here is the full AST together.

```rust
use num_bigint::BigInt;

pub type Symbol = String;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Path {
    pub segments: Vec<Symbol>,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Const(ConstItem),
    Struct(StructItem),
    Enum(EnumItem),
    Interface(InterfaceItem),
    Impl(ImplItem),
    Fn(FnItem),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Signedness {
    Signed,
    Unsigned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IntWidth {
    W8,
    W16,
    W32,
    W64,
    W128,
    Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntTy {
    pub signedness: Signedness,
    pub width: IntWidth,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Unit,
    Bool,
    Never,

    Int(IntTy),

    Path {
        path: Path,
        args: Vec<GenericArg>,
    },

    SelfTy,

    Array {
        elem: Box<Ty>,
        len: ConstExpr,
    },

    Ref {
        mutability: Mutability,
        ty: Box<Ty>,
    },

    Fn {
        params: Vec<Ty>,
        ret: Box<Ty>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mutability {
    Shared,
    Mutable,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GenericArg {
    Ty(Ty),
    Const(ConstExpr),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConstExpr {
    IntLit(BigInt),

    Path(Path),

    Param(Symbol),

    AssocConst {
        ty: Box<Ty>,
        interface: Path,
        name: Symbol,
    },

    Unary {
        op: ConstUnaryOp,
        expr: Box<ConstExpr>,
    },

    Binary {
        op: ConstBinaryOp,
        lhs: Box<ConstExpr>,
        rhs: Box<ConstExpr>,
    },

    Cast {
        expr: Box<ConstExpr>,
        ty: IntTy,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstUnaryOp {
    Plus,
    Neg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    BitAnd,
    BitOr,
    BitXor,
}

#[derive(Clone, Debug)]
pub struct ConstItem {
    pub name: Symbol,
    pub ty: IntTy,
    pub expr: ConstExpr,
}

#[derive(Clone, Debug)]
pub enum GenericParam {
    Type {
        name: Symbol,
        bounds: Vec<InterfaceRef>,
    },

    Const {
        name: Symbol,
        ty: IntTy,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InterfaceRef {
    pub path: Path,
    pub args: Vec<GenericArg>,
}

#[derive(Clone, Debug)]
pub enum WherePredicate {
    Implements {
        ty: Ty,
        interface: InterfaceRef,
    },

    ConstEq {
        lhs: ConstExpr,
        rhs: ConstExpr,
    },
}

#[derive(Clone, Debug)]
pub struct StructItem {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub where_predicates: Vec<WherePredicate>,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: Symbol,
    pub ty: Ty,
}

#[derive(Clone, Debug)]
pub struct EnumItem {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub where_predicates: Vec<WherePredicate>,
    pub repr: Option<IntTy>,
    pub variants: Vec<EnumVariant>,
}

#[derive(Clone, Debug)]
pub struct EnumVariant {
    pub name: Symbol,
    pub payload: VariantPayload,
    pub discriminant: Option<ConstExpr>,
}

#[derive(Clone, Debug)]
pub enum VariantPayload {
    Unit,
    Tuple(Vec<Ty>),
    Struct(Vec<Field>),
}

#[derive(Clone, Debug)]
pub struct InterfaceItem {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub super_interfaces: Vec<InterfaceRef>,
    pub members: Vec<InterfaceMember>,
}

#[derive(Clone, Debug)]
pub enum InterfaceMember {
    Method(MethodSig),
    AssocConst(AssocConstSig),
}

#[derive(Clone, Debug)]
pub struct AssocConstSig {
    pub name: Symbol,
    pub ty: IntTy,
    pub default: Option<ConstExpr>,
}

#[derive(Clone, Debug)]
pub struct MethodSig {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub receiver: Option<Receiver>,
    pub params: Vec<Param>,
    pub ret: Ty,
}

#[derive(Clone, Debug)]
pub enum Receiver {
    ByValue,
    ByRef {
        mutability: Mutability,
    },
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: Symbol,
    pub ty: Ty,
}

#[derive(Clone, Debug)]
pub struct ImplItem {
    pub generics: Vec<GenericParam>,
    pub where_predicates: Vec<WherePredicate>,
    pub interface: Option<InterfaceRef>,
    pub self_ty: Ty,
    pub members: Vec<ImplMember>,
}

#[derive(Clone, Debug)]
pub enum ImplMember {
    Method(MethodDef),
    AssocConst(AssocConstImpl),
}

#[derive(Clone, Debug)]
pub struct AssocConstImpl {
    pub name: Symbol,
    pub ty: IntTy,
    pub expr: ConstExpr,
}

#[derive(Clone, Debug)]
pub struct MethodDef {
    pub sig: MethodSig,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub struct FnItem {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub where_predicates: Vec<WherePredicate>,
    pub params: Vec<Param>,
    pub ret: Ty,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub enum Expr {
    UnitLit,
    BoolLit(bool),
    IntLit(BigInt),

    Var(Symbol),

    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },

    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },

    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },

    MethodCall {
        receiver: Box<Expr>,
        method: Symbol,
        args: Vec<Expr>,
    },

    StructLit {
        path: Path,
        fields: Vec<FieldExpr>,
    },

    EnumCtor {
        enum_path: Path,
        variant: Symbol,
        args: EnumCtorArgs,
    },

    Field {
        base: Box<Expr>,
        name: Symbol,
    },

    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },

    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },

    If {
        cond: Box<Expr>,
        then_branch: Block,
        else_branch: Option<Block>,
    },

    Block(Block),

    Return(Option<Box<Expr>>),

    Assign {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },

    Todo,
}

#[derive(Clone, Copy, Debug)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,

    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    And,
    Or,

    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let(LetStmt),
    Expr(Expr),
    Semi(Expr),
}

#[derive(Clone, Debug)]
pub struct LetStmt {
    pub name: Symbol,
    pub ty: Option<Ty>,
    pub init: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct FieldExpr {
    pub name: Symbol,
    pub expr: Expr,
}

#[derive(Clone, Debug)]
pub enum EnumCtorArgs {
    Unit,
    Tuple(Vec<Expr>),
    Struct(Vec<FieldExpr>),
}

#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pat: Pat,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub enum Pat {
    Wildcard,

    Binding {
        name: Symbol,
    },

    Unit,

    BoolLit(bool),

    IntLit(BigInt),

    EnumVariant {
        enum_path: Path,
        variant: Symbol,
        args: EnumPatArgs,
    },

    Struct {
        path: Path,
        fields: Vec<FieldPat>,
    },
}

#[derive(Clone, Debug)]
pub enum EnumPatArgs {
    Unit,
    Tuple(Vec<Pat>),
    Struct(Vec<FieldPat>),
}

#[derive(Clone, Debug)]
pub struct FieldPat {
    pub name: Symbol,
    pub pat: Pat,
}
```

---

# 29. Core invariant summary

The central invariants of this AST are:

```text
1. Structs and enums are concrete nominal types.

2. Interfaces are compile-time predicates over types.

3. Interfaces are not value types.

4. There is no dynamic interface object in Ty.

5. Impl items provide compile-time evidence for interface predicates.

6. Generic functions with interface bounds are monomorphized.

7. Const expressions can appear in:
   - top-level consts
   - array lengths
   - const generic arguments
   - enum discriminants
   - associated const defaults
   - associated const impls

8. Top-level const expressions are evaluated before type normalization.

9. Generic const expressions may remain symbolic until monomorphization.

10. Enum matches are checked for type consistency and exhaustiveness.

11. Interface method calls are resolved statically after monomorphization.

12. No runtime vtable or existential package is generated.
```

The resulting language model is:

```text
structs        = nominal product types
enums          = nominal sum types
interfaces     = compile-time type predicates
impls          = evidence for predicates
consts         = compile-time integer values
generics       = templates over types and const integers
monomorphizer  = turns generic code into concrete code
```
