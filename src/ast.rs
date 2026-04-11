#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    // 앵커
    At,           // @
    Hash,         // #

    // 키워드
    Main,         // main
    Function,     // function
    Vault,        // Vault
    Kill,         // Kill
    Exit,         // Exit
    Yield,        // yield
    Return,       // return
    If,           // if
    Else,         // else
    Loop,         // loop
    For,          // for
    In,           // in
    Break,        // break
    True,         // true
    False,        // false
    Alloc,        // alloc
    Free,         // free

    // 타입
    Int,          // int
    Float,        // float
    String,       // string
    Bool,         // bool

    // 리터럴
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),

    // 식별자
    Ident(String),

    // 심볼
    LParen,       // (
    RParen,       // )
    LBrace,       // {
    RBrace,       // }
    Semicolon,    // ;
    Comma,        // ,
    Eq,           // =
    Arrow,        // ->
    DotDot,       // ..
    Plus,         // +
    Minus,        // -
    Star,         // *
    Slash,        // /
    Lt,           // 
    Gt,           // >
    
    // 기타 
    EOF,
}

// 앵커 종류
#[derive(Debug, PartialEq, Clone)]
pub enum AnchorKind {
    Main,
    Event(String),   // @handler(event("click"))
    Thread,          // @worker(thread)
    OnError,         // @recovery(on_error)
    Timeout(u64),    // @limiter(timeout(5000))
}

// 타입
#[derive(Debug, PartialEq, Clone)]
pub enum Ty {
    Int,
    Float,
    String,
    Bool,
}

// 표현식
#[derive(Debug, PartialEq, Clone)]
pub enum Expr {
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    Bool(bool),
    Ident(String),
    BinOp {
        left:  Box<Expr>,
        op:    BinOpKind,
        right: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, PartialEq, Clone)]
pub enum BinOpKind {
    Add, Sub, Mul, Div,
    Lt, Gt, Eq,
}

// 구문(Statement)
#[derive(Debug, PartialEq, Clone)]
pub enum Stmt {
    // int x = 10;
    VarDecl {
        ty:    Ty,
        name:  String,
        value: Expr,
    },
    // Vault int x = 10;
    VaultDecl {
        ty:    Ty,
        name:  String,
        value: Expr,
    },
    // x = 10;
    Assign {
        name:  String,
        value: Expr,
    },
    // Kill "메시지";
    Kill(Option<String>),
    // Exit;
    Exit,
    // yield expr;
    Yield(Expr),
    // return expr;
    Return(Option<Expr>),
    // if expr { ... } else { ... }
    If {
        cond:      Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
    },
    // loop { ... }
    Loop(Vec<Stmt>),
    // for i in 0..10 { ... }
    For {
        var:  String,
        from: Expr,
        to:   Expr,
        body: Vec<Stmt>,
    },
    // break;
    Break,
    // free(x);
    Free(String),
    // 표현식 구문 (함수 호출 등)
    ExprStmt(Expr),
}

// 함수 파라미터
#[derive(Debug, PartialEq, Clone)]
pub struct Param {
    pub ty:   Ty,
    pub name: String,
}

// 최상위 선언
#[derive(Debug, PartialEq, Clone)]
pub enum TopLevel {
    // 앵커
    Anchor {
        name:      String,
        kind:      AnchorKind,
        retry:     Option<u32>,
        body:      Vec<Stmt>,
        children:  Vec<TopLevel>,
    },
    // function add(int a, int b) -> int { ... }
    Function {
        name:       String,
        params:     Vec<Param>,
        return_ty:  Option<Ty>,
        body:       Vec<Stmt>,
    },
}

// 프로그램 전체
#[derive(Debug, PartialEq, Clone)]
pub struct Program {
    pub items: Vec<TopLevel>,
}