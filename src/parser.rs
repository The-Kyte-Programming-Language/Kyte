use crate::ast::*;

const MAX_DEPTH: usize = 256;

pub struct Parser {
    tokens: Vec<Token>,
    lines:  Vec<usize>,
    cols:   Vec<usize>,
    pos:    usize,
    pub errors: Vec<String>,
    depth:  usize,
}

impl Parser {
    pub fn new(spanned_tokens: Vec<(Token, usize, usize)>) -> Self {
        let mut tokens = Vec::with_capacity(spanned_tokens.len());
        let mut lines = Vec::with_capacity(spanned_tokens.len());
        let mut cols = Vec::with_capacity(spanned_tokens.len());
        for (tok, line, col) in spanned_tokens {
            tokens.push(tok);
            lines.push(line);
            cols.push(col);
        }
        Parser { tokens, lines, cols, pos: 0, errors: Vec::new(), depth: 0 }
    }

    fn is_keyword(name: &str) -> bool {
        matches!(
            name,
            "main" | "fn" | "Vault" | "Kill" | "Exit" | "yield" | "return"
            | "if" | "else" | "loop" | "while" | "for" | "in" | "break"
            | "true" | "false" | "free" | "print" | "as" | "struct"
            | "int" | "float" | "string" | "bool" | "auto" | "assert"
            | "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64"
        )
    }

    /// 일반 식별자 컨텍스트에서 키워드는 변수명으로 사용 불가 (M02)
    fn eat_var_ident(&mut self) -> String {
        let name = self.eat_ident();
        if Self::is_keyword(&name) {
            self.errors.push(format!(
                "Reserved keyword '{}' cannot be used as identifier at line {}:{}",
                name, self.current_line(), self.current_col()
            ));
        }
        name
    }

    fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::EOF)
    }

    fn current_col(&self) -> usize {
        self.cols.get(self.pos).copied().unwrap_or(0)
    }

    fn current_line(&self) -> usize {
        self.lines.get(self.pos).copied().unwrap_or(0)
    }

    fn current_span(&self) -> Span {
        Span { line: self.current_line(), col: self.current_col() }
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn peek(&self, offset: usize) -> &Token {
        self.tokens.get(self.pos + offset).unwrap_or(&Token::EOF)
    }

    fn skip_decorator(&mut self) {
        self.parse_decorator();
    }

    /// #[name] 또는 #name 또는 #name(...) 형태의 decorator를 파싱하여 이름을 반환합니다.
    fn parse_decorator(&mut self) -> String {
        self.expect(&Token::Hash);

        // #[name] 형태 지원
        let has_bracket = self.current() == &Token::LBracket;
        if has_bracket {
            self.advance();
        }

        let name = self.eat_ident();

        if self.current() == &Token::LParen {
            self.advance();
            let mut depth = 1usize;
            while depth > 0 {
                match self.current() {
                    Token::LParen => {
                        depth += 1;
                        self.advance();
                    }
                    Token::RParen => {
                        depth -= 1;
                        self.advance();
                    }
                    Token::EOF => {
                        self.errors.push(format!(
                            "Unclosed decorator arguments at line {}:{}",
                            self.current_line(),
                            self.current_col()
                        ));
                        break;
                    },
                    _ => {
                        self.advance();
                    }
                }
            }
        }

        if has_bracket {
            if self.current() == &Token::RBracket {
                self.advance();
            }
        }

        name
    }

    fn expect(&mut self, expected: &Token) {
        if self.current() == expected {
            self.advance();
        } else {
            self.errors.push(format!(
                "Expected {:?} but got {:?} at line {}:{}",
                expected,
                self.current(),
                self.current_line(),
                self.current_col()
            ));
            // 에러 복구: 동기화 토큰까지 스킵 (세미콜론, 중괄호 등)
            if !matches!(self.current(), Token::Semicolon | Token::RBrace | Token::RParen | Token::RBracket | Token::EOF) {
                self.advance();
            }
        }
    }

    fn eat_ident(&mut self) -> String {
        match self.current().clone() {
            Token::Ident(s) => { self.advance(); s }
            // 키워드도 앵커 이름/종류로 쓸 수 있게 허용
            Token::Main     => { self.advance(); "main".to_string() }
            Token::Function => { self.advance(); "function".to_string() }
            Token::Vault    => { self.advance(); "vault".to_string() }
            Token::Kill     => { self.advance(); "kill".to_string() }
            Token::Exit     => { self.advance(); "exit".to_string() }
            Token::Yield    => { self.advance(); "yield".to_string() }
            Token::Return   => { self.advance(); "return".to_string() }
            Token::If       => { self.advance(); "if".to_string() }
            Token::Else     => { self.advance(); "else".to_string() }
            Token::Loop     => { self.advance(); "loop".to_string() }
            Token::For      => { self.advance(); "for".to_string() }
            Token::In       => { self.advance(); "in".to_string() }
            Token::Break    => { self.advance(); "break".to_string() }
            Token::True     => { self.advance(); "true".to_string() }
            Token::False    => { self.advance(); "false".to_string() }
            Token::Free    => { self.advance(); "free".to_string() }
            Token::Print   => { self.advance(); "print".to_string() }
            Token::While   => { self.advance(); "while".to_string() }
            Token::As      => { self.advance(); "as".to_string() }
            Token::Struct  => { self.advance(); "struct".to_string() }
            Token::Int      => { self.advance(); "int".to_string() }
            Token::Float    => { self.advance(); "float".to_string() }
            Token::String   => { self.advance(); "string".to_string() }
            Token::Bool     => { self.advance(); "bool".to_string() }
            Token::TyI8     => { self.advance(); "i8".to_string() }
            Token::TyI16    => { self.advance(); "i16".to_string() }
            Token::TyI32    => { self.advance(); "i32".to_string() }
            Token::TyI64    => { self.advance(); "i64".to_string() }
            Token::TyU8     => { self.advance(); "u8".to_string() }
            Token::TyU16    => { self.advance(); "u16".to_string() }
            Token::TyU32    => { self.advance(); "u32".to_string() }
            Token::TyU64    => { self.advance(); "u64".to_string() }
            t => {
                self.errors.push(format!(
                    "Expected identifier but got {:?} at line {}:{}",
                    t,
                    self.current_line(),
                    self.current_col()
                ));
                "_error_".to_string()
            },
        }
    }

    // 타입 파싱
    fn parse_ty(&mut self) -> Ty {
        let base = match self.current().clone() {
            Token::Int    => { self.advance(); Ty::Int }
            Token::Float  => { self.advance(); Ty::Float }
            Token::String => { self.advance(); Ty::String }
            Token::Bool   => { self.advance(); Ty::Bool }
            Token::TyI8   => { self.advance(); Ty::I8 }
            Token::TyI16  => { self.advance(); Ty::I16 }
            Token::TyI32  => { self.advance(); Ty::I32 }
            Token::TyI64  => { self.advance(); Ty::I64 }
            Token::TyU8   => { self.advance(); Ty::U8 }
            Token::TyU16  => { self.advance(); Ty::U16 }
            Token::TyU32  => { self.advance(); Ty::U32 }
            Token::TyU64  => { self.advance(); Ty::U64 }
            Token::Ident(name) => { self.advance(); Ty::Struct(name) }
            t => {
                self.errors.push(format!(
                    "Expected type but got {:?} at line {}:{}",
                    t,
                    self.current_line(),
                    self.current_col()
                ));
                Ty::Int // fallback
            },
        };
        // int[] 같은 배열 타입
        if self.current() == &Token::LBracket {
            self.advance();
            self.expect(&Token::RBracket);
            Ty::Array(Box::new(base))
        } else {
            base
        }
    }

    // 표현식 파싱 (우선순위: or < and < 비교 < 덧뺄셈 < 곱나눗셈 < 단항 < primary)
    fn parse_expr(&mut self) -> Expr {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.errors.push(format!(
                "Expression nesting too deep (>{}) at line {}:{}",
                MAX_DEPTH, self.current_line(), self.current_col()
            ));
            self.depth -= 1;
            return Expr::IntLit(0);
        }
        let result = self.parse_or();
        self.depth -= 1;
        result
    }

    fn parse_or(&mut self) -> Expr {
        let mut left = self.parse_and();
        loop {
            if self.current() != &Token::Or { break; }
            self.advance();
            let right = self.parse_and();
            left = Expr::BinOp { left: Box::new(left), op: BinOpKind::Or, right: Box::new(right) };
        }
        left
    }

    fn parse_and(&mut self) -> Expr {
        let mut left = self.parse_comparison();
        loop {
            if self.current() != &Token::And { break; }
            self.advance();
            let right = self.parse_comparison();
            left = Expr::BinOp { left: Box::new(left), op: BinOpKind::And, right: Box::new(right) };
        }
        left
    }

    fn parse_comparison(&mut self) -> Expr {
        let mut left = self.parse_additive();
        loop {
            let op = match self.current() {
                Token::Lt   => BinOpKind::Lt,
                Token::Gt   => BinOpKind::Gt,
                Token::Le   => BinOpKind::Le,
                Token::Ge   => BinOpKind::Ge,
                Token::EqEq => BinOpKind::Eq,
                Token::Neq  => BinOpKind::Neq,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive();
            left = Expr::BinOp { left: Box::new(left), op, right: Box::new(right) };
        }
        left
    }

    fn parse_additive(&mut self) -> Expr {
        let mut left = self.parse_multiplicative();
        loop {
            let op = match self.current() {
                Token::Plus  => BinOpKind::Add,
                Token::Minus => BinOpKind::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative();
            left = Expr::BinOp { left: Box::new(left), op, right: Box::new(right) };
        }
        left
    }

    fn parse_multiplicative(&mut self) -> Expr {
        let mut left = self.parse_unary();
        loop {
            let op = match self.current() {
                Token::Star    => BinOpKind::Mul,
                Token::Slash   => BinOpKind::Div,
                Token::Percent => BinOpKind::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary();
            left = Expr::BinOp { left: Box::new(left), op, right: Box::new(right) };
        }
        left
    }

    fn parse_unary(&mut self) -> Expr {
        match self.current() {
            Token::Minus => {
                self.advance();
                let expr = self.parse_unary();
                Expr::UnaryOp { op: UnaryOpKind::Neg, expr: Box::new(expr) }
            }
            Token::Not => {
                self.advance();
                let expr = self.parse_unary();
                Expr::UnaryOp { op: UnaryOpKind::Not, expr: Box::new(expr) }
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Expr {
        let mut expr = match self.current().clone() {
            Token::IntLit(n)    => { self.advance(); Expr::IntLit(n) }
            Token::FloatLit(f)  => { self.advance(); Expr::FloatLit(f) }
            Token::StringLit(s) => { self.advance(); Expr::StringLit(s) }
            Token::True         => { self.advance(); Expr::Bool(true) }
            Token::False        => { self.advance(); Expr::Bool(false) }
            Token::Ident(s)     => {
                self.advance();
                if self.current() == &Token::LBrace {
                    self.advance();
                    let mut fields = Vec::new();
                    while self.current() != &Token::RBrace {
                        let fname = self.eat_ident();
                        self.expect(&Token::Colon);
                        let fexpr = self.parse_expr();
                        fields.push((fname, fexpr));
                        if self.current() == &Token::Comma {
                            self.advance();
                        }
                    }
                    self.expect(&Token::RBrace);
                    Expr::StructInit { name: s, fields }
                }
                // 함수 호출인지 확인
                else if self.current() == &Token::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    while self.current() != &Token::RParen {
                        args.push(self.parse_expr());
                        if self.current() == &Token::Comma {
                            self.advance();
                        }
                    }
                    self.expect(&Token::RParen);
                    Expr::Call { name: s, args }
                } else {
                    Expr::Ident(s)
                }
            }
            // 배열 리터럴: [1, 2, 3]
            Token::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                while self.current() != &Token::RBracket {
                    elems.push(self.parse_expr());
                    if self.current() == &Token::Comma { self.advance(); }
                }
                self.expect(&Token::RBracket);
                Expr::ArrayLit(elems)
            }
            Token::LParen => {
                self.advance();
                let e = self.parse_expr();
                self.expect(&Token::RParen);
                e
            }
            t => {
                self.errors.push(format!(
                    "Unexpected token in expression: {:?} at line {}:{}",
                    t,
                    self.current_line(),
                    self.current_col()
                ));
                self.advance();
                Expr::IntLit(0) // fallback
            },
        };
        expr = self.parse_postfix_expr(expr);
        expr
    }

    // 후위: 인덱스 접근 expr[index], 타입 캐스팅 expr as ty, 멤버 접근/메서드 호출
    fn parse_postfix_expr(&mut self, mut expr: Expr) -> Expr {
        loop {
            match self.current() {
                Token::LBracket => {
                    self.advance();
                    let index = self.parse_expr();
                    self.expect(&Token::RBracket);
                    expr = Expr::Index { array: Box::new(expr), index: Box::new(index) };
                }
                Token::As => {
                    self.advance();
                    let ty = self.parse_ty();
                    expr = Expr::Cast { expr: Box::new(expr), ty };
                }
                Token::Dot => {
                    self.advance();
                    let member = self.eat_ident();
                    if self.current() == &Token::LParen {
                        self.advance();
                        let mut args = Vec::new();
                        while self.current() != &Token::RParen {
                            args.push(self.parse_expr());
                            if self.current() == &Token::Comma { self.advance(); }
                        }
                        self.expect(&Token::RParen);
                        expr = Expr::MethodCall { base: Box::new(expr), method: member, args };
                    } else {
                        expr = Expr::FieldAccess { base: Box::new(expr), field: member };
                    }
                }
                _ => break,
            }
        }
        expr
    }

    fn parse_ident_stmt(&mut self, name: String) -> Stmt {
        // 구조체 타입 변수 선언: User u = User { ... };
        if let Token::Ident(var_name) = self.current().clone() {
            let ty = Ty::Struct(name);
            self.advance();
            self.expect(&Token::Eq);
            let value = self.parse_expr();
            self.expect(&Token::Semicolon);
            return Stmt::VarDecl { ty, name: var_name, value };
        }

        // 구조체 배열 타입 변수 선언: User[] users = [...];
        if self.current() == &Token::LBracket
            && self.peek(1) == &Token::RBracket
            && matches!(self.peek(2), Token::Ident(_))
        {
            self.expect(&Token::LBracket);
            self.expect(&Token::RBracket);
            let var_name = self.eat_ident();
            self.expect(&Token::Eq);
            let value = self.parse_expr();
            self.expect(&Token::Semicolon);
            return Stmt::VarDecl {
                ty: Ty::Array(Box::new(Ty::Struct(name))),
                name: var_name,
                value,
            };
        }

        let base = if self.current() == &Token::LParen {
            // 함수 호출: name(args...)
            self.advance();
            let mut args = Vec::new();
            while self.current() != &Token::RParen {
                args.push(self.parse_expr());
                if self.current() == &Token::Comma { self.advance(); }
            }
            self.expect(&Token::RParen);
            Expr::Call { name, args }
        } else {
            Expr::Ident(name)
        };
        let expr = self.parse_postfix_expr(base);

        // 복합 대입 연산자 (+=, -=, *=, /=, %=)
        let compound_op = match self.current() {
            Token::PlusEq => Some(BinOpKind::Add),
            Token::MinusEq => Some(BinOpKind::Sub),
            Token::StarEq => Some(BinOpKind::Mul),
            Token::SlashEq => Some(BinOpKind::Div),
            Token::PercentEq => Some(BinOpKind::Mod),
            _ => None,
        };
        if let Some(op) = compound_op {
            self.advance();
            let value = self.parse_expr();
            self.expect(&Token::Semicolon);
            if let Expr::Ident(var_name) = expr {
                return Stmt::CompoundAssign {
                    name: var_name,
                    op,
                    value,
                };
            }
            self.errors.push(format!(
                "Invalid compound assignment target at line {}:{}",
                self.current_line(),
                self.current_col()
            ));
            return Stmt::ExprStmt(Expr::IntLit(0));
        }

        if self.current() == &Token::Eq {
            self.advance();
            let value = self.parse_expr();
            self.expect(&Token::Semicolon);
            return match expr {
                Expr::Ident(var_name) => Stmt::Assign {
                    name: var_name,
                    value,
                },
                Expr::Index { array, index } => match *array {
                    Expr::Ident(var_name) => Stmt::IndexAssign {
                        name: var_name,
                        index: *index,
                        value,
                    },
                    _ => {
                        self.errors.push(format!(
                            "Invalid index assignment target at line {}:{}",
                            self.current_line(),
                            self.current_col()
                        ));
                        Stmt::ExprStmt(Expr::IntLit(0))
                    },
                },
                Expr::FieldAccess { base, field } => match *base {
                    Expr::Ident(var_name) => Stmt::FieldAssign {
                        name: var_name,
                        field,
                        value,
                    },
                    _ => {
                        self.errors.push(format!(
                            "Invalid field assignment target at line {}:{}",
                            self.current_line(),
                            self.current_col()
                        ));
                        Stmt::ExprStmt(Expr::IntLit(0))
                    },
                },
                _ => {
                    self.errors.push(format!(
                        "Invalid assignment target at line {}:{}",
                        self.current_line(),
                        self.current_col()
                    ));
                    Stmt::ExprStmt(Expr::IntLit(0))
                },
            };
        }

        self.expect(&Token::Semicolon);
        Stmt::ExprStmt(expr)
    }

    // 구문 파싱
    fn parse_stmt(&mut self) -> (Stmt, Span) {
        let span = self.current_span();
        let stmt = match self.current().clone() {
            // Kill — 표현식(문자열 연결 등) 허용
            Token::Kill => {
                self.advance();
                if self.current() == &Token::Semicolon {
                    self.advance();
                    Stmt::Kill(None)
                } else {
                    let e = self.parse_expr();
                    self.expect(&Token::Semicolon);
                    Stmt::Kill(Some(e))
                }
            }
            // Exit
            Token::Exit => {
                self.advance();
                self.expect(&Token::Semicolon);
                Stmt::Exit
            }
            // yield
            Token::Yield => {
                self.advance();
                let e = self.parse_expr();
                self.expect(&Token::Semicolon);
                Stmt::Yield(e)
            }
            // return
            Token::Return => {
                self.advance();
                if self.current() == &Token::Semicolon {
                    self.advance();
                    Stmt::Return(None)
                } else {
                    let e = self.parse_expr();
                    self.expect(&Token::Semicolon);
                    Stmt::Return(Some(e))
                }
            }
            // break
            Token::Break => {
                self.advance();
                self.expect(&Token::Semicolon);
                Stmt::Break
            }
            // free
            Token::Free => {
                self.advance();
                self.expect(&Token::LParen);
                let name = self.eat_ident();
                self.expect(&Token::RParen);
                self.expect(&Token::Semicolon);
                Stmt::Free(name)
            }
            // print(expr, ...)
            Token::Print => {
                self.advance();
                self.expect(&Token::LParen);
                let mut args = Vec::new();
                while self.current() != &Token::RParen {
                    args.push(self.parse_expr());
                    if self.current() == &Token::Comma { self.advance(); }
                }
                self.expect(&Token::RParen);
                self.expect(&Token::Semicolon);
                Stmt::Print(args)
            }
            // assert(cond) or assert(cond, "msg")
            Token::Assert => {
                self.advance();
                self.expect(&Token::LParen);
                let cond = self.parse_expr();
                let message = if self.current() == &Token::Comma {
                    self.advance();
                    Some(self.parse_expr())
                } else {
                    None
                };
                self.expect(&Token::RParen);
                self.expect(&Token::Semicolon);
                Stmt::Assert { cond, message }
            }
            // if
            Token::If => {
                self.advance();
                let cond = self.parse_expr();
                self.expect(&Token::LBrace);
                let then_body = self.parse_body();
                self.expect(&Token::RBrace);
                let else_body = if self.current() == &Token::Else {
                    self.advance();
                    if self.current() == &Token::If {
                        // else if → 중첩 if로 변환
                        Some(vec![self.parse_stmt()])
                    } else {
                        self.expect(&Token::LBrace);
                        let b = self.parse_body();
                        self.expect(&Token::RBrace);
                        Some(b)
                    }
                } else {
                    None
                };
                Stmt::If { cond, then_body, else_body }
            }
            // loop
            Token::Loop => {
                self.advance();
                self.expect(&Token::LBrace);
                let body = self.parse_body();
                self.expect(&Token::RBrace);
                Stmt::Loop(body)
            }
            // while
            Token::While => {
                self.advance();
                let cond = self.parse_expr();
                self.expect(&Token::LBrace);
                let body = self.parse_body();
                self.expect(&Token::RBrace);
                Stmt::While { cond, body }
            }
            // for
            Token::For => {
                self.advance();
                let var = self.eat_ident();
                self.expect(&Token::In);
                let from = self.parse_primary();
                self.expect(&Token::DotDot);
                let to = self.parse_primary();
                self.expect(&Token::LBrace);
                let body = self.parse_body();
                self.expect(&Token::RBrace);
                Stmt::For { var, from, to, body }
            }
            // Vault 선언
            Token::Vault => {
                self.advance();
                let ty = self.parse_ty();
                let name = self.eat_ident();
                self.expect(&Token::Eq);
                let value = self.parse_expr();
                self.expect(&Token::Semicolon);
                Stmt::VaultDecl { ty, name, value }
            }
            // 변수 선언 or 대입 or 표현식
            Token::Int | Token::Float | Token::String | Token::Bool
            | Token::TyI8 | Token::TyI16 | Token::TyI32 | Token::TyI64
            | Token::TyU8 | Token::TyU16 | Token::TyU32 | Token::TyU64 => {
                let ty = self.parse_ty();
                let name = self.eat_var_ident();
                self.expect(&Token::Eq);
                let value = self.parse_expr();
                self.expect(&Token::Semicolon);
                Stmt::VarDecl { ty, name, value }
            }
            // A07: auto 타입 추론
            Token::Auto => {
                self.advance();
                let name = self.eat_var_ident();
                self.expect(&Token::Eq);
                let value = self.parse_expr();
                self.expect(&Token::Semicolon);
                Stmt::VarDecl { ty: Ty::Auto, name, value }
            }
            Token::Ident(name) => {
                self.advance();
                self.parse_ident_stmt(name)
            }
            t => {
                self.errors.push(format!(
                    "Unexpected token in statement: {:?} at line {}:{}",
                    t,
                    self.current_line(),
                    self.current_col()
                ));
                // 에러 복구: 세미콜론 또는 닫는 중괄호까지 스킵
                while !matches!(self.current(), Token::Semicolon | Token::RBrace | Token::EOF) {
                    self.advance();
                }
                if self.current() == &Token::Semicolon { self.advance(); }
                Stmt::ExprStmt(Expr::IntLit(0))
            },
        };
        (stmt, span)
    }

    fn parse_body(&mut self) -> Vec<(Stmt, Span)> {
        let mut stmts = Vec::new();
        loop {
            match self.current() {
                Token::RBrace | Token::EOF => break,
                Token::Hash => self.skip_decorator(),
                Token::At => stmts.push(self.parse_inline_anchor()),
                _ => stmts.push(self.parse_stmt()),
            }
        }
        stmts
    }

    // 앵커 종류 파싱 (공통 헬퍼)
    fn parse_anchor_kind(&mut self) -> AnchorKind {
        // 빈 앵커: @name()
        if self.current() == &Token::RParen {
            return AnchorKind::Plain;
        }

        let kind_ident = self.eat_ident();
        match kind_ident.as_str() {
            "main"   => AnchorKind::Main,
            "thread" => AnchorKind::Thread,
            "event"  => {
                self.expect(&Token::LParen);
                let event_name = self.eat_ident();
                self.expect(&Token::RParen);
                AnchorKind::Event(event_name)
            }
            k => {
                self.errors.push(format!(
                    "Unknown anchor kind: {} at line {}:{}",
                    k,
                    self.current_line(),
                    self.current_col()
                ));
                AnchorKind::Plain
            },
        }
    }

    // 블록 내부 인라인 앵커 파싱 (중괄호 필수)
    fn parse_inline_anchor(&mut self) -> (Stmt, Span) {
        let span = self.current_span();
        self.expect(&Token::At);
        let name = self.eat_ident();
        self.expect(&Token::LParen);
        let kind = self.parse_anchor_kind();
        self.expect(&Token::RParen);

        self.expect(&Token::LBrace);
        let body = self.parse_body();
        self.expect(&Token::RBrace);

        (Stmt::InlineAnchor { name, kind, body }, span)
    }

    // 최상위 앵커 파싱 @이름(형태) — 중괄호 필수
    fn parse_anchor(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::At);
        let name = self.eat_ident();
        self.expect(&Token::LParen);
        let kind = self.parse_anchor_kind();
        self.expect(&Token::RParen);

        self.expect(&Token::LBrace);
        // 본문 + 자식 앵커
        let mut body     = Vec::new();
        let mut children = Vec::new();

        loop {
            match self.current() {
                Token::RBrace | Token::EOF => break,
                Token::Hash => self.skip_decorator(),
                Token::At   => children.push(self.parse_child_anchor()),
                _ => body.push(self.parse_stmt()),
            }
        }
        self.expect(&Token::RBrace);

        (TopLevel::Anchor { name, kind, body, children }, span)
    }

    // 자식 앵커 파싱 (인라인과 동일하지만 TopLevel 반환)
    fn parse_child_anchor(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::At);
        let name = self.eat_ident();
        self.expect(&Token::LParen);
        let kind = self.parse_anchor_kind();
        self.expect(&Token::RParen);

        self.expect(&Token::LBrace);
        let mut body     = Vec::new();
        let mut children = Vec::new();
        loop {
            match self.current() {
                Token::RBrace | Token::EOF => break,
                Token::Hash => self.skip_decorator(),
                Token::At   => children.push(self.parse_child_anchor()),
                _ => body.push(self.parse_stmt()),
            }
        }
        self.expect(&Token::RBrace);

        (TopLevel::Anchor { name, kind, body, children }, span)
    }

    // 함수 파싱
    fn parse_function(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::Function);
        let first_name = self.eat_ident();
        let mut method_owner: Option<String> = None;
        let name = if self.current() == &Token::Dot {
            self.advance();
            let method = self.eat_ident();
            method_owner = Some(first_name.clone());
            format!("{}.{}", first_name, method)
        } else {
            first_name
        };
        self.expect(&Token::LParen);

        let mut params = Vec::new();
        if let Some(owner) = &method_owner {
            params.push(Param { ty: Ty::Struct(owner.clone()), name: "self".to_string() });
        }
        while self.current() != &Token::RParen {
            let ty   = self.parse_ty();
            let pname = self.eat_var_ident();
            params.push(Param { ty, name: pname });
            if self.current() == &Token::Comma { self.advance(); }
        }
        self.expect(&Token::RParen);

        let return_ty = if self.current() == &Token::Arrow {
            self.advance();
            Some(self.parse_ty())
        } else {
            None
        };

        self.expect(&Token::LBrace);
        let body = self.parse_body();
        self.expect(&Token::RBrace);

        (TopLevel::Function { name, params, return_ty, body, decorators: Vec::new() }, span)
    }

    // struct 선언 파싱
    fn parse_struct(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::Struct);
        let name = self.eat_ident();
        self.expect(&Token::LBrace);

        let mut fields = Vec::new();
        while self.current() != &Token::RBrace {
            let ty = self.parse_ty();
            let fname = self.eat_ident();
            self.expect(&Token::Semicolon);
            fields.push(StructField { ty, name: fname });
        }
        self.expect(&Token::RBrace);

        (TopLevel::Struct { name, fields }, span)
    }

    // 전체 파싱
    pub fn parse(&mut self) -> Program {
        let mut items = Vec::new();
        loop {
            match self.current() {
                Token::EOF      => break,
                Token::Hash     => {
                    // A10: decorator 수집 → 이어서 fn에 부착
                    let dec_name = self.parse_decorator();
                    if self.current() == &Token::Hash {
                        // 연속 decorator — skip (이미 파싱됨)
                        // TODO: 여러 개 decorator 지원
                    }
                    if self.current() == &Token::Function {
                        let (mut tl, sp) = self.parse_function();
                        if let TopLevel::Function { ref mut decorators, .. } = tl {
                            decorators.push(dec_name);
                        }
                        items.push((tl, sp));
                    }
                    // decorator가 fn이 아닌 곳에 붙은 경우 무시
                }
                Token::At       => items.push(self.parse_anchor()),
                Token::Function => items.push(self.parse_function()),
                Token::Struct   => items.push(self.parse_struct()),
                t => {
                    self.errors.push(format!(
                        "Unexpected top-level token: {:?} at line {}:{}",
                        t,
                        self.current_line(),
                        self.current_col()
                    ));
                    self.advance(); // 에러 복구: 스킵
                },
            }
        }
        Program { items }
    }
}