use crate::ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos:    usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::EOF)
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &Token) {
        if self.current() == expected {
            self.advance();
        } else {
            panic!("Expected {:?} but got {:?}", expected, self.current());
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
            Token::Int      => { self.advance(); "int".to_string() }
            Token::Float    => { self.advance(); "float".to_string() }
            Token::String   => { self.advance(); "string".to_string() }
            Token::Bool     => { self.advance(); "bool".to_string() }
            t => panic!("Expected identifier but got {:?}", t),
        }
    }

    // 타입 파싱
    fn parse_ty(&mut self) -> Ty {
        match self.current().clone() {
            Token::Int    => { self.advance(); Ty::Int }
            Token::Float  => { self.advance(); Ty::Float }
            Token::String => { self.advance(); Ty::String }
            Token::Bool   => { self.advance(); Ty::Bool }
            t => panic!("Expected type but got {:?}", t),
        }
    }

    // 표현식 파싱
    fn parse_expr(&mut self) -> Expr {
        let left = self.parse_primary();

        match self.current() {
            Token::Plus  => { self.advance(); Expr::BinOp { left: Box::new(left), op: BinOpKind::Add, right: Box::new(self.parse_expr()) } }
            Token::Minus => { self.advance(); Expr::BinOp { left: Box::new(left), op: BinOpKind::Sub, right: Box::new(self.parse_expr()) } }
            Token::Star  => { self.advance(); Expr::BinOp { left: Box::new(left), op: BinOpKind::Mul, right: Box::new(self.parse_expr()) } }
            Token::Slash => { self.advance(); Expr::BinOp { left: Box::new(left), op: BinOpKind::Div, right: Box::new(self.parse_expr()) } }
            Token::Lt    => { self.advance(); Expr::BinOp { left: Box::new(left), op: BinOpKind::Lt,  right: Box::new(self.parse_expr()) } }
            Token::Gt    => { self.advance(); Expr::BinOp { left: Box::new(left), op: BinOpKind::Gt,  right: Box::new(self.parse_expr()) } }
            _ => left,
        }
    }

    fn parse_primary(&mut self) -> Expr {
        match self.current().clone() {
            Token::IntLit(n)    => { self.advance(); Expr::IntLit(n) }
            Token::FloatLit(f)  => { self.advance(); Expr::FloatLit(f) }
            Token::StringLit(s) => { self.advance(); Expr::StringLit(s) }
            Token::True         => { self.advance(); Expr::Bool(true) }
            Token::False        => { self.advance(); Expr::Bool(false) }
            Token::Ident(s)     => {
                self.advance();
                // 함수 호출인지 확인
                if self.current() == &Token::LParen {
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
            Token::LParen => {
                self.advance();
                let e = self.parse_expr();
                self.expect(&Token::RParen);
                e
            }
            t => panic!("Unexpected token in expression: {:?}", t),
        }
    }

    // 구문 파싱
    fn parse_stmt(&mut self) -> Stmt {
        match self.current().clone() {
            // Kill
            Token::Kill => {
                self.advance();
                let msg = if let Token::StringLit(s) = self.current().clone() {
                    self.advance();
                    Some(s)
                } else {
                    None
                };
                self.expect(&Token::Semicolon);
                Stmt::Kill(msg)
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
            // if
            Token::If => {
                self.advance();
                let cond = self.parse_expr();
                self.expect(&Token::LBrace);
                let then_body = self.parse_body();
                self.expect(&Token::RBrace);
                let else_body = if self.current() == &Token::Else {
                    self.advance();
                    self.expect(&Token::LBrace);
                    let b = self.parse_body();
                    self.expect(&Token::RBrace);
                    Some(b)
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
            Token::Int | Token::Float | Token::String | Token::Bool => {
                let ty = self.parse_ty();
                let name = self.eat_ident();
                self.expect(&Token::Eq);
                let value = self.parse_expr();
                self.expect(&Token::Semicolon);
                Stmt::VarDecl { ty, name, value }
            }
            Token::Ident(name) => {
                self.advance();
                if self.current() == &Token::Eq {
                    self.advance();
                    let value = self.parse_expr();
                    self.expect(&Token::Semicolon);
                    Stmt::Assign { name, value }
                } else if self.current() == &Token::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    while self.current() != &Token::RParen {
                        args.push(self.parse_expr());
                        if self.current() == &Token::Comma { self.advance(); }
                    }
                    self.expect(&Token::RParen);
                    self.expect(&Token::Semicolon);
                    Stmt::ExprStmt(Expr::Call { name, args })
                } else {
                    panic!("Unexpected token after ident: {:?}", self.current())
                }
            }
            t => panic!("Unexpected token in statement: {:?}", t),
        }
    }

    fn parse_body(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        loop {
            match self.current() {
                Token::RBrace | Token::EOF | Token::At => break,
                _ => stmts.push(self.parse_stmt()),
            }
        }
        stmts
    }

    // 앵커 파싱 @이름(형태)
    fn parse_anchor(&mut self) -> TopLevel {
        self.expect(&Token::At);
        let name = self.eat_ident();

        self.expect(&Token::LParen);

        // 앵커 종류 파싱
        let kind_ident = self.eat_ident();
        let kind = match kind_ident.as_str() {
            "main" => AnchorKind::Main,
            "event" => {
                self.expect(&Token::LParen);
                let event_name = if let Token::StringLit(s) = self.current().clone() {
                    self.advance(); s
                } else { panic!("Expected event name string") };
                self.expect(&Token::RParen);
                AnchorKind::Event(event_name)
            }
            "thread"   => AnchorKind::Thread,
            "on_error" => AnchorKind::OnError,
            "timeout"  => {
                self.expect(&Token::LParen);
                let ms = if let Token::IntLit(n) = self.current().clone() {
                    self.advance(); n as u64
                } else { panic!("Expected timeout ms") };
                self.expect(&Token::RParen);
                AnchorKind::Timeout(ms)
            }
            k => panic!("Unknown anchor kind: {}", k),
        };

        // retry 파싱 (선택)
        let mut retry = None;
        if self.current() == &Token::Comma {
            self.advance();
            let opt = self.eat_ident();
            if opt == "retry" {
                self.expect(&Token::LParen);
                if let Token::IntLit(n) = self.current().clone() {
                    self.advance();
                    retry = Some(n as u32);
                }
                self.expect(&Token::RParen);
            }
        }

        self.expect(&Token::RParen);

        // 본문 + 자식 앵커
        let mut body     = Vec::new();
        let mut children = Vec::new();

        loop {
            match self.current() {
                Token::EOF => break,
                Token::At  => {
                    // 다음 앵커가 자식인지 형제인지는 analyzer에서 판단
                    // 여기선 일단 자식으로 파싱
                    children.push(self.parse_anchor());
                }
                Token::Function => break, // 함수 선언은 최상위로
                _ => body.push(self.parse_stmt()),
            }
        }

        TopLevel::Anchor { name, kind, retry, body, children }
    }

    // 함수 파싱
    fn parse_function(&mut self) -> TopLevel {
        self.expect(&Token::Function);
        let name = self.eat_ident();
        self.expect(&Token::LParen);

        let mut params = Vec::new();
        while self.current() != &Token::RParen {
            let ty   = self.parse_ty();
            let pname = self.eat_ident();
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

        TopLevel::Function { name, params, return_ty, body }
    }

    // 전체 파싱
    pub fn parse(&mut self) -> Program {
        let mut items = Vec::new();
        loop {
            match self.current() {
                Token::EOF      => break,
                Token::At       => items.push(self.parse_anchor()),
                Token::Function => items.push(self.parse_function()),
                t => panic!("Unexpected top-level token: {:?}", t),
            }
        }
        Program { items }
    }
}