use crate::span::{Node, Span};
use num_bigint::BigInt;
use std::fmt;

pub type Symbol = String;

pub type Path = Node<PathKind>;
pub type Program = Node<ProgramKind>;
pub type Item = Node<ItemKind>;
pub type Ty = Node<TyKind>;
pub type GenericArg = Node<GenericArgKind>;
pub type ConstExpr = Node<ConstExprKind>;
pub type GenericParam = Node<GenericParamKind>;
pub type InterfaceRef = Node<InterfaceRefKind>;
pub type WherePredicate = Node<WherePredicateKind>;
pub type StructItem = Node<StructItemKind>;
pub type Field = Node<FieldKind>;
pub type EnumItem = Node<EnumItemKind>;
pub type EnumVariant = Node<EnumVariantKind>;
pub type VariantPayload = Node<VariantPayloadKind>;
pub type InterfaceItem = Node<InterfaceItemKind>;
pub type InterfaceMember = Node<InterfaceMemberKind>;
pub type AssocConstSig = Node<AssocConstSigKind>;
pub type MethodSig = Node<MethodSigKind>;
pub type Receiver = Node<ReceiverKind>;
pub type Param = Node<ParamKind>;
pub type ImplItem = Node<ImplItemKind>;
pub type ImplMember = Node<ImplMemberKind>;
pub type AssocConstImpl = Node<AssocConstImplKind>;
pub type MethodDef = Node<MethodDefKind>;
pub type FnItem = Node<FnItemKind>;
pub type Expr = Node<ExprKind>;
pub type Block = Node<BlockKind>;
pub type Stmt = Node<StmtKind>;
pub type LetStmt = Node<LetStmtKind>;
pub type FieldExpr = Node<FieldExprKind>;
pub type EnumCtorArgs = Node<EnumCtorArgsKind>;
pub type MatchArm = Node<MatchArmKind>;
pub type Pat = Node<PatKind>;
pub type EnumPatArgs = Node<EnumPatArgsKind>;
pub type FieldPat = Node<FieldPatKind>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PathKind {
    pub segments: Vec<Symbol>,
}

impl PathKind {
    pub fn single(name: impl Into<Symbol>) -> Self {
        Self {
            segments: vec![name.into()],
        }
    }
}

pub fn path_name(path: &Path) -> String {
    path.kind.segments.join("::")
}

pub fn synthetic_path(name: impl Into<Symbol>) -> Path {
    Node::new(Span::default(), PathKind::single(name))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramKind {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemKind {
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

impl IntTy {
    pub const fn new(signedness: Signedness, width: IntWidth) -> Self {
        Self { signedness, width }
    }

    pub const fn i32() -> Self {
        Self::new(Signedness::Signed, IntWidth::W32)
    }

    pub const fn i64() -> Self {
        Self::new(Signedness::Signed, IntWidth::W64)
    }

    pub const fn u8() -> Self {
        Self::new(Signedness::Unsigned, IntWidth::W8)
    }

    pub const fn usize() -> Self {
        Self::new(Signedness::Unsigned, IntWidth::Size)
    }

    pub fn bits(self) -> u32 {
        match self.width {
            IntWidth::W8 => 8,
            IntWidth::W16 => 16,
            IntWidth::W32 => 32,
            IntWidth::W64 => 64,
            IntWidth::W128 => 128,
            IntWidth::Size => 64,
        }
    }
}

impl fmt::Display for IntTy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.signedness {
            Signedness::Signed => "i",
            Signedness::Unsigned => "u",
        };
        match self.width {
            IntWidth::Size => write!(f, "{}size", prefix),
            _ => write!(f, "{}{}", prefix, self.bits()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TyKind {
    Unit,
    Bool,
    Never,
    Int(IntTy),
    Path { path: Path, args: Vec<GenericArg> },
    SelfTy,
    Array { elem: Box<Ty>, len: ConstExpr },
    Ref { mutability: Mutability, ty: Box<Ty> },
    Fn { params: Vec<Ty>, ret: Box<Ty> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mutability {
    Shared,
    Mutable,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GenericArgKind {
    Ty(Ty),
    Const(ConstExpr),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConstExprKind {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstItemKind {
    pub name: Symbol,
    pub ty: IntTy,
    pub expr: ConstExpr,
}

pub type ConstItem = Node<ConstItemKind>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenericParamKind {
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
pub struct InterfaceRefKind {
    pub path: Path,
    pub args: Vec<GenericArg>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WherePredicateKind {
    Implements { ty: Ty, interface: InterfaceRef },
    ConstEq { lhs: ConstExpr, rhs: ConstExpr },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructItemKind {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub where_predicates: Vec<WherePredicate>,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldKind {
    pub name: Symbol,
    pub ty: Ty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumItemKind {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub where_predicates: Vec<WherePredicate>,
    pub repr: Option<IntTy>,
    pub variants: Vec<EnumVariant>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumVariantKind {
    pub name: Symbol,
    pub payload: VariantPayload,
    pub discriminant: Option<ConstExpr>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VariantPayloadKind {
    Unit,
    Tuple(Vec<Ty>),
    Struct(Vec<Field>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceItemKind {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub super_interfaces: Vec<InterfaceRef>,
    pub members: Vec<InterfaceMember>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InterfaceMemberKind {
    Method(MethodSig),
    AssocConst(AssocConstSig),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssocConstSigKind {
    pub name: Symbol,
    pub ty: IntTy,
    pub default: Option<ConstExpr>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodSigKind {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub receiver: Option<Receiver>,
    pub params: Vec<Param>,
    pub ret: Ty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceiverKind {
    ByValue,
    ByRef { mutability: Mutability },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamKind {
    pub name: Symbol,
    pub ty: Ty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImplItemKind {
    pub generics: Vec<GenericParam>,
    pub where_predicates: Vec<WherePredicate>,
    pub interface: Option<InterfaceRef>,
    pub self_ty: Ty,
    pub members: Vec<ImplMember>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImplMemberKind {
    Method(MethodDef),
    AssocConst(AssocConstImpl),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssocConstImplKind {
    pub name: Symbol,
    pub ty: IntTy,
    pub expr: ConstExpr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodDefKind {
    pub sig: MethodSig,
    pub body: Expr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnItemKind {
    pub name: Symbol,
    pub generics: Vec<GenericParam>,
    pub where_predicates: Vec<WherePredicate>,
    pub params: Vec<Param>,
    pub ret: Ty,
    pub body: Expr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExprKind {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockKind {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StmtKind {
    Let(LetStmt),
    Expr(Expr),
    Semi(Expr),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LetStmtKind {
    pub name: Symbol,
    pub ty: Option<Ty>,
    pub init: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldExprKind {
    pub name: Symbol,
    pub expr: Expr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnumCtorArgsKind {
    Unit,
    Tuple(Vec<Expr>),
    Struct(Vec<FieldExpr>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchArmKind {
    pub pat: Pat,
    pub body: Expr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatKind {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnumPatArgsKind {
    Unit,
    Tuple(Vec<Pat>),
    Struct(Vec<FieldPat>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldPatKind {
    pub name: Symbol,
    pub pat: Pat,
}

pub fn ty_unit(span: Span) -> Ty {
    Node::new(span, TyKind::Unit)
}

pub fn ty_bool(span: Span) -> Ty {
    Node::new(span, TyKind::Bool)
}

pub fn ty_int(span: Span, ty: IntTy) -> Ty {
    Node::new(span, TyKind::Int(ty))
}

pub fn ty_path(span: Span, name: impl Into<Symbol>) -> Ty {
    Node::new(
        span,
        TyKind::Path {
            path: Node::new(span, PathKind::single(name)),
            args: vec![],
        },
    )
}

pub fn ty_ref(span: Span, mutability: Mutability, ty: Ty) -> Ty {
    Node::new(
        span,
        TyKind::Ref {
            mutability,
            ty: Box::new(ty),
        },
    )
}

pub fn const_int(span: Span, value: impl Into<BigInt>) -> ConstExpr {
    Node::new(span, ConstExprKind::IntLit(value.into()))
}
