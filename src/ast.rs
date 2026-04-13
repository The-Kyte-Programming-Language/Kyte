#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    // 앵커
    At,   // @
    Hash, // #

    // 키워드
    Main,     // main
    Function, // function
    Vault,    // Vault
    Kill,     // Kill
    Exit,     // Exit
    Yield,    // yield
    Return,   // return
    If,       // if
    Else,     // else
    Loop,     // loop
    For,      // for
    In,       // in
    Break,    // break
    True,     // true
    False,    // false
    Free,     // free
    Print,    // print
    While,    // while
    As,       // as
    Struct,   // struct
    Auto,     // auto (A07: 타입 추론)
    Assert,   // assert (A10)
    Enum,     // enum
    Match,    // match
    FatArrow, // =>
    Trait,    // trait
    Impl,     // impl
    Mod,      // mod
    Const,    // const
    Fn,       // fn (closure)
    Pipe,     // |
    Import,   // import

    // 타입
    Int,    // int
    Float,  // float
    String, // string
    Bool,   // bool
    TyI8,   // i8
    TyI16,  // i16
    TyI32,  // i32
    TyI64,  // i64
    TyU8,   // u8
    TyU16,  // u16
    TyU32,  // u32
    TyU64,  // u64

    // 리터럴
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    FStringLit(String), // f"hello {name}" — raw content for parser to expand

    // 식별자
    Ident(String),

    // 심볼
    LParen,    // (
    RParen,    // )
    LBrace,    // {
    RBrace,    // }
    LBracket,  // [
    RBracket,  // ]
    Semicolon, // ;
    Comma,     // ,
    Colon,     // :
    Dot,       // .
    Eq,        // =
    EqEq,      // ==
    Neq,       // !=
    Le,        // <=
    Ge,        // >=
    Arrow,     // ->
    DotDot,    // ..
    Plus,      // +
    Minus,     // -
    Star,      // *
    Slash,     // /
    Percent,   // %
    PlusEq,    // +=
    MinusEq,   // -=
    StarEq,    // *=
    SlashEq,   // /=
    PercentEq, // %=
    Not,       // !
    And,       // &&
    Or,        // ||
    Lt,        // <
    Gt,        // >

    // 기타
    EOF,
}

// 앵커 종류
#[derive(Debug, PartialEq, Clone)]
pub enum AnchorKind {
    Main,
    Plain,         // @handler() — no explicit kind
    Thread,        // @worker(thread)
    Event(String), // @handler(event(error))
}

// 위치 정보
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

// 타입
#[derive(Debug, PartialEq, Clone)]
pub enum Ty {
    Int,
    Float,
    String,
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Array(Box<Ty>), // int[], u8[], etc.
    Struct(String),
    Auto,                         // auto (A07: 타입 추론, analyzer가 해결)
    Enum(String),                 // enum 타입
    TypeParam(String),            // 제네릭 타입 파라미터 (T, U 등)
    Fn(Vec<Ty>, Option<Box<Ty>>), // fn(int, int) -> bool  (클로저/함수 타입)
}

// 표현식
#[derive(Debug, PartialEq, Clone)]
pub enum Expr {
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    Bool(bool),
    Ident(String),
    UnaryOp {
        op: UnaryOpKind,
        expr: Box<Expr>,
    },
    BinOp {
        left: Box<Expr>,
        op: BinOpKind,
        right: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    // [1, 2, 3]
    ArrayLit(Vec<Expr>),
    // arr[index]
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
    },
    // expr as ty
    Cast {
        expr: Box<Expr>,
        ty: Ty,
    },
    StructInit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    FieldAccess {
        base: Box<Expr>,
        field: String,
    },
    MethodCall {
        base: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    // 열거형 변형 생성: Color.Red 또는 Option.Some(42)
    EnumVariant {
        enum_name: String,
        variant: String,
        value: Option<Box<Expr>>,
    },
    // 클로저: |x, y| { body } 또는 |x, y| expr
    Closure {
        params: Vec<(String, Option<Ty>)>, // (name, opt_type)
        body: Vec<(Stmt, Span)>,
    },
    // 문자열 보간: f"hello {name}, age={age}"
    FStringLit(Vec<FStringPart>),
}

#[derive(Debug, PartialEq, Clone)]
pub enum UnaryOpKind {
    Neg, // -
    Not, // !
}

#[derive(Debug, PartialEq, Clone)]
pub enum FStringPart {
    Literal(String),
    Expr(Expr),
}

#[derive(Debug, PartialEq, Clone)]
pub enum BinOpKind {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Lt,
    Gt,
    Eq,
    Neq,
    Le,
    Ge,
    And,
    Or,
}

// 구문(Statement)
#[derive(Debug, PartialEq, Clone)]
pub enum Stmt {
    // int x = 10;
    VarDecl {
        ty: Ty,
        name: String,
        value: Expr,
    },
    // const int X = 10;
    ConstDecl {
        ty: Ty,
        name: String,
        value: Expr,
    },
    // Vault int x = 10;
    VaultDecl {
        ty: Ty,
        name: String,
        value: Expr,
    },
    // x = 10;
    Assign {
        name: String,
        value: Expr,
    },
    // arr[i] = 10;
    IndexAssign {
        name: String,
        index: Expr,
        value: Expr,
    },
    // user.name = "a";
    FieldAssign {
        name: String,
        field: String,
        value: Expr,
    },
    // x += 10;  x -= 5;  etc.
    CompoundAssign {
        name: String,
        op: BinOpKind,
        value: Expr,
    },
    // Kill "메시지"; 또는 Kill expr;
    Kill(Option<Expr>),
    // Exit;
    Exit,
    // yield expr;
    Yield(Expr),
    // return expr;
    Return(Option<Expr>),
    // if expr { ... } else { ... }
    If {
        cond: Expr,
        then_body: Vec<(Stmt, Span)>,
        else_body: Option<Vec<(Stmt, Span)>>,
    },
    // loop { ... }
    Loop(Vec<(Stmt, Span)>),
    // while cond { ... }
    While {
        cond: Expr,
        body: Vec<(Stmt, Span)>,
    },
    // for i in 0..10 { ... }
    For {
        var: String,
        from: Expr,
        to: Expr,
        body: Vec<(Stmt, Span)>,
    },
    // break;
    Break,
    // free(x);
    Free(String),
    // print(expr, ...);
    Print(Vec<Expr>),
    // assert(cond);  assert(cond, "message");
    Assert {
        cond: Expr,
        message: Option<Expr>,
    },
    // match 문
    Match {
        expr: Expr,
        arms: Vec<MatchArm>,
    },
    // 블록 내부 인라인 앵커
    InlineAnchor {
        name: String,
        kind: AnchorKind,
        body: Vec<(Stmt, Span)>,
    },
    // 표현식 구문 (함수 호출 등)
    ExprStmt(Expr),
}

// 함수 파라미터
#[derive(Debug, PartialEq, Clone)]
pub struct Param {
    pub ty: Ty,
    pub name: String,
}

#[derive(Debug, PartialEq, Clone)]
pub struct StructField {
    pub ty: Ty,
    pub name: String,
}

#[derive(Debug, PartialEq, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub ty: Option<Ty>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub return_ty: Option<Ty>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Vec<(Stmt, Span)>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Pattern {
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    Bool(bool),
    EnumVariant {
        enum_name: String,
        variant: String,
        binding: Option<String>,
    },
    Wildcard,
}

// 최상위 선언
#[derive(Debug, PartialEq, Clone)]
pub enum TopLevel {
    // 앵커
    Anchor {
        name: String,
        kind: AnchorKind,
        body: Vec<(Stmt, Span)>,
        children: Vec<(TopLevel, Span)>,
    },
    // function add(int a, int b) -> int { ... }
    Function {
        name: String,
        type_params: Vec<String>, // 제네릭: <T, U>
        params: Vec<Param>,
        return_ty: Option<Ty>,
        body: Vec<(Stmt, Span)>,
        decorators: Vec<String>, // A10: #[test] 등
    },
    Struct {
        name: String,
        fields: Vec<StructField>,
    },
    Enum {
        name: String,
        variants: Vec<EnumVariant>,
    },
    // trait Printable { fn to_string(self) -> string; }
    Trait {
        name: String,
        methods: Vec<TraitMethod>,
    },
    // impl Printable for User { fn to_string(...) { ... } }
    Impl {
        trait_name: String,
        target_ty: String,
        methods: Vec<(TopLevel, Span)>,
    },
    // mod math { fn abs(...) {...} }
    Module {
        name: String,
        items: Vec<(TopLevel, Span)>,
    },
    // const int MAX = 100;  (최상위 상수)
    ConstDecl {
        ty: Ty,
        name: String,
        value: Expr,
    },
}

// 프로그램 전체
#[derive(Debug, PartialEq, Clone)]
pub struct Program {
    pub items: Vec<(TopLevel, Span)>,
}
